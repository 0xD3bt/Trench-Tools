use std::str::FromStr;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::json;
use shared_transaction_submit::{compiled_transaction_signers, fetch_multiple_account_data};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::{VersionedMessage, v0},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use solana_system_interface::instruction::{create_account, transfer};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use spl_token::instruction::{
    close_account as close_spl_account, initialize_account3, sync_native,
};

use crate::{
    extension_api::{MevMode, TradeSettlementAsset, TradeSide},
    provider_tip::pick_tip_account_for_provider,
    rpc_client::{
        CompiledTransaction, fetch_account_data, fetch_account_owner_and_data,
        fetch_minimum_balance_for_rent_exemption,
    },
    trade_dispatch::{CompiledAdapterTrade, TransactionDependencyMode},
    trade_planner::{
        LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerRuntimeBundle,
        PlannerVerificationSource, RaydiumCpmmRuntimeBundle, TradeLifecycle, TradeVenueFamily,
        WrapperAction,
    },
    trade_runtime::{RuntimeSellIntent, TradeRuntimeRequest},
    wallet_store::load_solana_wallet_by_env_key,
    warming_service::shared_warming_service,
};

const RAYDIUM_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
const RAYDIUM_CPMM_AUTH_SEED: &[u8] = b"vault_and_lp_mint_auth_seed";
const RAYDIUM_CPMM_TOKEN_0_MINT_OFFSET: usize = 168;
const RAYDIUM_CPMM_TOKEN_1_MINT_OFFSET: usize = 200;
const RAYDIUM_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
const RAYDIUM_CPMM_FEE_RATE_DENOMINATOR: u64 = 1_000_000;
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const JITODONTFRONT_ACCOUNT: &str = "jitodontfront111111111111111111111111111111";
const SPL_TOKEN_ACCOUNT_LEN: u64 = 165;
const RAYDIUM_CPMM_BUY_COMPUTE_UNIT_LIMIT: u32 = 340_000;
const RAYDIUM_CPMM_SELL_COMPUTE_UNIT_LIMIT: u32 = 340_000;
const PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT: u64 = 1_000_000;
const HELIUS_SENDER_MIN_TIP_LAMPORTS: u64 = 200_000;
const HELLO_MOON_MIN_TIP_LAMPORTS: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RaydiumCpmmPoolState {
    pub pool_id: Pubkey,
    pub config_id: Pubkey,
    pub vault_a: Pubkey,
    pub vault_b: Pubkey,
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub token_0_program: Pubkey,
    pub token_1_program: Pubkey,
    pub observation_id: Pubkey,
    pub mint_decimals_a: u8,
    pub mint_decimals_b: u8,
    pub protocol_fees_mint_a: u64,
    pub protocol_fees_mint_b: u64,
    pub fund_fees_mint_a: u64,
    pub fund_fees_mint_b: u64,
    pub enable_creator_fee: bool,
    pub creator_fees_mint_a: u64,
    pub creator_fees_mint_b: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RaydiumCpmmConfig {
    pub trade_fee_rate: u64,
    pub creator_fee_rate: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct RaydiumCpmmPoolContext {
    pub pool: RaydiumCpmmPoolState,
    pub config: RaydiumCpmmConfig,
    pub reserve_a: u64,
    pub reserve_b: u64,
    pub quote_mint: Pubkey,
}

#[derive(Debug, Clone)]
pub(crate) struct RaydiumCpmmClassification {
    pub mint: String,
    pub pool_id: String,
    pub quote_asset: PlannerQuoteAsset,
}

pub(crate) fn raydium_cpmm_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(RAYDIUM_CPMM_PROGRAM_ID)
        .map_err(|error| format!("Invalid Raydium CPMM program id: {error}"))
}

fn raydium_cpmm_pool_authority() -> Result<Pubkey, String> {
    let program = raydium_cpmm_program_id()?;
    Ok(Pubkey::find_program_address(&[RAYDIUM_CPMM_AUTH_SEED], &program).0)
}

pub(crate) fn raydium_cpmm_wsol_mint() -> Result<Pubkey, String> {
    Ok(crate::wrapper_abi::WSOL_MINT)
}

pub(crate) fn classify_raydium_cpmm_pool_address(
    address: &str,
    owner: &Pubkey,
    data: &[u8],
) -> Result<Option<RaydiumCpmmClassification>, String> {
    if *owner != raydium_cpmm_program_id()? {
        return Ok(None);
    }
    let pool_id = parse_pubkey(address.trim(), "Raydium CPMM pool id")?;
    let pool = match decode_raydium_cpmm_pool(pool_id, data) {
        Ok(pool) => pool,
        Err(_) => return Ok(None),
    };
    let wsol = raydium_cpmm_wsol_mint()?;
    let mint = if pool.token_0_mint == wsol {
        validate_cpmm_token_programs(&pool, &pool.token_1_mint)?;
        pool.token_1_mint
    } else if pool.token_1_mint == wsol {
        validate_cpmm_token_programs(&pool, &pool.token_0_mint)?;
        pool.token_0_mint
    } else {
        return Ok(None);
    };
    Ok(Some(RaydiumCpmmClassification {
        mint: mint.to_string(),
        pool_id: pool_id.to_string(),
        quote_asset: PlannerQuoteAsset::Wsol,
    }))
}

pub(crate) async fn plan_raydium_cpmm_trade(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let Some(pool_id) = request
        .pinned_pool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let pool_id = parse_pubkey(pool_id, "Raydium CPMM pool id")?;
    plan_raydium_cpmm_trade_for_pool_id(rpc_url, request, &pool_id).await
}

pub(crate) async fn plan_raydium_cpmm_trade_for_pool_id(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    pool_id: &Pubkey,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let (owner, pool_data) =
        fetch_account_owner_and_data(rpc_url, &pool_id.to_string(), &request.policy.commitment)
            .await?
            .ok_or_else(|| format!("Raydium CPMM pool {pool_id} was not found."))?;
    if owner != raydium_cpmm_program_id()? {
        return Ok(None);
    }
    let context = build_raydium_cpmm_pool_context_from_data(
        rpc_url,
        pool_id,
        &raydium_cpmm_wsol_mint()?,
        &request.policy.commitment,
        &pool_data,
    )
    .await?;
    let mint = parse_pubkey(&request.mint, "Raydium CPMM mint")?;
    validate_cpmm_context_for_mint(&context, &mint)?;
    Ok(Some(build_raydium_cpmm_selector(request, &context)))
}

pub(crate) async fn find_raydium_cpmm_pool_for_pair(
    rpc_url: &str,
    mint: &Pubkey,
    quote_mint: &Pubkey,
    commitment: &str,
) -> Result<Option<Pubkey>, String> {
    let mut candidates = Vec::new();
    for (left, right) in [(mint, quote_mint), (quote_mint, mint)] {
        candidates.extend(
            fetch_raydium_cpmm_pools_for_ordered_pair(rpc_url, left, right, commitment).await?,
        );
    }
    candidates.sort_by_key(|pool_id| pool_id.to_string());
    candidates.dedup();
    match candidates.as_slice() {
        [] => Ok(None),
        [pool_id] => Ok(Some(*pool_id)),
        _ => Err(format!(
            "Multiple Raydium CPMM pools matched mint {} and quote {}; refusing ambiguous route.",
            mint, quote_mint
        )),
    }
}

pub(crate) async fn compile_raydium_cpmm_trade(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    if !matches!(selector.family, TradeVenueFamily::RaydiumCpmm) {
        return Err(format!(
            "Raydium CPMM compiler received unsupported family {}.",
            selector.family.label()
        ));
    }
    if matches!(request.side, TradeSide::Sell)
        && matches!(request.sell_intent, Some(RuntimeSellIntent::SolOutput(_)))
    {
        return Err("Raydium CPMM exact-output sells are not enabled for this route.".to_string());
    }

    let rpc_url = crate::rpc_client::configured_rpc_url();
    let owner = load_solana_wallet_by_env_key(wallet_key)?;
    let context = load_raydium_cpmm_pool_context_for_selector(
        &rpc_url,
        selector,
        &raydium_cpmm_wsol_mint()?,
        &request.policy.commitment,
    )
    .await?;
    let mint = parse_pubkey(&request.mint, "Raydium CPMM mint")?;
    validate_cpmm_context_for_mint(&context, &mint)?;

    let transaction = match request.side {
        TradeSide::Buy => compile_cpmm_buy_transaction(&rpc_url, request, &owner, &context).await?,
        TradeSide::Sell => {
            compile_cpmm_sell_transaction(&rpc_url, request, wallet_key, &owner, &context).await?
        }
    };
    Ok(CompiledAdapterTrade {
        transactions: vec![transaction],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: if matches!(request.side, TradeSide::Buy) {
            Some(TradeSettlementAsset::Sol)
        } else {
            None
        },
    })
}

pub(crate) async fn quote_raydium_cpmm_holding_value_sol(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
    commitment: &str,
) -> Result<u64, String> {
    let pool_id = parse_pubkey(&selector.canonical_market_key, "Raydium CPMM selector pool")?;
    let context = load_raydium_cpmm_pool_context_by_pool_id(
        rpc_url,
        &pool_id,
        &raydium_cpmm_wsol_mint()?,
        commitment,
    )
    .await?;
    let mint = parse_pubkey(mint, "Raydium CPMM mint")?;
    validate_cpmm_context_for_mint(&context, &mint)?;
    let (expected_out, _) = raydium_cpmm_quote_exact_input(&context, &mint, token_amount_raw, 0)?;
    Ok(expected_out)
}

pub(crate) fn raydium_cpmm_fee_amount(amount: u64, fee_rate: u64) -> u64 {
    if fee_rate == 0 || amount == 0 {
        return 0;
    }
    let numerator = u128::from(amount).saturating_mul(u128::from(fee_rate))
        + u128::from(RAYDIUM_CPMM_FEE_RATE_DENOMINATOR - 1);
    (numerator / u128::from(RAYDIUM_CPMM_FEE_RATE_DENOMINATOR)) as u64
}

pub(crate) fn net_cpmm_reserve(
    vault_amount: u64,
    protocol_fees: u64,
    fund_fees: u64,
    creator_fees: u64,
) -> u64 {
    vault_amount
        .saturating_sub(protocol_fees)
        .saturating_sub(fund_fees)
        .saturating_sub(creator_fees)
}

pub(crate) fn raydium_cpmm_quote_exact_input(
    pool_context: &RaydiumCpmmPoolContext,
    input_mint: &Pubkey,
    amount_in: u64,
    slippage_bps: u64,
) -> Result<(u64, u64), String> {
    raydium_cpmm_quote_exact_input_parts(
        input_mint,
        &pool_context.pool.token_0_mint,
        &pool_context.pool.token_1_mint,
        pool_context.reserve_a,
        pool_context.reserve_b,
        pool_context.config.trade_fee_rate,
        pool_context.pool.enable_creator_fee,
        pool_context.config.creator_fee_rate,
        amount_in,
        slippage_bps,
    )
}

pub(crate) fn raydium_cpmm_quote_exact_input_parts(
    input_mint: &Pubkey,
    token_0_mint: &Pubkey,
    token_1_mint: &Pubkey,
    reserve_a: u64,
    reserve_b: u64,
    trade_fee_rate: u64,
    enable_creator_fee: bool,
    creator_fee_rate: u64,
    amount_in: u64,
    slippage_bps: u64,
) -> Result<(u64, u64), String> {
    if amount_in == 0 {
        return Ok((0, 0));
    }
    let (input_reserve, output_reserve) = if *input_mint == *token_0_mint {
        (reserve_a, reserve_b)
    } else if *input_mint == *token_1_mint {
        (reserve_b, reserve_a)
    } else {
        return Err("Raydium CPMM quote input mint does not match the selected pool.".to_string());
    };
    if input_reserve == 0 || output_reserve == 0 {
        return Err("Raydium CPMM pool had zero reserves.".to_string());
    }
    let trade_fee = raydium_cpmm_fee_amount(amount_in, trade_fee_rate);
    let input_after_trade_fee = amount_in.saturating_sub(trade_fee);
    let output_swapped = (u128::from(input_after_trade_fee) * u128::from(output_reserve))
        / u128::from(input_reserve.saturating_add(input_after_trade_fee));
    let creator_fee = if enable_creator_fee {
        raydium_cpmm_fee_amount(
            u64::try_from(output_swapped)
                .map_err(|error| format!("Raydium CPMM output exceeded u64: {error}"))?,
            creator_fee_rate,
        )
    } else {
        0
    };
    let expected_out = u64::try_from(output_swapped)
        .map_err(|error| format!("Raydium CPMM output exceeded u64: {error}"))?
        .saturating_sub(creator_fee);
    let min_out = apply_slippage_bps(expected_out, slippage_bps);
    Ok((expected_out, min_out))
}

pub(crate) fn build_raydium_cpmm_swap_exact_in_instruction(
    owner: &Pubkey,
    context: &RaydiumCpmmPoolContext,
    user_input_account: &Pubkey,
    user_output_account: &Pubkey,
    amount_in: u64,
    min_out: u64,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
) -> Result<Instruction, String> {
    build_raydium_cpmm_swap_exact_in_instruction_parts(
        owner,
        &context.pool.pool_id,
        &context.pool.config_id,
        &context.pool.vault_a,
        &context.pool.vault_b,
        &context.pool.token_0_mint,
        &context.pool.token_1_mint,
        &context.pool.token_0_program,
        &context.pool.token_1_program,
        &context.pool.observation_id,
        user_input_account,
        user_output_account,
        amount_in,
        min_out,
        input_mint,
        output_mint,
    )
}

pub(crate) fn build_raydium_cpmm_swap_exact_in_instruction_parts(
    owner: &Pubkey,
    pool_id: &Pubkey,
    config_id: &Pubkey,
    vault_a: &Pubkey,
    vault_b: &Pubkey,
    token_0_mint: &Pubkey,
    token_1_mint: &Pubkey,
    token_0_program: &Pubkey,
    token_1_program: &Pubkey,
    observation_id: &Pubkey,
    user_input_account: &Pubkey,
    user_output_account: &Pubkey,
    amount_in: u64,
    min_out: u64,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
) -> Result<Instruction, String> {
    let authority = raydium_cpmm_pool_authority()?;
    let input_is_a = *input_mint == *token_0_mint && *output_mint == *token_1_mint;
    let input_is_b = *input_mint == *token_1_mint && *output_mint == *token_0_mint;
    if !input_is_a && !input_is_b {
        return Err(
            "Raydium CPMM swap input/output mints do not match the selected pool.".to_string(),
        );
    }
    let (input_vault, output_vault, input_token_program, output_token_program) = if input_is_a {
        (vault_a, vault_b, token_0_program, token_1_program)
    } else {
        (vault_b, vault_a, token_1_program, token_0_program)
    };
    let mut data = Vec::with_capacity(24);
    data.extend_from_slice(&RAYDIUM_CPMM_SWAP_BASE_INPUT_DISCRIMINATOR);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_out.to_le_bytes());
    Ok(Instruction {
        program_id: raydium_cpmm_program_id()?,
        accounts: vec![
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(authority, false),
            AccountMeta::new_readonly(*config_id, false),
            AccountMeta::new(*pool_id, false),
            AccountMeta::new(*user_input_account, false),
            AccountMeta::new(*user_output_account, false),
            AccountMeta::new(*input_vault, false),
            AccountMeta::new(*output_vault, false),
            AccountMeta::new_readonly(*input_token_program, false),
            AccountMeta::new_readonly(*output_token_program, false),
            AccountMeta::new_readonly(*input_mint, false),
            AccountMeta::new_readonly(*output_mint, false),
            AccountMeta::new(*observation_id, false),
        ],
        data,
    })
}

