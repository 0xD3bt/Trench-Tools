use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::Value;
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{AddressLookupTableAccount, VersionedMessage, v0},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::{Transaction, VersionedTransaction},
};
use solana_system_interface::{instruction::transfer, program as system_program};
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;

use crate::{
    config::{NormalizedConfig, NormalizedRecipient},
    report::{LaunchReport, build_report, render_report},
    rpc::{CompiledTransaction, fetch_account_data, fetch_account_exists, fetch_latest_blockhash},
};

const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const MAYHEM_PROGRAM_ID: &str = "MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e";
const PUMP_FEE_PROGRAM_ID: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
const PUMP_AGENT_PAYMENTS_PROGRAM_ID: &str = "AgenTMiC2hvxGebTsgmsD4HHBa8WEcqGFf87iwRRxLo7";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const FIXED_COMPUTE_UNIT_LIMIT: u32 = 1_000_000;
const TOKEN_DECIMALS: u32 = 6;
const GLOBAL_ACCOUNT_DISCRIMINATOR_BYTES: usize = 8;
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const PLATFORM_GITHUB: u8 = 2;
const DEFAULT_LOOKUP_TABLES: [&str; 1] = ["AXVvmhWaaPtV52jqYuTNqp1xRrkbxhfJfeHQKxq5cbvZ"];

#[derive(Debug)]
pub struct NativePumpArtifacts {
    pub compiled_transactions: Vec<CompiledTransaction>,
    pub report: Value,
    pub text: String,
}

pub fn supports_native_pump_compile(config: &NormalizedConfig) -> bool {
    if config.launchpad != "pump" {
        return false;
    }
    if config.mode != "regular"
        && config.mode != "cashback"
        && config.mode != "agent-unlocked"
        && config.mode != "agent-custom"
        && config.mode != "agent-locked"
    {
        return false;
    }
    if config.feeSharing.generateLaterSetup
        && config.feeSharing.recipients.iter().any(|entry| {
            (!entry.githubUserId.is_empty() || !entry.githubUsername.is_empty())
                && entry.githubUserId.is_empty()
        })
    {
        return false;
    }
    if config.creatorFee.mode == "github" && config.creatorFee.githubUserId.is_empty() {
        return false;
    }
    if config.mode == "agent-custom"
        && config.agent.feeRecipients.iter().any(|entry| {
            (!entry.githubUserId.is_empty() || !entry.githubUsername.is_empty())
                && entry.githubUserId.is_empty()
        })
    {
        return false;
    }
    true
}

pub async fn try_compile_native_pump(
    rpc_url: &str,
    config: &NormalizedConfig,
    wallet_secret: &[u8],
    built_at: String,
    creator_public_key: String,
    config_path: Option<String>,
) -> Result<Option<NativePumpArtifacts>, String> {
    if !supports_native_pump_compile(config) {
        return Ok(None);
    }

    let creator_keypair = keypair_from_secret_bytes(wallet_secret)?;
    let creator = creator_keypair.pubkey();
    let agent_authority = resolve_agent_authority(config, &creator)?;
    let mint_keypair = Keypair::new();
    let mint = mint_keypair.pubkey();
    let (launch_creator, launch_pre_instructions) =
        resolve_launch_creator_and_pre_instructions(rpc_url, config, &creator).await?;
    let bundle_jito_tip = config.tx.jitoTipLamports > 0 && config.mode != "agent-unlocked";
    let lookup_tables = load_lookup_table_accounts(rpc_url, config).await?;
    let tx_format = select_native_format(&config.execution.txFormat, !lookup_tables.is_empty())?;
    let global = if config.devBuy.is_some() {
        Some(fetch_global_state(rpc_url).await?)
    } else {
        None
    };

    let launch_tx_config = NativeTxConfig {
        compute_unit_price_micro_lamports: config.tx.computeUnitPriceMicroLamports.unwrap_or(0),
        jito_tip_lamports: if bundle_jito_tip {
            0
        } else {
            config.tx.jitoTipLamports
        },
        jito_tip_account: if bundle_jito_tip {
            String::new()
        } else {
            config.tx.jitoTipAccount.clone()
        },
    };

    let (blockhash, last_valid_block_height) = fetch_latest_blockhash(rpc_url, "confirmed").await?;
    let mut launch_instructions = launch_pre_instructions;
    launch_instructions.extend(build_launch_instructions(
        config,
        mint,
        creator,
        launch_creator,
        agent_authority.as_ref(),
        global.as_ref(),
    )?);
    let mut compiled_transactions = vec![compile_transaction(
        "launch",
        tx_format,
        &blockhash,
        last_valid_block_height,
        &creator_keypair,
        Some(&mint_keypair),
        with_tx_settings(launch_instructions, &launch_tx_config, &creator)?,
        &lookup_tables,
    )?];

    if let Some(follow_up_label) = native_follow_up_label(config) {
        let follow_up_instructions = build_native_follow_up_instructions(
            rpc_url,
            config,
            mint,
            creator,
            agent_authority.as_ref(),
        )
        .await?;
        compiled_transactions.push(compile_transaction(
            follow_up_label,
            tx_format,
            &blockhash,
            last_valid_block_height,
            &creator_keypair,
            None,
            with_tx_settings(
                follow_up_instructions,
                &NativeTxConfig {
                    compute_unit_price_micro_lamports: config
                        .tx
                        .computeUnitPriceMicroLamports
                        .unwrap_or(0),
                    jito_tip_lamports: 0,
                    jito_tip_account: String::new(),
                },
                &creator,
            )?,
            &lookup_tables,
        )?);
    }

    if bundle_jito_tip {
        let tip_instruction = build_jito_tip_instruction(config, creator)?;
        compiled_transactions.push(compile_transaction(
            "jito-tip",
            tx_format,
            &blockhash,
            last_valid_block_height,
            &creator_keypair,
            None,
            vec![tip_instruction],
            &lookup_tables,
        )?);
    }

    let mut report = build_report(
        config,
        built_at,
        rpc_url.to_string(),
        creator_public_key,
        mint.to_string(),
        agent_authority.map(|authority| authority.to_string()),
        config_path,
    );
    if let Some(first_warning) = report.execution.warnings.first_mut() {
        *first_warning =
            "Rust engine owns validation, runtime state, and API contracts. Native Pump assembly now covers LaunchDeck's Pump launch modes end-to-end; non-Pump flows still fall back to the JS compile bridge."
                .to_string();
    }
    report.execution.warnings.push(
        "Native Pump assembly now includes compute-budget and priority-fee instructions for supported launch shapes."
            .to_string(),
    );
    apply_transaction_details(&mut report, &compiled_transactions, config)?;
    let text = render_report(&report);
    let report = serde_json::to_value(report).map_err(|error| error.to_string())?;

    Ok(Some(NativePumpArtifacts {
        compiled_transactions,
        report,
        text,
    }))
}

struct NativeTxConfig {
    compute_unit_price_micro_lamports: i64,
    jito_tip_lamports: i64,
    jito_tip_account: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeTxFormat {
    Legacy,
    V0,
    V0Alt,
}

#[derive(Debug, Clone)]
struct PumpGlobalState {
    fee_recipient: Pubkey,
    initial_virtual_token_reserves: u64,
    initial_virtual_sol_reserves: u64,
    initial_real_token_reserves: u64,
    fee_basis_points: u64,
    creator_fee_basis_points: u64,
    fee_recipients: [Pubkey; 7],
    reserved_fee_recipient: Pubkey,
    reserved_fee_recipients: [Pubkey; 7],
}

fn keypair_from_secret_bytes(bytes: &[u8]) -> Result<Keypair, String> {
    Keypair::try_from(bytes).map_err(|error| error.to_string())
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|error| format!("Invalid {label}: {error}"))
}

