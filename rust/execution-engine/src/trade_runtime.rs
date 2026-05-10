use crate::{
    extension_api::{
        BatchLifecycleStatus, BuyFundingPolicy, MevMode, SellSettlementPolicy,
        TradeSettlementAsset, TradeSide,
    },
    mint_warm_cache::{WarmFingerprint, build_fingerprint, shared_mint_warm_cache},
    rollout::{allow_non_canonical_pool_trades, wrapper_fee_vault_pubkey},
    route_index::{RouteIndexKey, shared_route_index},
    rpc_client::{
        CompiledTransaction, SentResult, configured_rpc_url,
        confirm_submitted_transactions_for_transport, rpc_request_with_client,
        shared_rpc_http_client, simulate_transactions,
        submit_independent_transactions_for_transport,
    },
    trade_dispatch::{
        CompiledAdapterTrade, TradeDispatchPlan, TransactionDependencyMode, adapter_for_selector,
        compile_trade_for_adapter, resolve_trade_plan, resolve_trade_plan_fresh,
    },
    trade_planner::{LifecycleAndCanonicalMarket, TradeVenueFamily},
    transport::{ExecutionTransportConfig, TransportPlan, build_transport_plan},
    wallet_store::load_solana_wallet_by_env_key,
    warming_service::shared_warming_service,
    wrapper_adapter::allowed_inner_program_pubkeys,
    wrapper_compile::{
        WrapCompiledTransactionError, WrapCompiledTransactionRequest, estimate_sol_in_fee_lamports,
        validate_already_wrapped_transaction, wrap_compiled_transaction,
    },
    wrapper_payload::{
        WrapperInstructionPayload, WrapperRouteClassification, build_wrapper_instruction_payload,
        classify_trade_route, format_lamports_as_sol, parse_sol_amount_to_lamports,
        trade_touches_sol,
    },
};
use serde_json::{Value, json};
use solana_sdk::signature::{Keypair, Signer};
use std::{collections::BTreeSet, time::Instant};

const SHARED_SUPER_LOOKUP_TABLE: &str = "7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc";
const DEFAULT_TRADE_EXECUTION_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(15);
const HELLOMOON_TRADE_EXECUTION_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(10);
#[derive(Debug, Clone)]
pub enum RuntimeSellIntent {
    Percent(String),
    SolOutput(String),
}

#[derive(Debug, Clone)]
pub struct RuntimeExecutionPolicy {
    pub slippage_percent: String,
    pub mev_mode: MevMode,
    pub auto_tip_enabled: bool,
    pub fee_sol: String,
    pub tip_sol: String,
    pub provider: String,
    pub endpoint_profile: String,
    pub commitment: String,
    pub skip_preflight: bool,
    pub track_send_block_height: bool,
    pub buy_funding_policy: BuyFundingPolicy,
    pub sell_settlement_policy: SellSettlementPolicy,
    pub sell_settlement_asset: TradeSettlementAsset,
}

