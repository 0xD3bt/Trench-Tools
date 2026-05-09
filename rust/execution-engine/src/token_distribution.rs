use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    str::FromStr,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::{VersionedMessage, v0},
    pubkey::Pubkey,
    signature::Signer,
    transaction::VersionedTransaction,
};
use solana_system_interface::instruction::transfer;
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use spl_token_2022_interface::{
    extension::{PodStateWithExtensions, transfer_hook},
    pod::PodMint,
};

use crate::{
    provider_tip::{pick_tip_account_for_provider, provider_required_tip_lamports},
    rpc_client::{
        CompiledTransaction, configured_rpc_url, confirm_submitted_transactions_for_transport,
        fetch_account_owner_and_data, fetch_latest_blockhash,
        submit_independent_transactions_for_transport,
    },
    transport::{ExecutionTransportConfig, build_transport_plan},
    wallet_store::load_solana_wallet_by_env_key,
};

const TOKEN_ACCOUNT_AMOUNT_OFFSET: usize = 64;
const TOKEN_ACCOUNT_AMOUNT_LEN: usize = 8;
const MINT_DECIMALS_OFFSET: usize = 44;
const SPLIT_SPREAD_BPS: u64 = 1_200;
const SPLIT_WEIGHT_SCALE_BPS: u64 = 10_000;
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const TOKEN_DISTRIBUTION_COMPUTE_UNIT_LIMIT: u32 = 200_000;
const PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT: u64 = 1_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenSplitRequest {
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub preset_id: String,
    pub mint: String,
    #[serde(default)]
    pub wallet_keys: Vec<String>,
    #[serde(default)]
    pub source_wallet_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenConsolidateRequest {
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub preset_id: String,
    pub mint: String,
    pub destination_wallet_key: String,
}

#[derive(Debug, Clone)]
pub struct TokenDistributionExecutionConfig {
    pub commitment: String,
    pub skip_preflight: bool,
    pub track_send_block_height: bool,
    pub provider: String,
    pub endpoint_profile: String,
    pub mev_mode: String,
    pub fee_sol: String,
    pub tip_sol: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenDistributionTransferResult {
    pub source_wallet_key: String,
    pub destination_wallet_key: String,
    pub amount_raw: u64,
    pub amount_ui: f64,
    #[serde(default)]
    pub signature: Option<String>,
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenDistributionResponse {
    pub ok: bool,
    pub action: String,
    pub mint: String,
    pub decimals: u8,
    pub transfer_count: usize,
    pub confirmed_count: usize,
    pub failed_count: usize,
    pub transfers: Vec<TokenDistributionTransferResult>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct MintMetadata {
    mint: Pubkey,
    decimals: u8,
    token_program: Pubkey,
}

#[derive(Debug, Clone)]
struct WalletTokenBalance {
    wallet_key: String,
    owner: Pubkey,
    ata: Pubkey,
    amount_raw: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedTokenTransfer {
    source_wallet_key: String,
    destination_wallet_key: String,
    amount_raw: u64,
}

pub async fn execute_split(
    request: TokenSplitRequest,
    config: TokenDistributionExecutionConfig,
) -> Result<TokenDistributionResponse, String> {
    let mint = request.mint.trim();
    if mint.is_empty() {
        return Err("Token split requires a mint.".to_string());
    }
    let wallet_keys = normalize_wallet_keys(&request.wallet_keys);
    if wallet_keys.len() < 2 {
        return Err("Select at least two wallets to split tokens.".to_string());
    }
    let source_wallet_keys = normalize_wallet_keys(&request.source_wallet_keys);
    if source_wallet_keys.is_empty() {
        return Err("Select at least one wallet that holds this token.".to_string());
    }

    let rpc_url = configured_rpc_url();
    let metadata = fetch_mint_metadata(&rpc_url, mint, &config.commitment).await?;
    let balances =
        fetch_wallet_token_balances(&rpc_url, &metadata, &wallet_keys, &config.commitment).await?;
    let source_set = source_wallet_keys.into_iter().collect::<HashSet<_>>();
    let planned = plan_split_transfers(&balances, &wallet_keys, &source_set, mint)?;
    execute_planned_transfers("split", mint, metadata, planned, config).await
}

pub async fn execute_consolidate(
    request: TokenConsolidateRequest,
    available_wallet_keys: Vec<String>,
    config: TokenDistributionExecutionConfig,
) -> Result<TokenDistributionResponse, String> {
    let mint = request.mint.trim();
    if mint.is_empty() {
        return Err("Token consolidation requires a mint.".to_string());
    }
    let destination_wallet_key = request.destination_wallet_key.trim();
    if destination_wallet_key.is_empty() {
        return Err("Token consolidation requires a destination wallet.".to_string());
    }
    let wallet_keys = normalize_wallet_keys(&available_wallet_keys);
    if !wallet_keys.iter().any(|key| key == destination_wallet_key) {
        return Err("Selected destination wallet is not available.".to_string());
    }

    let rpc_url = configured_rpc_url();
    let metadata = fetch_mint_metadata(&rpc_url, mint, &config.commitment).await?;
    let balances =
        fetch_wallet_token_balances(&rpc_url, &metadata, &wallet_keys, &config.commitment).await?;
    let planned = plan_consolidate_transfers(&balances, destination_wallet_key);
    execute_planned_transfers("consolidate", mint, metadata, planned, config).await
}

fn normalize_wallet_keys(keys: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    keys.iter()
        .map(|key| key.trim())
        .filter(|key| !key.is_empty())
        .filter_map(|key| {
            if seen.insert(key.to_string()) {
                Some(key.to_string())
            } else {
                None
            }
        })
        .collect()
}

async fn fetch_mint_metadata(
    rpc_url: &str,
    mint: &str,
    commitment: &str,
) -> Result<MintMetadata, String> {
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid mint {mint}: {error}"))?;
    let (token_program, mint_data) = fetch_account_owner_and_data(rpc_url, mint, commitment)
        .await?
        .ok_or_else(|| format!("Mint account {mint} was not found."))?;
    if token_program != spl_token::id() && token_program != token_2022_program_id()? {
        return Err(format!(
            "Mint {mint} is not owned by SPL Token or Token-2022: {token_program}"
        ));
    }
    if token_program == token_2022_program_id()? {
        reject_transfer_hook_mint(mint, &mint_data)?;
    }
    if mint_data.len() <= MINT_DECIMALS_OFFSET {
        return Err(format!("Mint account {mint} had invalid decimals data."));
    }
    Ok(MintMetadata {
        mint: mint_pubkey,
        decimals: mint_data[MINT_DECIMALS_OFFSET],
        token_program,
    })
}

fn reject_transfer_hook_mint(mint: &str, mint_data: &[u8]) -> Result<(), String> {
    let mint_state = PodStateWithExtensions::<PodMint>::unpack(mint_data).map_err(|error| {
        format!("Failed to inspect Token-2022 mint extensions for {mint}: {error}")
    })?;
    if let Some(program_id) = transfer_hook::get_program_id(&mint_state) {
        return Err(format!(
            "Token-2022 mint {mint} uses transfer hook program {program_id}; token distribution does not support transfer-hook mints yet."
        ));
    }
    Ok(())
}

async fn fetch_wallet_token_balances(
    rpc_url: &str,
    metadata: &MintMetadata,
    wallet_keys: &[String],
    commitment: &str,
) -> Result<Vec<WalletTokenBalance>, String> {
    let mut balances = Vec::with_capacity(wallet_keys.len());
    for wallet_key in wallet_keys {
        let owner_keypair = load_solana_wallet_by_env_key(wallet_key)?;
        let owner = owner_keypair.pubkey();
        let ata = get_associated_token_address_with_program_id(
            &owner,
            &metadata.mint,
            &metadata.token_program,
        );
        let amount_raw =
            match fetch_account_owner_and_data(rpc_url, &ata.to_string(), commitment).await? {
                Some((owner_program, data)) => {
                    if owner_program != metadata.token_program {
                        return Err(format!(
                            "Token account {ata} owner mismatch: expected {}, got {owner_program}.",
                            metadata.token_program
                        ));
                    }
                    parse_token_account_raw_balance(&data)?
                }
                None => 0,
            };
        balances.push(WalletTokenBalance {
            wallet_key: wallet_key.clone(),
            owner,
            ata,
            amount_raw,
        });
    }
    Ok(balances)
}

fn parse_token_account_raw_balance(data: &[u8]) -> Result<u64, String> {
    let end = TOKEN_ACCOUNT_AMOUNT_OFFSET + TOKEN_ACCOUNT_AMOUNT_LEN;
    if data.len() < end {
        return Err("Token account data was too short to contain a token amount.".to_string());
    }
    let amount_bytes: [u8; TOKEN_ACCOUNT_AMOUNT_LEN] = data[TOKEN_ACCOUNT_AMOUNT_OFFSET..end]
        .try_into()
        .map_err(|_| "Token account amount bytes were malformed.".to_string())?;
    Ok(u64::from_le_bytes(amount_bytes))
}

fn plan_split_transfers(
    balances: &[WalletTokenBalance],
    target_wallet_keys: &[String],
    source_wallet_keys: &HashSet<String>,
    mint: &str,
) -> Result<Vec<PlannedTokenTransfer>, String> {
    let balance_by_key = balances
        .iter()
        .map(|balance| (balance.wallet_key.clone(), balance.amount_raw))
        .collect::<HashMap<_, _>>();
    let target_amounts = split_target_amounts(target_wallet_keys, &balance_by_key, mint)?;
    let mut sources = Vec::<(String, u64)>::new();
    let mut destinations = Vec::<(String, u64)>::new();
    for wallet_key in target_wallet_keys {
        let current = balance_by_key.get(wallet_key).copied().unwrap_or(0);
        let target = target_amounts.get(wallet_key).copied().unwrap_or(0);
        if current > target && source_wallet_keys.contains(wallet_key) {
            sources.push((wallet_key.clone(), current - target));
        } else if target > current {
            destinations.push((wallet_key.clone(), target - current));
        }
    }

    let total_source = sources
        .iter()
        .fold(0u64, |sum, (_, amount)| sum.saturating_add(*amount));
    let total_destination = destinations
        .iter()
        .fold(0u64, |sum, (_, amount)| sum.saturating_add(*amount));
    if total_destination == 0 {
        return Ok(Vec::new());
    }
    if total_source < total_destination {
        return Err(
            "Selected holder wallets do not have enough excess balance to split.".to_string(),
        );
    }

    let mut transfers = Vec::new();
    let mut source_index = 0usize;
    for (destination_wallet_key, mut needed) in destinations {
        while needed > 0 {
            let Some((source_wallet_key, available)) = sources.get_mut(source_index) else {
                return Err("Unable to match split source and destination balances.".to_string());
            };
            if *available == 0 {
                source_index += 1;
                continue;
            }
            let amount = needed.min(*available);
            transfers.push(PlannedTokenTransfer {
                source_wallet_key: source_wallet_key.clone(),
                destination_wallet_key: destination_wallet_key.clone(),
                amount_raw: amount,
            });
            *available -= amount;
            needed -= amount;
            if *available == 0 {
                source_index += 1;
            }
        }
    }
    Ok(transfers)
}

fn split_target_amounts(
    target_wallet_keys: &[String],
    balance_by_key: &HashMap<String, u64>,
    mint: &str,
) -> Result<HashMap<String, u64>, String> {
    let total = target_wallet_keys.iter().fold(0u128, |sum, wallet_key| {
        sum.saturating_add(u128::from(
            balance_by_key.get(wallet_key).copied().unwrap_or(0),
        ))
    });
    if total == 0 {
        return Err("Selected wallets do not hold any tokens to split.".to_string());
    }

    let weights = split_weights(target_wallet_keys, mint);
    let total_weight = weights.iter().fold(0u128, |sum, (_, weight)| {
        sum.saturating_add(u128::from(*weight))
    });
    let mut allocations = Vec::with_capacity(weights.len());
    let mut allocated = 0u128;
    for (wallet_key, weight) in weights {
        let numerator = total.saturating_mul(u128::from(weight));
        let amount = numerator / total_weight;
        let remainder = numerator % total_weight;
        allocated = allocated.saturating_add(amount);
        allocations.push((wallet_key, amount, remainder));
    }
    let mut remaining = total.saturating_sub(allocated);
    allocations.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(&right.0)));
    for (_, amount, _) in &mut allocations {
        if remaining == 0 {
            break;
        }
        *amount = amount.saturating_add(1);
        remaining -= 1;
    }

    let mut out = HashMap::new();
    for (wallet_key, amount, _) in allocations {
        let amount_u64 = u64::try_from(amount)
            .map_err(|_| "Split allocation exceeded supported token amount range.".to_string())?;
        out.insert(wallet_key, amount_u64);
    }
    Ok(out)
}

fn split_weights(target_wallet_keys: &[String], mint: &str) -> Vec<(String, u64)> {
    if target_wallet_keys.len() <= 1 {
        return target_wallet_keys
            .iter()
            .map(|wallet_key| (wallet_key.clone(), SPLIT_WEIGHT_SCALE_BPS))
            .collect();
    }
    let half_spread = SPLIT_SPREAD_BPS / 2;
    let min_weight = SPLIT_WEIGHT_SCALE_BPS.saturating_sub(half_spread);
    let mut ranked = target_wallet_keys
        .iter()
        .map(|wallet_key| {
            let mut hasher = DefaultHasher::new();
            mint.hash(&mut hasher);
            wallet_key.hash(&mut hasher);
            (wallet_key.clone(), hasher.finish())
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    let denominator = u64::try_from(ranked.len().saturating_sub(1))
        .unwrap_or(1)
        .max(1);
    ranked
        .into_iter()
        .enumerate()
        .map(|(index, (wallet_key, _))| {
            let offset =
                SPLIT_SPREAD_BPS.saturating_mul(u64::try_from(index).unwrap_or(0)) / denominator;
            (wallet_key, min_weight.saturating_add(offset))
        })
        .collect()
}

fn plan_consolidate_transfers(
    balances: &[WalletTokenBalance],
    destination_wallet_key: &str,
) -> Vec<PlannedTokenTransfer> {
    balances
        .iter()
        .filter(|balance| balance.wallet_key != destination_wallet_key && balance.amount_raw > 0)
        .map(|balance| PlannedTokenTransfer {
            source_wallet_key: balance.wallet_key.clone(),
            destination_wallet_key: destination_wallet_key.to_string(),
            amount_raw: balance.amount_raw,
        })
        .collect()
}

async fn execute_planned_transfers(
    action: &str,
    mint: &str,
    metadata: MintMetadata,
    planned: Vec<PlannedTokenTransfer>,
    config: TokenDistributionExecutionConfig,
) -> Result<TokenDistributionResponse, String> {
    if planned.is_empty() {
        return Ok(TokenDistributionResponse {
            ok: true,
            action: action.to_string(),
            mint: mint.to_string(),
            decimals: metadata.decimals,
            transfer_count: 0,
            confirmed_count: 0,
            failed_count: 0,
            transfers: Vec::new(),
            warnings: Vec::new(),
        });
    }

    let rpc_url = configured_rpc_url();
    let wallet_keys = planned
        .iter()
        .flat_map(|transfer| {
            [
                transfer.source_wallet_key.clone(),
                transfer.destination_wallet_key.clone(),
            ]
        })
        .collect::<Vec<_>>();
    let balances = fetch_wallet_token_balances(
        &rpc_url,
        &metadata,
        &normalize_wallet_keys(&wallet_keys),
        &config.commitment,
    )
    .await?;
    let wallet_by_key = balances
        .into_iter()
        .map(|balance| (balance.wallet_key.clone(), balance))
        .collect::<HashMap<_, _>>();
    let scale = 10_f64.powi(i32::from(metadata.decimals));
    let mut results = Vec::with_capacity(planned.len());
    let mut confirmed_count = 0usize;
    let mut failed_count = 0usize;
    let mut warnings = config.warnings.clone();
    let provider = supported_distribution_provider(&config.provider)?;
    let compute_unit_price_micro_lamports = priority_fee_sol_to_micro_lamports(&config.fee_sol)?;
    for (index, transfer) in planned.into_iter().enumerate() {
        let (blockhash, _) = fetch_latest_blockhash(&rpc_url, &config.commitment).await?;
        let compiled_transaction = compile_transfer_transaction(
            index,
            &metadata,
            &transfer,
            &wallet_by_key,
            blockhash,
            &provider,
            &config.tip_sol,
            compute_unit_price_micro_lamports,
        )?;
        let transport_plan = build_transport_plan(
            &ExecutionTransportConfig {
                provider: provider.clone(),
                endpoint_profile: config.endpoint_profile.clone(),
                commitment: config.commitment.clone(),
                skip_preflight: config.skip_preflight,
                track_send_block_height: config.track_send_block_height,
                mev_mode: config.mev_mode.clone(),
                mev_protect: !config.mev_mode.trim().eq_ignore_ascii_case("off"),
            },
            1,
        );
        let submit_result = submit_independent_transactions_for_transport(
            &rpc_url,
            &transport_plan,
            std::slice::from_ref(&compiled_transaction),
        )
        .await;
        let (signature, error) = match submit_result {
            Ok((mut submitted, submit_warnings, submit_ms)) => {
                warnings.extend(submit_warnings);
                let confirm_result = confirm_submitted_transactions_for_transport(
                    &rpc_url,
                    &transport_plan,
                    &mut submitted,
                )
                .await;
                match confirm_result {
                    Ok((confirm_warnings, confirm_ms)) => {
                        warnings.extend(confirm_warnings);
                        eprintln!(
                            "[execution-engine][token-distribution] action={action} source={} destination={} submit_ms={submit_ms} confirm_ms={confirm_ms}",
                            transfer.source_wallet_key, transfer.destination_wallet_key
                        );
                        let result = submitted.into_iter().next();
                        let signature = result.as_ref().and_then(|entry| entry.signature.clone());
                        let error = result.and_then(|entry| {
                            if entry.confirmation_status.as_deref() == Some("failed") {
                                entry.error.or_else(|| {
                                    Some("Token transfer failed during confirmation.".to_string())
                                })
                            } else if entry.signature.is_none() {
                                Some("Transport did not return a transfer signature.".to_string())
                            } else {
                                entry.error
                            }
                        });
                        (signature, error)
                    }
                    Err(error) => {
                        let signature = submitted
                            .first()
                            .and_then(|entry| entry.signature.clone())
                            .or_else(|| compiled_transaction.signature.clone());
                        (signature, Some(error))
                    }
                }
            }
            Err(error) => (compiled_transaction.signature.clone(), Some(error)),
        };
        let failed = error.is_some() || signature.is_none();
        if failed {
            failed_count += 1;
        } else {
            confirmed_count += 1;
        }
        results.push(TokenDistributionTransferResult {
            source_wallet_key: transfer.source_wallet_key,
            destination_wallet_key: transfer.destination_wallet_key,
            amount_raw: transfer.amount_raw,
            amount_ui: transfer.amount_raw as f64 / scale,
            signature,
            status: if failed { "failed" } else { "confirmed" }.to_string(),
            error,
        });
    }

    Ok(TokenDistributionResponse {
        ok: failed_count == 0,
        action: action.to_string(),
        mint: mint.to_string(),
        decimals: metadata.decimals,
        transfer_count: results.len(),
        confirmed_count,
        failed_count,
        transfers: results,
        warnings,
    })
}

fn compile_transfer_transaction(
    index: usize,
    metadata: &MintMetadata,
    transfer: &PlannedTokenTransfer,
    wallet_by_key: &HashMap<String, WalletTokenBalance>,
    blockhash: solana_sdk::hash::Hash,
    provider: &str,
    tip_sol: &str,
    compute_unit_price_micro_lamports: u64,
) -> Result<CompiledTransaction, String> {
    let source = wallet_by_key
        .get(&transfer.source_wallet_key)
        .ok_or_else(|| {
            format!(
                "Missing source wallet balance for {}.",
                transfer.source_wallet_key
            )
        })?;
    let destination = wallet_by_key
        .get(&transfer.destination_wallet_key)
        .ok_or_else(|| {
            format!(
                "Missing destination wallet balance for {}.",
                transfer.destination_wallet_key
            )
        })?;
    let payer = load_solana_wallet_by_env_key(&transfer.source_wallet_key)?;
    let mut instructions = vec![build_compute_unit_limit_instruction(
        TOKEN_DISTRIBUTION_COMPUTE_UNIT_LIMIT,
    )?];
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }
    instructions.extend([
        create_associated_token_account_idempotent(
            &source.owner,
            &destination.owner,
            &metadata.mint,
            &metadata.token_program,
        ),
        transfer_checked_instruction(
            &metadata.token_program,
            &source.ata,
            &metadata.mint,
            &destination.ata,
            &source.owner,
            transfer.amount_raw,
            metadata.decimals,
        )?,
    ]);
    let (inline_tip_lamports, inline_tip_account) =
        if let Some((tip_instruction, tip_lamports, tip_account)) =
            resolve_inline_tip(&source.owner, provider, tip_sol)?
        {
            instructions.push(tip_instruction);
            (Some(tip_lamports), Some(tip_account))
        } else {
            (None, None)
        };
    let message = v0::Message::try_compile(&payer.pubkey(), &instructions, &[], blockhash)
        .map_err(|error| format!("Failed to compile token transfer transaction: {error}"))?;
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer])
        .map_err(|error| format!("Failed to sign token transfer transaction: {error}"))?;
    let signature = transaction
        .signatures
        .first()
        .map(|signature| signature.to_string());
    let serialized = bincode::serialize(&transaction)
        .map_err(|error| format!("Failed to serialize token transfer transaction: {error}"))?;
    Ok(CompiledTransaction {
        label: format!(
            "token-distribution-{}-{}-to-{}",
            index, transfer.source_wallet_key, transfer.destination_wallet_key
        ),
        format: "v0".to_string(),
        serialized_base64: BASE64.encode(serialized),
        signature,
        lookup_tables_used: Vec::new(),
        compute_unit_limit: Some(u64::from(TOKEN_DISTRIBUTION_COMPUTE_UNIT_LIMIT)),
        compute_unit_price_micro_lamports: (compute_unit_price_micro_lamports > 0)
            .then_some(compute_unit_price_micro_lamports),
        inline_tip_lamports,
        inline_tip_account,
    })
}

fn transfer_checked_instruction(
    token_program: &Pubkey,
    source: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    owner: &Pubkey,
    amount_raw: u64,
    decimals: u8,
) -> Result<Instruction, String> {
    if *token_program != spl_token::id() && *token_program != token_2022_program_id()? {
        return Err(format!(
            "Unsupported token program for transfer: {token_program}"
        ));
    }
    let data = spl_token::instruction::TokenInstruction::TransferChecked {
        amount: amount_raw,
        decimals,
    }
    .pack();
    Ok(Instruction {
        program_id: *token_program,
        accounts: vec![
            AccountMeta::new(*source, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(*owner, true),
        ],
        data,
    })
}

fn supported_distribution_provider(provider: &str) -> Result<String, String> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "helius" | "helius-sender" => Ok("helius-sender".to_string()),
        "hellomoon" | "hello-moon" | "lunar-lander" => Ok("hellomoon".to_string()),
        other => Err(format!(
            "Token distribution only supports Helius Sender or Hello Moon presets, got {other:?}."
        )),
    }
}

fn resolve_inline_tip(
    payer: &Pubkey,
    provider: &str,
    tip_sol: &str,
) -> Result<Option<(Instruction, u64, String)>, String> {
    let tip_account = pick_tip_account_for_provider(provider);
    if tip_account.trim().is_empty() {
        return Ok(None);
    }
    let required_lamports = provider_required_tip_lamports(provider).unwrap_or(0);
    let requested_lamports = parse_sol_lamports_field(tip_sol).unwrap_or(0);
    let lamports = requested_lamports.max(required_lamports);
    if lamports == 0 {
        return Ok(None);
    }
    let tip_pubkey = parse_pubkey(&tip_account, "token distribution tip account")?;
    Ok(Some((
        transfer(payer, &tip_pubkey, lamports),
        lamports,
        tip_account,
    )))
}

fn priority_fee_sol_to_micro_lamports(priority_fee_sol: &str) -> Result<u64, String> {
    let lamports = parse_sol_lamports_field(priority_fee_sol)?;
    if lamports == 0 {
        Ok(0)
    } else {
        Ok((lamports.saturating_mul(1_000_000)) / PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT)
    }
}

fn parse_sol_lamports_field(value: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    if trimmed.starts_with('-') {
        return Err("SOL fee field cannot be negative.".to_string());
    }
    let (whole, fraction) = trimmed.split_once('.').unwrap_or((trimmed, ""));
    if whole.is_empty() || !whole.chars().all(|character| character.is_ascii_digit()) {
        return Err(format!("Invalid SOL fee field: {trimmed}"));
    }
    if !fraction.chars().all(|character| character.is_ascii_digit()) {
        return Err(format!("Invalid SOL fee field: {trimmed}"));
    }
    let whole_lamports = whole
        .parse::<u64>()
        .map_err(|_| format!("SOL fee field is too large: {trimmed}"))?
        .checked_mul(1_000_000_000)
        .ok_or_else(|| format!("SOL fee field is too large: {trimmed}"))?;
    let mut fractional = fraction.to_string();
    if fractional.len() > 9 {
        if fractional[9..].chars().any(|character| character != '0') {
            return Err(format!(
                "SOL fee field has more than 9 decimal places: {trimmed}"
            ));
        }
        fractional.truncate(9);
    }
    while fractional.len() < 9 {
        fractional.push('0');
    }
    let fractional_lamports = if fractional.is_empty() {
        0
    } else {
        fractional
            .parse::<u64>()
            .map_err(|_| format!("Invalid SOL fee field: {trimmed}"))?
    };
    whole_lamports
        .checked_add(fractional_lamports)
        .ok_or_else(|| format!("SOL fee field is too large: {trimmed}"))
}

fn build_compute_unit_limit_instruction(compute_unit_limit: u32) -> Result<Instruction, String> {
    let mut data = vec![2];
    data.extend_from_slice(&compute_unit_limit.to_le_bytes());
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

fn compute_budget_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(COMPUTE_BUDGET_PROGRAM_ID)
        .map_err(|error| format!("Invalid Compute Budget program id: {error}"))
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|error| format!("Invalid {label}: {error}"))
}

