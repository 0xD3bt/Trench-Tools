use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, Instant},
};

use solana_sdk::signature::Signer;
use tokio::sync::Mutex;

use crate::{
    bonk_execution_support as launchdeck_bonk,
    extension_api::{TradeSettlementAsset, TradeSide},
    launchdeck_bridge::{map_compiled_transaction, to_launchdeck_execution},
    mint_warm_cache::{VenueWarmData, shared_mint_warm_cache},
    provider_tip::pick_tip_account_for_provider,
    trade_dispatch::{CompiledAdapterTrade, TransactionDependencyMode},
    trade_planner::{
        LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerVerificationSource, TradeLifecycle,
        TradeVenueFamily, WrapperAction,
    },
    trade_runtime::{RuntimeSellIntent, TradeRuntimeRequest},
    wallet_store::load_solana_wallet_by_env_key,
    wrapper_payload::parse_sol_amount_to_lamports,
};

const BONK_IMPORT_CONTEXT_TTL: Duration = Duration::from_millis(2_500);

pub type BonkImportContext = launchdeck_bonk::BonkImportContext;
pub type BonkPoolAddressClassification = launchdeck_bonk::BonkPoolAddressClassification;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportContextCacheMode {
    AllowCached,
    BypassCached,
}

#[derive(Debug, Clone)]
struct CachedBonkImportContext {
    context: launchdeck_bonk::BonkImportContext,
    fetched_at: Instant,
}

fn bonk_import_context_cache() -> &'static Mutex<HashMap<String, CachedBonkImportContext>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedBonkImportContext>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn classify_bonk_pool_address(
    input: &str,
    owner: &solana_sdk::pubkey::Pubkey,
    data: &[u8],
) -> Result<Option<BonkPoolAddressClassification>, String> {
    launchdeck_bonk::classify_bonk_pool_address(input, owner, data)
}

pub(crate) fn selector_from_classified_bonk_raydium_pair(
    request: &TradeRuntimeRequest,
    pool_id: &str,
    quote_asset: &str,
) -> Result<LifecycleAndCanonicalMarket, String> {
    map_bonk_context_to_selector(
        request,
        launchdeck_bonk::BonkImportContext {
            launchpad: "bonk".to_string(),
            mode: "classified-pair".to_string(),
            quoteAsset: quote_asset.trim().to_ascii_lowercase(),
            creator: String::new(),
            platformId: String::new(),
            configId: String::new(),
            poolId: pool_id.trim().to_string(),
            detectionSource: "raydium-classified-pair".to_string(),
        },
    )
}

pub async fn quote_sol_lamports_for_exact_usd1_input(
    rpc_url: &str,
    usd1_raw: u64,
) -> Result<u64, String> {
    launchdeck_bonk::quote_sol_lamports_for_exact_usd1_input(rpc_url, usd1_raw).await
}

fn bonk_import_context_cache_key(rpc_url: &str, mint: &str, pinned_pool: Option<&str>) -> String {
    format!(
        "{}|{}|{}",
        rpc_url.trim(),
        mint.trim(),
        pinned_pool.unwrap_or_default().trim()
    )
}

