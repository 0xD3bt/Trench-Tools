use crate::{
    extension_api::{
        BatchLifecycleStatus, BuyFundingPolicy, MevMode, SellSettlementPolicy,
        TradeSettlementAsset, TradeSide,
    },
    mint_warm_cache::{WarmFingerprint, build_fingerprint, shared_mint_warm_cache},
    rollout::{
        allow_non_canonical_pool_trades, runtime_execution_backend, wrapper_fee_vault_pubkey,
    },
    route_index::{RouteIndexKey, shared_route_index},
    rpc_client::{
        CompiledTransaction, SentResult, configured_rpc_url,
        confirm_submitted_transactions_for_transport, rpc_request_with_client,
        shared_rpc_http_client, simulate_transactions,
        submit_independent_transactions_for_transport,
    },
    trade_dispatch::{
        TradeDispatchPlan, TransactionDependencyMode, adapter_for_selector,
        compile_trade_for_adapter, resolve_trade_plan, resolve_trade_plan_fresh,
    },
    trade_planner::LifecycleAndCanonicalMarket,
    transport::{ExecutionTransportConfig, TransportPlan, build_transport_plan},
    wallet_store::load_solana_wallet_by_env_key,
    warming_service::shared_warming_service,
    wrapper_adapter::allowed_inner_program_pubkeys,
    wrapper_compile::{
        WrapCompiledTransactionError, WrapCompiledTransactionRequest, wrap_compiled_transaction,
    },
    wrapper_payload::{
        WrapperInstructionPayload, WrapperRouteClassification, build_wrapper_instruction_payload,
        classify_trade_route, trade_touches_sol,
    },
};
use serde_json::{Value, json};
use solana_sdk::signature::Signer;
use std::collections::BTreeSet;

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
    pub transport_plan: TransportPlan,
    pub transactions: Vec<CompiledTransaction>,
    pub primary_tx_index: usize,
    pub dependency_mode: TransactionDependencyMode,
    pub entry_preference_asset: Option<TradeSettlementAsset>,
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