async fn compile_cpmm_buy_transaction(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    owner: &Keypair,
    context: &RaydiumCpmmPoolContext,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let quote_mint = context.quote_mint;
    let (token_mint, token_program, quote_program) = cpmm_token_and_quote_programs(context)?;
    let amount_in = parse_decimal_units(
        request
            .buy_amount_sol
            .as_deref()
            .ok_or_else(|| "Missing buyAmountSol for Raydium CPMM buy.".to_string())?,
        9,
        "buyAmountSol",
    )?;
    if amount_in == 0 {
        return Err("Buy amount must be greater than zero.".to_string());
    }
    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let (_, min_out) =
        raydium_cpmm_quote_exact_input(context, &quote_mint, amount_in, slippage_bps)?;
    if min_out == 0 {
        return Err("Raydium CPMM buy quote resolved to zero tokens.".to_string());
    }
    let user_token_account =
        get_associated_token_address_with_program_id(&owner_pubkey, &token_mint, &token_program);
    let temp_wsol_account = Keypair::new();
    let temp_wsol_account_pubkey = temp_wsol_account.pubkey();
    let rent_lamports = shared_warming_service()
        .minimum_balance_for_rent_exemption(SPL_TOKEN_ACCOUNT_LEN, || async {
            fetch_minimum_balance_for_rent_exemption(
                rpc_url,
                &request.policy.commitment,
                SPL_TOKEN_ACCOUNT_LEN,
            )
            .await
        })
        .await?;

    let mut instructions =
        raydium_cpmm_prefix_instructions(request, RAYDIUM_CPMM_BUY_COMPUTE_UNIT_LIMIT)?;
    instructions.push(create_associated_token_account_idempotent(
        &owner_pubkey,
        &owner_pubkey,
        &token_mint,
        &token_program,
    ));
    instructions.extend(build_wrapped_sol_open_instructions(
        &owner_pubkey,
        &temp_wsol_account_pubkey,
        rent_lamports
            .checked_add(amount_in)
            .ok_or_else(|| "Raydium CPMM WSOL funding overflowed.".to_string())?,
        &quote_program,
    )?);
    instructions.push(build_raydium_cpmm_swap_exact_in_instruction(
        &owner_pubkey,
        context,
        &temp_wsol_account_pubkey,
        &user_token_account,
        amount_in,
        min_out,
        &quote_mint,
        &token_mint,
    )?);
    instructions.push(build_wrapped_sol_close_instruction(
        &quote_program,
        &owner_pubkey,
        &temp_wsol_account_pubkey,
    )?);
    finalize_raydium_cpmm_transaction(
        rpc_url,
        request,
        owner,
        &[&temp_wsol_account],
        instructions,
        "raydium-cpmm-buy",
        RAYDIUM_CPMM_BUY_COMPUTE_UNIT_LIMIT,
    )
    .await
}

