use std::str::FromStr;

use futures_util::{StreamExt, stream::FuturesUnordered};
use solana_sdk::pubkey::Pubkey;
use spl_token_2022_interface::{extension::PodStateWithExtensions, pod::PodMint};

use crate::{
    bonk_native::{
        classify_bonk_pool_address, compile_bonk_trade, plan_bonk_trade, plan_bonk_trade_uncached,
        selector_from_classified_bonk_raydium_pair,
    },
    extension_api::TradeSettlementAsset,
    meteora_native::{
        classify_bags_pool_address, compile_meteora_trade, plan_meteora_trade,
        plan_meteora_trade_uncached,
    },
    mint_warm_cache::{
        PrewarmedMint, build_fingerprint, prewarmed_from_plan, shared_mint_warm_cache,
    },
    pump_native::{
        bonding_curve_pda, canonical_pump_amm_pool, classify_pump_bonding_curve_address,
        compile_pump_trade, decode_pump_amm_pool_state, plan_pump_trade, pump_amm_program_id,
    },
    raydium_amm_v4_native::{
        classify_raydium_amm_v4_pool_address, compile_raydium_amm_v4_trade,
        plan_raydium_amm_v4_trade, raydium_amm_v4_program_id,
    },
    raydium_cpmm_native::{
        classify_raydium_cpmm_pool_address, compile_raydium_cpmm_trade, plan_raydium_cpmm_trade,
        raydium_cpmm_program_id,
    },
    raydium_launchlab_native::{
        classify_raydium_launchlab_pool_address, compile_raydium_launchlab_trade,
        plan_raydium_launchlab_trade, raydium_launchlab_program_id,
    },
    rollout::{TradeExecutionBackend, family_warm_enabled, preferred_execution_backend},
    route_index::{RouteIndexEntry, RouteIndexKey, shared_route_index},
    rpc_client::{
        CompiledTransaction, configured_rpc_url, fetch_account_owner_and_data_with_null_retry,
        fetch_multiple_account_owner_and_data,
    },
    stable_native::{
        compile_trusted_stable_trade, plan_trusted_stable_trade, trusted_stable_route_for_pool,
    },
    trade_planner::{
        LifecycleAndCanonicalMarket, PlannerQuoteAsset, TradeLifecycle, TradeVenueFamily,
    },
    trade_runtime::TradeRuntimeRequest,
    warm_metrics::{FamilyBucket, shared_warm_metrics},
    warming_service::shared_warming_service,
};

/// Extra retry attempts the route classifier issues when `getAccountInfo`
/// returns `null` for the submitted route input. The first attempt is always
/// made; this counts retries on top of it. Targets the very-fast-click case
/// where the pair has been announced over the venue WS faster than our read
/// RPC slot has surfaced it.
const CLASSIFIER_NULL_RETRY_EXTRA_ATTEMPTS: u32 = 2;
/// Backoff between classifier null-retry attempts.
const CLASSIFIER_NULL_RETRY_BACKOFF_MS: u64 = 50;

#[derive(Debug, Clone, Copy)]
pub enum TradeAdapter {
    PumpNative,
    RaydiumAmmV4Native,
    RaydiumCpmmNative,
    RaydiumLaunchLabNative,
    BonkNative,
    MeteoraNative,
    StableNative,
}

