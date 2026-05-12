use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, Instant},
};

use shared_extension_runtime::follow_contract::BagsLaunchMetadata;
use solana_sdk::signature::Signer;
use tokio::sync::Mutex;

use crate::{
    bags_execution_support as launchdeck_bags,
    extension_api::TradeSide,
    launchdeck_bridge::{map_compiled_transaction, to_launchdeck_execution},
    mint_warm_cache::{VenueWarmData, shared_mint_warm_cache},
    provider_tip::pick_tip_account_for_provider,
    trade_dispatch::{CompiledAdapterTrade, TransactionDependencyMode},
    trade_planner::{
        BagsRuntimeBundle, LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerRuntimeBundle,
        PlannerVerificationSource, TradeLifecycle, TradeVenueFamily, WrapperAction,
    },
    trade_runtime::{RuntimeSellIntent, TradeRuntimeRequest},
    wallet_store::load_solana_wallet_by_env_key,
    wrapper_payload::parse_sol_amount_to_lamports,
};

const BAGS_IMPORT_CONTEXT_TTL: Duration = Duration::from_millis(2_500);

pub type BagsImportContext = launchdeck_bags::BagsImportContext;
pub type BagsPoolAddressClassification = launchdeck_bags::BagsPoolAddressClassification;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportContextCacheMode {
    AllowCached,
    BypassCached,
}

#[derive(Debug, Clone)]
struct CachedBagsImportContext {
    context: launchdeck_bags::BagsImportContext,
    fetched_at: Instant,
}

#[derive(Debug, Clone)]
struct CachedBagsFollowBuyContext {
    context: launchdeck_bags::BagsFollowBuyContext,
    fetched_at: Instant,
}

fn bags_import_context_cache() -> &'static Mutex<HashMap<String, CachedBagsImportContext>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedBagsImportContext>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bags_follow_buy_context_cache() -> &'static Mutex<HashMap<String, CachedBagsFollowBuyContext>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedBagsFollowBuyContext>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn classify_bags_pool_address(
    input: &str,
    owner: &solana_sdk::pubkey::Pubkey,
    data: &[u8],
) -> Result<Option<BagsPoolAddressClassification>, String> {
    launchdeck_bags::classify_bags_pool_address(input, owner, data)
}

fn bags_import_context_cache_key(rpc_url: &str, mint: &str, pinned_pool: Option<&str>) -> String {
    format!(
        "{}|{}|{}",
        rpc_url.trim(),
        mint.trim(),
        pinned_pool.unwrap_or_default().trim()
    )
}