fn token_2022_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(TOKEN_2022_PROGRAM_ID)
        .map_err(|error| format!("Invalid Token-2022 program id: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn balance(wallet_key: &str, amount_raw: u64) -> WalletTokenBalance {
        WalletTokenBalance {
            wallet_key: wallet_key.to_string(),
            owner: Pubkey::new_unique(),
            ata: Pubkey::new_unique(),
            amount_raw,
        }
    }

    #[test]
    fn split_uses_selected_holders_only_as_sources() {
        let balances = vec![
            balance("a", 1_000),
            balance("b", 0),
            balance("c", 0),
            balance("unselected-holder", 5_000),
        ];
        let target_keys = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let source_keys = HashSet::from(["a".to_string()]);
        let transfers =
            plan_split_transfers(&balances, &target_keys, &source_keys, "mint").unwrap();
        assert!(!transfers.is_empty());
        assert!(
            transfers
                .iter()
                .all(|transfer| transfer.source_wallet_key == "a")
        );
        assert!(
            transfers
                .iter()
                .all(|transfer| transfer.source_wallet_key != "unselected-holder")
        );
    }

    #[test]
    fn split_targets_stay_within_twelve_percent_spread() {
        let keys = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];
        let balances = HashMap::from([
            ("a".to_string(), 1_000_000u64),
            ("b".to_string(), 0u64),
            ("c".to_string(), 0u64),
            ("d".to_string(), 0u64),
        ]);
        let targets = split_target_amounts(&keys, &balances, "mint").unwrap();
        let min = targets.values().copied().min().unwrap();
        let max = targets.values().copied().max().unwrap();
        assert!(max <= min + ((min as f64 * 0.13).ceil() as u64));
        assert_eq!(targets.values().copied().sum::<u64>(), 1_000_000);
    }

    #[test]
    fn split_preserves_raw_unit_remainder() {
        let keys = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let balances = HashMap::from([("a".to_string(), 10u64)]);
        let targets = split_target_amounts(&keys, &balances, "mint").unwrap();
        assert_eq!(targets.values().copied().sum::<u64>(), 10);
    }

    #[test]
    fn consolidate_excludes_destination_and_zero_balances() {
        let balances = vec![balance("a", 100), balance("b", 0), balance("c", 50)];
        let transfers = plan_consolidate_transfers(&balances, "a");
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].source_wallet_key, "c");
        assert_eq!(transfers[0].destination_wallet_key, "a");
        assert_eq!(transfers[0].amount_raw, 50);
    }

    #[test]
    fn transfer_instruction_supports_token_2022_program() {
        let source = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let destination = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let token_2022 = token_2022_program_id().expect("token-2022 id");
        let instruction =
            transfer_checked_instruction(&token_2022, &source, &mint, &destination, &owner, 1, 6)
                .expect("build token-2022 transfer");
        assert_eq!(instruction.program_id, token_2022);
    }

    #[test]
    fn distribution_provider_support_is_limited_to_helius_sender_and_hellomoon() {
        assert_eq!(
            supported_distribution_provider("helius").unwrap(),
            "helius-sender"
        );
        assert_eq!(
            supported_distribution_provider("hello-moon").unwrap(),
            "hellomoon"
        );
        assert!(supported_distribution_provider("standard-rpc").is_err());
    }

    #[test]
    fn priority_fee_sol_maps_to_micro_lamports() {
        assert_eq!(priority_fee_sol_to_micro_lamports("").unwrap(), 0);
        assert_eq!(
            priority_fee_sol_to_micro_lamports("0.001").unwrap(),
            1_000_000
        );
    }

    #[test]
    fn inline_tip_applies_provider_floor() {
        let payer = Pubkey::new_unique();
        let (_, lamports, account) = resolve_inline_tip(&payer, "hellomoon", "0")
            .unwrap()
            .unwrap();
        assert_eq!(lamports, 1_000_000);
        assert!(!account.trim().is_empty());
    }
}