impl TradeAdapter {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PumpNative => "pump-native",
            Self::RaydiumAmmV4Native => "raydium-amm-v4-native",
            Self::RaydiumCpmmNative => "raydium-cpmm-native",
            Self::RaydiumLaunchLabNative => "raydium-launchlab-native",
            Self::BonkNative => "bonk-native",
            Self::MeteoraNative => "meteora-native",
            Self::StableNative => "stable-native",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeInputKind {
    Mint,
    Pair,
}

impl TradeInputKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Mint => "mint",
            Self::Pair => "pair",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TradeDispatchPlan {
    pub adapter: TradeAdapter,
    pub selector: LifecycleAndCanonicalMarket,
    pub execution_backend: TradeExecutionBackend,
    pub raw_address: String,
    pub resolved_input_kind: TradeInputKind,
    /// The mint the planner actually planned against. Usually equal to
    /// `request.mint`, but when the caller supplied a pair / pool address
    /// and the classifier rewrote it, this is the normalized SPL token
    /// mint. `/prewarm` uses this to key the per-mint warm cache so the
    /// subsequent click (which arrives with the real mint) reads the same
    /// fingerprint.
    pub resolved_mint: String,
    /// Pinned pool pubkey the planner honored, if any. When the classifier
    /// rewrote a pair input the pool pubkey ends up here so the cache
    /// fingerprint on the click path can match the prewarm fingerprint.
    pub resolved_pinned_pool: Option<String>,
    pub non_canonical: bool,
}

#[derive(Debug, Clone)]
pub struct CompiledAdapterTrade {
    pub transactions: Vec<CompiledTransaction>,
    pub primary_tx_index: usize,
    pub dependency_mode: TransactionDependencyMode,
    pub entry_preference_asset: Option<TradeSettlementAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionDependencyMode {
    Independent,
    Dependent,
}

#[derive(Debug, Clone)]
pub struct RouteDescriptor {
    pub raw_address: String,
    pub resolved_input_kind: TradeInputKind,
    pub resolved_mint: String,
    pub resolved_pair: Option<String>,
    pub route_locked_pair: Option<String>,
    pub family: Option<TradeVenueFamily>,
    pub lifecycle: Option<TradeLifecycle>,
    pub quote_asset: Option<PlannerQuoteAsset>,
    pub canonical_market_key: Option<String>,
    pub non_canonical: bool,
}

impl RouteDescriptor {
    pub fn from_dispatch_plan(plan: &TradeDispatchPlan) -> Self {
        Self {
            raw_address: plan.raw_address.clone(),
            resolved_input_kind: plan.resolved_input_kind,
            resolved_mint: plan.resolved_mint.clone(),
            resolved_pair: plan.resolved_pinned_pool.clone(),
            route_locked_pair: plan.resolved_pinned_pool.clone(),
            family: Some(plan.selector.family.clone()),
            lifecycle: Some(plan.selector.lifecycle.clone()),
            quote_asset: Some(plan.selector.quote_asset.clone()),
            canonical_market_key: Some(plan.selector.canonical_market_key.clone()),
            non_canonical: plan.non_canonical,
        }
    }
}

pub fn adapter_for_selector(
    selector: &LifecycleAndCanonicalMarket,
) -> Result<TradeAdapter, String> {
    match selector.family {
        TradeVenueFamily::PumpBondingCurve | TradeVenueFamily::PumpAmm => {
            Ok(TradeAdapter::PumpNative)
        }
        TradeVenueFamily::RaydiumAmmV4 => Ok(TradeAdapter::RaydiumAmmV4Native),
        TradeVenueFamily::RaydiumCpmm => Ok(TradeAdapter::RaydiumCpmmNative),
        TradeVenueFamily::RaydiumLaunchLab => Ok(TradeAdapter::RaydiumLaunchLabNative),
        TradeVenueFamily::TrustedStableSwap => Ok(TradeAdapter::StableNative),
        TradeVenueFamily::BonkLaunchpad | TradeVenueFamily::BonkRaydium => {
            Ok(TradeAdapter::BonkNative)
        }
        TradeVenueFamily::MeteoraDbc | TradeVenueFamily::MeteoraDammV2 => {
            Ok(TradeAdapter::MeteoraNative)
        }
    }
}

fn planner_quote_asset_from_label(value: &str) -> Option<PlannerQuoteAsset> {
    match value.trim().to_ascii_lowercase().as_str() {
        "sol" => Some(PlannerQuoteAsset::Sol),
        "wsol" => Some(PlannerQuoteAsset::Wsol),
        "usd1" => Some(PlannerQuoteAsset::Usd1),
        "usdc" => Some(PlannerQuoteAsset::Usdc),
        "usdt" => Some(PlannerQuoteAsset::Usdt),
        _ => None,
    }
}

fn route_input_kind(raw_address: &str, resolved_pair: Option<&str>) -> TradeInputKind {
    if matches!(resolved_pair, Some(pair) if pair == raw_address.trim()) {
        TradeInputKind::Pair
    } else {
        TradeInputKind::Mint
    }
}

/// Classify the authoritative user-supplied address.
///
/// The product contract is intentionally narrow: `address` must be either a
/// token mint or a known pool/pair address. Bonding-curve accounts, token
/// accounts, and other account-like pubkeys are rejected here instead of being
/// translated into a mint.
async fn try_classify_route_descriptor(
    rpc_url: &str,
    input: &str,
    fallback_mint: Option<&str>,
    commitment: &str,
) -> Result<Option<RouteDescriptor>, String> {
    if trusted_stable_route_for_pool(input).is_some() {
        return Ok(None);
    }
    let classify = fetch_account_owner_and_data_with_null_retry(
        rpc_url,
        input,
        commitment,
        CLASSIFIER_NULL_RETRY_EXTRA_ATTEMPTS,
        CLASSIFIER_NULL_RETRY_BACKOFF_MS,
    )
    .await?;
    let Some((owner, data)) = classify else {
        if let Some(mint_hint) = fallback_mint
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != input.trim())
            && let Ok(Some(descriptor)) = Box::pin(try_classify_route_descriptor(
                rpc_url, mint_hint, None, commitment,
            ))
            .await
        {
            return Ok(Some(descriptor));
        }
        return Err(route_error(
            "unsupported_address",
            format!(
                "Address {} was not found; expected a token mint or supported pool/pair.",
                input.trim()
            ),
        ));
    };
    if let Some(classified) =
        classify_pump_bonding_curve_address(rpc_url, input, &owner, &data, commitment).await?
    {
        if classified.complete {
            return Ok(Some(RouteDescriptor {
                raw_address: input.trim().to_string(),
                resolved_input_kind: TradeInputKind::Pair,
                resolved_mint: classified.mint,
                resolved_pair: None,
                route_locked_pair: None,
                family: None,
                lifecycle: None,
                quote_asset: None,
                canonical_market_key: None,
                non_canonical: false,
            }));
        }
        return Ok(Some(RouteDescriptor {
            raw_address: input.trim().to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: classified.mint,
            resolved_pair: Some(classified.bonding_curve.clone()),
            route_locked_pair: Some(classified.bonding_curve.clone()),
            family: Some(TradeVenueFamily::PumpBondingCurve),
            lifecycle: Some(TradeLifecycle::PreMigration),
            quote_asset: Some(PlannerQuoteAsset::Sol),
            canonical_market_key: Some(classified.bonding_curve),
            non_canonical: false,
        }));
    }
    let pump_amm_owner = pump_amm_program_id()?;
    if owner == pump_amm_owner && data.len() >= 245 {
        let pool_pubkey = Pubkey::from_str(input)
            .map_err(|error| format!("input {input} is not a valid pubkey: {error}"))?;
        let pool = decode_pump_amm_pool_state(pool_pubkey, &data)?;
        let base_mint = pool.base_mint.to_string();
        let canonical_pool = crate::pump_native::canonical_pump_amm_pool(&pool.base_mint)?;
        let non_canonical = canonical_pool != pool_pubkey;
        return Ok(Some(RouteDescriptor {
            raw_address: input.trim().to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: base_mint,
            resolved_pair: Some(pool_pubkey.to_string()),
            route_locked_pair: Some(pool_pubkey.to_string()),
            family: Some(TradeVenueFamily::PumpAmm),
            lifecycle: Some(TradeLifecycle::PostMigration),
            quote_asset: Some(PlannerQuoteAsset::Wsol),
            canonical_market_key: Some(canonical_pool.to_string()),
            non_canonical,
        }));
    }
    if owner == raydium_amm_v4_program_id()? {
        if let Some((mint, pool)) =
            classify_raydium_amm_v4_pool_address(rpc_url, input, &data, commitment).await?
        {
            return Ok(Some(RouteDescriptor {
                raw_address: input.trim().to_string(),
                resolved_input_kind: TradeInputKind::Pair,
                resolved_mint: mint,
                resolved_pair: Some(input.trim().to_string()),
                route_locked_pair: Some(input.trim().to_string()),
                family: Some(TradeVenueFamily::RaydiumAmmV4),
                lifecycle: Some(TradeLifecycle::PostMigration),
                quote_asset: Some(PlannerQuoteAsset::Wsol),
                canonical_market_key: Some(pool),
                non_canonical: false,
            }));
        }
    }
    if owner == raydium_launchlab_program_id()?
        && let Some(classified) = classify_raydium_launchlab_pool_address(input, &owner, &data)?
    {
        if matches!(
            classified.status,
            shared_raydium_launchlab::LaunchLabPoolStatus::Migrating
        ) {
            return Err(route_error(
                "migration_in_progress",
                format!(
                    "Raydium LaunchLab pool {} for mint {} is migrating and cannot be traded.",
                    classified.pool_id, classified.mint
                ),
            ));
        }
        let lifecycle = match classified.status {
            shared_raydium_launchlab::LaunchLabPoolStatus::Trading => TradeLifecycle::PreMigration,
            shared_raydium_launchlab::LaunchLabPoolStatus::Migrated => {
                TradeLifecycle::PostMigration
            }
            shared_raydium_launchlab::LaunchLabPoolStatus::Unknown(status) => {
                return Err(route_error(
                    "unsupported_lifecycle",
                    format!(
                        "Raydium LaunchLab pool {} has unsupported status {}.",
                        classified.pool_id, status
                    ),
                ));
            }
            shared_raydium_launchlab::LaunchLabPoolStatus::Migrating => {
                unreachable!("handled above")
            }
        };
        return Ok(Some(RouteDescriptor {
            raw_address: input.trim().to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: classified.mint,
            resolved_pair: Some(classified.pool_id.clone()),
            route_locked_pair: Some(classified.pool_id.clone()),
            family: Some(TradeVenueFamily::RaydiumLaunchLab),
            lifecycle: Some(lifecycle),
            quote_asset: Some(PlannerQuoteAsset::Sol),
            canonical_market_key: Some(classified.pool_id),
            non_canonical: false,
        }));
    }
    if owner == raydium_cpmm_program_id()?
        && let Some(classified) = classify_raydium_cpmm_pool_address(input, &owner, &data)?
    {
        return Ok(Some(RouteDescriptor {
            raw_address: input.trim().to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: classified.mint,
            resolved_pair: Some(classified.pool_id.clone()),
            route_locked_pair: Some(classified.pool_id.clone()),
            family: Some(TradeVenueFamily::RaydiumCpmm),
            lifecycle: Some(TradeLifecycle::PostMigration),
            quote_asset: Some(classified.quote_asset),
            canonical_market_key: Some(classified.pool_id),
            non_canonical: false,
        }));
    }
    if let Some(classified) = classify_bonk_pool_address(input, &owner, &data)? {
        let family = match classified.family.as_str() {
            "launchpad" => TradeVenueFamily::BonkLaunchpad,
            _ => TradeVenueFamily::BonkRaydium,
        };
        let lifecycle = match family {
            TradeVenueFamily::BonkLaunchpad => TradeLifecycle::PreMigration,
            TradeVenueFamily::BonkRaydium => TradeLifecycle::PostMigration,
            _ => unreachable!("bonk route family is restricted"),
        };
        let canonical_market_key = classified.pool_id.clone();
        return Ok(Some(RouteDescriptor {
            raw_address: input.trim().to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: classified.mint,
            resolved_pair: Some(classified.pool_id),
            route_locked_pair: Some(input.trim().to_string()),
            family: Some(family),
            lifecycle: Some(lifecycle),
            quote_asset: planner_quote_asset_from_label(&classified.quote_asset),
            canonical_market_key: Some(canonical_market_key),
            non_canonical: false,
        }));
    }
    if let Some(classified) = classify_bags_pool_address(input, &owner, &data)? {
        let family = match classified.family.as_str() {
            "dbc" => TradeVenueFamily::MeteoraDbc,
            _ => TradeVenueFamily::MeteoraDammV2,
        };
        let lifecycle = match family {
            TradeVenueFamily::MeteoraDbc => TradeLifecycle::PreMigration,
            TradeVenueFamily::MeteoraDammV2 => TradeLifecycle::PostMigration,
            _ => unreachable!("bags route family is restricted"),
        };
        return Ok(Some(RouteDescriptor {
            raw_address: input.trim().to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: classified.mint,
            resolved_pair: Some(classified.market_key),
            route_locked_pair: Some(input.trim().to_string()),
            family: Some(family),
            lifecycle: Some(lifecycle),
            quote_asset: None,
            canonical_market_key: Some(input.trim().to_string()),
            non_canonical: false,
        }));
    }
    let token_owner = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
        .map_err(|error| format!("invalid token program id constant: {error}"))?;
    let token2022_owner = Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb")
        .map_err(|error| format!("invalid token2022 program id constant: {error}"))?;
    if is_supported_token_mint_account(&owner, &data, &token_owner, &token2022_owner) {
        return Ok(Some(RouteDescriptor {
            raw_address: input.trim().to_string(),
            resolved_input_kind: TradeInputKind::Mint,
            resolved_mint: input.trim().to_string(),
            resolved_pair: None,
            route_locked_pair: None,
            family: None,
            lifecycle: None,
            quote_asset: None,
            canonical_market_key: None,
            non_canonical: false,
        }));
    }
    Err(route_error(
        "unsupported_address",
        format!(
            "Address {} is not a token mint or supported pool/pair account.",
            input.trim()
        ),
    ))
}

async fn try_classify_companion_pair_descriptor(
    rpc_url: &str,
    mint: &str,
    pair: Option<&str>,
    commitment: &str,
) -> Result<Option<RouteDescriptor>, String> {
    let Some(pair) = pair.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if pair == mint.trim() {
        return Ok(None);
    }
    let Some(mut descriptor) =
        try_classify_route_descriptor(rpc_url, pair, None, commitment).await?
    else {
        return Ok(None);
    };
    if descriptor.resolved_mint != mint.trim() {
        return Err(route_error(
            "pair_mismatch",
            format!(
                "Selected pair {} resolved to mint {}, but the request targets mint {}.",
                pair, descriptor.resolved_mint, mint
            ),
        ));
    }
    descriptor.raw_address = mint.trim().to_string();
    descriptor.resolved_input_kind = TradeInputKind::Mint;
    Ok(Some(descriptor))
}

fn is_supported_token_mint_account(
    owner: &Pubkey,
    data: &[u8],
    token_owner: &Pubkey,
    token2022_owner: &Pubkey,
) -> bool {
    if owner == token_owner {
        return data.len() == 82;
    }
    owner == token2022_owner && PodStateWithExtensions::<PodMint>::unpack(data).is_ok()
}

pub async fn classify_route_input(
    rpc_url: &str,
    input: &str,
    commitment: &str,
) -> Result<Option<RouteDescriptor>, String> {
    try_classify_route_descriptor(rpc_url, input, None, commitment).await
}

fn route_error(code: &str, message: impl Into<String>) -> String {
    format!("[{code}] {}", message.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteCacheMode {
    AllowCached,
    BypassCached,
}

fn rewrite_request_after_normalization(
    request: &TradeRuntimeRequest,
    descriptor: Option<&RouteDescriptor>,
) -> TradeRuntimeRequest {
    let mut rewritten = request.clone();
    if let Some(descriptor) = descriptor {
        rewritten.mint = descriptor.resolved_mint.clone();
        if rewritten.pinned_pool.is_none() {
            rewritten.pinned_pool = descriptor
                .route_locked_pair
                .clone()
                .or_else(|| descriptor.resolved_pair.clone());
        }
    }
    rewritten
}

fn normalize_request_for_cached_plan(
    request: &TradeRuntimeRequest,
    plan: &TradeDispatchPlan,
) -> TradeRuntimeRequest {
    let mut normalized = request.clone();
    normalized.mint = plan.resolved_mint.clone();
    normalized.pinned_pool = plan.resolved_pinned_pool.clone();
    normalized
}

fn build_dispatch_plan(
    raw_address: &str,
    request: &TradeRuntimeRequest,
    descriptor: Option<&RouteDescriptor>,
    selector: LifecycleAndCanonicalMarket,
) -> Result<TradeDispatchPlan, String> {
    let mut expected_pair = request
        .pinned_pool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| descriptor.and_then(|value| value.route_locked_pair.clone()))
        .or_else(|| descriptor.and_then(|value| value.resolved_pair.clone()));
    let resolved_mint = descriptor
        .map(|value| value.resolved_mint.clone())
        .unwrap_or_else(|| request.mint.clone());
    let non_canonical = descriptor.map(|value| value.non_canonical).unwrap_or(false);
    if matches!(selector.family, TradeVenueFamily::PumpAmm)
        && let (Some(pair), Ok(mint_pubkey)) = (
            expected_pair.as_deref(),
            Pubkey::from_str(resolved_mint.as_str()),
        )
        && bonding_curve_pda(&mint_pubkey)
            .map(|bonding_curve| bonding_curve.to_string() == pair)
            .unwrap_or(false)
    {
        expected_pair = None;
    }
    if matches!(
        selector.family,
        TradeVenueFamily::RaydiumAmmV4 | TradeVenueFamily::RaydiumCpmm
    ) && descriptor.is_some_and(|value| {
        matches!(value.family, Some(TradeVenueFamily::RaydiumLaunchLab))
            && matches!(value.lifecycle, Some(TradeLifecycle::PostMigration))
    }) {
        expected_pair = Some(selector.canonical_market_key.clone());
    }
    if let Some(pair_lock) = expected_pair.as_deref() {
        if non_canonical {
            return Err(route_error(
                "non_canonical_blocked",
                format!(
                    "Selected pair {pair_lock} is not the canonical {} market for mint {}.",
                    selector.family.label(),
                    resolved_mint
                ),
            ));
        }
        let actual_pair = selector.canonical_market_key.trim();
        if !actual_pair.is_empty() && actual_pair != pair_lock {
            return Err(route_error(
                "pair_mismatch",
                format!(
                    "Selected pair {pair_lock} did not match the resolved {} market {} for mint {}.",
                    selector.family.label(),
                    actual_pair,
                    resolved_mint
                ),
            ));
        }
    }
    Ok(TradeDispatchPlan {
        adapter: adapter_for_selector(&selector)?,
        selector,
        execution_backend: preferred_execution_backend(),
        raw_address: raw_address.trim().to_string(),
        resolved_input_kind: descriptor
            .map(|value| value.resolved_input_kind)
            .unwrap_or_else(|| route_input_kind(raw_address, expected_pair.as_deref())),
        resolved_mint,
        resolved_pinned_pool: expected_pair,
        non_canonical,
    })
}

fn build_dispatch_plan_from_warm_entry(
    raw_address: &str,
    request: &TradeRuntimeRequest,
    entry: &PrewarmedMint,
) -> Result<TradeDispatchPlan, String> {
    let cached_plan = entry
        .plan
        .as_ref()
        .cloned()
        .ok_or_else(|| "Warm entry was missing a cached selector.".to_string())?;
    let descriptor = RouteDescriptor {
        raw_address: raw_address.trim().to_string(),
        resolved_input_kind: route_input_kind(raw_address, entry.resolved_pair.as_deref()),
        resolved_mint: entry.mint.clone(),
        resolved_pair: cached_plan
            .resolved_pinned_pool
            .clone()
            .or_else(|| entry.resolved_pair.clone()),
        route_locked_pair: cached_plan.resolved_pinned_pool.clone(),
        non_canonical: cached_plan.non_canonical,
        family: Some(cached_plan.selector.family.clone()),
        lifecycle: Some(cached_plan.selector.lifecycle.clone()),
        quote_asset: Some(cached_plan.selector.quote_asset.clone()),
        canonical_market_key: Some(cached_plan.selector.canonical_market_key.clone()),
    };
    build_dispatch_plan(
        raw_address,
        request,
        Some(&descriptor),
        cached_plan.selector,
    )
}

async fn cache_dispatch_plan_for_request(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    plan: &TradeDispatchPlan,
) {
    let normalized_request = normalize_request_for_cached_plan(request, plan);
    shared_warming_service()
        .cache_selector(
            rpc_url,
            &normalized_request.policy.commitment,
            side_label(&normalized_request.side),
            &route_policy_label(&normalized_request),
            &normalized_request.mint,
            normalized_request.pinned_pool.as_deref(),
            non_canonical_pool_trades_allowed(),
            plan.selector.clone(),
        )
        .await;
    for fingerprint_request in [request, &normalized_request] {
        let fingerprint = build_fingerprint(
            &fingerprint_request.mint,
            fingerprint_request.pinned_pool.as_deref(),
            rpc_url,
            &fingerprint_request.policy.commitment,
            &route_policy_label(fingerprint_request),
            non_canonical_pool_trades_allowed(),
        );
        let entry = prewarmed_from_plan(&fingerprint, plan.resolved_pinned_pool.clone(), plan);
        shared_mint_warm_cache().insert(fingerprint, entry).await;
    }
}

async fn resolve_family_plan(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    family: &TradeVenueFamily,
    cache_mode: RouteCacheMode,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    match (family, cache_mode) {
        (TradeVenueFamily::PumpBondingCurve | TradeVenueFamily::PumpAmm, _) => {
            plan_pump_trade(rpc_url, request).await
        }
        (TradeVenueFamily::RaydiumAmmV4, _) => plan_raydium_amm_v4_trade(rpc_url, request).await,
        (TradeVenueFamily::RaydiumCpmm, _) => plan_raydium_cpmm_trade(rpc_url, request).await,
        (TradeVenueFamily::RaydiumLaunchLab, _) => {
            plan_raydium_launchlab_trade(rpc_url, request).await
        }
        (
            TradeVenueFamily::BonkLaunchpad | TradeVenueFamily::BonkRaydium,
            RouteCacheMode::AllowCached,
        ) => plan_bonk_trade(rpc_url, request).await,
        (
            TradeVenueFamily::BonkLaunchpad | TradeVenueFamily::BonkRaydium,
            RouteCacheMode::BypassCached,
        ) => plan_bonk_trade_uncached(rpc_url, request).await,
        (
            TradeVenueFamily::MeteoraDbc | TradeVenueFamily::MeteoraDammV2,
            RouteCacheMode::AllowCached,
        ) => plan_meteora_trade(rpc_url, request).await,
        (
            TradeVenueFamily::MeteoraDbc | TradeVenueFamily::MeteoraDammV2,
            RouteCacheMode::BypassCached,
        ) => plan_meteora_trade_uncached(rpc_url, request).await,
        (TradeVenueFamily::TrustedStableSwap, _) => plan_trusted_stable_trade(request)
            .await
            .map(|plan| Some(plan.selector)),
    }
}

fn selector_from_authoritative_descriptor(
    request: &TradeRuntimeRequest,
    descriptor: &RouteDescriptor,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    if matches!(descriptor.family, Some(TradeVenueFamily::RaydiumLaunchLab)) {
        if matches!(descriptor.lifecycle, Some(TradeLifecycle::PostMigration)) {
            return Ok(None);
        }
        let Some(pool) = descriptor.resolved_pair.as_deref().or_else(|| {
            descriptor
                .canonical_market_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        }) else {
            return Ok(None);
        };
        return Ok(Some(LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PreMigration,
            family: TradeVenueFamily::RaydiumLaunchLab,
            canonical_market_key: pool.to_string(),
            quote_asset: PlannerQuoteAsset::Sol,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: match request.side {
                crate::extension_api::TradeSide::Buy => {
                    crate::trade_planner::WrapperAction::RaydiumLaunchLabSolBuy
                }
                crate::extension_api::TradeSide::Sell => {
                    crate::trade_planner::WrapperAction::RaydiumLaunchLabSolSell
                }
            },
            wrapper_accounts: vec![pool.to_string()],
            market_subtype: Some("launchlab-active".to_string()),
            direct_protocol_target: Some("raydium-launchlab".to_string()),
            input_amount_hint: request.buy_amount_sol.clone(),
            minimum_output_hint: None,
            runtime_bundle: None,
        }));
    }
    if !matches!(descriptor.family, Some(TradeVenueFamily::BonkRaydium)) {
        return Ok(None);
    }
    let Some(pool) = descriptor.resolved_pair.as_deref().or_else(|| {
        descriptor
            .canonical_market_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    }) else {
        return Ok(None);
    };
    let Some(quote_asset) = descriptor.quote_asset.as_ref() else {
        return Ok(None);
    };
    let quote_asset = match quote_asset {
        PlannerQuoteAsset::Sol | PlannerQuoteAsset::Wsol => "sol",
        PlannerQuoteAsset::Usd1 => "usd1",
        _ => return Ok(None),
    };
    selector_from_classified_bonk_raydium_pair(request, pool, quote_asset).map(Some)
}

fn planner_family_order() -> Vec<TradeVenueFamily> {
    vec![
        TradeVenueFamily::PumpBondingCurve,
        TradeVenueFamily::BonkLaunchpad,
        TradeVenueFamily::RaydiumLaunchLab,
        TradeVenueFamily::MeteoraDbc,
    ]
}

fn preferred_family_for_mint_suffix(mint: &str) -> Option<TradeVenueFamily> {
    let trimmed = mint.trim();
    if trimmed.ends_with("pump") {
        Some(TradeVenueFamily::PumpBondingCurve)
    } else if trimmed.ends_with("bonk") {
        Some(TradeVenueFamily::BonkLaunchpad)
    } else if trimmed.ends_with("BAGS")
        || trimmed.ends_with("brrr")
        || trimmed.ends_with("moon")
        || trimmed.ends_with("daos")
    {
        Some(TradeVenueFamily::MeteoraDbc)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
enum FamilyPlanRaceStatus {
    Pending,
    Miss,
    Hit(LifecycleAndCanonicalMarket),
    Failed(String),
}

fn first_priority_ready_selector(
    statuses: &[FamilyPlanRaceStatus],
) -> Option<LifecycleAndCanonicalMarket> {
    for status in statuses {
        match status {
            FamilyPlanRaceStatus::Pending => return None,
            FamilyPlanRaceStatus::Hit(selector) => return Some(selector.clone()),
            FamilyPlanRaceStatus::Miss | FamilyPlanRaceStatus::Failed(_) => {}
        }
    }
    None
}

async fn race_no_suffix_family_plans(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    cache_mode: RouteCacheMode,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let family_order = planner_family_order();
    let mut statuses = vec![FamilyPlanRaceStatus::Pending; family_order.len()];
    let mut futures = FuturesUnordered::new();
    for (index, family) in family_order.iter().cloned().enumerate() {
        futures.push(async move {
            let result = resolve_family_plan(rpc_url, request, &family, cache_mode).await;
            (index, family, result)
        });
    }

    while let Some((index, family, result)) = futures.next().await {
        match result {
            Ok(Some(selector)) => {
                eprintln!(
                    "[execution-engine][dispatch] priority-race branch resolved mint={} family={}",
                    request.mint,
                    family.label()
                );
                statuses[index] = FamilyPlanRaceStatus::Hit(selector);
            }
            Ok(None) => {
                statuses[index] = FamilyPlanRaceStatus::Miss;
            }
            Err(error) => {
                eprintln!(
                    "[execution-engine][dispatch] priority-race branch failed for mint {} family={}: {}",
                    request.mint,
                    family.label(),
                    error
                );
                statuses[index] = FamilyPlanRaceStatus::Failed(error);
            }
        }

        if let Some(selector) = first_priority_ready_selector(&statuses) {
            eprintln!(
                "[execution-engine][dispatch] priority-race selected mint={} family={} aborting_lower_priority=true",
                request.mint,
                selector.family.label()
            );
            return Ok(Some(selector));
        }
    }

    let branch_errors = family_order
        .iter()
        .zip(statuses.iter())
        .filter_map(|(family, status)| match status {
            FamilyPlanRaceStatus::Failed(error) => Some(format!("{}: {error}", family.label())),
            FamilyPlanRaceStatus::Pending
            | FamilyPlanRaceStatus::Miss
            | FamilyPlanRaceStatus::Hit(_) => None,
        })
        .collect::<Vec<_>>();
    if !branch_errors.is_empty() {
        return Err(route_error(
            "route_resolution_failed",
            format!(
                "No canonical route could be resolved for mint {}. Branch failures: {}",
                request.mint,
                branch_errors.join(" | ")
            ),
        ));
    }
    Ok(None)
}

async fn race_no_suffix_family_plans_with_optional_prefetch(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    cache_mode: RouteCacheMode,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    if !no_suffix_pda_prefetch_enabled() || request.pinned_pool.is_some() {
        return race_no_suffix_family_plans(rpc_url, request, cache_mode).await;
    }

    let prefetch = prefetch_no_suffix_deterministic_accounts(rpc_url, request);
    let race = race_no_suffix_family_plans(rpc_url, request, cache_mode);
    tokio::pin!(prefetch);
    tokio::pin!(race);
    tokio::select! {
        result = &mut race => result,
        _ = &mut prefetch => race.await,
    }
}

pub async fn resolve_trade_plan(
    request: &TradeRuntimeRequest,
) -> Result<TradeDispatchPlan, String> {
    let (result, metrics) =
        crate::route_metrics::collect_route_metrics(resolve_trade_plan_with_route_index(request))
            .await;
    log_route_plan_metrics(request, &result, false, &metrics);
    result
}

pub(crate) async fn resolve_trade_plan_fresh(
    request: &TradeRuntimeRequest,
) -> Result<TradeDispatchPlan, String> {
    let (result, metrics) = crate::route_metrics::collect_route_metrics(async {
        let rpc_url = configured_rpc_url();
        let key = route_index_key_for_request(request, &rpc_url);
        shared_route_index().invalidate(&key).await;
        resolve_trade_plan_with_cache_mode(request, RouteCacheMode::BypassCached).await
    })
    .await;
    log_route_plan_metrics(request, &result, true, &metrics);
    result
}

fn log_route_plan_metrics(
    request: &TradeRuntimeRequest,
    result: &Result<TradeDispatchPlan, String>,
    force_fresh: bool,
    metrics: &crate::route_metrics::RouteMetricsSnapshot,
) {
    let input_kind = route_metrics_input_kind(request, result);
    match result {
        Ok(plan) => {
            eprintln!(
                "[execution-engine][route-metrics] phase=plan mint={} family={} lifecycle={} input_kind={} force_fresh={} elapsed_ms={} rpc_total={} rpc_methods={}",
                request.mint,
                plan.selector.family.label(),
                plan.selector.lifecycle.label(),
                input_kind,
                force_fresh,
                metrics.elapsed_ms,
                metrics.rpc_total(),
                metrics.rpc_methods_json()
            );
        }
        Err(error) => {
            eprintln!(
                "[execution-engine][route-metrics] phase=plan mint={} family=unresolved input_kind={} force_fresh={} elapsed_ms={} rpc_total={} rpc_methods={} error_code={:?}",
                request.mint,
                input_kind,
                force_fresh,
                metrics.elapsed_ms,
                metrics.rpc_total(),
                metrics.rpc_methods_json(),
                route_error_code(error)
            );
        }
    }
}

fn route_metrics_input_kind(
    request: &TradeRuntimeRequest,
    result: &Result<TradeDispatchPlan, String>,
) -> &'static str {
    if request.pinned_pool.as_deref().is_some_and(|pool| {
        let pool = pool.trim();
        !pool.is_empty() && pool != request.mint.trim()
    }) {
        return "mint+pair";
    }
    match result {
        Ok(plan) => plan.resolved_input_kind.label(),
        Err(_) => "raw",
    }
}

async fn resolve_trade_plan_with_route_index(
    request: &TradeRuntimeRequest,
) -> Result<TradeDispatchPlan, String> {
    let rpc_url = configured_rpc_url();
    let key = route_index_key_for_request(request, &rpc_url);
    if let Some(entry) = shared_route_index().current(&key).await
        && family_warm_enabled(&entry.selector.family)
    {
        shared_warm_metrics()
            .record_mint_warm_hit(FamilyBucket::from_venue_family(&entry.selector.family));
        return build_dispatch_plan_from_route_index_entry(request, entry);
    }

    let flight_lock = shared_route_index().flight_lock(&key).await;
    let result = {
        let _guard = flight_lock.lock().await;
        if let Some(entry) = shared_route_index().current(&key).await
            && family_warm_enabled(&entry.selector.family)
        {
            shared_warm_metrics()
                .record_mint_warm_hit(FamilyBucket::from_venue_family(&entry.selector.family));
            build_dispatch_plan_from_route_index_entry(request, entry)
        } else {
            match resolve_trade_plan_with_cache_mode(request, RouteCacheMode::AllowCached).await {
                Ok(plan) => {
                    shared_route_index()
                        .insert_plan(key.clone(), &plan, "click_cold_resolve")
                        .await;
                    Ok(plan)
                }
                Err(error) => Err(error),
            }
        }
    };
    shared_route_index().finish_flight(&key, &flight_lock).await;
    result
}

fn route_index_key_for_request(request: &TradeRuntimeRequest, rpc_url: &str) -> RouteIndexKey {
    RouteIndexKey::new(
        &request.mint,
        rpc_url,
        &request.policy.commitment,
        side_label(&request.side),
        &route_policy_label(request),
        request.pinned_pool.as_deref(),
        non_canonical_pool_trades_allowed(),
    )
}

fn build_dispatch_plan_from_route_index_entry(
    request: &TradeRuntimeRequest,
    entry: RouteIndexEntry,
) -> Result<TradeDispatchPlan, String> {
    let descriptor = RouteDescriptor {
        raw_address: entry.submitted_address.clone(),
        resolved_input_kind: entry.input_kind,
        resolved_mint: entry.resolved_mint.clone(),
        resolved_pair: entry.resolved_pool.clone(),
        route_locked_pair: entry.resolved_pool.clone(),
        family: Some(entry.selector.family.clone()),
        lifecycle: Some(entry.selector.lifecycle.clone()),
        quote_asset: Some(entry.selector.quote_asset.clone()),
        canonical_market_key: Some(entry.selector.canonical_market_key.clone()),
        non_canonical: entry.non_canonical,
    };
    build_dispatch_plan(
        &entry.submitted_address,
        request,
        Some(&descriptor),
        entry.selector,
    )
}

fn route_error_code(error: &str) -> Option<&str> {
    let rest = error.strip_prefix('[')?;
    rest.split_once(']').map(|(code, _)| code)
}

async fn prefetch_classifier_accounts(
    rpc_url: &str,
    mint: &str,
    pinned_pool: Option<&str>,
    commitment: &str,
) {
    let Some(pool) = pinned_pool.map(str::trim).filter(|value| {
        !value.is_empty() && *value != mint.trim() && trusted_stable_route_for_pool(value).is_none()
    }) else {
        return;
    };
    let accounts = vec![mint.trim().to_string(), pool.to_string()];
    if let Err(error) = fetch_multiple_account_owner_and_data(rpc_url, &accounts, commitment).await
    {
        eprintln!(
            "[execution-engine][dispatch] classifier prefetch ignored mint={} pair={} error={}",
            mint, pool, error
        );
    }
}

fn no_suffix_pda_prefetch_enabled() -> bool {
    std::env::var("ROUTE_PDA_PREFLIGHT")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

async fn prefetch_no_suffix_deterministic_accounts(rpc_url: &str, request: &TradeRuntimeRequest) {
    if !no_suffix_pda_prefetch_enabled() || request.pinned_pool.is_some() {
        return;
    }
    let Ok(mint) = Pubkey::from_str(request.mint.trim()) else {
        return;
    };
    let mut accounts = vec![mint.to_string()];
    if let Ok(bonding_curve) = bonding_curve_pda(&mint) {
        accounts.push(bonding_curve.to_string());
    }
    if let Ok(pump_amm_pool) = canonical_pump_amm_pool(&mint) {
        accounts.push(pump_amm_pool.to_string());
    }
    accounts.sort();
    accounts.dedup();
    if accounts.len() <= 1 {
        return;
    }
    let started_at = std::time::Instant::now();
    match fetch_multiple_account_owner_and_data(rpc_url, &accounts, &request.policy.commitment)
        .await
    {
        Ok(results) => {
            let existing = results.iter().filter(|entry| entry.is_some()).count();
            eprintln!(
                "[execution-engine][dispatch] no-suffix-pda-prefetch mint={} accounts={} existing={} elapsed_ms={}",
                request.mint,
                accounts.len(),
                existing,
                started_at.elapsed().as_millis()
            );
        }
        Err(error) => {
            eprintln!(
                "[execution-engine][dispatch] no-suffix-pda-prefetch ignored mint={} error={}",
                request.mint, error
            );
        }
    }
}

async fn resolve_trade_plan_with_cache_mode(
    request: &TradeRuntimeRequest,
    cache_mode: RouteCacheMode,
) -> Result<TradeDispatchPlan, String> {
    let rpc_url = configured_rpc_url();
    let raw_address = request.mint.trim().to_string();

    if trusted_stable_route_for_pool(&raw_address).is_some()
        || request
            .pinned_pool
            .as_deref()
            .is_some_and(|pool| trusted_stable_route_for_pool(pool).is_some())
    {
        return plan_trusted_stable_trade(request).await;
    }

    if matches!(cache_mode, RouteCacheMode::AllowCached)
        && let Some(cached) = lookup_mint_warm(request, &rpc_url).await
        && let Some(plan) = cached
            .plan
            .as_ref()
            .filter(|plan| family_warm_enabled(&plan.selector.family))
    {
        let resolved_bucket = FamilyBucket::from_venue_family(&plan.selector.family);
        shared_warm_metrics().record_mint_warm_hit(resolved_bucket);
        eprintln!(
            "[execution-engine][dispatch] mint-warm hit mint={} family={} key={}",
            request.mint,
            plan.selector.family.label(),
            cached.warm_key
        );
        return build_dispatch_plan_from_warm_entry(&raw_address, request, &cached);
    }

    let (primary_descriptor, companion_descriptor) =
        if request.pinned_pool.as_deref().is_some_and(|pool| {
            let pool = pool.trim();
            !pool.is_empty() && pool != request.mint.trim()
        }) {
            prefetch_classifier_accounts(
                &rpc_url,
                &request.mint,
                request.pinned_pool.as_deref(),
                &request.policy.commitment,
            )
            .await;
            let (primary, companion) = tokio::join!(
                try_classify_route_descriptor(
                    &rpc_url,
                    &request.mint,
                    request.fallback_mint_hint.as_deref(),
                    &request.policy.commitment,
                ),
                try_classify_companion_pair_descriptor(
                    &rpc_url,
                    &request.mint,
                    request.pinned_pool.as_deref(),
                    &request.policy.commitment,
                ),
            );
            (primary?, companion)
        } else {
            (
                try_classify_route_descriptor(
                    &rpc_url,
                    &request.mint,
                    request.fallback_mint_hint.as_deref(),
                    &request.policy.commitment,
                )
                .await?,
                Ok(None),
            )
        };
    let mut descriptor = primary_descriptor;
    let mut ignore_invalid_companion_pair = false;
    if descriptor
        .as_ref()
        .is_none_or(|value| value.family.is_none() && value.resolved_pair.is_none())
    {
        match companion_descriptor {
            Ok(Some(companion_descriptor)) => {
                descriptor = Some(companion_descriptor);
            }
            Ok(None) => {
                ignore_invalid_companion_pair = request.pinned_pool.is_some();
            }
            Err(error) => {
                eprintln!(
                    "[execution-engine][dispatch] optional companion pair ignored for mint {}: {}",
                    request.mint, error
                );
                ignore_invalid_companion_pair = true;
            }
        }
    }
    if let Some(descriptor) = descriptor
        .as_ref()
        .filter(|value| value.resolved_mint != request.mint || value.resolved_pair.is_some())
    {
        let bucket = descriptor
            .family
            .as_ref()
            .map(FamilyBucket::from_venue_family)
            .unwrap_or(FamilyBucket::Pump);
        shared_warm_metrics().record_classifier_fallback(bucket);
    }
    let mut effective_request = rewrite_request_after_normalization(request, descriptor.as_ref());
    if ignore_invalid_companion_pair {
        effective_request.pinned_pool = None;
    }
    if descriptor
        .as_ref()
        .is_some_and(|value| value.non_canonical && effective_request.pinned_pool.is_some())
    {
        let pair_lock = effective_request.pinned_pool.clone().unwrap_or_default();
        let resolved_mint = descriptor
            .as_ref()
            .map(|value| value.resolved_mint.clone())
            .unwrap_or_else(|| effective_request.mint.clone());
        let family_label = descriptor
            .as_ref()
            .and_then(|value| value.family.as_ref())
            .map(TradeVenueFamily::label)
            .unwrap_or("route");
        return Err(route_error(
            "non_canonical_blocked",
            format!(
                "Selected pair {pair_lock} is not the canonical {family_label} market for mint {resolved_mint}."
            ),
        ));
    }

    if matches!(cache_mode, RouteCacheMode::AllowCached)
        && (effective_request.mint != request.mint
            || effective_request.pinned_pool != request.pinned_pool)
        && let Some(cached) = lookup_mint_warm(&effective_request, &rpc_url).await
        && cached
            .plan
            .as_ref()
            .is_some_and(|plan| family_warm_enabled(&plan.selector.family))
    {
        let selector = cached
            .plan
            .as_ref()
            .map(|plan| plan.selector.clone())
            .expect("checked cached selector");
        let resolved_bucket = FamilyBucket::from_venue_family(&selector.family);
        shared_warm_metrics().record_mint_warm_hit(resolved_bucket);
        return build_dispatch_plan_from_warm_entry(&raw_address, &effective_request, &cached);
    }

    if matches!(cache_mode, RouteCacheMode::AllowCached)
        && let Some(cached) = shared_warming_service()
            .current_selector(
                &rpc_url,
                &effective_request.policy.commitment,
                side_label(&effective_request.side),
                &route_policy_label(&effective_request),
                &effective_request.mint,
                effective_request.pinned_pool.as_deref(),
                non_canonical_pool_trades_allowed(),
            )
            .await
        && !cached.is_stale(now_unix_ms())
        && family_warm_enabled(&cached.selector.family)
    {
        let resolved_bucket = FamilyBucket::from_venue_family(&cached.selector.family);
        shared_warm_metrics().record_static_cache_hit(resolved_bucket);
        let dispatch_plan = build_dispatch_plan(
            &raw_address,
            &effective_request,
            descriptor.as_ref(),
            cached.selector,
        )?;
        cache_dispatch_plan_for_request(&rpc_url, &effective_request, &dispatch_plan).await;
        return Ok(dispatch_plan);
    }

    if let Some(authoritative_descriptor) = descriptor.as_ref().filter(|value| {
        value.family.is_some()
            && (value.resolved_input_kind == TradeInputKind::Pair
                || value.resolved_mint != request.mint
                || value.resolved_pair.is_some())
    }) {
        let family = authoritative_descriptor
            .family
            .as_ref()
            .expect("checked authoritative family");
        eprintln!(
            "[execution-engine][dispatch] authoritative-classifier raw={} family={} mint={} pair={:?}",
            request.mint,
            family.label(),
            authoritative_descriptor.resolved_mint,
            authoritative_descriptor.resolved_pair
        );
        if let Some(selector) =
            selector_from_authoritative_descriptor(&effective_request, authoritative_descriptor)?
        {
            let dispatch_plan = build_dispatch_plan(
                &raw_address,
                &effective_request,
                descriptor.as_ref(),
                selector,
            )?;
            cache_dispatch_plan_for_request(&rpc_url, &effective_request, &dispatch_plan).await;
            shared_warm_metrics().record_cold_path(FamilyBucket::from_venue_family(
                &dispatch_plan.selector.family,
            ));
            return Ok(dispatch_plan);
        }
        if let Some(selector) =
            resolve_family_plan(&rpc_url, &effective_request, family, cache_mode).await?
        {
            let dispatch_plan = build_dispatch_plan(
                &raw_address,
                &effective_request,
                descriptor.as_ref(),
                selector,
            )?;
            cache_dispatch_plan_for_request(&rpc_url, &effective_request, &dispatch_plan).await;
            shared_warm_metrics().record_cold_path(FamilyBucket::from_venue_family(
                &dispatch_plan.selector.family,
            ));
            return Ok(dispatch_plan);
        }
        shared_warm_metrics().record_unresolved();
        return Err(route_error(
            "family_resolution_failed",
            format!(
                "Classified {} input {} as {} for mint {}, but the {} planner could not resolve a supported route.",
                authoritative_descriptor.resolved_input_kind.label(),
                request.mint,
                family.label(),
                authoritative_descriptor.resolved_mint,
                family.label(),
            ),
        ));
    }

    if descriptor
        .as_ref()
        .is_none_or(|value| value.resolved_input_kind == TradeInputKind::Mint)
        && effective_request.pinned_pool.is_none()
        && let Some(preferred_family) = preferred_family_for_mint_suffix(&effective_request.mint)
    {
        eprintln!(
            "[execution-engine][dispatch] suffix-fast-path mint={} family={}",
            effective_request.mint,
            preferred_family.label()
        );
        match resolve_family_plan(&rpc_url, &effective_request, &preferred_family, cache_mode).await
        {
            Ok(Some(selector)) => {
                let dispatch_plan = build_dispatch_plan(
                    &raw_address,
                    &effective_request,
                    descriptor.as_ref(),
                    selector,
                )?;
                cache_dispatch_plan_for_request(&rpc_url, &effective_request, &dispatch_plan).await;
                shared_warm_metrics().record_cold_path(FamilyBucket::from_venue_family(
                    &dispatch_plan.selector.family,
                ));
                return Ok(dispatch_plan);
            }
            Ok(None) => {
                eprintln!(
                    "[execution-engine][dispatch] suffix-fast-path miss mint={} family={}",
                    effective_request.mint,
                    preferred_family.label()
                );
            }
            Err(error) => {
                eprintln!(
                    "[execution-engine][dispatch] suffix-fast-path failed mint={} family={}: {}",
                    effective_request.mint,
                    preferred_family.label(),
                    error
                );
            }
        }
    }

    let resolved_selector = {
        eprintln!(
            "[execution-engine][dispatch] mint-fallback mode=priority-race mint={}",
            effective_request.mint
        );
        race_no_suffix_family_plans_with_optional_prefetch(&rpc_url, &effective_request, cache_mode)
            .await?
    };

    if let Some(selector) = resolved_selector {
        let dispatch_plan = build_dispatch_plan(
            &raw_address,
            &effective_request,
            descriptor.as_ref(),
            selector,
        )?;
        cache_dispatch_plan_for_request(&rpc_url, &effective_request, &dispatch_plan).await;
        shared_warm_metrics().record_cold_path(FamilyBucket::from_venue_family(
            &dispatch_plan.selector.family,
        ));
        return Ok(dispatch_plan);
    }

    if let Some(descriptor) = descriptor.as_ref() {
        eprintln!(
            "[execution-engine][dispatch] classifier normalized {} -> mint={} pair={:?} non_canonical={} but no adapter matched",
            request.mint,
            descriptor.resolved_mint,
            descriptor.resolved_pair,
            descriptor.non_canonical
        );
    }
    shared_warm_metrics().record_unresolved();
    eprintln!(
        "[execution-engine][dispatch] no adapter matched address {} (probed pump, bonk, launchlab, bags/meteora mint discovery; explicit Raydium v4 requires a pool address)",
        request.mint
    );
    Err(route_error(
        if descriptor
            .as_ref()
            .is_some_and(|value| value.resolved_input_kind == TradeInputKind::Mint)
        {
            "unsupported_mint"
        } else {
            "unsupported_address"
        },
        format!(
            "No supported execution venue for address {}. Mint inputs probe Pump, Bonk, LaunchLab, and Bags; Raydium AMM v4 requires submitting the verified pool address.",
            request.mint
        ),
    ))
}

fn non_canonical_pool_trades_allowed() -> bool {
    false
}

/// Consult the mint warm cache using only route identity, transport context,
/// and the canonical-only policy bit.
async fn lookup_mint_warm(request: &TradeRuntimeRequest, rpc_url: &str) -> Option<PrewarmedMint> {
    let fingerprint = build_fingerprint(
        &request.mint,
        request.pinned_pool.as_deref(),
        rpc_url,
        &request.policy.commitment,
        &route_policy_label(request),
        non_canonical_pool_trades_allowed(),
    );
    if let Some(warm_key) = request.warm_key.as_deref() {
        if let Some(entry) = shared_mint_warm_cache().current_by_warm_key(warm_key).await {
            if warm_entry_matches_request(&entry, request, rpc_url) {
                return Some(entry);
            }
        }
    }
    if let Some(entry) = shared_mint_warm_cache().current(&fingerprint).await {
        if warm_entry_matches_request(&entry, request, rpc_url) {
            return Some(entry);
        }
    }
    None
}

fn warm_entry_matches_request(
    entry: &PrewarmedMint,
    request: &TradeRuntimeRequest,
    rpc_url: &str,
) -> bool {
    let Some(plan) = entry.plan.as_ref() else {
        return false;
    };
    if entry.allow_non_canonical != non_canonical_pool_trades_allowed() {
        return false;
    }
    if !warm_entry_fingerprint_context_matches_request(entry, request, rpc_url) {
        return false;
    }
    let request_mint = request.mint.trim();
    if request_mint.is_empty() {
        return false;
    }
    let mint_matches_entry = request_mint == entry.mint
        || matches!(entry.resolved_pair.as_deref(), Some(resolved_pair) if resolved_pair == request_mint);
    if !mint_matches_entry {
        return false;
    }
    let request_pool = request
        .pinned_pool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cached_route_lock = plan
        .resolved_pinned_pool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if request_pool != cached_route_lock {
        return false;
    }
    if !selector_matches_trade_side(&plan.selector, &request.side) {
        return false;
    }
    true
}

fn warm_key_field<'a>(warm_key: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    warm_key
        .split('|')
        .find_map(|part| part.strip_prefix(prefix.as_str()))
}

fn warm_entry_fingerprint_context_matches_request(
    entry: &PrewarmedMint,
    request: &TradeRuntimeRequest,
    rpc_url: &str,
) -> bool {
    let expected_policy = route_policy_label(request);
    let expected_commitment = request.policy.commitment.trim().to_ascii_lowercase();
    let expected_nc = if non_canonical_pool_trades_allowed() {
        "1"
    } else {
        "0"
    };

    warm_key_field(&entry.warm_key, "rpc").is_some_and(|rpc| rpc == rpc_url.trim())
        && warm_key_field(&entry.warm_key, "cmt")
            .is_some_and(|commitment| commitment == expected_commitment)
        && warm_key_field(&entry.warm_key, "policy").is_some_and(|policy| policy == expected_policy)
        && warm_key_field(&entry.warm_key, "nc").is_some_and(|nc| nc == expected_nc)
}

fn selector_matches_trade_side(
    selector: &LifecycleAndCanonicalMarket,
    requested_side: &crate::extension_api::TradeSide,
) -> bool {
    matches!(
        (selector.wrapper_action.clone(), requested_side),
        (
            crate::trade_planner::WrapperAction::PumpBondingCurveBuy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::PumpBondingCurveSell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::PumpAmmWsolBuy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::PumpAmmWsolSell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::RaydiumAmmV4WsolBuy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::RaydiumAmmV4WsolSell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::RaydiumCpmmWsolBuy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::RaydiumCpmmWsolSell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::RaydiumLaunchLabSolBuy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::RaydiumLaunchLabSolSell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::BonkLaunchpadSolBuy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::BonkLaunchpadSolSell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::BonkLaunchpadUsd1Buy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::BonkLaunchpadUsd1Sell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::BonkRaydiumSolBuy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::BonkRaydiumSolSell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::BonkRaydiumUsd1Buy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::BonkRaydiumUsd1Sell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::MeteoraDbcBuy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::MeteoraDbcSell,
            crate::extension_api::TradeSide::Sell
        ) | (
            crate::trade_planner::WrapperAction::MeteoraDammV2Buy,
            crate::extension_api::TradeSide::Buy
        ) | (
            crate::trade_planner::WrapperAction::MeteoraDammV2Sell,
            crate::extension_api::TradeSide::Sell
        )
    )
}

fn side_label(side: &crate::extension_api::TradeSide) -> &'static str {
    match side {
        crate::extension_api::TradeSide::Buy => "buy",
        crate::extension_api::TradeSide::Sell => "sell",
    }
}

fn buy_funding_policy_label(policy: crate::extension_api::BuyFundingPolicy) -> &'static str {
    match policy {
        crate::extension_api::BuyFundingPolicy::SolOnly => "sol_only",
        crate::extension_api::BuyFundingPolicy::PreferUsd1ElseTopUp => "prefer_usd1_else_top_up",
        crate::extension_api::BuyFundingPolicy::Usd1Only => "usd1_only",
    }
}