async fn cached_bonk_import_context(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Option<launchdeck_bonk::BonkImportContext> {
    let key = bonk_import_context_cache_key(rpc_url, &request.mint, request.pinned_pool.as_deref());
    let mut cache = bonk_import_context_cache().lock().await;
    if let Some(entry) = cache.get(&key) {
        if entry.fetched_at.elapsed() <= BONK_IMPORT_CONTEXT_TTL {
            return Some(entry.context.clone());
        }
    }
    cache.remove(&key);
    None
}

async fn warmed_bonk_import_context(
    request: &TradeRuntimeRequest,
) -> Option<launchdeck_bonk::BonkImportContext> {
    let warm_key = request.warm_key.as_deref()?.trim();
    if warm_key.is_empty() {
        return None;
    }
    let entry = shared_mint_warm_cache()
        .current_by_warm_key(warm_key)
        .await?;
    match entry.venue {
        VenueWarmData::Bonk {
            import_context: Some(context),
            ..
        } => Some(context),
        _ => None,
    }
}

async fn cache_bonk_import_context(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    context: &launchdeck_bonk::BonkImportContext,
) {
    bonk_import_context_cache().lock().await.insert(
        bonk_import_context_cache_key(rpc_url, &request.mint, request.pinned_pool.as_deref()),
        CachedBonkImportContext {
            context: context.clone(),
            fetched_at: Instant::now(),
        },
    );
}

async fn bonk_import_context_from_pinned_pool(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<launchdeck_bonk::BonkImportContext>, String> {
    let Some(pool) = request
        .pinned_pool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let Some((owner, data)) =
        crate::rpc_client::fetch_account_owner_and_data(rpc_url, pool, &request.policy.commitment)
            .await?
    else {
        return Ok(None);
    };
    let Some(classified) = classify_bonk_pool_address(pool, &owner, &data)? else {
        return Ok(None);
    };
    if classified.mint != request.mint {
        return Err(format!(
            "Selected Bonk pair {} resolved to mint {}, but the request targets mint {}.",
            pool, classified.mint, request.mint
        ));
    }
    if classified.family != "raydium" || classified.quote_asset.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(launchdeck_bonk::BonkImportContext {
        launchpad: "bonk".to_string(),
        mode: "classified-pair".to_string(),
        quoteAsset: classified.quote_asset,
        creator: String::new(),
        platformId: String::new(),
        configId: String::new(),
        poolId: classified.pool_id,
        detectionSource: "raydium-classified-pair".to_string(),
    }))
}

fn describe_bonk_context(context: &launchdeck_bonk::BonkImportContext) -> String {
    format!(
        "{} route pool={} source={}",
        context.quoteAsset, context.poolId, context.detectionSource
    )
}

async fn detect_bonk_import_context_for_request(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<launchdeck_bonk::BonkImportContext>, String> {
    let (usd1_result, sol_result) = tokio::join!(
        launchdeck_bonk::detect_bonk_import_context_with_quote_asset(
            rpc_url,
            &request.mint,
            "usd1"
        ),
        launchdeck_bonk::detect_bonk_import_context_with_quote_asset(rpc_url, &request.mint, "sol"),
    );
    match (usd1_result, sol_result) {
        (Ok(Some(usd1_context)), Ok(Some(sol_context))) => Err(format!(
            "Bonk canonical quote ambiguity for mint {}: {} and {} both resolved.",
            request.mint,
            describe_bonk_context(&usd1_context),
            describe_bonk_context(&sol_context)
        )),
        (Ok(Some(usd1_context)), sol_result) => {
            if let Err(error) = sol_result {
                eprintln!(
                    "[execution-engine][bonk] SOL quote probe failed while USD1 route resolved for mint {}: {}",
                    request.mint, error
                );
            }
            Ok(Some(usd1_context))
        }
        (usd1_result, Ok(Some(sol_context))) => {
            if let Err(error) = usd1_result {
                eprintln!(
                    "[execution-engine][bonk] USD1 quote probe failed while SOL route resolved for mint {}: {}",
                    request.mint, error
                );
            }
            Ok(Some(sol_context))
        }
        (Ok(None), Ok(None)) => Ok(None),
        (Ok(None), Err(sol_error)) => Err(sol_error),
        (Err(usd1_error), Ok(None)) => Err(usd1_error),
        (Err(usd1_error), Err(sol_error)) => Err(format!(
            "Bonk quote probes failed for mint {}. usd1: {}; sol: {}",
            request.mint, usd1_error, sol_error
        )),
    }
}

async fn load_bonk_import_context(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    cache_mode: ImportContextCacheMode,
) -> Result<Option<launchdeck_bonk::BonkImportContext>, String> {
    if let Some(context) = warmed_bonk_import_context(request).await {
        cache_bonk_import_context(rpc_url, request, &context).await;
        return Ok(Some(context));
    }
    if matches!(cache_mode, ImportContextCacheMode::AllowCached) {
        if let Some(context) = cached_bonk_import_context(rpc_url, request).await {
            return Ok(Some(context));
        }
    }
    if let Some(context) = bonk_import_context_from_pinned_pool(rpc_url, request).await? {
        cache_bonk_import_context(rpc_url, request, &context).await;
        return Ok(Some(context));
    }
    let context = detect_bonk_import_context_for_request(rpc_url, request).await?;
    if let Some(context) = context.as_ref() {
        cache_bonk_import_context(rpc_url, request, context).await;
    }
    Ok(context)
}

async fn plan_bonk_trade_with_cache_mode(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    cache_mode: ImportContextCacheMode,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    let Some(context) = load_bonk_import_context(rpc_url, request, cache_mode).await? else {
        return Ok(None);
    };
    Ok(Some(map_bonk_context_to_selector(request, context)?))
}

pub async fn plan_bonk_trade(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    plan_bonk_trade_with_cache_mode(rpc_url, request, ImportContextCacheMode::AllowCached).await
}

pub(crate) async fn plan_bonk_trade_uncached(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
) -> Result<Option<LifecycleAndCanonicalMarket>, String> {
    plan_bonk_trade_with_cache_mode(rpc_url, request, ImportContextCacheMode::BypassCached).await
}

pub async fn compile_bonk_trade(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
) -> Result<CompiledAdapterTrade, String> {
    let rpc_url = crate::rpc_client::configured_rpc_url();
    let owner = load_solana_wallet_by_env_key(wallet_key)?;
    let owner_bytes = owner.to_bytes();
    let launchdeck_execution = to_launchdeck_execution(&request.policy);
    let tip_account = pick_tip_account_for_provider(&request.policy.provider);
    let quote_asset = selector_quote_asset(selector)?;
    let launch_creator = selector_launch_creator(selector).unwrap_or_default();
    let launch_creator_override = if launch_creator.trim().is_empty() {
        None
    } else {
        Some(launch_creator.as_str())
    };
    let compiled = match request.side {
        TradeSide::Buy => {
            let buy_amount_sol = request
                .buy_amount_sol
                .as_deref()
                .ok_or_else(|| "Missing buyAmountSol for Bonk buy request.".to_string())?;
            let result = match selector.family {
                TradeVenueFamily::BonkLaunchpad => {
                    let pool_context = launchdeck_bonk::load_live_follow_buy_pool_context(
                        &rpc_url,
                        &request.mint,
                        quote_asset,
                        &request.policy.commitment,
                    )
                    .await?;
                    let usd1_route = if matches!(selector.quote_asset, PlannerQuoteAsset::Usd1) {
                        Some(
                            launchdeck_bonk::load_live_follow_buy_usd1_route_setup(&rpc_url)
                                .await?,
                        )
                    } else {
                        None
                    };
                    launchdeck_bonk::compile_follow_buy_transaction_with_metadata(
                        &rpc_url,
                        quote_asset,
                        &launchdeck_execution,
                        &tip_account,
                        &owner_bytes,
                        &request.mint,
                        buy_amount_sol,
                        true,
                        Some(&pool_context),
                        None,
                        usd1_route.as_ref(),
                    )
                    .await?
                }
                TradeVenueFamily::BonkRaydium => {
                    launchdeck_bonk::compile_follow_buy_transaction_with_metadata(
                        &rpc_url,
                        quote_asset,
                        &launchdeck_execution,
                        &tip_account,
                        &owner_bytes,
                        &request.mint,
                        buy_amount_sol,
                        true,
                        None,
                        bonk_buy_pool_override(request, selector),
                        None,
                    )
                    .await?
                }
                _ => {
                    return Err(format!(
                        "Bonk compiler received unsupported family {}.",
                        selector.family.label()
                    ));
                }
            };
            let entry_preference_asset =
                result
                    .entry_preference_asset
                    .as_deref()
                    .map(|asset| match asset {
                        "usd1" => TradeSettlementAsset::Usd1,
                        _ => TradeSettlementAsset::Sol,
                    });
            return Ok(CompiledAdapterTrade {
                transactions: result
                    .transactions
                    .into_iter()
                    .map(map_compiled_transaction)
                    .collect(),
                primary_tx_index: result.primary_tx_index,
                dependency_mode: if result.requires_ordered_execution {
                    TransactionDependencyMode::Dependent
                } else {
                    TransactionDependencyMode::Independent
                },
                entry_preference_asset,
            });
        }
        TradeSide::Sell => {
            let (sell_percent, token_amount_override) = resolve_bonk_sell_amount(
                &rpc_url,
                selector,
                request,
                wallet_key,
                &owner.pubkey().to_string(),
                quote_asset,
                request
                    .sell_intent
                    .as_ref()
                    .ok_or_else(|| "Missing sell intent for Bonk sell request.".to_string())?,
            )
            .await?;
            let settlement_asset = match request.policy.sell_settlement_asset {
                TradeSettlementAsset::Usd1 => "usd1",
                TradeSettlementAsset::Sol => "sol",
            };
            launchdeck_bonk::compile_follow_sell_transaction_with_token_amount_and_settlement(
                &rpc_url,
                quote_asset,
                &launchdeck_execution,
                &tip_account,
                &owner_bytes,
                &request.mint,
                sell_percent,
                token_amount_override,
                bonk_sell_pool_override(request, selector),
                bonk_sell_market_subtype_override(selector),
                launch_creator_override,
                settlement_asset,
            )
            .await?
            .ok_or_else(|| "Bonk sell compiler returned no transaction.".to_string())?
        }
    };
    Ok(CompiledAdapterTrade {
        transactions: vec![map_compiled_transaction(compiled)],
        primary_tx_index: 0,
        dependency_mode: TransactionDependencyMode::Independent,
        entry_preference_asset: None,
    })
}

fn map_bonk_context_to_selector(
    request: &TradeRuntimeRequest,
    context: launchdeck_bonk::BonkImportContext,
) -> Result<LifecycleAndCanonicalMarket, String> {
    let quote_asset = map_quote_asset(&context.quoteAsset)?;
    let is_raydium = is_raydium_detection_source(&context.detectionSource);
    let family = if is_raydium {
        TradeVenueFamily::BonkRaydium
    } else {
        TradeVenueFamily::BonkLaunchpad
    };
    let lifecycle = if is_raydium {
        TradeLifecycle::PostMigration
    } else {
        TradeLifecycle::PreMigration
    };
    let wrapper_action = match (family.clone(), quote_asset.clone(), request.side.clone()) {
        (TradeVenueFamily::BonkLaunchpad, PlannerQuoteAsset::Sol, TradeSide::Buy) => {
            WrapperAction::BonkLaunchpadSolBuy
        }
        (TradeVenueFamily::BonkLaunchpad, PlannerQuoteAsset::Sol, TradeSide::Sell) => {
            WrapperAction::BonkLaunchpadSolSell
        }
        (TradeVenueFamily::BonkLaunchpad, PlannerQuoteAsset::Usd1, TradeSide::Buy) => {
            WrapperAction::BonkLaunchpadUsd1Buy
        }
        (TradeVenueFamily::BonkLaunchpad, PlannerQuoteAsset::Usd1, TradeSide::Sell) => {
            WrapperAction::BonkLaunchpadUsd1Sell
        }
        (TradeVenueFamily::BonkRaydium, PlannerQuoteAsset::Sol, TradeSide::Buy) => {
            WrapperAction::BonkRaydiumSolBuy
        }
        (TradeVenueFamily::BonkRaydium, PlannerQuoteAsset::Sol, TradeSide::Sell) => {
            WrapperAction::BonkRaydiumSolSell
        }
        (TradeVenueFamily::BonkRaydium, PlannerQuoteAsset::Usd1, TradeSide::Buy) => {
            WrapperAction::BonkRaydiumUsd1Buy
        }
        (TradeVenueFamily::BonkRaydium, PlannerQuoteAsset::Usd1, TradeSide::Sell) => {
            WrapperAction::BonkRaydiumUsd1Sell
        }
        _ => {
            return Err(format!(
                "Bonk planner encountered unsupported quote asset {}.",
                context.quoteAsset
            ));
        }
    };
    let mut wrapper_accounts = vec![context.poolId.clone()];
    if !context.configId.trim().is_empty() {
        wrapper_accounts.push(context.configId.clone());
    }
    if !context.creator.trim().is_empty() {
        wrapper_accounts.push(context.creator.clone());
    }
    Ok(LifecycleAndCanonicalMarket {
        lifecycle,
        family,
        canonical_market_key: context.poolId.clone(),
        quote_asset,
        verification_source: if is_raydium {
            PlannerVerificationSource::HybridDerived
        } else {
            PlannerVerificationSource::OnchainDerived
        },
        wrapper_action,
        wrapper_accounts,
        market_subtype: Some(if is_raydium {
            "canonical-raydium".to_string()
        } else {
            context.mode
        }),
        direct_protocol_target: Some(if is_raydium {
            "raydium".to_string()
        } else {
            "bonk-launchpad".to_string()
        }),
        input_amount_hint: request.buy_amount_sol.clone(),
        minimum_output_hint: match &request.sell_intent {
            Some(RuntimeSellIntent::SolOutput(value)) => Some(value.clone()),
            _ => None,
        },
        runtime_bundle: None,
    })
}

fn map_quote_asset(value: &str) -> Result<PlannerQuoteAsset, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "sol" => Ok(PlannerQuoteAsset::Sol),
        "wsol" => Ok(PlannerQuoteAsset::Wsol),
        "usd1" => Ok(PlannerQuoteAsset::Usd1),
        other => Err(format!("Unsupported Bonk quote asset {other}.")),
    }
}

