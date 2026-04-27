use std::{collections::HashSet, str::FromStr, sync::OnceLock};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::{VersionedMessage, v0},
    pubkey::Pubkey,
    signature::Signer,
    transaction::VersionedTransaction,
};
use solana_system_interface::program as system_program;
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};

use crate::{
    rpc_client::{
        CompiledTransaction, configured_rpc_url, confirm_submitted_transactions_for_transport,
        fetch_account_owner_and_data, fetch_latest_blockhash,
        fetch_minimum_balance_for_rent_exemption, rpc_request_with_client, shared_rpc_http_client,
        submit_independent_transactions_for_transport,
    },
    transport::{ExecutionTransportConfig, build_transport_plan},
    wallet_store::load_solana_wallet_by_env_key,
};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const REWARDS_PARALLELISM: usize = 4;
const TOKEN_ACCOUNT_AMOUNT_OFFSET: usize = 64;
const TOKEN_ACCOUNT_AMOUNT_LEN: usize = 8;
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

const PUMP_CLAIM_CASHBACK_DISCRIMINATOR: [u8; 8] = [37, 58, 35, 126, 190, 53, 228, 197];
const PUMP_COLLECT_CREATOR_FEE_DISCRIMINATOR: [u8; 8] = [20, 22, 86, 123, 198, 28, 219, 132];
const PUMP_AMM_COLLECT_CREATOR_FEE_DISCRIMINATOR: [u8; 8] = [160, 57, 89, 42, 181, 139, 43, 66];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardsSummaryRequest {
    #[serde(default)]
    pub wallet_keys: Vec<String>,
    #[serde(default)]
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardsClaimRequest {
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub items: Vec<RewardClaimItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardClaimItem {
    pub id: String,
    pub provider_id: String,
    pub provider: String,
    pub reward_type: String,
    pub wallet_key: String,
    pub wallet_public_key: String,
    #[serde(default)]
    pub mint: String,
    #[serde(default)]
    pub amount_lamports: u64,
}

#[derive(Debug, Clone)]
pub struct RewardWallet {
    pub key: String,
    pub label: String,
    pub public_key: String,
}

#[derive(Debug, Clone)]
pub struct RewardsExecutionConfig {
    pub commitment: String,
    pub skip_preflight: bool,
    pub track_send_block_height: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardsSummaryResponse {
    pub ok: bool,
    pub providers: Vec<RewardProviderSummary>,
    #[serde(default)]
    pub errors: Vec<RewardError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardProviderSummary {
    pub provider_id: String,
    pub provider: String,
    pub reward_type: String,
    pub title: String,
    pub claimable_lamports: u64,
    pub claimed_lamports: u64,
    pub positions: Option<usize>,
    pub configured: bool,
    #[serde(default)]
    pub reason: String,
    pub rows: Vec<RewardRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardRow {
    pub id: String,
    pub provider_id: String,
    pub provider: String,
    pub reward_type: String,
    pub wallet_key: String,
    pub wallet_public_key: String,
    pub wallet_label: String,
    #[serde(default)]
    pub mint: String,
    pub amount_lamports: u64,
    pub amount_ui: f64,
    pub claimable: bool,
    pub configured: bool,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardError {
    pub provider_id: String,
    #[serde(default)]
    pub wallet_key: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardsClaimResponse {
    pub ok: bool,
    pub confirmed_count: usize,
    pub failed_count: usize,
    pub stale_count: usize,
    pub results: Vec<RewardClaimResult>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardClaimResult {
    pub id: String,
    pub provider_id: String,
    pub wallet_key: String,
    #[serde(default)]
    pub signature: Option<String>,
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
}

pub async fn summarize_rewards(
    wallets: Vec<RewardWallet>,
    config: RewardsExecutionConfig,
) -> RewardsSummaryResponse {
    let rpc_url = configured_rpc_url();
    let wallet_results = stream::iter(wallets.clone())
        .map(|wallet| {
            let rpc_url = rpc_url.clone();
            let commitment = config.commitment.clone();
            async move { summarize_wallet_rewards(&rpc_url, &commitment, wallet).await }
        })
        .buffer_unordered(REWARDS_PARALLELISM)
        .collect::<Vec<_>>()
        .await;

    let mut pump_creator_rows = Vec::new();
    let mut pump_cashback_rows = Vec::new();
    let mut bags_rows = Vec::new();
    let mut errors = Vec::new();
    for result in wallet_results {
        match result {
            Ok(mut rows) => {
                for row in rows.drain(..) {
                    match row.provider_id.as_str() {
                        "pumpCreator" => pump_creator_rows.push(row),
                        "pumpCashback" => pump_cashback_rows.push(row),
                        "bagsCreator" => bags_rows.push(row),
                        _ => {}
                    }
                }
            }
            Err(error) => errors.push(error),
        }
    }

    let bags_configured = active_bags_api_key().is_some();
    OkResponse {
        providers: vec![
            provider_summary(
                "pumpCreator",
                "pump",
                "creatorFees",
                "Pump.fun",
                pump_creator_rows,
                true,
                "",
            ),
            provider_summary(
                "pumpCashback",
                "pump",
                "cashback",
                "Cashback",
                pump_cashback_rows,
                true,
                "",
            ),
            provider_summary(
                "bagsCreator",
                "bags",
                "creatorFees",
                "Bags",
                bags_rows,
                bags_configured,
                if bags_configured {
                    ""
                } else {
                    "BAGS_API_KEY is not configured."
                },
            ),
        ],
        errors,
    }
    .into()
}

struct OkResponse {
    providers: Vec<RewardProviderSummary>,
    errors: Vec<RewardError>,
}

impl From<OkResponse> for RewardsSummaryResponse {
    fn from(value: OkResponse) -> Self {
        Self {
            ok: value.errors.is_empty(),
            providers: value.providers,
            errors: value.errors,
        }
    }
}

async fn summarize_wallet_rewards(
    rpc_url: &str,
    commitment: &str,
    wallet: RewardWallet,
) -> Result<Vec<RewardRow>, RewardError> {
    let owner = Pubkey::from_str(&wallet.public_key).map_err(|error| RewardError {
        provider_id: "wallet".to_string(),
        wallet_key: wallet.key.clone(),
        message: format!("Invalid wallet public key: {error}"),
    })?;
    let (pump_creator, pump_cashback) = tokio::join!(
        pump_creator_row(rpc_url, commitment, &wallet, &owner),
        pump_cashback_row(rpc_url, commitment, &wallet, &owner)
    );
    let bags = bags_rows_for_wallet(&wallet).await.unwrap_or_else(|error| {
        vec![RewardRow {
            id: format!("bagsCreator:{}:error", wallet.key),
            provider_id: "bagsCreator".to_string(),
            provider: "bags".to_string(),
            reward_type: "creatorFees".to_string(),
            wallet_key: wallet.key.clone(),
            wallet_public_key: wallet.public_key.clone(),
            wallet_label: wallet.label.clone(),
            mint: String::new(),
            amount_lamports: 0,
            amount_ui: 0.0,
            claimable: false,
            configured: active_bags_api_key().is_some(),
            reason: error,
        }]
    });
    let mut rows = vec![pump_creator, pump_cashback];
    rows.extend(bags);
    Ok(rows)
}

fn provider_summary(
    provider_id: &str,
    provider: &str,
    reward_type: &str,
    title: &str,
    rows: Vec<RewardRow>,
    configured: bool,
    reason: &str,
) -> RewardProviderSummary {
    let claimable_lamports = rows
        .iter()
        .filter(|row| row.claimable)
        .fold(0u64, |sum, row| sum.saturating_add(row.amount_lamports));
    let positions = if provider_id == "bagsCreator" {
        Some(rows.iter().filter(|row| row.amount_lamports > 0).count())
    } else {
        None
    };
    RewardProviderSummary {
        provider_id: provider_id.to_string(),
        provider: provider.to_string(),
        reward_type: reward_type.to_string(),
        title: title.to_string(),
        claimable_lamports,
        claimed_lamports: 0,
        positions,
        configured,
        reason: reason.to_string(),
        rows,
    }
}

async fn pump_creator_row(
    rpc_url: &str,
    commitment: &str,
    wallet: &RewardWallet,
    owner: &Pubkey,
) -> RewardRow {
    match pump_creator_claimable_lamports(rpc_url, commitment, owner).await {
        Ok(amount) => reward_row(
            "pumpCreator",
            "pump",
            "creatorFees",
            wallet,
            "",
            amount,
            amount > 0,
            true,
            "",
        ),
        Err(error) => reward_row(
            "pumpCreator",
            "pump",
            "creatorFees",
            wallet,
            "",
            0,
            false,
            true,
            &error,
        ),
    }
}

async fn pump_cashback_row(
    rpc_url: &str,
    commitment: &str,
    wallet: &RewardWallet,
    owner: &Pubkey,
) -> RewardRow {
    match pump_cashback_claimable_lamports(rpc_url, commitment, owner).await {
        Ok(amount) => reward_row(
            "pumpCashback",
            "pump",
            "cashback",
            wallet,
            "",
            amount,
            amount > 0,
            true,
            "",
        ),
        Err(error) => reward_row(
            "pumpCashback",
            "pump",
            "cashback",
            wallet,
            "",
            0,
            false,
            true,
            &error,
        ),
    }
}

fn reward_row(
    provider_id: &str,
    provider: &str,
    reward_type: &str,
    wallet: &RewardWallet,
    mint: &str,
    amount_lamports: u64,
    claimable: bool,
    configured: bool,
    reason: &str,
) -> RewardRow {
    let mint_suffix = if mint.is_empty() { "sol" } else { mint };
    RewardRow {
        id: format!("{provider_id}:{}:{mint_suffix}", wallet.key),
        provider_id: provider_id.to_string(),
        provider: provider.to_string(),
        reward_type: reward_type.to_string(),
        wallet_key: wallet.key.clone(),
        wallet_public_key: wallet.public_key.clone(),
        wallet_label: wallet.label.clone(),
        mint: mint.to_string(),
        amount_lamports,
        amount_ui: amount_lamports as f64 / LAMPORTS_PER_SOL,
        claimable,
        configured,
        reason: reason.to_string(),
    }
}

async fn pump_creator_claimable_lamports(
    rpc_url: &str,
    commitment: &str,
    owner: &Pubkey,
) -> Result<u64, String> {
    let curve_vault = creator_vault_pda(owner)?;
    let curve_amount =
        fetch_native_account_spendable_lamports(rpc_url, commitment, &curve_vault).await?;
    let amm_authority = pump_amm_coin_creator_vault_authority_pda(owner)?;
    let amm_ata = get_associated_token_address_with_program_id(
        &amm_authority,
        &wsol_mint()?,
        &token_program_id()?,
    );
    let amm_amount = fetch_token_account_amount(rpc_url, commitment, &amm_ata).await?;
    Ok(curve_amount.saturating_add(amm_amount))
}

async fn pump_cashback_claimable_lamports(
    rpc_url: &str,
    commitment: &str,
    owner: &Pubkey,
) -> Result<u64, String> {
    let native_accumulator = user_volume_accumulator_pda(owner)?;
    let native_amount =
        fetch_native_account_spendable_lamports(rpc_url, commitment, &native_accumulator).await?;
    let amm_wsol_ata = pump_amm_user_volume_accumulator_wsol_ata(owner)?;
    let amm_amount = fetch_token_account_amount(rpc_url, commitment, &amm_wsol_ata).await?;
    Ok(native_amount.saturating_add(amm_amount))
}

async fn fetch_native_account_spendable_lamports(
    rpc_url: &str,
    commitment: &str,
    account: &Pubkey,
) -> Result<u64, String> {
    let result = rpc_request_with_client(
        shared_rpc_http_client(),
        rpc_url,
        "getAccountInfo",
        json!([
            account.to_string(),
            { "encoding": "base64", "commitment": commitment }
        ]),
    )
    .await?;
    let Some(value) = result.get("value") else {
        return Ok(0);
    };
    if value.is_null() {
        return Ok(0);
    }
    let lamports = value.get("lamports").and_then(Value::as_u64).unwrap_or(0);
    let data_len = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .and_then(|data| BASE64.decode(data).ok())
        .map(|data| data.len() as u64)
        .unwrap_or(0);
    let rent = if data_len > 0 {
        fetch_minimum_balance_for_rent_exemption(rpc_url, commitment, data_len).await?
    } else {
        0
    };
    Ok(lamports.saturating_sub(rent))
}

async fn fetch_token_account_amount(
    rpc_url: &str,
    commitment: &str,
    account: &Pubkey,
) -> Result<u64, String> {
    match fetch_account_owner_and_data(rpc_url, &account.to_string(), commitment).await? {
        Some((_owner, data)) => parse_token_account_amount(&data),
        None => Ok(0),
    }
}

fn parse_token_account_amount(data: &[u8]) -> Result<u64, String> {
    let end = TOKEN_ACCOUNT_AMOUNT_OFFSET + TOKEN_ACCOUNT_AMOUNT_LEN;
    if data.len() < end {
        return Ok(0);
    }
    let bytes: [u8; TOKEN_ACCOUNT_AMOUNT_LEN] =
        data[TOKEN_ACCOUNT_AMOUNT_OFFSET..end]
            .try_into()
            .map_err(|_| "Malformed token amount bytes.".to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

pub async fn claim_rewards(
    request: RewardsClaimRequest,
    config: RewardsExecutionConfig,
) -> Result<RewardsClaimResponse, String> {
    let mut seen = HashSet::new();
    let claim_items = request
        .items
        .into_iter()
        .filter(|item| item.amount_lamports > 0 && seen.insert(item.id.clone()))
        .collect::<Vec<_>>();
    let claim_results = stream::iter(claim_items)
        .map(|item| {
            let config = config.clone();
            async move { claim_reward_item(&item, &config).await }
        })
        .buffer_unordered(REWARDS_PARALLELISM)
        .collect::<Vec<_>>()
        .await;
    let mut results = Vec::with_capacity(claim_results.len());
    let mut warnings = Vec::new();
    for (result, item_warnings) in claim_results {
        results.push(result);
        warnings.extend(item_warnings);
    }
    let confirmed_count = results
        .iter()
        .filter(|result| result.status == "confirmed")
        .count();
    let stale_count = results
        .iter()
        .filter(|result| result.status == "stale")
        .count();
    let failed_count = results
        .iter()
        .filter(|result| result.status == "failed")
        .count();
    Ok(RewardsClaimResponse {
        ok: failed_count == 0 && stale_count == 0,
        confirmed_count,
        failed_count,
        stale_count,
        results,
        warnings,
    })
}

async fn claim_reward_item(
    item: &RewardClaimItem,
    config: &RewardsExecutionConfig,
) -> (RewardClaimResult, Vec<String>) {
    let mut warnings = Vec::new();
    let compiled = match compile_reward_claim(item, config).await {
        Ok(compiled) => compiled,
        Err(error) => return (claim_result(item, None, "failed", Some(error)), warnings),
    };
    let rpc_url = configured_rpc_url();
    let transport_plan = build_transport_plan(
        &ExecutionTransportConfig {
            provider: "standard-rpc".to_string(),
            endpoint_profile: "global".to_string(),
            commitment: config.commitment.clone(),
            skip_preflight: config.skip_preflight,
            track_send_block_height: config.track_send_block_height,
            mev_mode: "off".to_string(),
            mev_protect: false,
        },
        1,
    );
    match submit_independent_transactions_for_transport(
        &rpc_url,
        &transport_plan,
        std::slice::from_ref(&compiled),
    )
    .await
    {
        Ok((mut submitted, submit_warnings, _submit_ms)) => {
            warnings.extend(submit_warnings);
            match confirm_submitted_transactions_for_transport(
                &rpc_url,
                &transport_plan,
                &mut submitted,
            )
            .await
            {
                Ok((confirm_warnings, _confirm_ms)) => {
                    warnings.extend(confirm_warnings);
                    let entry = submitted.into_iter().next();
                    let signature = entry.as_ref().and_then(|entry| entry.signature.clone());
                    let error = entry.and_then(|entry| entry.error);
                    if error.is_some() || signature.is_none() {
                        (claim_result(item, signature, "failed", error), warnings)
                    } else {
                        (claim_result(item, signature, "confirmed", None), warnings)
                    }
                }
                Err(error) => (
                    claim_result(
                        item,
                        submitted
                            .first()
                            .and_then(|entry| entry.signature.clone())
                            .or(compiled.signature),
                        "stale",
                        Some(error),
                    ),
                    warnings,
                ),
            }
        }
        Err(error) => (
            claim_result(item, compiled.signature, "failed", Some(error)),
            warnings,
        ),
    }
}

async fn compile_reward_claim(
    item: &RewardClaimItem,
    config: &RewardsExecutionConfig,
) -> Result<CompiledTransaction, String> {
    let owner = Pubkey::from_str(&item.wallet_public_key)
        .map_err(|error| format!("Invalid wallet public key: {error}"))?;
    match item.provider_id.as_str() {
        "pumpCreator" => compile_pump_creator_claim(&item.wallet_key, &owner, config).await,
        "pumpCashback" => compile_pump_cashback_claim(&item.wallet_key, &owner, config).await,
        "bagsCreator" => compile_bags_claim(item, config).await,
        other => Err(format!("Unsupported rewards provider: {other}")),
    }
}

async fn compile_pump_creator_claim(
    wallet_key: &str,
    owner: &Pubkey,
    config: &RewardsExecutionConfig,
) -> Result<CompiledTransaction, String> {
    let rpc_url = configured_rpc_url();
    let commitment = &config.commitment;
    let mut instructions = Vec::new();
    if fetch_native_account_spendable_lamports(&rpc_url, commitment, &creator_vault_pda(owner)?)
        .await?
        > 0
    {
        instructions.push(pump_collect_creator_fee_instruction(owner)?);
    }
    let amm_authority = pump_amm_coin_creator_vault_authority_pda(owner)?;
    let amm_ata = get_associated_token_address_with_program_id(
        &amm_authority,
        &wsol_mint()?,
        &token_program_id()?,
    );
    if fetch_token_account_amount(&rpc_url, commitment, &amm_ata).await? > 0 {
        instructions.push(create_associated_token_account_idempotent(
            owner,
            owner,
            &wsol_mint()?,
            &token_program_id()?,
        ));
        instructions.push(pump_amm_collect_creator_fee_instruction(owner)?);
    }
    if instructions.is_empty() {
        return Err("No Pump creator fees are currently claimable for this wallet.".to_string());
    }
    compile_instructions("pump-creator-claim", wallet_key, instructions, config).await
}

async fn compile_pump_cashback_claim(
    wallet_key: &str,
    owner: &Pubkey,
    config: &RewardsExecutionConfig,
) -> Result<CompiledTransaction, String> {
    let user_wsol_ata =
        get_associated_token_address_with_program_id(owner, &wsol_mint()?, &token_program_id()?);
    let rpc_url = configured_rpc_url();
    let commitment = &config.commitment;
    let mut instructions = Vec::new();
    if fetch_native_account_spendable_lamports(
        &rpc_url,
        commitment,
        &user_volume_accumulator_pda(owner)?,
    )
    .await?
        > 0
    {
        instructions.push(pump_claim_cashback_instruction(owner)?);
    }
    if fetch_token_account_amount(
        &rpc_url,
        commitment,
        &pump_amm_user_volume_accumulator_wsol_ata(owner)?,
    )
    .await?
        > 0
    {
        instructions.extend(pump_amm_cashback_claim_instructions(owner, &user_wsol_ata)?);
    }
    if instructions.is_empty() {
        return Err("No Pump cashback is currently claimable for this wallet.".to_string());
    }
    compile_instructions("pump-cashback-claim", wallet_key, instructions, config).await
}

fn pump_amm_cashback_claim_instructions(
    owner: &Pubkey,
    user_wsol_ata: &Pubkey,
) -> Result<Vec<Instruction>, String> {
    Ok(vec![
        create_associated_token_account_idempotent(
            owner,
            owner,
            &wsol_mint()?,
            &token_program_id()?,
        ),
        pump_amm_claim_cashback_instruction(owner)?,
        spl_token::instruction::close_account(
            &token_program_id()?,
            user_wsol_ata,
            owner,
            owner,
            &[],
        )
        .map_err(|error| format!("Failed to build WSOL close instruction: {error}"))?,
    ])
}

async fn compile_bags_claim(
    item: &RewardClaimItem,
    _config: &RewardsExecutionConfig,
) -> Result<CompiledTransaction, String> {
    let api_key = active_bags_api_key()
        .ok_or_else(|| "BAGS_API_KEY is required for Bags rewards.".to_string())?;
    let response = bags_http_client()
        .post(format!("{}/token-launch/claim-txs/v3", bags_api_base_url()))
        .header("x-api-key", api_key)
        .json(&json!({
            "feeClaimer": item.wallet_public_key,
            "tokenMint": item.mint
        }))
        .send()
        .await
        .map_err(|error| format!("Failed to request Bags claim transaction: {error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("Failed to read Bags claim response: {error}"))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("Failed to parse Bags claim response: {error}"))?;
    if !status.is_success()
        || !value
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Bags claim transaction request failed.");
        return Err(error.to_string());
    }
    let tx = value
        .get("response")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|entry| entry.get("tx"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Bags did not return a claim transaction.".to_string())?;
    sign_bags_transaction(&item.wallet_key, tx)
}

fn sign_bags_transaction(wallet_key: &str, tx_base58: &str) -> Result<CompiledTransaction, String> {
    let wallet = load_solana_wallet_by_env_key(wallet_key)?;
    let bytes = bs58::decode(tx_base58)
        .into_vec()
        .map_err(|error| format!("Failed to decode Bags transaction: {error}"))?;
    let unsigned: VersionedTransaction = bincode::deserialize(&bytes)
        .map_err(|error| format!("Failed to deserialize Bags transaction: {error}"))?;
    let signed = VersionedTransaction::try_new(unsigned.message, &[&wallet])
        .map_err(|error| format!("Failed to sign Bags claim transaction: {error}"))?;
    compiled_from_transaction("bags-creator-claim", signed)
}

async fn compile_instructions(
    label: &str,
    wallet_key: &str,
    instructions: Vec<Instruction>,
    config: &RewardsExecutionConfig,
) -> Result<CompiledTransaction, String> {
    let wallet = load_solana_wallet_by_env_key(wallet_key)?;
    let (blockhash, _) = fetch_latest_blockhash(&configured_rpc_url(), &config.commitment).await?;
    let message = v0::Message::try_compile(&wallet.pubkey(), &instructions, &[], blockhash)
        .map_err(|error| format!("Failed to compile rewards transaction: {error}"))?;
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&wallet])
        .map_err(|error| format!("Failed to sign rewards transaction: {error}"))?;
    compiled_from_transaction(label, transaction)
}

fn compiled_from_transaction(
    label: &str,
    transaction: VersionedTransaction,
) -> Result<CompiledTransaction, String> {
    let signature = transaction
        .signatures
        .first()
        .map(|signature| signature.to_string());
    let serialized = bincode::serialize(&transaction)
        .map_err(|error| format!("Failed to serialize rewards transaction: {error}"))?;
    Ok(CompiledTransaction {
        label: label.to_string(),
        format: "v0".to_string(),
        serialized_base64: BASE64.encode(serialized),
        signature,
        lookup_tables_used: Vec::new(),
        compute_unit_limit: None,
        compute_unit_price_micro_lamports: None,
        inline_tip_lamports: None,
        inline_tip_account: None,
    })
}

fn claim_result(
    item: &RewardClaimItem,
    signature: Option<String>,
    status: &str,
    error: Option<String>,
) -> RewardClaimResult {
    RewardClaimResult {
        id: item.id.clone(),
        provider_id: item.provider_id.clone(),
        wallet_key: item.wallet_key.clone(),
        signature,
        status: status.to_string(),
        error,
    }
}

fn pump_claim_cashback_instruction(user: &Pubkey) -> Result<Instruction, String> {
    let program = pump_program_id()?;
    Ok(Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(*user, true),
            AccountMeta::new(user_volume_accumulator_pda(user)?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&program), false),
            AccountMeta::new_readonly(program, false),
        ],
        data: PUMP_CLAIM_CASHBACK_DISCRIMINATOR.to_vec(),
    })
}

fn pump_amm_claim_cashback_instruction(user: &Pubkey) -> Result<Instruction, String> {
    let program = pump_amm_program_id()?;
    let user_accumulator = pump_amm_user_volume_accumulator_pda(user)?;
    Ok(Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(*user, true),
            AccountMeta::new(user_accumulator, false),
            AccountMeta::new_readonly(wsol_mint()?, false),
            AccountMeta::new_readonly(token_program_id()?, false),
            AccountMeta::new(pump_amm_user_volume_accumulator_wsol_ata(user)?, false),
            AccountMeta::new(
                get_associated_token_address_with_program_id(
                    user,
                    &wsol_mint()?,
                    &token_program_id()?,
                ),
                false,
            ),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&program), false),
            AccountMeta::new_readonly(program, false),
        ],
        data: PUMP_CLAIM_CASHBACK_DISCRIMINATOR.to_vec(),
    })
}

fn pump_collect_creator_fee_instruction(creator: &Pubkey) -> Result<Instruction, String> {
    let program = pump_program_id()?;
    Ok(Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new(*creator, true),
            AccountMeta::new(creator_vault_pda(creator)?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&program), false),
            AccountMeta::new_readonly(program, false),
        ],
        data: PUMP_COLLECT_CREATOR_FEE_DISCRIMINATOR.to_vec(),
    })
}

