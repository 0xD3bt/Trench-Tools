use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use shared_execution_routing::alt_manifest::{
    AltManifestEntry, PUMP_APR28_FEE_RECIPIENTS, lookup_table_address_content_hash,
    shared_alt_manifest_entries,
};
use shared_transaction_submit::{compiled_transaction_signers, fetch_multiple_account_data};
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::{AddressLookupTableAccount, VersionedMessage, v0},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use solana_system_interface::{
    instruction::{create_account, transfer},
    program as system_program,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use spl_token::instruction::{
    close_account as close_spl_account, initialize_account3, sync_native,
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    str::FromStr,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use uuid::Uuid;

use crate::{
    bonk_execution_support::build_trusted_raydium_clmm_swap_exact_in,
    extension_api::{MevMode, TradeSide},
    paths,
    provider_tip::pick_tip_account_for_provider,
    rollout::{wrapper_default_fee_bps, wrapper_fee_vault_pubkey},
    rpc_client::{
        CompiledTransaction, configured_rpc_url, fetch_account_data, fetch_account_exists,
        fetch_account_owner_and_data, fetch_minimum_balance_for_rent_exemption,
        fetch_owned_token_mints,
    },
    stable_native::trusted_stable_routes,
    trade_dispatch::{CompiledAdapterTrade, TransactionDependencyMode},
    trade_planner::{
        LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerRuntimeBundle,
        PlannerVerificationSource, PumpAmmRuntimeBundle, PumpBondingCurveRuntimeBundle,
        TradeLifecycle, TradeVenueFamily, WrapperAction,
    },
    trade_runtime::{RuntimeSellIntent, TradeRuntimeRequest},
    wallet_store::load_solana_wallet_by_env_key,
    warming_service::shared_warming_service,
    wrapper_abi::{
        ABI_VERSION as WRAPPER_ABI_VERSION, EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT,
        EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT, ExecuteAccounts, ExecuteSwapRouteAccounts,
        ExecuteSwapRouteRequest, SWAP_ROUTE_NO_PATCH_OFFSET, SwapLegInputSource,
        SwapRouteDirection, SwapRouteFeeMode, SwapRouteLeg, SwapRouteMode, SwapRouteSettlement,
        TOKEN_PROGRAM_ID as WRAPPER_TOKEN_PROGRAM_ID, build_execute_swap_route_instruction,
        config_pda, instructions_sysvar_id, route_wsol_pda,
    },
    wrapper_compile::estimate_sol_in_fee_lamports,
};

/// Minimum inline tip lamports per provider. These match the provider's
/// server-side enforcement — if we submit below these thresholds the
/// transport rejects the request with a JSON-RPC error.
/// - Helius Sender requires 200_000 lamports (0.0002 SOL) to one of their
///   configured tip accounts (source: Helius 500 error body).
/// - Hello Moon QUIC requires 1_000_000 lamports (0.001 SOL) — see
///   `launchdeck-engine::rpc::validate_hellomoon_transaction`.
const HELIUS_SENDER_MIN_TIP_LAMPORTS: u64 = 200_000;
const HELLO_MOON_MIN_TIP_LAMPORTS: u64 = 1_000_000;
const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

fn provider_min_tip_lamports(provider: &str) -> u64 {
    match provider.trim() {
        "helius-sender" => HELIUS_SENDER_MIN_TIP_LAMPORTS,
        "hellomoon" => HELLO_MOON_MIN_TIP_LAMPORTS,
        _ => 0,
    }
}

/// Resolves the inline tip (instruction + lamports + tip-account string) for a
/// compiled trade. If the chosen provider carries a minimum tip requirement
/// (Helius Sender, Hello Moon) the returned lamports value is floor-clamped to
/// that minimum, even when the preset's `tip_sol` is lower or empty, because
/// the transport will otherwise reject the transaction with an HTTP 500.
///
/// Returns `None` when no tip should be attached (e.g. provider has no tip
/// account configured — `standard-rpc`).
fn resolve_inline_tip(
    payer: &Pubkey,
    provider: &str,
    tip_sol: &str,
) -> Result<Option<(Instruction, u64, String)>, String> {
    let provider_tip_account_raw = pick_tip_account_for_provider(provider);
    // Fall back to the legacy env-var-configured Jito tip account when the
    // provider doesn't expose a well-known tip destination (e.g. standard-rpc
    // routes still want to honour the Jito tip if the operator configured
    // one). If *that* is also empty we simply skip the tip.
    let (tip_account_str, resolved_from_provider) = if provider_tip_account_raw.is_empty() {
        match configured_tip_account()? {
            Some(account) => (account.to_string(), false),
            None => return Ok(None),
        }
    } else {
        (provider_tip_account_raw, true)
    };

    let min_lamports = provider_min_tip_lamports(provider);
    let requested_lamports = parse_sol_lamports_field(tip_sol).unwrap_or(0);
    let lamports = if resolved_from_provider {
        // Floor-clamp to the provider minimum. Going *above* the minimum is
        // fine — that's what the user's "tipSol" preset field controls.
        requested_lamports.max(min_lamports)
    } else {
        // Non-provider-aware paths (standard-rpc with env-var tip) don't have
        // a minimum to enforce; honour the preset value only.
        requested_lamports
    };
    if lamports == 0 {
        return Ok(None);
    }

    if resolved_from_provider && requested_lamports < min_lamports {
        eprintln!(
            "[execution-engine][pump-native] tip floor-clamped for provider={} from {} to {} lamports (preset tip_sol={:?})",
            provider, requested_lamports, lamports, tip_sol
        );
    }

    let tip_pubkey = parse_pubkey(&tip_account_str, "tip account")?;
    Ok(Some((
        transfer(payer, &tip_pubkey, lamports),
        lamports,
        tip_account_str,
    )))
}

fn append_inline_tip(
    instructions: &mut Vec<Instruction>,
    payer: &Pubkey,
    provider: &str,
    tip_sol: &str,
) -> Result<(Option<u64>, Option<String>), String> {
    if let Some((tip_instruction, tip_lamports, tip_account)) =
        resolve_inline_tip(payer, provider, tip_sol)?
    {
        instructions.push(tip_instruction);
        Ok((Some(tip_lamports), Some(tip_account)))
    } else {
        Ok((None, None))
    }
}

fn build_uniqueness_memo_instruction(label: &str) -> Result<Instruction, String> {
    // Solana signatures are deterministic for a fully identical message. When
    // the user spam-clicks the same trade while the cached blockhash is still
    // current, repeated compiles can otherwise collapse onto the same
    // signature. A tiny memo keeps each click as a distinct transaction
    // without changing the actual trade semantics.
    Ok(Instruction {
        program_id: parse_pubkey(MEMO_PROGRAM_ID, "memo program")?,
        accounts: vec![],
        data: format!("tt:{label}:{}", Uuid::new_v4()).into_bytes(),
    })
}

const JITODONTFRONT_ACCOUNT: &str = "jitodontfront111111111111111111111111111111";
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const PUMP_FEE_PROGRAM_ID: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT: u64 = 1_000_000;
const PUMP_BUY_COMPUTE_UNIT_LIMIT: u32 = 280_000;
const PUMP_SELL_COMPUTE_UNIT_LIMIT: u32 = 280_000;
const PUMP_AMM_BUY_COMPUTE_UNIT_LIMIT: u32 = 280_000;
const PUMP_AMM_SELL_COMPUTE_UNIT_LIMIT: u32 = 280_000;
const SPL_TOKEN_ACCOUNT_LEN: u64 = 165;
const SHARED_SUPER_LOOKUP_TABLE: &str = "7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc";

#[derive(Debug, Clone)]
struct SharedLookupTableCacheEntry {
    table: AddressLookupTableAccount,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
struct PersistedLookupTableCache {
    tables: HashMap<String, PersistedLookupTableEntry>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct PersistedLookupTableEntry {
    addresses: Vec<String>,
    #[serde(default)]
    address_count: Option<usize>,
    #[serde(default)]
    content_hash: Option<String>,
    #[serde(default)]
    manifest_hash: Option<String>,
}

#[derive(Debug, Clone)]
struct CompiledTxCandidate {
    compiled: CompiledTransaction,
}

#[derive(Debug, Clone)]
struct PumpGlobalState {
    fee_recipient: Pubkey,
    fee_basis_points: u64,
    creator_fee_basis_points: u64,
    fee_recipients: [Pubkey; 7],
    reserved_fee_recipient: Pubkey,
    reserved_fee_recipients: [Pubkey; 7],
    buyback_fee_recipients: [Pubkey; 8],
    #[allow(dead_code)]
    buyback_basis_points: u64,
    #[allow(dead_code)]
    initial_virtual_quote_reserves: u64,
    #[allow(dead_code)]
    whitelisted_quote_mints: [Pubkey; 1],
}

#[derive(Debug, Clone)]
struct PumpBondingCurveState {
    virtual_token_reserves: u64,
    virtual_quote_reserves: u64,
    real_token_reserves: u64,
    real_quote_reserves: u64,
    complete: bool,
    creator: Pubkey,
    is_mayhem_mode: bool,
    cashback_enabled: bool,
    quote_mint: Pubkey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PumpQuoteAssetKind {
    Sol,
    Usdc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PumpQuoteAssetForMint {
    Wsol,
    Usdc,
}

#[derive(Debug, Clone)]
struct PumpQuoteAssetMeta {
    kind: PumpQuoteAssetKind,
    mint: Pubkey,
    token_program: Pubkey,
    decimals: u8,
    planner_asset: PlannerQuoteAsset,
}

#[derive(Debug, Clone)]
pub(crate) struct PumpBondingCurveAddressClassification {
    pub(crate) mint: String,
    pub(crate) bonding_curve: String,
    pub(crate) complete: bool,
    pub(crate) quote_asset: PlannerQuoteAsset,
}

#[derive(Debug, Clone)]
struct PumpAmmGlobalConfig {
    lp_fee_basis_points: u64,
    protocol_fee_basis_points: u64,
    protocol_fee_recipients: [Pubkey; 8],
    coin_creator_fee_basis_points: u64,
    reserved_fee_recipient: Pubkey,
    reserved_fee_recipients: [Pubkey; 7],
}

#[derive(Debug, Clone)]
pub(crate) struct PumpAmmPoolState {
    pub(crate) pubkey: Pubkey,
    #[allow(dead_code)]
    pub(crate) creator: Pubkey,
    pub(crate) base_mint: Pubkey,
    pub(crate) quote_mint: Pubkey,
    pub(crate) pool_base_token_account: Pubkey,
    pub(crate) pool_quote_token_account: Pubkey,
    pub(crate) coin_creator: Pubkey,
    pub(crate) is_mayhem_mode: bool,
    #[allow(dead_code)]
    pub(crate) is_cashback_coin: bool,
}

#[derive(Debug, Clone)]
struct PumpAmmFeeTier {
    market_cap_lamports_threshold: u128,
    fees: PumpAmmFees,
}

#[derive(Debug, Clone)]
struct PumpAmmFeeConfig {
    flat_fees: PumpAmmFees,
    fee_tiers: Vec<PumpAmmFeeTier>,
}

#[derive(Debug, Clone, Copy)]
struct PumpAmmFees {
    lp_fee_bps: u64,
    protocol_fee_bps: u64,
    creator_fee_bps: u64,
}

pub async fn supports_native_pump_trade(rpc_url: &str, mint: &str) -> Result<bool, String> {
    let mint = parse_pubkey(mint, "mint")?;
    fetch_account_exists(rpc_url, &bonding_curve_pda(&mint)?.to_string(), "confirmed").await
}

pub(crate) async fn classify_pump_bonding_curve_address(
    rpc_url: &str,
    input: &str,
    owner: &Pubkey,
    data: &[u8],
    commitment: &str,
) -> Result<Option<PumpBondingCurveAddressClassification>, String> {
    if *owner != pump_program_id()? {
        return Ok(None);
    }
    let Ok(curve) = decode_bonding_curve_state(data) else {
        return Ok(None);
    };
    let bonding_curve = parse_pubkey(input, "Pump bonding curve address")?;
    let mint_candidates =
        fetch_owned_pump_bonding_curve_token_mints(rpc_url, input, commitment).await?;
    let [(mint, _token_program)] = mint_candidates.as_slice() else {
        return Err(format!(
            "Pump bonding curve {} owned {} supported token mint accounts; expected exactly one.",
            input.trim(),
            mint_candidates.len()
        ));
    };
    let mint_pubkey = parse_pubkey(mint, "Pump bonding curve token mint")?;
    let expected_bonding_curve = bonding_curve_pda(&mint_pubkey)?;
    if expected_bonding_curve != bonding_curve {
        return Err(format!(
            "Pump bonding curve {} token account mint {} does not derive back to the submitted curve {}.",
            input.trim(),
            mint_pubkey,
            expected_bonding_curve
        ));
    }
    Ok(Some(PumpBondingCurveAddressClassification {
        mint: mint_pubkey.to_string(),
        bonding_curve: bonding_curve.to_string(),
        complete: curve.complete,
        quote_asset: pump_quote_asset_meta(&curve.quote_mint)?.planner_asset,
    }))
}

pub async fn plan_pump_trade(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let mint = parse_pubkey(&request.mint, "mint")?;
    let commitment = request.policy.commitment.trim();
    let commitment = if commitment.is_empty() {
        "confirmed"
    } else {
        commitment
    };

    let curve = match fetch_bonding_curve_state(rpc_url, &mint, commitment).await {
        Ok(curve) => Some(curve),
        // No bonding-curve PDA for this mint. That's expected for coins launched directly on
        // Pump.Swap / Pump AMM (Creator Studio path). Fall back to the canonical AMM pool lookup
        // which is derived purely from the mint. If we find a pool, this is a PostMigration
        // PumpAmm venue; otherwise it's not a Pump coin at all.
        Err(error) if error.contains("was not found") => None,
        Err(error) => return Err(error),
    };

    let is_complete = curve.as_ref().map(|c| c.complete).unwrap_or(true);
    if is_complete {
        let mut pinned_pubkey = match request.pinned_pool.as_deref().map(str::trim) {
            Some(value) if !value.is_empty() => Some(parse_pubkey(value, "pinned pool")?),
            _ => None,
        };
        if pinned_pubkey.as_ref().is_some_and(|pinned| {
            bonding_curve_pda(&mint)
                .map(|bonding_curve| bonding_curve == *pinned)
                .unwrap_or(false)
        }) {
            pinned_pubkey = None;
        }
        if let Some(pinned) = pinned_pubkey.as_ref() {
            let Some((owner, data)) =
                fetch_account_owner_and_data(rpc_url, &pinned.to_string(), commitment).await?
            else {
                return Err(format!(
                    "Pinned pool {pinned} was not found on-chain for mint {}.",
                    request.mint
                ));
            };
            if owner != pump_amm_program_id()? {
                return Ok(None);
            }
            let pinned_pool = decode_pump_amm_pool_state(*pinned, &data).map_err(|error| {
                format!(
                    "Pinned pool {pinned} is not a valid Pump AMM pool for mint {mint}: {error}"
                )
            })?;
            if pinned_pool.base_mint != mint {
                return Err(format!(
                    "Pinned pool {pinned} trades base mint {} but the request targets mint {mint}.",
                    pinned_pool.base_mint
                ));
            }
            let pinned_quote_meta = pump_quote_asset_meta(&pinned_pool.quote_mint)?;
            if !matches!(
                pinned_quote_meta.kind,
                PumpQuoteAssetKind::Sol | PumpQuoteAssetKind::Usdc
            ) {
                return Err(format!(
                    "Pinned pool {pinned} uses unsupported Pump AMM quote mint {}.",
                    pinned_pool.quote_mint
                ));
            }
            let canonical_pool = canonical_pump_amm_pool_for_quote(&mint, &pinned_pool.quote_mint)?;
            if *pinned != canonical_pool {
                return Err(format!(
                    "Selected pool is not the canonical Pump AMM pool for mint {} (input pool: {}).",
                    request.mint, pinned
                ));
            }
        }
        let pool =
            match find_pump_amm_pool_state(rpc_url, &mint, pinned_pubkey.as_ref(), commitment)
                .await?
            {
                Some(pool) => pool,
                None => {
                    if pinned_pubkey.is_some() {
                        return Err(format!(
                            "Pinned pool {} was not found on-chain for mint {}.",
                            pinned_pubkey
                                .as_ref()
                                .map(Pubkey::to_string)
                                .unwrap_or_default(),
                            request.mint
                        ));
                    }
                    if curve.is_some() {
                        return Ok(None);
                    }
                    // No bonding curve or Pump AMM pool -> not a Pump coin.
                    return Ok(None);
                }
            };
        let runtime_bundle =
            build_pump_amm_runtime_bundle(rpc_url, &mint, &pool, commitment).await?;
        let quote_meta = pump_quote_asset_meta(&pool.quote_mint)?;
        return Ok(Some(LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PostMigration,
            family: TradeVenueFamily::PumpAmm,
            canonical_market_key: pool.pubkey.to_string(),
            quote_asset: match quote_meta.kind {
                PumpQuoteAssetKind::Sol => PlannerQuoteAsset::Wsol,
                PumpQuoteAssetKind::Usdc => PlannerQuoteAsset::Usdc,
            },
            verification_source: PlannerVerificationSource::OnchainDerived,
            wrapper_action: match request.side {
                TradeSide::Buy => WrapperAction::PumpAmmBuy,
                TradeSide::Sell => WrapperAction::PumpAmmSell,
            },
            wrapper_accounts: vec![
                pool.pubkey.to_string(),
                pool.pool_base_token_account.to_string(),
                pool.pool_quote_token_account.to_string(),
            ],
            market_subtype: Some(
                match quote_meta.kind {
                    PumpQuoteAssetKind::Sol => "wsol",
                    PumpQuoteAssetKind::Usdc => "usdc",
                }
                .to_string(),
            ),
            direct_protocol_target: Some("pump-amm".to_string()),
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: Some(PlannerRuntimeBundle::PumpAmm(runtime_bundle)),
        }));
    }

    let curve = curve.expect("bonding-curve state present when not complete");
    let quote_meta = pump_quote_asset_meta(&curve.quote_mint)?;
    let runtime_bundle =
        build_pump_bonding_runtime_bundle(rpc_url, &mint, &curve, commitment).await?;

    Ok(Some(LifecycleAndCanonicalMarket {
        lifecycle: TradeLifecycle::PreMigration,
        family: TradeVenueFamily::PumpBondingCurve,
        canonical_market_key: bonding_curve_pda(&mint)?.to_string(),
        quote_asset: quote_meta.planner_asset,
        verification_source: PlannerVerificationSource::OnchainDerived,
        wrapper_action: match request.side {
            TradeSide::Buy => WrapperAction::PumpBondingCurveBuy,
            TradeSide::Sell => WrapperAction::PumpBondingCurveSell,
        },
        wrapper_accounts: vec![
            mint.to_string(),
            bonding_curve_pda(&mint)?.to_string(),
            curve.creator.to_string(),
        ],
        market_subtype: Some(
            match quote_meta.kind {
                PumpQuoteAssetKind::Sol => "bonding-curve-sol",
                PumpQuoteAssetKind::Usdc => "bonding-curve-usdc",
            }
            .to_string(),
        ),
        direct_protocol_target: Some("pump-bonding-curve".to_string()),
        input_amount_hint: None,
        minimum_output_hint: None,
        runtime_bundle: Some(PlannerRuntimeBundle::PumpBondingCurve(runtime_bundle)),
    }))
}

pub async fn compile_pump_trade(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    let rpc_url = configured_rpc_url();
    let owner = load_solana_wallet_by_env_key(wallet_key)?;
    let owner_pubkey = owner.pubkey();
    let mint = parse_pubkey(&request.mint, "mint")?;

    // For a selector already confirmed as post-migration Pump AMM, skip the
    // bonding-curve refresh entirely and compile straight against the cached
    // AMM runtime bundle. Pre-migration routes still need the live curve state.
    let curve = if matches!(selector.lifecycle, TradeLifecycle::PostMigration) {
        None
    } else {
        match fetch_bonding_curve_state(&rpc_url, &mint, request.policy.commitment.as_str()).await {
            Ok(curve) => Some(curve),
            Err(error) if error.contains("was not found") => None,
            Err(error) => return Err(error),
        }
    };
    if matches!(selector.lifecycle, TradeLifecycle::PostMigration)
        || curve.as_ref().map(|c| c.complete).unwrap_or(true)
    {
        let launch_creator = match selector.runtime_bundle.as_ref() {
            Some(PlannerRuntimeBundle::PumpAmm(bundle)) => {
                parse_pubkey(&bundle.coin_creator, "pump amm coin creator")?
            }
            _ => curve
                .as_ref()
                .map(|c| c.creator)
                .unwrap_or_else(Pubkey::default),
        };
        return compile_pump_amm_trade(
            &rpc_url,
            selector,
            request,
            owner,
            mint,
            launch_creator,
            wallet_key,
        )
        .await;
    }

    let curve = curve.expect("bonding-curve state present when not complete");
    let token_program = match selector.runtime_bundle.as_ref() {
        Some(PlannerRuntimeBundle::PumpBondingCurve(bundle)) => parse_pump_bonding_token_program(
            &bundle.token_program,
            "pump bonding curve runtime token program",
        )?,
        _ => {
            resolve_pump_bonding_mint_token_program(
                &rpc_url,
                &mint,
                request.policy.commitment.as_str(),
            )
            .await?
        }
    };
    let quote_meta = pump_quote_asset_meta(&curve.quote_mint)?;
    let global = fetch_global_state(&rpc_url).await?;
    let creator_vault_authority =
        resolve_follow_creator_vault_authority(&rpc_url, &mint, &curve.creator).await?;
    let is_mayhem_mode = curve.is_mayhem_mode;
    let is_cashback_coin = curve.cashback_enabled;
    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let compute_unit_limit = match request.side {
        TradeSide::Buy => PUMP_BUY_COMPUTE_UNIT_LIMIT,
        TradeSide::Sell => PUMP_SELL_COMPUTE_UNIT_LIMIT,
    };
    let compute_unit_price_micro_lamports =
        priority_fee_sol_to_micro_lamports(&request.policy.fee_sol)?;
    let jitodontfront_enabled =
        matches!(request.policy.mev_mode, MevMode::Reduced | MevMode::Secure);
    let mut core_instructions = match request.side {
        TradeSide::Buy => {
            let spend_lamports = parse_decimal_units(
                request
                    .buy_amount_sol
                    .as_deref()
                    .ok_or_else(|| "Missing buyAmountSol for buy request.".to_string())?,
                9,
                "buyAmountSol",
            )?;
            if spend_lamports == 0 {
                return Err("Buy amount must be greater than zero.".to_string());
            }
            if matches!(quote_meta.kind, PumpQuoteAssetKind::Usdc) {
                return compile_pump_usdc_bonding_buy_from_sol_route(
                    &rpc_url,
                    request,
                    &owner,
                    &owner_pubkey,
                    &mint,
                    &token_program,
                    spend_lamports,
                    compute_unit_limit,
                    compute_unit_price_micro_lamports,
                    jitodontfront_enabled,
                )
                .await;
            }
            let token_amount = quote_buy_tokens_from_curve(&curve, &global, spend_lamports);
            if token_amount == 0 {
                return Err("Pump native buy quote resolved to zero tokens.".to_string());
            }
            vec![
                build_create_token_ata_instruction(&owner_pubkey, &mint, &token_program)?,
                build_buy_exact_quote_in_v2_instruction(
                    &global,
                    &mint,
                    &creator_vault_authority,
                    &owner_pubkey,
                    spend_lamports,
                    apply_buy_token_slippage(token_amount, u64::from(slippage_bps)),
                    &token_program,
                    &quote_meta,
                    is_mayhem_mode,
                )?,
            ]
        }
        TradeSide::Sell => {
            if matches!(quote_meta.kind, PumpQuoteAssetKind::Usdc) {
                return compile_pump_usdc_bonding_sell_to_sol_route(
                    &rpc_url,
                    request,
                    &owner,
                    &owner_pubkey,
                    &mint,
                    &token_program,
                    wallet_key,
                    compute_unit_limit,
                    compute_unit_price_micro_lamports,
                    jitodontfront_enabled,
                )
                .await;
            }
            let token_amount = resolve_sell_token_amount(
                request
                    .sell_intent
                    .as_ref()
                    .ok_or_else(|| "Missing sell intent for sell request.".to_string())?,
                wallet_key,
                &owner_pubkey.to_string(),
                &request.mint,
                &curve,
                &global,
            )
            .await?;
            let gross_quote = quote_sell_quote_from_curve(&curve, &global, token_amount);
            let min_quote_output = apply_sell_side_slippage(gross_quote, slippage_bps);
            vec![build_sell_v2_instruction(
                &global,
                &mint,
                &creator_vault_authority,
                &owner_pubkey,
                token_amount,
                min_quote_output,
                &token_program,
                &quote_meta,
                is_cashback_coin,
                is_mayhem_mode,
            )?]
        }
    };
    if jitodontfront_enabled {
        apply_jitodontfront(&mut core_instructions, &owner_pubkey)?;
    }

    let mut instructions = vec![build_compute_unit_limit_instruction(compute_unit_limit)?];
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }
    instructions.extend(core_instructions);
    let (inline_tip_lamports, inline_tip_account) = append_inline_tip(
        &mut instructions,
        &owner_pubkey,
        &request.policy.provider,
        &request.policy.tip_sol,
    )?;

    let label = match request.side {
        TradeSide::Buy => "pump-native-buy",
        TradeSide::Sell => "pump-native-sell",
    };
    instructions.push(build_uniqueness_memo_instruction(label)?);
    let blockhash = shared_warming_service()
        .latest_blockhash(&rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let compiled = build_pump_transaction_with_lookup_preference(
        &rpc_url,
        label,
        blockhash,
        &[&owner],
        &instructions,
        compute_unit_limit,
        compute_unit_price_micro_lamports,
        inline_tip_lamports,
        inline_tip_account,
    )
    .await?;

    Ok(CompiledAdapterTrade {
        transactions: vec![compiled],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: None,
    })
}

async fn fetch_global_state(rpc_url: &str) -> Result<PumpGlobalState, String> {
    let address = global_pda()?.to_string();
    let account_data = shared_warming_service()
        .pump_bonding_global_bytes(rpc_url, "confirmed", &address, || async {
            fetch_account_data(rpc_url, &address, "confirmed").await
        })
        .await?;
    decode_global_state(&account_data)
}

async fn fetch_bonding_curve_state(
    rpc_url: &str,
    mint: &Pubkey,
    commitment: &str,
) -> Result<PumpBondingCurveState, String> {
    let account_data =
        fetch_account_data(rpc_url, &bonding_curve_pda(mint)?.to_string(), commitment).await?;
    decode_bonding_curve_state(&account_data)
}

async fn fetch_owned_pump_bonding_curve_token_mints(
    rpc_url: &str,
    bonding_curve: &str,
    commitment: &str,
) -> Result<Vec<(String, Pubkey)>, String> {
    let token_programs = [token_2022_program_id()?, token_program_id()?];
    let mut candidates: Vec<(String, Pubkey)> = Vec::new();
    for token_program in token_programs {
        let token_program_string = token_program.to_string();
        let mints =
            fetch_owned_token_mints(rpc_url, bonding_curve, commitment, &token_program_string)
                .await?;
        for mint in mints {
            if !candidates
                .iter()
                .any(|(existing_mint, _)| existing_mint == &mint)
            {
                candidates.push((mint, token_program));
            }
        }
    }
    Ok(candidates)
}

async fn resolve_pump_bonding_mint_token_program(
    rpc_url: &str,
    mint: &Pubkey,
    commitment: &str,
) -> Result<Pubkey, String> {
    let (owner, _) = fetch_account_owner_and_data(rpc_url, &mint.to_string(), commitment)
        .await?
        .ok_or_else(|| format!("Pump bonding curve mint account {mint} was not found."))?;
    ensure_supported_pump_bonding_token_program(&owner)?;
    Ok(owner)
}

fn parse_pump_bonding_token_program(value: &str, context: &str) -> Result<Pubkey, String> {
    let token_program = parse_pubkey(value, context)?;
    ensure_supported_pump_bonding_token_program(&token_program)?;
    Ok(token_program)
}

fn ensure_supported_pump_bonding_token_program(token_program: &Pubkey) -> Result<(), String> {
    if *token_program == token_2022_program_id()? || *token_program == token_program_id()? {
        Ok(())
    } else {
        Err(format!(
            "Pump bonding curve mint uses unsupported token program {token_program}."
        ))
    }
}

async fn resolve_follow_creator_vault_authority(
    rpc_url: &str,
    mint: &Pubkey,
    launch_creator: &Pubkey,
) -> Result<Pubkey, String> {
    let sharing_config = fee_sharing_config_pda(mint)?;
    let sharing_config_exists =
        fetch_account_exists(rpc_url, &sharing_config.to_string(), "confirmed").await?;
    Ok(if sharing_config_exists {
        sharing_config
    } else {
        *launch_creator
    })
}

async fn build_pump_bonding_runtime_bundle(
    rpc_url: &str,
    mint: &Pubkey,
    curve: &PumpBondingCurveState,
    commitment: &str,
) -> Result<PumpBondingCurveRuntimeBundle, String> {
    let bonding_curve = bonding_curve_pda(mint)?;
    let token_program = resolve_pump_bonding_mint_token_program(rpc_url, mint, commitment).await?;
    let global = fetch_global_state(rpc_url).await?;
    let creator_vault_authority =
        resolve_follow_creator_vault_authority(rpc_url, mint, &curve.creator).await?;
    let quote_meta = pump_quote_asset_meta(&curve.quote_mint)?;
    let fee_recipient = select_buy_fee_recipient(&global, curve.is_mayhem_mode);
    let buyback_fee_recipient = select_pump_buyback_fee_recipient(&global);
    let creator_vault = creator_vault_pda(&creator_vault_authority)?;
    Ok(PumpBondingCurveRuntimeBundle {
        mint: mint.to_string(),
        bonding_curve: bonding_curve.to_string(),
        bonding_curve_v2: bonding_curve_v2_pda(mint)?.to_string(),
        fee_sharing_config: fee_sharing_config_pda(mint)?.to_string(),
        creator_vault_authority: creator_vault_authority.to_string(),
        launch_creator: curve.creator.to_string(),
        token_program: token_program.to_string(),
        associated_bonding_curve: get_associated_token_address_with_program_id(
            &bonding_curve,
            mint,
            &token_program,
        )
        .to_string(),
        global_volume_accumulator: global_volume_accumulator_pda()?.to_string(),
        fee_config: fee_config_pda()?.to_string(),
        quote_mint: quote_meta.mint.to_string(),
        quote_token_program: quote_meta.token_program.to_string(),
        buyback_fee_recipient: buyback_fee_recipient.to_string(),
        associated_quote_fee_recipient: get_associated_token_address_with_program_id(
            &fee_recipient,
            &quote_meta.mint,
            &quote_meta.token_program,
        )
        .to_string(),
        associated_quote_buyback_fee_recipient: get_associated_token_address_with_program_id(
            &buyback_fee_recipient,
            &quote_meta.mint,
            &quote_meta.token_program,
        )
        .to_string(),
        associated_quote_bonding_curve: get_associated_token_address_with_program_id(
            &bonding_curve,
            &quote_meta.mint,
            &quote_meta.token_program,
        )
        .to_string(),
        associated_quote_user: String::new(),
        associated_creator_vault: get_associated_token_address_with_program_id(
            &creator_vault,
            &quote_meta.mint,
            &quote_meta.token_program,
        )
        .to_string(),
        associated_user_volume_accumulator: String::new(),
        is_mayhem_mode: curve.is_mayhem_mode,
        is_cashback_coin: curve.cashback_enabled,
    })
}

async fn build_pump_amm_runtime_bundle(
    rpc_url: &str,
    mint: &Pubkey,
    pool: &PumpAmmPoolState,
    commitment: &str,
) -> Result<PumpAmmRuntimeBundle, String> {
    let global_config = fetch_pump_amm_global_config(rpc_url).await?;
    let protocol_fee_recipient = select_pump_amm_fee_recipient(&global_config, pool.is_mayhem_mode);
    let quote_meta = pump_quote_asset_meta(&pool.quote_mint)?;
    let protocol_fee_recipient_token_account = get_associated_token_address_with_program_id(
        &protocol_fee_recipient,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let coin_creator_vault_authority =
        pump_amm_coin_creator_vault_authority_pda(&pool.coin_creator);
    let coin_creator_vault_ata = get_associated_token_address_with_program_id(
        &coin_creator_vault_authority,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let mint_token_program = fetch_account_owner_and_data(rpc_url, &mint.to_string(), commitment)
        .await?
        .map(|(owner, _)| owner)
        .ok_or_else(|| format!("Mint account {mint} was not found."))?;
    Ok(PumpAmmRuntimeBundle {
        pool: pool.pubkey.to_string(),
        pool_creator: pool.creator.to_string(),
        base_mint: pool.base_mint.to_string(),
        quote_mint: pool.quote_mint.to_string(),
        pool_base_token_account: pool.pool_base_token_account.to_string(),
        pool_quote_token_account: pool.pool_quote_token_account.to_string(),
        mint_token_program: mint_token_program.to_string(),
        global_config: pump_amm_global_config_pda()?.to_string(),
        fee_config: pump_amm_fee_config_pda()?.to_string(),
        protocol_fee_recipient: protocol_fee_recipient.to_string(),
        protocol_fee_recipient_token_account: protocol_fee_recipient_token_account.to_string(),
        coin_creator: pool.coin_creator.to_string(),
        coin_creator_vault_ata: coin_creator_vault_ata.to_string(),
        coin_creator_vault_authority: coin_creator_vault_authority.to_string(),
        global_volume_accumulator: pump_amm_global_volume_accumulator_pda().to_string(),
        is_mayhem_mode: pool.is_mayhem_mode,
        is_cashback_coin: pool.is_cashback_coin,
    })
}

fn configured_shared_super_lookup_table() -> String {
    SHARED_SUPER_LOOKUP_TABLE.to_string()
}

fn shared_lookup_table_cache() -> &'static Mutex<HashMap<String, SharedLookupTableCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, SharedLookupTableCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_persisted_shared_lookup_table_account(address: &str) -> Option<AddressLookupTableAccount> {
    let path = paths::bonk_lookup_table_cache_path();
    let cache = fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<PersistedLookupTableCache>(&raw).ok())?;
    let entry = cache.tables.get(address)?;
    let manifest_hash = shared_alt_manifest_hash();
    if entry.manifest_hash.as_deref() != Some(manifest_hash.as_str()) {
        eprintln!(
            "[execution-engine][alt-cache] ignoring stale shared ALT snapshot {} due to manifest hash mismatch",
            address
        );
        return None;
    }
    if entry.address_count != Some(entry.addresses.len()) {
        eprintln!(
            "[execution-engine][alt-cache] ignoring stale shared ALT snapshot {} due to missing/mismatched address count",
            address
        );
        return None;
    }
    let content_hash = lookup_table_address_content_hash(&entry.addresses);
    if entry.content_hash.as_deref() != Some(content_hash.as_str()) {
        eprintln!(
            "[execution-engine][alt-cache] ignoring stale shared ALT snapshot {} due to content hash mismatch",
            address
        );
        return None;
    }
    let key = parse_pubkey(address, "shared super alt address").ok()?;
    let addresses = entry
        .addresses
        .iter()
        .map(|value| parse_pubkey(value, "shared super alt address entry"))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let table = AddressLookupTableAccount { key, addresses };
    if !shared_lookup_table_satisfies_manifest(&table) {
        eprintln!(
            "[execution-engine][alt-cache] ignoring stale shared ALT snapshot {} because it is missing current manifest addresses",
            address
        );
        return None;
    }
    Some(table)
}

fn persist_shared_lookup_table_account(
    address: &str,
    table: &AddressLookupTableAccount,
) -> Result<(), String> {
    let path: PathBuf = paths::bonk_lookup_table_cache_path();
    let mut cache = fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<PersistedLookupTableCache>(&raw).ok())
        .unwrap_or_default();
    let addresses = table
        .addresses
        .iter()
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>();
    let address_count = addresses.len();
    let content_hash = lookup_table_address_content_hash(&addresses);
    let manifest_hash = shared_alt_manifest_hash();
    cache.tables.insert(
        address.to_string(),
        PersistedLookupTableEntry {
            addresses,
            address_count: Some(address_count),
            content_hash: Some(content_hash),
            manifest_hash: Some(manifest_hash),
        },
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&cache).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn shared_lookup_table_missing_manifest_addresses(
    table: &AddressLookupTableAccount,
) -> Vec<String> {
    let loaded = table
        .addresses
        .iter()
        .map(Pubkey::to_string)
        .collect::<HashSet<_>>();
    shared_alt_manifest_entries()
        .into_iter()
        .filter(|entry| entry.required)
        .filter(|entry| entry.family != "shared-alt")
        .map(|entry| entry.address)
        .filter(|address| !loaded.contains(address))
        .collect()
}

fn shared_lookup_table_satisfies_manifest(table: &AddressLookupTableAccount) -> bool {
    shared_lookup_table_missing_manifest_addresses(table).is_empty()
}

fn shared_alt_manifest_hash() -> String {
    let manifest_fingerprint = shared_alt_manifest_entries()
        .into_iter()
        .filter(|entry| entry.required)
        .filter(|entry| entry.family != "shared-alt")
        .map(|entry| {
            format!(
                "{}|{}|{}|{}",
                entry.address, entry.family, entry.label, entry.required
            )
        })
        .collect::<Vec<_>>();
    lookup_table_address_content_hash(&manifest_fingerprint)
}

async fn fetch_live_shared_lookup_table_account(
    rpc_url: &str,
    address: &str,
) -> Result<AddressLookupTableAccount, String> {
    let data = fetch_account_data(rpc_url, address, "confirmed").await?;
    let table = AddressLookupTable::deserialize(&data)
        .map_err(|error| format!("Failed to decode shared super ALT {address}: {error}"))?;
    let account = AddressLookupTableAccount {
        key: parse_pubkey(address, "shared super alt address")?,
        addresses: table.addresses.to_vec(),
    };
    let missing = shared_lookup_table_missing_manifest_addresses(&account);
    if !missing.is_empty() {
        return Err(format!(
            "Shared ALT {address} is missing {} current manifest address(es), including {}. Extend the table before compiling shared-ALT routes.",
            missing.len(),
            missing.first().cloned().unwrap_or_default()
        ));
    }
    Ok(account)
}

pub(crate) async fn load_shared_super_lookup_tables(
    _rpc_url: &str,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let address = configured_shared_super_lookup_table();
    if let Ok(cache) = shared_lookup_table_cache().lock() {
        if let Some(entry) = cache.get(&address) {
            if shared_lookup_table_satisfies_manifest(&entry.table) {
                return Ok(vec![entry.table.clone()]);
            }
            eprintln!(
                "[execution-engine][alt-cache] ignoring stale in-memory shared ALT snapshot {} because it is missing current manifest addresses",
                address
            );
        }
    }
    let account = load_persisted_shared_lookup_table_account(&address).ok_or_else(|| {
        format!(
            "Shared ALT {address} was not refreshed at startup and no manifest-matched disk snapshot is available."
        )
    })?;
    if let Ok(mut cache) = shared_lookup_table_cache().lock() {
        cache.insert(
            address.clone(),
            SharedLookupTableCacheEntry {
                table: account.clone(),
            },
        );
    }
    Ok(vec![account])
}

pub(crate) async fn refresh_shared_super_lookup_tables(
    rpc_url: &str,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let address = configured_shared_super_lookup_table();
    let account = fetch_live_shared_lookup_table_account(rpc_url, &address).await?;
    if let Ok(mut cache) = shared_lookup_table_cache().lock() {
        cache.insert(
            address.clone(),
            SharedLookupTableCacheEntry {
                table: account.clone(),
            },
        );
    }
    let _ = persist_shared_lookup_table_account(&account.key.to_string(), &account);
    Ok(vec![account])
}

pub(crate) async fn initialize_shared_super_lookup_tables(
    rpc_url: &str,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    match refresh_shared_super_lookup_tables(rpc_url).await {
        Ok(tables) => Ok(tables),
        Err(live_error) => {
            let address = configured_shared_super_lookup_table();
            let Some(account) = load_persisted_shared_lookup_table_account(&address) else {
                return Err(format!(
                    "live shared ALT refresh failed and no manifest-matched disk snapshot is available: {live_error}"
                ));
            };
            eprintln!(
                "[execution-engine][alt-cache] live shared ALT refresh failed for {}; using manifest-matched disk fallback: {}",
                address, live_error
            );
            if let Ok(mut cache) = shared_lookup_table_cache().lock() {
                cache.insert(
                    address,
                    SharedLookupTableCacheEntry {
                        table: account.clone(),
                    },
                );
            }
            Ok(vec![account])
        }
    }
}

fn compile_pump_transaction_candidate(
    label: &str,
    blockhash: solana_sdk::hash::Hash,
    signers: &[&Keypair],
    instructions: &[Instruction],
    lookup_tables: &[AddressLookupTableAccount],
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    inline_tip_lamports: Option<u64>,
    inline_tip_account: Option<String>,
) -> Result<CompiledTxCandidate, String> {
    if lookup_tables.is_empty() {
        return Err(format!(
            "Pump compilation requires the shared ALT {SHARED_SUPER_LOOKUP_TABLE}."
        ));
    }
    let payer = signers
        .first()
        .ok_or_else(|| "Pump transaction compile requires at least one signer.".to_string())?;
    let message = v0::Message::try_compile(&payer.pubkey(), instructions, lookup_tables, blockhash)
        .map_err(|error| format!("Failed to compile Pump v0-alt message: {error}"))?;
    let lookup_tables_used = message
        .address_table_lookups
        .iter()
        .map(|lookup| lookup.account_key.to_string())
        .collect::<Vec<_>>();
    if lookup_tables_used.len() != 1 || lookup_tables_used[0] != SHARED_SUPER_LOOKUP_TABLE {
        return Err(format!(
            "Pump v0-alt compilation must actually use the shared ALT {SHARED_SUPER_LOOKUP_TABLE}; used [{}].",
            lookup_tables_used.join(", ")
        ));
    }
    let message_for_diagnostics = message.clone();
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), signers)
        .map_err(|error| format!("Failed to sign Pump v0-alt transaction: {error}"))?;
    let signature = transaction
        .signatures
        .first()
        .map(|value| value.to_string())
        .ok_or_else(|| "Pump v0-alt transaction did not include a signature.".to_string())?;
    let serialized = bincode::serialize(&transaction)
        .map_err(|error| format!("Failed to serialize Pump v0-alt transaction: {error}"))?;
    let serialized_len = serialized.len();
    let serialized_base64 = BASE64.encode(serialized);
    compiled_transaction_signers::remember_compiled_transaction_signers(
        &serialized_base64,
        &signers[1..],
    );
    let extra_manifest_entries = pump_apr28_dynamic_alt_manifest_entries()?;
    crate::alt_diagnostics::emit_alt_coverage_diagnostics(
        "execution-engine",
        label,
        instructions,
        lookup_tables,
        &message_for_diagnostics,
        Some(serialized_len),
        &extra_manifest_entries,
    );
    Ok(CompiledTxCandidate {
        compiled: CompiledTransaction {
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
        },
    })
}

async fn build_pump_transaction_with_lookup_preference(
    rpc_url: &str,
    label: &str,
    blockhash: solana_sdk::hash::Hash,
    signers: &[&Keypair],
    instructions: &[Instruction],
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    inline_tip_lamports: Option<u64>,
    inline_tip_account: Option<String>,
) -> Result<CompiledTransaction, String> {
    let lookup_tables = load_shared_super_lookup_tables(rpc_url).await?;
    compile_pump_transaction_candidate(
        label,
        blockhash,
        signers,
        instructions,
        &lookup_tables,
        compute_unit_limit,
        compute_unit_price_micro_lamports,
        inline_tip_lamports,
        inline_tip_account,
    )
    .map(|candidate| candidate.compiled)
}

async fn compile_pump_amm_trade(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    owner: Keypair,
    mint: Pubkey,
    _launch_creator: Pubkey,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    let owner_pubkey = owner.pubkey();
    // Parse the caller-pinned pool set by pair-address classification. It
    // must still match the canonical Pump AMM pool before execution.
    let pinned_pool_pubkey = match request.pinned_pool.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => Some(parse_pubkey(value, "pinned pool")?),
        _ => None,
    };
    let (global_config, pool, fee_config, base_mint_supply, base_mint_decimals, mint_token_program) =
        if let Some(PlannerRuntimeBundle::PumpAmm(bundle)) = selector.runtime_bundle.as_ref() {
            let mint_account = fetch_account_data(rpc_url, &mint.to_string(), "confirmed").await?;
            let pool = PumpAmmPoolState {
                pubkey: parse_pubkey(&bundle.pool, "pump amm pool")?,
                creator: parse_pubkey(&bundle.pool_creator, "pump amm pool creator")?,
                base_mint: parse_pubkey(&bundle.base_mint, "pump amm base mint")?,
                quote_mint: parse_pubkey(&bundle.quote_mint, "pump amm quote mint")?,
                pool_base_token_account: parse_pubkey(
                    &bundle.pool_base_token_account,
                    "pump amm pool base token account",
                )?,
                pool_quote_token_account: parse_pubkey(
                    &bundle.pool_quote_token_account,
                    "pump amm pool quote token account",
                )?,
                coin_creator: parse_pubkey(&bundle.coin_creator, "pump amm coin creator")?,
                is_mayhem_mode: bundle.is_mayhem_mode,
                is_cashback_coin: bundle.is_cashback_coin,
            };
            (
                fetch_pump_amm_global_config(rpc_url).await?,
                pool,
                fetch_pump_amm_fee_config(rpc_url).await?,
                read_mint_supply(&mint_account)?,
                read_mint_decimals(&mint_account)?,
                parse_pubkey(&bundle.mint_token_program, "pump amm mint token program")?,
            )
        } else {
            let (global_config, pool, fee_config, base_mint_supply, base_mint_decimals) =
                fetch_pump_amm_runtime(rpc_url, &mint, pinned_pool_pubkey.as_ref()).await?;
            let mint_token_program =
                fetch_account_owner_and_data(rpc_url, &mint.to_string(), "confirmed")
                    .await?
                    .map(|(owner, _)| owner)
                    .ok_or_else(|| format!("Mint account {mint} was not found."))?;
            (
                global_config,
                pool,
                fee_config,
                base_mint_supply,
                base_mint_decimals,
                mint_token_program,
            )
        };
    let quote_meta = pump_quote_asset_meta(&pool.quote_mint)?;
    let reserve_accounts = vec![
        pool.pool_base_token_account.to_string(),
        pool.pool_quote_token_account.to_string(),
    ];
    let reserve_datas =
        fetch_multiple_account_data(rpc_url, &reserve_accounts, "confirmed").await?;
    if reserve_datas.len() != reserve_accounts.len() {
        return Err(format!(
            "Pump AMM reserve batch returned {} accounts for {} requested reserves.",
            reserve_datas.len(),
            reserve_accounts.len()
        ));
    }
    let base_reserve_data = reserve_datas[0].as_ref().ok_or_else(|| {
        format!(
            "Pump AMM base reserve account {} was not found.",
            reserve_accounts[0]
        )
    })?;
    let quote_reserve_data = reserve_datas[1].as_ref().ok_or_else(|| {
        format!(
            "Pump AMM quote reserve account {} was not found.",
            reserve_accounts[1]
        )
    })?;
    let base_reserve = read_token_account_amount(base_reserve_data)?;
    let quote_reserve = read_token_account_amount(quote_reserve_data)?;
    if base_reserve == 0 || quote_reserve == 0 {
        return Err("Pump AMM pool reserves are empty.".to_string());
    }

    let fees = compute_pump_amm_fees(
        &global_config,
        fee_config.as_ref(),
        &pool,
        base_mint_supply,
        base_reserve,
        quote_reserve,
    )?;

    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let compute_unit_limit = match request.side {
        TradeSide::Buy => PUMP_AMM_BUY_COMPUTE_UNIT_LIMIT,
        TradeSide::Sell => PUMP_AMM_SELL_COMPUTE_UNIT_LIMIT,
    };
    let compute_unit_price_micro_lamports =
        priority_fee_sol_to_micro_lamports(&request.policy.fee_sol)?;
    let jitodontfront_enabled =
        matches!(request.policy.mev_mode, MevMode::Reduced | MevMode::Secure);

    let user_base_token_account =
        get_associated_token_address_with_program_id(&owner_pubkey, &mint, &mint_token_program);
    let temp_quote_account = Keypair::new();
    let temp_quote_account_pubkey = temp_quote_account.pubkey();
    let quote_account_rent_lamports = shared_warming_service()
        .minimum_balance_for_rent_exemption(SPL_TOKEN_ACCOUNT_LEN, || async {
            fetch_minimum_balance_for_rent_exemption(
                rpc_url,
                &request.policy.commitment,
                SPL_TOKEN_ACCOUNT_LEN,
            )
            .await
        })
        .await?;

    let (
        protocol_fee_recipient,
        protocol_fee_recipient_token_account,
        coin_creator_vault_authority,
        coin_creator_vault_ata,
    ) = if let Some(PlannerRuntimeBundle::PumpAmm(bundle)) = selector.runtime_bundle.as_ref() {
        (
            parse_pubkey(
                &bundle.protocol_fee_recipient,
                "pump amm protocol fee recipient",
            )?,
            parse_pubkey(
                &bundle.protocol_fee_recipient_token_account,
                "pump amm protocol fee recipient token account",
            )?,
            parse_pubkey(
                &bundle.coin_creator_vault_authority,
                "pump amm coin creator vault authority",
            )?,
            parse_pubkey(
                &bundle.coin_creator_vault_ata,
                "pump amm coin creator vault ata",
            )?,
        )
    } else {
        let protocol_fee_recipient =
            select_pump_amm_fee_recipient(&global_config, pool.is_mayhem_mode);
        let protocol_fee_recipient_token_account = get_associated_token_address_with_program_id(
            &protocol_fee_recipient,
            &quote_meta.mint,
            &quote_meta.token_program,
        );
        let coin_creator_vault_authority =
            pump_amm_coin_creator_vault_authority_pda(&pool.coin_creator);
        let coin_creator_vault_ata = get_associated_token_address_with_program_id(
            &coin_creator_vault_authority,
            &quote_meta.mint,
            &quote_meta.token_program,
        );
        (
            protocol_fee_recipient,
            protocol_fee_recipient_token_account,
            coin_creator_vault_authority,
            coin_creator_vault_ata,
        )
    };
    if matches!(quote_meta.kind, PumpQuoteAssetKind::Usdc) && matches!(request.side, TradeSide::Buy)
    {
        return compile_pump_usdc_amm_buy_from_sol_route(
            rpc_url,
            request,
            &owner,
            &owner_pubkey,
            &mint,
            &pool,
            &mint_token_program,
            base_reserve,
            quote_reserve,
            fees,
            pool.coin_creator != Pubkey::default(),
            compute_unit_limit,
            compute_unit_price_micro_lamports,
            jitodontfront_enabled,
            protocol_fee_recipient,
            protocol_fee_recipient_token_account,
            coin_creator_vault_ata,
            coin_creator_vault_authority,
        )
        .await;
    }
    if matches!(quote_meta.kind, PumpQuoteAssetKind::Usdc) {
        return compile_pump_usdc_amm_sell_to_sol_route(
            rpc_url,
            request,
            &owner,
            &owner_pubkey,
            &mint,
            &pool,
            &mint_token_program,
            base_mint_supply,
            base_mint_decimals,
            base_reserve,
            quote_reserve,
            fees,
            compute_unit_limit,
            compute_unit_price_micro_lamports,
            jitodontfront_enabled,
            protocol_fee_recipient,
            protocol_fee_recipient_token_account,
            coin_creator_vault_ata,
            coin_creator_vault_authority,
            wallet_key,
        )
        .await;
    }

    let mut instructions = vec![build_compute_unit_limit_instruction(compute_unit_limit)?];
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }
    append_pump_amm_setup_instructions(
        rpc_url,
        &mut instructions,
        &pool,
        &owner_pubkey,
        matches!(request.side, TradeSide::Buy) && pool.is_cashback_coin,
    )
    .await?;

    if matches!(request.side, TradeSide::Buy) {
        instructions.push(build_create_generic_ata_instruction(
            &owner_pubkey,
            &mint,
            &mint_token_program,
        )?);
        let spendable_quote_in = parse_decimal_units(
            request
                .buy_amount_sol
                .as_deref()
                .ok_or_else(|| "Missing buyAmountSol for buy request.".to_string())?,
            9,
            "buyAmountSol",
        )?;
        if spendable_quote_in == 0 {
            return Err("Buy amount must be greater than zero.".to_string());
        }
        let base_amount_out = pump_amm_buy_quote_input(
            spendable_quote_in,
            base_reserve,
            quote_reserve,
            fees,
            pool.coin_creator != Pubkey::default(),
        );
        if base_amount_out == 0 {
            return Err("Pump AMM native buy quote resolved to zero tokens.".to_string());
        }
        let max_quote_amount_in =
            apply_buy_slippage_buffer(spendable_quote_in, u64::from(slippage_bps));
        eprintln!(
            "[execution-engine][pump-amm] quote_in={} base_out={} max_quote={} base_reserve={} quote_reserve={} fees(lp={},protocol={},creator={}) mint={}",
            spendable_quote_in,
            base_amount_out,
            max_quote_amount_in,
            base_reserve,
            quote_reserve,
            fees.lp_fee_bps,
            fees.protocol_fee_bps,
            fees.creator_fee_bps,
            mint
        );
        instructions.extend(build_wrapped_sol_open_instructions(
            &owner_pubkey,
            &temp_quote_account_pubkey,
            quote_account_rent_lamports
                .checked_add(spendable_quote_in)
                .ok_or_else(|| "Wrapped SOL funding overflowed.".to_string())?,
            true,
        )?);
        instructions.push(build_pump_amm_buy_exact_quote_in_instruction(
            &pool,
            &owner_pubkey,
            &user_base_token_account,
            &temp_quote_account_pubkey,
            &protocol_fee_recipient,
            &protocol_fee_recipient_token_account,
            &coin_creator_vault_ata,
            &coin_creator_vault_authority,
            &mint_token_program,
            spendable_quote_in,
            apply_sell_side_slippage(base_amount_out, slippage_bps),
            pool.is_cashback_coin,
        )?);
    } else {
        let sell_intent = request
            .sell_intent
            .as_ref()
            .ok_or_else(|| "Missing sell intent for sell request.".to_string())?;
        instructions.extend(build_wrapped_sol_open_instructions(
            &owner_pubkey,
            &temp_quote_account_pubkey,
            quote_account_rent_lamports,
            false,
        )?);
        let base_amount_in = resolve_pump_amm_sell_input_amount(
            sell_intent,
            wallet_key,
            &owner_pubkey.to_string(),
            &request.mint,
            base_mint_supply,
            base_mint_decimals,
            base_reserve,
            quote_reserve,
            fees,
            pool.coin_creator != Pubkey::default(),
            quote_meta.decimals,
        )
        .await?;
        let quote_amount_out = pump_amm_sell_base_input(
            base_amount_in,
            base_reserve,
            quote_reserve,
            fees,
            pool.coin_creator != Pubkey::default(),
        )?;
        let min_quote_amount_out = apply_sell_side_slippage(quote_amount_out, slippage_bps);
        instructions.push(build_pump_amm_sell_instruction(
            &pool,
            &owner_pubkey,
            &user_base_token_account,
            &temp_quote_account_pubkey,
            &protocol_fee_recipient,
            &protocol_fee_recipient_token_account,
            &coin_creator_vault_ata,
            &coin_creator_vault_authority,
            &mint_token_program,
            base_amount_in,
            min_quote_amount_out,
            pool.is_cashback_coin,
        )?);
    }

    instructions.push(build_wrapped_sol_close_instruction(
        &owner_pubkey,
        &temp_quote_account_pubkey,
    )?);
    let (inline_tip_lamports, inline_tip_account) = append_inline_tip(
        &mut instructions,
        &owner_pubkey,
        &request.policy.provider,
        &request.policy.tip_sol,
    )?;
    if matches!(request.policy.mev_mode, MevMode::Reduced | MevMode::Secure) {
        apply_jitodontfront(&mut instructions, &owner_pubkey)?;
    }

    let label = match request.side {
        TradeSide::Buy => "pump-amm-buy",
        TradeSide::Sell => "pump-amm-sell",
    };
    instructions.push(build_uniqueness_memo_instruction(label)?);
    let blockhash = shared_warming_service()
        .latest_blockhash(rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let compiled = build_pump_transaction_with_lookup_preference(
        rpc_url,
        label,
        blockhash,
        &[&owner, &temp_quote_account],
        &instructions,
        compute_unit_limit,
        compute_unit_price_micro_lamports,
        inline_tip_lamports,
        inline_tip_account,
    )
    .await?;

    Ok(CompiledAdapterTrade {
        transactions: vec![compiled],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: None,
    })
}

fn route_account_index(
    route_accounts: &[AccountMeta],
    pubkey: &Pubkey,
    context: &str,
) -> Result<u16, String> {
    route_accounts
        .iter()
        .position(|meta| meta.pubkey == *pubkey)
        .ok_or_else(|| format!("{context} account was not present in wrapper route accounts"))?
        .try_into()
        .map_err(|_| format!("{context} account index does not fit in u16"))
}

fn route_len_u16(len: usize, context: &str) -> Result<u16, String> {
    len.try_into()
        .map_err(|_| format!("{context} route account count does not fit in u16"))
}

fn route_fee_lamports_floor(gross_lamports: u64, fee_bps: u16) -> Result<u64, String> {
    gross_lamports
        .checked_mul(u64::from(fee_bps))
        .ok_or_else(|| "Pump wrapper route fee calculation overflowed".to_string())
        .map(|value| value / 10_000)
}

#[allow(clippy::too_many_arguments)]
fn build_pump_usdc_sell_to_sol_route_instruction(
    owner: &Pubkey,
    pump_ix: Instruction,
    pump_input_amount: u64,
    quote_amount_in: u64,
    user_usdc_account: &Pubkey,
    unwind: crate::bonk_execution_support::TrustedRaydiumClmmSwap,
    route_wsol_account: &Pubkey,
    fee_bps: u16,
    label: &str,
) -> Result<Instruction, String> {
    let mut route_accounts = vec![
        AccountMeta::new_readonly(pump_ix.program_id, false),
        AccountMeta::new_readonly(unwind.instruction.program_id, false),
    ];
    let pump_program_index = 0u16;
    let unwind_program_index = 1u16;
    let pump_accounts_start = route_len_u16(route_accounts.len(), label)?;
    route_accounts.extend(pump_ix.accounts.iter().cloned());
    let pump_accounts_len = route_len_u16(pump_ix.accounts.len(), label)?;
    let pump_output_index =
        route_account_index(route_accounts.as_slice(), user_usdc_account, label)?;
    let unwind_accounts_start = route_len_u16(route_accounts.len(), label)?;
    route_accounts.extend(unwind.instruction.accounts.iter().cloned());
    let unwind_accounts_len = route_len_u16(unwind.instruction.accounts.len(), label)?;
    let unwind_output_index =
        route_account_index(route_accounts.as_slice(), route_wsol_account, label)?;
    let min_net_sol_out = unwind
        .min_out
        .checked_sub(route_fee_lamports_floor(unwind.min_out, fee_bps)?)
        .ok_or_else(|| "Pump USDC sell unwind minimum output fee underflowed".to_string())?;
    let fee_vault = wrapper_fee_vault_pubkey();
    let fee_vault_wsol_ata = get_associated_token_address_with_program_id(
        &fee_vault,
        &wsol_mint()?,
        &WRAPPER_TOKEN_PROGRAM_ID,
    );
    let config_pda_pubkey = config_pda().0;
    let instructions_sysvar = instructions_sysvar_id();
    let execute_accounts = ExecuteAccounts {
        user: owner,
        config_pda: &config_pda_pubkey,
        fee_vault: &fee_vault,
        fee_vault_wsol_ata: &fee_vault_wsol_ata,
        user_wsol_ata: route_wsol_account,
        instructions_sysvar: &instructions_sysvar,
        inner_program: &pump_ix.program_id,
        token_program: &WRAPPER_TOKEN_PROGRAM_ID,
    };
    let swap_route_accounts = ExecuteSwapRouteAccounts {
        execute: execute_accounts,
        token_fee_vault_ata: None,
    };
    let wrapper_request = ExecuteSwapRouteRequest {
        version: WRAPPER_ABI_VERSION,
        route_mode: SwapRouteMode::Mixed,
        direction: SwapRouteDirection::Sell,
        settlement: SwapRouteSettlement::Wsol,
        fee_mode: SwapRouteFeeMode::WsolPost,
        wsol_lane: 0,
        fee_bps,
        gross_sol_in_lamports: 0,
        gross_token_in_amount: 0,
        min_net_output: min_net_sol_out,
        route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
            + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
        intermediate_account_index: pump_output_index,
        token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        legs: vec![
            SwapRouteLeg {
                program_account_index: pump_program_index,
                accounts_start: pump_accounts_start,
                accounts_len: pump_accounts_len,
                input_source: SwapLegInputSource::Fixed,
                input_amount: pump_input_amount,
                input_patch_offset: SWAP_ROUTE_NO_PATCH_OFFSET,
                output_account_index: pump_output_index,
                ix_data: pump_ix.data,
            },
            SwapRouteLeg {
                program_account_index: unwind_program_index,
                accounts_start: unwind_accounts_start,
                accounts_len: unwind_accounts_len,
                input_source: SwapLegInputSource::PreviousTokenDelta,
                input_amount: quote_amount_in,
                input_patch_offset: 8,
                output_account_index: unwind_output_index,
                ix_data: unwind.instruction.data,
            },
        ],
    };
    build_execute_swap_route_instruction(&swap_route_accounts, &wrapper_request, &route_accounts)
}

#[allow(clippy::too_many_arguments)]
async fn compile_pump_usdc_bonding_buy_from_sol_route(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    owner: &Keypair,
    owner_pubkey: &Pubkey,
    mint: &Pubkey,
    base_token_program: &Pubkey,
    gross_sol_in_lamports: u64,
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    jitodontfront_enabled: bool,
) -> Result<CompiledAdapterTrade, String> {
    let fee_bps = wrapper_default_fee_bps();
    let net_sol_in_lamports = gross_sol_in_lamports
        .checked_sub(estimate_sol_in_fee_lamports(gross_sol_in_lamports, fee_bps))
        .ok_or_else(|| "Pump USDC route wrapper fee exceeds gross SOL input.".to_string())?;
    if net_sol_in_lamports == 0 {
        return Err(
            "Pump USDC route net SOL input resolves to zero after wrapper fee.".to_string(),
        );
    }
    let stable_route = trusted_stable_routes()
        .iter()
        .find(|route| route.label == "raydium-wsol-usdc")
        .ok_or_else(|| "Trusted Raydium SOL/USDC route is not configured.".to_string())?;
    let route_wsol_account = route_wsol_pda(owner_pubkey, 0).0;
    let usdc_mint = usdc_mint()?;
    let quote_token_program = token_program_id()?;
    let user_usdc_account = get_associated_token_address_with_program_id(
        owner_pubkey,
        &usdc_mint,
        &quote_token_program,
    );
    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let (conversion_slippage_bps, pump_slippage_bps) = split_two_leg_slippage_bps(slippage_bps);
    let conversion = build_trusted_raydium_clmm_swap_exact_in(
        rpc_url,
        stable_route.pool,
        &request.policy.commitment,
        owner_pubkey,
        &route_wsol_account,
        &user_usdc_account,
        &wsol_mint()?,
        &usdc_mint,
        net_sol_in_lamports,
        u64::from(conversion_slippage_bps),
    )
    .await?;
    let global = fetch_global_state(rpc_url).await?;
    let curve = fetch_bonding_curve_state(rpc_url, mint, &request.policy.commitment).await?;
    let creator_vault_authority =
        resolve_follow_creator_vault_authority(rpc_url, mint, &curve.creator).await?;
    let quote_meta = pump_quote_asset_meta(&curve.quote_mint)?;
    if !matches!(quote_meta.kind, PumpQuoteAssetKind::Usdc) {
        return Err("Pump USDC route was selected for a non-USDC bonding curve.".to_string());
    }
    let expected_tokens = quote_buy_tokens_from_curve(&curve, &global, conversion.min_out);
    if expected_tokens == 0 {
        return Err("Pump USDC buy route quote resolved to zero tokens.".to_string());
    }
    let pump_ix = build_buy_exact_quote_in_v2_instruction(
        &global,
        mint,
        &creator_vault_authority,
        owner_pubkey,
        conversion.min_out,
        apply_buy_token_slippage(expected_tokens, u64::from(pump_slippage_bps)),
        base_token_program,
        &quote_meta,
        curve.is_mayhem_mode,
    )?;
    let user_base_account =
        get_associated_token_address_with_program_id(owner_pubkey, mint, base_token_program);
    let mut route_accounts = vec![
        AccountMeta::new_readonly(conversion.instruction.program_id, false),
        AccountMeta::new_readonly(pump_ix.program_id, false),
    ];
    let conversion_program_index = 0u16;
    let pump_program_index = 1u16;
    let conversion_accounts_start =
        route_len_u16(route_accounts.len(), "Pump USDC conversion route leg")?;
    route_accounts.extend(conversion.instruction.accounts.iter().cloned());
    let conversion_accounts_len = route_len_u16(
        conversion.instruction.accounts.len(),
        "Pump USDC conversion route leg",
    )?;
    let conversion_output_index = route_account_index(
        &route_accounts,
        &user_usdc_account,
        "Pump USDC conversion output",
    )?;
    let pump_accounts_start = route_len_u16(route_accounts.len(), "Pump USDC buy route leg")?;
    route_accounts.extend(pump_ix.accounts.iter().cloned());
    let pump_accounts_len = route_len_u16(pump_ix.accounts.len(), "Pump USDC buy route leg")?;
    let pump_output_index =
        route_account_index(&route_accounts, &user_base_account, "Pump USDC buy output")?;
    let fee_vault = wrapper_fee_vault_pubkey();
    let zeroed_wsol = Pubkey::new_from_array([0; 32]);
    let config_pda_pubkey = config_pda().0;
    let instructions_sysvar = instructions_sysvar_id();
    let execute_accounts = ExecuteAccounts {
        user: owner_pubkey,
        config_pda: &config_pda_pubkey,
        fee_vault: &fee_vault,
        fee_vault_wsol_ata: &zeroed_wsol,
        user_wsol_ata: &route_wsol_account,
        instructions_sysvar: &instructions_sysvar,
        inner_program: &conversion.instruction.program_id,
        token_program: &WRAPPER_TOKEN_PROGRAM_ID,
    };
    let swap_route_accounts = ExecuteSwapRouteAccounts {
        execute: execute_accounts,
        token_fee_vault_ata: None,
    };
    let wrapper_request = ExecuteSwapRouteRequest {
        version: WRAPPER_ABI_VERSION,
        route_mode: SwapRouteMode::Mixed,
        direction: SwapRouteDirection::Buy,
        settlement: SwapRouteSettlement::Token,
        fee_mode: SwapRouteFeeMode::SolPre,
        wsol_lane: 0,
        fee_bps,
        gross_sol_in_lamports,
        gross_token_in_amount: 0,
        min_net_output: apply_buy_token_slippage(expected_tokens, u64::from(pump_slippage_bps)),
        route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
            + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
        intermediate_account_index: conversion_output_index,
        token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        legs: vec![
            SwapRouteLeg {
                program_account_index: conversion_program_index,
                accounts_start: conversion_accounts_start,
                accounts_len: conversion_accounts_len,
                input_source: SwapLegInputSource::GrossSolNetOfFee,
                input_amount: net_sol_in_lamports,
                input_patch_offset: 8,
                output_account_index: conversion_output_index,
                ix_data: conversion.instruction.data,
            },
            SwapRouteLeg {
                program_account_index: pump_program_index,
                accounts_start: pump_accounts_start,
                accounts_len: pump_accounts_len,
                input_source: SwapLegInputSource::PreviousTokenDelta,
                input_amount: conversion.min_out,
                input_patch_offset: 8,
                output_account_index: pump_output_index,
                ix_data: pump_ix.data,
            },
        ],
    };
    let wrapper_ix = build_execute_swap_route_instruction(
        &swap_route_accounts,
        &wrapper_request,
        &route_accounts,
    )?;
    let mut instructions = vec![build_compute_unit_limit_instruction(compute_unit_limit)?];
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }
    instructions.push(build_create_generic_ata_instruction(
        owner_pubkey,
        &usdc_mint,
        &quote_token_program,
    )?);
    instructions.push(build_create_generic_ata_instruction(
        owner_pubkey,
        mint,
        base_token_program,
    )?);
    instructions.push(wrapper_ix);
    if jitodontfront_enabled {
        apply_jitodontfront(&mut instructions, owner_pubkey)?;
    }
    let (inline_tip_lamports, inline_tip_account) = append_inline_tip(
        &mut instructions,
        owner_pubkey,
        &request.policy.provider,
        &request.policy.tip_sol,
    )?;
    instructions.push(build_uniqueness_memo_instruction("pump-usdc-bonding-buy")?);
    let blockhash = shared_warming_service()
        .latest_blockhash(rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let compiled = build_pump_transaction_with_lookup_preference(
        rpc_url,
        "pump-usdc-bonding-buy",
        blockhash,
        &[owner],
        &instructions,
        compute_unit_limit,
        compute_unit_price_micro_lamports,
        inline_tip_lamports,
        inline_tip_account,
    )
    .await?;
    Ok(CompiledAdapterTrade {
        transactions: vec![compiled],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: None,
    })
}

#[allow(clippy::too_many_arguments)]
async fn compile_pump_usdc_bonding_sell_to_sol_route(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    owner: &Keypair,
    owner_pubkey: &Pubkey,
    mint: &Pubkey,
    base_token_program: &Pubkey,
    wallet_key: &str,
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    jitodontfront_enabled: bool,
) -> Result<CompiledAdapterTrade, String> {
    let stable_route = trusted_stable_routes()
        .iter()
        .find(|route| route.label == "raydium-wsol-usdc")
        .ok_or_else(|| "Trusted Raydium SOL/USDC route is not configured.".to_string())?;
    let usdc_mint = usdc_mint()?;
    let wsol_mint = wsol_mint()?;
    let quote_token_program = token_program_id()?;
    let user_usdc_account = get_associated_token_address_with_program_id(
        owner_pubkey,
        &usdc_mint,
        &quote_token_program,
    );
    let route_wsol_account = route_wsol_pda(owner_pubkey, 0).0;
    let global = fetch_global_state(rpc_url).await?;
    let curve = fetch_bonding_curve_state(rpc_url, mint, &request.policy.commitment).await?;
    let quote_meta = pump_quote_asset_meta(&curve.quote_mint)?;
    let creator_vault_authority =
        resolve_follow_creator_vault_authority(rpc_url, mint, &curve.creator).await?;
    if !matches!(quote_meta.kind, PumpQuoteAssetKind::Usdc) {
        return Err("Pump USDC sell route was selected for a non-USDC bonding curve.".to_string());
    }
    let token_amount = resolve_sell_token_amount(
        request
            .sell_intent
            .as_ref()
            .ok_or_else(|| "Missing sell intent for sell request.".to_string())?,
        wallet_key,
        &owner_pubkey.to_string(),
        &request.mint,
        &curve,
        &global,
    )
    .await?;
    let expected_usdc_out = quote_sell_quote_from_curve(&curve, &global, token_amount);
    if expected_usdc_out == 0 {
        return Err("Pump USDC sell route quote resolved to zero USDC.".to_string());
    }
    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let (pump_slippage_bps, unwind_slippage_bps) = split_two_leg_slippage_bps(slippage_bps);
    let min_usdc_out = apply_sell_side_slippage(expected_usdc_out, pump_slippage_bps);
    let pump_ix = build_sell_v2_instruction(
        &global,
        mint,
        &creator_vault_authority,
        owner_pubkey,
        token_amount,
        min_usdc_out,
        base_token_program,
        &quote_meta,
        curve.cashback_enabled,
        curve.is_mayhem_mode,
    )?;
    let unwind = build_trusted_raydium_clmm_swap_exact_in(
        rpc_url,
        stable_route.pool,
        &request.policy.commitment,
        owner_pubkey,
        &user_usdc_account,
        &route_wsol_account,
        &usdc_mint,
        &wsol_mint,
        expected_usdc_out,
        u64::from(unwind_slippage_bps),
    )
    .await?;
    let wrapper_ix = build_pump_usdc_sell_to_sol_route_instruction(
        owner_pubkey,
        pump_ix,
        token_amount,
        min_usdc_out,
        &user_usdc_account,
        unwind,
        &route_wsol_account,
        wrapper_default_fee_bps(),
        "Pump USDC bonding sell route",
    )?;
    let mut instructions = vec![build_compute_unit_limit_instruction(compute_unit_limit)?];
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }
    instructions.push(build_create_generic_ata_instruction(
        owner_pubkey,
        &usdc_mint,
        &quote_token_program,
    )?);
    instructions.push(build_create_generic_ata_instruction(
        owner_pubkey,
        mint,
        base_token_program,
    )?);
    instructions.push(wrapper_ix);
    if jitodontfront_enabled {
        apply_jitodontfront(&mut instructions, owner_pubkey)?;
    }
    let (inline_tip_lamports, inline_tip_account) = append_inline_tip(
        &mut instructions,
        owner_pubkey,
        &request.policy.provider,
        &request.policy.tip_sol,
    )?;
    instructions.push(build_uniqueness_memo_instruction("pump-usdc-bonding-sell")?);
    let blockhash = shared_warming_service()
        .latest_blockhash(rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let compiled = build_pump_transaction_with_lookup_preference(
        rpc_url,
        "pump-usdc-bonding-sell",
        blockhash,
        &[owner],
        &instructions,
        compute_unit_limit,
        compute_unit_price_micro_lamports,
        inline_tip_lamports,
        inline_tip_account,
    )
    .await?;
    Ok(CompiledAdapterTrade {
        transactions: vec![compiled],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: None,
    })
}

#[allow(clippy::too_many_arguments)]
async fn compile_pump_usdc_amm_buy_from_sol_route(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    owner: &Keypair,
    owner_pubkey: &Pubkey,
    mint: &Pubkey,
    pool: &PumpAmmPoolState,
    base_token_program: &Pubkey,
    base_reserve: u64,
    quote_reserve: u64,
    fees: PumpAmmFees,
    has_coin_creator: bool,
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    jitodontfront_enabled: bool,
    protocol_fee_recipient: Pubkey,
    protocol_fee_recipient_token_account: Pubkey,
    coin_creator_vault_ata: Pubkey,
    coin_creator_vault_authority: Pubkey,
) -> Result<CompiledAdapterTrade, String> {
    let gross_sol_in_lamports = parse_decimal_units(
        request
            .buy_amount_sol
            .as_deref()
            .ok_or_else(|| "Missing buyAmountSol for buy request.".to_string())?,
        9,
        "buyAmountSol",
    )?;
    let fee_bps = wrapper_default_fee_bps();
    let net_sol_in_lamports = gross_sol_in_lamports
        .checked_sub(estimate_sol_in_fee_lamports(gross_sol_in_lamports, fee_bps))
        .ok_or_else(|| "Pump AMM USDC route wrapper fee exceeds gross SOL input.".to_string())?;
    if net_sol_in_lamports == 0 {
        return Err(
            "Pump AMM USDC route net SOL input resolves to zero after wrapper fee.".to_string(),
        );
    }
    let stable_route = trusted_stable_routes()
        .iter()
        .find(|route| route.label == "raydium-wsol-usdc")
        .ok_or_else(|| "Trusted Raydium SOL/USDC route is not configured.".to_string())?;
    let route_wsol_account = route_wsol_pda(owner_pubkey, 0).0;
    let usdc_mint = usdc_mint()?;
    let quote_token_program = token_program_id()?;
    let user_usdc_account = get_associated_token_address_with_program_id(
        owner_pubkey,
        &usdc_mint,
        &quote_token_program,
    );
    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let (conversion_slippage_bps, pump_slippage_bps) = split_two_leg_slippage_bps(slippage_bps);
    let conversion = build_trusted_raydium_clmm_swap_exact_in(
        rpc_url,
        stable_route.pool,
        &request.policy.commitment,
        owner_pubkey,
        &route_wsol_account,
        &user_usdc_account,
        &wsol_mint()?,
        &usdc_mint,
        net_sol_in_lamports,
        u64::from(conversion_slippage_bps),
    )
    .await?;
    let base_amount_out = pump_amm_buy_quote_input(
        conversion.min_out,
        base_reserve,
        quote_reserve,
        fees,
        has_coin_creator,
    );
    if base_amount_out == 0 {
        return Err("Pump AMM USDC buy route quote resolved to zero tokens.".to_string());
    }
    let user_base_account =
        get_associated_token_address_with_program_id(owner_pubkey, mint, base_token_program);
    let pump_ix = build_pump_amm_buy_exact_quote_in_instruction(
        pool,
        owner_pubkey,
        &user_base_account,
        &user_usdc_account,
        &protocol_fee_recipient,
        &protocol_fee_recipient_token_account,
        &coin_creator_vault_ata,
        &coin_creator_vault_authority,
        base_token_program,
        conversion.min_out,
        apply_sell_side_slippage(base_amount_out, pump_slippage_bps),
        pool.is_cashback_coin,
    )?;
    let mut route_accounts = vec![
        AccountMeta::new_readonly(conversion.instruction.program_id, false),
        AccountMeta::new_readonly(pump_ix.program_id, false),
    ];
    let conversion_program_index = 0u16;
    let pump_program_index = 1u16;
    let conversion_accounts_start =
        route_len_u16(route_accounts.len(), "Pump AMM USDC conversion route leg")?;
    route_accounts.extend(conversion.instruction.accounts.iter().cloned());
    let conversion_accounts_len = route_len_u16(
        conversion.instruction.accounts.len(),
        "Pump AMM USDC conversion route leg",
    )?;
    let conversion_output_index = route_account_index(
        &route_accounts,
        &user_usdc_account,
        "Pump AMM USDC conversion output",
    )?;
    let pump_accounts_start = route_len_u16(route_accounts.len(), "Pump AMM USDC buy route leg")?;
    route_accounts.extend(pump_ix.accounts.iter().cloned());
    let pump_accounts_len = route_len_u16(pump_ix.accounts.len(), "Pump AMM USDC buy route leg")?;
    let pump_output_index = route_account_index(
        &route_accounts,
        &user_base_account,
        "Pump AMM USDC buy output",
    )?;
    let fee_vault = wrapper_fee_vault_pubkey();
    let zeroed_wsol = Pubkey::new_from_array([0; 32]);
    let config_pda_pubkey = config_pda().0;
    let instructions_sysvar = instructions_sysvar_id();
    let execute_accounts = ExecuteAccounts {
        user: owner_pubkey,
        config_pda: &config_pda_pubkey,
        fee_vault: &fee_vault,
        fee_vault_wsol_ata: &zeroed_wsol,
        user_wsol_ata: &route_wsol_account,
        instructions_sysvar: &instructions_sysvar,
        inner_program: &conversion.instruction.program_id,
        token_program: &WRAPPER_TOKEN_PROGRAM_ID,
    };
    let swap_route_accounts = ExecuteSwapRouteAccounts {
        execute: execute_accounts,
        token_fee_vault_ata: None,
    };
    let wrapper_request = ExecuteSwapRouteRequest {
        version: WRAPPER_ABI_VERSION,
        route_mode: SwapRouteMode::Mixed,
        direction: SwapRouteDirection::Buy,
        settlement: SwapRouteSettlement::Token,
        fee_mode: SwapRouteFeeMode::SolPre,
        wsol_lane: 0,
        fee_bps,
        gross_sol_in_lamports,
        gross_token_in_amount: 0,
        min_net_output: apply_sell_side_slippage(base_amount_out, pump_slippage_bps),
        route_accounts_offset: EXECUTE_SWAP_ROUTE_FIXED_ACCOUNT_COUNT
            + EXECUTE_SWAP_ROUTE_WSOL_ACCOUNT_COUNT,
        intermediate_account_index: conversion_output_index,
        token_fee_account_index: SWAP_ROUTE_NO_PATCH_OFFSET,
        legs: vec![
            SwapRouteLeg {
                program_account_index: conversion_program_index,
                accounts_start: conversion_accounts_start,
                accounts_len: conversion_accounts_len,
                input_source: SwapLegInputSource::GrossSolNetOfFee,
                input_amount: net_sol_in_lamports,
                input_patch_offset: 8,
                output_account_index: conversion_output_index,
                ix_data: conversion.instruction.data,
            },
            SwapRouteLeg {
                program_account_index: pump_program_index,
                accounts_start: pump_accounts_start,
                accounts_len: pump_accounts_len,
                input_source: SwapLegInputSource::PreviousTokenDelta,
                input_amount: conversion.min_out,
                input_patch_offset: 8,
                output_account_index: pump_output_index,
                ix_data: pump_ix.data,
            },
        ],
    };
    let wrapper_ix = build_execute_swap_route_instruction(
        &swap_route_accounts,
        &wrapper_request,
        &route_accounts,
    )?;
    let mut instructions = vec![build_compute_unit_limit_instruction(compute_unit_limit)?];
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }
    append_pump_amm_setup_instructions(
        rpc_url,
        &mut instructions,
        pool,
        owner_pubkey,
        pool.is_cashback_coin,
    )
    .await?;
    instructions.push(build_create_generic_ata_instruction(
        owner_pubkey,
        &usdc_mint,
        &quote_token_program,
    )?);
    instructions.push(build_create_generic_ata_instruction(
        owner_pubkey,
        mint,
        base_token_program,
    )?);
    instructions.push(wrapper_ix);
    if jitodontfront_enabled {
        apply_jitodontfront(&mut instructions, owner_pubkey)?;
    }
    let (inline_tip_lamports, inline_tip_account) = append_inline_tip(
        &mut instructions,
        owner_pubkey,
        &request.policy.provider,
        &request.policy.tip_sol,
    )?;
    instructions.push(build_uniqueness_memo_instruction("pump-usdc-amm-buy")?);
    let blockhash = shared_warming_service()
        .latest_blockhash(rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let compiled = build_pump_transaction_with_lookup_preference(
        rpc_url,
        "pump-usdc-amm-buy",
        blockhash,
        &[owner],
        &instructions,
        compute_unit_limit,
        compute_unit_price_micro_lamports,
        inline_tip_lamports,
        inline_tip_account,
    )
    .await?;
    Ok(CompiledAdapterTrade {
        transactions: vec![compiled],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: None,
    })
}

#[allow(clippy::too_many_arguments)]
async fn compile_pump_usdc_amm_sell_to_sol_route(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    owner: &Keypair,
    owner_pubkey: &Pubkey,
    mint: &Pubkey,
    pool: &PumpAmmPoolState,
    base_token_program: &Pubkey,
    base_mint_supply: u64,
    base_mint_decimals: u8,
    base_reserve: u64,
    quote_reserve: u64,
    fees: PumpAmmFees,
    compute_unit_limit: u32,
    compute_unit_price_micro_lamports: u64,
    jitodontfront_enabled: bool,
    protocol_fee_recipient: Pubkey,
    protocol_fee_recipient_token_account: Pubkey,
    coin_creator_vault_ata: Pubkey,
    coin_creator_vault_authority: Pubkey,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    let stable_route = trusted_stable_routes()
        .iter()
        .find(|route| route.label == "raydium-wsol-usdc")
        .ok_or_else(|| "Trusted Raydium SOL/USDC route is not configured.".to_string())?;
    let usdc_mint = usdc_mint()?;
    let wsol_mint = wsol_mint()?;
    let quote_token_program = token_program_id()?;
    if pool.quote_mint != usdc_mint {
        return Err("Pump AMM USDC sell route was selected for a non-USDC pool.".to_string());
    }
    let user_usdc_account = get_associated_token_address_with_program_id(
        owner_pubkey,
        &usdc_mint,
        &quote_token_program,
    );
    let user_base_account =
        get_associated_token_address_with_program_id(owner_pubkey, mint, base_token_program);
    let route_wsol_account = route_wsol_pda(owner_pubkey, 0).0;
    let sell_intent = request
        .sell_intent
        .as_ref()
        .ok_or_else(|| "Missing sell intent for sell request.".to_string())?;
    let base_amount_in = resolve_pump_amm_sell_input_amount(
        sell_intent,
        wallet_key,
        &owner_pubkey.to_string(),
        &request.mint,
        base_mint_supply,
        base_mint_decimals,
        base_reserve,
        quote_reserve,
        fees,
        pool.coin_creator != Pubkey::default(),
        6,
    )
    .await?;
    let expected_usdc_out = pump_amm_sell_base_input(
        base_amount_in,
        base_reserve,
        quote_reserve,
        fees,
        pool.coin_creator != Pubkey::default(),
    )?;
    if expected_usdc_out == 0 {
        return Err("Pump AMM USDC sell route quote resolved to zero USDC.".to_string());
    }
    let slippage_bps = parse_slippage_bps(Some(request.policy.slippage_percent.as_str()))?;
    let (pump_slippage_bps, unwind_slippage_bps) = split_two_leg_slippage_bps(slippage_bps);
    let min_usdc_out = apply_sell_side_slippage(expected_usdc_out, pump_slippage_bps);
    let pump_ix = build_pump_amm_sell_instruction(
        pool,
        owner_pubkey,
        &user_base_account,
        &user_usdc_account,
        &protocol_fee_recipient,
        &protocol_fee_recipient_token_account,
        &coin_creator_vault_ata,
        &coin_creator_vault_authority,
        base_token_program,
        base_amount_in,
        min_usdc_out,
        pool.is_cashback_coin,
    )?;
    let unwind = build_trusted_raydium_clmm_swap_exact_in(
        rpc_url,
        stable_route.pool,
        &request.policy.commitment,
        owner_pubkey,
        &user_usdc_account,
        &route_wsol_account,
        &usdc_mint,
        &wsol_mint,
        expected_usdc_out,
        u64::from(unwind_slippage_bps),
    )
    .await?;
    let wrapper_ix = build_pump_usdc_sell_to_sol_route_instruction(
        owner_pubkey,
        pump_ix,
        base_amount_in,
        min_usdc_out,
        &user_usdc_account,
        unwind,
        &route_wsol_account,
        wrapper_default_fee_bps(),
        "Pump AMM USDC sell route",
    )?;
    let mut instructions = vec![build_compute_unit_limit_instruction(compute_unit_limit)?];
    if compute_unit_price_micro_lamports > 0 {
        instructions.push(build_compute_unit_price_instruction(
            compute_unit_price_micro_lamports,
        )?);
    }
    append_pump_amm_setup_instructions(rpc_url, &mut instructions, pool, owner_pubkey, false)
        .await?;
    instructions.push(build_create_generic_ata_instruction(
        owner_pubkey,
        &usdc_mint,
        &quote_token_program,
    )?);
    instructions.push(build_create_generic_ata_instruction(
        owner_pubkey,
        mint,
        base_token_program,
    )?);
    instructions.push(wrapper_ix);
    if jitodontfront_enabled {
        apply_jitodontfront(&mut instructions, owner_pubkey)?;
    }
    let (inline_tip_lamports, inline_tip_account) = append_inline_tip(
        &mut instructions,
        owner_pubkey,
        &request.policy.provider,
        &request.policy.tip_sol,
    )?;
    instructions.push(build_uniqueness_memo_instruction("pump-usdc-amm-sell")?);
    let blockhash = shared_warming_service()
        .latest_blockhash(rpc_url, &request.policy.commitment)
        .await?
        .blockhash;
    let compiled = build_pump_transaction_with_lookup_preference(
        rpc_url,
        "pump-usdc-amm-sell",
        blockhash,
        &[owner],
        &instructions,
        compute_unit_limit,
        compute_unit_price_micro_lamports,
        inline_tip_lamports,
        inline_tip_account,
    )
    .await?;
    Ok(CompiledAdapterTrade {
        transactions: vec![compiled],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: None,
    })
}

async fn fetch_pump_amm_runtime(
    rpc_url: &str,
    mint: &Pubkey,
    pinned_pool: Option<&Pubkey>,
) -> Result<
    (
        PumpAmmGlobalConfig,
        PumpAmmPoolState,
        Option<PumpAmmFeeConfig>,
        u64,
        u8,
    ),
    String,
> {
    let global_config = fetch_pump_amm_global_config(rpc_url).await?;
    let pool = find_pump_amm_pool_state(rpc_url, mint, pinned_pool, "confirmed")
        .await?
        .ok_or_else(|| match pinned_pool {
            Some(pinned) => {
                format!("Pinned Pump AMM pool {pinned} was not found on-chain for mint {mint}.")
            }
            None => format!("No supported Pump AMM pool was found for mint {mint}."),
        })?;
    let fee_config = fetch_pump_amm_fee_config(rpc_url).await?;
    // Read supply and decimals from the same on-chain mint account fetch.
    // Decimals flow into the wallet-token cache reconstruction so the
    // Pump AMM sell-sizing path stops hardcoding `6` — if Pump ever
    // supports a Token-2022 mint with different decimals, the cache
    // value will still round-trip correctly.
    let mint_account = fetch_account_data(rpc_url, &mint.to_string(), "confirmed").await?;
    let base_mint_supply = read_mint_supply(&mint_account)?;
    let base_mint_decimals = read_mint_decimals(&mint_account)?;
    Ok((
        global_config,
        pool,
        fee_config,
        base_mint_supply,
        base_mint_decimals,
    ))
}

async fn resolve_sell_token_amount(
    sell_intent: &RuntimeSellIntent,
    wallet_key: &str,
    owner: &str,
    mint: &str,
    curve: &PumpBondingCurveState,
    global: &PumpGlobalState,
) -> Result<u64, String> {
    // Bonding-curve Pump mints are fixed at 6 decimals by the
    // launch program — it initializes the mint with `decimals = 6`
    // and never updates them. We skip the extra `getAccountInfo` for
    // the mint account on this hot path and hardcode the value; if
    // Pump ever changes their bonding-curve layout this becomes a
    // compile-time-invisible footgun but the RPC fallback inside
    // `fetch_token_balance_with_cache` would still give a correct
    // balance (it re-reads decimals from on-chain).
    const PUMP_BONDING_DECIMALS: u8 = 6;
    let balance = crate::wallet_token_cache::fetch_token_balance_with_cache(
        Some(wallet_key),
        owner,
        mint,
        PUMP_BONDING_DECIMALS,
    )
    .await?;
    if balance.amount_raw == 0 {
        return Err("You have 0 tokens.".to_string());
    }
    let token_amount = match sell_intent {
        RuntimeSellIntent::Percent(value) => {
            let percent_bps = u128::from(parse_percent_to_bps(value)?);
            ((u128::from(balance.amount_raw) * percent_bps) / 10_000u128).min(u128::from(u64::MAX))
                as u64
        }
        RuntimeSellIntent::SolOutput(value) => {
            let quote_meta = pump_quote_asset_meta(&curve.quote_mint)?;
            if !matches!(quote_meta.kind, PumpQuoteAssetKind::Sol) {
                return Err(
                    "sellOutputSol is not supported for USDC-quoted Pump bonding curves yet; use sellPercent for this route."
                        .to_string(),
                );
            }
            let desired_output =
                parse_decimal_units(value, usize::from(quote_meta.decimals), "sellOutputSol")?;
            if desired_output == 0 {
                return Err("sellOutputSol must be greater than zero.".to_string());
            }
            crate::sell_target_sizing::choose_target_sized_token_amount(
                balance.amount_raw,
                desired_output,
                |amount| {
                    crate::sell_target_sizing::net_sol_after_wrapper_fee(
                        quote_sell_quote_from_curve(curve, global, amount),
                    )
                },
            )?
        }
    };
    if token_amount == 0 {
        return Err("Sell amount resolves to zero tokens.".to_string());
    }
    if token_amount > balance.amount_raw {
        return Err(format!(
            "Wallet balance is too small for the requested sell amount. Need {token_amount}, have {}.",
            balance.amount_raw
        ));
    }
    Ok(token_amount)
}

#[derive(Debug, Clone)]
enum CachedPumpQuoteSnapshot {
    BondingCurve {
        curve: PumpBondingCurveState,
        global: PumpGlobalState,
    },
    Amm {
        global_config: PumpAmmGlobalConfig,
        pool: PumpAmmPoolState,
        fee_config: Option<PumpAmmFeeConfig>,
        base_mint_supply: u64,
        base_reserve: u64,
        quote_reserve: u64,
    },
}

#[derive(Debug, Clone)]
struct CachedPumpQuoteSnapshotEntry {
    fetched_at: Instant,
    snapshot: CachedPumpQuoteSnapshot,
}

fn pump_quote_snapshot_cache()
-> &'static tokio::sync::Mutex<HashMap<String, CachedPumpQuoteSnapshotEntry>> {
    static CACHE: OnceLock<tokio::sync::Mutex<HashMap<String, CachedPumpQuoteSnapshotEntry>>> =
        OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn pump_quote_snapshot_ttl(selector: &LifecycleAndCanonicalMarket) -> Duration {
    match selector.lifecycle {
        TradeLifecycle::PreMigration => Duration::from_millis(1_500),
        TradeLifecycle::PostMigration => Duration::from_millis(3_000),
    }
}

fn pump_quote_snapshot_key(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    commitment: &str,
) -> String {
    format!(
        "rpc={}|cmt={}|family={}|market={}|quote={}|mint={}",
        rpc_url,
        commitment,
        selector.family.label(),
        selector.canonical_market_key,
        selector.quote_asset.label(),
        mint
    )
}

fn quote_pump_snapshot(
    snapshot: &CachedPumpQuoteSnapshot,
    token_amount_raw: u64,
) -> Result<(u64, PumpQuoteAssetKind), String> {
    match snapshot {
        CachedPumpQuoteSnapshot::BondingCurve { curve, global } => {
            let quote_meta = pump_quote_asset_meta(&curve.quote_mint)?;
            Ok((
                quote_sell_quote_from_curve(curve, global, token_amount_raw),
                quote_meta.kind,
            ))
        }
        CachedPumpQuoteSnapshot::Amm {
            global_config,
            pool,
            fee_config,
            base_mint_supply,
            base_reserve,
            quote_reserve,
        } => {
            let fees = compute_pump_amm_fees(
                global_config,
                fee_config.as_ref(),
                pool,
                *base_mint_supply,
                *base_reserve,
                *quote_reserve,
            )?;
            Ok((
                pump_amm_sell_base_input(
                    token_amount_raw,
                    *base_reserve,
                    *quote_reserve,
                    fees,
                    pool.coin_creator != Pubkey::default(),
                )?,
                pump_quote_asset_meta(&pool.quote_mint)?.kind,
            ))
        }
    }
}

async fn pump_quote_value_to_sol_lamports(
    rpc_url: &str,
    quote_amount: u64,
    quote_kind: PumpQuoteAssetKind,
    commitment: &str,
) -> Result<u64, String> {
    match quote_kind {
        PumpQuoteAssetKind::Sol => Ok(quote_amount),
        PumpQuoteAssetKind::Usdc => {
            if quote_amount == 0 {
                return Ok(0);
            }
            let stable_route = trusted_stable_routes()
                .iter()
                .find(|route| route.label == "raydium-wsol-usdc")
                .ok_or_else(|| "Trusted Raydium SOL/USDC route is not configured.".to_string())?;
            let quote = crate::bonk_execution_support::quote_trusted_raydium_clmm_exact_in(
                rpc_url,
                stable_route.pool,
                commitment,
                &usdc_mint()?,
                &wsol_mint()?,
                quote_amount,
                0,
            )
            .await?;
            Ok(quote.min_out)
        }
    }
}

pub(crate) async fn quote_pump_holding_value_sol(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
    commitment: &str,
) -> Result<u64, String> {
    quote_pump_holding_value_sol_with_cache(
        rpc_url,
        selector,
        mint,
        token_amount_raw,
        commitment,
        true,
    )
    .await
}

pub(crate) async fn quote_pump_holding_value_sol_fresh(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
    commitment: &str,
) -> Result<u64, String> {
    quote_pump_holding_value_sol_with_cache(
        rpc_url,
        selector,
        mint,
        token_amount_raw,
        commitment,
        false,
    )
    .await
}

async fn quote_pump_holding_value_sol_with_cache(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
    commitment: &str,
    use_cache: bool,
) -> Result<u64, String> {
    if token_amount_raw == 0 {
        return Ok(0);
    }
    let cache_key = pump_quote_snapshot_key(rpc_url, selector, mint, commitment);
    if use_cache {
        let cache_ttl = pump_quote_snapshot_ttl(selector);
        let cache = pump_quote_snapshot_cache().lock().await;
        if let Some(entry) = cache.get(&cache_key)
            && entry.fetched_at.elapsed() <= cache_ttl
        {
            let (quote_amount, quote_kind) =
                quote_pump_snapshot(&entry.snapshot, token_amount_raw)?;
            return pump_quote_value_to_sol_lamports(rpc_url, quote_amount, quote_kind, commitment)
                .await;
        }
    }
    let mint_pubkey = parse_pubkey(mint, "Pump quote mint")?;
    let snapshot = match selector.family {
        TradeVenueFamily::PumpBondingCurve => {
            let curve = fetch_bonding_curve_state(rpc_url, &mint_pubkey, commitment).await?;
            let global = fetch_global_state(rpc_url).await?;
            CachedPumpQuoteSnapshot::BondingCurve { curve, global }
        }
        TradeVenueFamily::PumpAmm => {
            let (global_config, pool, fee_config, base_mint_supply, _) =
                if let Some(PlannerRuntimeBundle::PumpAmm(bundle)) =
                    selector.runtime_bundle.as_ref()
                {
                    let mint_account =
                        fetch_account_data(rpc_url, &mint_pubkey.to_string(), commitment).await?;
                    (
                        fetch_pump_amm_global_config(rpc_url).await?,
                        PumpAmmPoolState {
                            pubkey: parse_pubkey(&bundle.pool, "pump amm pool")?,
                            creator: parse_pubkey(&bundle.pool_creator, "pump amm pool creator")?,
                            base_mint: parse_pubkey(&bundle.base_mint, "pump amm base mint")?,
                            quote_mint: parse_pubkey(&bundle.quote_mint, "pump amm quote mint")?,
                            pool_base_token_account: parse_pubkey(
                                &bundle.pool_base_token_account,
                                "pump amm pool base token account",
                            )?,
                            pool_quote_token_account: parse_pubkey(
                                &bundle.pool_quote_token_account,
                                "pump amm pool quote token account",
                            )?,
                            coin_creator: parse_pubkey(
                                &bundle.coin_creator,
                                "pump amm coin creator",
                            )?,
                            is_mayhem_mode: bundle.is_mayhem_mode,
                            is_cashback_coin: bundle.is_cashback_coin,
                        },
                        fetch_pump_amm_fee_config(rpc_url).await?,
                        read_mint_supply(&mint_account)?,
                        read_mint_decimals(&mint_account)?,
                    )
                } else {
                    fetch_pump_amm_runtime(rpc_url, &mint_pubkey, None).await?
                };
            pump_quote_asset_meta(&pool.quote_mint)?;
            let reserve_accounts = vec![
                pool.pool_base_token_account.to_string(),
                pool.pool_quote_token_account.to_string(),
            ];
            let reserve_datas =
                fetch_multiple_account_data(rpc_url, &reserve_accounts, commitment).await?;
            if reserve_datas.len() != reserve_accounts.len() {
                return Err(format!(
                    "Pump AMM reserve fetch returned {} accounts for {} requested accounts.",
                    reserve_datas.len(),
                    reserve_accounts.len()
                ));
            }
            let base_reserve =
                read_token_account_amount(reserve_datas[0].as_deref().ok_or_else(|| {
                    format!(
                        "Pump AMM base reserve account {} was not found.",
                        pool.pool_base_token_account
                    )
                })?)?;
            let quote_reserve =
                read_token_account_amount(reserve_datas[1].as_deref().ok_or_else(|| {
                    format!(
                        "Pump AMM quote reserve account {} was not found.",
                        pool.pool_quote_token_account
                    )
                })?)?;
            CachedPumpQuoteSnapshot::Amm {
                global_config,
                pool,
                fee_config,
                base_mint_supply,
                base_reserve,
                quote_reserve,
            }
        }
        _ => Err(format!(
            "Pump quote helper does not support family {}.",
            selector.family.label()
        ))?,
    };
    if use_cache {
        let mut cache = pump_quote_snapshot_cache().lock().await;
        cache.insert(
            cache_key,
            CachedPumpQuoteSnapshotEntry {
                fetched_at: Instant::now(),
                snapshot: snapshot.clone(),
            },
        );
        if cache.len() > 256 {
            cache.retain(|_, entry| entry.fetched_at.elapsed() <= Duration::from_secs(30));
        }
    }
    let (quote_amount, quote_kind) = quote_pump_snapshot(&snapshot, token_amount_raw)?;
    pump_quote_value_to_sol_lamports(rpc_url, quote_amount, quote_kind, commitment).await
}

#[cfg(test)]
fn required_tokens_for_net_sol_output(
    curve: &PumpBondingCurveState,
    global: &PumpGlobalState,
    desired_output: u64,
) -> Result<u64, String> {
    if desired_output >= curve.real_quote_reserves {
        return Err(
            "Requested sell output exceeds the Pump curve's available SOL reserves.".to_string(),
        );
    }
    let total_fee_basis_points =
        u128::from(global.fee_basis_points) + u128::from(global.creator_fee_basis_points);
    if total_fee_basis_points >= 10_000 {
        return Err("Pump fee configuration is invalid.".to_string());
    }
    let gross_output = ceil_div(
        u128::from(desired_output) * 10_000u128,
        10_000u128.saturating_sub(total_fee_basis_points),
    );
    let virtual_quote_reserves = u128::from(curve.virtual_quote_reserves);
    if gross_output >= virtual_quote_reserves {
        return Err("Requested sell output is too large for the active Pump curve.".to_string());
    }
    let numerator = gross_output.saturating_mul(u128::from(curve.virtual_token_reserves));
    let denominator = virtual_quote_reserves.saturating_sub(gross_output);
    let mut token_amount = ceil_div(numerator, denominator)
        .min(u128::from(u64::MAX))
        .try_into()
        .unwrap_or(u64::MAX);
    while quote_sell_quote_from_curve(curve, global, token_amount) < desired_output {
        token_amount = token_amount
            .checked_add(1)
            .ok_or_else(|| "Sell token amount overflowed.".to_string())?;
    }
    Ok(token_amount)
}

fn decode_global_state(data: &[u8]) -> Result<PumpGlobalState, String> {
    if data.len() < 8 {
        return Err("Pump global account data was too short.".to_string());
    }
    let mut offset = 8usize;
    let _initialized = read_bool(data, &mut offset)?;
    let _authority = read_pubkey(data, &mut offset)?;
    let fee_recipient = read_pubkey(data, &mut offset)?;
    let _initial_virtual_token_reserves = read_u64(data, &mut offset)?;
    let _initial_virtual_sol_reserves = read_u64(data, &mut offset)?;
    let _initial_real_token_reserves = read_u64(data, &mut offset)?;
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
    let _is_cashback_enabled = if offset < data.len() {
        read_bool(data, &mut offset)?
    } else {
        false
    };
    let buyback_fee_recipients = if offset < data.len() {
        read_pubkey_array::<8>(data, &mut offset)?
    } else {
        [Pubkey::default(); 8]
    };
    let buyback_basis_points = if offset < data.len() {
        read_u64(data, &mut offset)?
    } else {
        0
    };
    let initial_virtual_quote_reserves = if offset < data.len() {
        read_u64(data, &mut offset)?
    } else {
        0
    };
    let whitelisted_quote_mints = if offset < data.len() {
        read_pubkey_array::<1>(data, &mut offset)?
    } else {
        [Pubkey::default(); 1]
    };

    Ok(PumpGlobalState {
        fee_recipient,
        fee_basis_points,
        creator_fee_basis_points,
        fee_recipients,
        reserved_fee_recipient,
        reserved_fee_recipients,
        buyback_fee_recipients,
        buyback_basis_points,
        initial_virtual_quote_reserves,
        whitelisted_quote_mints,
    })
}

fn decode_bonding_curve_state(data: &[u8]) -> Result<PumpBondingCurveState, String> {
    if data.len() < 81 {
        return Err("Pump bonding curve account data was too short.".to_string());
    }
    let mut offset = 8usize;
    let virtual_token_reserves = read_u64(data, &mut offset)?;
    let virtual_quote_reserves = read_u64(data, &mut offset)?;
    let real_token_reserves = read_u64(data, &mut offset)?;
    let real_quote_reserves = read_u64(data, &mut offset)?;
    let _token_total_supply = read_u64(data, &mut offset)?;
    let complete = read_bool(data, &mut offset)?;
    let creator = read_pubkey(data, &mut offset)?;
    let is_mayhem_mode = if offset < data.len() {
        read_bool(data, &mut offset)?
    } else {
        false
    };
    let cashback_enabled = if offset < data.len() {
        read_bool(data, &mut offset)?
    } else {
        false
    };
    let quote_mint = if offset < data.len() {
        read_pubkey(data, &mut offset)?
    } else {
        Pubkey::default()
    };

    Ok(PumpBondingCurveState {
        virtual_token_reserves,
        virtual_quote_reserves,
        real_token_reserves,
        real_quote_reserves,
        complete,
        creator,
        is_mayhem_mode,
        cashback_enabled,
        quote_mint,
    })
}

async fn fetch_pump_amm_global_config(rpc_url: &str) -> Result<PumpAmmGlobalConfig, String> {
    let address = pump_amm_global_config_pda()?.to_string();
    let account_data = shared_warming_service()
        .global_state_account_bytes(rpc_url, "confirmed", &address, || async {
            fetch_account_data(rpc_url, &address, "confirmed").await
        })
        .await?;
    decode_pump_amm_global_config(&account_data)
}

async fn fetch_pump_amm_fee_config(rpc_url: &str) -> Result<Option<PumpAmmFeeConfig>, String> {
    let fee_config_address = pump_amm_fee_config_pda()?.to_string();
    match shared_warming_service()
        .global_state_account_bytes(rpc_url, "confirmed", &fee_config_address, || async {
            fetch_account_data(rpc_url, &fee_config_address, "confirmed").await
        })
        .await
    {
        Ok(account_data) => decode_pump_amm_fee_config(&account_data).map(Some),
        Err(error) if error.contains("was not found") => Ok(None),
        Err(error) => Err(error),
    }
}

/// Locate a Pump AMM pool for the given mint across supported quote assets.
///
/// When `pinned_pool` is `Some`, the caller has explicitly selected a pool
/// (e.g. pasted a pair address into the panel) and the non-canonical pool
/// policy has already permitted it. We fetch that exact pool and verify the
/// on-chain state is a Pump AMM pool for this mint — the on-chain
/// verification is intentional so a stray pubkey or a pool for a different
/// mint cannot quietly route through.
///
/// When `pinned_pool` is `None`, only the canonical authority-owned pool is
/// checked. Creator-derived secondary pools are intentionally not considered.
async fn find_pump_amm_pool_state(
    rpc_url: &str,
    mint: &Pubkey,
    pinned_pool: Option<&Pubkey>,
    commitment: &str,
) -> Result<Option<PumpAmmPoolState>, String> {
    if let Some(pinned) = pinned_pool {
        let account_data = match fetch_account_data(rpc_url, &pinned.to_string(), commitment).await
        {
            Ok(data) => data,
            Err(error) if error.contains("was not found") => return Ok(None),
            Err(error) => return Err(error),
        };
        let pool = decode_pump_amm_pool_state(*pinned, &account_data).map_err(|error| {
            format!("Pinned pool {pinned} is not a valid Pump AMM pool for mint {mint}: {error}")
        })?;
        let canonical_pool = canonical_pump_amm_pool_for_quote(mint, &pool.quote_mint)?;
        if *pinned != canonical_pool {
            return Err(format!(
                "Selected Pump AMM pool is not the canonical Pump AMM pool for mint {mint} (input pool: {pinned})."
            ));
        }
        if pool.base_mint != *mint {
            return Err(format!(
                "Pinned pool {pinned} trades base mint {} but the request targets mint {mint}.",
                pool.base_mint
            ));
        }
        pump_quote_asset_meta(&pool.quote_mint)?;
        return Ok(Some(pool));
    }

    let quote_candidates = [wsol_mint()?, usdc_mint()?];
    for quote_mint in quote_candidates {
        let canonical_pool = canonical_pump_amm_pool_for_quote(mint, &quote_mint)?;
        match fetch_account_data(rpc_url, &canonical_pool.to_string(), commitment).await {
            Ok(account_data) => {
                let pool = decode_pump_amm_pool_state(canonical_pool, &account_data)?;
                if pool.base_mint == *mint && pool.quote_mint == quote_mint {
                    return Ok(Some(pool));
                }
            }
            Err(error) if error.contains("was not found") => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn decode_pump_amm_global_config(data: &[u8]) -> Result<PumpAmmGlobalConfig, String> {
    if data.len() < 8 + 32 + 8 + 8 + 1 + (8 * 32) + 8 + 32 + 32 + 32 + 1 + (7 * 32) + 1 {
        return Err("Pump AMM global config account data was too short.".to_string());
    }
    let mut offset = 8usize;
    let _admin = read_pubkey(data, &mut offset)?;
    let lp_fee_basis_points = read_u64(data, &mut offset)?;
    let protocol_fee_basis_points = read_u64(data, &mut offset)?;
    offset = offset.saturating_add(1);
    let protocol_fee_recipients = read_pubkey_array::<8>(data, &mut offset)?;
    let coin_creator_fee_basis_points = read_u64(data, &mut offset)?;
    let _admin_set_coin_creator_authority = read_pubkey(data, &mut offset)?;
    let _whitelist_pda = read_pubkey(data, &mut offset)?;
    let reserved_fee_recipient = read_pubkey(data, &mut offset)?;
    let _mayhem_mode_enabled = read_bool(data, &mut offset)?;
    let reserved_fee_recipients = read_pubkey_array::<7>(data, &mut offset)?;
    let _is_cashback_enabled = read_bool(data, &mut offset)?;
    Ok(PumpAmmGlobalConfig {
        lp_fee_basis_points,
        protocol_fee_basis_points,
        protocol_fee_recipients,
        coin_creator_fee_basis_points,
        reserved_fee_recipient,
        reserved_fee_recipients,
    })
}

pub(crate) fn decode_pump_amm_pool_state(
    pool_pubkey: Pubkey,
    data: &[u8],
) -> Result<PumpAmmPoolState, String> {
    if data.len() < 243 {
        return Err("Pump AMM pool account data was too short.".to_string());
    }
    let mut offset = 8usize;
    offset = offset.saturating_add(1);
    offset = offset.saturating_add(2);
    let creator = read_pubkey(data, &mut offset)?;
    let base_mint = read_pubkey(data, &mut offset)?;
    let quote_mint = read_pubkey(data, &mut offset)?;
    let _lp_mint = read_pubkey(data, &mut offset)?;
    let pool_base_token_account = read_pubkey(data, &mut offset)?;
    let pool_quote_token_account = read_pubkey(data, &mut offset)?;
    let _lp_supply = read_u64(data, &mut offset)?;
    let coin_creator = if offset + 32 <= data.len() {
        read_pubkey(data, &mut offset)?
    } else {
        Pubkey::default()
    };
    let is_mayhem_mode = if offset < data.len() {
        read_bool(data, &mut offset)?
    } else {
        false
    };
    let is_cashback_coin = if offset < data.len() {
        read_bool(data, &mut offset)?
    } else {
        false
    };

    Ok(PumpAmmPoolState {
        pubkey: pool_pubkey,
        creator,
        base_mint,
        quote_mint,
        pool_base_token_account,
        pool_quote_token_account,
        coin_creator,
        is_mayhem_mode,
        is_cashback_coin,
    })
}

fn decode_pump_amm_fee_config(data: &[u8]) -> Result<PumpAmmFeeConfig, String> {
    if data.len() < 8 + 1 + 32 + 24 + 4 {
        return Err("Pump AMM fee config account data was too short.".to_string());
    }
    let mut offset = 8usize;
    offset = offset.saturating_add(1);
    let _admin = read_pubkey(data, &mut offset)?;
    let flat_fees = read_pump_amm_fees(data, &mut offset)?;
    let fee_tiers_len = read_u32(data, &mut offset)? as usize;
    let mut fee_tiers = Vec::with_capacity(fee_tiers_len);
    for _ in 0..fee_tiers_len {
        let market_cap_lamports_threshold = read_u128(data, &mut offset)?;
        let fees = read_pump_amm_fees(data, &mut offset)?;
        fee_tiers.push(PumpAmmFeeTier {
            market_cap_lamports_threshold,
            fees,
        });
    }
    Ok(PumpAmmFeeConfig {
        flat_fees,
        fee_tiers,
    })
}

fn compute_pump_amm_fees(
    global_config: &PumpAmmGlobalConfig,
    fee_config: Option<&PumpAmmFeeConfig>,
    pool: &PumpAmmPoolState,
    base_mint_supply: u64,
    base_reserve: u64,
    quote_reserve: u64,
) -> Result<PumpAmmFees, String> {
    let Some(fee_config) = fee_config else {
        return Ok(PumpAmmFees {
            lp_fee_bps: global_config.lp_fee_basis_points,
            protocol_fee_bps: global_config.protocol_fee_basis_points,
            creator_fee_bps: global_config.coin_creator_fee_basis_points,
        });
    };

    if !is_pump_amm_canonical_pool(pool)? {
        return Ok(fee_config.flat_fees);
    }

    let market_cap = pool_market_cap_quote_units(base_mint_supply, base_reserve, quote_reserve)?;
    let first_tier = fee_config
        .fee_tiers
        .first()
        .cloned()
        .ok_or_else(|| "Pump AMM fee config had no fee tiers.".to_string())?;
    if market_cap < first_tier.market_cap_lamports_threshold {
        return Ok(first_tier.fees);
    }
    for tier in fee_config.fee_tiers.iter().rev() {
        if market_cap >= tier.market_cap_lamports_threshold {
            return Ok(tier.fees);
        }
    }
    Ok(first_tier.fees)
}

fn is_pump_amm_canonical_pool(pool: &PumpAmmPoolState) -> Result<bool, String> {
    Ok(pool.creator == pump_amm_pool_authority_pda(&pool.base_mint)?)
}

fn pool_market_cap_quote_units(
    base_mint_supply: u64,
    base_reserve: u64,
    quote_reserve: u64,
) -> Result<u128, String> {
    if base_reserve == 0 {
        return Err("Pump AMM base reserve was zero.".to_string());
    }
    Ok((u128::from(quote_reserve) * u128::from(base_mint_supply)) / u128::from(base_reserve))
}

fn select_pump_amm_fee_recipient(
    global_config: &PumpAmmGlobalConfig,
    is_mayhem_mode: bool,
) -> Pubkey {
    if is_mayhem_mode {
        if global_config.reserved_fee_recipient != Pubkey::default() {
            return global_config.reserved_fee_recipient;
        }
        if let Some(entry) = global_config
            .reserved_fee_recipients
            .iter()
            .copied()
            .find(|value| *value != Pubkey::default())
        {
            return entry;
        }
    }
    global_config
        .protocol_fee_recipients
        .iter()
        .copied()
        .find(|value| *value != Pubkey::default())
        .unwrap_or_default()
}

fn pump_amm_buy_quote_input(
    quote: u64,
    base_reserve: u64,
    quote_reserve: u64,
    fees: PumpAmmFees,
    has_coin_creator: bool,
) -> u64 {
    let total_fee_bps = u128::from(fees.lp_fee_bps)
        + u128::from(fees.protocol_fee_bps)
        + if has_coin_creator {
            u128::from(fees.creator_fee_bps)
        } else {
            0
        };
    let effective_quote =
        (u128::from(quote) * 10_000u128) / (10_000u128.saturating_add(total_fee_bps));
    let numerator = u128::from(base_reserve).saturating_mul(effective_quote);
    let denominator = u128::from(quote_reserve).saturating_add(effective_quote);
    if denominator == 0 {
        0
    } else {
        (numerator / denominator).min(u128::from(u64::MAX)) as u64
    }
}

fn pump_amm_sell_base_input(
    base: u64,
    base_reserve: u64,
    quote_reserve: u64,
    fees: PumpAmmFees,
    has_coin_creator: bool,
) -> Result<u64, String> {
    if base == 0 {
        return Err("Sell amount resolves to zero tokens.".to_string());
    }
    let raw_quote_amount = (u128::from(quote_reserve) * u128::from(base))
        / u128::from(base_reserve.saturating_add(base));
    let lp_fee = fee_amount(raw_quote_amount, fees.lp_fee_bps);
    let protocol_fee = fee_amount(raw_quote_amount, fees.protocol_fee_bps);
    let creator_fee = if has_coin_creator {
        fee_amount(raw_quote_amount, fees.creator_fee_bps)
    } else {
        0
    };
    Ok(raw_quote_amount
        .saturating_sub(lp_fee)
        .saturating_sub(protocol_fee)
        .saturating_sub(creator_fee)
        .min(u128::from(u64::MAX)) as u64)
}

#[cfg(test)]
fn pump_amm_sell_quote_input(
    quote: u64,
    base_reserve: u64,
    quote_reserve: u64,
    fees: PumpAmmFees,
    has_coin_creator: bool,
) -> Result<u64, String> {
    let total_fee_bps = u128::from(fees.lp_fee_bps)
        + u128::from(fees.protocol_fee_bps)
        + if has_coin_creator {
            u128::from(fees.creator_fee_bps)
        } else {
            0
        };
    if total_fee_bps >= 10_000 {
        return Err("Pump AMM fee configuration is invalid.".to_string());
    }
    let raw_quote_amount = ceil_div(
        u128::from(quote) * 10_000u128,
        10_000u128.saturating_sub(total_fee_bps),
    );
    if raw_quote_amount >= u128::from(quote_reserve) {
        return Err("Requested Pump AMM output exceeds the quote reserve.".to_string());
    }
    Ok(ceil_div(
        u128::from(base_reserve).saturating_mul(raw_quote_amount),
        u128::from(quote_reserve).saturating_sub(raw_quote_amount),
    )
    .min(u128::from(u64::MAX)) as u64)
}

async fn resolve_pump_amm_sell_input_amount(
    sell_intent: &RuntimeSellIntent,
    wallet_key: &str,
    owner: &str,
    mint: &str,
    base_mint_supply: u64,
    base_mint_decimals: u8,
    base_reserve: u64,
    quote_reserve: u64,
    fees: PumpAmmFees,
    has_coin_creator: bool,
    quote_decimals: u8,
) -> Result<u64, String> {
    let _ = base_mint_supply;
    // Decimals come from the fresh mint account fetched alongside the
    // pool state (see `fetch_pump_amm_runtime`). Passing the live value
    // into the wallet-token cache makes the UI-amount → raw
    // reconstruction correct for any decimals choice, not just the
    // historical Pump-default 6.
    let balance = crate::wallet_token_cache::fetch_token_balance_with_cache(
        Some(wallet_key),
        owner,
        mint,
        base_mint_decimals,
    )
    .await?;
    if balance.amount_raw == 0 {
        return Err("You have 0 tokens.".to_string());
    }
    let base_amount = match sell_intent {
        RuntimeSellIntent::Percent(value) => {
            let percent_bps = u128::from(parse_percent_to_bps(value)?);
            ((u128::from(balance.amount_raw) * percent_bps) / 10_000u128).min(u128::from(u64::MAX))
                as u64
        }
        RuntimeSellIntent::SolOutput(value) => {
            if quote_decimals != 9 {
                return Err(
                    "sellOutputSol is not supported for stable-quoted Pump AMM pools yet; use sellPercent for this route."
                        .to_string(),
                );
            }
            let desired_quote =
                parse_decimal_units(value, usize::from(quote_decimals), "sellOutputSol")?;
            if desired_quote == 0 {
                return Err("sellOutputSol must be greater than zero.".to_string());
            }
            crate::sell_target_sizing::choose_target_sized_token_amount(
                balance.amount_raw,
                desired_quote,
                |amount| {
                    pump_amm_sell_base_input(
                        amount,
                        base_reserve,
                        quote_reserve,
                        fees,
                        has_coin_creator,
                    )
                    .and_then(crate::sell_target_sizing::net_sol_after_wrapper_fee)
                },
            )?
        }
    };
    if base_amount == 0 {
        return Err("Sell amount resolves to zero tokens.".to_string());
    }
    if base_amount > balance.amount_raw {
        return Err(format!(
            "Wallet balance is too small for the requested Pump AMM sell. Need {base_amount}, have {}.",
            balance.amount_raw
        ));
    }
    Ok(base_amount)
}

fn build_create_generic_ata_instruction(
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Result<Instruction, String> {
    Ok(create_associated_token_account_idempotent(
        owner,
        owner,
        mint,
        token_program,
    ))
}

fn build_wrapped_sol_open_instructions(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
    lamports: u64,
    sync_after_initialize: bool,
) -> Result<Vec<Instruction>, String> {
    let token_program = token_program_id()?;
    let mut instructions = vec![
        create_account(
            owner,
            wrapped_account,
            lamports,
            SPL_TOKEN_ACCOUNT_LEN,
            &token_program,
        ),
        initialize_account3(&token_program, wrapped_account, &wsol_mint()?, owner).map_err(
            |error| format!("Failed to build wrapped SOL initialize instruction: {error}"),
        )?,
    ];
    if sync_after_initialize {
        instructions.push(
            sync_native(&token_program, wrapped_account)
                .map_err(|error| format!("Failed to build syncNative instruction: {error}"))?,
        );
    }
    Ok(instructions)
}

fn build_wrapped_sol_close_instruction(
    owner: &Pubkey,
    wrapped_account: &Pubkey,
) -> Result<Instruction, String> {
    close_spl_account(&token_program_id()?, wrapped_account, owner, owner, &[])
        .map_err(|error| format!("Failed to build WSOL close instruction: {error}"))
}

fn build_pump_amm_extend_account_instruction(
    pool: &Pubkey,
    user: &Pubkey,
) -> Result<Instruction, String> {
    Ok(Instruction {
        program_id: pump_amm_program_id()?,
        accounts: vec![
            AccountMeta::new(*pool, false),
            AccountMeta::new_readonly(*user, true),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&pump_amm_program_id()?), false),
            AccountMeta::new_readonly(pump_amm_program_id()?, false),
        ],
        data: vec![234, 102, 194, 203, 150, 72, 62, 229],
    })
}

fn build_pump_amm_buy_exact_quote_in_instruction(
    pool: &PumpAmmPoolState,
    user: &Pubkey,
    user_base_token_account: &Pubkey,
    user_quote_token_account: &Pubkey,
    protocol_fee_recipient: &Pubkey,
    protocol_fee_recipient_token_account: &Pubkey,
    coin_creator_vault_ata: &Pubkey,
    coin_creator_vault_authority: &Pubkey,
    base_token_program: &Pubkey,
    spendable_quote_in: u64,
    min_base_amount_out: u64,
    append_cashback_remaining_accounts: bool,
) -> Result<Instruction, String> {
    let mut data = vec![198, 46, 21, 82, 180, 217, 232, 112];
    data.extend_from_slice(&spendable_quote_in.to_le_bytes());
    data.extend_from_slice(&min_base_amount_out.to_le_bytes());
    let mut accounts = vec![
        AccountMeta::new(pool.pubkey, false),
        AccountMeta::new(*user, true),
        AccountMeta::new_readonly(pump_amm_global_config_pda()?, false),
        AccountMeta::new_readonly(pool.base_mint, false),
        AccountMeta::new_readonly(pool.quote_mint, false),
        AccountMeta::new(*user_base_token_account, false),
        AccountMeta::new(*user_quote_token_account, false),
        AccountMeta::new(pool.pool_base_token_account, false),
        AccountMeta::new(pool.pool_quote_token_account, false),
        AccountMeta::new_readonly(*protocol_fee_recipient, false),
        AccountMeta::new(*protocol_fee_recipient_token_account, false),
        AccountMeta::new_readonly(*base_token_program, false),
        AccountMeta::new_readonly(token_program_id()?, false),
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(spl_associated_token_account::id(), false),
        AccountMeta::new_readonly(event_authority_pda(&pump_amm_program_id()?), false),
        AccountMeta::new_readonly(pump_amm_program_id()?, false),
        AccountMeta::new(*coin_creator_vault_ata, false),
        AccountMeta::new_readonly(*coin_creator_vault_authority, false),
        AccountMeta::new_readonly(pump_amm_global_volume_accumulator_pda(), false),
        AccountMeta::new(pump_amm_user_volume_accumulator_pda(user), false),
        AccountMeta::new_readonly(pump_amm_fee_config_pda()?, false),
        AccountMeta::new_readonly(pump_fee_program_id()?, false),
    ];
    if append_cashback_remaining_accounts {
        accounts.push(AccountMeta::new(
            pump_amm_user_volume_accumulator_wsol_ata(user)?,
            false,
        ));
    }
    accounts.push(AccountMeta::new_readonly(
        pump_amm_pool_v2_pda(&pool.base_mint),
        false,
    ));
    accounts.push(AccountMeta::new_readonly(
        selected_pump_apr28_fee_recipient()?,
        false,
    ));
    accounts.push(AccountMeta::new(
        pump_apr28_fee_recipient_ata_for_quote_mint(&pool.quote_mint, &token_program_id()?)?,
        false,
    ));
    Ok(Instruction {
        program_id: pump_amm_program_id()?,
        accounts,
        data,
    })
}

fn build_pump_amm_sell_instruction(
    pool: &PumpAmmPoolState,
    user: &Pubkey,
    user_base_token_account: &Pubkey,
    user_quote_token_account: &Pubkey,
    protocol_fee_recipient: &Pubkey,
    protocol_fee_recipient_token_account: &Pubkey,
    coin_creator_vault_ata: &Pubkey,
    coin_creator_vault_authority: &Pubkey,
    base_token_program: &Pubkey,
    base_amount_in: u64,
    min_quote_amount_out: u64,
    append_cashback_remaining_accounts: bool,
) -> Result<Instruction, String> {
    let mut data = vec![51, 230, 133, 164, 1, 127, 131, 173];
    data.extend_from_slice(&base_amount_in.to_le_bytes());
    data.extend_from_slice(&min_quote_amount_out.to_le_bytes());
    let mut accounts = vec![
        AccountMeta::new(pool.pubkey, false),
        AccountMeta::new(*user, true),
        AccountMeta::new_readonly(pump_amm_global_config_pda()?, false),
        AccountMeta::new_readonly(pool.base_mint, false),
        AccountMeta::new_readonly(pool.quote_mint, false),
        AccountMeta::new(*user_base_token_account, false),
        AccountMeta::new(*user_quote_token_account, false),
        AccountMeta::new(pool.pool_base_token_account, false),
        AccountMeta::new(pool.pool_quote_token_account, false),
        AccountMeta::new_readonly(*protocol_fee_recipient, false),
        AccountMeta::new(*protocol_fee_recipient_token_account, false),
        AccountMeta::new_readonly(*base_token_program, false),
        AccountMeta::new_readonly(token_program_id()?, false),
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(spl_associated_token_account::id(), false),
        AccountMeta::new_readonly(event_authority_pda(&pump_amm_program_id()?), false),
        AccountMeta::new_readonly(pump_amm_program_id()?, false),
        AccountMeta::new(*coin_creator_vault_ata, false),
        AccountMeta::new_readonly(*coin_creator_vault_authority, false),
        AccountMeta::new_readonly(pump_amm_fee_config_pda()?, false),
        AccountMeta::new_readonly(pump_fee_program_id()?, false),
    ];
    if append_cashback_remaining_accounts {
        accounts.push(AccountMeta::new(
            pump_amm_user_volume_accumulator_wsol_ata(user)?,
            false,
        ));
        accounts.push(AccountMeta::new(
            pump_amm_user_volume_accumulator_pda(user),
            false,
        ));
    }
    accounts.push(AccountMeta::new_readonly(
        pump_amm_pool_v2_pda(&pool.base_mint),
        false,
    ));
    accounts.push(AccountMeta::new_readonly(
        selected_pump_apr28_fee_recipient()?,
        false,
    ));
    accounts.push(AccountMeta::new(
        pump_apr28_fee_recipient_ata_for_quote_mint(&pool.quote_mint, &token_program_id()?)?,
        false,
    ));
    Ok(Instruction {
        program_id: pump_amm_program_id()?,
        accounts,
        data,
    })
}

fn apply_sell_side_slippage(value: u64, slippage_bps: u16) -> u64 {
    let minimum = ((u128::from(value)
        * u128::from(10_000u64.saturating_sub(u64::from(slippage_bps))))
        / 10_000u128)
        .min(u128::from(u64::MAX)) as u64;
    if value > 0 && minimum == 0 {
        1
    } else {
        minimum
    }
}

fn split_two_leg_slippage_bps(slippage_bps: u16) -> (u16, u16) {
    let capped = slippage_bps.min(10_000);
    (capped.saturating_sub(capped / 2), capped / 2)
}

fn fee_amount(value: u128, fee_bps: u64) -> u128 {
    (value * u128::from(fee_bps)) / 10_000u128
}

fn read_token_account_amount(data: &[u8]) -> Result<u64, String> {
    if data.len() < 72 {
        return Err("Token account data was shorter than expected.".to_string());
    }
    let bytes: [u8; 8] = data[64..72]
        .try_into()
        .map_err(|_| "Token account amount bytes were invalid.".to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_mint_supply(data: &[u8]) -> Result<u64, String> {
    if data.len() < 44 {
        return Err("Mint account data was shorter than expected.".to_string());
    }
    let bytes: [u8; 8] = data[36..44]
        .try_into()
        .map_err(|_| "Mint supply bytes were invalid.".to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

/// SPL mint account layout:
/// - 0..4   mint_authority option tag
/// - 4..36  mint_authority pubkey (conditional)
/// - 36..44 supply (u64 LE)
/// - 44     decimals (u8)
/// Token-2022 mints share the core layout for this prefix.
fn read_mint_decimals(data: &[u8]) -> Result<u8, String> {
    if data.len() < 45 {
        return Err("Mint account data was shorter than expected (decimals).".to_string());
    }
    Ok(data[44])
}

fn build_create_token_ata_instruction(
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Result<Instruction, String> {
    Ok(create_associated_token_account_idempotent(
        owner,
        owner,
        mint,
        token_program,
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_buy_exact_quote_in_v2_instruction(
    global: &PumpGlobalState,
    mint: &Pubkey,
    creator_vault_authority: &Pubkey,
    user: &Pubkey,
    spendable_quote_in: u64,
    min_tokens_out: u64,
    base_token_program: &Pubkey,
    quote_meta: &PumpQuoteAssetMeta,
    mayhem_mode: bool,
) -> Result<Instruction, String> {
    let pump_program = pump_program_id()?;
    let bonding_curve = bonding_curve_pda(mint)?;
    let creator_vault = creator_vault_pda(creator_vault_authority)?;
    let user_volume_accumulator = user_volume_accumulator_pda(user)?;
    let fee_recipient = select_buy_fee_recipient(global, mayhem_mode);
    let buyback_fee_recipient = select_pump_buyback_fee_recipient(global);
    let associated_base_bonding_curve =
        get_associated_token_address_with_program_id(&bonding_curve, mint, base_token_program);
    let associated_quote_bonding_curve = get_associated_token_address_with_program_id(
        &bonding_curve,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let associated_base_user =
        get_associated_token_address_with_program_id(user, mint, base_token_program);
    let associated_quote_user = get_associated_token_address_with_program_id(
        user,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let associated_creator_vault = get_associated_token_address_with_program_id(
        &creator_vault,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let associated_user_volume_accumulator = get_associated_token_address_with_program_id(
        &user_volume_accumulator,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let mut data = vec![194, 171, 28, 70, 104, 77, 91, 47];
    data.extend_from_slice(&spendable_quote_in.to_le_bytes());
    data.extend_from_slice(&min_tokens_out.to_le_bytes());
    Ok(Instruction {
        program_id: pump_program,
        accounts: vec![
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(quote_meta.mint, false),
            AccountMeta::new_readonly(*base_token_program, false),
            AccountMeta::new_readonly(quote_meta.token_program, false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new(fee_recipient, false),
            AccountMeta::new(
                get_associated_token_address_with_program_id(
                    &fee_recipient,
                    &quote_meta.mint,
                    &quote_meta.token_program,
                ),
                false,
            ),
            AccountMeta::new(buyback_fee_recipient, false),
            AccountMeta::new(
                get_associated_token_address_with_program_id(
                    &buyback_fee_recipient,
                    &quote_meta.mint,
                    &quote_meta.token_program,
                ),
                false,
            ),
            AccountMeta::new(bonding_curve, false),
            AccountMeta::new(associated_base_bonding_curve, false),
            AccountMeta::new(associated_quote_bonding_curve, false),
            AccountMeta::new(*user, true),
            AccountMeta::new(associated_base_user, false),
            AccountMeta::new(associated_quote_user, false),
            AccountMeta::new(creator_vault, false),
            AccountMeta::new(associated_creator_vault, false),
            AccountMeta::new(fee_sharing_config_pda(mint)?, false),
            AccountMeta::new_readonly(global_volume_accumulator_pda()?, false),
            AccountMeta::new(user_volume_accumulator, false),
            AccountMeta::new(associated_user_volume_accumulator, false),
            AccountMeta::new_readonly(fee_config_pda()?, false),
            AccountMeta::new_readonly(pump_fee_program_id()?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&pump_program), false),
            AccountMeta::new_readonly(pump_program, false),
        ],
        data,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_sell_v2_instruction(
    global: &PumpGlobalState,
    mint: &Pubkey,
    creator_vault_authority: &Pubkey,
    user: &Pubkey,
    token_amount: u64,
    min_quote_output: u64,
    base_token_program: &Pubkey,
    quote_meta: &PumpQuoteAssetMeta,
    _cashback_enabled: bool,
    mayhem_mode: bool,
) -> Result<Instruction, String> {
    let pump_program = pump_program_id()?;
    let bonding_curve = bonding_curve_pda(mint)?;
    let creator_vault = creator_vault_pda(creator_vault_authority)?;
    let user_volume_accumulator = user_volume_accumulator_pda(user)?;
    let fee_recipient = select_buy_fee_recipient(global, mayhem_mode);
    let buyback_fee_recipient = select_pump_buyback_fee_recipient(global);
    let associated_base_bonding_curve =
        get_associated_token_address_with_program_id(&bonding_curve, mint, base_token_program);
    let associated_quote_bonding_curve = get_associated_token_address_with_program_id(
        &bonding_curve,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let associated_base_user =
        get_associated_token_address_with_program_id(user, mint, base_token_program);
    let associated_quote_user = get_associated_token_address_with_program_id(
        user,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let associated_creator_vault = get_associated_token_address_with_program_id(
        &creator_vault,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let associated_user_volume_accumulator = get_associated_token_address_with_program_id(
        &user_volume_accumulator,
        &quote_meta.mint,
        &quote_meta.token_program,
    );
    let mut data = vec![93, 246, 130, 60, 231, 233, 64, 178];
    data.extend_from_slice(&token_amount.to_le_bytes());
    data.extend_from_slice(&min_quote_output.to_le_bytes());
    Ok(Instruction {
        program_id: pump_program,
        accounts: vec![
            AccountMeta::new_readonly(global_pda()?, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(quote_meta.mint, false),
            AccountMeta::new_readonly(*base_token_program, false),
            AccountMeta::new_readonly(quote_meta.token_program, false),
            AccountMeta::new_readonly(spl_associated_token_account::id(), false),
            AccountMeta::new(fee_recipient, false),
            AccountMeta::new(
                get_associated_token_address_with_program_id(
                    &fee_recipient,
                    &quote_meta.mint,
                    &quote_meta.token_program,
                ),
                false,
            ),
            AccountMeta::new(buyback_fee_recipient, false),
            AccountMeta::new(
                get_associated_token_address_with_program_id(
                    &buyback_fee_recipient,
                    &quote_meta.mint,
                    &quote_meta.token_program,
                ),
                false,
            ),
            AccountMeta::new(bonding_curve, false),
            AccountMeta::new(associated_base_bonding_curve, false),
            AccountMeta::new(associated_quote_bonding_curve, false),
            AccountMeta::new(*user, true),
            AccountMeta::new(associated_base_user, false),
            AccountMeta::new(associated_quote_user, false),
            AccountMeta::new(creator_vault, false),
            AccountMeta::new(associated_creator_vault, false),
            AccountMeta::new(fee_sharing_config_pda(mint)?, false),
            AccountMeta::new(user_volume_accumulator, false),
            AccountMeta::new(associated_user_volume_accumulator, false),
            AccountMeta::new_readonly(fee_config_pda()?, false),
            AccountMeta::new_readonly(pump_fee_program_id()?, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(event_authority_pda(&pump_program), false),
            AccountMeta::new_readonly(pump_program, false),
        ],
        data,
    })
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

fn configured_tip_account() -> Result<Option<Pubkey>, String> {
    let raw = std::env::var("EXECUTION_ENGINE_JITO_TIP_ACCOUNT")
        .ok()
        .or_else(|| std::env::var("JITO_TIP_ACCOUNT").ok())
        .unwrap_or_default();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        parse_pubkey(trimmed, "tip account").map(Some)
    }
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
    let dontfront = parse_pubkey(JITODONTFRONT_ACCOUNT, "jitodontfront")?;
    let mut instruction = transfer(payer, payer, 0);
    instruction
        .accounts
        .push(AccountMeta::new_readonly(dontfront, false));
    instructions.insert(0, instruction);
    Ok(())
}

fn quote_buy_tokens_from_curve(
    curve: &PumpBondingCurveState,
    global: &PumpGlobalState,
    spendable_quote: u64,
) -> u64 {
    if spendable_quote == 0 {
        return 0;
    }
    let total_fee_basis_points = compute_total_fee_basis_points(global);
    let input_amount = ((u128::from(spendable_quote).saturating_sub(1)) * 10_000u128)
        / (10_000u128 + total_fee_basis_points);
    if input_amount == 0 {
        return 0;
    }
    let tokens = (input_amount * u128::from(curve.virtual_token_reserves))
        / (u128::from(curve.virtual_quote_reserves) + input_amount);
    tokens.min(u128::from(curve.real_token_reserves)) as u64
}

fn quote_sell_quote_from_curve(
    curve: &PumpBondingCurveState,
    global: &PumpGlobalState,
    token_amount: u64,
) -> u64 {
    if token_amount == 0 {
        return 0;
    }
    let gross_output = (u128::from(token_amount) * u128::from(curve.virtual_quote_reserves))
        / (u128::from(curve.virtual_token_reserves) + u128::from(token_amount));
    let protocol_fee = ceil_div(gross_output * u128::from(global.fee_basis_points), 10_000);
    let creator_fee = ceil_div(
        gross_output * u128::from(global.creator_fee_basis_points),
        10_000,
    );
    gross_output
        .saturating_sub(protocol_fee)
        .saturating_sub(creator_fee)
        .min(u128::from(curve.real_quote_reserves)) as u64
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
    }
    if global.fee_recipient != Pubkey::default() {
        return global.fee_recipient;
    }
    global
        .fee_recipients
        .iter()
        .copied()
        .find(|entry| *entry != Pubkey::default())
        .unwrap_or_default()
}

fn select_pump_buyback_fee_recipient(global: &PumpGlobalState) -> Pubkey {
    global
        .buyback_fee_recipients
        .iter()
        .copied()
        .find(|entry| *entry != Pubkey::default())
        .unwrap_or_default()
}

fn apply_buy_slippage_buffer(sol_amount: u64, slippage_bps: u64) -> u64 {
    ceil_div(
        u128::from(sol_amount) * u128::from(10_000u64.saturating_add(slippage_bps)),
        10_000,
    )
    .min(u128::from(u64::MAX)) as u64
}

fn apply_buy_token_slippage(token_amount: u64, slippage_bps: u64) -> u64 {
    let minimum = (u128::from(token_amount)
        * u128::from(10_000u64.saturating_sub(slippage_bps.min(10_000))))
        / 10_000u128;
    let minimum = minimum.min(u128::from(u64::MAX)) as u64;
    if token_amount > 0 && minimum == 0 {
        1
    } else {
        minimum
    }
}

fn priority_fee_sol_to_micro_lamports(priority_fee_sol: &str) -> Result<u64, String> {
    let lamports = parse_decimal_units(priority_fee_sol, 9, "feeSol")?;
    if lamports == 0 {
        Ok(0)
    } else {
        Ok((lamports.saturating_mul(1_000_000)) / PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT)
    }
}

fn parse_slippage_bps(value: Option<&str>) -> Result<u16, String> {
    let raw = value.unwrap_or("20").trim();
    if raw.is_empty() {
        return Ok(2_000);
    }
    let hundredths = parse_decimal_scaled(raw, 2, "slippagePercent")?;
    if hundredths > 1_000_000 {
        return Err("slippagePercent is out of range.".to_string());
    }
    let bps =
        u16::try_from(hundredths).map_err(|_| "slippagePercent is out of range.".to_string())?;
    Ok(if bps == 0 { 1 } else { bps })
}

fn parse_percent_to_bps(value: &str) -> Result<u16, String> {
    let hundredths = parse_decimal_scaled(value, 2, "sellPercent")?;
    if hundredths > 10_000 {
        return Err("Sell percent must be between 0 and 100.".to_string());
    }
    u16::try_from(hundredths).map_err(|_| "Sell percent is out of range.".to_string())
}

fn parse_decimal_units(value: &str, decimals: usize, label: &str) -> Result<u64, String> {
    let scaled = parse_decimal_scaled(value, decimals, label)?;
    u64::try_from(scaled).map_err(|_| format!("{label} is too large."))
}

fn parse_decimal_scaled(value: &str, decimals: usize, label: &str) -> Result<u128, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    let mut parts = trimmed.split('.');
    let whole = parts.next().unwrap_or_default();
    let fractional = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err(format!("{label} is not a valid number."));
    }
    if !whole.chars().all(|ch| ch.is_ascii_digit())
        || !fractional.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err(format!("{label} is not a valid number."));
    }
    if fractional.len() > decimals {
        return Err(format!("{label} has too many decimal places."));
    }

    let scale = 10_u128
        .checked_pow(u32::try_from(decimals).map_err(|_| format!("{label} scale overflowed."))?)
        .ok_or_else(|| format!("{label} scale overflowed."))?;
    let whole_value = if whole.is_empty() {
        0
    } else {
        whole
            .parse::<u128>()
            .map_err(|error| format!("{label} parse failed: {error}"))?
    };
    let fractional_value = if fractional.is_empty() {
        0
    } else {
        let padded = format!("{fractional:0<width$}", width = decimals);
        padded
            .parse::<u128>()
            .map_err(|error| format!("{label} parse failed: {error}"))?
    };

    whole_value
        .checked_mul(scale)
        .and_then(|value| value.checked_add(fractional_value))
        .ok_or_else(|| format!("{label} is too large."))
}

fn parse_sol_lamports_field(value: &str) -> Option<u64> {
    parse_decimal_units(value, 9, "tipSol")
        .ok()
        .filter(|lamports| *lamports > 0)
}

fn compute_total_fee_basis_points(global: &PumpGlobalState) -> u128 {
    u128::from(global.fee_basis_points) + u128::from(global.creator_fee_basis_points)
}

fn ceil_div(numerator: u128, denominator: u128) -> u128 {
    numerator.div_ceil(denominator)
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|error| format!("Invalid {label}: {error}"))
}

pub(crate) fn pump_program_id() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_PROGRAM_ID, "Pump program id")
}

pub(crate) fn pump_amm_program_id() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_AMM_PROGRAM_ID, "Pump AMM program id")
}

fn token_program_id() -> Result<Pubkey, String> {
    parse_pubkey(TOKEN_PROGRAM_ID, "Token program id")
}

fn token_2022_program_id() -> Result<Pubkey, String> {
    parse_pubkey(TOKEN_2022_PROGRAM_ID, "Token 2022 program id")
}

fn usdc_mint() -> Result<Pubkey, String> {
    parse_pubkey(USDC_MINT, "USDC mint")
}

fn pump_fee_program_id() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_FEE_PROGRAM_ID, "Pump fee program id")
}

fn compute_budget_program_id() -> Result<Pubkey, String> {
    parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID, "Compute Budget program id")
}

fn wsol_mint() -> Result<Pubkey, String> {
    parse_pubkey(WSOL_MINT, "WSOL mint")
}

fn pump_quote_asset_meta(raw_quote_mint: &Pubkey) -> Result<PumpQuoteAssetMeta, String> {
    let wsol = wsol_mint()?;
    let usdc = usdc_mint()?;
    let resolved_mint = if *raw_quote_mint == Pubkey::default() {
        wsol
    } else {
        *raw_quote_mint
    };
    if resolved_mint == wsol {
        return Ok(PumpQuoteAssetMeta {
            kind: PumpQuoteAssetKind::Sol,
            mint: resolved_mint,
            token_program: token_program_id()?,
            decimals: 9,
            planner_asset: PlannerQuoteAsset::Sol,
        });
    }
    if resolved_mint == usdc {
        return Ok(PumpQuoteAssetMeta {
            kind: PumpQuoteAssetKind::Usdc,
            mint: resolved_mint,
            token_program: token_program_id()?,
            decimals: 6,
            planner_asset: PlannerQuoteAsset::Usdc,
        });
    }
    Err(format!(
        "Pump bonding curve uses unsupported quote mint {resolved_mint}."
    ))
}

pub(crate) fn pump_quote_asset_for_mint(
    raw_quote_mint: &Pubkey,
) -> Result<PumpQuoteAssetForMint, String> {
    match pump_quote_asset_meta(raw_quote_mint)?.kind {
        PumpQuoteAssetKind::Sol => Ok(PumpQuoteAssetForMint::Wsol),
        PumpQuoteAssetKind::Usdc => Ok(PumpQuoteAssetForMint::Usdc),
    }
}

fn event_authority_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"__event_authority"], program_id).0
}

pub(crate) fn selected_pump_apr28_fee_recipient() -> Result<Pubkey, String> {
    parse_pubkey(PUMP_APR28_FEE_RECIPIENTS[0], "Pump April 28 fee recipient")
}

pub(crate) fn pump_apr28_fee_recipient_ata_for_quote_mint(
    quote_mint: &Pubkey,
    quote_token_program: &Pubkey,
) -> Result<Pubkey, String> {
    Ok(get_associated_token_address_with_program_id(
        &selected_pump_apr28_fee_recipient()?,
        quote_mint,
        quote_token_program,
    ))
}

fn pump_apr28_dynamic_alt_manifest_entries() -> Result<Vec<AltManifestEntry>, String> {
    Ok(vec![AltManifestEntry::required(
        pump_apr28_fee_recipient_ata_for_quote_mint(&wsol_mint()?, &token_program_id()?)?
            .to_string(),
        "pump-upgrade",
        "pump-apr28-wsol-fee-recipient-ata",
        "Execution-engine Pump AMM WSOL quote routes emit the selected April 28 fee-recipient ATA",
    )])
}

pub(crate) fn bonding_curve_pda(mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &pump_program_id()?).0)
}

fn global_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global"], &pump_program_id()?).0)
}

fn pump_amm_global_config_pda() -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(&[b"global_config"], &pump_amm_program_id()?).0)
}

pub(crate) fn pump_amm_pool_authority_pda(base_mint: &Pubkey) -> Result<Pubkey, String> {
    Ok(Pubkey::find_program_address(
        &[b"pool-authority", base_mint.as_ref()],
        &pump_program_id()?,
    )
    .0)
}

/// Derives the canonical (authority-owned) Pump AMM WSOL pool for a given
/// base mint, without issuing any RPC. Used by the warm classifier to
/// distinguish a canonical pool selection from a user-selected non-canonical
/// pool (e.g. pasted pair address for a low-liquidity secondary pool).
#[allow(dead_code)]
pub(crate) fn canonical_pump_amm_pool(base_mint: &Pubkey) -> Result<Pubkey, String> {
    canonical_pump_amm_pool_for_quote(base_mint, &wsol_mint()?)
}

pub(crate) fn canonical_pump_amm_pool_for_quote(
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
) -> Result<Pubkey, String> {
    pump_quote_asset_meta(quote_mint)?;
    let canonical_creator = pump_amm_pool_authority_pda(base_mint)?;
    derive_pump_amm_pool_address(&canonical_creator, base_mint, quote_mint, 0)
}

pub(crate) fn derive_pump_amm_pool_address(
    creator: &Pubkey,
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
    index: u16,
) -> Result<Pubkey, String> {
    let index_bytes = index.to_le_bytes();
    Ok(Pubkey::find_program_address(
        &[
            b"pool",
            &index_bytes,
            creator.as_ref(),
            base_mint.as_ref(),
            quote_mint.as_ref(),
        ],
        &pump_amm_program_id()?,
    )
    .0)
}

fn pump_amm_coin_creator_vault_authority_pda(coin_creator: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"creator_vault", coin_creator.as_ref()],
        &pump_amm_program_id().expect("pump amm program id"),
    )
    .0
}

fn pump_amm_pool_v2_pda(base_mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"pool-v2", base_mint.as_ref()],
        &pump_amm_program_id().expect("pump amm program id"),
    )
    .0
}