fn pump_program_id() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_PROGRAM_ID, "PUMP program id")
}

fn mayhem_program_id() -> Result<Pubkey, String> {
    parse_pubkey(MAYHEM_PROGRAM_ID, "Mayhem program id")
}

fn pump_amm_program_id() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_AMM_PROGRAM_ID, "PUMP AMM program id")
}

fn token_2022_program_id() -> Result<Pubkey, String> {
    parse_pubkey(TOKEN_2022_PROGRAM_ID, "Token 2022 program id")
}

fn token_program_id() -> Result<Pubkey, String> {
    parse_pubkey(TOKEN_PROGRAM_ID, "Token program id")
}

fn pump_fee_program_id() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_FEE_PROGRAM_ID, "PUMP fee program id")
}

fn compute_budget_program_id() -> Result<Pubkey, String> {
    parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID, "Compute Budget program id")
}

fn pump_agent_payments_program_id() -> Result<Pubkey, String> {
    parse_pubkey(
        PUMP_AGENT_PAYMENTS_PROGRAM_ID,
        "Pump Agent Payments program id",
    )
}

fn wsol_mint() -> Result<Pubkey, String> {
    parse_pubkey(WSOL_MINT, "WSOL mint")
}

fn event_authority_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"__event_authority"], program_id).0
}

fn bonding_curve_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &pump_program_id()?).0)
}

fn mint_authority_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"mint-authority"], &pump_program_id()?).0)
}

fn global_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global"], &pump_program_id()?).0)
}

fn global_params_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global-params"], &mayhem_program_id()?).0)
}

fn global_volume_accumulator_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global_volume_accumulator"], &pump_program_id()?).0)
}

fn user_volume_accumulator_pda(user: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"user_volume_accumulator", user.as_ref()],
        &pump_program_id()?,
    )
    .0)
}

fn creator_vault_pda(creator: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"creator-vault", creator.as_ref()], &pump_program_id()?).0)
}

fn bonding_curve_v2_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"bonding-curve-v2", mint.as_ref()], &pump_program_id()?).0)
}

fn fee_config_pda() -> Result<Pubkey, String> {
    let pump_program = pump_program_id()?;
    let fee_program = pump_fee_program_id()?;
    Ok(Pubkey::find_program_address(&[b"fee_config", pump_program.as_ref()], &fee_program).0)
}

fn fee_sharing_config_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(
        Pubkey::find_program_address(&[b"sharing-config", mint.as_ref()], &pump_fee_program_id()?)
            .0,
    )
}

fn fee_program_global_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"fee-program-global"], &pump_fee_program_id()?).0)
}

fn agent_global_config_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global-config"], &pump_agent_payments_program_id()?).0)
}

fn token_agent_payments_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"token-agent-payments", mint.as_ref()],
        &pump_agent_payments_program_id()?,
    )
    .0)
}

fn coin_creator_vault_authority_pda(coin_creator: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"creator_vault", coin_creator.as_ref()],
        &pump_amm_program_id()?,
    )
    .0)
}

fn coin_creator_vault_ata_pda(coin_creator_vault_authority: &Pubkey) -> Result<Pubkey, String> {
    Ok(get_associated_token_address_with_program_id(
        coin_creator_vault_authority,
        &wsol_mint()?,
        &token_program_id()?,
    ))
}

fn social_fee_pda(user_id: &str, platform: u8) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"social-fee-pda", user_id.as_bytes(), &[platform]],
        &pump_fee_program_id()?,
    )
    .0)
}

async fn fetch_global_state(rpc_url: &str) -> Result<PumpGlobalState, String> {
    let account_data = fetch_account_data(rpc_url, &global_pda()?.to_string(), "confirmed").await?;
    decode_global_state(&account_data)
}

fn read_bool(data: &[u8], offset: &mut usize) -> Result<bool, String> {
    let Some(byte) = data.get(*offset) else {
        return Err("Unexpected end of global account while reading bool.".to_string());
    };
    *offset += 1;
    Ok(*byte != 0)
}

fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    let end = offset.saturating_add(8);
    let bytes: [u8; 8] = data
        .get(*offset..end)
        .ok_or_else(|| "Unexpected end of global account while reading u64.".to_string())?
        .try_into()
        .map_err(|_| "Failed to read u64 bytes from global account.".to_string())?;
    *offset = end;
    Ok(u64::from_le_bytes(bytes))
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Result<Pubkey, String> {
    let end = offset.saturating_add(32);
    let bytes: [u8; 32] = data
        .get(*offset..end)
        .ok_or_else(|| "Unexpected end of global account while reading pubkey.".to_string())?
        .try_into()
        .map_err(|_| "Failed to read pubkey bytes from global account.".to_string())?;
    *offset = end;
    Ok(Pubkey::new_from_array(bytes))
}

fn read_pubkey_array<const N: usize>(
    data: &[u8],
    offset: &mut usize,
) -> Result<[Pubkey; N], String> {
    let mut values = Vec::with_capacity(N);
    for _ in 0..N {
        values.push(read_pubkey(data, offset)?);
    }
    values
        .try_into()
        .map_err(|_| "Failed to decode pubkey array from global account.".to_string())
}

fn decode_global_state(data: &[u8]) -> Result<PumpGlobalState, String> {
    if data.len() < GLOBAL_ACCOUNT_DISCRIMINATOR_BYTES {
        return Err("Global account data was too short.".to_string());
    }
    let mut offset = GLOBAL_ACCOUNT_DISCRIMINATOR_BYTES;
    let _initialized = read_bool(data, &mut offset)?;
    let _authority = read_pubkey(data, &mut offset)?;
    let fee_recipient = read_pubkey(data, &mut offset)?;
    let initial_virtual_token_reserves = read_u64(data, &mut offset)?;
    let initial_virtual_sol_reserves = read_u64(data, &mut offset)?;
    let initial_real_token_reserves = read_u64(data, &mut offset)?;
    let _token_total_supply = read_u64(data, &mut offset)?;
    let fee_basis_points = read_u64(data, &mut offset)?;
    let _withdraw_authority = read_pubkey(data, &mut offset)?;
    let _enable_migrate = read_bool(data, &mut offset)?;
    let _pool_migration_fee = read_u64(data, &mut offset)?;
    let creator_fee_basis_points = read_u64(data, &mut offset)?;
    let fee_recipients = read_pubkey_array::<7>(data, &mut offset)?;
    let _set_creator_authority = read_pubkey(data, &mut offset)?;
    let _admin_set_creator_authority = read_pubkey(data, &mut offset)?;
    let _create_v2_enabled = read_bool(data, &mut offset)?;
    let _whitelist_pda = read_pubkey(data, &mut offset)?;
    let reserved_fee_recipient = read_pubkey(data, &mut offset)?;
    let _mayhem_mode_enabled = read_bool(data, &mut offset)?;
    let reserved_fee_recipients = read_pubkey_array::<7>(data, &mut offset)?;
    let _is_cashback_enabled = read_bool(data, &mut offset)?;

    Ok(PumpGlobalState {
        fee_recipient,
        initial_virtual_token_reserves,
        initial_virtual_sol_reserves,
        initial_real_token_reserves,
        fee_basis_points,
        creator_fee_basis_points,
        fee_recipients,
        reserved_fee_recipient,
        reserved_fee_recipients,
    })
}