async fn cached_bags_import_context(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Option<launchdeck_bags::BagsImportContext> {
    let key = bags_import_context_cache_key(rpc_url, &request.mint, request.pinned_pool.as_deref());
    let mut cache = bags_import_context_cache().lock().await;
    if let Some(entry) = cache.get(&key) {
        if entry.fetched_at.elapsed() <= BAGS_IMPORT_CONTEXT_TTL {
            return Some(entry.context.clone());
        }
    }
    cache.remove(&key);
    None
}

async fn warmed_bags_import_context(
    request: &TradeRuntimeRequest,
) -> Option<launchdeck_bags::BagsImportContext> {
    let warm_key = request.warm_key.as_deref()?.trim();
    if warm_key.is_empty() {
        return None;
    }
    let entry = shared_mint_warm_cache()
        .current_by_warm_key(warm_key)
        .await?;
    match entry.venue {
        VenueWarmData::Bags {
            import_context: Some(context),
            ..
        } => Some(context),
        _ => None,
    }
}

async fn cache_bags_import_context(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    context: &launchdeck_bags::BagsImportContext,
) {
    bags_import_context_cache().lock().await.insert(
        bags_import_context_cache_key(rpc_url, &request.mint, request.pinned_pool.as_deref()),
        CachedBagsImportContext {
            context: context.clone(),
            fetched_at: Instant::now(),
        },
    );
}

async fn warmed_bags_follow_buy_context(
    request: &TradeRuntimeRequest,
) -> Option<launchdeck_bags::BagsFollowBuyContext> {
    let warm_key = request.warm_key.as_deref()?.trim();
    if warm_key.is_empty() {
        return None;
    }
    let mut cache = bags_follow_buy_context_cache().lock().await;
    if let Some(entry) = cache.get(warm_key) {
        if entry.fetched_at.elapsed() <= BAGS_IMPORT_CONTEXT_TTL {
            return Some(entry.context.clone());
        }
    }
    cache.remove(warm_key);
    None
}

async fn cache_bags_follow_buy_context(
    warm_key: &str,
    context: &launchdeck_bags::BagsFollowBuyContext,
) {
    let key = warm_key.trim();
    if key.is_empty() {
        return;
    }
    bags_follow_buy_context_cache().lock().await.insert(
        key.to_string(),
        CachedBagsFollowBuyContext {
            context: context.clone(),
            fetched_at: Instant::now(),
        },
    );
}

fn bags_context_is_executable(context: &launchdeck_bags::BagsImportContext) -> bool {
    !context.marketKey.trim().is_empty()
        && !context.venue.trim().is_empty()
        && (!context.configKey.trim().is_empty()
            || context.launchMetadata.as_ref().is_some_and(|metadata| {
                !metadata.expectedDammDerivationMode.trim().is_empty()
                    && context.venue.to_ascii_lowercase().contains("damm")
            }))
}

async fn load_bags_import_context(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    cache_mode: ImportContextCacheMode,
) -> Result<Option<launchdeck_bags::BagsImportContext>, String> {
    if let Some(context) = warmed_bags_import_context(request).await {
        cache_bags_import_context(rpc_url, request, &context).await;
        return Ok(Some(context));
    }
    if matches!(cache_mode, ImportContextCacheMode::AllowCached) {
        if let Some(context) = cached_bags_import_context(rpc_url, request).await {
            return Ok(Some(context));
        }
    }
    if let Some(pool) = request
        .pinned_pool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && let Some(context) = launchdeck_bags::detect_bags_import_context_from_pool(
            rpc_url,
            &request.mint,
            pool,
            &request.policy.commitment,
        )
        .await?
    {
        cache_bags_import_context(rpc_url, request, &context).await;
        return Ok(Some(context));
    }
    let detected = launchdeck_bags::detect_bags_import_context(rpc_url, &request.mint).await?;
    let context = if detected.as_ref().is_some_and(bags_context_is_executable) {
        detected
    } else {
        match request.pinned_pool.as_deref() {
            Some(pool) => launchdeck_bags::detect_bags_import_context_from_pool(
                rpc_url,
                &request.mint,
                pool,
                &request.policy.commitment,
            )
            .await?
            .or(detected),
            None => detected,
        }
    };
    let Some(context) = context else {
        return Ok(None);
    };
    if !bags_context_is_executable(&context) {
        return Ok(None);
    }
    cache_bags_import_context(rpc_url, request, &context).await;
    Ok(Some(context))
}

async fn plan_meteora_trade_with_cache_mode(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    cache_mode: ImportContextCacheMode,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let Some(context) = load_bags_import_context(rpc_url, request, cache_mode).await? else {
        return Ok(None);
    };
    let bags_launch = context
        .launchMetadata
        .clone()
        .unwrap_or_else(|| launch_metadata_from_context(&context));
    Ok(Some(map_bags_context_to_selector(
        request,
        context,
        bags_launch,
    )))
}

pub async fn plan_meteora_trade(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    plan_meteora_trade_with_cache_mode(rpc_url, request, ImportContextCacheMode::AllowCached).await
}

pub(crate) async fn plan_meteora_trade_uncached(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    plan_meteora_trade_with_cache_mode(rpc_url, request, ImportContextCacheMode::BypassCached).await
}

pub async fn compile_meteora_trade(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    let rpc_url = crate::rpc_client::configured_rpc_url();
    let owner = load_solana_wallet_by_env_key(wallet_key)?;
    let owner_bytes = owner.to_bytes();
    let launchdeck_execution = to_launchdeck_execution(&request.policy);
    let tip_account = pick_tip_account_for_provider(&request.policy.provider);
    let bags_launch = selector_to_bags_launch(selector);
    let direct_protocol_target = selector
        .direct_protocol_target
        .as_deref()
        .unwrap_or_else(|| selector.family.label());
    let quote_asset = selector.quote_asset.label();
    let compiled = match request.side {
        TradeSide::Buy => {
            let buy_amount_sol = request
                .buy_amount_sol
                .as_deref()
                .ok_or_else(|| "Missing buyAmountSol for Meteora buy request.".to_string())?;
            let context_started_at = Instant::now();
            let follow_buy_context = match warmed_bags_follow_buy_context(request).await {
                Some(context) => Some(context),
                None => {
                    let loaded = launchdeck_bags::load_follow_buy_context(
                        &rpc_url,
                        &request.mint,
                        &launchdeck_execution.commitment,
                        Some(&bags_launch),
                    )
                    .await?;
                    if let (Some(warm_key), Some(context)) =
                        (request.warm_key.as_deref(), loaded.as_ref())
                    {
                        cache_bags_follow_buy_context(warm_key, context).await;
                    }
                    loaded
                }
            };
            crate::route_metrics::record_phase_ms(
                "context_fetch",
                context_started_at.elapsed().as_millis(),
            );
            launchdeck_bags::compile_follow_buy_transaction_for_meteora_target(
                &rpc_url,
                &launchdeck_execution,
                &tip_account,
                &owner_bytes,
                &request.mint,
                buy_amount_sol,
                Some(&bags_launch),
                follow_buy_context.as_ref(),
                direct_protocol_target,
                quote_asset,
            )
            .await?
        }
        TradeSide::Sell => {
            let (sell_percent, token_amount_override) = resolve_meteora_sell_amount(
                &rpc_url,
                selector,
                request,
                wallet_key,
                &owner.pubkey().to_string(),
                request
                    .sell_intent
                    .as_ref()
                    .ok_or_else(|| "Missing sell intent for Meteora sell request.".to_string())?,
            )
            .await?;
            launchdeck_bags::compile_follow_sell_transaction_for_meteora_target(
                &rpc_url,
                &launchdeck_execution,
                &tip_account,
                &owner_bytes,
                &request.mint,
                sell_percent,
                token_amount_override,
                Some(&bags_launch),
                direct_protocol_target,
                quote_asset,
            )
            .await?
        }
    };
    Ok(CompiledAdapterTrade {
        transactions: vec![map_compiled_transaction(compiled)],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: None,
    })
}

pub async fn warm_meteora_compile_snapshot(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    warm_key: &str,
) -> Result<bool, String> {
    let key = warm_key.trim();
    if key.is_empty() || !matches!(request.side, TradeSide::Buy) {
        return Ok(false);
    }
    let rpc_url = crate::rpc_client::configured_rpc_url();
    let launchdeck_execution = to_launchdeck_execution(&request.policy);
    let bags_launch = selector_to_bags_launch(selector);
    let context_started_at = Instant::now();
    let Some(context) = launchdeck_bags::load_follow_buy_context(
        &rpc_url,
        &request.mint,
        &launchdeck_execution.commitment,
        Some(&bags_launch),
    )
    .await?
    else {
        return Ok(false);
    };
    crate::route_metrics::record_phase_ms(
        "context_fetch",
        context_started_at.elapsed().as_millis(),
    );
    cache_bags_follow_buy_context(key, &context).await;
    Ok(true)
}

fn map_bags_context_to_selector(
    request: &TradeRuntimeRequest,
    context: launchdeck_bags::BagsImportContext,
    bags_launch: BagsLaunchMetadata,
) -> LifecycleAndCanonicalMarket {
    let is_damm = context.venue.to_ascii_lowercase().contains("damm");
    let family = if is_damm {
        TradeVenueFamily::MeteoraDammV2
    } else {
        TradeVenueFamily::MeteoraDbc
    };
    let wrapper_action = match (family.clone(), request.side.clone()) {
        (TradeVenueFamily::MeteoraDbc, TradeSide::Buy) => WrapperAction::MeteoraDbcBuy,
        (TradeVenueFamily::MeteoraDbc, TradeSide::Sell) => WrapperAction::MeteoraDbcSell,
        (TradeVenueFamily::MeteoraDammV2, TradeSide::Buy) => WrapperAction::MeteoraDammV2Buy,
        (TradeVenueFamily::MeteoraDammV2, TradeSide::Sell) => WrapperAction::MeteoraDammV2Sell,
        _ => unreachable!("meteora selector family is restricted"),
    };
    let mut wrapper_accounts = vec![context.marketKey.clone()];
    if !context.configKey.trim().is_empty() {
        wrapper_accounts.push(context.configKey.clone());
    }
    if !context.creator.trim().is_empty() {
        wrapper_accounts.push(context.creator.clone());
    }
    let quote_asset = match context.quoteAsset.trim().to_ascii_lowercase().as_str() {
        "usdc" => PlannerQuoteAsset::Usdc,
        "wsol" => PlannerQuoteAsset::Wsol,
        _ => PlannerQuoteAsset::Sol,
    };
    LifecycleAndCanonicalMarket {
        lifecycle: if is_damm {
            TradeLifecycle::PostMigration
        } else {
            TradeLifecycle::PreMigration
        },
        family,
        canonical_market_key: context.marketKey,
        quote_asset,
        verification_source: PlannerVerificationSource::OnchainDerived,
        wrapper_action,
        wrapper_accounts,
        market_subtype: Some(if context.mode.trim().is_empty() {
            context.venue.clone()
        } else {
            context.mode
        }),
        direct_protocol_target: Some(if is_damm {
            "meteora-damm-v2".to_string()
        } else {
            "meteora-dbc".to_string()
        }),
        input_amount_hint: request.buy_amount_sol.clone(),
        minimum_output_hint: match &request.sell_intent {
            Some(RuntimeSellIntent::SolOutput(value)) => Some(value.clone()),
            _ => None,
        },
        runtime_bundle: Some(PlannerRuntimeBundle::Bags(BagsRuntimeBundle {
            bags_launch,
        })),
    }
}

fn launch_metadata_from_context(
    context: &launchdeck_bags::BagsImportContext,
) -> BagsLaunchMetadata {
    let is_damm = context.venue.to_ascii_lowercase().contains("damm");
    BagsLaunchMetadata {
        configKey: context.configKey.clone(),
        migrationFeeOption: None,
        expectedMigrationFamily: if is_damm {
            "damm-v2".to_string()
        } else {
            "dbc".to_string()
        },
        expectedDammConfigKey: context.configKey.clone(),
        expectedDammDerivationMode: context.mode.clone(),
        preMigrationDbcPoolAddress: if is_damm {
            String::new()
        } else {
            context.marketKey.clone()
        },
        postMigrationDammPoolAddress: if is_damm {
            context.marketKey.clone()
        } else {
            String::new()
        },
    }
}

pub(crate) fn selector_to_bags_launch(
    selector: &LifecycleAndCanonicalMarket,
) -> BagsLaunchMetadata {
    if let Some(PlannerRuntimeBundle::Bags(bundle)) = selector.runtime_bundle.as_ref() {
        return bundle.bags_launch.clone();
    }
    BagsLaunchMetadata {
        configKey: selector
            .wrapper_accounts
            .get(1)
            .cloned()
            .unwrap_or_default(),
        migrationFeeOption: None,
        expectedMigrationFamily: match selector.family {
            TradeVenueFamily::MeteoraDbc => "dbc".to_string(),
            TradeVenueFamily::MeteoraDammV2 => "damm-v2".to_string(),
            _ => String::new(),
        },
        expectedDammConfigKey: selector
            .wrapper_accounts
            .get(1)
            .cloned()
            .unwrap_or_default(),
        expectedDammDerivationMode: selector.market_subtype.clone().unwrap_or_default(),
        preMigrationDbcPoolAddress: if matches!(selector.family, TradeVenueFamily::MeteoraDbc) {
            selector.canonical_market_key.clone()
        } else {
            String::new()
        },
        postMigrationDammPoolAddress: if matches!(selector.family, TradeVenueFamily::MeteoraDammV2)
        {
            selector.canonical_market_key.clone()
        } else {
            String::new()
        },
    }
}

pub(crate) fn selector_to_bags_import_context(
    selector: &LifecycleAndCanonicalMarket,
) -> Option<launchdeck_bags::BagsImportContext> {
    let venue = match selector.family {
        TradeVenueFamily::MeteoraDbc => "Meteora Dynamic Bonding Curve",
        TradeVenueFamily::MeteoraDammV2 => "Meteora DAMM v2",
        _ => return None,
    };
    Some(launchdeck_bags::BagsImportContext {
        launchpad: "meteora".to_string(),
        mode: selector.market_subtype.clone().unwrap_or_default(),
        quoteAsset: match selector.quote_asset {
            PlannerQuoteAsset::Usdc => "usdc".to_string(),
            PlannerQuoteAsset::Wsol => "wsol".to_string(),
            _ => "sol".to_string(),
        },
        creator: selector_launch_creator(selector).unwrap_or_default(),
        marketKey: selector.canonical_market_key.clone(),
        configKey: selector
            .wrapper_accounts
            .get(1)
            .cloned()
            .unwrap_or_default(),
        venue: venue.to_string(),
        detectionSource: "cached-selector".to_string(),
        feeRecipients: Vec::new(),
        notes: Vec::new(),
        launchMetadata: Some(selector_to_bags_launch(selector)),
    })
}

fn selector_launch_creator(selector: &LifecycleAndCanonicalMarket) -> Option<String> {
    match selector.wrapper_accounts.as_slice() {
        [_, creator] => Some(creator.clone()),
        [_, _, creator, ..] => Some(creator.clone()),
        _ => None,
    }
}

fn parse_sell_percent(intent: &RuntimeSellIntent) -> Result<u8, String> {
    match intent {
        RuntimeSellIntent::Percent(value) => {
            let parsed = value.trim().parse::<u8>().map_err(|_| {
                format!("Meteora sell percent must be an integer between 1 and 100: {value}")
            })?;
            if parsed == 0 || parsed > 100 {
                return Err(format!(
                    "Meteora sell percent must be between 1 and 100: {value}"
                ));
            }
            Ok(parsed)
        }
        RuntimeSellIntent::SolOutput(_) => Err(
            "Meteora native sell currently supports percent-based exits only; sellOutputSol remains guarded."
                .to_string(),
        ),
    }
}

async fn resolve_meteora_sell_amount(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
    owner: &str,
    intent: &RuntimeSellIntent,
) -> Result<(u8, Option<u64>), String> {
    match intent {
        RuntimeSellIntent::Percent(_) => Ok((parse_sell_percent(intent)?, None)),
        RuntimeSellIntent::SolOutput(value) => {
            let target_lamports = parse_sol_amount_to_lamports(value);
            if target_lamports == 0 {
                return Err("sellOutputSol must be greater than zero.".to_string());
            }
            let decimals = read_mint_decimals(
                &crate::rpc_client::fetch_account_data(
                    rpc_url,
                    &request.mint,
                    &request.policy.commitment,
                )
                .await?,
            )?;
            let balance = crate::wallet_token_cache::fetch_token_balance_with_cache(
                Some(wallet_key),
                owner,
                &request.mint,
                decimals,
            )
            .await?;
            let bags_launch = selector_to_bags_launch(selector);
            let amount = choose_meteora_target_sized_amount(
                rpc_url,
                selector,
                request,
                Some(&bags_launch),
                balance.amount_raw,
                target_lamports,
            )
            .await?;
            Ok((100, Some(amount)))
        }
    }
}

async fn choose_meteora_target_sized_amount(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    bags_launch: Option<&BagsLaunchMetadata>,
    available_raw: u64,
    target_lamports: u64,
) -> Result<u64, String> {
    if available_raw == 0 {
        return Err("You have 0 tokens.".to_string());
    }
    let full_quote =
        quote_meteora_target_sell_value_sol(rpc_url, selector, request, bags_launch, available_raw)
            .await?;
    if full_quote == 0 {
        return Err("Meteora sell quote resolved to zero SOL.".to_string());
    }
    if full_quote < target_lamports {
        return Err(crate::sell_target_sizing::unreachable_target_message(
            target_lamports,
            full_quote,
        ));
    }

    let mut best = Some((available_raw, full_quote));
    let mut low = 1u64;
    let mut high = available_raw.saturating_sub(1);
    let estimate = crate::sell_target_sizing::target_amount_estimate(
        available_raw,
        target_lamports,
        full_quote,
    );
    if estimate < available_raw {
        let quoted =
            quote_meteora_target_sell_value_sol(rpc_url, selector, request, bags_launch, estimate)
                .await?;
        if quoted == 0 {
            low = estimate.saturating_add(1);
        } else {
            best = Some(crate::sell_target_sizing::prefer_better_target_amount(
                best,
                estimate,
                quoted,
                target_lamports,
            ));
            if quoted < target_lamports {
                low = estimate.saturating_add(1);
            } else {
                high = estimate.saturating_sub(1);
            }
        }
    }
    for _ in 0..crate::sell_target_sizing::RPC_TARGET_SIZING_MAX_REFINEMENT_PROBES {
        if low > high {
            break;
        }
        let amount = low + (high - low) / 2;
        let quoted =
            quote_meteora_target_sell_value_sol(rpc_url, selector, request, bags_launch, amount)
                .await?;
        if quoted == 0 {
            low = amount.saturating_add(1);
            continue;
        }
        best = Some(crate::sell_target_sizing::prefer_better_target_amount(
            best,
            amount,
            quoted,
            target_lamports,
        ));
        if quoted < target_lamports {
            low = amount.saturating_add(1);
        } else {
            high = amount - 1;
        }
    }
    best.map(|(amount, _)| amount)
        .ok_or_else(|| "Meteora sell quote resolved to zero SOL.".to_string())
}

async fn quote_meteora_target_sell_value_sol(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    bags_launch: Option<&BagsLaunchMetadata>,
    amount: u64,
) -> Result<u64, String> {
    let gross_sol_lamports = launchdeck_bags::quote_bags_target_sell_value_sol(
        rpc_url,
        &request.mint,
        amount,
        &request.policy.commitment,
        bags_launch,
        selector
            .direct_protocol_target
            .as_deref()
            .unwrap_or_else(|| selector.family.label()),
        selector.quote_asset.label(),
    )
    .await?;
    crate::sell_target_sizing::net_sol_after_wrapper_fee(gross_sol_lamports)
}

fn read_mint_decimals(data: &[u8]) -> Result<u8, String> {
    data.get(44)
        .copied()
        .ok_or_else(|| "Mint account data was shorter than expected (decimals).".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_damm_context_to_post_migration_selector() {
        let selector = map_bags_context_to_selector(
            &TradeRuntimeRequest {
                side: TradeSide::Sell,
                mint: "mint".to_string(),
                buy_amount_sol: None,
                sell_intent: Some(RuntimeSellIntent::Percent("50".to_string())),
                policy: crate::trade_runtime::RuntimeExecutionPolicy {
                    slippage_percent: "5".to_string(),
                    mev_mode: crate::extension_api::MevMode::Off,
                    auto_tip_enabled: false,
                    fee_sol: "0".to_string(),
                    tip_sol: "0".to_string(),
                    provider: String::new(),
                    endpoint_profile: String::new(),
                    commitment: "processed".to_string(),
                    skip_preflight: false,
                    track_send_block_height: false,
                    buy_funding_policy: crate::extension_api::BuyFundingPolicy::SolOnly,
                    sell_settlement_policy: crate::extension_api::SellSettlementPolicy::AlwaysToSol,
                    sell_settlement_asset: crate::extension_api::TradeSettlementAsset::Sol,
                },
                platform_label: None,
                planned_route: None,
                planned_trade: None,
                pinned_pool: None,
                warm_key: None,
                fallback_mint_hint: None,
            },
            launchdeck_bags::BagsImportContext {
                launchpad: "bagsapp".to_string(),
                mode: "default".to_string(),
                quoteAsset: "sol".to_string(),
                creator: "creator".to_string(),
                marketKey: "market".to_string(),
                configKey: "config".to_string(),
                venue: "Meteora DAMM v2".to_string(),
                detectionSource: "bags-state+rpc-damm-v2".to_string(),
                feeRecipients: Vec::new(),
                notes: Vec::new(),
                launchMetadata: None,
            },
            BagsLaunchMetadata {
                configKey: "config".to_string(),
                ..Default::default()
            },
        );
        assert_eq!(selector.family, TradeVenueFamily::MeteoraDammV2);
        assert_eq!(selector.lifecycle, TradeLifecycle::PostMigration);
        assert_eq!(selector.wrapper_action, WrapperAction::MeteoraDammV2Sell);
    }

    #[test]
    fn selector_to_bags_launch_prefers_runtime_bundle_metadata() {
        let selector = LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PostMigration,
            family: TradeVenueFamily::MeteoraDammV2,
            canonical_market_key: "market".to_string(),
            quote_asset: PlannerQuoteAsset::Sol,
            verification_source: PlannerVerificationSource::OnchainDerived,
            wrapper_action: WrapperAction::MeteoraDammV2Buy,
            wrapper_accounts: vec!["market".to_string(), "fallback-config".to_string()],
            market_subtype: Some("fallback-mode".to_string()),
            direct_protocol_target: Some("meteora-damm-v2".to_string()),
            input_amount_hint: Some("0.5".to_string()),
            minimum_output_hint: None,
            runtime_bundle: Some(PlannerRuntimeBundle::Bags(BagsRuntimeBundle {
                bags_launch: BagsLaunchMetadata {
                    configKey: "runtime-config".to_string(),
                    migrationFeeOption: Some(7),
                    expectedMigrationFamily: "damm-v2".to_string(),
                    expectedDammConfigKey: "runtime-damm-config".to_string(),
                    expectedDammDerivationMode: "runtime-derivation".to_string(),
                    preMigrationDbcPoolAddress: "dbc-pool".to_string(),
                    postMigrationDammPoolAddress: "damm-pool".to_string(),
                },
            })),
        };

        let bags_launch = selector_to_bags_launch(&selector);

        assert_eq!(bags_launch.configKey, "runtime-config");
        assert_eq!(bags_launch.expectedDammConfigKey, "runtime-damm-config");
        assert_eq!(bags_launch.expectedDammDerivationMode, "runtime-derivation");
        assert_eq!(bags_launch.preMigrationDbcPoolAddress, "dbc-pool");
        assert_eq!(bags_launch.migrationFeeOption, Some(7));
    }

    #[test]
    fn rejects_bags_context_without_executable_canonical_market() {
        let context = launchdeck_bags::BagsImportContext {
            launchpad: "bagsapp".to_string(),
            mode: "default".to_string(),
            quoteAsset: "sol".to_string(),
            creator: "creator".to_string(),
            marketKey: String::new(),
            configKey: "config".to_string(),
            venue: "Meteora DAMM v2".to_string(),
            detectionSource: "bags-state".to_string(),
            feeRecipients: Vec::new(),
            notes: Vec::new(),
            launchMetadata: None,
        };
        assert!(!bags_context_is_executable(&context));
    }
}