#[derive(Debug, Clone)]
pub struct TradeRuntimeRequest {
    pub side: TradeSide,
    pub mint: String,
    pub buy_amount_sol: Option<String>,
    pub sell_intent: Option<RuntimeSellIntent>,
    pub policy: RuntimeExecutionPolicy,
    pub platform_label: Option<String>,
    pub planned_route: Option<TradeDispatchPlan>,
    pub planned_trade: Option<LifecycleAndCanonicalMarket>,
    /// Optional pool pubkey the caller has pinned for this trade. When set,
    /// the compile path targets this specific pool instead of re-deriving
    /// the canonical one. Today this is only honored for Pump AMM when the
    /// global `allow_non_canonical_pool_trades` policy is on. `None` means
    /// "let the planner discover / pick the canonical pool".
    pub pinned_pool: Option<String>,
    /// Opaque key returned by `/prewarm` identifying a specific warm-cache
    /// entry this trade is expected to consume. Used to match the click
    /// against the right prewarmed plan and to tighten metrics.
    pub warm_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CompiledTradePlan {
    pub adapter: &'static str,
    pub execution_backend: &'static str,
    pub selector: LifecycleAndCanonicalMarket,
    pub normalized_request: TradeRuntimeRequest,
    pub warm_invalidation_fingerprints: Vec<WarmFingerprint>,
    pub wrapper_route: WrapperRouteClassification,
    pub wrapper_payload: Option<WrapperInstructionPayload>,
    pub wrapper_fee_plan: RuntimeWrapperFeePlan,
    pub transport_plan: TransportPlan,
    pub transactions: Vec<CompiledTransaction>,
    pub primary_tx_index: usize,
    pub dependency_mode: TransactionDependencyMode,
    pub entry_preference_asset: Option<TradeSettlementAsset>,
    pub compile_metrics: crate::route_metrics::RouteMetricsSnapshot,
}

#[derive(Debug, Clone)]
pub struct RuntimeWrapperFeePlan {
    pub is_trade_leg: bool,
    pub fee_bps: u16,
    pub side: TradeSide,
    pub route: WrapperRouteClassification,
    pub fee_asset: &'static str,
    pub route_conversion: bool,
    pub passthrough_allowed: bool,
}

#[derive(Debug, Clone)]
pub struct WalletExecutionOutcome {
    pub wallet_key: String,
    pub status: BatchLifecycleStatus,
    pub tx_signature: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutedRuntimeTrade {
    pub tx_signature: String,
    pub entry_preference_asset: Option<TradeSettlementAsset>,
}

pub async fn compile_wallet_trade(
    request: &TradeRuntimeRequest,
    wallet_key: &str,
) -> Result<CompiledTradePlan, String> {
    compile_wallet_trade_with_route_mode(request, wallet_key, false).await
}

pub async fn execute_wallet_trade_with_pre_submit_check<F>(
    request: TradeRuntimeRequest,
    wallet_key: String,
    pre_submit_check: F,
) -> Result<ExecutedRuntimeTrade, String>
where
    F: Fn(&str, &CompiledTradePlan) -> Result<(), String> + Send + Sync,
{
    execute_wallet_trade_inner(
        request,
        wallet_key,
        Some(pre_submit_check),
        Option::<fn(&str)>::None,
    )
    .await
}

pub async fn execute_wallet_trade_with_pre_submit_check_and_submit_callback<F, C>(
    request: TradeRuntimeRequest,
    wallet_key: String,
    pre_submit_check: F,
    on_submitted: C,
) -> Result<ExecutedRuntimeTrade, String>
where
    F: Fn(&str, &CompiledTradePlan) -> Result<(), String> + Send + Sync,
    C: Fn(&str) + Send + Sync,
{
    execute_wallet_trade_inner(
        request,
        wallet_key,
        Some(pre_submit_check),
        Some(on_submitted),
    )
    .await
}

fn request_with_net_wrapper_buy_input(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wrapper_route: WrapperRouteClassification,
    wrapper_payload: Option<&WrapperInstructionPayload>,
) -> Result<TradeRuntimeRequest, String> {
    if !matches!(request.side, TradeSide::Buy)
        || !matches!(wrapper_route, WrapperRouteClassification::SolIn)
        || adapter_builds_wrapped_gross_sol_route(selector, request, wrapper_route)
    {
        return Ok(request.clone());
    }
    let Some(payload) = wrapper_payload else {
        return Ok(request.clone());
    };
    let gross = payload.route_metadata.gross_sol_in_lamports;
    if gross == 0 {
        return Ok(request.clone());
    }
    let fee = estimate_sol_in_fee_lamports(gross, payload.route_metadata.fee_bps);
    let net = gross
        .checked_sub(fee)
        .ok_or_else(|| "Wrapper buy fee exceeds gross SOL input.".to_string())?;
    if net == 0 {
        return Err("Wrapper buy net venue input resolves to zero after fee.".to_string());
    }
    let original = request
        .buy_amount_sol
        .as_deref()
        .map(parse_sol_amount_to_lamports)
        .unwrap_or(0);
    if original != gross {
        return Err("Wrapper buy gross SOL input drifted from request amount.".to_string());
    }
    let mut adjusted = request.clone();
    adjusted.buy_amount_sol = Some(format_lamports_as_sol(net));
    Ok(adjusted)
}

fn adapter_builds_wrapped_gross_sol_route(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    wrapper_route: WrapperRouteClassification,
) -> bool {
    matches!(request.side, TradeSide::Buy)
        && matches!(wrapper_route, WrapperRouteClassification::SolIn)
        && (matches!(
            selector.family,
            crate::trade_planner::TradeVenueFamily::BonkRaydium
                | crate::trade_planner::TradeVenueFamily::BonkLaunchpad
        ) && matches!(
            selector.quote_asset,
            crate::trade_planner::PlannerQuoteAsset::Usd1
        ) || matches!(
            selector.family,
            crate::trade_planner::TradeVenueFamily::MeteoraDbc
                | crate::trade_planner::TradeVenueFamily::MeteoraDammV2
        ) && matches!(
            selector.quote_asset,
            crate::trade_planner::PlannerQuoteAsset::Usdc
        ))
}

fn wrapper_inner_program_diagnostic_label(
    selector: &LifecycleAndCanonicalMarket,
    transactions: &[CompiledTransaction],
) -> String {
    let has_meteora_usdc_route = transactions
        .iter()
        .any(|transaction| transaction.label.starts_with("meteora-usdc"));
    if has_meteora_usdc_route {
        return match selector.family {
            crate::trade_planner::TradeVenueFamily::MeteoraDbc => {
                "raydium-clmm+meteora-dbc".to_string()
            }
            crate::trade_planner::TradeVenueFamily::MeteoraDammV2 => {
                "raydium-clmm+meteora-damm-v2".to_string()
            }
            _ => crate::wrapper_adapter::inner_program_label_for_selector(selector)
                .unwrap_or("<unknown>")
                .to_string(),
        };
    }
    crate::wrapper_adapter::inner_program_label_for_selector(selector)
        .unwrap_or("<unknown>")
        .to_string()
}

fn selector_allows_native_no_sol_trade(selector: &LifecycleAndCanonicalMarket) -> bool {
    matches!(
        selector.family,
        crate::trade_planner::TradeVenueFamily::TrustedStableSwap
    )
}

fn validate_compiled_adapter_trade(transactions: &CompiledAdapterTrade) -> Result<(), String> {
    if transactions.transactions.is_empty() {
        return Err("Trade compiler did not return any transactions.".to_string());
    }
    if transactions.primary_tx_index >= transactions.transactions.len() {
        return Err(format!(
            "Trade compiler returned invalid primary transaction index {} for {} transaction(s).",
            transactions.primary_tx_index,
            transactions.transactions.len()
        ));
    }
    Ok(())
}

async fn compile_wallet_trade_with_route_mode(
    request: &TradeRuntimeRequest,
    wallet_key: &str,
    force_fresh_route: bool,
) -> Result<CompiledTradePlan, String> {
    let rpc_url = configured_rpc_url();
    let wrapper_request = normalize_request_for_wrapper_trade(request);
    let route_conversion = wrapper_request.policy.buy_funding_policy
        != request.policy.buy_funding_policy
        || wrapper_request.policy.sell_settlement_policy != request.policy.sell_settlement_policy
        || wrapper_request.policy.sell_settlement_asset != request.policy.sell_settlement_asset;
    let dispatch_plan = if force_fresh_route {
        let mut reroute_request = wrapper_request.clone();
        reroute_request.planned_route = None;
        reroute_request.planned_trade = None;
        reroute_request.warm_key = None;
        resolve_trade_plan_fresh(&reroute_request).await?
    } else if let Some(planned_route) = wrapper_request.planned_route.clone() {
        reuse_or_refresh_planned_route(&rpc_url, &wrapper_request, planned_route).await?
    } else if let Some(selector) = wrapper_request.planned_trade.clone() {
        reuse_or_refresh_planned_selector(&rpc_url, &wrapper_request, selector).await?
    } else {
        resolve_trade_plan(&wrapper_request).await?
    };
    let wrapper_normalized_request =
        normalize_request_for_dispatch_plan(&wrapper_request, &dispatch_plan);
    let wrapper_route = classify_trade_route(&dispatch_plan.selector, &wrapper_normalized_request);
    if !wrapper_route.touches_sol() && selector_allows_native_no_sol_trade(&dispatch_plan.selector)
    {
        let normalized_request = normalize_request_for_dispatch_plan(request, &dispatch_plan);
        let warm_invalidation_fingerprints =
            warm_invalidation_fingerprints(request, &normalized_request, &rpc_url);
        shared_warming_service()
            .cache_selector(
                &rpc_url,
                &normalized_request.policy.commitment,
                side_label(&normalized_request.side),
                &route_policy_label(&normalized_request),
                &normalized_request.mint,
                normalized_request.pinned_pool.as_deref(),
                allow_non_canonical_pool_trades(),
                dispatch_plan.selector.clone(),
            )
            .await;
        let (transactions_result, compile_metrics) =
            crate::route_metrics::collect_route_metrics(async {
                let adapter_started_at = Instant::now();
                let result = compile_trade_for_adapter(
                    dispatch_plan.adapter,
                    &dispatch_plan.selector,
                    &normalized_request,
                    wallet_key,
                )
                .await;
                crate::route_metrics::record_phase_ms(
                    "adapter_compile",
                    adapter_started_at.elapsed().as_millis(),
                );
                result
            })
            .await;
        let transactions = transactions_result?;
        validate_compiled_adapter_trade(&transactions)?;
        let transport_plan = build_transport_plan(
            &ExecutionTransportConfig {
                provider: normalized_request.policy.provider.clone(),
                endpoint_profile: normalized_request.policy.endpoint_profile.clone(),
                commitment: normalized_request.policy.commitment.clone(),
                skip_preflight: normalized_request.policy.skip_preflight,
                track_send_block_height: normalized_request.policy.track_send_block_height,
                mev_mode: crate::launchdeck_bridge::to_launchdeck_execution(
                    &normalized_request.policy,
                )
                .mevMode,
                mev_protect: !matches!(normalized_request.policy.mev_mode, MevMode::Off),
            },
            transactions.transactions.len(),
        );
        validate_transport_for_compiled_trade(
            &transport_plan,
            transactions.dependency_mode,
            transactions.transactions.len(),
        )?;
        eprintln!(
            "[execution-engine][trade-runtime] compile wallet={} mint={} family={} market={} provider={} transport={} endpoint_profile={} pinned_pool={:?} warm_key={:?} native_no_sol=true",
            wallet_key,
            normalized_request.mint,
            dispatch_plan.selector.family.label(),
            dispatch_plan.selector.canonical_market_key,
            normalized_request.policy.provider,
            transport_plan.transport_type,
            transport_plan.resolved_endpoint_profile,
            normalized_request.pinned_pool,
            normalized_request.warm_key,
        );
        validate_runtime_shared_alt_boundary(
            &dispatch_plan.selector,
            dispatch_plan.adapter.label(),
            &transactions.transactions,
        )?;
        return Ok(CompiledTradePlan {
            adapter: dispatch_plan.adapter.label(),
            execution_backend: "native",
            selector: dispatch_plan.selector,
            normalized_request,
            warm_invalidation_fingerprints,
            wrapper_route,
            wrapper_payload: None,
            wrapper_fee_plan: RuntimeWrapperFeePlan {
                is_trade_leg: false,
                fee_bps: 0,
                side: request.side.clone(),
                route: wrapper_route,
                fee_asset: "none",
                route_conversion: false,
                passthrough_allowed: true,
            },
            transport_plan,
            transactions: transactions.transactions,
            primary_tx_index: transactions.primary_tx_index,
            dependency_mode: transactions.dependency_mode,
            entry_preference_asset: transactions.entry_preference_asset,
            compile_metrics,
        });
    }
    let normalized_request = wrapper_normalized_request;
    let warm_invalidation_fingerprints =
        warm_invalidation_fingerprints(&wrapper_request, &normalized_request, &rpc_url);
    shared_warming_service()
        .cache_selector(
            &rpc_url,
            &normalized_request.policy.commitment,
            side_label(&normalized_request.side),
            &route_policy_label(&normalized_request),
            &normalized_request.mint,
            normalized_request.pinned_pool.as_deref(),
            allow_non_canonical_pool_trades(),
            dispatch_plan.selector.clone(),
        )
        .await;
    if !wrapper_route.touches_sol() {
        return Err(format!(
            "Trade route cannot be routed through the Trench wrapper: side={} family={} quote={:?} policy={}.",
            side_label(&normalized_request.side),
            dispatch_plan.selector.family.label(),
            dispatch_plan.selector.quote_asset,
            route_policy_label(&normalized_request)
        ));
    }
    let wrapper_payer = load_solana_wallet_by_env_key(wallet_key)?;
    let wallet_pubkey = wrapper_payer.pubkey().to_string();
    let wrapper_payload = Some(build_wrapper_instruction_payload(
        &dispatch_plan.selector,
        &normalized_request,
        wallet_pubkey,
    ));
    let adapter_request = request_with_net_wrapper_buy_input(
        &dispatch_plan.selector,
        &normalized_request,
        wrapper_route,
        wrapper_payload.as_ref(),
    )?;
    let (compile_result, compile_metrics) = crate::route_metrics::collect_route_metrics(async {
        let adapter_started_at = Instant::now();
        let transactions = compile_trade_for_adapter(
            dispatch_plan.adapter,
            &dispatch_plan.selector,
            &adapter_request,
            wallet_key,
        )
        .await?;
        crate::route_metrics::record_phase_ms(
            "adapter_compile",
            adapter_started_at.elapsed().as_millis(),
        );
        validate_compiled_adapter_trade(&transactions)?;
        let transport_plan_started_at = Instant::now();
        let transport_plan = build_transport_plan(
            &ExecutionTransportConfig {
                provider: normalized_request.policy.provider.clone(),
                endpoint_profile: normalized_request.policy.endpoint_profile.clone(),
                commitment: normalized_request.policy.commitment.clone(),
                skip_preflight: normalized_request.policy.skip_preflight,
                track_send_block_height: normalized_request.policy.track_send_block_height,
                mev_mode: crate::launchdeck_bridge::to_launchdeck_execution(
                    &normalized_request.policy,
                )
                .mevMode,
                mev_protect: !matches!(normalized_request.policy.mev_mode, MevMode::Off),
            },
            transactions.transactions.len(),
        );
        crate::route_metrics::record_phase_ms(
            "transport_plan",
            transport_plan_started_at.elapsed().as_millis(),
        );
        validate_transport_for_compiled_trade(
            &transport_plan,
            transactions.dependency_mode,
            transactions.transactions.len(),
        )?;
        validate_runtime_shared_alt_boundary(
            &dispatch_plan.selector,
            dispatch_plan.adapter.label(),
            &transactions.transactions,
        )?;
        let wrapper_inner_program_label = wrapper_inner_program_diagnostic_label(
            &dispatch_plan.selector,
            &transactions.transactions,
        );
        let wrapper_fee_plan = RuntimeWrapperFeePlan {
            is_trade_leg: true,
            fee_bps: wrapper_payload
                .as_ref()
                .map(|payload| payload.route_metadata.fee_bps)
                .unwrap_or_else(crate::rollout::wrapper_default_fee_bps),
            side: normalized_request.side.clone(),
            route: wrapper_route,
            fee_asset: "sol/wsol",
            route_conversion,
            passthrough_allowed: false,
        };
        let wrapper_started_at = Instant::now();
        let final_transactions = wrap_native_transactions(
            &dispatch_plan.selector,
            &normalized_request,
            &transactions.transactions,
            transactions.primary_tx_index,
            &wrapper_payer,
            &wrapper_payload,
            wrapper_route,
        )
        .await?;
        crate::route_metrics::record_phase_ms(
            "wrapper_compile",
            wrapper_started_at.elapsed().as_millis(),
        );
        Ok::<
            (
                CompiledAdapterTrade,
                TransportPlan,
                String,
                RuntimeWrapperFeePlan,
                Vec<CompiledTransaction>,
            ),
            String,
        >((
            transactions,
            transport_plan,
            wrapper_inner_program_label,
            wrapper_fee_plan,
            final_transactions,
        ))
    })
    .await;
    let (
        transactions,
        transport_plan,
        wrapper_inner_program_label,
        wrapper_fee_plan,
        final_transactions,
    ) = compile_result?;
    eprintln!(
        "[execution-engine][trade-runtime] compile wallet={} mint={} family={} market={} provider={} transport={} endpoint_profile={} pinned_pool={:?} warm_key={:?}",
        wallet_key,
        normalized_request.mint,
        dispatch_plan.selector.family.label(),
        dispatch_plan.selector.canonical_market_key,
        normalized_request.policy.provider,
        transport_plan.transport_type,
        transport_plan.resolved_endpoint_profile,
        normalized_request.pinned_pool,
        normalized_request.warm_key,
    );
    eprintln!(
        "[execution-engine][trade-runtime] wrapper-classify wallet={} mint={} family={} route={} fee_bps={} route_conversion={} touches_sol={} inner_program={}",
        wallet_key,
        normalized_request.mint,
        dispatch_plan.selector.family.label(),
        wrapper_route.label(),
        wrapper_fee_plan.fee_bps,
        wrapper_fee_plan.route_conversion,
        wrapper_route.touches_sol(),
        wrapper_inner_program_label
    );

    let execution_backend = "wrapper";

    Ok(CompiledTradePlan {
        adapter: dispatch_plan.adapter.label(),
        execution_backend,
        selector: dispatch_plan.selector,
        normalized_request,
        warm_invalidation_fingerprints,
        wrapper_route,
        wrapper_payload,
        wrapper_fee_plan,
        transport_plan,
        transactions: final_transactions,
        primary_tx_index: transactions.primary_tx_index,
        dependency_mode: transactions.dependency_mode,
        entry_preference_asset: transactions.entry_preference_asset,
        compile_metrics,
    })
}

/// Convert native-compiled transactions into wrapper-wrapped ones.
async fn wrap_native_transactions(
    selector: &LifecycleAndCanonicalMarket,
    normalized_request: &TradeRuntimeRequest,
    transactions: &[CompiledTransaction],
    primary_tx_index: usize,
    payer: &Keypair,
    wrapper_payload: &Option<WrapperInstructionPayload>,
    wrapper_route: WrapperRouteClassification,
) -> Result<Vec<CompiledTransaction>, String> {
    let rpc_url = configured_rpc_url();
    let fee_vault = wrapper_fee_vault_pubkey();
    let lookup_tables = crate::pump_native::load_shared_super_lookup_tables(&rpc_url).await?;
    let allowed_programs = allowed_inner_program_pubkeys();

    // Missing payload falls back to a zeroed SolOut estimate.
    let (route_kind, fee_bps, gross_sol_in, min_net_output) = match wrapper_payload {
        Some(payload) => {
            let metadata = &payload.route_metadata;
            (
                payload.route_classification.to_abi().ok_or_else(|| {
                    "Internal error: wrapper payload cannot wrap NoSol route".to_string()
                })?,
                metadata.fee_bps,
                metadata.gross_sol_in_lamports,
                metadata.min_net_output,
            )
        }
        None => {
            let kind = match wrapper_route {
                WrapperRouteClassification::SolIn => crate::wrapper_abi::WrapperRouteKind::SolIn,
                WrapperRouteClassification::SolOut => crate::wrapper_abi::WrapperRouteKind::SolOut,
                WrapperRouteClassification::NoSol => {
                    return Err(
                        "Internal error: wrap_native_transactions called on NoSol route"
                            .to_string(),
                    );
                }
            };
            (kind, crate::rollout::wrapper_default_fee_bps(), 0, 0)
        }
    };

    let mut wrapped = Vec::with_capacity(transactions.len());
    for (index, source) in transactions.iter().enumerate() {
        let request = WrapCompiledTransactionRequest {
            label: source.label.clone(),
            route_kind,
            fee_bps,
            fee_vault,
            gross_sol_in_lamports: gross_sol_in,
            min_net_output,
            select_first_allowlisted_venue_instruction: matches!(
                selector.family,
                crate::trade_planner::TradeVenueFamily::BonkRaydium
                    | crate::trade_planner::TradeVenueFamily::BonkLaunchpad
            ) && source.label == "follow-buy-atomic",
            select_last_allowlisted_venue_instruction: matches!(
                selector.family,
                crate::trade_planner::TradeVenueFamily::BonkRaydium
                    | crate::trade_planner::TradeVenueFamily::BonkLaunchpad
            ) && source.label == "follow-sell"
                && matches!(wrapper_route, WrapperRouteClassification::SolOut),
        };
        match wrap_compiled_transaction(source, &payer, &lookup_tables, &allowed_programs, &request)
        {
            Ok(tx) => {
                if index != primary_tx_index {
                    return Err(format!(
                        "Wrapper wrap refused non-primary venue transaction {} (mint={}, family={}): multiple trade legs require per-leg wrapper payloads.",
                        source.label,
                        normalized_request.mint,
                        selector.family.label()
                    ));
                }
                wrapped.push(tx);
            }
            Err(WrapCompiledTransactionError::NoVenueInstruction) => {
                if index == primary_tx_index {
                    return Err(format!(
                        "Wrapper wrap failed for primary trade transaction {} (mint={}, family={}): {}",
                        source.label,
                        normalized_request.mint,
                        selector.family.label(),
                        WrapCompiledTransactionError::NoVenueInstruction
                    ));
                }
                // Setup-only/non-venue transactions stay unchanged.
                eprintln!(
                    "[execution-engine][wrapper-wrap] setup-passthrough label={} reason=no_venue_instruction family={}",
                    source.label,
                    selector.family.label()
                );
                wrapped.push(source.clone());
            }
            Err(WrapCompiledTransactionError::AlreadyWrapped)
                if allow_already_wrapped_passthrough(selector, &source.label, wrapper_route) =>
            {
                validate_already_wrapped_transaction(
                    source,
                    &payer.pubkey(),
                    &lookup_tables,
                    &request,
                )
                .map_err(|error| {
                    format!(
                        "Wrapper wrap refused already-wrapped transaction {} (mint={}, family={}): {}",
                        source.label,
                        normalized_request.mint,
                        selector.family.label(),
                        error
                    )
                })?;
                eprintln!(
                    "[execution-engine][wrapper-wrap] passthrough label={} reason=already_wrapped_dynamic_route family={}",
                    source.label,
                    selector.family.label()
                );
                wrapped.push(source.clone());
            }
            Err(WrapCompiledTransactionError::AlreadyWrapped) => {
                return Err(format!(
                    "Wrapper wrap refused already-wrapped transaction {} (mint={}, family={}): {}",
                    source.label,
                    normalized_request.mint,
                    selector.family.label(),
                    WrapCompiledTransactionError::AlreadyWrapped
                ));
            }
            Err(error) => {
                return Err(format!(
                    "Wrapper wrap failed for {} (mint={}, family={}): {}",
                    source.label,
                    normalized_request.mint,
                    selector.family.label(),
                    error
                ));
            }
        }
    }
    Ok(wrapped)
}

fn allow_already_wrapped_passthrough(
    selector: &LifecycleAndCanonicalMarket,
    source_label: &str,
    wrapper_route: WrapperRouteClassification,
) -> bool {
    let bonk_dynamic = matches!(
        selector.family,
        crate::trade_planner::TradeVenueFamily::BonkRaydium
            | crate::trade_planner::TradeVenueFamily::BonkLaunchpad
    ) && matches!(
        (source_label, wrapper_route),
        ("follow-buy-atomic", WrapperRouteClassification::SolIn)
            | ("follow-sell", WrapperRouteClassification::SolOut)
    );
    let meteora_usdc_dynamic = matches!(
        selector.family,
        crate::trade_planner::TradeVenueFamily::MeteoraDbc
            | crate::trade_planner::TradeVenueFamily::MeteoraDammV2
    ) && matches!(
        (source_label, wrapper_route),
        ("meteora-usdc-buy", WrapperRouteClassification::SolIn)
            | ("meteora-usdc-damm-buy", WrapperRouteClassification::SolIn)
            | ("meteora-usdc-sell", WrapperRouteClassification::SolOut)
            | ("meteora-usdc-damm-sell", WrapperRouteClassification::SolOut)
    );
    bonk_dynamic || meteora_usdc_dynamic
}

pub fn trade_request_touches_sol(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
) -> bool {
    trade_touches_sol(selector, request)
}

pub async fn plan_trade_request(
    request: &TradeRuntimeRequest,
) -> Result<LifecycleAndCanonicalMarket, String> {
    Ok(resolve_trade_plan(request).await?.selector)
}

pub async fn execute_compiled_trade(plan: CompiledTradePlan) -> Result<String, String> {
    execute_compiled_trade_inner(plan, Option::<&fn(&str)>::None).await
}

async fn execute_compiled_trade_inner<C>(
    plan: CompiledTradePlan,
    on_submitted: Option<&C>,
) -> Result<String, String>
where
    C: Fn(&str) + Send + Sync,
{
    let rpc_url = configured_rpc_url();
    validate_runtime_shared_alt_boundary(&plan.selector, plan.adapter, &plan.transactions)?;
    validate_transport_for_compiled_trade(
        &plan.transport_plan,
        plan.dependency_mode,
        plan.transactions.len(),
    )?;
    eprintln!(
        "[execution-engine][trade-runtime] execute adapter={} provider={} transport={} endpoints={} tx_count={}",
        plan.adapter,
        plan.transport_plan.resolved_provider,
        plan.transport_plan.transport_type,
        transport_endpoint_summary(&plan.transport_plan),
        plan.transactions.len()
    );

    let (mut submitted, submit_warnings, submit_elapsed_ms) =
        submit_independent_transactions_for_transport(
            &rpc_url,
            &plan.transport_plan,
            &plan.transactions,
        )
        .await?;
    if let Some(signature) = submitted
        .get(plan.primary_tx_index)
        .and_then(|result| result.signature.as_deref())
        .filter(|signature| !signature.trim().is_empty())
    {
        if let Some(callback) = on_submitted {
            callback(signature);
        }
        eprintln!(
            "[execution-engine][latency] phase=transport-submitted adapter={} transport={} submit_ms={} signature={}",
            plan.adapter, plan.transport_plan.transport_type, submit_elapsed_ms, signature
        );
    }
    let (confirm_warnings, confirm_elapsed_ms) = confirm_submitted_transactions_for_transport(
        &rpc_url,
        &plan.transport_plan,
        &mut submitted,
    )
    .await?;

    if !submit_warnings.is_empty() || !confirm_warnings.is_empty() {
        let combined = submit_warnings
            .into_iter()
            .chain(confirm_warnings.into_iter())
            .collect::<Vec<_>>()
            .join(" | ");
        if !combined.trim().is_empty() {
            eprintln!("execution-engine {} warnings: {}", plan.adapter, combined);
        }
    }

    let result = validate_submitted_trade_results(
        &submitted,
        plan.primary_tx_index,
        plan.dependency_mode,
        &plan.transport_plan.commitment,
    );
    match result {
        Ok(result) => {
            let signature = result
                .signature
                .clone()
                .ok_or_else(|| "Transport did not return a confirmed signature.".to_string())?;
            eprintln!(
                "[execution-engine][latency] phase=transport-confirm adapter={} transport={} submit_ms={} confirm_ms={} signature={}",
                plan.adapter,
                plan.transport_plan.transport_type,
                submit_elapsed_ms,
                confirm_elapsed_ms,
                signature
            );
            Ok(signature)
        }
        Err(error) => {
            if let Some(reconciled) =
                reconcile_primary_submission_status(&rpc_url, &plan, &submitted).await
            {
                return reconciled;
            }
            Err(append_primary_failure_diagnostics(&rpc_url, &plan, error).await)
        }
    }
}

fn transport_endpoint_summary(plan: &TransportPlan) -> String {
    let endpoints = match plan.transport_type.as_str() {
        "hellomoon-quic" => &plan.hello_moon_quic_endpoints,
        "hellomoon-bundle" => &plan.hello_moon_bundle_endpoints,
        "helius-sender" => &plan.helius_sender_endpoints,
        "standard-rpc" => &plan.standard_rpc_submit_endpoints,
        _ => return "-".to_string(),
    };
    if endpoints.is_empty() {
        "-".to_string()
    } else {
        endpoints.join(",")
    }
}

fn reconcile_submitted_signature_status(
    signature: &str,
    status: &Value,
) -> Option<Result<String, String>> {
    if status.is_null() {
        return None;
    }
    if let Some(err) = status.get("err").filter(|value| !value.is_null()) {
        return Some(Err(format!(
            "Transaction {signature} failed on-chain: {err}"
        )));
    }
    let has_ok_status = status.pointer("/status/Ok").is_some();
    let confirmation_status = status.get("confirmationStatus").and_then(Value::as_str);
    if has_ok_status || matches!(confirmation_status, Some("confirmed" | "finalized")) {
        return Some(Ok(signature.to_string()));
    }
    None
}

async fn reconcile_primary_submission_status(
    rpc_url: &str,
    plan: &CompiledTradePlan,
    submitted: &[SentResult],
) -> Option<Result<String, String>> {
    let signature = submitted.get(plan.primary_tx_index)?.signature.clone()?;
    for attempt in 0..3 {
        let status_result = match rpc_request_with_client(
            shared_rpc_http_client(),
            rpc_url,
            "getSignatureStatuses",
            json!([
                [signature.clone()],
                {
                    "searchTransactionHistory": true
                }
            ]),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => return None,
        };
        let status = status_result
            .get("value")
            .and_then(Value::as_array)
            .and_then(|entries| entries.first());
        if let Some(status) = status
            && let Some(reconciled) = reconcile_submitted_signature_status(&signature, status)
        {
            return Some(reconciled);
        }
        if attempt < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
    None
}

async fn append_primary_failure_diagnostics(
    rpc_url: &str,
    plan: &CompiledTradePlan,
    error: String,
) -> String {
    let Some(primary) = plan.transactions.get(plan.primary_tx_index) else {
        return error;
    };
    let simulated = simulate_transactions(
        rpc_url,
        std::slice::from_ref(primary),
        &plan.transport_plan.commitment,
    )
    .await;
    match simulated {
        Ok((results, warnings)) => {
            let mut diagnostics = Vec::new();
            if let Some(primary_result) = results.first() {
                if let Some(sim_err) = primary_result.err.as_ref() {
                    diagnostics.push(format!("simulate_err={sim_err}"));
                }
                if let Some(units) = primary_result.units_consumed {
                    diagnostics.push(format!("simulate_units={units}"));
                }
                let logs_tail = primary_result
                    .logs
                    .iter()
                    .rev()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>();
                if !logs_tail.is_empty() {
                    diagnostics.push(format!("simulate_logs={}", logs_tail.join(" // ")));
                }
            }
            if !warnings.is_empty() {
                diagnostics.push(format!("simulate_warnings={}", warnings.join(" // ")));
            }
            if diagnostics.is_empty() {
                error
            } else {
                format!("{error} | diagnostics: {}", diagnostics.join(" | "))
            }
        }
        Err(sim_error) => format!("{error} | diagnostics: simulate_failed={sim_error}"),
    }
}

fn normalize_request_for_dispatch_plan(
    request: &TradeRuntimeRequest,
    dispatch_plan: &TradeDispatchPlan,
) -> TradeRuntimeRequest {
    let mut normalized = request.clone();
    normalized.mint = dispatch_plan.resolved_mint.clone();
    normalized.pinned_pool = dispatch_plan.resolved_pinned_pool.clone();
    normalized
}

fn normalize_request_for_wrapper_trade(request: &TradeRuntimeRequest) -> TradeRuntimeRequest {
    let mut normalized = request.clone();
    let original_policy = normalized.policy.clone();
    match normalized.side {
        TradeSide::Buy => {
            normalized.policy.buy_funding_policy = BuyFundingPolicy::SolOnly;
        }
        TradeSide::Sell => {
            normalized.policy.sell_settlement_policy = SellSettlementPolicy::AlwaysToSol;
            normalized.policy.sell_settlement_asset = TradeSettlementAsset::Sol;
        }
    }
    if normalized.policy.buy_funding_policy != original_policy.buy_funding_policy
        || normalized.policy.sell_settlement_policy != original_policy.sell_settlement_policy
        || normalized.policy.sell_settlement_asset != original_policy.sell_settlement_asset
    {
        normalized.planned_route = None;
        normalized.planned_trade = None;
        normalized.warm_key = None;
    }
    normalized
}

fn warm_invalidation_fingerprints(
    request: &TradeRuntimeRequest,
    normalized_request: &TradeRuntimeRequest,
    rpc_url: &str,
) -> Vec<WarmFingerprint> {
    let mut fingerprints = Vec::new();
    for candidate in [request, normalized_request] {
        let fingerprint = build_fingerprint(
            &candidate.mint,
            candidate.pinned_pool.as_deref(),
            rpc_url,
            &candidate.policy.commitment,
            &route_policy_label(candidate),
            crate::rollout::allow_non_canonical_pool_trades(),
        );
        if !fingerprints.contains(&fingerprint) {
            fingerprints.push(fingerprint);
        }
    }
    fingerprints
}

fn stale_route_error_class(error: &str) -> Option<&'static str> {
    let normalized = error.trim();
    if normalized.contains("[stale_route_reclassified]") {
        Some("stale_route_reclassified")
    } else if normalized.contains("migration_family_unresolved") {
        Some("migration_family_unresolved")
    } else if normalized.contains("canonical_damm_pool_not_found") {
        Some("canonical_damm_pool_not_found")
    } else if normalized.contains("no Pump AMM WSOL pool was found") {
        Some("pump_amm_missing_after_completion")
    } else {
        None
    }
}

fn pump_creator_vault_auto_retry_enabled() -> bool {
    match std::env::var("EXECUTION_ENGINE_ENABLE_PUMP_CREATOR_VAULT_AUTO_RETRY") {
        Ok(value) => parse_bool_like_env(&value).unwrap_or(true),
        Err(_) => true,
    }
}

fn parse_bool_like_env(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn pump_creator_vault_retry_error_class(
    error: &str,
    family: &TradeVenueFamily,
) -> Option<&'static str> {
    if !matches!(family, TradeVenueFamily::PumpBondingCurve)
        || !pump_creator_vault_auto_retry_enabled()
    {
        return None;
    }
    let normalized = error.to_ascii_lowercase();
    let has_creator_vault_context = normalized.contains("creator_vault")
        || normalized.contains("constraintseeds")
        || normalized.contains("seeds constraint was violated");
    let has_custom_2006 = normalized.contains("\"custom\":2006")
        || normalized.contains("\"custom\": 2006")
        || normalized.contains("custom:2006")
        || normalized.contains("custom: 2006")
        || normalized.contains("error number: 2006")
        || normalized.contains("custom program error: 0x7d6");
    if has_custom_2006 || has_creator_vault_context {
        Some("pump_creator_vault_constraint_seeds")
    } else {
        None
    }
}

fn reroute_request_after_stale_error(request: &TradeRuntimeRequest) -> TradeRuntimeRequest {
    let mut reroute = request.clone();
    reroute.planned_route = None;
    reroute.planned_trade = None;
    reroute.warm_key = None;
    reroute
}

async fn invalidate_route_retry_caches(
    request: &TradeRuntimeRequest,
    normalized_request: Option<&TradeRuntimeRequest>,
    fingerprints: &[WarmFingerprint],
    rpc_url: &str,
) {
    for fingerprint in fingerprints {
        shared_mint_warm_cache().invalidate(fingerprint).await;
    }
    let mut candidates = vec![request.clone()];
    if let Some(normalized_request) = normalized_request {
        candidates.push(normalized_request.clone());
    } else if let Some(planned_route) = request.planned_route.as_ref() {
        candidates.push(normalize_request_for_dispatch_plan(request, planned_route));
    }
    for candidate in candidates {
        let route_key = RouteIndexKey::new(
            &candidate.mint,
            rpc_url,
            &candidate.policy.commitment,
            side_label(&candidate.side),
            &route_policy_label(&candidate),
            candidate.pinned_pool.as_deref(),
            allow_non_canonical_pool_trades(),
        );
        shared_route_index().invalidate(&route_key).await;
        shared_warming_service()
            .invalidate_selector(
                rpc_url,
                &candidate.policy.commitment,
                side_label(&candidate.side),
                &route_policy_label(&candidate),
                &candidate.mint,
                candidate.pinned_pool.as_deref(),
                allow_non_canonical_pool_trades(),
            )
            .await;
        shared_warming_service()
            .invalidate_selectors_for_mint(
                rpc_url,
                &candidate.policy.commitment,
                side_label(&candidate.side),
                &candidate.mint,
            )
            .await;
    }
}

fn validate_transport_for_compiled_trade(
    transport_plan: &TransportPlan,
    dependency_mode: TransactionDependencyMode,
    transaction_count: usize,
) -> Result<(), String> {
    if dependency_mode != TransactionDependencyMode::Dependent || transaction_count <= 1 {
        return Ok(());
    }
    if transport_plan.execution_class == "bundle" || transport_plan.ordering == "bundle" {
        return Ok(());
    }
    Err(format!(
        "Dependent multi-transaction execution requires bundle transport, but {} resolved to {} (class={}, ordering={}).",
        transport_plan.requested_provider,
        transport_plan.transport_type,
        transport_plan.execution_class,
        transport_plan.ordering
    ))
}

fn validate_submitted_trade_results<'a>(
    submitted: &'a [SentResult],
    primary_tx_index: usize,
    dependency_mode: TransactionDependencyMode,
    commitment: &str,
) -> Result<&'a SentResult, String> {
    let primary = submitted
        .get(primary_tx_index)
        .ok_or_else(|| "Transport did not return a submission result.".to_string())?;
    let primary_signature = primary
        .signature
        .clone()
        .unwrap_or_else(|| primary.label.clone());
    let results_to_validate: &[SentResult] =
        if dependency_mode == TransactionDependencyMode::Dependent {
            submitted
        } else {
            std::slice::from_ref(primary)
        };
    for result in results_to_validate {
        if let Some(error) = result.error.as_deref() {
            return Err(error.to_string());
        }
        let confirmed = matches!(
            result.confirmation_status.as_deref(),
            Some("confirmed" | "finalized")
        );
        let signature = result
            .signature
            .clone()
            .unwrap_or_else(|| result.label.clone());
        if !confirmed {
            let _ = signature;
            return Err(format!(
                "Transport submitted transaction {primary_signature}, but {commitment} confirmation was not observed."
            ));
        }
    }
    Ok(primary)
}