pub(crate) fn selector_to_bonk_import_context(
    selector: &LifecycleAndCanonicalMarket,
) -> Option<launchdeck_bonk::BonkImportContext> {
    let quote_asset = match selector.quote_asset {
        PlannerQuoteAsset::Sol => "sol",
        PlannerQuoteAsset::Usd1 => "usd1",
        PlannerQuoteAsset::Wsol | PlannerQuoteAsset::Usdc | PlannerQuoteAsset::Usdt => return None,
    };
    let family = match selector.family {
        TradeVenueFamily::BonkLaunchpad => "launchpad",
        TradeVenueFamily::BonkRaydium => "raydium",
        _ => return None,
    };
    let config_id = selector
        .wrapper_accounts
        .get(1)
        .cloned()
        .unwrap_or_default();
    let creator = selector_launch_creator(selector).unwrap_or_default();
    Some(launchdeck_bonk::BonkImportContext {
        launchpad: "bonk".to_string(),
        mode: selector.market_subtype.clone().unwrap_or_default(),
        quoteAsset: quote_asset.to_string(),
        creator,
        platformId: String::new(),
        configId: config_id,
        poolId: selector.canonical_market_key.clone(),
        detectionSource: format!("{family}-cached-selector"),
    })
}

fn selector_quote_asset(selector: &LifecycleAndCanonicalMarket) -> Result<&'static str, String> {
    match selector.quote_asset {
        PlannerQuoteAsset::Sol => Ok("sol"),
        PlannerQuoteAsset::Usd1 => Ok("usd1"),
        PlannerQuoteAsset::Wsol => {
            Err("Bonk compiler does not support WSOL quote planning.".to_string())
        }
        PlannerQuoteAsset::Usdc | PlannerQuoteAsset::Usdt => {
            Err("Bonk compiler does not support stable quote planning.".to_string())
        }
    }
}