fn parse_decimal_u64(value: &str, decimals: u32, label: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} cannot be blank."));
    }
    if !trimmed
        .chars()
        .all(|char| char.is_ascii_digit() || char == '.')
    {
        return Err(format!(
            "{label} must be a positive decimal string. Got: {value}"
        ));
    }
    let mut parts = trimmed.split('.');
    let whole = parts.next().unwrap_or_default();
    let fractional = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err(format!(
            "{label} must be a positive decimal string. Got: {value}"
        ));
    }
    if fractional.len() > decimals as usize {
        return Err(format!(
            "{label} supports at most {decimals} decimal places. Got: {value}"
        ));
    }
    let mut normalized = String::new();
    normalized.push_str(whole);
    normalized.push_str(&format!("{fractional:0<width$}", width = decimals as usize));
    let digits = normalized.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(0);
    }
    digits.parse::<u64>().map_err(|error| error.to_string())
}

fn ceil_div(numerator: u128, denominator: u128) -> u128 {
    numerator.div_ceil(denominator)
}

fn compute_total_fee_basis_points(global: &PumpGlobalState, include_creator_fee: bool) -> u128 {
    u128::from(global.fee_basis_points)
        + if include_creator_fee {
            u128::from(global.creator_fee_basis_points)
        } else {
            0
        }
}

fn quote_buy_tokens_from_sol(global: &PumpGlobalState, spendable_sol: u64) -> u64 {
    if spendable_sol == 0 {
        return 0;
    }
    let total_fee_basis_points = compute_total_fee_basis_points(global, true);
    let input_amount = ((u128::from(spendable_sol).saturating_sub(1)) * 10_000)
        / (10_000 + total_fee_basis_points);
    let virtual_token_reserves = u128::from(global.initial_virtual_token_reserves);
    let virtual_sol_reserves = u128::from(global.initial_virtual_sol_reserves);
    let tokens = (input_amount * virtual_token_reserves) / (virtual_sol_reserves + input_amount);
    tokens
        .min(u128::from(global.initial_real_token_reserves))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn quote_buy_sol_from_tokens(global: &PumpGlobalState, token_amount: u64) -> u64 {
    if token_amount == 0 {
        return 0;
    }
    let amount = u128::from(token_amount).min(u128::from(global.initial_real_token_reserves));
    let virtual_token_reserves = u128::from(global.initial_virtual_token_reserves);
    let virtual_sol_reserves = u128::from(global.initial_virtual_sol_reserves);
    let sol_cost = ((amount * virtual_sol_reserves) / (virtual_token_reserves - amount)) + 1;
    let protocol_fee = ceil_div(sol_cost * u128::from(global.fee_basis_points), 10_000);
    let creator_fee = ceil_div(
        sol_cost * u128::from(global.creator_fee_basis_points),
        10_000,
    );
    (sol_cost + protocol_fee + creator_fee)
        .min(u128::from(u64::MAX))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn resolve_dev_buy_quote(
    config: &NormalizedConfig,
    global: &PumpGlobalState,
) -> Result<Option<(u64, u64)>, String> {
    let Some(dev_buy) = &config.devBuy else {
        return Ok(None);
    };
    if dev_buy.mode == "sol" {
        let sol_amount = parse_decimal_u64(&dev_buy.amount, 9, "devBuy.amount")?;
        return Ok(Some((
            sol_amount,
            quote_buy_tokens_from_sol(global, sol_amount),
        )));
    }
    if dev_buy.mode == "tokens" {
        let token_amount = parse_decimal_u64(&dev_buy.amount, TOKEN_DECIMALS, "devBuy.amount")?;
        return Ok(Some((
            quote_buy_sol_from_tokens(global, token_amount),
            token_amount,
        )));
    }
    Err(format!(
        "Unsupported devBuy.mode for native Pump compile: {}",
        dev_buy.mode
    ))
}

fn select_buy_fee_recipient(global: &PumpGlobalState, mayhem_mode: bool) -> Pubkey {
    if mayhem_mode {
        if global.reserved_fee_recipient != Pubkey::default() {
            return global.reserved_fee_recipient;
        }
        if let Some(entry) = global
            .reserved_fee_recipients
            .iter()
            .copied()
            .find(|entry| *entry != Pubkey::default())
        {
            return entry;
        }
    } else {
        if global.fee_recipient != Pubkey::default() {
            return global.fee_recipient;
        }
        if let Some(entry) = global
            .fee_recipients
            .iter()
            .copied()
            .find(|entry| *entry != Pubkey::default())
        {
            return entry;
        }
    }
    Pubkey::default()
}

fn sol_vault_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"sol-vault"], &mayhem_program_id()?).0)
}

fn mayhem_state_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"mayhem-state", mint.as_ref()], &mayhem_program_id()?).0)
}

async fn resolve_launch_creator_and_pre_instructions(
    rpc_url: &str,
    config: &NormalizedConfig,
    creator: &Pubkey,
) -> Result<(Pubkey, Vec<Instruction>), String> {
    if config.creatorFee.mode == "wallet" && !config.creatorFee.address.is_empty() {
        return Ok((
            parse_pubkey(&config.creatorFee.address, "creatorFee.address")?,
            vec![],
        ));
    }
    if config.creatorFee.mode == "github" && !config.creatorFee.githubUserId.is_empty() {
        let social_fee = social_fee_pda(&config.creatorFee.githubUserId, PLATFORM_GITHUB)?;
        let mut pre_instructions = Vec::new();
        if !fetch_account_exists(rpc_url, &social_fee.to_string(), "confirmed").await? {
            pre_instructions.push(build_create_social_fee_pda_instruction(
                creator,
                &config.creatorFee.githubUserId,
                PLATFORM_GITHUB,
            )?);
        }
        return Ok((social_fee, pre_instructions));
    }
    Ok((*creator, vec![]))
}

fn resolve_agent_authority(
    config: &NormalizedConfig,
    creator: &Pubkey,
) -> Result<Option<Pubkey>, String> {
    if config.mode == "regular" || config.mode == "cashback" {
        return Ok(None);
    }
    if config.agent.authority.trim().is_empty() {
        return Ok(Some(*creator));
    }
    Ok(Some(parse_pubkey(
        &config.agent.authority,
        "agent.authority",
    )?))
}

fn build_launch_instructions(
    config: &NormalizedConfig,
    mint: Pubkey,
    creator: Pubkey,
    launch_creator: Pubkey,
    agent_authority: Option<&Pubkey>,
    global: Option<&PumpGlobalState>,
) -> Result<Vec<Instruction>, String> {
    let mut instructions = vec![build_create_v2_instruction(
        &mint,
        &creator,
        &launch_creator,
        &config.token.name,
        &config.token.symbol,
        &config.token.uri,
        config.token.mayhemMode,
        config.mode == "cashback",
    )?];
    if let Some(global) = global {
        if let Some((sol_amount, token_amount)) = resolve_dev_buy_quote(config, global)? {
            instructions.push(build_extend_account_instruction(
                &bonding_curve_pda(&mint)?,
                &creator,
            )?);
            instructions.push(build_create_token_ata_instruction(&creator, &mint)?);
            instructions.push(build_buy_instruction(
                global,
                &mint,
                &launch_creator,
                &creator,
                sol_amount,
                token_amount,
                config.token.mayhemMode,
            )?);
        }
    }
    if config.mode == "agent-unlocked" {
        let authority = agent_authority
            .ok_or_else(|| "agent authority is required for agent-unlocked mode.".to_string())?;
        instructions.push(build_agent_initialize_instruction(
            &mint,
            &creator,
            authority,
            config.agent.buybackBps.unwrap_or(0) as u16,
        )?);
    }
    Ok(instructions)
}

fn encode_borsh_string(buffer: &mut Vec<u8>, value: &str) {
    buffer.extend_from_slice(&(value.len() as u32).to_le_bytes());
    buffer.extend_from_slice(value.as_bytes());
}