fn validate_runtime_shared_alt_boundary(
    selector: &LifecycleAndCanonicalMarket,
    adapter_label: &str,
    transactions: &[CompiledTransaction],
) -> Result<(), String> {
    if !selector_requires_shared_alt(selector) {
        return Ok(());
    }
    let mut issues = Vec::new();
    for transaction in transactions {
        let used_tables = transaction
            .lookup_tables_used
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect::<BTreeSet<_>>();
        if used_tables.is_empty() {
            issues.push(format!(
                "{} missing shared ALT usage (format={})",
                transaction.label, transaction.format
            ));
            continue;
        }
        if !used_tables.contains(SHARED_SUPER_LOOKUP_TABLE) || used_tables.len() != 1 {
            issues.push(format!(
                "{} used [{}] instead of strict shared ALT {}",
                transaction.label,
                used_tables.into_iter().collect::<Vec<_>>().join(", "),
                SHARED_SUPER_LOOKUP_TABLE
            ));
        }
    }
    if issues.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Shared ALT runtime guard rejected {} {} trade: {}",
        adapter_label,
        selector.family.label(),
        issues.join(" | ")
    ))
}

fn selector_requires_shared_alt(selector: &LifecycleAndCanonicalMarket) -> bool {
    matches!(
        selector.family,
        crate::trade_planner::TradeVenueFamily::PumpBondingCurve
            | crate::trade_planner::TradeVenueFamily::PumpAmm
            | crate::trade_planner::TradeVenueFamily::RaydiumAmmV4
            | crate::trade_planner::TradeVenueFamily::RaydiumCpmm
            | crate::trade_planner::TradeVenueFamily::RaydiumLaunchLab
            | crate::trade_planner::TradeVenueFamily::BonkLaunchpad
            | crate::trade_planner::TradeVenueFamily::BonkRaydium
            | crate::trade_planner::TradeVenueFamily::MeteoraDbc
            | crate::trade_planner::TradeVenueFamily::MeteoraDammV2
            | crate::trade_planner::TradeVenueFamily::TrustedStableSwap
    )
}