fn selector_launch_creator(selector: &LifecycleAndCanonicalMarket) -> Option<String> {
    match selector.wrapper_accounts.as_slice() {
        [_, creator] => Some(creator.clone()),
        [_, _, creator, ..] => Some(creator.clone()),
        _ => None,
    }
}

fn bonk_buy_pool_override<'a>(
    request: &'a TradeRuntimeRequest,
    selector: &'a LifecycleAndCanonicalMarket,
) -> Option<&'a str> {
    match selector.family {
        TradeVenueFamily::BonkRaydium => request
            .pinned_pool
            .as_deref()
            .or(Some(selector.canonical_market_key.as_str())),
        _ => None,
    }
}

fn bonk_sell_pool_override<'a>(
    request: &'a TradeRuntimeRequest,
    selector: &'a LifecycleAndCanonicalMarket,
) -> Option<&'a str> {
    match selector.family {
        TradeVenueFamily::BonkLaunchpad | TradeVenueFamily::BonkRaydium => request
            .pinned_pool
            .as_deref()
            .or(Some(selector.canonical_market_key.as_str())),
        _ => None,
    }
}

fn bonk_sell_market_subtype_override(selector: &LifecycleAndCanonicalMarket) -> Option<&str> {
    match selector.family {
        TradeVenueFamily::BonkLaunchpad => selector.market_subtype.as_deref(),
        TradeVenueFamily::BonkRaydium => None,
        _ => None,
    }
}