async fn compile_cpmm_sell_transaction(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
    owner: &Keypair,
    context: &RaydiumCpmmPoolContext,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let quote_mint = context.quote_mint;
    let (token_mint, token_program, quote_program) = cpmm_token_and_quote_programs(context)?;
    let sell_intent = request
        .sell_intent
        .as_ref()
        .ok_or_else(|| "Missing sell intent for Raydium CPMM sell.".to_string())?;
    let percent_bps = match sell_intent {
        RuntimeSellIntent::Percent(value) => parse_percent_to_bps(value)?,
        RuntimeSellIntent::SolOutput(_) => {
            return Err(
                "Raydium CPMM exact-output sells are not enabled for this route.".to_string(),
            );
        }
    };
    let balance = crate::wallet_token_cache::fetch_token_balance_with_cache(
        Some(wallet_key),
        &owner_pubkey.to_string(),
        &token_mint.to_string(),
        token_decimals(context, &token_mint),
    )
    .await?;
    let amount_in = ((u128::from(balance.amount_raw) * u128::from(percent_bps)) / 10_000u128)
        .min(u128::from(u64::MAX)) as u64;
    if amount_in == 0 {
        return Err("Raydium CPMM sell amount resolved to zero.".to_string());
    }
    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let (_, min_out) =
        raydium_cpmm_quote_exact_input(context, &token_mint, amount_in, slippage_bps)?;
    if min_out == 0 {
        return Err("Raydium CPMM sell quote resolved to zero SOL.".to_string());
    }
    let user_token_account =
        get_associated_token_address_with_program_id(&owner_pubkey, &token_mint, &token_program);
    let temp_wsol_account = Keypair::new();
    let temp_wsol_account_pubkey = temp_wsol_account.pubkey();
    let rent_lamports = shared_warming_service()
        .minimum_balance_for_rent_exemption(SPL_TOKEN_ACCOUNT_LEN, || async {
            fetch_minimum_balance_for_rent_exemption(
                rpc_url,
                &request.policy.commitment,
                SPL_TOKEN_ACCOUNT_LEN,
            )
            .await
        })
        .await?;

    let mut instructions =
        raydium_cpmm_prefix_instructions(request, RAYDIUM_CPMM_SELL_COMPUTE_UNIT_LIMIT)?;
    instructions.extend(build_wrapped_sol_open_instructions(
        &owner_pubkey,
        &temp_wsol_account_pubkey,
        rent_lamports,
        &quote_program,
    )?);
    instructions.push(build_raydium_cpmm_swap_exact_in_instruction(
        &owner_pubkey,
        context,
        &user_token_account,
        &temp_wsol_account_pubkey,
        amount_in,
        min_out,
        &token_mint,
        &quote_mint,
    )?);
    instructions.push(build_wrapped_sol_close_instruction(
        &quote_program,
        &owner_pubkey,
        &temp_wsol_account_pubkey,
    )?);
    finalize_raydium_cpmm_transaction(
        rpc_url,
        request,
        owner,
        &[&temp_wsol_account],
        instructions,
        "raydium-cpmm-sell",
        RAYDIUM_CPMM_SELL_COMPUTE_UNIT_LIMIT,
    )
    .await
}