async fn reuse_or_refresh_planned_selector(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    selector: LifecycleAndCanonicalMarket,
) -> Result<crate::trade_dispatch::TradeDispatchPlan, String> {
    if let Some(cached) = shared_warming_service()
        .current_selector(
            rpc_url,
            &request.policy.commitment,
            side_label(&request.side),
            &route_policy_label(request),
            &request.mint,
            request.pinned_pool.as_deref(),
            allow_non_canonical_pool_trades(),
        )
        .await
    {
        if !cached.is_stale(now_unix_ms()) && cached.selector.same_route_as(&selector) {
            return Ok(crate::trade_dispatch::TradeDispatchPlan {
                adapter: adapter_for_selector(&selector)?,
                selector,
                execution_backend: crate::rollout::preferred_execution_backend(),
                raw_address: request.mint.clone(),
                resolved_input_kind: crate::trade_dispatch::TradeInputKind::Mint,
                resolved_mint: request.mint.clone(),
                resolved_pinned_pool: request.pinned_pool.clone(),
                non_canonical: false,
            });
        }
    }
    let refreshed = resolve_trade_plan(request).await?;
    if !refreshed.selector.same_route_as(&selector) {
        return Err(format!(
            "[stale_route_reclassified] Planner output changed during send-time freshness check for address {}.",
            request.mint
        ));
    }
    Ok(refreshed)
}