fn parse_sell_percent(intent: &RuntimeSellIntent) -> Result<u8, String> {
    match intent {
        RuntimeSellIntent::Percent(value) => {
            let parsed = value
                .trim()
                .parse::<u8>()
                .map_err(|_| format!("Bonk sell percent must be an integer between 1 and 100: {value}"))?;
            if parsed == 0 || parsed > 100 {
                return Err(format!(
                    "Bonk sell percent must be between 1 and 100: {value}"
                ));
            }
            Ok(parsed)
        }
        RuntimeSellIntent::SolOutput(_) => Err(
            "Bonk native sell currently supports percent-based exits only; sellOutputSol remains guarded."
                .to_string(),
        ),
    }
}

async fn resolve_bonk_sell_amount(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wallet_key: &str,
    owner: &str,
    quote_asset: &str,
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
            let amount = choose_bonk_target_sized_amount(
                rpc_url,
                selector,
                request,
                quote_asset,
                balance.amount_raw,
                target_lamports,
            )
            .await?;
            Ok((100, Some(amount)))
        }
    }
}

async fn choose_bonk_target_sized_amount(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    quote_asset: &str,
    available_raw: u64,
    target_lamports: u64,
) -> Result<u64, String> {
    if available_raw == 0 {
        return Err("You have 0 tokens.".to_string());
    }
    let full_quote =
        quote_bonk_token_amount_to_sol(rpc_url, selector, request, quote_asset, available_raw)
            .await?;
    if full_quote == 0 {
        return Err("Bonk sell quote resolved to zero SOL.".to_string());
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
            quote_bonk_token_amount_to_sol(rpc_url, selector, request, quote_asset, estimate)
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
            quote_bonk_token_amount_to_sol(rpc_url, selector, request, quote_asset, amount).await?;
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
        .ok_or_else(|| "Bonk sell quote resolved to zero SOL.".to_string())
}

async fn quote_bonk_token_amount_to_sol(
    rpc_url: &str,
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    quote_asset: &str,
    token_amount_raw: u64,
) -> Result<u64, String> {
    let (quote_raw, raw_quote_asset) = launchdeck_bonk::quote_bonk_holding_value_quote_raw(
        rpc_url,
        &request.mint,
        bonk_sell_pool_override(request, selector),
        quote_asset,
        token_amount_raw,
        &request.policy.commitment,
    )
    .await?;
    let gross_sol_lamports = if raw_quote_asset.eq_ignore_ascii_case("usd1") {
        launchdeck_bonk::quote_sol_lamports_for_exact_usd1_input_with_max_setup_age(
            rpc_url,
            quote_raw,
            Duration::from_millis(1_500),
        )
        .await
    } else {
        Ok(quote_raw)
    }?;
    crate::sell_target_sizing::net_sol_after_wrapper_fee(gross_sol_lamports)
}

fn read_mint_decimals(data: &[u8]) -> Result<u8, String> {
    data.get(44)
        .copied()
        .ok_or_else(|| "Mint account data was shorter than expected (decimals).".to_string())
}

fn is_raydium_detection_source(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.starts_with("raydium-") && normalized != "raydium-launchpad"
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_launchpad_context_to_pre_migration_selector() {
        let selector = map_bonk_context_to_selector(
            &TradeRuntimeRequest {
                side: TradeSide::Buy,
                mint: "mint".to_string(),
                buy_amount_sol: Some("0.5".to_string()),
                sell_intent: None,
                policy: crate::trade_runtime::RuntimeExecutionPolicy {
                    slippage_percent: "10".to_string(),
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
            launchdeck_bonk::BonkImportContext {
                launchpad: "bonk".to_string(),
                mode: "regular".to_string(),
                quoteAsset: "usd1".to_string(),
                creator: "creator".to_string(),
                platformId: String::new(),
                configId: "config".to_string(),
                poolId: "pool".to_string(),
                detectionSource: "raydium-launchpad".to_string(),
            },
        )
        .expect("selector");
        assert_eq!(selector.family, TradeVenueFamily::BonkLaunchpad);
        assert_eq!(selector.lifecycle, TradeLifecycle::PreMigration);
        assert_eq!(selector.wrapper_action, WrapperAction::BonkLaunchpadUsd1Buy);
    }

    #[test]
    fn migrated_sell_reuses_resolved_pool_key() {
        let request = TradeRuntimeRequest {
            side: TradeSide::Sell,
            mint: "mint".to_string(),
            buy_amount_sol: None,
            sell_intent: Some(RuntimeSellIntent::Percent("100".to_string())),
            policy: crate::trade_runtime::RuntimeExecutionPolicy {
                slippage_percent: "10".to_string(),
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
        };
        let selector = LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PostMigration,
            family: TradeVenueFamily::BonkRaydium,
            canonical_market_key: "raydium-pool".to_string(),
            quote_asset: PlannerQuoteAsset::Usd1,
            verification_source: PlannerVerificationSource::HybridDerived,
            wrapper_action: WrapperAction::BonkRaydiumUsd1Sell,
            wrapper_accounts: vec!["raydium-pool".to_string()],
            market_subtype: Some("canonical-raydium".to_string()),
            direct_protocol_target: Some("raydium".to_string()),
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        };
        assert_eq!(
            bonk_sell_pool_override(&request, &selector),
            Some("raydium-pool")
        );
        assert!(bonk_sell_market_subtype_override(&selector).is_none());
    }
}