fn sell_settlement_asset_label(asset: TradeSettlementAsset) -> &'static str {
    match asset {
        TradeSettlementAsset::Sol => "sol",
        TradeSettlementAsset::Usd1 => "usd1",
    }
}

fn route_policy_label(request: &TradeRuntimeRequest) -> String {
    let fee_bps = crate::rollout::wrapper_default_fee_bps();
    match request.side {
        crate::extension_api::TradeSide::Buy => {
            format!(
                "buy:{}:wrapper_fee_bps={}:conversion={}",
                buy_funding_policy_label(request.policy.buy_funding_policy),
                fee_bps,
                route_policy_conversion_label(request)
            )
        }
        crate::extension_api::TradeSide::Sell => {
            format!(
                "sell:{}:wrapper_fee_bps={}:conversion={}",
                sell_settlement_asset_label(request.policy.sell_settlement_asset),
                fee_bps,
                route_policy_conversion_label(request)
            )
        }
    }
}

fn route_policy_conversion_label(request: &TradeRuntimeRequest) -> &'static str {
    match request.side {
        crate::extension_api::TradeSide::Buy
            if request.policy.buy_funding_policy
                != crate::extension_api::BuyFundingPolicy::SolOnly =>
        {
            "to_sol_in"
        }
        crate::extension_api::TradeSide::Sell
            if request.policy.sell_settlement_asset
                != crate::extension_api::TradeSettlementAsset::Sol =>
        {
            "to_sol_out"
        }
        _ => "none",
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

pub async fn compile_trade_for_adapter(
    adapter: TradeAdapter,
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    match adapter {
        TradeAdapter::PumpNative => compile_pump_trade(selector, request, wallet_key).await,
        TradeAdapter::RaydiumAmmV4Native => {
            compile_raydium_amm_v4_trade(selector, request, wallet_key).await
        }
        TradeAdapter::RaydiumCpmmNative => {
            compile_raydium_cpmm_trade(selector, request, wallet_key).await
        }
        TradeAdapter::RaydiumLaunchLabNative => {
            compile_raydium_launchlab_trade(selector, request, wallet_key).await
        }
        TradeAdapter::BonkNative => compile_bonk_trade(selector, request, wallet_key).await,
        TradeAdapter::MeteoraNative => compile_meteora_trade(selector, request, wallet_key).await,
        TradeAdapter::StableNative => {
            compile_trusted_stable_trade(selector, request, wallet_key).await
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::{
        extension_api::{MevMode, TradeSide},
        rollout::set_allow_non_canonical_pool_trades,
        trade_runtime::RuntimeExecutionPolicy,
    };

    static NON_CANONICAL_POLICY_TEST_GUARD: Mutex<()> = Mutex::new(());

    fn test_runtime_request() -> TradeRuntimeRequest {
        TradeRuntimeRequest {
            side: TradeSide::Buy,
            mint: "Pool222".to_string(),
            buy_amount_sol: Some("0.5".to_string()),
            sell_intent: None,
            policy: RuntimeExecutionPolicy {
                slippage_percent: "5".to_string(),
                mev_mode: MevMode::Off,
                auto_tip_enabled: false,
                fee_sol: "0".to_string(),
                tip_sol: "0".to_string(),
                provider: "standard-rpc".to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: false,
                track_send_block_height: false,
                buy_funding_policy: crate::extension_api::BuyFundingPolicy::SolOnly,
                sell_settlement_policy: crate::extension_api::SellSettlementPolicy::AlwaysToSol,
                sell_settlement_asset: crate::extension_api::TradeSettlementAsset::Sol,
            },
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: Some("Pool222".to_string()),
            warm_key: None,
            fallback_mint_hint: None,
        }
    }

    fn test_warm_key_for_request(request: &TradeRuntimeRequest) -> String {
        format!(
            "mint=Mint111|pool=Pool222|rpc=test|cmt=confirmed|policy={}|nc=0",
            route_policy_label(request)
        )
    }

    #[test]
    fn route_policy_ignores_source_platform_label() {
        let mut axiom = test_runtime_request();
        axiom.platform_label = Some("axiom".to_string());
        let mut j7 = test_runtime_request();
        j7.platform_label = Some("j7".to_string());

        assert_eq!(route_policy_label(&axiom), route_policy_label(&j7));
        assert!(!route_policy_label(&axiom).contains("platform="));
    }

    fn test_bonk_selector(canonical_market_key: &str) -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: TradeVenueFamily::BonkRaydium,
            canonical_market_key: canonical_market_key.to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Usd1,
            verification_source: crate::trade_planner::PlannerVerificationSource::HybridDerived,
            wrapper_action: crate::trade_planner::WrapperAction::BonkRaydiumUsd1Buy,
            wrapper_accounts: vec![canonical_market_key.to_string()],
            market_subtype: Some("canonical-raydium".to_string()),
            direct_protocol_target: Some("raydium".to_string()),
            input_amount_hint: Some("0.5".to_string()),
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    #[test]
    fn priority_race_waits_for_higher_priority_branches() {
        let bonk = test_bonk_selector("bonk-pool");
        let mut statuses = vec![
            FamilyPlanRaceStatus::Pending,
            FamilyPlanRaceStatus::Hit(bonk.clone()),
            FamilyPlanRaceStatus::Pending,
            FamilyPlanRaceStatus::Pending,
        ];
        assert!(first_priority_ready_selector(&statuses).is_none());

        statuses[0] = FamilyPlanRaceStatus::Miss;
        let selected = first_priority_ready_selector(&statuses).expect("bonk ready");
        assert_eq!(selected.family, TradeVenueFamily::BonkRaydium);
        assert_eq!(selected.canonical_market_key, "bonk-pool");
    }

    #[test]
    fn route_metrics_input_kind_labels_pair_and_mint_pair() {
        let mut mint_request = test_runtime_request();
        mint_request.pinned_pool = None;
        let mut pair_plan = TradeDispatchPlan {
            adapter: TradeAdapter::BonkNative,
            selector: test_bonk_selector("bonk-pool"),
            execution_backend: crate::rollout::TradeExecutionBackend::Native,
            raw_address: "bonk-pool".to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: "Mint111".to_string(),
            resolved_pinned_pool: Some("bonk-pool".to_string()),
            non_canonical: false,
        };
        assert_eq!(
            route_metrics_input_kind(&mint_request, &Ok(pair_plan.clone())),
            "pair"
        );

        pair_plan.resolved_input_kind = TradeInputKind::Mint;
        assert_eq!(
            route_metrics_input_kind(&mint_request, &Ok(pair_plan)),
            "mint"
        );

        let mut mint_pair_request = test_runtime_request();
        mint_pair_request.mint = "Mint111".to_string();
        mint_pair_request.pinned_pool = Some("Pool222".to_string());
        assert_eq!(
            route_metrics_input_kind(
                &mint_pair_request,
                &Err("[unsupported_mint] unsupported".to_string())
            ),
            "mint+pair"
        );
    }

    fn test_launchlab_selector(canonical_market_key: &str) -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PreMigration,
            family: TradeVenueFamily::RaydiumLaunchLab,
            canonical_market_key: canonical_market_key.to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Sol,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::RaydiumLaunchLabSolBuy,
            wrapper_accounts: vec![canonical_market_key.to_string()],
            market_subtype: Some("launchlab-active".to_string()),
            direct_protocol_target: Some("raydium-launchlab".to_string()),
            input_amount_hint: Some("0.5".to_string()),
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    fn test_raydium_amm_v4_selector(canonical_market_key: &str) -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: TradeVenueFamily::RaydiumAmmV4,
            canonical_market_key: canonical_market_key.to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Wsol,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::RaydiumAmmV4WsolBuy,
            wrapper_accounts: vec![canonical_market_key.to_string()],
            market_subtype: Some("amm-v4".to_string()),
            direct_protocol_target: Some("raydium-amm-v4".to_string()),
            input_amount_hint: Some("0.5".to_string()),
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    fn test_raydium_cpmm_selector(canonical_market_key: &str) -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: TradeVenueFamily::RaydiumCpmm,
            canonical_market_key: canonical_market_key.to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Wsol,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::RaydiumCpmmWsolBuy,
            wrapper_accounts: vec![canonical_market_key.to_string()],
            market_subtype: Some("cpmm".to_string()),
            direct_protocol_target: Some("raydium-cpmm".to_string()),
            input_amount_hint: Some("0.5".to_string()),
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    fn test_warm_entry_for_request(request: &TradeRuntimeRequest) -> PrewarmedMint {
        PrewarmedMint {
            mint: "Mint111".to_string(),
            resolved_pair: Some("Pool222".to_string()),
            warm_key: test_warm_key_for_request(request),
            allow_non_canonical: false,
            plan: Some(crate::mint_warm_cache::CachedPlan {
                selector: test_bonk_selector("pool-1"),
                resolved_pinned_pool: Some("Pool222".to_string()),
                non_canonical: false,
            }),
            venue: crate::mint_warm_cache::VenueWarmData::Bonk {
                quote_asset: Some("USD1".to_string()),
                import_context: None,
            },
            warmed_at_unix_ms: 1,
            last_used_at_unix_ms: 1,
        }
    }

    fn test_bonk_pair_descriptor(
        pair: &str,
        canonical_market_key: &str,
        non_canonical: bool,
    ) -> RouteDescriptor {
        RouteDescriptor {
            raw_address: pair.to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: "Mint111".to_string(),
            resolved_pair: Some(pair.to_string()),
            route_locked_pair: Some(pair.to_string()),
            family: Some(TradeVenueFamily::BonkRaydium),
            lifecycle: Some(crate::trade_planner::TradeLifecycle::PostMigration),
            quote_asset: Some(crate::trade_planner::PlannerQuoteAsset::Usd1),
            canonical_market_key: Some(canonical_market_key.to_string()),
            non_canonical,
        }
    }

    fn test_bags_selector(canonical_market_key: &str) -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: TradeVenueFamily::MeteoraDammV2,
            canonical_market_key: canonical_market_key.to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Sol,
            verification_source: crate::trade_planner::PlannerVerificationSource::HybridDerived,
            wrapper_action: crate::trade_planner::WrapperAction::MeteoraDammV2Buy,
            wrapper_accounts: vec![canonical_market_key.to_string()],
            market_subtype: Some("damm".to_string()),
            direct_protocol_target: Some("meteora-damm-v2".to_string()),
            input_amount_hint: Some("0.5".to_string()),
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    fn test_bags_pair_descriptor(
        pair: &str,
        canonical_market_key: &str,
        non_canonical: bool,
    ) -> RouteDescriptor {
        RouteDescriptor {
            raw_address: pair.to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: "Mint111".to_string(),
            resolved_pair: Some(pair.to_string()),
            route_locked_pair: Some(pair.to_string()),
            family: Some(TradeVenueFamily::MeteoraDammV2),
            lifecycle: Some(crate::trade_planner::TradeLifecycle::PostMigration),
            quote_asset: Some(crate::trade_planner::PlannerQuoteAsset::Sol),
            canonical_market_key: Some(canonical_market_key.to_string()),
            non_canonical,
        }
    }

    #[test]
    fn adapter_selection_is_family_driven() {
        assert!(matches!(
            adapter_for_selector(&LifecycleAndCanonicalMarket {
                lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
                family: TradeVenueFamily::MeteoraDammV2,
                canonical_market_key: "pool".to_string(),
                quote_asset: crate::trade_planner::PlannerQuoteAsset::Sol,
                verification_source:
                    crate::trade_planner::PlannerVerificationSource::OnchainDerived,
                wrapper_action: crate::trade_planner::WrapperAction::MeteoraDammV2Buy,
                wrapper_accounts: Vec::new(),
                market_subtype: None,
                direct_protocol_target: None,
                input_amount_hint: None,
                minimum_output_hint: None,
                runtime_bundle: None,
            })
            .expect("adapter"),
            TradeAdapter::MeteoraNative
        ));
        assert!(matches!(
            adapter_for_selector(&LifecycleAndCanonicalMarket {
                lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
                family: TradeVenueFamily::RaydiumAmmV4,
                canonical_market_key: "pool".to_string(),
                quote_asset: crate::trade_planner::PlannerQuoteAsset::Wsol,
                verification_source:
                    crate::trade_planner::PlannerVerificationSource::OnchainDerived,
                wrapper_action: crate::trade_planner::WrapperAction::RaydiumAmmV4WsolBuy,
                wrapper_accounts: Vec::new(),
                market_subtype: Some("amm-v4".to_string()),
                direct_protocol_target: Some("raydium-amm-v4".to_string()),
                input_amount_hint: None,
                minimum_output_hint: None,
                runtime_bundle: None,
            })
            .expect("adapter"),
            TradeAdapter::RaydiumAmmV4Native
        ));
        assert!(matches!(
            adapter_for_selector(&LifecycleAndCanonicalMarket {
                lifecycle: crate::trade_planner::TradeLifecycle::PreMigration,
                family: TradeVenueFamily::RaydiumLaunchLab,
                canonical_market_key: "pool".to_string(),
                quote_asset: crate::trade_planner::PlannerQuoteAsset::Sol,
                verification_source:
                    crate::trade_planner::PlannerVerificationSource::OnchainDerived,
                wrapper_action: crate::trade_planner::WrapperAction::RaydiumLaunchLabSolBuy,
                wrapper_accounts: Vec::new(),
                market_subtype: Some("launchlab-active".to_string()),
                direct_protocol_target: Some("raydium-launchlab".to_string()),
                input_amount_hint: None,
                minimum_output_hint: None,
                runtime_bundle: None,
            })
            .expect("adapter"),
            TradeAdapter::RaydiumLaunchLabNative
        ));
    }

    #[test]
    fn strict_input_contract_accepts_mint_account_not_token_account() {
        let token_owner =
            Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").expect("token program");
        let token2022_owner = Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb")
            .expect("token 2022 program");

        assert!(is_supported_token_mint_account(
            &token_owner,
            &vec![0; 82],
            &token_owner,
            &token2022_owner
        ));
        assert!(!is_supported_token_mint_account(
            &token_owner,
            &vec![0; 165],
            &token_owner,
            &token2022_owner
        ));
    }

    #[test]
    fn warm_entry_matches_resolved_pair_requests() {
        let request = TradeRuntimeRequest {
            side: TradeSide::Buy,
            mint: "Mint111".to_string(),
            buy_amount_sol: Some("0.5".to_string()),
            sell_intent: None,
            policy: RuntimeExecutionPolicy {
                slippage_percent: "5".to_string(),
                mev_mode: MevMode::Off,
                auto_tip_enabled: false,
                fee_sol: "0".to_string(),
                tip_sol: "0".to_string(),
                provider: "standard-rpc".to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: false,
                track_send_block_height: false,
                buy_funding_policy: crate::extension_api::BuyFundingPolicy::SolOnly,
                sell_settlement_policy: crate::extension_api::SellSettlementPolicy::AlwaysToSol,
                sell_settlement_asset: crate::extension_api::TradeSettlementAsset::Sol,
            },
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: Some("Pool222".to_string()),
            warm_key: Some("warm".to_string()),
            fallback_mint_hint: None,
        };
        let entry = PrewarmedMint {
            mint: "Mint111".to_string(),
            resolved_pair: Some("Pool222".to_string()),
            warm_key: test_warm_key_for_request(&request),
            allow_non_canonical: false,
            plan: Some(crate::mint_warm_cache::CachedPlan {
                selector: LifecycleAndCanonicalMarket {
                    lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
                    family: TradeVenueFamily::BonkRaydium,
                    canonical_market_key: "pool-1".to_string(),
                    quote_asset: crate::trade_planner::PlannerQuoteAsset::Usd1,
                    verification_source:
                        crate::trade_planner::PlannerVerificationSource::HybridDerived,
                    wrapper_action: crate::trade_planner::WrapperAction::BonkRaydiumUsd1Buy,
                    wrapper_accounts: vec!["pool-1".to_string()],
                    market_subtype: Some("canonical-raydium".to_string()),
                    direct_protocol_target: Some("raydium".to_string()),
                    input_amount_hint: Some("0.5".to_string()),
                    minimum_output_hint: None,
                    runtime_bundle: None,
                },
                resolved_pinned_pool: Some("Pool222".to_string()),
                non_canonical: false,
            }),
            venue: crate::mint_warm_cache::VenueWarmData::Bonk {
                quote_asset: Some("USD1".to_string()),
                import_context: None,
            },
            warmed_at_unix_ms: 1,
            last_used_at_unix_ms: 1,
        };
        assert!(warm_entry_matches_request(&entry, &request, "test"));
    }

    #[test]
    fn warm_entry_rejects_added_pair_lock() {
        let request = TradeRuntimeRequest {
            side: TradeSide::Buy,
            mint: "Mint111".to_string(),
            buy_amount_sol: Some("0.5".to_string()),
            sell_intent: None,
            policy: RuntimeExecutionPolicy {
                slippage_percent: "5".to_string(),
                mev_mode: MevMode::Off,
                auto_tip_enabled: false,
                fee_sol: "0".to_string(),
                tip_sol: "0".to_string(),
                provider: "standard-rpc".to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: false,
                track_send_block_height: false,
                buy_funding_policy: crate::extension_api::BuyFundingPolicy::SolOnly,
                sell_settlement_policy: crate::extension_api::SellSettlementPolicy::AlwaysToSol,
                sell_settlement_asset: crate::extension_api::TradeSettlementAsset::Sol,
            },
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: Some("Pool222".to_string()),
            warm_key: Some("warm".to_string()),
            fallback_mint_hint: None,
        };
        let entry = PrewarmedMint {
            mint: "Mint111".to_string(),
            resolved_pair: None,
            warm_key: test_warm_key_for_request(&request),
            allow_non_canonical: false,
            plan: Some(crate::mint_warm_cache::CachedPlan {
                selector: LifecycleAndCanonicalMarket {
                    lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
                    family: TradeVenueFamily::BonkRaydium,
                    canonical_market_key: "pool-1".to_string(),
                    quote_asset: crate::trade_planner::PlannerQuoteAsset::Usd1,
                    verification_source:
                        crate::trade_planner::PlannerVerificationSource::HybridDerived,
                    wrapper_action: crate::trade_planner::WrapperAction::BonkRaydiumUsd1Buy,
                    wrapper_accounts: vec!["pool-1".to_string()],
                    market_subtype: Some("canonical-raydium".to_string()),
                    direct_protocol_target: Some("raydium".to_string()),
                    input_amount_hint: Some("0.5".to_string()),
                    minimum_output_hint: None,
                    runtime_bundle: None,
                },
                resolved_pinned_pool: None,
                non_canonical: false,
            }),
            venue: crate::mint_warm_cache::VenueWarmData::Bonk {
                quote_asset: Some("USD1".to_string()),
                import_context: None,
            },
            warmed_at_unix_ms: 1,
            last_used_at_unix_ms: 1,
        };

        assert!(!warm_entry_matches_request(&entry, &request, "test"));
    }

    #[test]
    fn build_dispatch_plan_rejects_pair_mismatch() {
        let request = test_runtime_request();
        let descriptor = test_bonk_pair_descriptor("Pool222", "pool-1", false);
        let error = build_dispatch_plan(
            "Pool222",
            &request,
            Some(&descriptor),
            test_bonk_selector("pool-1"),
        )
        .expect_err("pair mismatch should fail");

        assert!(error.contains("[pair_mismatch]"));
    }

    #[test]
    fn build_dispatch_plan_accepts_canonical_cpmm_pair() {
        let request = test_runtime_request();
        let descriptor = test_bonk_pair_descriptor("Pool222", "Pool222", false);

        let plan = build_dispatch_plan(
            "Pool222",
            &request,
            Some(&descriptor),
            test_bonk_selector("Pool222"),
        )
        .expect("canonical pair should pass");

        assert_eq!(plan.resolved_mint, "Mint111");
        assert_eq!(plan.resolved_pinned_pool.as_deref(), Some("Pool222"));
        assert!(!plan.non_canonical);
    }

    #[test]
    fn build_dispatch_plan_rejects_non_canonical_cpmm_pair() {
        let request = test_runtime_request();
        let descriptor = test_bonk_pair_descriptor("Pool222", "pool-1", true);

        let error = build_dispatch_plan(
            "Pool222",
            &request,
            Some(&descriptor),
            test_bonk_selector("pool-1"),
        )
        .expect_err("non-canonical pair should fail");

        assert!(error.contains("[non_canonical_blocked]"));
    }

    #[test]
    fn build_dispatch_plan_blocks_bonk_non_canonical_pair_even_when_flag_enabled() {
        let _guard = NON_CANONICAL_POLICY_TEST_GUARD
            .lock()
            .expect("policy guard");
        set_allow_non_canonical_pool_trades(true);

        let request = test_runtime_request();
        let descriptor = test_bonk_pair_descriptor("Pool222", "pool-1", true);
        let error = build_dispatch_plan(
            "Pool222",
            &request,
            Some(&descriptor),
            test_bonk_selector("pool-1"),
        )
        .expect_err("bonk non-canonical pair should stay blocked");

        set_allow_non_canonical_pool_trades(false);
        assert!(error.contains("[non_canonical_blocked]"));
    }

    #[test]
    fn build_dispatch_plan_blocks_bags_non_canonical_pair_even_when_flag_enabled() {
        let _guard = NON_CANONICAL_POLICY_TEST_GUARD
            .lock()
            .expect("policy guard");
        set_allow_non_canonical_pool_trades(true);

        let request = test_runtime_request();
        let descriptor = test_bags_pair_descriptor("Pool222", "pool-1", true);
        let error = build_dispatch_plan(
            "Pool222",
            &request,
            Some(&descriptor),
            test_bags_selector("pool-1"),
        )
        .expect_err("bags non-canonical pair should stay blocked");

        set_allow_non_canonical_pool_trades(false);
        assert!(error.contains("[non_canonical_blocked]"));
    }

    #[test]
    fn build_dispatch_plan_accepts_canonical_clmm_pair_when_selector_matches() {
        let mut request = test_runtime_request();
        request.mint = "PoolClmm".to_string();
        request.pinned_pool = Some("PoolClmm".to_string());
        let descriptor = RouteDescriptor {
            quote_asset: Some(crate::trade_planner::PlannerQuoteAsset::Sol),
            ..test_bonk_pair_descriptor("PoolClmm", "PoolClmm", false)
        };
        let selector = LifecycleAndCanonicalMarket {
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Sol,
            ..test_bonk_selector("PoolClmm")
        };

        let plan = build_dispatch_plan("PoolClmm", &request, Some(&descriptor), selector)
            .expect("canonical clmm pair should pass");

        assert_eq!(plan.resolved_pinned_pool.as_deref(), Some("PoolClmm"));
        assert!(!plan.non_canonical);
    }

    #[test]
    fn warm_entry_rejects_cached_non_canonical_route_metadata() {
        let request = TradeRuntimeRequest {
            side: TradeSide::Buy,
            mint: "Pool222".to_string(),
            buy_amount_sol: Some("0.5".to_string()),
            sell_intent: None,
            policy: RuntimeExecutionPolicy {
                slippage_percent: "5".to_string(),
                mev_mode: MevMode::Off,
                auto_tip_enabled: false,
                fee_sol: "0".to_string(),
                tip_sol: "0".to_string(),
                provider: "standard-rpc".to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: false,
                track_send_block_height: false,
                buy_funding_policy: crate::extension_api::BuyFundingPolicy::SolOnly,
                sell_settlement_policy: crate::extension_api::SellSettlementPolicy::AlwaysToSol,
                sell_settlement_asset: crate::extension_api::TradeSettlementAsset::Sol,
            },
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: Some("Pool222".to_string()),
            warm_key: Some("warm".to_string()),
            fallback_mint_hint: None,
        };
        let entry = PrewarmedMint {
            mint: "Mint111".to_string(),
            resolved_pair: Some("Pool222".to_string()),
            warm_key: test_warm_key_for_request(&request),
            allow_non_canonical: false,
            plan: Some(crate::mint_warm_cache::CachedPlan {
                selector: LifecycleAndCanonicalMarket {
                    lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
                    family: TradeVenueFamily::BonkRaydium,
                    canonical_market_key: "Pool222".to_string(),
                    quote_asset: crate::trade_planner::PlannerQuoteAsset::Usd1,
                    verification_source:
                        crate::trade_planner::PlannerVerificationSource::HybridDerived,
                    wrapper_action: crate::trade_planner::WrapperAction::BonkRaydiumUsd1Buy,
                    wrapper_accounts: vec!["Pool222".to_string()],
                    market_subtype: Some("non-canonical-raydium".to_string()),
                    direct_protocol_target: Some("raydium".to_string()),
                    input_amount_hint: Some("0.5".to_string()),
                    minimum_output_hint: None,
                    runtime_bundle: None,
                },
                resolved_pinned_pool: Some("Pool222".to_string()),
                non_canonical: true,
            }),
            venue: crate::mint_warm_cache::VenueWarmData::Bonk {
                quote_asset: Some("USD1".to_string()),
                import_context: None,
            },
            warmed_at_unix_ms: 1,
            last_used_at_unix_ms: 1,
        };

        let error = build_dispatch_plan_from_warm_entry("Pool222", &request, &entry)
            .expect_err("non-canonical warm plan should fail");

        assert!(error.contains("[non_canonical_blocked]"));
    }

    #[test]
    fn warm_entry_rejects_policy_change() {
        let request = TradeRuntimeRequest {
            side: TradeSide::Buy,
            mint: "Mint111".to_string(),
            buy_amount_sol: Some("0.5".to_string()),
            sell_intent: None,
            policy: RuntimeExecutionPolicy {
                slippage_percent: "5".to_string(),
                mev_mode: MevMode::Off,
                auto_tip_enabled: false,
                fee_sol: "0".to_string(),
                tip_sol: "0".to_string(),
                provider: "standard-rpc".to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: false,
                track_send_block_height: false,
                buy_funding_policy: crate::extension_api::BuyFundingPolicy::SolOnly,
                sell_settlement_policy: crate::extension_api::SellSettlementPolicy::AlwaysToSol,
                sell_settlement_asset: crate::extension_api::TradeSettlementAsset::Sol,
            },
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: Some("Pool222".to_string()),
            warm_key: Some("warm".to_string()),
            fallback_mint_hint: None,
        };
        let entry = PrewarmedMint {
            mint: "Mint111".to_string(),
            resolved_pair: Some("Pool222".to_string()),
            warm_key: "mint=Mint111|pool=Pool222|rpc=test|cmt=confirmed|policy=buy:usd1_only|nc=0"
                .to_string(),
            allow_non_canonical: false,
            plan: Some(crate::mint_warm_cache::CachedPlan {
                selector: LifecycleAndCanonicalMarket {
                    lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
                    family: TradeVenueFamily::BonkRaydium,
                    canonical_market_key: "pool-1".to_string(),
                    quote_asset: crate::trade_planner::PlannerQuoteAsset::Usd1,
                    verification_source:
                        crate::trade_planner::PlannerVerificationSource::HybridDerived,
                    wrapper_action: crate::trade_planner::WrapperAction::BonkRaydiumUsd1Buy,
                    wrapper_accounts: vec!["pool-1".to_string()],
                    market_subtype: Some("canonical-raydium".to_string()),
                    direct_protocol_target: Some("raydium".to_string()),
                    input_amount_hint: Some("0.5".to_string()),
                    minimum_output_hint: None,
                    runtime_bundle: None,
                },
                resolved_pinned_pool: Some("Pool222".to_string()),
                non_canonical: false,
            }),
            venue: crate::mint_warm_cache::VenueWarmData::Bonk {
                quote_asset: Some("USD1".to_string()),
                import_context: None,
            },
            warmed_at_unix_ms: 1,
            last_used_at_unix_ms: 1,
        };

        assert!(!warm_entry_matches_request(&entry, &request, "test"));
    }

    #[test]
    fn warm_entry_rejects_rpc_change() {
        let mut request = test_runtime_request();
        request.mint = "Mint111".to_string();
        request.pinned_pool = Some("Pool222".to_string());
        let entry = test_warm_entry_for_request(&request);

        assert!(!warm_entry_matches_request(&entry, &request, "other-rpc"));
    }

    #[test]
    fn warm_entry_rejects_commitment_change() {
        let mut request = test_runtime_request();
        request.mint = "Mint111".to_string();
        request.pinned_pool = Some("Pool222".to_string());
        let entry = test_warm_entry_for_request(&request);
        request.policy.commitment = "finalized".to_string();

        assert!(!warm_entry_matches_request(&entry, &request, "test"));
    }

    #[test]
    fn warm_entry_rejects_mismatched_trade_side() {
        let request = TradeRuntimeRequest {
            side: TradeSide::Sell,
            mint: "Mint111".to_string(),
            buy_amount_sol: None,
            sell_intent: Some(crate::trade_runtime::RuntimeSellIntent::Percent(
                "50".to_string(),
            )),
            policy: RuntimeExecutionPolicy {
                slippage_percent: "5".to_string(),
                mev_mode: MevMode::Off,
                auto_tip_enabled: false,
                fee_sol: "0".to_string(),
                tip_sol: "0".to_string(),
                provider: "standard-rpc".to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: false,
                track_send_block_height: false,
                buy_funding_policy: crate::extension_api::BuyFundingPolicy::SolOnly,
                sell_settlement_policy: crate::extension_api::SellSettlementPolicy::AlwaysToSol,
                sell_settlement_asset: crate::extension_api::TradeSettlementAsset::Sol,
            },
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: Some("Pool222".to_string()),
            warm_key: Some("warm".to_string()),
            fallback_mint_hint: None,
        };
        let entry = PrewarmedMint {
            mint: "Mint111".to_string(),
            resolved_pair: Some("Pool222".to_string()),
            warm_key: test_warm_key_for_request(&request),
            allow_non_canonical: false,
            plan: Some(crate::mint_warm_cache::CachedPlan {
                selector: LifecycleAndCanonicalMarket {
                    lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
                    family: TradeVenueFamily::BonkRaydium,
                    canonical_market_key: "pool-1".to_string(),
                    quote_asset: crate::trade_planner::PlannerQuoteAsset::Usd1,
                    verification_source:
                        crate::trade_planner::PlannerVerificationSource::HybridDerived,
                    wrapper_action: crate::trade_planner::WrapperAction::BonkRaydiumUsd1Buy,
                    wrapper_accounts: vec!["pool-1".to_string()],
                    market_subtype: Some("canonical-raydium".to_string()),
                    direct_protocol_target: Some("raydium".to_string()),
                    input_amount_hint: Some("0.5".to_string()),
                    minimum_output_hint: None,
                    runtime_bundle: None,
                },
                resolved_pinned_pool: Some("Pool222".to_string()),
                non_canonical: false,
            }),
            venue: crate::mint_warm_cache::VenueWarmData::Bonk {
                quote_asset: Some("USD1".to_string()),
                import_context: None,
            },
            warmed_at_unix_ms: 1,
            last_used_at_unix_ms: 1,
        };
        assert!(!warm_entry_matches_request(&entry, &request, "test"));
    }

    #[test]
    fn pair_descriptor_rewrite_sets_verified_route_identity_only() {
        let request = test_runtime_request();
        let descriptor = test_bonk_pair_descriptor("Pool222", "Pool222", false);

        let rewritten = rewrite_request_after_normalization(&request, Some(&descriptor));

        assert_eq!(rewritten.mint, "Mint111");
        assert_eq!(rewritten.pinned_pool.as_deref(), Some("Pool222"));
    }

    #[test]
    fn planner_family_order_includes_launchlab_without_suffix_fast_path() {
        let order = planner_family_order();

        assert!(matches!(
            order.first(),
            Some(TradeVenueFamily::PumpBondingCurve)
        ));
        assert!(matches!(
            order.get(2),
            Some(TradeVenueFamily::RaydiumLaunchLab)
        ));
        assert_eq!(preferred_family_for_mint_suffix("TokenLaunchLab"), None);
    }

    #[test]
    fn meteora_suffixes_use_generic_meteora_fast_path() {
        for mint in [
            "5pmJkpsb78BbGKczXzzXwmQ1xuZS4prUkm6YwrQKbrrr",
            "4168osQ3gt5hLET4vFJxiJ3Tw1ZJA1anHbjdex5Amoon",
            "B816wyHj3bcfFnYzGRGkRpFEfE974CHLHSptzo9sdaos",
            "CkAmbNo5pLZT4eziCrvckpXcPRPMPuqDqpifmzT7BAGS",
        ] {
            assert_eq!(
                preferred_family_for_mint_suffix(mint),
                Some(TradeVenueFamily::MeteoraDbc)
            );
        }
    }

    #[test]
    fn concurrent_family_tiebreak_uses_stable_order() {
        let pump_selector = LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: TradeVenueFamily::PumpAmm,
            canonical_market_key: "pump-pool".to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Wsol,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::PumpAmmWsolBuy,
            wrapper_accounts: vec!["pump-pool".to_string()],
            market_subtype: Some("canonical-pump-amm".to_string()),
            direct_protocol_target: Some("pump-amm".to_string()),
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        };

        let statuses = vec![
            FamilyPlanRaceStatus::Hit(pump_selector),
            FamilyPlanRaceStatus::Miss,
            FamilyPlanRaceStatus::Miss,
            FamilyPlanRaceStatus::Hit(test_bags_selector("bags-pool")),
        ];
        let selected = first_priority_ready_selector(&statuses).expect("expected selector");

        assert!(matches!(selected.family, TradeVenueFamily::PumpAmm));
        assert_eq!(selected.canonical_market_key, "pump-pool");
    }

    #[test]
    fn launchlab_tiebreaks_before_bags_when_both_match() {
        let statuses = vec![
            FamilyPlanRaceStatus::Miss,
            FamilyPlanRaceStatus::Miss,
            FamilyPlanRaceStatus::Hit(test_launchlab_selector("launchlab-pool")),
            FamilyPlanRaceStatus::Hit(test_bags_selector("bags-pool")),
        ];
        let selected = first_priority_ready_selector(&statuses).expect("expected selector");

        assert!(matches!(
            selected.family,
            TradeVenueFamily::RaydiumLaunchLab
        ));
        assert_eq!(selected.canonical_market_key, "launchlab-pool");
    }

    #[test]
    fn migrated_launchlab_pair_resolves_to_verified_raydium_destination() {
        let request = test_runtime_request();
        let descriptor = RouteDescriptor {
            raw_address: "launchlab-pool".to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: "Mint111".to_string(),
            resolved_pair: Some("launchlab-pool".to_string()),
            route_locked_pair: Some("launchlab-pool".to_string()),
            family: Some(TradeVenueFamily::RaydiumLaunchLab),
            lifecycle: Some(crate::trade_planner::TradeLifecycle::PostMigration),
            quote_asset: Some(crate::trade_planner::PlannerQuoteAsset::Sol),
            canonical_market_key: Some("launchlab-pool".to_string()),
            non_canonical: false,
        };

        let plan = build_dispatch_plan(
            "launchlab-pool",
            &request,
            Some(&descriptor),
            test_raydium_amm_v4_selector("raydium-amm-v4-pool"),
        )
        .expect("plan");

        assert_eq!(
            plan.resolved_pinned_pool.as_deref(),
            Some("raydium-amm-v4-pool")
        );
        assert!(matches!(
            plan.selector.family,
            TradeVenueFamily::RaydiumAmmV4
        ));
    }

    #[test]
    fn migrated_launchlab_pair_resolves_to_verified_cpmm_destination() {
        let request = test_runtime_request();
        let descriptor = RouteDescriptor {
            raw_address: "launchlab-pool".to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: "Mint111".to_string(),
            resolved_pair: Some("launchlab-pool".to_string()),
            route_locked_pair: Some("launchlab-pool".to_string()),
            family: Some(TradeVenueFamily::RaydiumLaunchLab),
            lifecycle: Some(crate::trade_planner::TradeLifecycle::PostMigration),
            quote_asset: Some(crate::trade_planner::PlannerQuoteAsset::Sol),
            canonical_market_key: Some("launchlab-pool".to_string()),
            non_canonical: false,
        };

        let plan = build_dispatch_plan(
            "launchlab-pool",
            &request,
            Some(&descriptor),
            test_raydium_cpmm_selector("raydium-cpmm-pool"),
        )
        .expect("plan");

        assert_eq!(
            plan.resolved_pinned_pool.as_deref(),
            Some("raydium-cpmm-pool")
        );
        assert!(matches!(plan.adapter, TradeAdapter::RaydiumCpmmNative));
        assert!(matches!(
            plan.selector.family,
            TradeVenueFamily::RaydiumCpmm
        ));
    }
}