async fn reuse_or_refresh_planned_route(
    rpc_url: &str,
    request: &TradeRuntimeRequest,
    planned_route: TradeDispatchPlan,
) -> Result<crate::trade_dispatch::TradeDispatchPlan, String> {
    let normalized_request = normalize_request_for_dispatch_plan(request, &planned_route);
    if let Some(cached) = shared_warming_service()
        .current_selector(
            rpc_url,
            &normalized_request.policy.commitment,
            side_label(&normalized_request.side),
            &route_policy_label(&normalized_request),
            &normalized_request.mint,
            normalized_request.pinned_pool.as_deref(),
            allow_non_canonical_pool_trades(),
        )
        .await
    {
        if !cached.is_stale(now_unix_ms()) && cached.selector.same_route_as(&planned_route.selector)
        {
            let mut reused = planned_route;
            reused.selector = cached.selector;
            return Ok(reused);
        }
    }
    let refreshed = resolve_trade_plan(request).await?;
    if refreshed.selector.same_route_as(&planned_route.selector) {
        return Ok(refreshed);
    }
    let migration_refresh_allowed = matches!(
        planned_route.selector.lifecycle,
        crate::trade_planner::TradeLifecycle::PreMigration
    ) && matches!(
        refreshed.selector.lifecycle,
        crate::trade_planner::TradeLifecycle::PostMigration
    ) && request
        .pinned_pool
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none();
    if migration_refresh_allowed {
        eprintln!(
            "[execution-engine][trade-runtime] [stale_route_reclassified] address={} old_market={} new_market={}",
            request.mint,
            planned_route.selector.canonical_market_key,
            refreshed.selector.canonical_market_key
        );
        return Ok(refreshed);
    }
    Err(format!(
        "[stale_route_reclassified] Planner output changed during send-time freshness check for address {}.",
        request.mint
    ))
}