async fn compile_wallet_trade_with_route_mode(
    request: &TradeRuntimeRequest,
    wallet_key: &str,
    force_fresh_route: bool,
) -> Result<CompiledTradePlan, String> {
    let rpc_url = configured_rpc_url();
    let dispatch_plan = if force_fresh_route {
        let mut reroute_request = request.clone();
        reroute_request.planned_route = None;
        reroute_request.planned_trade = None;
        reroute_request.warm_key = None;
        resolve_trade_plan_fresh(&reroute_request).await?
    } else if let Some(planned_route) = request.planned_route.clone() {
        reuse_or_refresh_planned_route(&rpc_url, request, planned_route).await?
    } else if let Some(selector) = request.planned_trade.clone() {
        reuse_or_refresh_planned_selector(&rpc_url, request, selector).await?
    } else {
        resolve_trade_plan(request).await?
    };
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
    let transactions = compile_trade_for_adapter(
        dispatch_plan.adapter,
        &dispatch_plan.selector,
        &normalized_request,
        wallet_key,
    )
    .await?;
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
    let transport_plan = build_transport_plan(
        &ExecutionTransportConfig {
            provider: normalized_request.policy.provider.clone(),
            endpoint_profile: normalized_request.policy.endpoint_profile.clone(),
            commitment: normalized_request.policy.commitment.clone(),
            skip_preflight: normalized_request.policy.skip_preflight,
            track_send_block_height: normalized_request.policy.track_send_block_height,
            mev_mode: crate::launchdeck_bridge::to_launchdeck_execution(&normalized_request.policy)
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
    validate_runtime_shared_alt_boundary(
        &dispatch_plan.selector,
        dispatch_plan.adapter.label(),
        &transactions.transactions,
    )?;
    let wallet_pubkey = load_solana_wallet_by_env_key(wallet_key)?
        .pubkey()
        .to_string();
    let wrapper_route = classify_trade_route(&dispatch_plan.selector, &normalized_request);
    let wrapper_payload = if wrapper_route.touches_sol() {
        Some(build_wrapper_instruction_payload(
            &dispatch_plan.selector,
            &normalized_request,
            wallet_pubkey,
        ))
    } else {
        None
    };
    let wrapper_inner_program_label = if wrapper_route.touches_sol() {
        crate::wrapper_adapter::inner_program_label_for_selector(&dispatch_plan.selector)
            .unwrap_or("<unknown>")
    } else {
        "<none>"
    };
    eprintln!(
        "[execution-engine][trade-runtime] wrapper-classify wallet={} mint={} family={} route={} touches_sol={} inner_program={}",
        wallet_key,
        normalized_request.mint,
        dispatch_plan.selector.family.label(),
        wrapper_route.label(),
        wrapper_route.touches_sol(),
        wrapper_inner_program_label
    );

    // Route SOL-touching trades through the wrapper program.
    let final_transactions = if wrapper_route.touches_sol() {
        wrap_native_transactions(
            &dispatch_plan.selector,
            &normalized_request,
            &transactions.transactions,
            wallet_key,
            &wrapper_payload,
            wrapper_route,
        )
        .await?
    } else {
        transactions.transactions
    };

    let execution_backend = if wrapper_route.touches_sol() {
        "wrapper"
    } else {
        runtime_execution_backend().label()
    };

    Ok(CompiledTradePlan {
        adapter: dispatch_plan.adapter.label(),
        execution_backend,
        selector: dispatch_plan.selector,
        normalized_request,
        warm_invalidation_fingerprints,
        wrapper_route,
        wrapper_payload,
        transport_plan,
        transactions: final_transactions,
        primary_tx_index: transactions.primary_tx_index,
        dependency_mode: transactions.dependency_mode,
        entry_preference_asset: transactions.entry_preference_asset,
    })
}

/// Convert native-compiled transactions into wrapper-wrapped ones.
async fn wrap_native_transactions(
    selector: &LifecycleAndCanonicalMarket,
    normalized_request: &TradeRuntimeRequest,
    transactions: &[CompiledTransaction],
    wallet_key: &str,
    wrapper_payload: &Option<WrapperInstructionPayload>,
    wrapper_route: WrapperRouteClassification,
) -> Result<Vec<CompiledTransaction>, String> {
    let rpc_url = configured_rpc_url();
    let fee_vault = wrapper_fee_vault_pubkey();
    let payer = load_solana_wallet_by_env_key(wallet_key)?;
    let lookup_tables = crate::pump_native::load_shared_super_lookup_tables(&rpc_url).await?;
    let allowed_programs = allowed_inner_program_pubkeys();

    // Missing payload falls back to a zeroed SolOut estimate.
    let (route_kind, fee_bps, gross_sol_in, min_net_output) = match wrapper_payload {
        Some(payload) => {
            let req = &payload.request;
            (
                req.route_kind,
                req.fee_bps,
                req.gross_sol_in_lamports,
                req.min_net_output,
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
    for source in transactions {
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
            Ok(tx) => wrapped.push(tx),
            Err(WrapCompiledTransactionError::NoVenueInstruction) => {
                // Non-venue transactions stay unchanged.
                eprintln!(
                    "[execution-engine][wrapper-wrap] skip label={} reason=no_venue_instruction family={}",
                    source.label,
                    selector.family.label()
                );
                wrapped.push(source.clone());
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
    match request.side {
        TradeSide::Buy => format!(
            "buy:{}",
            buy_funding_policy_label(request.policy.buy_funding_policy)
        ),
        TradeSide::Sell => format!(
            "sell:{}",
            sell_settlement_asset_label(request.policy.sell_settlement_asset)
        ),
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

// Retry only on stale-route errors that happen before a signed tx lands.
pub async fn execute_wallet_trade(
    request: TradeRuntimeRequest,
    wallet_key: String,
) -> Result<ExecutedRuntimeTrade, String> {
    let timeout_wallet_label = crate::shared_config::wallet_display_label(&wallet_key);
    let timeout_side = side_label(&request.side).to_string();
    let trade_execution_timeout = trade_execution_hard_timeout(&request.policy.provider);
    tokio::time::timeout(trade_execution_timeout, async move {
        let rpc_url = configured_rpc_url();
        let mut rerouted_once = false;
        let mut active_request = request.clone();
        loop {
            let compile_started_at = now_unix_ms();
            let compiled = match compile_wallet_trade_with_route_mode(
                &active_request,
                &wallet_key,
                rerouted_once,
            )
            .await
            {
                Ok(compiled) => compiled,
                Err(error) => {
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
            eprintln!(
                "[execution-engine][latency] phase=compile wallet={} mint={} family={} compile_ms={}",
                wallet_key,
                compiled.normalized_request.mint,
                compiled.selector.family.label(),
                now_unix_ms().saturating_sub(compile_started_at)
            );
            let entry_preference_asset = compiled.entry_preference_asset;
            let normalized_request = compiled.normalized_request.clone();
            let warm_invalidation_fingerprints = compiled.warm_invalidation_fingerprints.clone();
            let execute_started_at = now_unix_ms();
            match execute_compiled_trade(compiled).await {
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