fn cpmm_token_and_quote_programs(
    context: &RaydiumCpmmPoolContext,
) -> Result<(Pubkey, Pubkey, Pubkey), String> {
    if context.pool.token_0_mint == context.quote_mint {
        Ok((
            context.pool.token_1_mint,
            context.pool.token_1_program,
            context.pool.token_0_program,
        ))
    } else if context.pool.token_1_mint == context.quote_mint {
        Ok((
            context.pool.token_0_mint,
            context.pool.token_0_program,
            context.pool.token_1_program,
        ))
    } else {
        Err("Raydium CPMM quote mint did not match the selected pool.".to_string())
    }
}

fn token_decimals(context: &RaydiumCpmmPoolContext, mint: &Pubkey) -> u8 {
    if *mint == context.pool.token_0_mint {
        context.pool.mint_decimals_a
    } else {
        context.pool.mint_decimals_b
    }
}

fn build_raydium_cpmm_selector(
    request: &TradeRuntimeRequest,
    context: &RaydiumCpmmPoolContext,
) -> LifecycleAndCanonicalMarket {
    LifecycleAndCanonicalMarket {
        lifecycle: TradeLifecycle::PostMigration,
        family: TradeVenueFamily::RaydiumCpmm,
        canonical_market_key: context.pool.pool_id.to_string(),
        quote_asset: PlannerQuoteAsset::Wsol,
        verification_source: PlannerVerificationSource::OnchainDerived,
        wrapper_action: match request.side {
            TradeSide::Buy => WrapperAction::RaydiumCpmmWsolBuy,
            TradeSide::Sell => WrapperAction::RaydiumCpmmWsolSell,
        },
        wrapper_accounts: vec![
            context.pool.pool_id.to_string(),
            context.pool.vault_a.to_string(),
            context.pool.vault_b.to_string(),
            context.pool.observation_id.to_string(),
        ],
        market_subtype: Some("cpmm".to_string()),
        direct_protocol_target: Some("raydium-cpmm".to_string()),
        input_amount_hint: request.buy_amount_sol.clone(),
        minimum_output_hint: None,
        runtime_bundle: Some(PlannerRuntimeBundle::RaydiumCpmm(
            RaydiumCpmmRuntimeBundle {
                pool: context.pool.pool_id.to_string(),
                config_id: context.pool.config_id.to_string(),
                vault_a: context.pool.vault_a.to_string(),
                vault_b: context.pool.vault_b.to_string(),
                token_0_mint: context.pool.token_0_mint.to_string(),
                token_1_mint: context.pool.token_1_mint.to_string(),
                token_0_program: context.pool.token_0_program.to_string(),
                token_1_program: context.pool.token_1_program.to_string(),
                observation_id: context.pool.observation_id.to_string(),
                mint_decimals_a: context.pool.mint_decimals_a,
                mint_decimals_b: context.pool.mint_decimals_b,
                protocol_fees_mint_a: context.pool.protocol_fees_mint_a,
                protocol_fees_mint_b: context.pool.protocol_fees_mint_b,
                fund_fees_mint_a: context.pool.fund_fees_mint_a,
                fund_fees_mint_b: context.pool.fund_fees_mint_b,
                enable_creator_fee: context.pool.enable_creator_fee,
                creator_fees_mint_a: context.pool.creator_fees_mint_a,
                creator_fees_mint_b: context.pool.creator_fees_mint_b,
                reserve_a: context.reserve_a,
                reserve_b: context.reserve_b,
                trade_fee_rate: context.config.trade_fee_rate,
                creator_fee_rate: context.config.creator_fee_rate,
            },
        )),
    }
}

fn validate_cpmm_context_for_mint(
    context: &RaydiumCpmmPoolContext,
    mint: &Pubkey,
) -> Result<(), String> {
    if context.quote_mint != raydium_cpmm_wsol_mint()? {
        return Err("Raydium CPMM route currently supports WSOL quote only.".to_string());
    }
    if context.pool.token_0_mint != *mint && context.pool.token_1_mint != *mint {
        return Err(format!(
            "Raydium CPMM pool {} does not contain requested mint {}.",
            context.pool.pool_id, mint
        ));
    }
    if context.pool.token_0_mint != context.quote_mint
        && context.pool.token_1_mint != context.quote_mint
    {
        return Err(format!(
            "Raydium CPMM pool {} is not WSOL quoted.",
            context.pool.pool_id
        ));
    }
    validate_cpmm_token_programs(&context.pool, mint)?;
    Ok(())
}

fn validate_cpmm_token_programs(pool: &RaydiumCpmmPoolState, mint: &Pubkey) -> Result<(), String> {
    let spl_token_program = spl_token::id();
    let wsol = raydium_cpmm_wsol_mint()?;
    let quote_program = if pool.token_0_mint == wsol {
        pool.token_0_program
    } else if pool.token_1_mint == wsol {
        pool.token_1_program
    } else {
        return Err(format!(
            "Raydium CPMM pool {} is not WSOL quoted.",
            pool.pool_id
        ));
    };
    if quote_program != spl_token_program {
        return Err(format!(
            "Raydium CPMM pool {} uses unsupported WSOL token program {}; expected SPL Token.",
            pool.pool_id, quote_program
        ));
    }
    let mint_program = if pool.token_0_mint == *mint {
        pool.token_0_program
    } else if pool.token_1_mint == *mint {
        pool.token_1_program
    } else {
        return Err(format!(
            "Raydium CPMM pool {} does not contain requested mint {}.",
            pool.pool_id, mint
        ));
    };
    if mint_program != spl_token_program {
        return Err(format!(
            "Raydium CPMM pool {} uses unsupported token program {} for mint {}; Token-2022 CPMM routes are not enabled.",
            pool.pool_id, mint_program, mint
        ));
    }
    Ok(())
}