fn side_label(side: &TradeSide) -> &'static str {
    match side {
        TradeSide::Buy => "buy",
        TradeSide::Sell => "sell",
    }
}

fn buy_funding_policy_label(policy: BuyFundingPolicy) -> &'static str {
    match policy {
        BuyFundingPolicy::SolOnly => "sol_only",
        BuyFundingPolicy::PreferUsd1ElseTopUp => "prefer_usd1_else_top_up",
        BuyFundingPolicy::Usd1Only => "usd1_only",
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
        TradeSide::Buy => format!(
            "buy:{}:wrapper_fee_bps={}:conversion={}",
            buy_funding_policy_label(request.policy.buy_funding_policy),
            fee_bps,
            route_policy_conversion_label(request)
        ),
        TradeSide::Sell => format!(
            "sell:{}:wrapper_fee_bps={}:conversion={}",
            sell_settlement_asset_label(request.policy.sell_settlement_asset),
            fee_bps,
            route_policy_conversion_label(request)
        ),
    }
}

fn route_policy_conversion_label(request: &TradeRuntimeRequest) -> &'static str {
    match request.side {
        TradeSide::Buy if request.policy.buy_funding_policy != BuyFundingPolicy::SolOnly => {
            "to_sol_in"
        }
        TradeSide::Sell if request.policy.sell_settlement_asset != TradeSettlementAsset::Sol => {
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

// Retry narrowly classified stale routes plus Pump creator-vault seed races.
pub async fn execute_wallet_trade(
    request: TradeRuntimeRequest,
    wallet_key: String,
) -> Result<ExecutedRuntimeTrade, String> {
    execute_wallet_trade_inner(
        request,
        wallet_key,
        Option::<fn(&str, &CompiledTradePlan) -> Result<(), String>>::None,
        Option::<fn(&str)>::None,
    )
    .await
}

async fn execute_wallet_trade_inner<F, C>(
    request: TradeRuntimeRequest,
    wallet_key: String,
    pre_submit_check: Option<F>,
    on_submitted: Option<C>,
) -> Result<ExecutedRuntimeTrade, String>
where
    F: Fn(&str, &CompiledTradePlan) -> Result<(), String> + Send + Sync,
    C: Fn(&str) + Send + Sync,
{
    let timeout_wallet_label = crate::shared_config::wallet_display_label(&wallet_key);
    let timeout_side = side_label(&request.side).to_string();
    let trade_execution_timeout = trade_execution_hard_timeout(&request.policy.provider);
    tokio::time::timeout(trade_execution_timeout, async move {
        let rpc_url = configured_rpc_url();
        let mut rerouted_once = false;
        let mut creator_vault_retry_used = false;
        let mut active_request = request.clone();
        loop {
            let compile_started_at = now_unix_ms();
            let force_fresh_route = rerouted_once || creator_vault_retry_used;
            let compiled = match compile_wallet_trade_with_route_mode(
                &active_request,
                &wallet_key,
                force_fresh_route,
            )
            .await
            {
                Ok(compiled) => compiled,
                Err(error) => {
                    if creator_vault_retry_used {
                        return Err(error);
                    }
                    if !rerouted_once && let Some(error_class) = stale_route_error_class(&error) {
                        eprintln!(
                            "[execution-engine][trade-runtime] stale-route retry phase=compile class={} address={}",
                            error_class, active_request.mint
                        );
                        invalidate_route_retry_caches(&active_request, None, &[], &rpc_url).await;
                        active_request = reroute_request_after_stale_error(&active_request);
                        rerouted_once = true;
                        continue;
                    }
                    return Err(error);
                }
            };
            let compile_elapsed_ms = now_unix_ms().saturating_sub(compile_started_at);
            eprintln!(
                "[execution-engine][latency] phase=compile wallet={} mint={} family={} compile_ms={}",
                wallet_key,
                compiled.normalized_request.mint,
                compiled.selector.family.label(),
                compile_elapsed_ms
            );
            eprintln!(
            "[execution-engine][route-metrics] phase=compile wallet={} mint={} family={} elapsed_ms={} rpc_total={} rpc_methods={} phase_ms={}",
                wallet_key,
                compiled.normalized_request.mint,
                compiled.selector.family.label(),
                compiled.compile_metrics.elapsed_ms,
                compiled.compile_metrics.rpc_total(),
            compiled.compile_metrics.rpc_methods_json(),
            compiled.compile_metrics.phases_json()
            );
            let entry_preference_asset = compiled.entry_preference_asset;
            let normalized_request = compiled.normalized_request.clone();
            let warm_invalidation_fingerprints = compiled.warm_invalidation_fingerprints.clone();
            let selector_family = compiled.selector.family.clone();
            if let Some(check) = pre_submit_check.as_ref() {
                check(&wallet_key, &compiled)?;
            }
            let execute_started_at = now_unix_ms();
            match execute_compiled_trade_inner(compiled, on_submitted.as_ref()).await {
                Ok(signature) => {
                    invalidate_mint_warm_entries(&warm_invalidation_fingerprints).await;
                    crate::wallet_token_cache::shared_wallet_token_cache()
                        .invalidate(&wallet_key, &normalized_request.mint)
                        .await;
                    eprintln!(
                        "[execution-engine][latency] phase=wallet-execute wallet={} mint={} execute_ms={}",
                        wallet_key,
                        normalized_request.mint,
                        now_unix_ms().saturating_sub(execute_started_at)
                    );
                    return Ok(ExecutedRuntimeTrade {
                        tx_signature: signature,
                        entry_preference_asset,
                    });
                }
                Err(error) => {
                    if creator_vault_retry_used {
                        return Err(error);
                    }
                    if !rerouted_once && let Some(error_class) = stale_route_error_class(&error) {
                        eprintln!(
                            "[execution-engine][trade-runtime] stale-route retry phase=execute class={} address={}",
                            error_class, normalized_request.mint
                        );
                        invalidate_route_retry_caches(
                            &active_request,
                            Some(&normalized_request),
                            &warm_invalidation_fingerprints,
                            &rpc_url,
                        )
                        .await;
                        active_request = reroute_request_after_stale_error(&active_request);
                        rerouted_once = true;
                        continue;
                    }
                    if let Some(error_class) =
                        pump_creator_vault_retry_error_class(&error, &selector_family)
                    {
                        eprintln!(
                            "[execution-engine][trade-runtime] pump-creator-vault retry phase=execute class={} address={}",
                            error_class, normalized_request.mint
                        );
                        invalidate_route_retry_caches(
                            &active_request,
                            Some(&normalized_request),
                            &warm_invalidation_fingerprints,
                            &rpc_url,
                        )
                        .await;
                        active_request = reroute_request_after_stale_error(&active_request);
                        creator_vault_retry_used = true;
                        continue;
                    }
                    return Err(error);
                }
            }
        }
    })
    .await
    .map_err(|_| {
        format!(
            "{} transaction timed out after {}s for wallet {}",
            timeout_side,
            trade_execution_timeout.as_secs(),
            timeout_wallet_label,
        )
    })?
}

fn trade_execution_hard_timeout(provider: &str) -> std::time::Duration {
    if provider.trim().eq_ignore_ascii_case("hellomoon") {
        HELLOMOON_TRADE_EXECUTION_HARD_TIMEOUT
    } else {
        DEFAULT_TRADE_EXECUTION_HARD_TIMEOUT
    }
}

async fn invalidate_mint_warm_entries(fingerprints: &[WarmFingerprint]) {
    for fingerprint in fingerprints {
        shared_mint_warm_cache().invalidate(fingerprint).await;
    }
}

pub async fn execute_wallet_trade_outcome(
    request: TradeRuntimeRequest,
    wallet_key: String,
) -> WalletExecutionOutcome {
    match execute_wallet_trade(request, wallet_key.clone()).await {
        Ok(result) => WalletExecutionOutcome {
            wallet_key,
            status: BatchLifecycleStatus::Confirmed,
            tx_signature: Some(result.tx_signature),
            error: None,
        },
        Err(error) => WalletExecutionOutcome {
            wallet_key,
            status: BatchLifecycleStatus::Failed,
            tx_signature: None,
            error: Some(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trade_planner::{
        PlannerQuoteAsset, PlannerVerificationSource, TradeLifecycle, TradeVenueFamily,
        WrapperAction,
    };
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn sample_runtime_request() -> TradeRuntimeRequest {
        TradeRuntimeRequest {
            side: TradeSide::Buy,
            mint: "Pair111".to_string(),
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
                buy_funding_policy: BuyFundingPolicy::SolOnly,
                sell_settlement_policy: SellSettlementPolicy::AlwaysToSol,
                sell_settlement_asset: TradeSettlementAsset::Sol,
            },
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: Some("Pair111".to_string()),
            warm_key: Some("warm-1".to_string()),
        }
    }

    fn sample_selector() -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PostMigration,
            family: TradeVenueFamily::BonkRaydium,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: PlannerQuoteAsset::Usd1,
            verification_source: PlannerVerificationSource::HybridDerived,
            wrapper_action: WrapperAction::BonkRaydiumUsd1Buy,
            wrapper_accounts: vec!["pool-1".to_string()],
            market_subtype: None,
            direct_protocol_target: Some("raydium".to_string()),
            input_amount_hint: Some("0.5".to_string()),
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    fn sample_direct_stable_selector() -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PostMigration,
            family: TradeVenueFamily::TrustedStableSwap,
            canonical_market_key: "stable-pool-1".to_string(),
            quote_asset: PlannerQuoteAsset::Usdc,
            verification_source: PlannerVerificationSource::OnchainDerived,
            wrapper_action: WrapperAction::TrustedStableSwapBuy,
            wrapper_accounts: vec!["stable-pool-1".to_string()],
            market_subtype: Some("raydium-clmm".to_string()),
            direct_protocol_target: Some("raydium-clmm".to_string()),
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    fn sample_transaction(lookup_tables_used: &[&str]) -> CompiledTransaction {
        CompiledTransaction {
            label: "follow-buy".to_string(),
            format: "v0-alt".to_string(),
            serialized_base64: "AA==".to_string(),
            signature: Some("sig".to_string()),
            lookup_tables_used: lookup_tables_used
                .iter()
                .map(|value| value.to_string())
                .collect(),
            compute_unit_limit: None,
            compute_unit_price_micro_lamports: None,
            inline_tip_lamports: None,
            inline_tip_account: None,
        }
    }

    fn sample_transport_plan(provider: &str, transaction_count: usize) -> TransportPlan {
        build_transport_plan(
            &ExecutionTransportConfig {
                provider: provider.to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: false,
                track_send_block_height: false,
                mev_mode: if provider == "hellomoon" {
                    "secure".to_string()
                } else {
                    "off".to_string()
                },
                mev_protect: provider == "hellomoon",
            },
            transaction_count,
        )
    }

    fn sample_sent_result(
        label: &str,
        signature: &str,
        confirmation_status: Option<&str>,
        error: Option<&str>,
    ) -> SentResult {
        SentResult {
            label: label.to_string(),
            format: "v0-alt".to_string(),
            signature: Some(signature.to_string()),
            transport_type: "standard-rpc-fanout".to_string(),
            endpoint: None,
            attempted_endpoints: vec![],
            skip_preflight: false,
            max_retries: 0,
            confirmation_status: confirmation_status.map(str::to_string),
            error: error.map(str::to_string),
            bundle_id: None,
            attempted_bundle_ids: vec![],
            transaction_subscribe_account_required: vec![],
        }
    }

    #[test]
    fn shared_alt_runtime_guard_accepts_strict_shared_alt_usage() {
        assert!(
            validate_runtime_shared_alt_boundary(
                &sample_selector(),
                "bonk-native",
                &[sample_transaction(&[SHARED_SUPER_LOOKUP_TABLE])],
            )
            .is_ok()
        );
    }

    #[test]
    fn shared_alt_runtime_guard_rejects_missing_lookup_usage() {
        let error = validate_runtime_shared_alt_boundary(
            &sample_selector(),
            "bonk-native",
            &[sample_transaction(&[])],
        )
        .expect_err("missing lookup table usage should fail");
        assert!(error.contains("Shared ALT runtime guard rejected"));
        assert!(error.contains("missing shared ALT usage"));
    }

    #[test]
    fn normalize_request_for_dispatch_plan_rewrites_pair_inputs() {
        let request = sample_runtime_request();
        let dispatch_plan = TradeDispatchPlan {
            adapter: crate::trade_dispatch::adapter_for_selector(&sample_selector())
                .expect("adapter"),
            selector: sample_selector(),
            execution_backend: crate::rollout::preferred_execution_backend(),
            raw_address: "Pair111".to_string(),
            resolved_input_kind: crate::trade_dispatch::TradeInputKind::Pair,
            resolved_mint: "Mint111".to_string(),
            resolved_pinned_pool: Some("Pool222".to_string()),
            non_canonical: false,
        };

        let normalized = normalize_request_for_dispatch_plan(&request, &dispatch_plan);

        assert_eq!(normalized.mint, "Mint111");
        assert_eq!(normalized.pinned_pool.as_deref(), Some("Pool222"));
        assert_eq!(normalized.warm_key.as_deref(), Some("warm-1"));
    }

    #[test]
    fn normalize_request_for_wrapper_trade_converts_non_sol_buy_and_clears_cached_route() {
        let mut request = sample_runtime_request();
        request.policy.buy_funding_policy = BuyFundingPolicy::Usd1Only;
        request.planned_trade = Some(sample_selector());
        request.warm_key = Some("old-non-feeable-warm-key".to_string());

        let normalized = normalize_request_for_wrapper_trade(&request);

        assert_eq!(
            normalized.policy.buy_funding_policy,
            BuyFundingPolicy::SolOnly
        );
        assert!(normalized.planned_trade.is_none());
        assert!(normalized.warm_key.is_none());
    }

    #[test]
    fn trusted_stable_no_sol_routes_are_native_direct_eligible() {
        let request = sample_runtime_request();
        let selector = sample_direct_stable_selector();

        assert_eq!(
            classify_trade_route(&selector, &request),
            WrapperRouteClassification::NoSol
        );
        assert!(selector_allows_native_no_sol_trade(&selector));
        assert!(!selector_allows_native_no_sol_trade(&sample_selector()));
    }

    #[test]
    fn wrapper_buy_adapter_request_uses_net_venue_input() {
        let request = sample_runtime_request();
        let mut selector = sample_selector();
        selector.quote_asset = PlannerQuoteAsset::Sol;
        let gross = 500_000_000;
        let payload = WrapperInstructionPayload {
            route_classification: WrapperRouteClassification::SolIn,
            route_metadata: crate::wrapper_payload::WrapperRouteMetadata {
                version: crate::wrapper_abi::ABI_VERSION,
                route_mode: Some(crate::wrapper_abi::SwapRouteMode::SolIn),
                direction: Some(crate::wrapper_abi::SwapRouteDirection::Buy),
                settlement: Some(crate::wrapper_abi::SwapRouteSettlement::Token),
                fee_mode: Some(crate::wrapper_abi::SwapRouteFeeMode::SolPre),
                fee_bps: 10,
                gross_sol_in_lamports: gross,
                gross_token_in_amount: 0,
                min_net_output: 0,
            },
            fee_lamports_estimate: 500_000,
            gross_sol_in_lamports: gross,
        };

        let adjusted = request_with_net_wrapper_buy_input(
            &selector,
            &request,
            WrapperRouteClassification::SolIn,
            Some(&payload),
        )
        .expect("adjust");

        assert_eq!(adjusted.buy_amount_sol.as_deref(), Some("0.4995"));
    }

    #[test]
    fn wrapper_buy_adapter_request_preserves_bonk_usd1_gross_input() {
        let request = sample_runtime_request();
        let gross = 500_000_000;
        let payload = WrapperInstructionPayload {
            route_classification: WrapperRouteClassification::SolIn,
            route_metadata: crate::wrapper_payload::WrapperRouteMetadata {
                version: crate::wrapper_abi::ABI_VERSION,
                route_mode: Some(crate::wrapper_abi::SwapRouteMode::SolIn),
                direction: Some(crate::wrapper_abi::SwapRouteDirection::Buy),
                settlement: Some(crate::wrapper_abi::SwapRouteSettlement::Token),
                fee_mode: Some(crate::wrapper_abi::SwapRouteFeeMode::SolPre),
                fee_bps: 10,
                gross_sol_in_lamports: gross,
                gross_token_in_amount: 0,
                min_net_output: 0,
            },
            fee_lamports_estimate: 500_000,
            gross_sol_in_lamports: gross,
        };

        let adjusted = request_with_net_wrapper_buy_input(
            &sample_selector(),
            &request,
            WrapperRouteClassification::SolIn,
            Some(&payload),
        )
        .expect("adjust");

        assert_eq!(adjusted.buy_amount_sol.as_deref(), Some("0.5"));
    }

    #[test]
    fn already_wrapped_passthrough_is_limited_to_known_dynamic_routes() {
        let bonk = sample_selector();
        assert!(allow_already_wrapped_passthrough(
            &bonk,
            "follow-buy-atomic",
            WrapperRouteClassification::SolIn
        ));
        assert!(allow_already_wrapped_passthrough(
            &bonk,
            "follow-sell",
            WrapperRouteClassification::SolOut
        ));
        assert!(!allow_already_wrapped_passthrough(
            &bonk,
            "follow-buy",
            WrapperRouteClassification::SolIn
        ));

        let mut meteora = bonk.clone();
        meteora.family = TradeVenueFamily::MeteoraDammV2;
        meteora.quote_asset = PlannerQuoteAsset::Usdc;
        assert!(allow_already_wrapped_passthrough(
            &meteora,
            "meteora-usdc-damm-buy",
            WrapperRouteClassification::SolIn
        ));
        assert!(allow_already_wrapped_passthrough(
            &meteora,
            "meteora-usdc-damm-sell",
            WrapperRouteClassification::SolOut
        ));
        assert!(!allow_already_wrapped_passthrough(
            &meteora,
            "follow-buy",
            WrapperRouteClassification::SolIn
        ));

        let mut pump = bonk.clone();
        pump.family = TradeVenueFamily::PumpBondingCurve;
        assert!(!allow_already_wrapped_passthrough(
            &pump,
            "follow-buy-atomic",
            WrapperRouteClassification::SolIn
        ));
    }

    #[test]
    fn normalize_request_for_wrapper_trade_converts_non_sol_sell_and_clears_cached_route() {
        let mut request = sample_runtime_request();
        request.side = TradeSide::Sell;
        request.buy_amount_sol = None;
        request.sell_intent = Some(RuntimeSellIntent::Percent("100".to_string()));
        request.policy.sell_settlement_policy = SellSettlementPolicy::AlwaysToUsd1;
        request.policy.sell_settlement_asset = TradeSettlementAsset::Usd1;
        request.planned_trade = Some(sample_selector());
        request.warm_key = Some("old-non-feeable-warm-key".to_string());

        let normalized = normalize_request_for_wrapper_trade(&request);

        assert_eq!(
            normalized.policy.sell_settlement_policy,
            SellSettlementPolicy::AlwaysToSol
        );
        assert_eq!(
            normalized.policy.sell_settlement_asset,
            TradeSettlementAsset::Sol
        );
        assert!(normalized.planned_trade.is_none());
        assert!(normalized.warm_key.is_none());
    }

    #[test]
    fn route_policy_label_includes_fee_tier_and_conversion_state() {
        let mut request = sample_runtime_request();
        request.policy.buy_funding_policy = BuyFundingPolicy::Usd1Only;
        let buy_label = route_policy_label(&request);
        assert!(buy_label.contains("wrapper_fee_bps="));
        assert!(buy_label.contains("conversion=to_sol_in"));

        request.side = TradeSide::Sell;
        request.policy.sell_settlement_asset = TradeSettlementAsset::Usd1;
        let sell_label = route_policy_label(&request);
        assert!(sell_label.contains("wrapper_fee_bps="));
        assert!(sell_label.contains("conversion=to_sol_out"));
    }

    #[test]
    fn warm_invalidation_fingerprints_include_raw_and_normalized_shapes() {
        let mut request = sample_runtime_request();
        request.mint = "Mint111".to_string();
        let dispatch_plan = TradeDispatchPlan {
            adapter: crate::trade_dispatch::adapter_for_selector(&sample_selector())
                .expect("adapter"),
            selector: sample_selector(),
            execution_backend: crate::rollout::preferred_execution_backend(),
            raw_address: "Pair111".to_string(),
            resolved_input_kind: crate::trade_dispatch::TradeInputKind::Pair,
            resolved_mint: "Mint111".to_string(),
            resolved_pinned_pool: Some("Pool222".to_string()),
            non_canonical: false,
        };
        let normalized = normalize_request_for_dispatch_plan(&request, &dispatch_plan);

        let fingerprints =
            warm_invalidation_fingerprints(&request, &normalized, &configured_rpc_url());

        assert_eq!(fingerprints.len(), 2);
        assert!(fingerprints.iter().any(|fingerprint| {
            fingerprint.mint == "Mint111" && fingerprint.pinned_pool.as_deref() == Some("Pair111")
        }));
        assert!(fingerprints.iter().any(|fingerprint| {
            fingerprint.mint == "Mint111" && fingerprint.pinned_pool.as_deref() == Some("Pool222")
        }));
    }

    #[test]
    fn dependent_transport_requires_bundle_capability() {
        let error = validate_transport_for_compiled_trade(
            &sample_transport_plan("standard-rpc", 2),
            TransactionDependencyMode::Dependent,
            2,
        )
        .expect_err("non-bundle transport should be rejected");

        assert!(error.contains("requires bundle transport"));
        assert!(
            validate_transport_for_compiled_trade(
                &sample_transport_plan("jito-bundle", 2),
                TransactionDependencyMode::Dependent,
                2,
            )
            .is_ok()
        );
    }

    #[test]
    fn dependent_result_validation_checks_all_transactions() {
        let submitted = vec![
            sample_sent_result("topup", "sig-topup", Some("confirmed"), None),
            sample_sent_result("action", "sig-action", Some("confirmed"), Some("boom")),
        ];
        let error = validate_submitted_trade_results(
            &submitted,
            1,
            TransactionDependencyMode::Dependent,
            "confirmed",
        )
        .expect_err("dependent batches should fail on any leg");

        assert!(error.contains("boom"));
    }

    #[test]
    fn dependent_result_validation_preserves_primary_signature_for_confirmation_gaps() {
        let submitted = vec![
            sample_sent_result("topup", "sig-topup", Some("confirmed"), None),
            sample_sent_result("action", "sig-action", Some("processed"), None),
        ];
        let error = validate_submitted_trade_results(
            &submitted,
            1,
            TransactionDependencyMode::Dependent,
            "finalized",
        )
        .expect_err("missing confirmation should bubble the primary signature");

        assert_eq!(
            error,
            "Transport submitted transaction sig-action, but finalized confirmation was not observed."
        );
    }

    #[test]
    fn stale_route_error_class_matches_supported_retry_errors() {
        assert_eq!(
            stale_route_error_class("[stale_route_reclassified] route changed"),
            Some("stale_route_reclassified")
        );
        assert_eq!(
            stale_route_error_class("[migration_family_unresolved] venue changed"),
            Some("migration_family_unresolved")
        );
        assert_eq!(
            stale_route_error_class("[canonical_damm_pool_not_found] missing pool"),
            Some("canonical_damm_pool_not_found")
        );
        assert_eq!(
            stale_route_error_class(
                "mint reports complete=true but no Pump AMM WSOL pool was found"
            ),
            Some("pump_amm_missing_after_completion")
        );
        assert_eq!(stale_route_error_class("transport timeout"), None);
    }

    #[test]
    fn stale_route_classifier_rejects_post_submit_errors() {
        let post_submit_errors = [
            "Transport submitted transaction sig-X, but finalized confirmation was not observed.",
            "Transport did not return a submission result.",
            "Helius sender returned HTTP 500 for bundle",
            "Standard RPC submission failed for action on all attempted endpoints: blockhash not found",
            "Program log: custom program error: 0x17",
            "Transaction simulation failed: Error processing Instruction 3: custom program error",
            "Blockhash expired",
            "InstructionError(0, InsufficientFundsForRent)",
            "Transaction was not confirmed in 60.00 seconds.",
        ];
        for error in post_submit_errors {
            assert!(
                stale_route_error_class(error).is_none(),
                "post-submit error must not be classified as stale-route: {error}"
            );
        }
    }

    #[test]
    fn pump_creator_vault_retry_detects_pump_custom_2006() {
        let _guard = env_lock().lock().expect("env lock");
        unsafe {
            std::env::remove_var("EXECUTION_ENGINE_ENABLE_PUMP_CREATOR_VAULT_AUTO_RETRY");
        }
        assert_eq!(
            pump_creator_vault_retry_error_class(
                r#"Transaction abc failed on-chain: {"InstructionError":[4,{"Custom":2006}]}"#,
                &TradeVenueFamily::PumpBondingCurve,
            ),
            Some("pump_creator_vault_constraint_seeds")
        );
        assert_eq!(
            pump_creator_vault_retry_error_class(
                "Program log: AnchorError caused by account: creator_vault. Error Code: ConstraintSeeds. Error Number: 2006.",
                &TradeVenueFamily::PumpBondingCurve,
            ),
            Some("pump_creator_vault_constraint_seeds")
        );
    }

    #[test]
    fn pump_creator_vault_retry_ignores_non_pump_or_non_2006_errors() {
        let _guard = env_lock().lock().expect("env lock");
        unsafe {
            std::env::remove_var("EXECUTION_ENGINE_ENABLE_PUMP_CREATOR_VAULT_AUTO_RETRY");
        }
        assert_eq!(
            pump_creator_vault_retry_error_class(
                r#"Transaction abc failed on-chain: {"InstructionError":[4,{"Custom":2006}]}"#,
                &TradeVenueFamily::PumpAmm,
            ),
            None
        );
        assert_eq!(
            pump_creator_vault_retry_error_class(
                r#"Transaction abc failed on-chain: {"InstructionError":[4,{"Custom":6003}]}"#,
                &TradeVenueFamily::PumpBondingCurve,
            ),
            None
        );
    }

    #[test]
    fn pump_creator_vault_retry_respects_env_opt_out() {
        let _guard = env_lock().lock().expect("env lock");
        unsafe {
            std::env::set_var(
                "EXECUTION_ENGINE_ENABLE_PUMP_CREATOR_VAULT_AUTO_RETRY",
                "false",
            );
        }
        assert_eq!(
            pump_creator_vault_retry_error_class(
                r#"Transaction abc failed on-chain: {"InstructionError":[4,{"Custom":2006}]}"#,
                &TradeVenueFamily::PumpBondingCurve,
            ),
            None
        );
        unsafe {
            std::env::remove_var("EXECUTION_ENGINE_ENABLE_PUMP_CREATOR_VAULT_AUTO_RETRY");
        }
    }

    #[test]
    fn reconcile_submitted_signature_status_accepts_ok_status_without_commitment_label() {
        let status = json!({
            "slot": 123,
            "confirmationStatus": null,
            "status": {
                "Ok": null
            },
            "err": null
        });

        assert_eq!(
            reconcile_submitted_signature_status("sig-1", &status),
            Some(Ok("sig-1".to_string()))
        );
    }

    #[test]
    fn reconcile_submitted_signature_status_returns_actual_onchain_error() {
        let status = json!({
            "slot": 123,
            "confirmationStatus": "finalized",
            "status": {
                "Err": {
                    "InstructionError": [7, { "Custom": 6013 }]
                }
            },
            "err": {
                "InstructionError": [7, { "Custom": 6013 }]
            }
        });

        assert_eq!(
            reconcile_submitted_signature_status("sig-2", &status),
            Some(Err(
                "Transaction sig-2 failed on-chain: {\"InstructionError\":[7,{\"Custom\":6013}]}"
                    .to_string()
            ))
        );
    }

    #[test]
    fn reroute_request_after_stale_error_clears_cached_route_biases() {
        let mut request = sample_runtime_request();
        request.planned_trade = Some(sample_selector());

        let reroute = reroute_request_after_stale_error(&request);

        assert!(reroute.planned_route.is_none());
        assert!(reroute.planned_trade.is_none());
        assert!(reroute.warm_key.is_none());
        assert_eq!(reroute.pinned_pool.as_deref(), Some("Pair111"));
    }
}