fn pump_amm_global_volume_accumulator_pda() -> Pubkey {
    Pubkey::find_program_address(
        &[b"global_volume_accumulator"],
        &pump_amm_program_id().expect("pump amm program id"),
    )
    .0
}

fn pump_amm_user_volume_accumulator_pda(user: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"user_volume_accumulator", user.as_ref()],
        &pump_amm_program_id().expect("pump amm program id"),
    )
    .0
}

fn pump_amm_user_volume_accumulator_wsol_ata(user: &Pubkey) -> Result<Pubkey, String> {
    Ok(get_associated_token_address_with_program_id(
        &pump_amm_user_volume_accumulator_pda(user),
        &wsol_mint()?,
        &token_program_id()?,
    ))
}

fn pump_amm_fee_config_pda() -> Result<Pubkey, String> {
    let fee_program = pump_fee_program_id()?;
    let fee_config_seed_owner = Pubkey::new_from_array([
        12, 20, 222, 252, 130, 94, 198, 118, 148, 37, 8, 24, 187, 101, 64, 101, 244, 41, 141, 49,
        86, 213, 113, 180, 212, 248, 9, 12, 24, 233, 168, 99,
    ]);
    Ok(Pubkey::find_program_address(
        &[b"fee_config", fee_config_seed_owner.as_ref()],
        &fee_program,
    )
    .0)
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

fn pool_requires_extend_account() -> bool {
    true
}

async fn append_pump_amm_setup_instructions(
    rpc_url: &str,
    instructions: &mut Vec<Instruction>,
    pool: &PumpAmmPoolState,
    owner: &Pubkey,
    include_cashback_volume_account: bool,
) -> Result<(), String> {
    if pool_requires_extend_account() {
        let pool_data = fetch_account_data(rpc_url, &pool.pubkey.to_string(), "confirmed").await?;
        if pool_data.len() < 300 {
            instructions.push(build_pump_amm_extend_account_instruction(
                &pool.pubkey,
                owner,
            )?);
        }
    }
    if include_cashback_volume_account {
        instructions.push(create_associated_token_account_idempotent(
            owner,
            &pump_amm_user_volume_accumulator_pda(owner),
            &wsol_mint()?,
            &token_program_id()?,
        ));
    }
    Ok(())
}

fn read_u32(data: &[u8], offset: &mut usize) -> Result<u32, String> {
    let end = offset.saturating_add(4);
    let bytes: [u8; 4] = data
        .get(*offset..end)
        .ok_or_else(|| "Unexpected end of account data while reading u32.".to_string())?
        .try_into()
        .map_err(|_| "Failed to read u32 bytes from account data.".to_string())?;
    *offset = end;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u128(data: &[u8], offset: &mut usize) -> Result<u128, String> {
    let end = offset.saturating_add(16);
    let bytes: [u8; 16] = data
        .get(*offset..end)
        .ok_or_else(|| "Unexpected end of account data while reading u128.".to_string())?
        .try_into()
        .map_err(|_| "Failed to read u128 bytes from account data.".to_string())?;
    *offset = end;
    Ok(u128::from_le_bytes(bytes))
}

fn read_pump_amm_fees(data: &[u8], offset: &mut usize) -> Result<PumpAmmFees, String> {
    Ok(PumpAmmFees {
        lp_fee_bps: read_u64(data, offset)?,
        protocol_fee_bps: read_u64(data, offset)?,
        creator_fee_bps: read_u64(data, offset)?,
    })
}

fn read_bool(data: &[u8], offset: &mut usize) -> Result<bool, String> {
    let Some(byte) = data.get(*offset) else {
        return Err("Unexpected end of account data while reading bool.".to_string());
    };
    *offset += 1;
    Ok(*byte != 0)
}

fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    let end = offset.saturating_add(8);
    let bytes: [u8; 8] = data
        .get(*offset..end)
        .ok_or_else(|| "Unexpected end of account data while reading u64.".to_string())?
        .try_into()
        .map_err(|_| "Failed to read u64 bytes from account data.".to_string())?;
    *offset = end;
    Ok(u64::from_le_bytes(bytes))
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Result<Pubkey, String> {
    let end = offset.saturating_add(32);
    let bytes: [u8; 32] = data
        .get(*offset..end)
        .ok_or_else(|| "Unexpected end of account data while reading pubkey.".to_string())?
        .try_into()
        .map_err(|_| "Failed to read pubkey bytes from account data.".to_string())?;
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
        .map_err(|_| "Failed to decode pubkey array from account data.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_global() -> PumpGlobalState {
        PumpGlobalState {
            fee_recipient: Pubkey::new_unique(),
            fee_basis_points: 100,
            creator_fee_basis_points: 50,
            fee_recipients: [Pubkey::default(); 7],
            reserved_fee_recipient: Pubkey::default(),
            reserved_fee_recipients: [Pubkey::default(); 7],
            buyback_fee_recipients: [Pubkey::default(); 8],
            buyback_basis_points: 0,
            initial_virtual_quote_reserves: 0,
            whitelisted_quote_mints: [Pubkey::default(); 1],
        }
    }

    fn sample_curve() -> PumpBondingCurveState {
        PumpBondingCurveState {
            virtual_token_reserves: 900_000_000_000_000,
            virtual_quote_reserves: 40_000_000_000,
            real_token_reserves: 600_000_000_000_000,
            real_quote_reserves: 35_000_000_000,
            complete: false,
            creator: Pubkey::new_unique(),
            is_mayhem_mode: false,
            cashback_enabled: false,
            quote_mint: Pubkey::default(),
        }
    }

    fn sample_amm_fees() -> PumpAmmFees {
        PumpAmmFees {
            lp_fee_bps: 20,
            protocol_fee_bps: 5,
            creator_fee_bps: 5,
        }
    }

    fn sample_amm_pool() -> PumpAmmPoolState {
        let base_mint = Pubkey::new_unique();
        PumpAmmPoolState {
            pubkey: Pubkey::new_unique(),
            creator: pump_amm_pool_authority_pda(&base_mint).expect("canonical creator"),
            base_mint,
            quote_mint: wsol_mint().expect("wsol mint"),
            pool_base_token_account: Pubkey::new_unique(),
            pool_quote_token_account: Pubkey::new_unique(),
            coin_creator: Pubkey::new_unique(),
            is_mayhem_mode: false,
            is_cashback_coin: false,
        }
    }

    fn assert_v2_bonding_curve_token_program_accounts(
        instruction: &Instruction,
        mint: &Pubkey,
        user: &Pubkey,
        token_program: &Pubkey,
    ) {
        let bonding_curve = bonding_curve_pda(mint).expect("bonding curve");
        assert_eq!(instruction.accounts[3].pubkey, *token_program);
        assert_eq!(
            instruction.accounts[11].pubkey,
            get_associated_token_address_with_program_id(&bonding_curve, mint, token_program)
        );
        assert_eq!(
            instruction.accounts[14].pubkey,
            get_associated_token_address_with_program_id(user, mint, token_program)
        );
    }

    #[test]
    fn bonding_curve_exact_quote_v2_buy_uses_fixed_account_order() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let token_program = token_2022_program_id().expect("token 2022 program");
        let user = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let quote_meta = pump_quote_asset_meta(&Pubkey::default()).expect("quote meta");
        let instruction = build_buy_exact_quote_in_v2_instruction(
            &global,
            &mint,
            &creator,
            &user,
            100_000_000,
            1_000_000,
            &token_program,
            &quote_meta,
            false,
        )
        .expect("buy instruction");

        assert_eq!(instruction.accounts.len(), 27);
        assert_eq!(&instruction.data[..8], &[194, 171, 28, 70, 104, 77, 91, 47]);
        assert_eq!(
            instruction.accounts[0].pubkey,
            global_pda().expect("global")
        );
        assert_eq!(instruction.accounts[1].pubkey, mint);
        assert_eq!(
            instruction.accounts[2].pubkey,
            wsol_mint().expect("wsol mint")
        );
        assert_eq!(
            instruction.accounts[19].pubkey,
            global_volume_accumulator_pda().expect("global volume")
        );
        assert_eq!(
            instruction.accounts[20].pubkey,
            user_volume_accumulator_pda(&user).expect("user volume")
        );
        assert_v2_bonding_curve_token_program_accounts(&instruction, &mint, &user, &token_program);
    }

    #[test]
    fn bonding_curve_exact_quote_v2_buy_encodes_spend_and_min_tokens() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let spend_lamports = 100_000_000;
        let min_tokens_out = 1_000_000;
        let token_program = token_2022_program_id().expect("token 2022 program");
        let quote_meta = pump_quote_asset_meta(&Pubkey::default()).expect("quote meta");
        let instruction = build_buy_exact_quote_in_v2_instruction(
            &global,
            &mint,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            spend_lamports,
            min_tokens_out,
            &token_program,
            &quote_meta,
            false,
        )
        .expect("buy exact quote v2 instruction");

        assert_eq!(&instruction.data[..8], &[194, 171, 28, 70, 104, 77, 91, 47]);
        assert_eq!(&instruction.data[8..16], &spend_lamports.to_le_bytes());
        assert_eq!(&instruction.data[16..24], &min_tokens_out.to_le_bytes());
    }

    #[test]
    fn bonding_curve_buy_token_slippage_floors_positive_quote_to_one() {
        assert_eq!(apply_buy_token_slippage(1_000, 0), 1_000);
        assert_eq!(apply_buy_token_slippage(1_000, 5_000), 500);
        assert_eq!(apply_buy_token_slippage(1_000, 10_000), 1);
        assert_eq!(apply_buy_token_slippage(0, 10_000), 0);
    }

    #[test]
    fn bonding_curve_exact_sol_buy_quote_math_keeps_spend_fixed() {
        let global = sample_global();
        let curve = sample_curve();
        let spend_lamports = 500_000_000;
        let quoted_tokens = quote_buy_tokens_from_curve(&curve, &global, spend_lamports);
        let token_program = token_2022_program_id().expect("token 2022 program");

        assert!(quoted_tokens > 1);
        assert_eq!(apply_buy_token_slippage(quoted_tokens, 0), quoted_tokens);
        assert_eq!(
            apply_buy_token_slippage(quoted_tokens, 5_000),
            quoted_tokens / 2
        );
        assert_eq!(apply_buy_token_slippage(quoted_tokens, 10_000), 1);

        let quote_meta = pump_quote_asset_meta(&Pubkey::default()).expect("quote meta");
        let instruction = build_buy_exact_quote_in_v2_instruction(
            &global,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            spend_lamports,
            apply_buy_token_slippage(quoted_tokens, 10_000),
            &token_program,
            &quote_meta,
            false,
        )
        .expect("buy exact quote v2 instruction");
        assert_eq!(&instruction.data[8..16], &spend_lamports.to_le_bytes());
    }

    #[test]
    fn pump_quote_meta_uses_quote_specific_decimals() {
        let default_meta = pump_quote_asset_meta(&Pubkey::default()).expect("default quote");
        assert_eq!(default_meta.kind, PumpQuoteAssetKind::Sol);
        assert_eq!(default_meta.decimals, 9);
        assert_eq!(default_meta.planner_asset, PlannerQuoteAsset::Sol);

        let usdc_meta = pump_quote_asset_meta(&usdc_mint().expect("usdc")).expect("usdc quote");
        assert_eq!(usdc_meta.kind, PumpQuoteAssetKind::Usdc);
        assert_eq!(usdc_meta.decimals, 6);
        assert_eq!(usdc_meta.planner_asset, PlannerQuoteAsset::Usdc);
    }

    #[test]
    fn split_two_leg_slippage_preserves_total_budget_shape() {
        assert_eq!(split_two_leg_slippage_bps(0), (0, 0));
        assert_eq!(split_two_leg_slippage_bps(501), (251, 250));
        assert_eq!(split_two_leg_slippage_bps(20_000), (5_000, 5_000));
    }

    #[test]
    fn pump_amm_selector_uses_quote_neutral_wrapper_action() {
        let action = match TradeSide::Buy {
            TradeSide::Buy => WrapperAction::PumpAmmBuy,
            TradeSide::Sell => WrapperAction::PumpAmmSell,
        };
        assert_eq!(action, WrapperAction::PumpAmmBuy);
    }

    #[test]
    fn bonding_curve_classification_quote_asset_follows_curve_quote_mint() {
        let mut curve = sample_curve();
        curve.quote_mint = usdc_mint().expect("usdc mint");
        let quote_asset = pump_quote_asset_meta(&curve.quote_mint)
            .expect("quote meta")
            .planner_asset;

        assert_eq!(quote_asset, PlannerQuoteAsset::Usdc);
    }

    #[test]
    fn canonical_pump_amm_pool_derives_distinct_quote_pools() {
        let base_mint = Pubkey::new_unique();
        let wsol_pool = canonical_pump_amm_pool_for_quote(&base_mint, &wsol_mint().expect("wsol"))
            .expect("wsol pool");
        let usdc_pool = canonical_pump_amm_pool_for_quote(&base_mint, &usdc_mint().expect("usdc"))
            .expect("usdc pool");

        assert_ne!(wsol_pool, usdc_pool);
    }

    #[test]
    fn bonding_curve_buy_uses_supplied_token_program_for_atas() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let token_program = token_program_id().expect("token program");
        let quote_meta = pump_quote_asset_meta(&Pubkey::default()).expect("quote meta");
        let instruction = build_buy_exact_quote_in_v2_instruction(
            &global,
            &mint,
            &Pubkey::new_unique(),
            &user,
            100_000_000,
            1_000_000,
            &token_program,
            &quote_meta,
            false,
        )
        .expect("buy instruction");

        assert_v2_bonding_curve_token_program_accounts(&instruction, &mint, &user, &token_program);
    }

    #[test]
    fn bonding_curve_buy_uses_supplied_creator_vault_authority() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let creator_vault_authority = Pubkey::new_unique();
        let token_program = token_program_id().expect("token program");
        let quote_meta = pump_quote_asset_meta(&Pubkey::default()).expect("quote meta");
        let instruction = build_buy_exact_quote_in_v2_instruction(
            &global,
            &mint,
            &creator_vault_authority,
            &user,
            100_000_000,
            1_000_000,
            &token_program,
            &quote_meta,
            false,
        )
        .expect("buy instruction");
        let creator_vault = creator_vault_pda(&creator_vault_authority).expect("creator vault pda");

        assert_eq!(instruction.accounts[16].pubkey, creator_vault);
        assert_eq!(
            instruction.accounts[17].pubkey,
            get_associated_token_address_with_program_id(
                &creator_vault,
                &quote_meta.mint,
                &quote_meta.token_program,
            )
        );
    }

    #[test]
    fn bonding_curve_sell_v2_omits_global_volume_accumulator() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let token_program = token_2022_program_id().expect("token 2022 program");
        let user = Pubkey::new_unique();
        let creator = Pubkey::new_unique();
        let quote_meta = pump_quote_asset_meta(&Pubkey::default()).expect("quote meta");
        let instruction = build_sell_v2_instruction(
            &global,
            &mint,
            &creator,
            &user,
            1_000_000,
            100_000_000,
            &token_program,
            &quote_meta,
            false,
            false,
        )
        .expect("sell instruction");

        assert_eq!(instruction.accounts.len(), 26);
        assert_eq!(
            &instruction.data[..8],
            &[93, 246, 130, 60, 231, 233, 64, 178]
        );
        assert!(
            !instruction
                .accounts
                .iter()
                .any(|meta| meta.pubkey == global_volume_accumulator_pda().expect("global volume"))
        );
        assert_eq!(
            instruction.accounts[19].pubkey,
            user_volume_accumulator_pda(&user).expect("user volume")
        );
        assert_v2_bonding_curve_token_program_accounts(&instruction, &mint, &user, &token_program);
    }

    #[test]
    fn bonding_curve_sell_uses_supplied_token_program_for_atas() {
        let global = sample_global();
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let token_program = token_program_id().expect("token program");
        let quote_meta = pump_quote_asset_meta(&Pubkey::default()).expect("quote meta");
        let instruction = build_sell_v2_instruction(
            &global,
            &mint,
            &Pubkey::new_unique(),
            &user,
            1_000_000,
            100_000_000,
            &token_program,
            &quote_meta,
            false,
            false,
        )
        .expect("sell instruction");

        assert_v2_bonding_curve_token_program_accounts(&instruction, &mint, &user, &token_program);
    }

    #[test]
    fn pump_amm_instructions_append_apr28_fee_recipient_and_quote_ata_after_pool_v2() {
        let pool = sample_amm_pool();
        let user = Pubkey::new_unique();
        let expected_quote_ata = pump_apr28_fee_recipient_ata_for_quote_mint(
            &pool.quote_mint,
            &token_program_id().unwrap(),
        )
        .expect("April 28 quote ATA");

        let buy = build_pump_amm_buy_exact_quote_in_instruction(
            &pool,
            &user,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &token_program_id().expect("token program"),
            100_000_000,
            1_000,
            false,
        )
        .expect("AMM buy instruction");
        assert_eq!(buy.accounts.len(), 26);
        assert_eq!(
            buy.accounts[23].pubkey,
            pump_amm_pool_v2_pda(&pool.base_mint)
        );
        assert_eq!(
            buy.accounts[24].pubkey,
            selected_pump_apr28_fee_recipient().expect("April 28 recipient")
        );
        assert!(!buy.accounts[24].is_writable);
        assert_eq!(buy.accounts[25].pubkey, expected_quote_ata);
        assert!(buy.accounts[25].is_writable);

        let sell = build_pump_amm_sell_instruction(
            &pool,
            &user,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &token_program_id().expect("token program"),
            1_000,
            100_000_000,
            false,
        )
        .expect("AMM sell instruction");
        assert_eq!(sell.accounts.len(), 24);
        assert_eq!(
            sell.accounts[21].pubkey,
            pump_amm_pool_v2_pda(&pool.base_mint)
        );
        assert_eq!(
            sell.accounts[22].pubkey,
            selected_pump_apr28_fee_recipient().expect("April 28 recipient")
        );
        assert!(!sell.accounts[22].is_writable);
        assert_eq!(sell.accounts[23].pubkey, expected_quote_ata);
        assert!(sell.accounts[23].is_writable);
    }

    #[test]
    fn exact_output_solver_meets_requested_net_output() {
        let global = sample_global();
        let curve = sample_curve();
        let requested = 250_000_000;
        let token_amount =
            required_tokens_for_net_sol_output(&curve, &global, requested).expect("token amount");
        assert!(token_amount > 0);
        assert!(quote_sell_quote_from_curve(&curve, &global, token_amount) >= requested);
    }

    #[test]
    fn parse_percent_rejects_above_hundred() {
        assert!(parse_percent_to_bps("100.01").is_err());
        assert_eq!(parse_percent_to_bps("25").expect("25%"), 2_500);
    }

    #[test]
    fn pump_amm_sell_quote_input_meets_requested_output() {
        let fees = sample_amm_fees();
        let desired_quote = 250_000_000;
        let base_amount =
            pump_amm_sell_quote_input(desired_quote, 1_000_000_000_000, 50_000_000_000, fees, true)
                .expect("base amount");
        let received_quote =
            pump_amm_sell_base_input(base_amount, 1_000_000_000_000, 50_000_000_000, fees, true)
                .expect("quote amount");
        assert!(received_quote >= desired_quote);
    }

    #[test]
    fn pump_amm_fee_tier_selection_uses_market_cap_thresholds() {
        let pool = sample_amm_pool();
        let global = PumpAmmGlobalConfig {
            lp_fee_basis_points: 20,
            protocol_fee_basis_points: 5,
            protocol_fee_recipients: [Pubkey::new_unique(); 8],
            coin_creator_fee_basis_points: 5,
            reserved_fee_recipient: Pubkey::default(),
            reserved_fee_recipients: [Pubkey::default(); 7],
        };
        let fee_config = PumpAmmFeeConfig {
            flat_fees: PumpAmmFees {
                lp_fee_bps: 25,
                protocol_fee_bps: 5,
                creator_fee_bps: 0,
            },
            fee_tiers: vec![
                PumpAmmFeeTier {
                    market_cap_lamports_threshold: 0,
                    fees: PumpAmmFees {
                        lp_fee_bps: 2,
                        protocol_fee_bps: 93,
                        creator_fee_bps: 30,
                    },
                },
                PumpAmmFeeTier {
                    market_cap_lamports_threshold: 420 * 1_000_000_000,
                    fees: PumpAmmFees {
                        lp_fee_bps: 20,
                        protocol_fee_bps: 5,
                        creator_fee_bps: 95,
                    },
                },
            ],
        };

        let low_cap_fees = compute_pump_amm_fees(
            &global,
            Some(&fee_config),
            &pool,
            1_000_000_000_000_000,
            1_000_000_000_000,
            100_000_000_000 / 1_000,
        )
        .expect("low cap fees");
        assert_eq!(low_cap_fees.lp_fee_bps, 2);
        assert_eq!(low_cap_fees.protocol_fee_bps, 93);

        let high_cap_fees = compute_pump_amm_fees(
            &global,
            Some(&fee_config),
            &pool,
            1_000_000_000_000_000,
            1_000_000_000_000,
            500_000_000_000 / 1_000,
        )
        .expect("high cap fees");
        assert_eq!(high_cap_fees.lp_fee_bps, 20);
        assert_eq!(high_cap_fees.creator_fee_bps, 95);
    }

    #[test]
    fn decode_bonding_curve_state_reads_mayhem_and_cashback_flags() {
        let creator = Pubkey::new_unique();
        let mut data = vec![0u8; 8];
        data.extend_from_slice(&1u64.to_le_bytes());
        data.extend_from_slice(&2u64.to_le_bytes());
        data.extend_from_slice(&3u64.to_le_bytes());
        data.extend_from_slice(&4u64.to_le_bytes());
        data.extend_from_slice(&5u64.to_le_bytes());
        data.push(0);
        data.extend_from_slice(creator.as_ref());
        data.push(1);
        data.push(1);

        let decoded = decode_bonding_curve_state(&data).expect("decode bonding curve");
        assert!(!decoded.complete);
        assert_eq!(decoded.creator, creator);
        assert!(decoded.is_mayhem_mode);
        assert!(decoded.cashback_enabled);
    }
}