fn build_create_v2_instruction(
    mint: &Pubkey,
    user: &Pubkey,
    creator: &Pubkey,
    name: &str,
    symbol: &str,
    uri: &str,
    mayhem_mode: bool,
    cashback_enabled: bool,
) -> Result<Instruction, String> {
    let pump_program = pump_program_id()?;
    let token_2022 = token_2022_program_id()?;
    let associated_bonding_curve =
        get_associated_token_address_with_program_id(&bonding_curve_pda(mint)?, mint, &token_2022);
    let sol_vault = sol_vault_pda()?;
    let mayhem_state = mayhem_state_pda(mint)?;
    let mayhem_token_vault =
        get_associated_token_address_with_program_id(&sol_vault, mint, &token_2022);

    let mut data = vec![214, 144, 76, 236, 95, 139, 49, 180];
    encode_borsh_string(&mut data, name);
    encode_borsh_string(&mut data, symbol);
    encode_borsh_string(&mut data, uri);
    data.extend_from_slice(creator.as_ref());
    data.push(u8::from(mayhem_mode));
    data.push(u8::from(cashback_enabled));

    Ok(Instruction {
        program_id: pump_program,
        accounts: vec![
            AccountMeta::new(*mint, true),
            AccountMeta::new_readonly(mint_authority_pda()?, false),
            AccountMeta::new(bonding_curve_pda(mint)?, false),
            AccountMeta::new(associated_bonding_curve, false),
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new(*user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(token_2022, false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new(mayhem_program_id()?, false),
            AccountMeta::new_readonly(global_params_pda()?, false),
            AccountMeta::new(sol_vault, false),
            AccountMeta::new(mayhem_state, false),
            AccountMeta::new(mayhem_token_vault, false),
            AccountMeta::new_readonly(event_authority_pda(&pump_program), false),
            AccountMeta::new_readonly(pump_program, false),
        ],
        data,
    })
}

fn build_jito_tip_instruction(
    config: &NormalizedConfig,
    payer: Pubkey,
) -> Result<Instruction, String> {
    let tip_account = parse_pubkey(&config.tx.jitoTipAccount, "tx.jitoTipAccount")?;
    Ok(transfer(
        &payer,
        &tip_account,
        config.tx.jitoTipLamports as u64,
    ))
}

fn build_agent_initialize_instruction(
    mint: &Pubkey,
    creator: &Pubkey,
    agent_authority: &Pubkey,
    buyback_bps: u16,
) -> Result<Instruction, String> {
    let program_id = pump_agent_payments_program_id()?;
    let mut data = vec![180, 248, 163, 8, 49, 94, 126, 96];
    data.extend_from_slice(agent_authority.as_ref());
    data.extend_from_slice(&buyback_bps.to_le_bytes());
    Ok(Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*creator, true),
            AccountMeta::new_readonly(bonding_curve_pda(mint)?, false),
            AccountMeta::new(agent_global_config_pda()?, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(token_agent_payments_pda(mint)?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&program_id), false),
            AccountMeta::new_readonly(program_id, false),
        ],
        data,
    })
}

fn build_extend_account_instruction(
    account: &Pubkey,
    user: &Pubkey,
) -> Result<Instruction, String> {
    let program_id = pump_program_id()?;
    Ok(Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*account, false),
            AccountMeta::new_readonly(*user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&program_id), false),
            AccountMeta::new_readonly(program_id, false),
        ],
        data: vec![234, 102, 194, 203, 150, 72, 62, 229],
    })
}

fn build_create_token_ata_instruction(
    owner: &Pubkey,
    mint: &Pubkey,
) -> Result<Instruction, String> {
    Ok(
        spl_associated_token_account::instruction::create_associated_token_account_idempotent(
            owner,
            owner,
            mint,
            &token_2022_program_id()?,
        ),
    )
}

fn build_buy_instruction(
    global: &PumpGlobalState,
    mint: &Pubkey,
    launch_creator: &Pubkey,
    user: &Pubkey,
    sol_amount: u64,
    token_amount: u64,
    mayhem_mode: bool,
) -> Result<Instruction, String> {
    let pump_program = pump_program_id()?;
    let token_2022 = token_2022_program_id()?;
    let bonding_curve = bonding_curve_pda(mint)?;
    let associated_bonding_curve =
        get_associated_token_address_with_program_id(&bonding_curve, mint, &token_2022);
    let associated_user = get_associated_token_address_with_program_id(user, mint, &token_2022);
    let mut data = vec![102, 6, 61, 18, 1, 218, 235, 234];
    data.extend_from_slice(&token_amount.to_le_bytes());
    data.extend_from_slice(&sol_amount.to_le_bytes());
    data.push(1);
    Ok(Instruction {
        program_id: pump_program,
        accounts: vec![
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new(select_buy_fee_recipient(global, mayhem_mode), false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(bonding_curve, false),
            AccountMeta::new(associated_bonding_curve, false),
            AccountMeta::new(associated_user, false),
            AccountMeta::new(*user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(token_2022, false),
            AccountMeta::new(creator_vault_pda(launch_creator)?, false),
            AccountMeta::new_readonly(event_authority_pda(&pump_program), false),
            AccountMeta::new_readonly(pump_program, false),
            AccountMeta::new_readonly(global_volume_accumulator_pda()?, false),
            AccountMeta::new(user_volume_accumulator_pda(user)?, false),
            AccountMeta::new_readonly(bonding_curve_v2_pda(mint)?, false),
            AccountMeta::new_readonly(fee_config_pda()?, false),
            AccountMeta::new_readonly(pump_fee_program_id()?, false),
        ],
        data,
    })
}

fn recipient_pubkey(recipient: &NormalizedRecipient) -> Result<Pubkey, String> {
    if !recipient.address.is_empty() {
        return parse_pubkey(&recipient.address, "feeSharing.recipient.address");
    }
    if !recipient.githubUserId.is_empty() {
        return social_fee_pda(&recipient.githubUserId, PLATFORM_GITHUB);
    }
    Err("Each fee-sharing recipient must provide either an address or githubUserId.".to_string())
}

fn encode_shareholders(recipients: &[NormalizedRecipient]) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    data.extend_from_slice(&(recipients.len() as u32).to_le_bytes());
    for recipient in recipients {
        let address = recipient_pubkey(recipient)?;
        data.extend_from_slice(address.as_ref());
        let share_bps = u16::try_from(recipient.shareBps).map_err(|_| {
            format!(
                "shareBps is out of range for recipient {}",
                recipient.address
            )
        })?;
        data.extend_from_slice(&share_bps.to_le_bytes());
    }
    Ok(data)
}

fn build_create_fee_sharing_config_instruction(
    mint: &Pubkey,
    payer: &Pubkey,
) -> Result<Instruction, String> {
    let program_id = pump_fee_program_id()?;
    Ok(Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(event_authority_pda(&program_id), false),
            AccountMeta::new_readonly(program_id, false),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(fee_sharing_config_pda(mint)?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(bonding_curve_pda(mint)?, false),
            AccountMeta::new_readonly(pump_program_id()?, false),
            AccountMeta::new_readonly(event_authority_pda(&pump_program_id()?), false),
        ],
        data: vec![195, 78, 86, 76, 111, 52, 251, 213],
    })
}