pub(crate) async fn load_raydium_cpmm_pool_context_by_pool_id(
    rpc_url: &str,
    pool_id: &Pubkey,
    quote_mint: &Pubkey,
    commitment: &str,
) -> Result<RaydiumCpmmPoolContext, String> {
    let pool_data = fetch_account_data(rpc_url, &pool_id.to_string(), commitment).await?;
    build_raydium_cpmm_pool_context_from_data(rpc_url, pool_id, quote_mint, commitment, &pool_data)
        .await
}

async fn load_raydium_cpmm_pool_context_for_selector(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    quote_mint: &Pubkey,
    commitment: &str,
) -> Result<RaydiumCpmmPoolContext, String> {
    let Some(PlannerRuntimeBundle::RaydiumCpmm(bundle)) = selector.runtime_bundle.as_ref() else {
        let pool_id = parse_pubkey(&selector.canonical_market_key, "Raydium CPMM selector pool")?;
        return load_raydium_cpmm_pool_context_by_pool_id(
            rpc_url, &pool_id, quote_mint, commitment,
        )
        .await;
    };

    let pool_id = parse_pubkey(&bundle.pool, "Raydium CPMM pool")?;
    let pool_data = fetch_account_data(rpc_url, &bundle.pool, commitment).await?;
    let fresh_pool = decode_raydium_cpmm_pool(pool_id, &pool_data)?;
    let mut pool = cpmm_pool_from_runtime_bundle(bundle)?;
    if fresh_pool.token_0_mint != pool.token_0_mint
        || fresh_pool.token_1_mint != pool.token_1_mint
        || fresh_pool.vault_a != pool.vault_a
        || fresh_pool.vault_b != pool.vault_b
        || fresh_pool.config_id != pool.config_id
        || fresh_pool.token_0_program != pool.token_0_program
        || fresh_pool.token_1_program != pool.token_1_program
        || fresh_pool.observation_id != pool.observation_id
    {
        return Err(format!(
            "Raydium CPMM runtime bundle for pool {} no longer matches live pool metadata.",
            pool_id
        ));
    }
    pool.protocol_fees_mint_a = fresh_pool.protocol_fees_mint_a;
    pool.protocol_fees_mint_b = fresh_pool.protocol_fees_mint_b;
    pool.fund_fees_mint_a = fresh_pool.fund_fees_mint_a;
    pool.fund_fees_mint_b = fresh_pool.fund_fees_mint_b;
    pool.enable_creator_fee = fresh_pool.enable_creator_fee;
    pool.creator_fees_mint_a = fresh_pool.creator_fees_mint_a;
    pool.creator_fees_mint_b = fresh_pool.creator_fees_mint_b;
    build_raydium_cpmm_pool_context_from_parts(
        rpc_url,
        pool,
        RaydiumCpmmConfig {
            trade_fee_rate: bundle.trade_fee_rate,
            creator_fee_rate: bundle.creator_fee_rate,
        },
        quote_mint,
        commitment,
    )
    .await
}

fn cpmm_pool_from_runtime_bundle(
    bundle: &RaydiumCpmmRuntimeBundle,
) -> Result<RaydiumCpmmPoolState, String> {
    Ok(RaydiumCpmmPoolState {
        pool_id: parse_pubkey(&bundle.pool, "Raydium CPMM pool")?,
        config_id: parse_pubkey(&bundle.config_id, "Raydium CPMM config")?,
        vault_a: parse_pubkey(&bundle.vault_a, "Raydium CPMM vault A")?,
        vault_b: parse_pubkey(&bundle.vault_b, "Raydium CPMM vault B")?,
        token_0_mint: parse_pubkey(&bundle.token_0_mint, "Raydium CPMM token 0 mint")?,
        token_1_mint: parse_pubkey(&bundle.token_1_mint, "Raydium CPMM token 1 mint")?,
        token_0_program: parse_pubkey(&bundle.token_0_program, "Raydium CPMM token 0 program")?,
        token_1_program: parse_pubkey(&bundle.token_1_program, "Raydium CPMM token 1 program")?,
        observation_id: parse_pubkey(&bundle.observation_id, "Raydium CPMM observation")?,
        mint_decimals_a: bundle.mint_decimals_a,
        mint_decimals_b: bundle.mint_decimals_b,
        protocol_fees_mint_a: bundle.protocol_fees_mint_a,
        protocol_fees_mint_b: bundle.protocol_fees_mint_b,
        fund_fees_mint_a: bundle.fund_fees_mint_a,
        fund_fees_mint_b: bundle.fund_fees_mint_b,
        enable_creator_fee: bundle.enable_creator_fee,
        creator_fees_mint_a: bundle.creator_fees_mint_a,
        creator_fees_mint_b: bundle.creator_fees_mint_b,
    })
}

pub(crate) async fn build_raydium_cpmm_pool_context_from_data(
    rpc_url: &str,
    pool_id: &Pubkey,
    quote_mint: &Pubkey,
    commitment: &str,
    pool_data: &[u8],
) -> Result<RaydiumCpmmPoolContext, String> {
    let pool = decode_raydium_cpmm_pool(*pool_id, pool_data)?;
    if pool.token_0_mint != *quote_mint && pool.token_1_mint != *quote_mint {
        return Err(format!(
            "Raydium CPMM pool {} does not contain quote mint {}.",
            pool.pool_id, quote_mint
        ));
    }
    let config_id = pool.config_id.to_string();
    let config_data = fetch_account_data(rpc_url, &config_id, commitment).await?;
    let config = decode_raydium_cpmm_config(&config_data)?;
    build_raydium_cpmm_pool_context_from_parts(rpc_url, pool, config, quote_mint, commitment).await
}

async fn build_raydium_cpmm_pool_context_from_parts(
    rpc_url: &str,
    pool: RaydiumCpmmPoolState,
    config: RaydiumCpmmConfig,
    quote_mint: &Pubkey,
    commitment: &str,
) -> Result<RaydiumCpmmPoolContext, String> {
    if pool.token_0_mint != *quote_mint && pool.token_1_mint != *quote_mint {
        return Err(format!(
            "Raydium CPMM pool {} does not contain quote mint {}.",
            pool.pool_id, quote_mint
        ));
    }
    let vault_accounts = vec![pool.vault_a.to_string(), pool.vault_b.to_string()];
    let vault_datas = fetch_multiple_account_data(rpc_url, &vault_accounts, commitment).await?;
    if vault_datas.len() != 2 {
        return Err(
            "Raydium CPMM vault lookup returned an unexpected number of accounts.".to_string(),
        );
    }
    let vault_a_data = vault_datas
        .first()
        .and_then(|value| value.as_ref())
        .ok_or_else(|| "Raydium CPMM vault A account was missing.".to_string())?;
    let vault_b_data = vault_datas
        .get(1)
        .and_then(|value| value.as_ref())
        .ok_or_else(|| "Raydium CPMM vault B account was missing.".to_string())?;
    let creator_fees_a = if pool.enable_creator_fee {
        pool.creator_fees_mint_a
    } else {
        0
    };
    let creator_fees_b = if pool.enable_creator_fee {
        pool.creator_fees_mint_b
    } else {
        0
    };
    Ok(RaydiumCpmmPoolContext {
        reserve_a: net_cpmm_reserve(
            read_token_account_amount(vault_a_data)?,
            pool.protocol_fees_mint_a,
            pool.fund_fees_mint_a,
            creator_fees_a,
        ),
        reserve_b: net_cpmm_reserve(
            read_token_account_amount(vault_b_data)?,
            pool.protocol_fees_mint_b,
            pool.fund_fees_mint_b,
            creator_fees_b,
        ),
        pool,
        config,
        quote_mint: *quote_mint,
    })
}