fn pump_amm_collect_creator_fee_instruction(coin_creator: &Pubkey) -> Result<Instruction, String> {
    let program = pump_amm_program_id()?;
    let authority = pump_amm_coin_creator_vault_authority_pda(coin_creator)?;
    Ok(Instruction {
        program_id: program,
        accounts: vec![
            AccountMeta::new_readonly(wsol_mint()?, false),
            AccountMeta::new_readonly(token_program_id()?, false),
            AccountMeta::new_readonly(*coin_creator, true),
            AccountMeta::new_readonly(authority, false),
            AccountMeta::new(
                get_associated_token_address_with_program_id(
                    &authority,
                    &wsol_mint()?,
                    &token_program_id()?,
                ),
                false,
            ),
            AccountMeta::new(
                get_associated_token_address_with_program_id(
                    coin_creator,
                    &wsol_mint()?,
                    &token_program_id()?,
                ),
                false,
            ),
            AccountMeta::new_readonly(event_authority_pda(&program), false),
            AccountMeta::new_readonly(program, false),
        ],
        data: PUMP_AMM_COLLECT_CREATOR_FEE_DISCRIMINATOR.to_vec(),
    })
}

async fn bags_rows_for_wallet(wallet: &RewardWallet) -> Result<Vec<RewardRow>, String> {
    let Some(api_key) = active_bags_api_key() else {
        return Ok(vec![reward_row(
            "bagsCreator",
            "bags",
            "creatorFees",
            wallet,
            "",
            0,
            false,
            false,
            "BAGS_API_KEY is not configured.",
        )]);
    };
    let response = bags_http_client()
        .get(format!(
            "{}/token-launch/claimable-positions",
            bags_api_base_url()
        ))
        .header("x-api-key", api_key)
        .query(&[("wallet", wallet.public_key.as_str())])
        .send()
        .await
        .map_err(|error| format!("Failed to query Bags rewards: {error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("Failed to read Bags rewards response: {error}"))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("Failed to parse Bags rewards response: {error}"))?;
    if !status.is_success()
        || !value
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Bags rewards query failed.");
        return Err(error.to_string());
    }
    let positions = value
        .get("response")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if positions.is_empty() {
        return Ok(vec![reward_row(
            "bagsCreator",
            "bags",
            "creatorFees",
            wallet,
            "",
            0,
            false,
            true,
            "",
        )]);
    }
    Ok(positions
        .into_iter()
        .filter_map(|position| {
            let mint = position
                .get("baseMint")
                .and_then(Value::as_str)?
                .to_string();
            let amount = position
                .get("totalClaimableLamportsUserShare")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            Some(reward_row(
                "bagsCreator",
                "bags",
                "creatorFees",
                wallet,
                &mint,
                amount,
                amount > 0,
                true,
                "",
            ))
        })
        .collect())
}

fn active_bags_api_key() -> Option<String> {
    crate::bags_execution_support::active_bags_api_key_for_rewards()
}

fn bags_api_base_url() -> String {
    crate::bags_execution_support::bags_api_base_url_for_rewards()
}

fn bags_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("bags rewards client")
    })
}