fn build_update_fee_shares_instruction(
    mint: &Pubkey,
    authority: &Pubkey,
    recipients: &[crate::config::NormalizedRecipient],
) -> Result<Instruction, String> {
    let program_id = pump_fee_program_id()?;
    let sharing_config = fee_sharing_config_pda(mint)?;
    let coin_creator_vault_authority = coin_creator_vault_authority_pda(&sharing_config)?;
    let mut data = vec![189, 13, 136, 99, 187, 164, 237, 35];
    data.extend_from_slice(&encode_shareholders(recipients)?);
    let mut accounts = vec![
        AccountMeta::new_readonly(event_authority_pda(&program_id), false),
        AccountMeta::new_readonly(program_id, false),
        AccountMeta::new_readonly(*authority, true),
        AccountMeta::new_readonly(global_pda()?, false),
        AccountMeta::new_readonly(*mint, false),
        AccountMeta::new(sharing_config, false),
        AccountMeta::new_readonly(bonding_curve_pda(mint)?, false),
        AccountMeta::new(creator_vault_pda(&sharing_config)?, false),
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(pump_program_id()?, false),
        AccountMeta::new_readonly(event_authority_pda(&pump_program_id()?), false),
        AccountMeta::new_readonly(pump_amm_program_id()?, false),
        AccountMeta::new_readonly(event_authority_pda(&pump_amm_program_id()?), false),
        AccountMeta::new_readonly(wsol_mint()?, false),
        AccountMeta::new_readonly(token_program_id()?, false),
        AccountMeta::new_readonly(spl_associated_token_account::id(), false),
        AccountMeta::new(coin_creator_vault_authority, false),
        AccountMeta::new(
            coin_creator_vault_ata_pda(&coin_creator_vault_authority)?,
            false,
        ),
    ];
    accounts.push(AccountMeta::new(*authority, false));
    Ok(Instruction {
        program_id,
        accounts,
        data,
    })
}

fn build_revoke_fee_sharing_authority_instruction(
    mint: &Pubkey,
    payer: &Pubkey,
) -> Result<Instruction, String> {
    let program_id = pump_fee_program_id()?;
    Ok(Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(fee_sharing_config_pda(mint)?, false),
            AccountMeta::new_readonly(event_authority_pda(&program_id), false),
            AccountMeta::new_readonly(program_id, false),
        ],
        data: vec![18, 233, 158, 39, 185, 207, 58, 104],
    })
}

fn build_create_social_fee_pda_instruction(
    payer: &Pubkey,
    user_id: &str,
    platform: u8,
) -> Result<Instruction, String> {
    let program_id = pump_fee_program_id()?;
    let mut data = vec![144, 224, 59, 211, 78, 248, 202, 220];
    encode_borsh_string(&mut data, user_id);
    data.push(platform);
    Ok(Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(social_fee_pda(user_id, platform)?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(fee_program_global_pda()?, false),
            AccountMeta::new_readonly(event_authority_pda(&program_id), false),
            AccountMeta::new_readonly(program_id, false),
        ],
        data,
    })
}

async fn build_social_fee_pda_create_instructions(
    rpc_url: &str,
    recipients: &[NormalizedRecipient],
    payer: &Pubkey,
) -> Result<Vec<Instruction>, String> {
    let mut instructions = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for recipient in recipients {
        if recipient.githubUserId.is_empty() {
            continue;
        }
        if !seen.insert(recipient.githubUserId.clone()) {
            continue;
        }
        let social_fee = social_fee_pda(&recipient.githubUserId, PLATFORM_GITHUB)?;
        if !fetch_account_exists(rpc_url, &social_fee.to_string(), "confirmed").await? {
            instructions.push(build_create_social_fee_pda_instruction(
                payer,
                &recipient.githubUserId,
                PLATFORM_GITHUB,
            )?);
        }
    }
    Ok(instructions)
}

async fn build_fee_sharing_follow_up_instructions(
    rpc_url: &str,
    config: &NormalizedConfig,
    mint: Pubkey,
    creator: Pubkey,
) -> Result<Vec<Instruction>, String> {
    build_fee_sharing_setup_instructions(
        rpc_url,
        mint,
        creator,
        &config.feeSharing.recipients,
        true,
    )
    .await
}

async fn build_fee_sharing_setup_instructions(
    rpc_url: &str,
    mint: Pubkey,
    creator: Pubkey,
    recipients: &[NormalizedRecipient],
    lock: bool,
) -> Result<Vec<Instruction>, String> {
    if recipients.is_empty() {
        return Err("fee sharing recipients are required for native follow-up setup.".to_string());
    }
    let mut instructions = vec![build_create_fee_sharing_config_instruction(
        &mint, &creator,
    )?];
    instructions
        .extend(build_social_fee_pda_create_instructions(rpc_url, recipients, &creator).await?);
    instructions.push(build_update_fee_shares_instruction(
        &mint, &creator, recipients,
    )?);
    if lock {
        instructions.push(build_revoke_fee_sharing_authority_instruction(
            &mint, &creator,
        )?);
    }
    Ok(instructions)
}

fn resolve_agent_fee_recipients(
    config: &NormalizedConfig,
    mint: &Pubkey,
    creator: &Pubkey,
) -> Result<Vec<NormalizedRecipient>, String> {
    let configured = if config.agent.feeRecipients.is_empty() {
        let buyback_bps = config.agent.buybackBps.unwrap_or(0);
        let mut recipients = Vec::new();
        if buyback_bps > 0 {
            recipients.push(NormalizedRecipient {
                r#type: Some("agent".to_string()),
                address: String::new(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: buyback_bps,
            });
        }
        let wallet_share = 10_000 - buyback_bps;
        if wallet_share > 0 {
            recipients.push(NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: if config.agent.feeReceiver.is_empty() {
                    creator.to_string()
                } else {
                    config.agent.feeReceiver.clone()
                },
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: wallet_share,
            });
        }
        recipients
    } else {
        config.agent.feeRecipients.clone()
    };

    let agent_receiver = token_agent_payments_pda(mint)?.to_string();
    let mut resolved = Vec::with_capacity(configured.len());
    for entry in configured {
        if entry.r#type.as_deref() == Some("agent") {
            resolved.push(NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: agent_receiver.clone(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: entry.shareBps,
            });
        } else {
            resolved.push(entry);
        }
    }
    Ok(resolved)
}

fn native_follow_up_label(config: &NormalizedConfig) -> Option<&'static str> {
    match config.mode.as_str() {
        "regular" | "cashback" if config.feeSharing.generateLaterSetup => Some("follow-up"),
        "agent-custom" => Some("agent-setup"),
        "agent-locked" => Some("agent-setup"),
        _ => None,
    }
}

async fn build_native_follow_up_instructions(
    rpc_url: &str,
    config: &NormalizedConfig,
    mint: Pubkey,
    creator: Pubkey,
    agent_authority: Option<&Pubkey>,
) -> Result<Vec<Instruction>, String> {
    match config.mode.as_str() {
        "regular" | "cashback" => {
            build_fee_sharing_follow_up_instructions(rpc_url, config, mint, creator).await
        }
        "agent-custom" => {
            let authority = agent_authority
                .ok_or_else(|| "agent authority is required for agent-custom mode.".to_string())?;
            let mut instructions = vec![build_agent_initialize_instruction(
                &mint,
                &creator,
                authority,
                config.agent.buybackBps.unwrap_or(0) as u16,
            )?];
            let recipients = resolve_agent_fee_recipients(config, &mint, &creator)?;
            instructions.extend(
                build_fee_sharing_setup_instructions(rpc_url, mint, creator, &recipients, false)
                    .await?,
            );
            Ok(instructions)
        }
        "agent-locked" => {
            let authority = agent_authority
                .ok_or_else(|| "agent authority is required for agent-locked mode.".to_string())?;
            let mut instructions = vec![build_agent_initialize_instruction(
                &mint,
                &creator,
                authority,
                config.agent.buybackBps.unwrap_or(0) as u16,
            )?];
            let recipients = vec![NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: token_agent_payments_pda(&mint)?.to_string(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 10_000,
            }];
            instructions.extend(
                build_fee_sharing_setup_instructions(rpc_url, mint, creator, &recipients, true)
                    .await?,
            );
            Ok(instructions)
        }
        unsupported => Err(format!(
            "Native Pump follow-up builder does not support mode={unsupported}."
        )),
    }
}