pub(crate) fn decode_raydium_cpmm_pool(
    pool_id: Pubkey,
    data: &[u8],
) -> Result<RaydiumCpmmPoolState, String> {
    let mut offset = 0usize;
    offset += 8;
    let config_id = read_pubkey(data, &mut offset)?;
    let _pool_creator = read_pubkey(data, &mut offset)?;
    let vault_a = read_pubkey(data, &mut offset)?;
    let vault_b = read_pubkey(data, &mut offset)?;
    let _lp_mint = read_pubkey(data, &mut offset)?;
    let token_0_mint = read_pubkey(data, &mut offset)?;
    let token_1_mint = read_pubkey(data, &mut offset)?;
    let token_0_program = read_pubkey(data, &mut offset)?;
    let token_1_program = read_pubkey(data, &mut offset)?;
    let observation_id = read_pubkey(data, &mut offset)?;
    let _bump = read_u8(data, &mut offset)?;
    let _status = read_u8(data, &mut offset)?;
    let _lp_decimals = read_u8(data, &mut offset)?;
    let mint_decimals_a = read_u8(data, &mut offset)?;
    let mint_decimals_b = read_u8(data, &mut offset)?;
    let _lp_amount = read_u64(data, &mut offset)?;
    let protocol_fees_mint_a = read_u64(data, &mut offset)?;
    let protocol_fees_mint_b = read_u64(data, &mut offset)?;
    let fund_fees_mint_a = read_u64(data, &mut offset)?;
    let fund_fees_mint_b = read_u64(data, &mut offset)?;
    let _open_time = read_u64(data, &mut offset)?;
    let _epoch = read_u64(data, &mut offset)?;
    let _fee_on = read_u8(data, &mut offset)?;
    let enable_creator_fee = read_bool(data, &mut offset)?;
    offset += 6;
    let creator_fees_mint_a = read_u64(data, &mut offset)?;
    let creator_fees_mint_b = read_u64(data, &mut offset)?;
    Ok(RaydiumCpmmPoolState {
        pool_id,
        config_id,
        vault_a,
        vault_b,
        token_0_mint,
        token_1_mint,
        token_0_program,
        token_1_program,
        observation_id,
        mint_decimals_a,
        mint_decimals_b,
        protocol_fees_mint_a,
        protocol_fees_mint_b,
        fund_fees_mint_a,
        fund_fees_mint_b,
        enable_creator_fee,
        creator_fees_mint_a,
        creator_fees_mint_b,
    })
}

pub(crate) fn decode_raydium_cpmm_config(data: &[u8]) -> Result<RaydiumCpmmConfig, String> {
    let mut offset = 0usize;
    offset += 8;
    let _bump = read_u8(data, &mut offset)?;
    let _disable_create_pool = read_bool(data, &mut offset)?;
    let _index = read_u16(data, &mut offset)?;
    let trade_fee_rate = read_u64(data, &mut offset)?;
    let _protocol_fee_rate = read_u64(data, &mut offset)?;
    let _fund_fee_rate = read_u64(data, &mut offset)?;
    let _create_pool_fee = read_u64(data, &mut offset)?;
    let _protocol_owner = read_pubkey(data, &mut offset)?;
    let _fund_owner = read_pubkey(data, &mut offset)?;
    let creator_fee_rate = read_u64(data, &mut offset)?;
    Ok(RaydiumCpmmConfig {
        trade_fee_rate,
        creator_fee_rate,
    })
}

async fn fetch_raydium_cpmm_pools_for_ordered_pair(
    rpc_url: &str,
    left_mint: &Pubkey,
    right_mint: &Pubkey,
    commitment: &str,
) -> Result<Vec<Pubkey>, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": "execution-engine-raydium-cpmm-pools",
        "method": "getProgramAccounts",
        "params": [
            RAYDIUM_CPMM_PROGRAM_ID,
            {
                "commitment": commitment,
                "encoding": "base64",
                "filters": [
                    {
                        "memcmp": {
                            "offset": RAYDIUM_CPMM_TOKEN_0_MINT_OFFSET,
                            "bytes": left_mint.to_string()
                        }
                    },
                    {
                        "memcmp": {
                            "offset": RAYDIUM_CPMM_TOKEN_1_MINT_OFFSET,
                            "bytes": right_mint.to_string()
                        }
                    }
                ]
            }
        ]
    });
    let response = reqwest::Client::new()
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("Failed to fetch Raydium CPMM pools from RPC: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch Raydium CPMM pools from RPC: status {}.",
            response.status()
        ));
    }
    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse Raydium CPMM pool RPC response: {error}"))?;
    if let Some(error) = parsed.get("error") {
        let message = error
            .get("message")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        return Err(format!("Raydium CPMM pool RPC query failed: {message}"));
    }
    let accounts = parsed
        .get("result")
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            "Raydium CPMM pool RPC response did not include result accounts.".to_string()
        })?;
    let mut pools = Vec::with_capacity(accounts.len());
    for account in accounts {
        let Some(pubkey) = account.get("pubkey").and_then(|value| value.as_str()) else {
            continue;
        };
        let Ok(pool_id) = Pubkey::from_str(pubkey) else {
            continue;
        };
        let Some(encoded_data) = account
            .get("account")
            .and_then(|value| value.get("data"))
            .and_then(encoded_account_data)
        else {
            continue;
        };
        let Ok(data) = BASE64.decode(encoded_data.trim()) else {
            continue;
        };
        if decode_raydium_cpmm_pool(pool_id, &data).is_ok() {
            pools.push(pool_id);
        }
    }
    Ok(pools)
}

fn encoded_account_data(value: &serde_json::Value) -> Option<&str> {
    value.as_str().or_else(|| {
        value
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.as_str())
    })
}

fn raydium_cpmm_prefix_instructions(
    request: &TradeRuntimeRequest,
    compute_unit_limit: u32,
) -> Result<Vec<Instruction>, String> {
    let mut instructions = vec![build_compute_unit_limit_instruction(compute_unit_limit)?];
    let compute_unit_price_micro_lamports =
        priority_fee_sol_to_micro_lamports(&request.policy.fee_sol)?;
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }
    Ok(instructions)
}