fn pump_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(PUMP_PROGRAM_ID).map_err(|error| format!("Invalid Pump program id: {error}"))
}

fn pump_amm_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(PUMP_AMM_PROGRAM_ID)
        .map_err(|error| format!("Invalid Pump AMM program id: {error}"))
}

fn token_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(TOKEN_PROGRAM_ID).map_err(|error| format!("Invalid token program id: {error}"))
}

fn wsol_mint() -> Result<Pubkey, String> {
    Pubkey::from_str(WSOL_MINT).map_err(|error| format!("Invalid WSOL mint: {error}"))
}

fn event_authority_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"__event_authority"], program_id).0
}

fn creator_vault_pda(creator: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"creator-vault", creator.as_ref()], &pump_program_id()?).0)
}

fn user_volume_accumulator_pda(user: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"user_volume_accumulator", user.as_ref()],
        &pump_program_id()?,
    )
    .0)
}

fn pump_amm_coin_creator_vault_authority_pda(coin_creator: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"creator_vault", coin_creator.as_ref()],
        &pump_amm_program_id()?,
    )
    .0)
}

fn pump_amm_user_volume_accumulator_pda(user: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"user_volume_accumulator", user.as_ref()],
        &pump_amm_program_id()?,
    )
    .0)
}

fn pump_amm_user_volume_accumulator_wsol_ata(user: &Pubkey) -> Result<Pubkey, String> {
    Ok(get_associated_token_address_with_program_id(
        &pump_amm_user_volume_accumulator_pda(user)?,
        &wsol_mint()?,
        &token_program_id()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pump_amm_cashback_claim_contains_create_claim_close_order() {
        let user = Pubkey::new_unique();
        let user_wsol_ata = get_associated_token_address_with_program_id(
            &user,
            &wsol_mint().expect("wsol"),
            &token_program_id().expect("token program"),
        );
        let instructions = pump_amm_cashback_claim_instructions(&user, &user_wsol_ata)
            .expect("cashback instruction group");
        assert_eq!(instructions.len(), 3);
        assert_eq!(
            instructions[0].program_id,
            spl_associated_token_account::id()
        );
        assert_eq!(
            instructions[1].program_id,
            pump_amm_program_id().expect("amm id")
        );
        assert_eq!(instructions[1].data, PUMP_CLAIM_CASHBACK_DISCRIMINATOR);
        assert_eq!(
            instructions[2].program_id,
            token_program_id().expect("token program")
        );
    }

    #[test]
    fn pump_amm_creator_fee_accounts_match_idl_order() {
        let creator = Pubkey::new_unique();
        let instruction =
            pump_amm_collect_creator_fee_instruction(&creator).expect("creator fee instruction");
        assert_eq!(instruction.data, PUMP_AMM_COLLECT_CREATOR_FEE_DISCRIMINATOR);
        assert_eq!(instruction.accounts.len(), 8);
        assert_eq!(instruction.accounts[0].pubkey, wsol_mint().expect("wsol"));
        assert_eq!(
            instruction.accounts[1].pubkey,
            token_program_id().expect("token program")
        );
        assert_eq!(instruction.accounts[2].pubkey, creator);
        assert!(instruction.accounts[2].is_signer);
    }

    #[test]
    fn zero_amount_rows_are_not_claimable() {
        let wallet = RewardWallet {
            key: "wallet-a".to_string(),
            label: "Wallet A".to_string(),
            public_key: Pubkey::new_unique().to_string(),
        };
        let row = reward_row(
            "pumpCreator",
            "pump",
            "creatorFees",
            &wallet,
            "",
            0,
            false,
            true,
            "",
        );
        assert!(!row.claimable);
        assert_eq!(row.amount_lamports, 0);
    }
}