fn build_compute_unit_limit_instruction() -> Result<Instruction, String> {
    let mut data = vec![2];
    data.extend_from_slice(&FIXED_COMPUTE_UNIT_LIMIT.to_le_bytes());
    Ok(Instruction {
        program_id: compute_budget_program_id()?,
        accounts: vec![],
        data,
    })
}

fn build_compute_unit_price_instruction(micro_lamports: u64) -> Result<Instruction, String> {
    let mut data = vec![3];
    data.extend_from_slice(&micro_lamports.to_le_bytes());
    Ok(Instruction {
        program_id: compute_budget_program_id()?,
        accounts: vec![],
        data,
    })
}

fn with_tx_settings(
    core_instructions: Vec<Instruction>,
    tx_config: &NativeTxConfig,
    payer: &Pubkey,
) -> Result<Vec<Instruction>, String> {
    let mut instructions = vec![build_compute_unit_limit_instruction()?];
    if tx_config.compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            tx_config.compute_unit_price_micro_lamports as u64,
        )?);
    }
    instructions.extend(core_instructions);
    if tx_config.jito_tip_lamports > 0 {
        let tip_account = parse_pubkey(&tx_config.jito_tip_account, "tx.jitoTipAccount")?;
        instructions.push(transfer(
            payer,
            &tip_account,
            tx_config.jito_tip_lamports as u64,
        ));
    }
    Ok(instructions)
}

fn parse_lookup_table_addresses(config: &NormalizedConfig) -> Vec<String> {
    let mut values = Vec::new();
    if config.tx.useDefaultLookupTables {
        values.extend(DEFAULT_LOOKUP_TABLES.iter().map(|entry| entry.to_string()));
    }
    values.extend(config.tx.lookupTables.clone());
    let mut deduped = Vec::new();
    for value in values {
        if !deduped.iter().any(|entry| entry == &value) {
            deduped.push(value);
        }
    }
    deduped
}

async fn load_lookup_table_accounts(
    rpc_url: &str,
    config: &NormalizedConfig,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let requested = parse_lookup_table_addresses(config);
    let should_load = matches!(config.execution.txFormat.as_str(), "auto" | "v0-alt");
    if !should_load || requested.is_empty() {
        return Ok(vec![]);
    }

    let mut lookup_tables = Vec::new();
    for address in requested {
        if !fetch_account_exists(rpc_url, &address, "confirmed").await? {
            continue;
        }
        let data = fetch_account_data(rpc_url, &address, "confirmed").await?;
        let table = AddressLookupTable::deserialize(&data)
            .map_err(|error| format!("Failed to decode address lookup table {address}: {error}"))?;
        lookup_tables.push(AddressLookupTableAccount {
            key: parse_pubkey(&address, "lookup table address")?,
            addresses: table.addresses.to_vec(),
        });
    }
    Ok(lookup_tables)
}

fn select_native_format(
    requested: &str,
    has_lookup_tables: bool,
) -> Result<NativeTxFormat, String> {
    match requested {
        "legacy" => Ok(NativeTxFormat::Legacy),
        "v0" => Ok(NativeTxFormat::V0),
        "v0-alt" => {
            if has_lookup_tables {
                Ok(NativeTxFormat::V0Alt)
            } else {
                Err("Native Pump compile requires at least one loaded lookup table for txFormat=v0-alt.".to_string())
            }
        }
        "auto" => {
            if has_lookup_tables {
                Ok(NativeTxFormat::V0Alt)
            } else {
                Ok(NativeTxFormat::V0)
            }
        }
        unsupported => Err(format!(
            "Native Pump compile does not yet support txFormat={unsupported}."
        )),
    }
}

fn compile_transaction(
    label: &str,
    tx_format: NativeTxFormat,
    blockhash: &str,
    last_valid_block_height: u64,
    payer: &Keypair,
    mint_signer: Option<&Keypair>,
    instructions: Vec<Instruction>,
    lookup_tables: &[AddressLookupTableAccount],
) -> Result<CompiledTransaction, String> {
    let hash = Hash::from_str(blockhash).map_err(|error| error.to_string())?;
    let (serialized, format, lookup_tables_used) = if tx_format == NativeTxFormat::Legacy {
        let signers: Vec<&Keypair> = match mint_signer {
            Some(mint) => vec![payer, mint],
            None => vec![payer],
        };
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &signers,
            hash,
        );
        (
            bincode::serialize(&transaction).map_err(|error| error.to_string())?,
            "legacy".to_string(),
            vec![],
        )
    } else {
        let dynamic_lookups = if tx_format == NativeTxFormat::V0Alt {
            lookup_tables
        } else {
            &[]
        };
        let message =
            v0::Message::try_compile(&payer.pubkey(), &instructions, dynamic_lookups, hash)
                .map_err(|error| error.to_string())?;
        let lookup_tables_used = message
            .address_table_lookups
            .iter()
            .map(|lookup| lookup.account_key.to_string())
            .collect::<Vec<_>>();
        let signers: Vec<&Keypair> = match mint_signer {
            Some(mint) => vec![payer, mint],
            None => vec![payer],
        };
        let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
            .map_err(|error| error.to_string())?;
        (
            bincode::serialize(&transaction).map_err(|error| error.to_string())?,
            if lookup_tables_used.is_empty() {
                "v0".to_string()
            } else {
                "v0-alt".to_string()
            },
            lookup_tables_used,
        )
    };

    Ok(CompiledTransaction {
        label: label.to_string(),
        format,
        blockhash: blockhash.to_string(),
        lastValidBlockHeight: last_valid_block_height,
        serializedBase64: BASE64.encode(serialized),
        lookupTablesUsed: lookup_tables_used,
    })
}