async fn finalize_raydium_cpmm_transaction(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    owner: &Keypair,
    extra_signers: &[&Keypair],
    mut instructions: Vec<Instruction>,
    label: &str,
    compute_unit_limit: u32,
) -> Result<CompiledTransaction, String> {
    let owner_pubkey = owner.pubkey();
    let compute_unit_price_micro_lamports =
        priority_fee_sol_to_micro_lamports(&request.policy.fee_sol)?;
    let (inline_tip_lamports, inline_tip_account) =
        if let Some((tip_instruction, tip_lamports, tip_account)) = resolve_inline_tip(
            &owner_pubkey,
            &request.policy.provider,
            &request.policy.tip_sol,
        )? {
            instructions.push(tip_instruction);
            (Some(tip_lamports), Some(tip_account))
        } else {
            (None, None)
        };
    if matches!(request.policy.mev_mode, MevMode::Reduced | MevMode::Secure) {
        apply_jitodontfront(&mut instructions, &owner_pubkey)?;
    }
    instructions.push(build_uniqueness_memo_instruction(label)?);
    let blockhash = shared_warming_service()
        .latest_blockhash(rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let lookup_tables = crate::pump_native::load_shared_super_lookup_tables(rpc_url).await?;
    let message = v0::Message::try_compile(&owner_pubkey, &instructions, &lookup_tables, blockhash)
        .map_err(|error| format!("Failed to compile Raydium CPMM transaction: {error}"))?;
    let lookup_tables_used = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect::<Vec<_>>();
    let mut signers = Vec::with_capacity(1 + extra_signers.len());
    signers.push(owner);
    signers.extend(extra_signers.iter().copied());
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .map_err(|error| format!("Failed to sign Raydium CPMM transaction: {error}"))?;
    let signature = transaction
        .signatures
        .first()
        .map(|value| value.to_string())
        .ok_or_else(|| "Raydium CPMM transaction did not include a signature.".to_string())?;
    let serialized = bincode::serialize(&transaction)
        .map_err(|error| format!("Failed to serialize Raydium CPMM transaction: {error}"))?;
    let serialized_base64 = BASE64.encode(serialized);
    compiled_transaction_signers::remember_compiled_transaction_signers(
        &serialized_base64,
        extra_signers,
    );
    Ok(CompiledTransaction {
        label: label.to_string(),
        format: "v0-alt".to_string(),
        serialized_base64,
        signature: Some(signature),
        lookup_tables_used,
        compute_unit_limit: Some(u64::from(compute_unit_limit)),
        compute_unit_price_micro_lamports: if compute_unit_price_micro_lamports > 0 {
            Some(compute_unit_price_micro_lamports)
        } else {
            None
        },
        inline_tip_lamports,
        inline_tip_account,
    })
}

fn build_wrapped_sol_open_instructions(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
    lamports: u64,
    token_program: &Pubkey,
) -> Result<Vec<Instruction>, String> {
    Ok(vec![
        create_account(
            owner,
            wrapped_account,
            lamports,
            SPL_TOKEN_ACCOUNT_LEN,
            token_program,
        ),
        initialize_account3(
            token_program,
            wrapped_account,
            &raydium_cpmm_wsol_mint()?,
            owner,
        )
        .map_err(|error| format!("Failed to initialize Raydium CPMM WSOL account: {error}"))?,
        sync_native(token_program, wrapped_account)
            .map_err(|error| format!("Failed to build Raydium CPMM syncNative: {error}"))?,
    ])
}

fn build_wrapped_sol_close_instruction(
    token_program: &Pubkey,
    owner: &Pubkey,
    wrapped_account: &Pubkey,
) -> Result<Instruction, String> {
    close_spl_account(token_program, wrapped_account, owner, owner, &[])
        .map_err(|error| format!("Failed to build Raydium CPMM WSOL close: {error}"))
}

fn build_compute_unit_limit_instruction(compute_unit_limit: u32) -> Result<Instruction, String> {
    let mut data = vec![2];
    data.extend_from_slice(&compute_unit_limit.to_le_bytes());
    Ok(Instruction {
        program_id: parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID, "compute budget program")?,
        accounts: vec![],
        data,
    })
}

fn build_compute_unit_price_instruction(micro_lamports: u64) -> Result<Instruction, String> {
    let mut data = vec![3];
    data.extend_from_slice(&micro_lamports.to_le_bytes());
    Ok(Instruction {
        program_id: parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID, "compute budget program")?,
        accounts: vec![],
        data,
    })
}

fn build_uniqueness_memo_instruction(label: &str) -> Result<Instruction, String> {
    Ok(Instruction {
        program_id: parse_pubkey(MEMO_PROGRAM_ID, "memo program")?,
        accounts: vec![],
        data: format!("tt:{label}:{}", now_unix_ms()).into_bytes(),
    })
}

fn resolve_inline_tip(
    payer: &Pubkey,
    provider: &str,
    tip_sol: &str,
) -> Result<Option<(Instruction, u64, String)>, String> {
    let provider_tip_account_raw = pick_tip_account_for_provider(provider);
    let (tip_account_str, resolved_from_provider) = if provider_tip_account_raw.is_empty() {
        match configured_tip_account()? {
            Some(account) => (account.to_string(), false),
            None => return Ok(None),
        }
    } else {
        (provider_tip_account_raw, true)
    };
    let min_lamports = provider_min_tip_lamports(provider);
    let requested_lamports = parse_decimal_units(tip_sol, 9, "tipSol").unwrap_or(0);
    let tip_lamports = if resolved_from_provider {
        requested_lamports.max(min_lamports)
    } else {
        requested_lamports
    };
    if tip_lamports == 0 {
        return Ok(None);
    }
    let tip_account = parse_pubkey(&tip_account_str, "tip account")?;
    Ok(Some((
        transfer(payer, &tip_account, tip_lamports),
        tip_lamports,
        tip_account.to_string(),
    )))
}

fn apply_jitodontfront(instructions: &mut Vec<Instruction>, payer: &Pubkey) -> Result<(), String> {
    if instructions.iter().any(|instruction| {
        instruction
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == JITODONTFRONT_ACCOUNT)
    }) {
        return Ok(());
    }
    let account = parse_pubkey(JITODONTFRONT_ACCOUNT, "jitodontfront account")?;
    let mut instruction = transfer(payer, payer, 0);
    instruction
        .accounts
        .push(AccountMeta::new_readonly(account, false));
    instructions.insert(0, instruction);
    Ok(())
}

fn configured_tip_account() -> Result<Option<Pubkey>, String> {
    match std::env::var("PLATFORM_TIP_ACCOUNT").ok() {
        Some(value) if !value.trim().is_empty() => {
            parse_pubkey(value.trim(), "PLATFORM_TIP_ACCOUNT").map(Some)
        }
        _ => Ok(None),
    }
}