fn apply_transaction_details(
    report: &mut LaunchReport,
    compiled_transactions: &[CompiledTransaction],
    config: &NormalizedConfig,
) -> Result<(), String> {
    for (index, summary) in report.transactions.iter_mut().enumerate() {
        let Some(compiled) = compiled_transactions.get(index) else {
            break;
        };
        let raw = BASE64
            .decode(&compiled.serializedBase64)
            .map_err(|error| error.to_string())?;
        if compiled.format == "legacy" {
            summary.legacyLength = Some(raw.len());
            summary.v0Error = Some("Native path compiled as legacy only.".to_string());
            summary.v0AltError = Some("Native path compiled as legacy only.".to_string());
        } else {
            if compiled.format == "v0-alt" {
                summary.v0AltLength = Some(raw.len());
                summary.v0Error = Some("Native path compiled with lookup tables.".to_string());
            } else {
                summary.v0Length = Some(raw.len());
                summary.v0AltError =
                    Some("Native path compiled as versioned without lookup tables.".to_string());
            }
            summary.legacyError = Some("Native path compiled as versioned only.".to_string());
        }
        summary.fitsWithAlts = compiled.format == "v0-alt";
        summary.lookupTablesUsed = if compiled.format == "v0-alt" {
            compiled.lookupTablesUsed.clone()
        } else {
            vec![]
        };
        summary.exceedsPacketLimit = raw.len() > 1232;
        summary.instructionSummary = if summary.label == "launch" {
            {
                let mut instructions = vec![crate::report::InstructionSummary {
                    index: 0,
                    programId: COMPUTE_BUDGET_PROGRAM_ID.to_string(),
                    keyCount: 0,
                    writableKeys: 0,
                    signerKeys: 0,
                }];
                let next_index = if summary
                    .feeSettings
                    .computeUnitPriceMicroLamports
                    .unwrap_or(0)
                    > 0
                {
                    instructions.push(crate::report::InstructionSummary {
                        index: 1,
                        programId: COMPUTE_BUDGET_PROGRAM_ID.to_string(),
                        keyCount: 0,
                        writableKeys: 0,
                        signerKeys: 0,
                    });
                    2
                } else {
                    1
                };
                let mut next_index = next_index;
                instructions.push(crate::report::InstructionSummary {
                    index: next_index,
                    programId: PUMP_PROGRAM_ID.to_string(),
                    keyCount: 16,
                    writableKeys: 8,
                    signerKeys: 2,
                });
                next_index += 1;
                if config.devBuy.is_some() {
                    instructions.push(crate::report::InstructionSummary {
                        index: next_index,
                        programId: PUMP_PROGRAM_ID.to_string(),
                        keyCount: 5,
                        writableKeys: 1,
                        signerKeys: 1,
                    });
                    next_index += 1;
                    instructions.push(crate::report::InstructionSummary {
                        index: next_index,
                        programId: spl_associated_token_account::id().to_string(),
                        keyCount: 6,
                        writableKeys: 2,
                        signerKeys: 1,
                    });
                    next_index += 1;
                    instructions.push(crate::report::InstructionSummary {
                        index: next_index,
                        programId: PUMP_PROGRAM_ID.to_string(),
                        keyCount: 17,
                        writableKeys: 8,
                        signerKeys: 1,
                    });
                    next_index += 1;
                }
                if config.mode == "agent-unlocked" {
                    instructions.push(crate::report::InstructionSummary {
                        index: next_index,
                        programId: PUMP_AGENT_PAYMENTS_PROGRAM_ID.to_string(),
                        keyCount: 8,
                        writableKeys: 3,
                        signerKeys: 1,
                    });
                    next_index += 1;
                }
                if summary.feeSettings.jitoTipLamports > 0 {
                    instructions.push(crate::report::InstructionSummary {
                        index: next_index,
                        programId: "11111111111111111111111111111111".to_string(),
                        keyCount: 2,
                        writableKeys: 2,
                        signerKeys: 1,
                    });
                }
                instructions
            }
        } else if summary.label == "follow-up" || summary.label == "agent-setup" {
            let social_count = if summary.label == "follow-up" {
                config
                    .feeSharing
                    .recipients
                    .iter()
                    .filter(|entry| !entry.githubUserId.is_empty())
                    .count()
            } else if config.mode == "agent-custom" {
                config
                    .agent
                    .feeRecipients
                    .iter()
                    .filter(|entry| !entry.githubUserId.is_empty())
                    .count()
            } else {
                0
            };
            let mut instructions = vec![crate::report::InstructionSummary {
                index: 0,
                programId: COMPUTE_BUDGET_PROGRAM_ID.to_string(),
                keyCount: 0,
                writableKeys: 0,
                signerKeys: 0,
            }];
            let mut next_index = 1;
            if summary
                .feeSettings
                .computeUnitPriceMicroLamports
                .unwrap_or(0)
                > 0
            {
                instructions.push(crate::report::InstructionSummary {
                    index: 1,
                    programId: COMPUTE_BUDGET_PROGRAM_ID.to_string(),
                    keyCount: 0,
                    writableKeys: 0,
                    signerKeys: 0,
                });
                next_index = 2;
            }
            if summary.label == "agent-setup" {
                instructions.push(crate::report::InstructionSummary {
                    index: next_index,
                    programId: PUMP_AGENT_PAYMENTS_PROGRAM_ID.to_string(),
                    keyCount: 8,
                    writableKeys: 3,
                    signerKeys: 1,
                });
                next_index += 1;
            }
            instructions.push(crate::report::InstructionSummary {
                index: next_index,
                programId: PUMP_FEE_PROGRAM_ID.to_string(),
                keyCount: 10,
                writableKeys: 3,
                signerKeys: 1,
            });
            for offset in 0..social_count {
                instructions.push(crate::report::InstructionSummary {
                    index: next_index + 1 + offset,
                    programId: PUMP_FEE_PROGRAM_ID.to_string(),
                    keyCount: 6,
                    writableKeys: 2,
                    signerKeys: 1,
                });
            }
            instructions.push(crate::report::InstructionSummary {
                index: next_index + 1 + social_count,
                programId: PUMP_FEE_PROGRAM_ID.to_string(),
                keyCount: 19,
                writableKeys: 4,
                signerKeys: 1,
            });
            if summary.label == "follow-up" || config.mode == "agent-locked" {
                instructions.push(crate::report::InstructionSummary {
                    index: next_index + 2 + social_count,
                    programId: PUMP_FEE_PROGRAM_ID.to_string(),
                    keyCount: 6,
                    writableKeys: 2,
                    signerKeys: 1,
                });
            }
            instructions
        } else if summary.label == "jito-tip" {
            vec![crate::report::InstructionSummary {
                index: 0,
                programId: "11111111111111111111111111111111".to_string(),
                keyCount: 2,
                writableKeys: 2,
                signerKeys: 1,
            }]
        } else {
            vec![crate::report::InstructionSummary {
                index: 0,
                programId: "11111111111111111111111111111111".to_string(),
                keyCount: 2,
                writableKeys: 2,
                signerKeys: 1,
            }]
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RawConfig, normalize_raw_config};

    fn regular_config() -> crate::config::NormalizedConfig {
        let mut raw = RawConfig {
            mode: "regular".to_string(),
            launchpad: "pump".to_string(),
            ..RawConfig::default()
        };
        raw.token.name = "LaunchDeck".to_string();
        raw.token.symbol = "LDECK".to_string();
        raw.token.uri = "ipfs://fixture".to_string();
        normalize_raw_config(raw).expect("normalized config")
    }

    #[test]
    fn supports_plain_regular_pump_launches() {
        let config = regular_config();
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_sol_dev_buy_for_native_pump_path() {
        let mut config = regular_config();
        config.devBuy = Some(crate::config::NormalizedDevBuy {
            mode: "sol".to_string(),
            amount: "0.1".to_string(),
            source: "test".to_string(),
        });
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_agent_unlocked_for_native_pump_path() {
        let mut config = regular_config();
        config.mode = "agent-unlocked".to_string();
        config.agent.buybackBps = Some(2_500);
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_agent_custom_for_native_pump_path() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.feeRecipients = vec![
            crate::config::NormalizedRecipient {
                r#type: Some("agent".to_string()),
                address: String::new(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 2_500,
            },
            crate::config::NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: Pubkey::new_unique().to_string(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 7_500,
            },
        ];
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_agent_locked_for_native_pump_path() {
        let mut config = regular_config();
        config.mode = "agent-locked".to_string();
        config.agent.buybackBps = Some(2_500);
        config.creatorFee.mode = "agent-escrow".to_string();
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn auto_prefers_v0_alt_when_lookup_tables_are_loaded() {
        assert_eq!(
            select_native_format("auto", true).expect("format"),
            NativeTxFormat::V0Alt
        );
        assert_eq!(
            select_native_format("auto", false).expect("format"),
            NativeTxFormat::V0
        );
    }

    #[test]
    fn v0_alt_requires_loaded_lookup_tables() {
        assert_eq!(
            select_native_format("v0-alt", true).expect("format"),
            NativeTxFormat::V0Alt
        );
        assert!(select_native_format("v0-alt", false).is_err());
    }

    #[test]
    fn supports_wallet_only_deferred_fee_sharing() {
        let mut config = regular_config();
        config.feeSharing.generateLaterSetup = true;
        config.feeSharing.recipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("wallet".to_string()),
            address: Pubkey::new_unique().to_string(),
            githubUserId: String::new(),
            githubUsername: String::new(),
            shareBps: 10_000,
        }];
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_github_creator_fee_routing_with_user_id() {
        let mut config = regular_config();
        config.creatorFee.mode = "github".to_string();
        config.creatorFee.githubUserId = "12345".to_string();
        config.creatorFee.githubUsername = "launchdeck".to_string();
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn supports_social_fee_sharing_follow_up_when_user_ids_are_present() {
        let mut config = regular_config();
        config.feeSharing.generateLaterSetup = true;
        config.feeSharing.recipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("github".to_string()),
            address: String::new(),
            githubUserId: "12345".to_string(),
            githubUsername: "launchdeck".to_string(),
            shareBps: 10_000,
        }];
        assert!(supports_native_pump_compile(&config));
    }

    #[test]
    fn still_rejects_social_fee_sharing_without_user_id() {
        let mut config = regular_config();
        config.feeSharing.generateLaterSetup = true;
        config.feeSharing.recipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("github".to_string()),
            address: String::new(),
            githubUserId: String::new(),
            githubUsername: "launchdeck".to_string(),
            shareBps: 10_000,
        }];
        assert!(!supports_native_pump_compile(&config));
    }

    #[test]
    fn still_rejects_agent_custom_social_recipient_without_user_id() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.feeRecipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("github".to_string()),
            address: String::new(),
            githubUserId: String::new(),
            githubUsername: "launchdeck".to_string(),
            shareBps: 10_000,
        }];
        assert!(!supports_native_pump_compile(&config));
    }

    #[test]
    fn create_v2_instruction_shape_matches_expected_accounts() {
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let instruction = build_create_v2_instruction(
            &mint,
            &user,
            &creator,
            "LaunchDeck",
            "LDECK",
            "ipfs://fixture",
            false,
            false,
        )
        .expect("instruction");

        assert_eq!(instruction.program_id.to_string(), PUMP_PROGRAM_ID);
        assert_eq!(instruction.accounts.len(), 16);
        assert_eq!(
            &instruction.data[..8],
            &[214, 144, 76, 236, 95, 139, 49, 180]
        );
    }

    #[test]
    fn buy_instruction_shape_matches_expected_accounts() {
        let global = PumpGlobalState {
            fee_recipient: Pubkey::new_unique(),
            initial_virtual_token_reserves: 1_073_000_000_000_000,
            initial_virtual_sol_reserves: 30_000_000_000,
            initial_real_token_reserves: 793_100_000_000_000,
            fee_basis_points: 100,
            creator_fee_basis_points: 50,
            fee_recipients: [Pubkey::default(); 7],
            reserved_fee_recipient: Pubkey::default(),
            reserved_fee_recipients: [Pubkey::default(); 7],
        };
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let instruction = build_buy_instruction(
            &global,
            &mint,
            &creator,
            &user,
            100_000_000,
            1_000_000,
            false,
        )
        .expect("buy instruction");

        assert_eq!(instruction.program_id.to_string(), PUMP_PROGRAM_ID);
        assert_eq!(instruction.accounts.len(), 17);
        assert_eq!(&instruction.data[..8], &[102, 6, 61, 18, 1, 218, 235, 234]);
    }

    #[test]
    fn compute_budget_instruction_layout_matches_web3() {
        let limit = build_compute_unit_limit_instruction().expect("limit instruction");
        let price = build_compute_unit_price_instruction(123_456).expect("price instruction");

        assert_eq!(limit.program_id.to_string(), COMPUTE_BUDGET_PROGRAM_ID);
        assert_eq!(limit.data, [2, 64, 66, 15, 0]);
        assert_eq!(price.program_id.to_string(), COMPUTE_BUDGET_PROGRAM_ID);
        assert_eq!(price.data, [3, 64, 226, 1, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn social_fee_pda_instruction_shape_matches_expected_accounts() {
        let payer = Pubkey::new_unique();
        let instruction = build_create_social_fee_pda_instruction(&payer, "12345", PLATFORM_GITHUB)
            .expect("social fee instruction");

        assert_eq!(instruction.program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instruction.accounts.len(), 6);
        assert_eq!(
            &instruction.data[..8],
            &[144, 224, 59, 211, 78, 248, 202, 220]
        );
    }

    #[test]
    fn agent_initialize_instruction_shape_matches_sdk_layout() {
        let mint = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();
        let instruction =
            build_agent_initialize_instruction(&mint, &creator, &agent_authority, 2_500)
                .expect("agent initialize instruction");

        assert_eq!(
            instruction.program_id.to_string(),
            PUMP_AGENT_PAYMENTS_PROGRAM_ID
        );
        assert_eq!(instruction.accounts.len(), 8);
        assert_eq!(&instruction.data[..8], &[180, 248, 163, 8, 49, 94, 126, 96]);
        assert_eq!(&instruction.data[8..40], agent_authority.as_ref(),);
        assert_eq!(&instruction.data[40..42], &2_500u16.to_le_bytes());
    }

    #[tokio::test]
    async fn fee_sharing_follow_up_contains_three_instructions() {
        let mut config = regular_config();
        config.feeSharing.generateLaterSetup = true;
        config.feeSharing.recipients = vec![crate::config::NormalizedRecipient {
            r#type: Some("wallet".to_string()),
            address: Pubkey::new_unique().to_string(),
            githubUserId: String::new(),
            githubUsername: String::new(),
            shareBps: 10_000,
        }];
        let instructions = build_fee_sharing_follow_up_instructions(
            "http://127.0.0.1:8899",
            &config,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        )
        .await
        .expect("follow-up instructions");

        assert_eq!(instructions.len(), 3);
        assert_eq!(instructions[0].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instructions[1].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instructions[2].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
    }

    #[tokio::test]
    async fn agent_locked_follow_up_contains_agent_init_and_fee_setup() {
        let mut config = regular_config();
        config.mode = "agent-locked".to_string();
        config.agent.buybackBps = Some(2_500);
        config.creatorFee.mode = "agent-escrow".to_string();
        let creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();
        let instructions = build_native_follow_up_instructions(
            "http://127.0.0.1:8899",
            &config,
            Pubkey::new_unique(),
            creator,
            Some(&agent_authority),
        )
        .await
        .expect("agent locked follow-up instructions");

        assert_eq!(instructions.len(), 4);
        assert_eq!(
            instructions[0].program_id.to_string(),
            PUMP_AGENT_PAYMENTS_PROGRAM_ID
        );
        assert_eq!(instructions[1].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instructions[2].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instructions[3].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
    }

    #[tokio::test]
    async fn agent_custom_follow_up_contains_agent_init_and_fee_setup_without_revoke() {
        let mut config = regular_config();
        config.mode = "agent-custom".to_string();
        config.agent.buybackBps = Some(2_500);
        config.agent.feeRecipients = vec![
            crate::config::NormalizedRecipient {
                r#type: Some("agent".to_string()),
                address: String::new(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 2_500,
            },
            crate::config::NormalizedRecipient {
                r#type: Some("wallet".to_string()),
                address: Pubkey::new_unique().to_string(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: 7_500,
            },
        ];
        let creator = Pubkey::new_unique();
        let agent_authority = Pubkey::new_unique();
        let instructions = build_native_follow_up_instructions(
            "http://127.0.0.1:8899",
            &config,
            Pubkey::new_unique(),
            creator,
            Some(&agent_authority),
        )
        .await
        .expect("agent custom follow-up instructions");

        assert_eq!(instructions.len(), 3);
        assert_eq!(
            instructions[0].program_id.to_string(),
            PUMP_AGENT_PAYMENTS_PROGRAM_ID
        );
        assert_eq!(instructions[1].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
        assert_eq!(instructions[2].program_id.to_string(), PUMP_FEE_PROGRAM_ID);
    }
}