fn provider_min_tip_lamports(provider: &str) -> u64 {
    let normalized = provider.trim().to_ascii_lowercase();
    if normalized.contains("helius-sender") {
        HELIUS_SENDER_MIN_TIP_LAMPORTS
    } else if normalized.contains("hello-moon") || normalized.contains("hellomoon") {
        HELLO_MOON_MIN_TIP_LAMPORTS
    } else {
        0
    }
}

fn priority_fee_sol_to_micro_lamports(value: &str) -> Result<u64, String> {
    let lamports = parse_decimal_units(value, 9, "priority fee")?;
    lamports
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_div(PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT))
        .ok_or_else(|| "Priority fee conversion overflowed.".to_string())
}

fn parse_slippage_bps(value: Option<&str>) -> Result<u64, String> {
    let raw = value.unwrap_or("5").trim();
    if raw.is_empty() {
        return Ok(500);
    }
    let parsed = raw
        .parse::<f64>()
        .map_err(|error| format!("Invalid slippage percent: {error}"))?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err("Slippage percent must be non-negative.".to_string());
    }
    Ok((parsed * 100.0).round().clamp(0.0, 10_000.0) as u64)
}

fn parse_percent_to_bps(value: &str) -> Result<u64, String> {
    let parsed = value
        .trim()
        .parse::<f64>()
        .map_err(|error| format!("Invalid sell percent: {error}"))?;
    if !parsed.is_finite() || parsed <= 0.0 || parsed > 100.0 {
        return Err("Sell percent must be between 0 and 100.".to_string());
    }
    Ok((parsed * 100.0).round().clamp(1.0, 10_000.0) as u64)
}

fn parse_decimal_units(value: &str, decimals: u32, label: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} is required."));
    }
    let (whole, fraction) = trimmed
        .split_once('.')
        .map(|(whole, fraction)| (whole, fraction))
        .unwrap_or((trimmed, ""));
    let whole_value = whole
        .parse::<u64>()
        .map_err(|error| format!("Invalid {label}: {error}"))?;
    let decimals_usize = decimals as usize;
    if fraction.len() > decimals_usize {
        return Err(format!("{label} has too many decimal places."));
    }
    let mut fractional = fraction.to_string();
    while fractional.len() < decimals_usize {
        fractional.push('0');
    }
    let fractional_value = if fractional.is_empty() {
        0
    } else {
        fractional
            .parse::<u64>()
            .map_err(|error| format!("Invalid {label}: {error}"))?
    };
    let scale = 10u64
        .checked_pow(decimals)
        .ok_or_else(|| format!("{label} decimals overflowed."))?;
    whole_value
        .checked_mul(scale)
        .and_then(|value| value.checked_add(fractional_value))
        .ok_or_else(|| format!("{label} is too large."))
}

fn read_token_account_amount(data: &[u8]) -> Result<u64, String> {
    if data.len() < 72 {
        return Err("Token account data was shorter than expected.".to_string());
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&data[64..72]);
    Ok(u64::from_le_bytes(raw))
}

fn apply_slippage_bps(amount: u64, slippage_bps: u64) -> u64 {
    let keep_bps = 10_000u64.saturating_sub(slippage_bps.min(10_000));
    let minimum =
        ((u128::from(amount) * u128::from(keep_bps)) / 10_000u128).min(u128::from(u64::MAX)) as u64;
    if amount > 0 && minimum == 0 {
        1
    } else {
        minimum
    }
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|error| format!("Invalid {label}: {error}"))
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Result<Pubkey, String> {
    let end = offset.saturating_add(32);
    if end > data.len() {
        return Err("Raydium CPMM account data was shorter than expected.".to_string());
    }
    let pubkey = Pubkey::new_from_array(
        data[*offset..end]
            .try_into()
            .map_err(|_| "Invalid Raydium CPMM pubkey bytes.".to_string())?,
    );
    *offset = end;
    Ok(pubkey)
}

fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    let end = offset.saturating_add(8);
    if end > data.len() {
        return Err("Raydium CPMM account data was shorter than expected.".to_string());
    }
    let value = u64::from_le_bytes(
        data[*offset..end]
            .try_into()
            .map_err(|_| "Invalid Raydium CPMM u64 bytes.".to_string())?,
    );
    *offset = end;
    Ok(value)
}

fn read_u16(data: &[u8], offset: &mut usize) -> Result<u16, String> {
    let end = offset.saturating_add(2);
    if end > data.len() {
        return Err("Raydium CPMM account data was shorter than expected.".to_string());
    }
    let value = u16::from_le_bytes(
        data[*offset..end]
            .try_into()
            .map_err(|_| "Invalid Raydium CPMM u16 bytes.".to_string())?,
    );
    *offset = end;
    Ok(value)
}

fn read_u8(data: &[u8], offset: &mut usize) -> Result<u8, String> {
    let value = *data
        .get(*offset)
        .ok_or_else(|| "Raydium CPMM account data was shorter than expected.".to_string())?;
    *offset += 1;
    Ok(value)
}

fn read_bool(data: &[u8], offset: &mut usize) -> Result<bool, String> {
    match read_u8(data, offset)? {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(format!("Invalid Raydium CPMM bool value {other}.")),
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpmm_fee_uses_ceiling_division() {
        assert_eq!(raydium_cpmm_fee_amount(100, 1), 1);
        assert_eq!(raydium_cpmm_fee_amount(1_000_000, 2_500), 2_500);
    }

    #[test]
    fn cpmm_quote_applies_trade_and_creator_fees() {
        let token_0 = Pubkey::new_unique();
        let token_1 = Pubkey::new_unique();
        let (expected, min_out) = raydium_cpmm_quote_exact_input_parts(
            &token_0, &token_0, &token_1, 1_000_000, 2_000_000, 2_500, true, 1_000, 10_000, 500,
        )
        .expect("quote");
        assert!(expected > min_out);
        assert!(min_out > 0);
    }

    #[test]
    fn cpmm_validation_rejects_unsupported_token_programs() {
        let mint = Pubkey::new_unique();
        let pool = RaydiumCpmmPoolState {
            pool_id: Pubkey::new_unique(),
            config_id: Pubkey::new_unique(),
            vault_a: Pubkey::new_unique(),
            vault_b: Pubkey::new_unique(),
            token_0_mint: raydium_cpmm_wsol_mint().expect("wsol"),
            token_1_mint: mint,
            token_0_program: spl_token::id(),
            token_1_program: Pubkey::new_unique(),
            observation_id: Pubkey::new_unique(),
            mint_decimals_a: 9,
            mint_decimals_b: 6,
            protocol_fees_mint_a: 0,
            protocol_fees_mint_b: 0,
            fund_fees_mint_a: 0,
            fund_fees_mint_b: 0,
            enable_creator_fee: false,
            creator_fees_mint_a: 0,
            creator_fees_mint_b: 0,
        };

        let error = validate_cpmm_token_programs(&pool, &mint).expect_err("unsupported program");
        assert!(error.contains("Token-2022 CPMM routes are not enabled"));
    }
}
