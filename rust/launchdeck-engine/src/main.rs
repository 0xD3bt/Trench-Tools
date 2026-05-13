mod alt_diagnostics;
mod app_logs;
mod bags_native;
mod bonk_native;
mod compiled_transaction_signers;
mod config;
mod crypto;
mod endpoint_profile;
mod execution_engine_bridge;
mod follow;
#[path = "follow/controlplane.rs"]
mod follow_controlplane;
mod fs_utils;
mod image_library;
mod launchpad_dispatch;
mod launchpad_runtime;
mod launchpad_warm;
mod launchpads;
mod observability;
mod paths;
mod provider_tip;
mod providers;
mod pump_native;
mod report;
mod reports_browser;
mod rpc;
mod runtime;
mod strategies;
mod transport;
mod ui_bridge;
mod ui_config;
mod vamp;
mod vanity_pool;
mod wallet;
mod warm_manager;
mod wrapper_compile;

use crate::{
    app_logs::{list_error_logs, list_live_logs, record_error, record_info, record_warn},
    bags_native::{
        bags_runtime_status_payload, compile_launch_transaction as compile_bags_launch_transaction,
        summarize_transactions as summarize_bags_transactions,
    },
    config::{
        NormalizedConfig, NormalizedFollowLaunch, RawConfig,
        configured_bags_setup_confirm_timeout_secs, configured_bags_setup_gate_commitment,
        normalize_raw_config,
    },
    execution_engine_bridge::{
        confirmed_trade_record_from_sent_result, record_confirmed_trades,
        spawn_startup_outbox_flush_task,
    },
    follow::{
        FollowArmRequest, FollowCancelRequest, FollowDaemonClient, FollowJobResponse,
        FollowReserveRequest, FollowStopAllRequest, build_action_records,
        should_use_post_setup_creator_vault_for_buy,
    },
    follow_controlplane::{
        attach_follow_daemon_report, cancel_follow_job_best_effort,
        cancel_reserved_follow_job_on_launch_failure, configured_follow_daemon_base_url,
        configured_follow_daemon_transport, follow_active_jobs_count, follow_daemon_browser_client,
        follow_daemon_status_payload, spawn_follow_job_activity_refresh_task,
    },
    fs_utils::atomic_write,
    image_library::{
        build_image_library_payload, create_category, delete_image, save_image_bytes, update_image,
    },
    launchpad_dispatch::{
        NativeLaunchArtifacts, compile_atomic_follow_buy_for_launchpad,
        derive_canonical_pool_id_for_launchpad, launchpad_action_backend,
        launchpad_action_rollout_state, maybe_wrap_launch_dev_buy_transaction,
        predict_dev_buy_token_amount_for_launchpad,
    },
    launchpad_runtime::{
        FeeRecipientLookupRequest, LaunchQuoteRequest, NativeLaunchCompileRequest,
        StartupWarmLaunchpadPayloads, compile_native_launch, lookup_fee_recipient, quote_launch,
        warm_launchpads_for_startup,
    },
    launchpad_warm::{
        LaunchpadWarmBuildReport, build_launchpad_warm_context, launchpad_warm_env_snapshot,
    },
    launchpads::launchpad_registry,
    observability::{
        clear_outbound_provider_http_traffic, log_event, new_trace_context, persist_launch_report,
        record_outbound_provider_http_request, rpc_traffic_snapshot,
        update_persisted_launch_report,
    },
    provider_tip::pick_tip_account_for_provider,
    providers::{provider_availability_registry, provider_registry},
    pump_native::{warm_default_lookup_tables, warm_pump_global_state},
    report::{
        BenchmarkMode, ExecutionTimings, FollowJobTimings, LaunchReport,
        build_benchmark_timing_groups, build_report, configured_benchmark_mode, render_report,
        sanitize_execution_timings_for_mode,
    },
    reports_browser::{list_persisted_reports, read_persisted_report_bundle},
    rpc::{
        CompiledTransaction, JitoWarmResult, SendTimingBreakdown, configured_warm_rpc_url,
        confirm_submitted_transactions_for_transport,
        derive_helius_transaction_subscribe_account_required, fetch_current_block_height,
        is_blockhash_valid, prewarm_helius_transaction_subscribe_endpoint,
        prewarm_hellomoon_bundle_endpoint, prewarm_hellomoon_quic_endpoint,
        prewarm_jito_bundle_endpoint, prewarm_rpc_endpoint, prewarm_watch_websocket_endpoint,
        record_warm_rpc_failure, record_warm_rpc_success, refresh_latest_blockhash_cache,
        reserve_warm_rpc_call, simulate_transactions,
        submit_independent_transactions_for_transport, submit_transactions_for_transport,
        warm_rpc_cooldown_remaining_ms, warm_rpc_error_should_cooldown,
    },
    runtime::{
        RuntimeRegistry, RuntimeRequest, RuntimeResponse, fail_worker, heartbeat_worker,
        list_workers, restore_workers, start_worker, stop_worker,
    },
    strategies::strategy_registry,
    transport::{
        TransportPlan, build_transport_plan, configured_enable_helius_transaction_subscribe,
        configured_helius_sender_endpoint, configured_helius_sender_endpoints_for_profile,
        configured_hellomoon_bundle_endpoints_for_profile, configured_hellomoon_mev_protect,
        configured_hellomoon_quic_endpoints_for_profile, configured_jito_bundle_endpoints,
        configured_jito_bundle_endpoints_for_profile, configured_provider_region,
        configured_shared_region, configured_standard_rpc_submit_endpoints,
        configured_watch_endpoints_for_provider, default_endpoint_profile,
        default_endpoint_profile_for_provider, estimate_transaction_count,
        helius_sender_endpoint_override_active, jito_bundle_endpoint_override_active,
        prefers_helius_transaction_subscribe_path, resolved_helius_priority_fee_rpc_url,
        resolved_helius_transaction_subscribe_ws_url,
    },
    ui_bridge::{build_raw_config_from_form, quote_from_form, upload_metadata_from_form},
    ui_config::{
        create_default_persistent_config, read_persistent_config, write_persistent_config,
    },
    vamp::{fetch_imported_token_metadata, import_remote_image_to_library},
    vanity_pool::{
        mark_vanity_reservation_used, preload_vanity_pool, refresh_vanity_pool_with_rpc,
        vanity_pool_status_payload,
    },
    wallet::{
        enrich_wallet_statuses, list_solana_env_wallets, load_solana_wallet_by_env_key,
        public_key_from_secret, read_keypair_bytes, selected_wallet_key_or_default,
        selected_wallet_key_or_default_from_wallets,
    },
    warm_manager::{
        WarmActivityRequest, WarmControlState, WarmLifecycleMode, WarmRouteSelection,
        WarmTargetHealth, WarmTargetStatus, WatchWarmTarget, WatchWarmTransport,
    },
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Multipart, Path as AxumPath, Query, State},
    http::{HeaderMap, Response, StatusCode, header},
    middleware::{self, Next},
    routing::{get, post},
};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared_auth::AuthManager;
use shared_extension_runtime::resource_mode::idle_suspension_enabled as global_idle_suspension_enabled;
use shared_fee_market::{
    AutoFeeActionReport, AutoFeeReport, DEFAULT_AUTO_FEE_HELIUS_PRIORITY_LEVEL,
    DEFAULT_AUTO_FEE_JITO_TIP_PERCENTILE, FeeMarketSnapshot, SharedFeeMarketConfig,
    SharedFeeMarketRuntime, action_priority_estimate, action_tip_estimate,
    apply_auto_fee_estimate_buffer, format_lamports_to_sol_decimal, format_priority_price_note,
    lamports_to_priority_fee_micro_lamports, normalize_helius_priority_level,
    normalize_jito_tip_percentile, parse_auto_fee_cap_lamports, parse_sol_decimal_to_lamports,
    priority_price_micro_lamports_to_sol_equivalent, provider_uses_auto_fee_priority,
    provider_uses_auto_fee_tip, resolve_auto_fee_components_with_total_cap,
    shared_fee_market_status_payload,
};
use std::{
    collections::HashSet,
    fs,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
struct AppState {
    auth: Option<Arc<AuthManager>>,
    runtime: Arc<RuntimeRegistry>,
    warm: Arc<Mutex<WarmControlState>>,
}

#[derive(Debug, Clone)]
enum WarmAttemptResult {
    Success,
    RateLimited(String),
    Error(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WarmRecoveredTarget {
    label: String,
    category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    target: String,
    recovered_error: String,
}

#[derive(Debug, Clone)]
struct WarmTargetAttempt {
    id: String,
    category: String,
    provider: Option<String>,
    label: String,
    target: String,
    result: WarmAttemptResult,
}

#[derive(Debug)]
struct WarmAttemptOutcome {
    attempt: WarmTargetAttempt,
    success: bool,
    error: Option<String>,
}

type WarmProbeFuture = Pin<Box<dyn Future<Output = WarmAttemptOutcome> + Send>>;
type WarmProbeBatchFuture = Pin<Box<dyn Future<Output = Vec<WarmAttemptOutcome>> + Send>>;

struct WarmInFlightGuard {
    warm: Arc<Mutex<WarmControlState>>,
}

impl Drop for WarmInFlightGuard {
    fn drop(&mut self) {
        let mut warm = self
            .warm
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        warm.in_flight_requests = warm.in_flight_requests.saturating_sub(1);
    }
}

fn build_warm_target_id(category: &str, provider: Option<&str>, target: &str) -> String {
    match provider.map(str::trim).filter(|value| !value.is_empty()) {
        Some(provider) => format!("{category}:{provider}:{target}"),
        None => format!("{category}:{target}"),
    }
}

fn build_warm_target_attempt(
    category: &str,
    provider: Option<&str>,
    label: &str,
    target: impl Into<String>,
    result: WarmAttemptResult,
) -> WarmTargetAttempt {
    let target = target.into();
    WarmTargetAttempt {
        id: build_warm_target_id(category, provider, &target),
        category: category.to_string(),
        provider: provider.map(ToString::to_string),
        label: label.to_string(),
        target,
        result,
    }
}

fn warm_attempt_outcome(attempt: WarmTargetAttempt) -> WarmAttemptOutcome {
    let success = matches!(attempt.result, WarmAttemptResult::Success);
    let error = match &attempt.result {
        WarmAttemptResult::Error(message) => {
            Some(format!("{} {}: {}", attempt.label, attempt.target, message))
        }
        _ => None,
    };
    WarmAttemptOutcome {
        attempt,
        success,
        error,
    }
}

fn boxed_warm_probe<F>(
    category: &str,
    provider: Option<&str>,
    label: &str,
    target: impl Into<String>,
    future: F,
) -> WarmProbeFuture
where
    F: Future<Output = Result<(), String>> + Send + 'static,
{
    let category = category.to_string();
    let provider = provider.map(ToString::to_string);
    let label = label.to_string();
    let target = target.into();
    let timeout_ms = configured_warm_probe_timeout_ms();
    Box::pin(async move {
        let result = match tokio::time::timeout(Duration::from_millis(timeout_ms), future).await {
            Ok(Ok(())) => WarmAttemptResult::Success,
            Ok(Err(error)) => WarmAttemptResult::Error(error),
            Err(_) => WarmAttemptResult::Error(format!("timed out after {timeout_ms}ms")),
        };
        warm_attempt_outcome(build_warm_target_attempt(
            &category,
            provider.as_deref(),
            &label,
            target,
            result,
        ))
    })
}

fn boxed_jito_warm_probe(endpoint: crate::transport::JitoBundleEndpoint) -> WarmProbeFuture {
    let timeout_ms = configured_warm_probe_timeout_ms();
    let target = endpoint.name.clone();
    Box::pin(async move {
        let result = match tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            prewarm_jito_bundle_endpoint(&endpoint),
        )
        .await
        {
            Ok(Ok(JitoWarmResult::Warmed)) => WarmAttemptResult::Success,
            Ok(Ok(JitoWarmResult::RateLimited(message))) => WarmAttemptResult::RateLimited(message),
            Ok(Err(error)) => WarmAttemptResult::Error(error),
            Err(_) => WarmAttemptResult::Error(format!("timed out after {timeout_ms}ms")),
        };
        warm_attempt_outcome(build_warm_target_attempt(
            "endpoint",
            Some("jito-bundle"),
            "Jito Bundle",
            target,
            result,
        ))
    })
}

fn boxed_watch_warm_probe(target: WatchWarmTarget) -> WarmProbeBatchFuture {
    let timeout_ms = configured_warm_probe_timeout_ms();
    Box::pin(async move {
        let primary_result = match target.transport {
            WatchWarmTransport::StandardWs => {
                tokio::time::timeout(
                    Duration::from_millis(timeout_ms),
                    prewarm_watch_websocket_endpoint(&target.target),
                )
                .await
            }
            WatchWarmTransport::HeliusTransactionSubscribe => {
                tokio::time::timeout(
                    Duration::from_millis(timeout_ms),
                    prewarm_helius_transaction_subscribe_endpoint(&target.target),
                )
                .await
            }
        };
        let primary_attempt = match primary_result {
            Ok(Ok(())) => WarmAttemptResult::Success,
            Ok(Err(error)) => WarmAttemptResult::Error(error),
            Err(_) => WarmAttemptResult::Error(format!("timed out after {timeout_ms}ms")),
        };
        let mut outcomes = vec![warm_attempt_outcome(build_warm_target_attempt(
            "watch-endpoint",
            None,
            &target.label,
            target.target.clone(),
            primary_attempt.clone(),
        ))];
        if target.transport == WatchWarmTransport::HeliusTransactionSubscribe
            && matches!(primary_attempt, WarmAttemptResult::Error(_))
        {
            if let Some(fallback_endpoint) = target.fallback_target.as_deref() {
                let fallback_result = tokio::time::timeout(
                    Duration::from_millis(timeout_ms),
                    prewarm_watch_websocket_endpoint(fallback_endpoint),
                )
                .await;
                let fallback_attempt = match fallback_result {
                    Ok(Ok(())) => WarmAttemptResult::Success,
                    Ok(Err(error)) => WarmAttemptResult::Error(error),
                    Err(_) => WarmAttemptResult::Error(format!("timed out after {timeout_ms}ms")),
                };
                outcomes.push(warm_attempt_outcome(build_warm_target_attempt(
                    "watch-endpoint",
                    None,
                    "Watcher WS",
                    fallback_endpoint.to_string(),
                    fallback_attempt,
                )));
            }
        }
        outcomes
    })
}

fn continuous_state_warm_probe_futures(
    main_rpc_url: &str,
    warm_rpc_url: &str,
) -> Vec<WarmProbeFuture> {
    let mut futures: Vec<WarmProbeFuture> = Vec::new();
    let warm_rpc_target = warm_rpc_url.to_string();
    let warm_rpc_label = if main_rpc_url.trim() == warm_rpc_url.trim() {
        "Primary RPC"
    } else {
        "Warm RPC"
    };
    if main_rpc_url.trim() != warm_rpc_url.trim()
        && !reserve_warm_rpc_call(&warm_rpc_target, "getVersion")
    {
        let warm_rpc_label = warm_rpc_label.to_string();
        futures.push(Box::pin(async move {
            warm_attempt_outcome(build_warm_target_attempt(
                "state",
                None,
                &warm_rpc_label,
                warm_rpc_target,
                WarmAttemptResult::RateLimited(
                    "optional warm RPC rate limit/cooldown active; skipping Shyft probe"
                        .to_string(),
                ),
            ))
        }));
    } else {
        let main_rpc_url = main_rpc_url.to_string();
        let warm_rpc_label = warm_rpc_label.to_string();
        futures.push(Box::pin(async move {
            let timeout_ms = configured_warm_probe_timeout_ms();
            let result = match tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                prewarm_rpc_endpoint(&warm_rpc_target),
            )
            .await
            {
                Ok(Ok(())) => {
                    if main_rpc_url.trim() != warm_rpc_target.trim() {
                        record_warm_rpc_success(&warm_rpc_target);
                    }
                    WarmAttemptResult::Success
                }
                Ok(Err(error)) => {
                    if main_rpc_url.trim() != warm_rpc_target.trim() {
                        record_warm_rpc_failure(&warm_rpc_target, &error);
                    }
                    if main_rpc_url.trim() != warm_rpc_target.trim()
                        && warm_rpc_error_should_cooldown(&error)
                    {
                        WarmAttemptResult::RateLimited(error)
                    } else {
                        WarmAttemptResult::Error(error)
                    }
                }
                Err(_) => {
                    let error = format!("timed out after {timeout_ms}ms");
                    if main_rpc_url.trim() != warm_rpc_target.trim() {
                        record_warm_rpc_failure(&warm_rpc_target, &error);
                        WarmAttemptResult::RateLimited(error)
                    } else {
                        WarmAttemptResult::Error(error)
                    }
                }
            };
            warm_attempt_outcome(build_warm_target_attempt(
                "state",
                None,
                &warm_rpc_label,
                warm_rpc_target,
                result,
            ))
        }));
    }
    if should_prewarm_primary_rpc_separately(main_rpc_url, warm_rpc_url) {
        let primary_target = main_rpc_url.to_string();
        futures.push(boxed_warm_probe(
            "state",
            None,
            "Primary RPC",
            primary_target.clone(),
            async move { prewarm_rpc_endpoint(&primary_target).await },
        ));
    }
    let fee_market_target = main_rpc_url.to_string();
    futures.push(boxed_warm_probe(
        "state",
        None,
        "Fee Market",
        fee_market_target.clone(),
        async move {
            fetch_fee_market_snapshot(&fee_market_target)
                .await
                .map(|_| ())
        },
    ));
    futures
}

fn endpoint_warm_probe_futures(
    routes: &[WarmRouteSelection],
    main_rpc_url: &str,
) -> Vec<WarmProbeFuture> {
    let mut futures = Vec::new();
    if routes.iter().any(|route| route.provider == "standard-rpc") {
        for endpoint in configured_standard_rpc_warm_endpoints(main_rpc_url) {
            let endpoint_target = endpoint.clone();
            futures.push(boxed_warm_probe(
                "endpoint",
                Some("standard-rpc"),
                "Standard RPC",
                endpoint,
                async move { prewarm_rpc_endpoint(&endpoint_target).await },
            ));
        }
    }
    if routes.iter().any(|route| route.provider == "helius-sender") {
        let mut seen = HashSet::new();
        for route in routes
            .iter()
            .filter(|route| route.provider == "helius-sender")
        {
            for endpoint in configured_helius_sender_endpoints_for_profile(&route.endpoint_profile)
            {
                if !seen.insert(endpoint.clone()) {
                    continue;
                }
                let endpoint_target = endpoint.clone();
                futures.push(boxed_warm_probe(
                    "endpoint",
                    Some("helius-sender"),
                    "Helius Sender",
                    endpoint,
                    async move { prewarm_helius_sender_endpoint(&endpoint_target).await },
                ));
            }
        }
    }
    if routes.iter().any(|route| route.provider == "hellomoon") {
        let mut seen = HashSet::new();
        let mev_protect = configured_hellomoon_mev_protect();
        for route in routes.iter().filter(|route| route.provider == "hellomoon") {
            let endpoints = if route.hellomoon_mev_mode == "secure" {
                configured_hellomoon_bundle_endpoints_for_profile(&route.endpoint_profile)
            } else {
                configured_hellomoon_quic_endpoints_for_profile(&route.endpoint_profile)
            };
            for endpoint in endpoints {
                if !seen.insert(endpoint.clone()) {
                    continue;
                }
                let endpoint_target = endpoint.clone();
                if route.hellomoon_mev_mode == "secure" {
                    futures.push(boxed_warm_probe(
                        "endpoint",
                        Some("hellomoon"),
                        "Hello Moon Bundle",
                        endpoint,
                        async move { prewarm_hellomoon_bundle_endpoint(&endpoint_target).await },
                    ));
                } else {
                    futures.push(boxed_warm_probe(
                        "endpoint",
                        Some("hellomoon"),
                        "Hello Moon QUIC",
                        endpoint,
                        async move {
                            prewarm_hellomoon_quic_endpoint(&endpoint_target, mev_protect).await
                        },
                    ));
                }
            }
        }
    }
    if routes.iter().any(|route| route.provider == "jito-bundle") {
        let mut seen = HashSet::new();
        for route in routes
            .iter()
            .filter(|route| route.provider == "jito-bundle")
        {
            for endpoint in configured_jito_bundle_endpoints_for_profile(&route.endpoint_profile) {
                if !seen.insert(endpoint.send.clone()) {
                    continue;
                }
                futures.push(boxed_jito_warm_probe(endpoint));
            }
        }
    }
    futures
}

fn watch_warm_probe_futures(routes: &[WarmRouteSelection]) -> Vec<WarmProbeBatchFuture> {
    configured_watch_warm_targets(routes)
        .into_iter()
        .map(boxed_watch_warm_probe)
        .collect()
}

fn set_all_warm_targets_inactive(warm: &mut WarmControlState) {
    for target in warm.warm_targets.values_mut() {
        target.active = false;
    }
}

fn apply_warm_target_attempts(
    warm: &mut WarmControlState,
    attempts: &[WarmTargetAttempt],
    attempt_at_ms: u64,
) -> Vec<WarmRecoveredTarget> {
    let active_ids = attempts
        .iter()
        .map(|attempt| attempt.id.clone())
        .collect::<HashSet<_>>();
    let mut recovered_targets = Vec::new();
    for target in warm.warm_targets.values_mut() {
        target.active = active_ids.contains(&target.id);
    }
    for attempt in attempts {
        let entry = warm
            .warm_targets
            .entry(attempt.id.clone())
            .or_insert_with(|| WarmTargetStatus {
                id: attempt.id.clone(),
                category: attempt.category.clone(),
                provider: attempt.provider.clone(),
                label: attempt.label.clone(),
                target: attempt.target.clone(),
                active: true,
                last_attempt_at_ms: None,
                status: WarmTargetHealth::Error,
                last_success_at_ms: None,
                last_rate_limited_at_ms: None,
                last_rate_limit_message: None,
                last_error: None,
                last_error_at_ms: None,
                last_recovered_at_ms: None,
                last_recovered_error: None,
                consecutive_failures: 0,
            });
        let previous_status = entry.status.clone();
        let previous_error = entry.last_error.clone();
        let previous_rate_limit_message = entry.last_rate_limit_message.clone();
        entry.category = attempt.category.clone();
        entry.provider = attempt.provider.clone();
        entry.label = attempt.label.clone();
        entry.target = attempt.target.clone();
        entry.active = true;
        entry.last_attempt_at_ms = Some(attempt_at_ms);
        match &attempt.result {
            WarmAttemptResult::Success => {
                entry.status = WarmTargetHealth::Healthy;
                entry.last_success_at_ms = Some(attempt_at_ms);
                entry.last_rate_limited_at_ms = None;
                entry.last_rate_limit_message = None;
                entry.last_error_at_ms = None;
                if previous_error
                    .as_deref()
                    .is_some_and(|error| !error.trim().is_empty())
                {
                    entry.last_recovered_at_ms = Some(attempt_at_ms);
                    entry.last_recovered_error = previous_error.clone();
                    recovered_targets.push(WarmRecoveredTarget {
                        label: entry.label.clone(),
                        category: entry.category.clone(),
                        provider: entry.provider.clone(),
                        target: entry.target.clone(),
                        recovered_error: previous_error.unwrap_or_default(),
                    });
                }
                entry.last_error = None;
                entry.consecutive_failures = 0;
            }
            WarmAttemptResult::RateLimited(message) => {
                entry.status = WarmTargetHealth::RateLimited;
                entry.last_success_at_ms = Some(attempt_at_ms);
                entry.last_rate_limited_at_ms = Some(attempt_at_ms);
                entry.last_rate_limit_message = Some(message.clone());
                entry.last_error_at_ms = None;
                entry.last_error = None;
                entry.consecutive_failures = 0;
                if previous_status != WarmTargetHealth::RateLimited
                    || previous_rate_limit_message.as_deref() != Some(message.as_str())
                {
                    record_warn(
                        "warm",
                        format!("{} warm target rate-limited", entry.label),
                        Some(json!({
                            "provider": entry.provider,
                            "target": entry.target,
                            "category": entry.category,
                            "status": "rate-limited",
                            "message": message,
                        })),
                    );
                }
            }
            WarmAttemptResult::Error(error) => {
                entry.status = WarmTargetHealth::Error;
                entry.last_rate_limited_at_ms = None;
                entry.last_rate_limit_message = None;
                entry.last_error = Some(error.clone());
                entry.last_error_at_ms = Some(attempt_at_ms);
                entry.last_recovered_at_ms = None;
                entry.last_recovered_error = None;
                entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
                if previous_status != WarmTargetHealth::Error
                    || previous_error.as_deref() != Some(error.as_str())
                {
                    record_error(
                        "warm",
                        format!("{} warm target failed", entry.label),
                        Some(json!({
                            "provider": entry.provider,
                            "target": entry.target,
                            "category": entry.category,
                            "status": "error",
                            "message": error,
                            "consecutiveFailures": entry.consecutive_failures,
                        })),
                    );
                }
            }
        }
    }
    prune_stale_warm_targets_after_attempt(warm, &active_ids, attempt_at_ms);
    recovered_targets
}

fn record_warm_recovery_summary(scope: &str, recovered_targets: &[WarmRecoveredTarget]) {
    if recovered_targets.is_empty() {
        return;
    }
    let labels = recovered_targets
        .iter()
        .map(|target| target.label.clone())
        .collect::<Vec<_>>();
    let preview = labels
        .iter()
        .take(4)
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ");
    let extra = labels.len().saturating_sub(4);
    let summary = if extra > 0 {
        format!("{preview} | +{extra} more")
    } else {
        preview
    };
    record_info(
        "warm",
        format!(
            "{} recovered {} warm target{}",
            scope,
            recovered_targets.len(),
            if recovered_targets.len() == 1 {
                ""
            } else {
                "s"
            }
        ),
        Some(json!({
            "scope": scope,
            "status": "healthy",
            "count": recovered_targets.len(),
            "summary": summary,
            "targets": recovered_targets,
        })),
    );
}

fn record_startup_warm_attempts(
    warm: &mut WarmControlState,
    attempts: &[WarmTargetAttempt],
    attempt_at_ms: u64,
) {
    let recovered_targets = apply_warm_target_attempts(warm, attempts, attempt_at_ms);
    record_warm_recovery_summary("startup warm", &recovered_targets);
    warm.last_warm_attempt_at_ms = Some(u128::from(attempt_at_ms));
    if attempts
        .iter()
        .any(|attempt| matches!(attempt.result, WarmAttemptResult::Success))
    {
        let attempt_at_ms_u128 = u128::from(attempt_at_ms);
        warm.last_warm_success_at_ms = Some(attempt_at_ms_u128);
        warm.last_activity_at_ms = attempt_at_ms_u128;
        warm.browser_active = true;
        warm.continuous_active = true;
        warm.current_reason = "active-operator-activity".to_string();
        warm.last_error = None;
    } else {
        warm.last_error = Some("startup warm failed".to_string());
    }
}

fn collect_warm_targets(warm: &WarmControlState, category: &str) -> Vec<WarmTargetStatus> {
    let mut targets = warm
        .warm_targets
        .values()
        .filter(|target| target.category == category)
        .cloned()
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then(left.provider.cmp(&right.provider))
            .then(left.label.cmp(&right.label))
            .then(left.target.cmp(&right.target))
    });
    targets
}

fn payload_warm_targets(
    warm: &WarmControlState,
    category: &str,
    active: bool,
) -> Vec<WarmTargetStatus> {
    let mut targets = collect_warm_targets(warm, category);
    if !active {
        for target in &mut targets {
            target.active = false;
        }
    }
    targets
}

fn effective_suspend_at_ms(
    warm: &WarmControlState,
    active: bool,
    reason: &str,
    now_ms: u128,
) -> Option<u64> {
    if let Some(value) = warm.last_suspend_at_ms {
        return Some(value as u64);
    }
    if active || reason != "suspended-idle" || warm.last_activity_at_ms == 0 {
        return None;
    }
    let idle_timeout_ms = u128::from(configured_idle_warm_timeout_ms());
    let suspended_at_ms = warm
        .last_activity_at_ms
        .saturating_add(idle_timeout_ms)
        .min(now_ms);
    Some(suspended_at_ms.min(u128::from(u64::MAX)) as u64)
}

/// Remove warm-target rows not included in the latest pass once their last attempt is older than this.
const WARM_TARGET_STALE_RETENTION_MS: u64 = 60 * 60 * 1000;
const RECENT_WARM_SUCCESS_REUSE_MS: u64 = 30 * 1000;

fn prune_stale_warm_targets_after_attempt(
    warm: &mut WarmControlState,
    current_attempt_ids: &HashSet<String>,
    now_ms: u64,
) {
    warm.warm_targets.retain(|id, target| {
        if current_attempt_ids.contains(id) {
            return true;
        }
        match target.last_attempt_at_ms {
            None => false,
            Some(last_attempt_ms) => {
                now_ms.saturating_sub(last_attempt_ms) <= WARM_TARGET_STALE_RETENTION_MS
            }
        }
    });
}

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    service: &'static str,
    version: &'static str,
    mode: &'static str,
}

#[derive(Deserialize, Default)]
struct EngineRequest {
    action: Option<String>,
    form: Option<Value>,
    #[serde(rename = "rawConfig")]
    raw_config: Option<Value>,
    #[serde(default)]
    #[serde(rename = "prepareRequestPayloadMs")]
    prepare_request_payload_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
struct StatusRequest {
    wallet: Option<String>,
}

#[derive(Deserialize, Default)]
struct StatusQuery {
    wallet: Option<String>,
}

#[derive(Deserialize, Default)]
struct RunRequest {
    action: Option<String>,
    form: Option<Value>,
    #[serde(default)]
    #[serde(rename = "clientPreRequestMs")]
    client_pre_request_ms: Option<u64>,
    #[serde(default)]
    #[serde(rename = "prepareRequestPayloadMs")]
    prepare_request_payload_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
struct WarmPresenceRequest {
    #[serde(default)]
    active: bool,
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
struct FollowCancelApiRequest {
    #[serde(rename = "traceId")]
    trace_id: String,
    #[serde(rename = "actionId")]
    action_id: Option<String>,
    note: Option<String>,
}

#[derive(Deserialize, Default)]
struct FollowStopAllApiRequest {
    note: Option<String>,
}

#[derive(Deserialize, Default)]
struct MetadataUploadRequest {
    form: Option<Value>,
}

#[derive(Deserialize, Default)]
struct SettingsSaveRequest {
    config: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BagsStoredCredentials {
    #[serde(default)]
    #[serde(rename = "apiKey")]
    api_key: String,
    #[serde(default)]
    #[serde(rename = "authToken")]
    auth_token: String,
    #[serde(default)]
    #[serde(rename = "agentUsername")]
    agent_username: String,
    #[serde(default)]
    #[serde(rename = "verifiedWallet")]
    verified_wallet: String,
}

#[derive(Deserialize, Default)]
struct BagsIdentityInitRequest {
    #[serde(default)]
    #[serde(rename = "apiKey")]
    api_key: String,
    #[serde(default)]
    #[serde(rename = "saveApiKey")]
    save_api_key: bool,
    #[serde(default)]
    #[serde(rename = "agentUsername")]
    agent_username: String,
}

#[derive(Deserialize, Default)]
struct BagsIdentityVerifyRequest {
    #[serde(default)]
    #[serde(rename = "apiKey")]
    api_key: String,
    #[serde(default)]
    #[serde(rename = "saveApiKey")]
    save_api_key: bool,
    #[serde(default)]
    #[serde(rename = "agentUsername")]
    agent_username: String,
    #[serde(default)]
    #[serde(rename = "publicIdentifier")]
    public_identifier: String,
    #[serde(default)]
    secret: String,
    #[serde(default)]
    #[serde(rename = "postId")]
    post_id: String,
    #[serde(default)]
    #[serde(rename = "walletEnvKey")]
    wallet_env_key: String,
}

#[derive(Deserialize, Default)]
struct BagsIdentityStatusQuery {
    wallet: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Default)]
struct BagsFeeRecipientLookupQuery {
    provider: Option<String>,
    username: Option<String>,
    #[serde(rename = "githubUserId")]
    github_user_id: Option<String>,
}

#[derive(Deserialize, Default)]
struct ReportsQuery {
    sort: Option<String>,
}

#[derive(Deserialize, Default)]
struct LogsQuery {
    view: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize, Default)]
struct ReportViewQuery {
    id: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Default)]
struct QuoteQuery {
    launchpad: Option<String>,
    quoteAsset: Option<String>,
    launchMode: Option<String>,
    mode: Option<String>,
    amount: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Default)]
struct VampRequest {
    contractAddress: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Default)]
struct VanityValidateRequest {
    privateKey: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Default)]
struct ImagesQuery {
    search: Option<String>,
    category: Option<String>,
    favoritesOnly: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Default)]
struct ImageUpdateRequest {
    id: Option<String>,
    name: Option<String>,
    tags: Option<Value>,
    category: Option<String>,
    isFavorite: Option<bool>,
}

#[derive(Deserialize, Default)]
struct ImageCategoryRequest {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
struct ImageDeleteRequest {
    id: Option<String>,
}

const USD1_MINT: &str = "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB";

fn configured_engine_port() -> u16 {
    std::env::var("LAUNCHDECK_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8789)
}

fn configured_runtime_state_path() -> std::path::PathBuf {
    paths::runtime_state_path()
}

fn extract_request_auth_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn authorize(headers: &HeaderMap, state: &AppState) -> Result<(), (StatusCode, Json<Value>)> {
    let Some(auth) = &state.auth else {
        return Ok(());
    };
    let token = extract_request_auth_token(headers).ok_or((
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "ok": false,
            "error": "Missing bearer token.",
        })),
    ))?;
    auth.verify_token(token).map_err(|error| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    Ok(())
}

async fn require_authorized_api_request(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
    next: Next,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    authorize(request.headers(), &state)?;
    Ok(next.run(request).await)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "launchdeck-engine",
        version: std::env!("CARGO_PKG_VERSION"),
        mode: "rust-native-only",
    })
}

fn configured_rpc_url() -> String {
    if let Ok(explicit) = std::env::var("SOLANA_RPC_URL") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "http://127.0.0.1:8899".to_string()
}

fn configured_startup_warm_enabled() -> bool {
    if let Ok(value) = std::env::var("LAUNCHDECK_ENABLE_STARTUP_WARM") {
        return parse_env_bool_flag(&value, true);
    }
    !parse_env_bool_flag(
        &std::env::var("LAUNCHDECK_DISABLE_STARTUP_WARM").unwrap_or_default(),
        false,
    )
}

fn parse_env_bool_flag(value: &str, default: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn configured_continuous_warm_enabled() -> bool {
    parse_env_bool_flag(
        &std::env::var("LAUNCHDECK_ENABLE_CONTINUOUS_WARM").unwrap_or_default(),
        true,
    )
}

fn configured_idle_warm_suspend_enabled() -> bool {
    if !global_idle_suspension_enabled() {
        return false;
    }
    parse_env_bool_flag(
        &std::env::var("LAUNCHDECK_ENABLE_IDLE_WARM_SUSPEND").unwrap_or_default(),
        true,
    )
}

fn configured_continuous_warm_interval_ms() -> u64 {
    std::env::var("LAUNCHDECK_CONTINUOUS_WARM_INTERVAL_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value >= 1_000)
        .unwrap_or(50_000)
}

fn configured_continuous_warm_pass_timeout_ms() -> u64 {
    std::env::var("LAUNCHDECK_CONTINUOUS_WARM_PASS_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value >= 10_000)
        .unwrap_or(120_000)
}

fn configured_warm_probe_timeout_ms() -> u64 {
    std::env::var("LAUNCHDECK_WARM_PROBE_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value >= 1_000)
        .unwrap_or(5_000)
}

fn configured_idle_warm_timeout_ms() -> u64 {
    std::env::var("LAUNCHDECK_IDLE_WARM_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value >= 1_000)
        .unwrap_or(75_000)
}

fn push_unique_string(values: &mut Vec<String>, candidate: &str) {
    let normalized = candidate.trim().to_ascii_lowercase();
    if normalized.is_empty() || values.iter().any(|value| value == &normalized) {
        return;
    }
    values.push(normalized);
}

fn normalize_warm_provider(provider: &str) -> Option<String> {
    let normalized = provider.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "helius-sender" | "hellomoon" | "standard-rpc" | "jito-bundle" => Some(normalized),
        _ => None,
    }
}

fn normalize_warm_endpoint_profile(provider: &str, endpoint_profile: &str) -> String {
    let normalized_provider = match normalize_warm_provider(provider) {
        Some(value) => value,
        None => return String::new(),
    };
    if normalized_provider == "standard-rpc" {
        return String::new();
    }
    let trimmed = endpoint_profile.trim();
    if trimmed.is_empty() {
        return default_endpoint_profile_for_provider(&normalized_provider);
    }
    crate::endpoint_profile::parse_config_endpoint_profile(trimmed)
        .unwrap_or_else(|_| default_endpoint_profile_for_provider(&normalized_provider))
}

fn normalize_warm_hellomoon_mev_mode(provider: &str, mev_mode: &str) -> String {
    if provider != "hellomoon" {
        return String::new();
    }
    if mev_mode.trim().eq_ignore_ascii_case("secure") {
        "secure".to_string()
    } else {
        String::new()
    }
}

fn push_unique_warm_route(
    values: &mut Vec<WarmRouteSelection>,
    provider: &str,
    endpoint_profile: &str,
    mev_mode: &str,
) {
    let Some(provider) = normalize_warm_provider(provider) else {
        return;
    };
    let endpoint_profile = normalize_warm_endpoint_profile(&provider, endpoint_profile);
    let hellomoon_mev_mode = normalize_warm_hellomoon_mev_mode(&provider, mev_mode);
    if values.iter().any(|entry| {
        entry.provider == provider
            && entry.endpoint_profile == endpoint_profile
            && entry.hellomoon_mev_mode == hellomoon_mev_mode
    }) {
        return;
    }
    values.push(WarmRouteSelection {
        provider,
        endpoint_profile,
        hellomoon_mev_mode,
    });
}

fn configured_active_warm_routes() -> Vec<WarmRouteSelection> {
    let config = read_persistent_config();
    let active_preset_id = config
        .get("defaults")
        .and_then(|value| value.get("activePresetId"))
        .and_then(Value::as_str)
        .unwrap_or("preset1");
    let preset = config
        .get("presets")
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(active_preset_id))
                .or_else(|| items.first())
        });
    let mut routes = Vec::new();
    for key in ["creationSettings", "buySettings", "sellSettings"] {
        let settings = preset.and_then(|value| value.get(key));
        let provider = settings
            .and_then(|value| value.get("provider"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let endpoint_profile = settings
            .and_then(|value| value.get("endpointProfile"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mev_mode = settings
            .and_then(|value| value.get("mevMode"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        push_unique_warm_route(&mut routes, provider, endpoint_profile, mev_mode);
    }
    if routes.is_empty() {
        push_unique_warm_route(&mut routes, "helius-sender", "", "");
    }
    routes
}

fn merged_warm_routes(selected: &[WarmRouteSelection]) -> Vec<WarmRouteSelection> {
    let mut routes = configured_active_warm_routes();
    for route in selected {
        push_unique_warm_route(
            &mut routes,
            &route.provider,
            &route.endpoint_profile,
            &route.hellomoon_mev_mode,
        );
    }
    routes
}

fn warm_routes_for_execution(
    execution: &crate::config::NormalizedExecution,
) -> Vec<WarmRouteSelection> {
    let mut routes = Vec::new();
    for (provider, endpoint_profile, mev_mode) in [
        (
            execution.provider.as_str(),
            execution.endpointProfile.as_str(),
            execution.mevMode.as_str(),
        ),
        (
            execution.buyProvider.as_str(),
            execution.buyEndpointProfile.as_str(),
            execution.buyMevMode.as_str(),
        ),
        (
            execution.sellProvider.as_str(),
            execution.sellEndpointProfile.as_str(),
            execution.sellMevMode.as_str(),
        ),
    ] {
        push_unique_warm_route(&mut routes, provider, endpoint_profile, mev_mode);
    }
    routes
}

fn warm_routes_from_normalized_config_value(value: &Value) -> Vec<WarmRouteSelection> {
    let execution = value.get("execution");
    let mut routes = Vec::new();
    for (provider_key, endpoint_profile_key, mev_mode_key) in [
        ("provider", "endpointProfile", "mevMode"),
        ("buyProvider", "buyEndpointProfile", "buyMevMode"),
        ("sellProvider", "sellEndpointProfile", "sellMevMode"),
    ] {
        let provider = execution
            .and_then(|entry| entry.get(provider_key))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let endpoint_profile = execution
            .and_then(|entry| entry.get(endpoint_profile_key))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mev_mode = execution
            .and_then(|entry| entry.get(mev_mode_key))
            .and_then(Value::as_str)
            .unwrap_or_default();
        push_unique_warm_route(&mut routes, provider, endpoint_profile, mev_mode);
    }
    routes
}

fn effective_warm_routes(warm: &WarmControlState) -> Vec<WarmRouteSelection> {
    let mut routes = merged_warm_routes(&warm.selected_routes);
    for route in &warm.follow_job_routes {
        push_unique_warm_route(
            &mut routes,
            &route.provider,
            &route.endpoint_profile,
            &route.hellomoon_mev_mode,
        );
    }
    routes
}

fn warm_route_providers(routes: &[WarmRouteSelection]) -> Vec<String> {
    let mut providers = Vec::new();
    for route in routes {
        push_unique_string(&mut providers, &route.provider);
    }
    providers
}

fn activity_routes(payload: &WarmActivityRequest) -> Vec<WarmRouteSelection> {
    let mut routes = Vec::new();
    for (provider, endpoint_profile, mev_mode) in [
        (
            payload.creation_provider.as_deref(),
            payload.creation_endpoint_profile.as_deref(),
            payload.creation_mev_mode.as_deref(),
        ),
        (
            payload.buy_provider.as_deref(),
            payload.buy_endpoint_profile.as_deref(),
            payload.buy_mev_mode.as_deref(),
        ),
        (
            payload.sell_provider.as_deref(),
            payload.sell_endpoint_profile.as_deref(),
            payload.sell_mev_mode.as_deref(),
        ),
    ] {
        if let Some(provider) = provider {
            push_unique_warm_route(
                &mut routes,
                provider,
                endpoint_profile.unwrap_or_default(),
                mev_mode.unwrap_or_default(),
            );
        }
    }
    routes
}

fn startup_warm_routes(payload: &WarmActivityRequest) -> Vec<WarmRouteSelection> {
    let routes = activity_routes(payload);
    if routes.is_empty() {
        configured_active_warm_routes()
    } else {
        routes
    }
}

fn configured_standard_rpc_warm_endpoints(main_rpc_url: &str) -> Vec<String> {
    let mut endpoints = configured_standard_rpc_submit_endpoints();
    if endpoints.is_empty() {
        endpoints.push(main_rpc_url.to_string());
    }
    endpoints
}

fn should_prewarm_primary_rpc_separately(main_rpc_url: &str, warm_rpc_url: &str) -> bool {
    let _ = (main_rpc_url, warm_rpc_url);
    false
}

fn configured_watch_warm_targets(routes: &[WarmRouteSelection]) -> Vec<WatchWarmTarget> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    let helius_transaction_subscribe_enabled = configured_enable_helius_transaction_subscribe();
    for route in routes {
        for endpoint in
            configured_watch_endpoints_for_provider(&route.provider, &route.endpoint_profile)
        {
            let trimmed = endpoint.trim();
            if trimmed.is_empty() {
                continue;
            }
            if prefers_helius_transaction_subscribe_path(
                helius_transaction_subscribe_enabled,
                Some(trimmed),
            ) {
                if let Some(helius_endpoint) =
                    resolved_helius_transaction_subscribe_ws_url(Some(trimmed))
                {
                    let key = format!("helius-transaction-subscribe:{helius_endpoint}");
                    if seen.insert(key) {
                        targets.push(WatchWarmTarget {
                            label: "Helius transactionSubscribe WS".to_string(),
                            target: helius_endpoint,
                            transport: WatchWarmTransport::HeliusTransactionSubscribe,
                            fallback_target: Some(trimmed.to_string()),
                        });
                    }
                    continue;
                }
            }
            let key = format!("standard-ws:{trimmed}");
            if seen.insert(key) {
                targets.push(WatchWarmTarget {
                    label: "Watcher WS".to_string(),
                    target: trimmed.to_string(),
                    transport: WatchWarmTransport::StandardWs,
                    fallback_target: None,
                });
            }
        }
    }
    targets
}

fn warm_succeeded_recently(warm: &WarmControlState, now_ms: u128) -> bool {
    warm.last_error.is_none()
        && warm
            .last_warm_success_at_ms
            .is_some_and(|last_success_at_ms| {
                now_ms.saturating_sub(last_success_at_ms)
                    <= u128::from(RECENT_WARM_SUCCESS_REUSE_MS)
            })
}

fn warm_targets_need_resume_pass(warm: &WarmControlState) -> bool {
    !warm.warm_targets.is_empty() && warm.warm_targets.values().all(|target| !target.active)
}

fn warm_pass_in_flight_is_stale(warm: &WarmControlState, now_ms: u128) -> bool {
    warm.warm_pass_in_flight
        && warm.last_warm_attempt_at_ms.is_some_and(|started_at_ms| {
            now_ms.saturating_sub(started_at_ms)
                > u128::from(configured_continuous_warm_pass_timeout_ms())
        })
}

fn mark_operator_activity(state: &Arc<AppState>, routes: Vec<WarmRouteSelection>) -> bool {
    let now = current_time_ms();
    let mut warm = state
        .warm
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let route_matches_current = routes.is_empty() || routes == warm.selected_routes;
    let warm_is_active = warm_gate_state(&warm, now, 0).0;
    let should_trigger_immediate_rewarm = if !route_matches_current {
        true
    } else if !warm_is_active && warm_targets_need_resume_pass(&warm) {
        true
    } else {
        !warm_is_active && !warm_succeeded_recently(&warm, now)
    };
    warm.last_activity_at_ms = now;
    warm.browser_active = true;
    if should_trigger_immediate_rewarm {
        warm.last_resume_at_ms = Some(now);
        warm.last_suspend_at_ms = None;
        warm.continuous_active = true;
    }
    warm.current_reason = "active-operator-activity".to_string();
    if !routes.is_empty() {
        warm.selected_routes = routes;
    }
    should_trigger_immediate_rewarm
}

fn mark_browser_presence(state: &Arc<AppState>, active: bool, reason: &str) {
    let now = current_time_ms();
    let mut warm = state
        .warm
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !active && !configured_idle_warm_suspend_enabled() {
        warm.browser_active = true;
        warm.continuous_active = true;
        warm.current_reason = "active-always-on-resource-mode".to_string();
        warm.last_resume_at_ms.get_or_insert(now);
        warm.last_suspend_at_ms = None;
        return;
    }
    if active {
        warm.last_activity_at_ms = now;
        warm.browser_active = true;
        warm.current_reason = if reason.trim().is_empty() {
            "active-browser-presence".to_string()
        } else {
            reason.trim().to_string()
        };
        warm.last_resume_at_ms = Some(now);
        warm.last_suspend_at_ms = None;
        return;
    }
    warm.browser_active = false;
    warm.continuous_active = false;
    warm.last_suspend_at_ms = Some(now);
    warm.current_reason = if reason.trim().is_empty() {
        "inactive-browser-presence".to_string()
    } else {
        reason.trim().to_string()
    };
    set_all_warm_targets_inactive(&mut warm);
}

fn mark_in_flight_engine_request(state: &Arc<AppState>) -> WarmInFlightGuard {
    let now = current_time_ms();
    let mut warm = state
        .warm
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    warm.in_flight_requests = warm.in_flight_requests.saturating_add(1);
    warm.last_activity_at_ms = now;
    warm.browser_active = true;
    WarmInFlightGuard {
        warm: state.warm.clone(),
    }
}

fn warm_gate_state(
    warm: &WarmControlState,
    now_ms: u128,
    follow_active_jobs: u64,
) -> (bool, bool, String) {
    if !configured_continuous_warm_enabled() {
        return (false, false, "disabled-by-env".to_string());
    }
    if warm.in_flight_requests > 0 {
        return (true, true, "active-in-flight-request".to_string());
    }
    if follow_active_jobs > 0 {
        return (true, true, "active-follow-jobs".to_string());
    }
    if !configured_idle_warm_suspend_enabled() {
        return (
            true,
            warm.last_activity_at_ms > 0,
            "active-continuous-warm".to_string(),
        );
    }
    if warm.last_activity_at_ms == 0 {
        return (false, false, "idle-awaiting-browser-activity".to_string());
    }
    let idle_timeout_ms = u128::from(configured_idle_warm_timeout_ms());
    let idle_for_ms = now_ms.saturating_sub(warm.last_activity_at_ms);
    if idle_for_ms <= idle_timeout_ms {
        return (true, true, "active-operator-activity".to_string());
    }
    (false, false, "suspended-idle".to_string())
}

const IDLE_BACKGROUND_REQUEST_POLL_INTERVAL: Duration = Duration::from_secs(1);
const ENGINE_BLOCKHASH_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const VANITY_POOL_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

fn background_request_gate_active(warm: &WarmControlState, now_ms: u128) -> bool {
    if !configured_idle_warm_suspend_enabled() {
        return true;
    }
    if warm.in_flight_requests > 0 {
        return true;
    }
    if warm.follow_jobs_active {
        return true;
    }
    if warm.last_activity_at_ms == 0 {
        return false;
    }
    now_ms.saturating_sub(warm.last_activity_at_ms) <= u128::from(configured_idle_warm_timeout_ms())
}

fn engine_background_requests_active(state: &Arc<AppState>) -> bool {
    let now_ms = current_time_ms();
    let warm = state
        .warm
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    background_request_gate_active(&warm, now_ms)
}

fn idle_suspended_without_inflight(state: &Arc<AppState>, follow_active_jobs: u64) -> bool {
    let now_ms = current_time_ms();
    let warm = state
        .warm
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (active, _, reason) = warm_gate_state(&warm, now_ms, follow_active_jobs);
    !active && reason == "suspended-idle" && warm.in_flight_requests == 0
}

fn warm_state_payload(state: &Arc<AppState>, follow_active_jobs: u64) -> Value {
    let now_ms = current_time_ms();
    let warm = state
        .warm
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (active, browser_active, reason) = warm_gate_state(&warm, now_ms, follow_active_jobs);
    let mode = if active {
        WarmLifecycleMode::Active
    } else if browser_active || follow_active_jobs > 0 || warm.warm_pass_in_flight {
        WarmLifecycleMode::Maintenance
    } else {
        WarmLifecycleMode::Idle
    };
    let selected_routes = effective_warm_routes(&warm);
    let selected_providers = warm_route_providers(&selected_routes);
    let state_targets = payload_warm_targets(&warm, "state", active);
    let endpoint_targets = payload_warm_targets(&warm, "endpoint", active);
    let watch_targets = payload_warm_targets(&warm, "watch-endpoint", active);
    let main_rpc_url = configured_rpc_url();
    let warm_rpc_url = configured_warm_rpc_url(&main_rpc_url);
    let warm_rpc_configured = warm_rpc_url.trim() != main_rpc_url.trim();
    let warm_rpc_cooldown_ms = if warm_rpc_configured {
        warm_rpc_cooldown_remaining_ms(&warm_rpc_url)
    } else {
        None
    };
    json!({
        "startupEnabled": configured_startup_warm_enabled(),
        "continuousEnabled": configured_continuous_warm_enabled(),
        "idleSuspendEnabled": configured_idle_warm_suspend_enabled(),
        "warmRpcConfigured": warm_rpc_configured,
        "warmRpcCooldownRemainingMs": warm_rpc_cooldown_ms,
        "warmRpcMode": if warm_rpc_configured { "best-effort" } else { "primary" },
        "intervalMs": configured_continuous_warm_interval_ms(),
        "idleTimeoutMs": configured_idle_warm_timeout_ms(),
        "active": active,
        "mode": mode,
        "suspended": configured_continuous_warm_enabled() && !active,
        "browserActive": browser_active,
        "reason": reason,
        "selectedProviders": selected_providers,
        "inFlightRequests": warm.in_flight_requests,
        "followActiveJobs": follow_active_jobs,
        "lastActivityAtMs": if warm.last_activity_at_ms > 0 { Some(warm.last_activity_at_ms as u64) } else { None },
        "idleForMs": if warm.last_activity_at_ms > 0 { Some(now_ms.saturating_sub(warm.last_activity_at_ms) as u64) } else { None },
        "lastResumeAtMs": warm.last_resume_at_ms.map(|value| value as u64),
        "lastSuspendAtMs": effective_suspend_at_ms(&warm, active, &reason, now_ms),
        "passInFlight": warm.warm_pass_in_flight,
        "lastWarmAttemptAtMs": warm.last_warm_attempt_at_ms.map(|value| value as u64),
        "lastWarmSuccessAtMs": warm.last_warm_success_at_ms.map(|value| value as u64),
        "lastError": warm.last_error.clone(),
        "stateTargets": state_targets,
        "endpointTargets": endpoint_targets,
        "watchTargets": watch_targets,
    })
}

async fn execute_continuous_warm_pass(state: &Arc<AppState>) {
    let follow_active_jobs = follow_active_jobs_count().await;
    let (routes, attempt_started_at_ms) = {
        let now_ms = current_time_ms();
        let mut warm = state
            .warm
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        warm.follow_jobs_active = follow_active_jobs > 0;
        let routes = effective_warm_routes(&warm);
        let (should_warm, browser_active, reason) =
            warm_gate_state(&warm, now_ms, follow_active_jobs);
        if should_warm != warm.continuous_active {
            if should_warm {
                warm.last_resume_at_ms = Some(now_ms);
            } else {
                warm.last_suspend_at_ms = Some(now_ms);
            }
        }
        warm.browser_active = browser_active;
        warm.continuous_active = should_warm;
        warm.current_reason = reason;
        if !should_warm {
            set_all_warm_targets_inactive(&mut warm);
            return;
        }
        if warm.warm_pass_in_flight {
            if warm_pass_in_flight_is_stale(&warm, now_ms) {
                warm.warm_pass_in_flight = false;
                warm.last_error = Some(format!(
                    "Continuous warm pass exceeded {}ms. Resetting stale in-flight state.",
                    configured_continuous_warm_pass_timeout_ms()
                ));
            } else {
                return;
            }
        }
        warm.warm_pass_in_flight = true;
        warm.last_warm_attempt_at_ms = Some(now_ms);
        (routes, now_ms as u64)
    };

    let main_rpc_url = configured_rpc_url();
    let warm_rpc_url = configured_warm_rpc_url(&main_rpc_url);
    let (state_outcomes, endpoint_outcomes, watch_outcomes_nested) = tokio::join!(
        join_all(continuous_state_warm_probe_futures(
            &main_rpc_url,
            &warm_rpc_url
        )),
        join_all(endpoint_warm_probe_futures(&routes, &main_rpc_url)),
        join_all(watch_warm_probe_futures(&routes)),
    );
    let mut outcomes = state_outcomes;
    outcomes.extend(endpoint_outcomes);
    for watch_outcomes in watch_outcomes_nested {
        outcomes.extend(watch_outcomes);
    }
    let success_count = outcomes.iter().filter(|outcome| outcome.success).count();
    let errors = outcomes
        .iter()
        .filter_map(|outcome| outcome.error.clone())
        .collect::<Vec<_>>();
    let attempts = outcomes
        .into_iter()
        .map(|outcome| outcome.attempt)
        .collect::<Vec<_>>();

    let now_ms = current_time_ms();
    let mut warm = state
        .warm
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    warm.warm_pass_in_flight = false;
    let recovered_targets = apply_warm_target_attempts(&mut warm, &attempts, attempt_started_at_ms);
    record_warm_recovery_summary("continuous warm", &recovered_targets);
    if success_count > 0 {
        warm.last_warm_success_at_ms = Some(now_ms);
        warm.last_error = None;
    } else if !errors.is_empty() {
        warm.last_error = Some(errors.join(" | "));
    }
}

fn spawn_continuous_warm_task(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let pass_timeout_ms = configured_continuous_warm_pass_timeout_ms();
            if tokio::time::timeout(
                Duration::from_millis(pass_timeout_ms),
                execute_continuous_warm_pass(&state),
            )
            .await
            .is_err()
            {
                let mut warm = state
                    .warm
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                warm.warm_pass_in_flight = false;
                warm.last_error = Some(format!(
                    "Continuous warm pass timed out after {}ms and was reset.",
                    pass_timeout_ms
                ));
                if warm.continuous_active {
                    set_all_warm_targets_inactive(&mut warm);
                }
            }
            tokio::time::sleep(Duration::from_millis(
                configured_continuous_warm_interval_ms(),
            ))
            .await;
        }
    });
}

fn configured_base_url() -> String {
    format!("http://127.0.0.1:{}", configured_engine_port())
}

fn resolve_signer_source(selected_wallet_key: &str) -> String {
    if !selected_wallet_key.trim().is_empty() {
        return format!("env:{}", selected_wallet_key.trim());
    }
    if std::env::var("SOLANA_PRIVATE_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return "env:SOLANA_PRIVATE_KEY".to_string();
    }
    if std::env::var("SOLANA_KEYPAIR_PATH")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return "env:SOLANA_KEYPAIR_PATH".to_string();
    }
    "unknown".to_string()
}

fn guess_content_type(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "avif" => "image/avif",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}

fn cache_control_for_path(path: &std::path::Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "html" {
        return "no-store";
    }
    if path.to_string_lossy().contains("uploads") {
        return "public, max-age=86400";
    }
    if matches!(
        extension.as_str(),
        "js" | "css" | "svg" | "png" | "avif" | "jpg" | "jpeg" | "webp" | "gif"
    ) {
        return "no-cache";
    }
    "no-store"
}

fn file_response(path: std::path::PathBuf) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    let body = std::fs::read(&path).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": "Not found",
            })),
        )
    })?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, guess_content_type(&path))
        .header(header::CACHE_CONTROL, cache_control_for_path(&path))
        .body(Body::from(body))
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "error": error.to_string(),
                })),
            )
        })
}

fn launchdeck_index_response(
    state: &AppState,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    let path = paths::ui_dir().join("launchdeck").join("index.html");
    let mut body = std::fs::read_to_string(&path).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": "Not found",
            })),
        )
    })?;
    if let Some(auth) = &state.auth {
        let token = auth.default_token().map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })?;
        let injected = format!(
            "<script>window.__ldToken = {};</script>",
            serde_json::to_string(&token).unwrap_or_else(|_| "\"\"".to_string())
        );
        if let Some(index) = body.rfind("<script") {
            body.insert_str(index, &injected);
        } else if let Some(index) = body.rfind("</body>") {
            body.insert_str(index, &injected);
        } else {
            body.push_str(&injected);
        }
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, guess_content_type(&path))
        .header(header::CACHE_CONTROL, cache_control_for_path(&path))
        .body(Body::from(body))
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "error": error.to_string(),
                })),
            )
        })
}

async fn serve_launchdeck_index(
    State(state): State<Arc<AppState>>,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    launchdeck_index_response(&state)
}

fn static_not_found() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "ok": false,
            "error": "Not found",
        })),
    )
}

fn safe_ui_relative_path(requested: &str) -> Option<std::path::PathBuf> {
    let trimmed = requested.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Some(std::path::PathBuf::from("index.html"));
    }
    let mut relative = std::path::PathBuf::new();
    for component in std::path::Path::new(trimmed).components() {
        match component {
            std::path::Component::Normal(value) => relative.push(value),
            _ => return None,
        }
    }
    if requested.ends_with('/') {
        relative.push("index.html");
    }
    Some(relative)
}

fn try_file_response(
    path: std::path::PathBuf,
) -> Option<Result<Response<Body>, (StatusCode, Json<Value>)>> {
    if path.is_file() {
        return Some(file_response(path));
    }
    None
}

fn now_timestamp_string() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    millis.to_string()
}

fn current_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn attach_timing(mut payload: Value, started_at_ms: u128) -> Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "timingMs".to_string(),
            Value::from(current_time_ms().saturating_sub(started_at_ms) as u64),
        );
    }
    payload
}

fn synthetic_mint_address(trace_id: &str) -> String {
    let clean = trace_id.replace('-', "");
    let mut bytes = Vec::with_capacity(32);
    while bytes.len() < 32 {
        bytes.extend_from_slice(clean.as_bytes());
    }
    bytes.truncate(32);
    bs58::encode(bytes).into_string()
}

fn render_report_value(report: &Value) -> Value {
    serde_json::from_value::<LaunchReport>(report.clone())
        .map(|parsed| Value::String(render_report(&parsed)))
        .unwrap_or_else(|_| Value::String(String::new()))
}

fn response_metadata_uri(prepared_metadata_uri: Option<String>, report: &Value) -> Option<String> {
    prepared_metadata_uri.or_else(|| {
        report
            .get("metadataUri")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn append_execution_warning(report: &mut Value, warning: &str) {
    let Some(execution) = report.get_mut("execution") else {
        return;
    };
    let mut existing_warnings = execution
        .get("warnings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    existing_warnings.push(Value::String(warning.to_string()));
    execution["warnings"] = Value::Array(existing_warnings);
}

fn append_execution_note(report: &mut Value, note: &str) {
    let Some(execution) = report.get_mut("execution") else {
        return;
    };
    let mut existing_notes = execution
        .get("notes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    existing_notes.push(Value::String(note.to_string()));
    execution["notes"] = Value::Array(existing_notes);
}

fn set_execution_value(report: &mut Value, key: &str, value: Value) {
    let Some(execution) = report.get_mut("execution") else {
        return;
    };
    execution[key] = value;
}

fn set_report_timing(report: &mut Value, key: &str, value_ms: u128) {
    if report_benchmark_mode(report) == BenchmarkMode::Off {
        return;
    }
    if let Some(execution) = report.get_mut("execution") {
        if execution.get("timings").is_none()
            || execution.get("timings").is_some_and(Value::is_null)
        {
            execution["timings"] = json!({});
        }
        execution["timings"][key] = Value::from(value_ms as u64);
    }
}

fn set_optional_report_timing(report: &mut Value, key: &str, value_ms: Option<u128>) {
    if let Some(value_ms) = value_ms {
        set_report_timing(report, key, value_ms);
    }
}

fn set_report_benchmark_mode(report: &mut Value, mode: BenchmarkMode) {
    if let Some(execution) = report.get_mut("execution") {
        if execution.get("timings").is_none()
            || execution.get("timings").is_some_and(Value::is_null)
        {
            execution["timings"] = json!({});
        }
        execution["timings"]["benchmarkMode"] = Value::String(mode.as_str().to_string());
    }
}

fn report_benchmark_mode(report: &Value) -> BenchmarkMode {
    report
        .get("execution")
        .and_then(|value| value.get("timings"))
        .and_then(|timings| timings.get("benchmarkMode"))
        .and_then(Value::as_str)
        .map(BenchmarkMode::from_value)
        .unwrap_or_else(configured_benchmark_mode)
}

fn parse_report_execution_timings(report: &Value) -> ExecutionTimings {
    report
        .get("execution")
        .and_then(|value| value.get("timings"))
        .cloned()
        .and_then(|value| serde_json::from_value::<ExecutionTimings>(value).ok())
        .unwrap_or_default()
}

fn parse_follow_job_timings(report: &Value) -> Option<FollowJobTimings> {
    report
        .get("followDaemon")
        .and_then(|value| value.get("job"))
        .and_then(|job| job.get("timings"))
        .cloned()
        .and_then(|value| serde_json::from_value::<FollowJobTimings>(value).ok())
}

fn refresh_report_benchmark(report: &mut Value) {
    let mode = report_benchmark_mode(report);
    if mode == BenchmarkMode::Off {
        if let Some(execution) = report.get_mut("execution") {
            execution["timings"] = Value::Null;
        }
        report["benchmark"] = json!({
            "mode": mode.as_str(),
            "timings": {},
            "timingGroups": [],
            "sent": [],
        });
        return;
    }
    let timings =
        sanitize_execution_timings_for_mode(&parse_report_execution_timings(report), mode);
    if let Some(execution) = report.get_mut("execution") {
        execution["timings"] = serde_json::to_value(&timings).unwrap_or_else(|_| json!({}));
    }
    let sent_items = report
        .get("execution")
        .and_then(|value| value.get("sent"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let sent = sent_items
        .into_iter()
        .map(|item| {
            let send_slot = item
                .get("sendObservedSlot")
                .or_else(|| item.get("sendObservedBlockHeight"))
                .and_then(Value::as_u64);
            let confirmed_slot = item
                .get("confirmedObservedSlot")
                .or_else(|| item.get("confirmedObservedBlockHeight"))
                .and_then(Value::as_u64);
            json!({
                "label": item.get("label").cloned().unwrap_or_else(|| Value::String("(unknown)".to_string())),
                "signature": item.get("signature").cloned().unwrap_or(Value::Null),
                "confirmationStatus": item.get("confirmationStatus").cloned().unwrap_or(Value::Null),
                "sendSlot": send_slot,
                "confirmedObservedSlot": confirmed_slot,
                "slotsToConfirm": match (send_slot, confirmed_slot) {
                    (Some(send_slot), Some(confirmed_slot)) => Some(confirmed_slot.saturating_sub(send_slot)),
                    _ => None,
                },
                "confirmedSlot": item.get("confirmedSlot").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let timing_groups =
        build_benchmark_timing_groups(&timings, parse_follow_job_timings(report).as_ref());
    report["benchmark"] = json!({
        "mode": mode.as_str(),
        "timings": timings,
        "timingGroups": timing_groups,
        "sent": sent,
    });
}

fn split_same_time_snipes(
    follow_launch: &NormalizedFollowLaunch,
) -> (
    Vec<crate::config::NormalizedFollowLaunchSnipe>,
    NormalizedFollowLaunch,
) {
    let mut same_time = Vec::new();
    let mut deferred = follow_launch.clone();
    deferred.snipes = follow_launch
        .snipes
        .iter()
        .filter_map(|snipe| {
            if !snipe.enabled {
                return None;
            }
            if snipe.submitWithLaunch {
                same_time.push(snipe.clone());
                if snipe.retryOnFailure {
                    let mut retry_snipe = snipe.clone();
                    retry_snipe.submitWithLaunch = false;
                    retry_snipe.submitDelayMs = 450;
                    retry_snipe.targetBlockOffset = None;
                    retry_snipe.skipIfTokenBalancePositive = true;
                    Some(retry_snipe)
                } else {
                    None
                }
            } else {
                Some(snipe.clone())
            }
        })
        .collect::<Vec<_>>();
    deferred.enabled = !deferred.snipes.is_empty()
        || deferred
            .devAutoSell
            .as_ref()
            .is_some_and(|sell| sell.enabled);
    (same_time, deferred)
}

fn has_same_time_snipes(follow_launch: &NormalizedFollowLaunch) -> bool {
    follow_launch
        .snipes
        .iter()
        .any(|snipe| snipe.enabled && snipe.submitWithLaunch)
}

async fn collect_bags_setup_transaction_diagnostics(
    rpc_url: &str,
    commitment: &str,
    transaction: &CompiledTransaction,
    sent: Option<&[crate::rpc::SentResult]>,
) -> String {
    let mut diagnostics = vec![
        format!("label={}", transaction.label),
        format!("format={}", transaction.format),
        format!("blockhash={}", transaction.blockhash),
        format!(
            "localLastValidBlockHeight={}",
            transaction.lastValidBlockHeight
        ),
        format!(
            "computeUnitLimit={}",
            transaction
                .computeUnitLimit
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string())
        ),
        format!(
            "computeUnitPriceMicroLamports={}",
            transaction
                .computeUnitPriceMicroLamports
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string())
        ),
        format!(
            "inlineTipLamports={}",
            transaction
                .inlineTipLamports
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string())
        ),
        format!(
            "inlineTipAccount={}",
            transaction
                .inlineTipAccount
                .clone()
                .unwrap_or_else(|| "null".to_string())
        ),
    ];
    if let Some(signature) = transaction.signature.as_deref() {
        diagnostics.push(format!("signature={signature}"));
    }
    match fetch_current_block_height(rpc_url, commitment).await {
        Ok(value) => diagnostics.push(format!("currentBlockHeight={value}")),
        Err(error) => diagnostics.push(format!("currentBlockHeightError={error}")),
    }
    match is_blockhash_valid(rpc_url, &transaction.blockhash, commitment).await {
        Ok(valid) => diagnostics.push(format!("actualBlockhashValid={valid}")),
        Err(error) => diagnostics.push(format!("actualBlockhashValidError={error}")),
    }
    match simulate_transactions(rpc_url, std::slice::from_ref(transaction), commitment).await {
        Ok((simulation, warnings)) => {
            if let Some(result) = simulation.first() {
                diagnostics.push(format!(
                    "simulationErr={}",
                    result
                        .err
                        .as_ref()
                        .map(Value::to_string)
                        .unwrap_or_else(|| "null".to_string())
                ));
                if let Some(units) = result.unitsConsumed {
                    diagnostics.push(format!("simulationUnitsConsumed={units}"));
                }
                if !result.logs.is_empty() {
                    diagnostics.push(format!(
                        "simulationLogs={}",
                        result
                            .logs
                            .iter()
                            .take(12)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" || ")
                    ));
                }
            }
            if !warnings.is_empty() {
                diagnostics.push(format!("simulationWarnings={}", warnings.join(" || ")));
            }
        }
        Err(error) => diagnostics.push(format!("simulationRequestError={error}")),
    }
    if let Some(sent_results) = sent {
        if let Some(first) = sent_results.first() {
            diagnostics.push(format!("transportType={}", first.transportType));
            diagnostics.push(format!(
                "transportEndpoint={}",
                first.endpoint.clone().unwrap_or_else(|| "null".to_string())
            ));
            if !first.attemptedEndpoints.is_empty() {
                diagnostics.push(format!(
                    "attemptedEndpoints={}",
                    first.attemptedEndpoints.join(",")
                ));
            }
            if let Some(value) = first.sendObservedSlot {
                diagnostics.push(format!("sendObservedSlot={value}"));
            }
        }
    }
    diagnostics.join(" | ")
}

fn actual_helius_sender_endpoint(sent: &[crate::rpc::SentResult]) -> Option<String> {
    sent.iter().find_map(|entry| {
        if entry.transportType != "helius-sender" {
            return None;
        }
        entry
            .endpoint
            .as_ref()
            .filter(|value| !value.is_empty())
            .cloned()
    })
}

fn standard_rpc_transport_plan(base: &TransportPlan, primary_rpc_url: &str) -> TransportPlan {
    let mut plan = base.clone();
    plan.requestedProvider = "standard-rpc".to_string();
    plan.resolvedProvider = "standard-rpc".to_string();
    plan.transportType = "standard-rpc-fanout".to_string();
    plan.executionClass = "sequential".to_string();
    plan.ordering = "sequential".to_string();
    plan.supportsBundle = false;
    plan.requiresInlineTip = false;
    plan.requiresPriorityFee = false;
    plan.separateTipTransaction = false;
    plan.skipPreflight = base.skipPreflight
        || primary_rpc_url
            .trim()
            .to_ascii_lowercase()
            .contains("helius");
    plan.maxRetries = 0;
    plan.helloMoonQuicEndpoint = None;
    plan.helloMoonQuicEndpoints = vec![];
    plan.helloMoonBundleEndpoint = None;
    plan.helloMoonBundleEndpoints = vec![];
    plan.heliusSenderEndpoint = None;
    plan.heliusSenderEndpoints = vec![];
    plan.jitoBundleEndpoints = vec![];
    plan
}

fn build_buy_transport_plan(
    execution: &crate::config::NormalizedExecution,
    transaction_count: usize,
) -> TransportPlan {
    let mut buy_execution = execution.clone();
    buy_execution.provider = execution.buyProvider.clone();
    buy_execution.endpointProfile = execution.buyEndpointProfile.clone();
    buy_execution.mevProtect = execution.buyMevProtect;
    buy_execution.mevMode = execution.buyMevMode.clone();
    buy_execution.jitodontfront = execution.buyJitodontfront;
    buy_execution.autoGas = execution.buyAutoGas;
    buy_execution.autoMode = execution.buyAutoMode.clone();
    buy_execution.priorityFeeSol = execution.buyPriorityFeeSol.clone();
    buy_execution.tipSol = execution.buyTipSol.clone();
    buy_execution.maxPriorityFeeSol = execution.buyMaxPriorityFeeSol.clone();
    buy_execution.maxTipSol = execution.buyMaxTipSol.clone();
    build_transport_plan(&buy_execution, transaction_count)
}

async fn send_transactions_sequential_for_transport(
    rpc_url: &str,
    transport_plan: &TransportPlan,
    transactions: &[CompiledTransaction],
    commitment: &str,
    skip_preflight: bool,
    track_send_block_height: bool,
    confirm_timeout_secs: Option<u64>,
) -> Result<
    (
        Vec<crate::rpc::SentResult>,
        Vec<String>,
        SendTimingBreakdown,
    ),
    String,
> {
    if transactions.is_empty() {
        return Ok((vec![], vec![], SendTimingBreakdown::default()));
    }
    let mut sent = Vec::with_capacity(transactions.len());
    let mut warnings = Vec::new();
    let mut timing = SendTimingBreakdown::default();
    for transaction in transactions {
        let (mut submitted, mut entry_warnings, submit_ms) = submit_transactions_for_transport(
            rpc_url,
            transport_plan,
            std::slice::from_ref(transaction),
            commitment,
            skip_preflight,
            track_send_block_height,
        )
        .await?;
        warnings.append(&mut entry_warnings);
        timing.submit_ms = timing.submit_ms.saturating_add(submit_ms);

        let confirm_future = confirm_submitted_transactions_for_transport(
            rpc_url,
            transport_plan,
            &mut submitted,
            commitment,
            track_send_block_height,
        );
        let confirm_result = if let Some(timeout_secs) = confirm_timeout_secs {
            match tokio::time::timeout(Duration::from_secs(timeout_secs), confirm_future).await {
                Ok(result) => result,
                Err(_) => Err(format!(
                    "Timed out waiting for transaction {} to reach {} ({}s sequential confirmation budget exceeded).",
                    transaction.label, commitment, timeout_secs
                )),
            }
        } else {
            confirm_future.await
        };

        match confirm_result {
            Ok((mut confirm_warnings, confirm_ms)) => {
                warnings.append(&mut confirm_warnings);
                timing.confirm_ms = timing.confirm_ms.saturating_add(confirm_ms);
                sent.append(&mut submitted);
            }
            Err(error) => {
                let actual_blockhash_valid =
                    is_blockhash_valid(rpc_url, &transaction.blockhash, commitment)
                        .await
                        .ok();
                let current_block_height =
                    fetch_current_block_height(rpc_url, commitment).await.ok();
                return Err(match current_block_height {
                    Some(height) => format!(
                        "{error} (current block height {}; actualBlockhashValid={:?})",
                        height, actual_blockhash_valid
                    ),
                    None => format!(
                        "{error} (actualBlockhashValid={:?})",
                        actual_blockhash_valid
                    ),
                });
            }
        }
    }
    Ok((sent, warnings, timing))
}

fn should_reserve_follow_job(
    follow_launch: &NormalizedFollowLaunch,
    deferred_setup_transactions: &[CompiledTransaction],
) -> bool {
    follow_launch.enabled || !deferred_setup_transactions.is_empty()
}

fn same_time_wallet_label(wallet_env_key: &str) -> String {
    let trimmed = wallet_env_key.trim();
    let suffix = trimmed.trim_start_matches("SOLANA_PRIVATE_KEY").trim();
    if suffix.is_empty() {
        "primary".to_string()
    } else {
        suffix.to_string()
    }
}

fn launchdeck_coin_trade_records_from_sent_results(
    sent: &[crate::rpc::SentResult],
    wallet_keys: &[String],
    mint: &str,
    client_request_id: Option<&str>,
) -> Result<Vec<execution_engine_bridge::ExecutionEngineConfirmedTradeRecord>, String> {
    if sent.len() != wallet_keys.len() {
        return Err(format!(
            "LaunchDeck coin-trade wallet mapping mismatch: sent={} wallets={}",
            sent.len(),
            wallet_keys.len()
        ));
    }
    Ok(sent
        .iter()
        .zip(wallet_keys.iter())
        .filter_map(|(result, wallet_key)| {
            confirmed_trade_record_from_sent_result(
                result,
                wallet_key,
                mint,
                client_request_id,
                Some(result.label.as_str()),
            )
        })
        .collect())
}

async fn record_launchdeck_coin_trades_best_effort(
    trace_id: &str,
    sent: &[crate::rpc::SentResult],
    wallet_keys: &[String],
    mint: &str,
    phase: &str,
) {
    let records = match launchdeck_coin_trade_records_from_sent_results(
        sent,
        wallet_keys,
        mint,
        Some(trace_id),
    ) {
        Ok(records) => records,
        Err(error) => {
            record_warn(
                "execution-engine-bridge",
                "Failed to map LaunchDeck confirmed trades for execution-engine.",
                Some(json!({
                    "traceId": trace_id,
                    "phase": phase,
                    "message": error,
                })),
            );
            return;
        }
    };
    if records.is_empty() {
        return;
    }
    if let Err(error) = record_confirmed_trades(&records).await {
        record_warn(
            "execution-engine-bridge",
            "Failed to hand LaunchDeck confirmed trades to execution-engine.",
            Some(json!({
                "traceId": trace_id,
                "phase": phase,
                "message": error,
            })),
        );
    }
}

const DEFAULT_HELIUS_PRIORITY_REFRESH_INTERVAL_MS: u64 = 30_000;
const DEFAULT_WALLET_STATUS_REFRESH_INTERVAL_MS: u64 = 30_000;

#[derive(Debug, Clone)]
struct AutoFeeResolutionSummary {
    notes: Vec<String>,
    report: AutoFeeReport,
}

fn shared_fee_market_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("shared fee market client")
    })
}

fn auto_fee_helius_priority_level() -> String {
    let value = std::env::var("HELIUS_PRIORITY_LEVEL")
        .or_else(|_| std::env::var("TRENCH_AUTO_FEE_HELIUS_PRIORITY_LEVEL"))
        .or_else(|_| std::env::var("LAUNCHDECK_AUTO_FEE_HELIUS_PRIORITY_LEVEL"))
        .unwrap_or_else(|_| DEFAULT_AUTO_FEE_HELIUS_PRIORITY_LEVEL.to_string());
    normalize_helius_priority_level(&value)
}

fn auto_fee_jito_tip_percentile() -> String {
    let value = std::env::var("JITO_TIP_PERCENTILE")
        .or_else(|_| std::env::var("TRENCH_AUTO_FEE_JITO_TIP_PERCENTILE"))
        .or_else(|_| std::env::var("LAUNCHDECK_AUTO_FEE_JITO_TIP_PERCENTILE"))
        .unwrap_or_else(|_| DEFAULT_AUTO_FEE_JITO_TIP_PERCENTILE.to_string());
    normalize_jito_tip_percentile(&value)
}

fn configured_helius_priority_refresh_interval() -> Duration {
    Duration::from_millis(
        std::env::var("HELIUS_PRIORITY_REFRESH_INTERVAL_MS")
            .or_else(|_| std::env::var("TRENCH_HELIUS_PRIORITY_REFRESH_INTERVAL_MS"))
            .or_else(|_| std::env::var("LAUNCHDECK_HELIUS_PRIORITY_REFRESH_INTERVAL_MS"))
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_HELIUS_PRIORITY_REFRESH_INTERVAL_MS),
    )
}

fn configured_wallet_status_refresh_interval_ms() -> u64 {
    std::env::var("LAUNCHDECK_WALLET_STATUS_REFRESH_INTERVAL_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_WALLET_STATUS_REFRESH_INTERVAL_MS)
}

const FEE_TEMPLATE_LAUNCH_ACCOUNT_KEYS: [&str; 8] = [
    "ComputeBudget111111111111111111111111111111",
    "11111111111111111111111111111111",
    "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
    "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
    "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s",
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
];

fn shared_fee_market_runtime(rpc_url: &str) -> SharedFeeMarketRuntime {
    let mut config = SharedFeeMarketConfig::new(
        paths::shared_fee_market_cache_path(),
        rpc_url.to_string(),
        resolved_helius_priority_fee_rpc_url(rpc_url),
        format!("launchdeck-engine-{}", std::process::id()),
        FEE_TEMPLATE_LAUNCH_ACCOUNT_KEYS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
    );
    config.helius_priority_level = auto_fee_helius_priority_level();
    config.jito_tip_percentile = auto_fee_jito_tip_percentile();
    config.helius_refresh_interval = configured_helius_priority_refresh_interval();
    SharedFeeMarketRuntime::new(config)
}

fn helius_sender_ping_endpoint_url(endpoint_url: &str) -> String {
    let trimmed = endpoint_url.trim().trim_end_matches('/');
    if let Some(prefix) = trimmed.strip_suffix("/fast") {
        return format!("{prefix}/ping");
    }
    if trimmed.ends_with("/ping") {
        return trimmed.to_string();
    }
    format!("{trimmed}/ping")
}

async fn prewarm_helius_sender_endpoint(rpc_url: &str) -> Result<(), String> {
    let ping_url = helius_sender_ping_endpoint_url(rpc_url);
    let response = shared_fee_market_http_client()
        .get(&ping_url)
        .send()
        .await
        .map_err(|error| format!("Sender ping request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("(body unavailable)"));
        return Err(format!("Sender ping failed with status {status}: {body}"));
    }
    Ok(())
}

async fn fetch_fee_market_snapshot(rpc_url: &str) -> Result<FeeMarketSnapshot, String> {
    shared_fee_market_runtime(rpc_url)
        .fetch_fee_market_snapshot()
        .await
}

fn spawn_fee_market_snapshot_refresh_task(state: Arc<AppState>, rpc_url: String) {
    let primary_rpc_url = rpc_url.clone();
    let helius_state = state.clone();
    tokio::spawn(async move {
        loop {
            if engine_background_requests_active(&helius_state) {
                // Tighten retry on failure so a transient Helius hiccup
                // doesn't leave the snapshot stale for a full refresh
                // interval (which produced visible "Auto Fee unavailable"
                // warnings on the FE).
                let outcome = shared_fee_market_runtime(&primary_rpc_url)
                    .refresh_helius_if_leased()
                    .await;
                let sleep_for = match outcome {
                    shared_fee_market::RefreshOutcome::Failed => {
                        Duration::from_millis(shared_fee_market::HELIUS_REFRESH_RETRY_BACKOFF_MS)
                    }
                    _ => configured_helius_priority_refresh_interval(),
                };
                tokio::time::sleep(sleep_for).await;
            } else {
                tokio::time::sleep(IDLE_BACKGROUND_REQUEST_POLL_INTERVAL).await;
            }
        }
    });
    let jito_state = state.clone();
    tokio::spawn(async move {
        loop {
            if engine_background_requests_active(&jito_state) {
                let runtime = shared_fee_market_runtime(&rpc_url);
                let outcome = runtime.refresh_jito_if_leased().await;
                let sleep_for = match outcome {
                    shared_fee_market::RefreshOutcome::Failed => {
                        Duration::from_millis(shared_fee_market::HELIUS_REFRESH_RETRY_BACKOFF_MS)
                    }
                    _ => runtime.config().jito_reconnect_delay,
                };
                tokio::time::sleep(sleep_for).await;
            } else {
                tokio::time::sleep(IDLE_BACKGROUND_REQUEST_POLL_INTERVAL).await;
            }
        }
    });
}

fn spawn_engine_blockhash_refresh_task(
    state: Arc<AppState>,
    rpc_url: String,
    commitment: &'static str,
) {
    tokio::spawn(async move {
        loop {
            if engine_background_requests_active(&state) {
                let _ = refresh_latest_blockhash_cache(&rpc_url, commitment).await;
                tokio::time::sleep(ENGINE_BLOCKHASH_REFRESH_INTERVAL).await;
            } else {
                tokio::time::sleep(IDLE_BACKGROUND_REQUEST_POLL_INTERVAL).await;
            }
        }
    });
}

fn log_vanity_pool_startup_format_diagnostics(status: &Value) {
    let Some(launchpads) = status.get("launchpads").and_then(Value::as_array) else {
        return;
    };
    let mut total_problem_lines = 0usize;
    let mut diagnostics = Vec::new();
    for launchpad in launchpads {
        let invalid = launchpad
            .get("invalid")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize;
        let duplicates = launchpad
            .get("duplicates")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize;
        if invalid == 0 && duplicates == 0 {
            continue;
        }
        total_problem_lines =
            total_problem_lines.saturating_add(invalid.saturating_add(duplicates));
        let launchpad_name = launchpad
            .get("launchpad")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        for diagnostic in launchpad
            .get("diagnostics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let line = diagnostic
                .get("line")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let code = diagnostic
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("invalid");
            if code != "invalid" && code != "duplicate" {
                continue;
            }
            let message = diagnostic
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Invalid vanity queue entry.");
            diagnostics.push(format!("{launchpad_name}.txt line {line}: {message}"));
        }
    }
    if total_problem_lines == 0 {
        return;
    }
    let summary = format!(
        "Vanity queue format warning: {total_problem_lines} invalid or duplicate line(s) were found. These entries will be skipped until fixed."
    );
    eprintln!("{summary}");
    for diagnostic in diagnostics.iter().take(12) {
        eprintln!("  - {diagnostic}");
    }
    if diagnostics.len() > 12 {
        eprintln!(
            "  - ... {} more vanity queue diagnostic(s).",
            diagnostics.len().saturating_sub(12)
        );
    }
    record_warn(
        "vanity-pool",
        summary,
        Some(json!({
            "diagnostics": diagnostics,
        })),
    );
}

fn spawn_vanity_pool_refresh_task(rpc_url: String) {
    match preload_vanity_pool() {
        Ok(status) => log_vanity_pool_startup_format_diagnostics(&status),
        Err(error) => {
            record_warn(
                "vanity-pool",
                format!("Vanity pool preload failed: {error}"),
                None,
            );
        }
    }
    tokio::spawn(async move {
        loop {
            if let Err(error) = refresh_vanity_pool_with_rpc(&rpc_url).await {
                record_warn(
                    "vanity-pool",
                    format!("Vanity pool background refresh failed: {error}"),
                    None,
                );
            }
            tokio::time::sleep(VANITY_POOL_REFRESH_INTERVAL).await;
        }
    });
}

async fn resolve_auto_execution_fees(
    rpc_url: &str,
    normalized: &mut NormalizedConfig,
    transport_plan: &crate::transport::TransportPlan,
    _wallet_secret: &[u8],
    _creator_public_key: &str,
) -> Result<AutoFeeResolutionSummary, String> {
    let needs_auto = normalized.execution.autoGas
        || normalized.execution.buyAutoGas
        || normalized.execution.sellAutoGas;
    let mut notes = Vec::new();
    let jito_tip_percentile = auto_fee_jito_tip_percentile();
    if !needs_auto {
        return Ok(AutoFeeResolutionSummary {
            notes,
            report: AutoFeeReport {
                jito_tip_percentile: jito_tip_percentile.clone(),
                snapshot: FeeMarketSnapshot::default(),
                creation: AutoFeeActionReport {
                    enabled: normalized.execution.autoGas,
                    provider: normalized.execution.provider.clone(),
                    prioritySource: "off".to_string(),
                    priorityEstimateLamports: None,
                    resolvedPriorityLamports: None,
                    tipSource: "off".to_string(),
                    tipEstimateLamports: None,
                    resolvedTipLamports: None,
                    capLamports: None,
                },
                buy: AutoFeeActionReport {
                    enabled: normalized.execution.buyAutoGas,
                    provider: normalized.execution.buyProvider.clone(),
                    prioritySource: "off".to_string(),
                    priorityEstimateLamports: None,
                    resolvedPriorityLamports: None,
                    tipSource: "off".to_string(),
                    tipEstimateLamports: None,
                    resolvedTipLamports: None,
                    capLamports: None,
                },
                sell: AutoFeeActionReport {
                    enabled: normalized.execution.sellAutoGas,
                    provider: normalized.execution.sellProvider.clone(),
                    prioritySource: "off".to_string(),
                    priorityEstimateLamports: None,
                    resolvedPriorityLamports: None,
                    tipSource: "off".to_string(),
                    tipEstimateLamports: None,
                    resolvedTipLamports: None,
                    capLamports: None,
                },
            },
        });
    }

    let market_status = shared_fee_market_runtime(rpc_url).read_snapshot_status();
    let market = market_status
        .as_ref()
        .map(|status| status.snapshot.clone())
        .unwrap_or_default();
    let mut creation_report = AutoFeeActionReport {
        enabled: normalized.execution.autoGas,
        provider: normalized.execution.provider.clone(),
        prioritySource: "off".to_string(),
        priorityEstimateLamports: None,
        resolvedPriorityLamports: None,
        tipSource: "off".to_string(),
        tipEstimateLamports: None,
        resolvedTipLamports: None,
        capLamports: None,
    };
    let mut buy_report = AutoFeeActionReport {
        enabled: normalized.execution.buyAutoGas,
        provider: normalized.execution.buyProvider.clone(),
        prioritySource: "off".to_string(),
        priorityEstimateLamports: None,
        resolvedPriorityLamports: None,
        tipSource: "off".to_string(),
        tipEstimateLamports: None,
        resolvedTipLamports: None,
        capLamports: None,
    };
    let mut sell_report = AutoFeeActionReport {
        enabled: normalized.execution.sellAutoGas,
        provider: normalized.execution.sellProvider.clone(),
        prioritySource: "off".to_string(),
        priorityEstimateLamports: None,
        resolvedPriorityLamports: None,
        tipSource: "off".to_string(),
        tipEstimateLamports: None,
        resolvedTipLamports: None,
        capLamports: None,
    };

    let priority_estimate_for_action = |action: &str| -> (Option<u64>, String) {
        if !market_status
            .as_ref()
            .map(|status| status.helius_fresh)
            .unwrap_or(false)
        {
            (None, "stale".to_string())
        } else {
            action_priority_estimate(&market, action)
        }
    };
    let tip_estimate_for_action = || -> (Option<u64>, String) {
        if !market_status
            .as_ref()
            .map(|status| status.jito_fresh)
            .unwrap_or(false)
        {
            (None, "stale".to_string())
        } else {
            action_tip_estimate(&market, &jito_tip_percentile)
        }
    };

    if normalized.execution.autoGas {
        let cap_lamports = parse_auto_fee_cap_lamports(
            if !normalized.execution.maxPriorityFeeSol.trim().is_empty() {
                &normalized.execution.maxPriorityFeeSol
            } else {
                &normalized.execution.maxTipSol
            },
        );
        let provider = normalized.execution.provider.as_str();
        creation_report.capLamports = cap_lamports;
        let uses_priority =
            provider_uses_auto_fee_priority(provider, &transport_plan.executionClass, "creation");
        let uses_tip = provider_uses_auto_fee_tip(provider, "creation");
        let (resolved_priority, resolved_tip) =
            resolve_auto_fee_components_with_total_cap(
                if uses_priority {
                    let (estimated, source) = priority_estimate_for_action("creation");
                    creation_report.prioritySource = source;
                    creation_report.priorityEstimateLamports = estimated;
                    Some(apply_auto_fee_estimate_buffer(estimated.ok_or_else(|| {
                    "Creation auto fee is enabled but no Helius priority estimate was returned."
                        .to_string()
                })?))
                } else {
                    None
                },
                if uses_tip {
                    let (estimated, source) = tip_estimate_for_action();
                    creation_report.tipSource = source;
                    creation_report.tipEstimateLamports = estimated;
                    Some(apply_auto_fee_estimate_buffer(estimated.ok_or_else(
                        || {
                            "Creation auto fee is enabled but no Jito tip estimate was returned."
                                .to_string()
                        },
                    )?))
                } else {
                    None
                },
                cap_lamports,
                provider,
                "Creation",
            )?;

        if let Some(resolved) = resolved_priority {
            normalized.execution.priorityFeeSol =
                priority_price_micro_lamports_to_sol_equivalent(resolved);
            normalized.tx.computeUnitPriceMicroLamports =
                Some(lamports_to_priority_fee_micro_lamports(resolved) as i64);
            creation_report.resolvedPriorityLamports = Some(resolved);
        } else {
            normalized.execution.priorityFeeSol.clear();
            normalized.tx.computeUnitPriceMicroLamports = Some(0);
        }

        if let Some(resolved) = resolved_tip {
            normalized.execution.tipSol = format_lamports_to_sol_decimal(resolved);
            normalized.tx.jitoTipLamports = resolved as i64;
            normalized.tx.jitoTipAccount = pick_tip_account_for_provider(provider);
            creation_report.resolvedTipLamports = Some(resolved);
        } else {
            normalized.execution.tipSol.clear();
            normalized.tx.jitoTipLamports = 0;
            normalized.tx.jitoTipAccount.clear();
        }

        notes.push(format!(
            "Creation auto fee resolved{}: priority price={} | tip={} SOL",
            cap_lamports
                .map(|cap| format!(" with cap {} SOL", format_lamports_to_sol_decimal(cap)))
                .unwrap_or_default(),
            if normalized.execution.priorityFeeSol.trim().is_empty() {
                "off".to_string()
            } else {
                format_priority_price_note(
                    creation_report.resolvedPriorityLamports.unwrap_or_default(),
                )
            },
            if normalized.execution.tipSol.trim().is_empty() {
                "off".to_string()
            } else {
                normalized.execution.tipSol.clone()
            }
        ));
    }

    if normalized.execution.buyAutoGas {
        let cap_lamports = parse_auto_fee_cap_lamports(
            if !normalized.execution.buyMaxPriorityFeeSol.trim().is_empty() {
                &normalized.execution.buyMaxPriorityFeeSol
            } else {
                &normalized.execution.buyMaxTipSol
            },
        );
        let provider = normalized.execution.buyProvider.as_str();
        buy_report.capLamports = cap_lamports;
        let uses_priority =
            provider_uses_auto_fee_priority(provider, &transport_plan.executionClass, "buy");
        let uses_tip = provider_uses_auto_fee_tip(provider, "buy");
        let (resolved_priority, resolved_tip) = resolve_auto_fee_components_with_total_cap(
            if uses_priority {
                let (estimated, source) = priority_estimate_for_action("buy");
                buy_report.prioritySource = source;
                buy_report.priorityEstimateLamports = estimated;
                Some(apply_auto_fee_estimate_buffer(estimated.ok_or_else(
                    || {
                        "Buy auto fee is enabled but no Helius priority estimate was returned."
                            .to_string()
                    },
                )?))
            } else {
                None
            },
            if uses_tip {
                let (estimated, source) = tip_estimate_for_action();
                buy_report.tipSource = source;
                buy_report.tipEstimateLamports = estimated;
                Some(apply_auto_fee_estimate_buffer(estimated.ok_or_else(
                    || "Buy auto fee is enabled but no Jito tip estimate was returned.".to_string(),
                )?))
            } else {
                None
            },
            cap_lamports,
            provider,
            "Buy",
        )?;

        if let Some(resolved) = resolved_priority {
            normalized.execution.buyPriorityFeeSol =
                priority_price_micro_lamports_to_sol_equivalent(resolved);
            buy_report.resolvedPriorityLamports = Some(resolved);
        } else {
            normalized.execution.buyPriorityFeeSol.clear();
        }

        if let Some(resolved) = resolved_tip {
            normalized.execution.buyTipSol = format_lamports_to_sol_decimal(resolved);
            buy_report.resolvedTipLamports = Some(resolved);
        } else {
            normalized.execution.buyTipSol.clear();
        }

        notes.push(format!(
            "Buy auto fee resolved{}: priority price={} | tip={} SOL",
            cap_lamports
                .map(|cap| format!(" with cap {} SOL", format_lamports_to_sol_decimal(cap)))
                .unwrap_or_default(),
            if normalized.execution.buyPriorityFeeSol.trim().is_empty() {
                "off".to_string()
            } else {
                format_priority_price_note(buy_report.resolvedPriorityLamports.unwrap_or_default())
            },
            if normalized.execution.buyTipSol.trim().is_empty() {
                "off".to_string()
            } else {
                normalized.execution.buyTipSol.clone()
            }
        ));
    }

    if normalized.execution.sellAutoGas {
        let cap_lamports = parse_auto_fee_cap_lamports(
            if !normalized.execution.sellMaxPriorityFeeSol.trim().is_empty() {
                &normalized.execution.sellMaxPriorityFeeSol
            } else {
                &normalized.execution.sellMaxTipSol
            },
        );
        let provider = normalized.execution.sellProvider.as_str();
        sell_report.capLamports = cap_lamports;
        let uses_priority =
            provider_uses_auto_fee_priority(provider, &transport_plan.executionClass, "sell");
        let uses_tip = provider_uses_auto_fee_tip(provider, "sell");
        let (resolved_priority, resolved_tip) = resolve_auto_fee_components_with_total_cap(
            if uses_priority {
                let (estimated, source) = priority_estimate_for_action("sell");
                sell_report.prioritySource = source;
                sell_report.priorityEstimateLamports = estimated;
                Some(apply_auto_fee_estimate_buffer(estimated.ok_or_else(
                    || {
                        "Sell auto fee is enabled but no Helius priority estimate was returned."
                            .to_string()
                    },
                )?))
            } else {
                None
            },
            if uses_tip {
                let (estimated, source) = tip_estimate_for_action();
                sell_report.tipSource = source;
                sell_report.tipEstimateLamports = estimated;
                Some(apply_auto_fee_estimate_buffer(estimated.ok_or_else(
                    || {
                        "Sell auto fee is enabled but no Jito tip estimate was returned."
                            .to_string()
                    },
                )?))
            } else {
                None
            },
            cap_lamports,
            provider,
            "Sell",
        )?;

        if let Some(resolved) = resolved_priority {
            normalized.execution.sellPriorityFeeSol =
                priority_price_micro_lamports_to_sol_equivalent(resolved);
            sell_report.resolvedPriorityLamports = Some(resolved);
        } else {
            normalized.execution.sellPriorityFeeSol.clear();
        }

        if let Some(resolved) = resolved_tip {
            normalized.execution.sellTipSol = format_lamports_to_sol_decimal(resolved);
            sell_report.resolvedTipLamports = Some(resolved);
        } else {
            normalized.execution.sellTipSol.clear();
        }

        notes.push(format!(
            "Sell auto fee resolved{}: priority price={} | tip={} SOL",
            cap_lamports
                .map(|cap| format!(" with cap {} SOL", format_lamports_to_sol_decimal(cap)))
                .unwrap_or_default(),
            if normalized.execution.sellPriorityFeeSol.trim().is_empty() {
                "off".to_string()
            } else {
                format_priority_price_note(sell_report.resolvedPriorityLamports.unwrap_or_default())
            },
            if normalized.execution.sellTipSol.trim().is_empty() {
                "off".to_string()
            } else {
                normalized.execution.sellTipSol.clone()
            }
        ));
    }

    Ok(AutoFeeResolutionSummary {
        notes,
        report: AutoFeeReport {
            jito_tip_percentile,
            snapshot: market,
            creation: creation_report,
            buy: buy_report,
            sell: sell_report,
        },
    })
}

fn apply_same_time_creation_fee_guard(
    normalized: &mut NormalizedConfig,
) -> Result<Option<String>, String> {
    if !has_same_time_snipes(&normalized.followLaunch) {
        return Ok(None);
    }
    let creation_priority = parse_sol_decimal_to_lamports(&normalized.execution.priorityFeeSol)
        .ok_or_else(|| {
            "Invalid creation priority fee while applying same-time guard.".to_string()
        })?;
    let buy_priority = parse_sol_decimal_to_lamports(&normalized.execution.buyPriorityFeeSol)
        .ok_or_else(|| "Invalid buy priority fee while applying same-time guard.".to_string())?;
    let creation_tip = parse_sol_decimal_to_lamports(&normalized.execution.tipSol)
        .ok_or_else(|| "Invalid creation tip while applying same-time guard.".to_string())?;
    let buy_tip = parse_sol_decimal_to_lamports(&normalized.execution.buyTipSol)
        .ok_or_else(|| "Invalid buy tip while applying same-time guard.".to_string())?;
    let mut adjusted_fields = Vec::new();
    if creation_priority <= buy_priority {
        let next_priority = buy_priority.saturating_add(1);
        if normalized.execution.autoGas {
            if let Some(cap_lamports) =
                parse_auto_fee_cap_lamports(&normalized.execution.maxPriorityFeeSol)
            {
                if next_priority > cap_lamports {
                    return Err(
                        "Same-time sniper safeguard needs a higher launch priority fee than your Creation max auto fee allows."
                            .to_string(),
                    );
                }
            }
        }
        let next_priority_text = format_lamports_to_sol_decimal(next_priority);
        normalized.execution.priorityFeeSol = next_priority_text.clone();
        normalized.execution.maxPriorityFeeSol = next_priority_text;
        normalized.tx.computeUnitPriceMicroLamports =
            Some(lamports_to_priority_fee_micro_lamports(next_priority) as i64);
        adjusted_fields.push("priority fee");
    }
    if creation_tip <= buy_tip {
        let next_tip = buy_tip.saturating_add(1);
        if normalized.execution.autoGas {
            if let Some(cap_lamports) = parse_auto_fee_cap_lamports(&normalized.execution.maxTipSol)
            {
                if next_tip > cap_lamports {
                    return Err(
                        "Same-time sniper safeguard needs a higher launch tip than your Creation max auto fee allows."
                            .to_string(),
                    );
                }
            }
        }
        let next_tip_text = format_lamports_to_sol_decimal(next_tip);
        normalized.execution.tipSol = next_tip_text.clone();
        normalized.execution.maxTipSol = next_tip_text;
        normalized.tx.jitoTipLamports = next_tip as i64;
        adjusted_fields.push("tip");
    }
    if adjusted_fields.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!(
            "Same-time sniper safeguard raised launch {} above same-time buy fees.",
            adjusted_fields.join(" and ")
        )))
    }
}

async fn compile_same_time_snipes(
    rpc_url: &str,
    normalized: &crate::config::NormalizedConfig,
    mint: &str,
    launch_creator: &str,
    snipes: &[crate::config::NormalizedFollowLaunchSnipe],
    allow_ata_creation: bool,
    bags_launch: Option<&crate::follow::BagsLaunchMetadata>,
) -> Result<Vec<CompiledTransaction>, String> {
    let (predicted_dev_buy_tokens, predicted_dev_buy_quote_amount, pump_cashback_enabled) =
        resolve_predicted_creator_dev_buy_effect(rpc_url, normalized).await?;
    let buy_tip_account = pick_tip_account_for_provider(&normalized.execution.buyProvider);
    let tasks = snipes.iter().enumerate().map(|(index, snipe)| {
        let buy_tip_account = buy_tip_account.clone();
        async move {
            let wallet_secret = load_solana_wallet_by_env_key(&snipe.walletEnvKey)?;
            let mut tx = compile_atomic_follow_buy_for_launchpad(
                &normalized.launchpad,
                &normalized.mode,
                &normalized.quoteAsset,
                rpc_url,
                &normalized.execution,
                normalized.token.mayhemMode,
                &buy_tip_account,
                &wallet_secret,
                mint,
                launch_creator,
                &snipe.buyAmountSol,
                allow_ata_creation,
                predicted_dev_buy_tokens,
                predicted_dev_buy_quote_amount,
                pump_cashback_enabled,
                bags_launch,
                normalized.wrapperDefaultFeeBps,
            )
            .await?;
            tx.label = format!(
                "sniper-buy-{}-wallet-{}",
                index + 1,
                same_time_wallet_label(&snipe.walletEnvKey)
            );
            Ok::<Vec<CompiledTransaction>, String>(vec![tx])
        }
    });
    join_all(tasks)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map(|groups| groups.into_iter().flatten().collect())
}

async fn resolve_predicted_creator_dev_buy_effect(
    rpc_url: &str,
    normalized: &NormalizedConfig,
) -> Result<(Option<u64>, Option<u64>, Option<bool>), String> {
    match normalized.launchpad.as_str() {
        "pump" => Ok((
            predict_dev_buy_token_amount_for_launchpad(rpc_url, normalized).await?,
            None,
            Some(normalized.mode == "cashback"),
        )),
        "bonk" => {
            let effect = crate::bonk_native::predict_dev_buy_effect(rpc_url, normalized).await?;
            Ok((
                effect.as_ref().map(|value| value.token_amount),
                effect.as_ref().map(|value| value.requested_quote_amount_b),
                None,
            ))
        }
        "bagsapp" => Ok((None, None, None)),
        other => Err(format!(
            "Unsupported launchpad for predicted creator dev buy effect: {other}"
        )),
    }
}

fn launch_prefers_post_setup_creator_vault_for_follow(
    launchpad: &str,
    mode: &str,
    generate_later_setup: bool,
    has_fee_recipients: bool,
) -> bool {
    matches!(mode, "agent-custom" | "agent-locked")
        || (launchpad == "pump" && mode == "regular" && generate_later_setup && has_fee_recipients)
}

fn prefers_post_setup_creator_vault_for_follow(normalized: &NormalizedConfig) -> bool {
    launch_prefers_post_setup_creator_vault_for_follow(
        &normalized.launchpad,
        &normalized.mode,
        normalized.feeSharing.generateLaterSetup,
        !normalized.feeSharing.recipients.is_empty(),
    )
}

async fn compile_presigned_follow_actions(
    rpc_url: &str,
    normalized: &NormalizedConfig,
    mint: &str,
    launch_creator: &str,
    allow_ata_creation: bool,
) -> Result<Vec<crate::follow::FollowActionRecord>, String> {
    let mut actions = build_action_records(&normalized.followLaunch);
    let buy_tip_account = pick_tip_account_for_provider(&normalized.execution.buyProvider);
    let mut predicted_dev_buy_effect: Option<(Option<u64>, Option<u64>, Option<bool>)> = None;
    let bonk_pool_id = if normalized.launchpad == "bonk" {
        derive_canonical_pool_id_for_launchpad(
            rpc_url,
            &normalized.launchpad,
            &normalized.quoteAsset,
            mint,
        )
        .await?
    } else {
        None
    };
    for action in &mut actions {
        let wallet_key = selected_wallet_key_or_default(&action.walletEnvKey)
            .ok_or_else(|| format!("Wallet env key not found: {}", action.walletEnvKey))?;
        let wallet_secret = load_solana_wallet_by_env_key(&wallet_key)?;
        if let Some(pool_id) = bonk_pool_id.as_ref() {
            action.poolId = Some(pool_id.clone());
        }
        match action.kind {
            crate::follow::FollowActionKind::SniperBuy => {
                let Some(buy_amount_sol) = action.buyAmountSol.as_deref() else {
                    continue;
                };
                if action.targetBlockOffset.unwrap_or_default() > 0 {
                    continue;
                }
                if normalized.launchpad == "pump"
                    && should_use_post_setup_creator_vault_for_buy(
                        prefers_post_setup_creator_vault_for_follow(normalized),
                        action,
                        &normalized.execution.buyMevMode,
                    )
                {
                    continue;
                }
                let (
                    predicted_dev_buy_tokens,
                    predicted_dev_buy_quote_amount,
                    pump_cashback_enabled,
                ) = match predicted_dev_buy_effect {
                    Some(value) => value,
                    None => {
                        let value =
                            resolve_predicted_creator_dev_buy_effect(rpc_url, normalized).await?;
                        predicted_dev_buy_effect = Some(value);
                        value
                    }
                };
                let mut tx = compile_atomic_follow_buy_for_launchpad(
                    &normalized.launchpad,
                    &normalized.mode,
                    &normalized.quoteAsset,
                    rpc_url,
                    &normalized.execution,
                    normalized.token.mayhemMode,
                    &buy_tip_account,
                    &wallet_secret,
                    mint,
                    launch_creator,
                    buy_amount_sol,
                    allow_ata_creation,
                    predicted_dev_buy_tokens,
                    predicted_dev_buy_quote_amount,
                    pump_cashback_enabled,
                    None,
                    normalized.wrapperDefaultFeeBps,
                )
                .await?;
                tx.label = action.actionId.clone();
                action.preSignedTransactions = vec![tx];
            }
            _ => {}
        }
    }
    Ok(actions)
}

#[cfg(test)]
fn follow_action_is_immediate_presign_candidate(
    action: &crate::follow::FollowActionRecord,
) -> bool {
    action.marketCap.is_none()
        && action.delayMs.unwrap_or_default() == 0
        && action.submitDelayMs.unwrap_or_default() == 0
        && action.targetBlockOffset.unwrap_or_default() == 0
}

async fn build_status_payload(
    state: &Arc<AppState>,
    requested_wallet: &str,
    strict_wallet_selection: bool,
) -> Result<Value, String> {
    let mut payload = build_bootstrap_fast_payload(requested_wallet, strict_wallet_selection)?;
    let wallet_status =
        build_wallet_status_payload(requested_wallet, strict_wallet_selection).await?;
    let runtime_status = build_runtime_status_payload(state).await;
    payload["rpcUrl"] = wallet_status
        .get("rpcUrl")
        .cloned()
        .unwrap_or(Value::String(configured_rpc_url()));
    payload["connected"] = wallet_status
        .get("connected")
        .cloned()
        .unwrap_or(Value::Bool(false));
    payload["wallets"] = wallet_status
        .get("wallets")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    payload["selectedWalletKey"] = wallet_status
        .get("selectedWalletKey")
        .cloned()
        .unwrap_or(Value::Null);
    payload["wallet"] = wallet_status.get("wallet").cloned().unwrap_or(Value::Null);
    payload["balanceLamports"] = wallet_status
        .get("balanceLamports")
        .cloned()
        .unwrap_or(Value::Null);
    payload["balanceSol"] = wallet_status
        .get("balanceSol")
        .cloned()
        .unwrap_or(Value::Null);
    payload["usd1Balance"] = wallet_status
        .get("usd1Balance")
        .cloned()
        .unwrap_or(Value::Null);
    payload["transport"] = runtime_status
        .get("transport")
        .cloned()
        .unwrap_or(Value::Null);
    payload["followDaemon"] = runtime_status
        .get("followDaemon")
        .cloned()
        .unwrap_or(Value::Null);
    payload["runtime"] = runtime_status
        .get("runtime")
        .cloned()
        .unwrap_or(Value::Null);
    payload["runtimeWorkers"] = runtime_status
        .get("runtimeWorkers")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    payload["vanityQueues"] = vanity_pool_status_payload().unwrap_or_else(|error| {
        json!({
            "ok": false,
            "error": error,
        })
    });
    Ok(payload)
}

fn resolve_selected_wallet_key(
    requested_wallet: &str,
    strict_wallet_selection: bool,
    raw_wallets: &[crate::wallet::WalletSummary],
) -> Result<Option<String>, String> {
    let requested_wallet = requested_wallet.trim();
    let requested_wallet_is_known = requested_wallet.is_empty()
        || raw_wallets
            .iter()
            .any(|wallet| wallet.envKey == requested_wallet);
    if strict_wallet_selection && !requested_wallet_is_known {
        return Err(format!("Unknown wallet env key: {}", requested_wallet));
    }
    Ok(selected_wallet_key_or_default_from_wallets(
        if requested_wallet_is_known {
            requested_wallet
        } else {
            ""
        },
        raw_wallets,
    ))
}

fn build_bootstrap_fast_payload(
    requested_wallet: &str,
    strict_wallet_selection: bool,
) -> Result<Value, String> {
    let raw_wallets = list_solana_env_wallets();
    let selected_wallet_key =
        resolve_selected_wallet_key(requested_wallet, strict_wallet_selection, &raw_wallets)?;
    let selected_wallet = selected_wallet_key.as_ref().and_then(|env_key| {
        raw_wallets
            .iter()
            .find(|wallet| wallet.envKey == *env_key)
            .cloned()
    });
    Ok(json!({
        "ok": true,
        "service": "launchdeck-engine",
        "engineBackend": "rust",
        "implemented": true,
        "executionMode": "rust-native-only",
        "message": "Rust engine is online with native Pump execution, native RPC transport, and native runtime workers. Unsupported requests fail explicitly instead of falling back to JavaScript.",
        "connected": selected_wallet.is_some(),
        "wallets": raw_wallets,
        "selectedWalletKey": selected_wallet_key,
        "wallet": selected_wallet.as_ref().and_then(|wallet| wallet.publicKey.clone()),
        "providers": provider_availability_registry(),
        "providerRegistry": provider_registry(),
        "launchpads": launchpad_registry(),
        "startupWarm": {
            "enabled": configured_startup_warm_enabled(),
        },
        "uiRefresh": {
            "walletStatusIntervalMs": configured_wallet_status_refresh_interval_ms(),
        },
        "regionRouting": build_region_routing_payload(),
        "config": read_persistent_config(),
    }))
}

async fn build_wallet_status_payload(
    requested_wallet: &str,
    strict_wallet_selection: bool,
) -> Result<Value, String> {
    let raw_wallets = list_solana_env_wallets();
    let selected_wallet_key =
        resolve_selected_wallet_key(requested_wallet, strict_wallet_selection, &raw_wallets)?;
    let rpc_url = configured_rpc_url();
    let wallets = enrich_wallet_statuses(&rpc_url, USD1_MINT, &raw_wallets).await;
    let selected_wallet = selected_wallet_key.as_ref().and_then(|env_key| {
        wallets
            .iter()
            .find(|wallet| wallet.envKey == *env_key)
            .cloned()
    });
    Ok(json!({
        "ok": true,
        "rpcUrl": rpc_url,
        "connected": selected_wallet.is_some(),
        "wallets": wallets,
        "selectedWalletKey": selected_wallet_key,
        "wallet": selected_wallet.as_ref().and_then(|wallet| wallet.publicKey.clone()),
        "balanceLamports": selected_wallet.as_ref().and_then(|wallet| wallet.balanceLamports),
        "balanceSol": selected_wallet.as_ref().and_then(|wallet| wallet.balanceSol),
        "usd1Balance": selected_wallet.as_ref().and_then(|wallet| wallet.usd1Balance),
    }))
}

fn build_launchpad_backend_status_payload() -> Value {
    let launchpad_actions: [(&str, &[&str]); 3] = [
        (
            "pump",
            &["startup-warm", "quote", "market-snapshot", "build-launch"],
        ),
        (
            "bonk",
            &[
                "startup-warm",
                "quote",
                "market-snapshot",
                "import-context",
                "follow-buy",
                "follow-sell",
                "build-launch",
            ],
        ),
        (
            "bagsapp",
            &[
                "startup-warm",
                "quote",
                "market-snapshot",
                "import-context",
                "fee-recipient-lookup",
                "follow-buy",
                "follow-sell",
                "prepare-launch",
                "build-launch",
            ],
        ),
    ];
    let mut payload = serde_json::Map::new();
    for (launchpad, actions) in launchpad_actions {
        let mut action_payload = serde_json::Map::new();
        for action in actions {
            action_payload.insert(
                action.to_string(),
                json!({
                    "backend": launchpad_action_backend(launchpad, action),
                    "rolloutState": launchpad_action_rollout_state(launchpad, action),
                }),
            );
        }
        payload.insert(launchpad.to_string(), Value::Object(action_payload));
    }
    Value::Object(payload)
}

async fn build_runtime_status_payload(state: &Arc<AppState>) -> Value {
    let (runtime_workers, follow_daemon) =
        tokio::join!(list_workers(&state.runtime), follow_daemon_status_payload(),);
    let follow_active_jobs = follow_daemon
        .get("health")
        .and_then(|value| value.get("activeJobs"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if idle_suspended_without_inflight(state, follow_active_jobs) {
        clear_outbound_provider_http_traffic();
    }
    let warm = warm_state_payload(state, follow_active_jobs);
    let mut diagnostics = runtime_diagnostics_from_warm_payload(&warm);
    let vanity_queues = vanity_pool_status_payload().unwrap_or_else(|error| {
        json!({
            "ok": false,
            "error": error,
        })
    });
    diagnostics.extend(runtime_diagnostics_from_vanity_status(&vanity_queues));
    json!({
        "ok": true,
        "service": "launchdeck-engine",
        "transport": {
            "heliusSenderEndpoint": configured_helius_sender_endpoint(),
            "jitoBundleEndpoints": configured_jito_bundle_endpoints(),
        },
        "followDaemon": follow_daemon,
        "runtime": {
            "statePath": state.runtime.storage_path,
            "workerCount": runtime_workers.len(),
        },
        "launchpads": launchpad_registry(),
        "launchpadBackends": build_launchpad_backend_status_payload(),
        "nativeFallbacks": {
            "bagsapp": bags_runtime_status_payload(),
        },
        "warm": warm,
        "vanityQueues": vanity_queues,
        "diagnostics": diagnostics,
        "autoFee": shared_fee_market_status_payload(shared_fee_market_runtime(&configured_rpc_url()).config()),
        "runtimeWorkers": runtime_workers,
        "rpcTraffic": rpc_traffic_snapshot(),
    })
}

fn diagnostic_host(target: &str) -> Option<String> {
    reqwest::Url::parse(target.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
}

fn runtime_diagnostics_from_warm_payload(warm: &Value) -> Vec<Value> {
    let mut diagnostics = Vec::new();
    for field in ["stateTargets", "endpointTargets", "watchTargets"] {
        let Some(targets) = warm.get(field).and_then(Value::as_array) else {
            continue;
        };
        for target in targets {
            if target.get("active").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let has_error = target
                .get("lastError")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|value| !value.is_empty());
            if !has_error {
                continue;
            }
            let label = target
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("warm target");
            let target_url = target.get("target").and_then(Value::as_str).unwrap_or("");
            let endpoint_kind = if label.eq_ignore_ascii_case("Warm RPC") {
                "warm-rpc"
            } else {
                "provider"
            };
            let env_var = None;
            let recovered_by_primary =
                endpoint_kind == "warm-rpc"
                    && targets.iter().any(|candidate| {
                        candidate.get("label").and_then(Value::as_str).is_some_and(
                            |candidate_label| candidate_label.eq_ignore_ascii_case("Primary RPC"),
                        ) && candidate.get("active").and_then(Value::as_bool) != Some(false)
                            && candidate
                                .get("status")
                                .and_then(Value::as_str)
                                .is_some_and(|status| status.eq_ignore_ascii_case("healthy"))
                            && !candidate
                                .get("lastError")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .is_some_and(|value| !value.is_empty())
                    });
            let host = diagnostic_host(target_url);
            let fingerprint = format!(
                "launchdeck-engine:{}:{}:{}:{}",
                endpoint_kind,
                env_var.unwrap_or(""),
                host.as_deref().unwrap_or(""),
                if recovered_by_primary {
                    "warm_endpoint_failed_primary_used"
                } else {
                    "provider_degraded"
                }
            );
            diagnostics.push(json!({
                "key": fingerprint,
                "fingerprint": fingerprint,
                "severity": if endpoint_kind == "provider" || recovered_by_primary { "warning" } else { "critical" },
                "source": "launchdeck-engine",
                "code": if recovered_by_primary { "warm_endpoint_failed_primary_used" } else if endpoint_kind == "provider" { "provider_degraded" } else { "warm_endpoint_failed" },
                "message": if recovered_by_primary { format!("{label} is degraded; using primary fallback.") } else { format!("{label} is degraded.") },
                "detail": format!("{label} reported an error. Check the configured endpoint or provider key."),
                "envVar": env_var,
                "endpointKind": endpoint_kind,
                "host": host,
                "active": true,
                "restartRequired": endpoint_kind == "warm-rpc" || endpoint_kind == "warm-ws",
                "firstSeenAtMs": target.get("lastErrorAtMs").and_then(Value::as_u64).unwrap_or_else(|| current_time_ms() as u64),
                "lastSeenAtMs": target.get("lastErrorAtMs").and_then(Value::as_u64).unwrap_or_else(|| current_time_ms() as u64),
            }));
        }
    }
    diagnostics
}

fn runtime_diagnostics_from_vanity_status(status: &Value) -> Vec<Value> {
    let mut diagnostics = Vec::new();
    let Some(launchpads) = status.get("launchpads").and_then(Value::as_array) else {
        return diagnostics;
    };
    for launchpad in launchpads {
        let launchpad_name = launchpad
            .get("launchpad")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        for diagnostic in launchpad
            .get("diagnostics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let code = diagnostic
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("invalid");
            if code != "invalid" && code != "duplicate" {
                continue;
            }
            let line = diagnostic
                .get("line")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let message = diagnostic
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Invalid vanity queue entry.");
            diagnostics.push(json!({
                "severity": "warning",
                "source": "vanity-pool",
                "code": format!("vanity_queue_{code}"),
                "message": format!("{launchpad_name}.txt line {line}: {message}"),
                "restartRequired": false,
            }));
        }
    }
    diagnostics
}

fn build_region_routing_payload() -> Value {
    json!({
        "shared": {
            "configured": configured_shared_region(),
            "effective": default_endpoint_profile(),
        },
        "providers": {
            "helius-sender": {
                "configured": configured_provider_region("helius-sender"),
                "effective": default_endpoint_profile_for_provider("helius-sender"),
                "endpointOverrideActive": helius_sender_endpoint_override_active(),
            },
            "jito-bundle": {
                "configured": configured_provider_region("jito-bundle"),
                "effective": default_endpoint_profile_for_provider("jito-bundle"),
                "endpointOverrideActive": jito_bundle_endpoint_override_active(),
            }
        },
        "restartRequired": true,
    })
}

async fn engine_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<StatusRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    authorize(&headers, &state)?;
    build_status_payload(&state, &payload.wallet.unwrap_or_default(), true)
        .await
        .map(Json)
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })
}

async fn execute_engine_action_payload(
    _state: &Arc<AppState>,
    payload: EngineRequest,
) -> Result<Value, (StatusCode, Json<Value>)> {
    let trace = new_trace_context();
    let action_started = Instant::now();
    let action = payload.action.unwrap_or_else(|| "unknown".to_string());
    let benchmark_mode = configured_benchmark_mode();
    let prepare_request_payload_ms = payload.prepare_request_payload_ms.map(u128::from);
    if action == "quote" {
        let form_value = payload.form.ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": "form is required for quote requests.",
                    "traceId": trace.traceId,
                })),
            )
        })?;
        let quote = quote_from_form(&configured_rpc_url(), form_value)
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "error": error,
                        "traceId": trace.traceId,
                    })),
                )
            })?;
        return Ok(json!({
            "ok": true,
            "service": "launchdeck-engine",
            "action": action,
            "implemented": true,
            "quote": quote,
            "traceId": trace.traceId,
            "elapsedMs": current_time_ms().saturating_sub(trace.startedAtMs),
        }));
    }
    let form_prepare_started = Instant::now();
    let (raw_config_value, prepared_metadata_uri, prepared_metadata_warning, form_to_raw_config_ms) =
        if let Some(raw_config_value) = payload.raw_config {
            (raw_config_value, None, None, None)
        } else if let Some(form_value) = payload.form.clone() {
            let (raw_config, metadata_uri, metadata_warning) =
                build_raw_config_from_form(&action, form_value)
                    .await
                    .map_err(|error| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "ok": false,
                                "error": error,
                                "traceId": trace.traceId,
                            })),
                        )
                    })?;
            let raw_config_value = serde_json::to_value(raw_config).map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "error": error.to_string(),
                        "traceId": trace.traceId,
                    })),
                )
            })?;
            (
                raw_config_value,
                metadata_uri,
                metadata_warning,
                Some(form_prepare_started.elapsed().as_millis()),
            )
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": "rawConfig or form is required.",
                    "traceId": trace.traceId,
                })),
            ));
        };
    let parsed: RawConfig = serde_json::from_value(raw_config_value.clone()).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": format!("Invalid rawConfig payload: {error}"),
                "traceId": trace.traceId,
            })),
        )
    })?;
    let normalize_started = Instant::now();
    let mut normalized = normalize_raw_config(parsed).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error.to_string(),
                "traceId": trace.traceId,
            })),
        )
    })?;
    let normalize_config_ms = normalize_started.elapsed().as_millis();
    log_event(
        "engine_action_received",
        &trace.traceId,
        json!({
            "action": action,
            "mode": normalized.mode,
            "launchpad": normalized.launchpad,
            "provider": normalized.execution.provider,
            "walletKey": raw_config_value.get("selectedWalletKey").cloned().unwrap_or(Value::Null),
        }),
    );
    let selected_wallet_key = raw_config_value
        .get("selectedWalletKey")
        .and_then(Value::as_str)
        .and_then(|value| selected_wallet_key_or_default(value))
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": "Creator keypair is required via selected wallet key or SOLANA_PRIVATE_KEY.",
                    "traceId": trace.traceId,
                })),
            )
        })?;
    let wallet_load_started = Instant::now();
    let wallet_secret = load_solana_wallet_by_env_key(&selected_wallet_key).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
                "traceId": trace.traceId,
            })),
        )
    })?;
    let creator_public_key = public_key_from_secret(&wallet_secret).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
                "traceId": trace.traceId,
            })),
        )
    })?;
    let wallet_load_ms = wallet_load_started.elapsed().as_millis();
    let preview_agent_authority = if normalized.mode == "regular" || normalized.mode == "cashback" {
        None
    } else if !normalized.agent.authority.trim().is_empty() {
        Some(normalized.agent.authority.clone())
    } else {
        Some(creator_public_key.clone())
    };
    let transport_plan_started = Instant::now();
    let transport_plan = build_transport_plan(
        &normalized.execution,
        estimate_transaction_count(&normalized),
    );
    let transport_plan_build_ms = transport_plan_started.elapsed().as_millis();
    let rpc_url = configured_rpc_url();
    let auto_fee_started = Instant::now();
    let auto_fee_resolution = resolve_auto_execution_fees(
        &rpc_url,
        &mut normalized,
        &transport_plan,
        &wallet_secret,
        &creator_public_key,
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
                "traceId": trace.traceId,
            })),
        )
    })?;
    let auto_fee_resolve_ms = auto_fee_started.elapsed().as_millis();
    let same_time_guard_started = Instant::now();
    let same_time_fee_guard_warning =
        apply_same_time_creation_fee_guard(&mut normalized).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                    "traceId": trace.traceId,
                })),
            )
        })?;
    let same_time_fee_guard_ms = same_time_guard_started.elapsed().as_millis();
    let should_persist_report = normalized.tx.writeReport || action == "send";
    if action == "build" {
        let report_build_started = Instant::now();
        let report = build_report(
            &normalized,
            &transport_plan,
            now_timestamp_string(),
            configured_rpc_url(),
            creator_public_key.clone(),
            synthetic_mint_address(&trace.traceId),
            preview_agent_authority,
            Some("Rust native build".to_string()),
            vec![],
        );
        let report_build_ms = report_build_started.elapsed().as_millis();
        let mut report_value = serde_json::to_value(&report).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error.to_string(),
                    "traceId": trace.traceId,
                })),
            )
        })?;
        if let Some(warning) = same_time_fee_guard_warning.as_deref() {
            append_execution_warning(&mut report_value, warning);
        }
        set_report_benchmark_mode(&mut report_value, benchmark_mode);
        set_optional_report_timing(
            &mut report_value,
            "prepareRequestPayloadMs",
            prepare_request_payload_ms,
        );
        for note in &auto_fee_resolution.notes {
            append_execution_note(&mut report_value, note);
        }
        if benchmark_mode == BenchmarkMode::Full {
            set_execution_value(
                &mut report_value,
                "autoFee",
                serde_json::to_value(&auto_fee_resolution.report).unwrap_or_else(|_| json!({})),
            );
        }
        if let Some(value) = form_to_raw_config_ms {
            set_report_timing(&mut report_value, "formToRawConfigMs", value);
        }
        set_report_timing(&mut report_value, "normalizeConfigMs", normalize_config_ms);
        set_report_timing(&mut report_value, "walletLoadMs", wallet_load_ms);
        set_report_timing(
            &mut report_value,
            "transportPlanBuildMs",
            transport_plan_build_ms,
        );
        set_report_timing(&mut report_value, "autoFeeResolveMs", auto_fee_resolve_ms);
        set_report_timing(
            &mut report_value,
            "sameTimeFeeGuardMs",
            same_time_fee_guard_ms,
        );
        set_report_timing(&mut report_value, "reportBuildMs", report_build_ms);
        let send_log_path = if should_persist_report {
            let persist_started = Instant::now();
            let path =
                persist_launch_report(&trace.traceId, &action, &transport_plan, &report_value)
                    .map_err(|error| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "ok": false,
                                "error": error,
                                "traceId": trace.traceId,
                            })),
                        )
                    })?;
            set_report_timing(
                &mut report_value,
                "persistReportMs",
                persist_started.elapsed().as_millis(),
            );
            set_report_timing(
                &mut report_value,
                "persistInitialSnapshotMs",
                persist_started.elapsed().as_millis(),
            );
            report_value["outPath"] = Value::String(path.clone());
            Some(path)
        } else {
            None
        };
        let backend_elapsed_ms = action_started.elapsed().as_millis();
        set_report_timing(&mut report_value, "executionTotalMs", backend_elapsed_ms);
        set_report_timing(
            &mut report_value,
            "backendTotalElapsedMs",
            backend_elapsed_ms,
        );
        set_report_timing(&mut report_value, "totalElapsedMs", backend_elapsed_ms);
        refresh_report_benchmark(&mut report_value);
        if let Some(path) = send_log_path.as_ref() {
            let finalize_started = Instant::now();
            update_persisted_launch_report(
                path,
                &trace.traceId,
                &action,
                &transport_plan,
                &report_value,
            )
            .map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "error": format!("Action completed but failed to finalize persisted report: {error}"),
                        "traceId": trace.traceId,
                    })),
                )
            })?;
            set_report_timing(
                &mut report_value,
                "persistFinalReportUpdateMs",
                finalize_started.elapsed().as_millis(),
            );
        }
        log_event(
            "engine_action_completed",
            &trace.traceId,
            json!({
                "action": action,
                "executor": "rust-native",
            }),
        );
        return Ok(json!({
            "ok": true,
            "service": "launchdeck-engine",
            "action": action,
            "implemented": true,
            "executionImplemented": true,
            "executor": "rust-native",
            "message": "Rust engine validated the request and built a native planning report.",
            "traceId": trace.traceId,
            "elapsedMs": current_time_ms().saturating_sub(trace.startedAtMs),
            "receivedForm": payload.form.is_some(),
            "receivedRawConfig": true,
            "normalizedConfig": redacted_normalized_config(&normalized),
            "transportPlan": transport_plan,
            "report": report_value,
            "sendLogPath": send_log_path,
            "text": render_report_value(&report_value),
            "metadataUri": response_metadata_uri(prepared_metadata_uri, &report_value),
            "metadataWarning": prepared_metadata_warning,
        }));
    }
    let compile_started_ms = current_time_ms();
    let (launch_blockhash_prime, warm_report) = if action == "send" || action == "simulate" {
        match build_launchpad_warm_context(&configured_rpc_url(), &normalized.execution.commitment)
            .await
        {
            Ok((ctx, report)) => {
                let prime = if ctx.blockhash.is_empty() {
                    None
                } else {
                    Some((ctx.blockhash, ctx.last_valid_block_height))
                };
                (prime, report)
            }
            Err(error) => {
                record_warn(
                    "launch-warm",
                    format!("Launch warm context failed (continuing): {error}"),
                    Some(json!({ "traceId": trace.traceId })),
                );
                (None, launchpad_warm_env_snapshot())
            }
        }
    } else {
        (None, LaunchpadWarmBuildReport::default())
    };
    let native_artifacts = compile_native_launch(NativeLaunchCompileRequest {
        rpc_url: &configured_rpc_url(),
        config: &normalized,
        transport_plan: &transport_plan,
        wallet_secret: &wallet_secret,
        built_at: now_timestamp_string(),
        creator_public_key: creator_public_key.clone(),
        config_path: Some("Rust native compile".to_string()),
        allow_ata_creation: action == "send",
        launch_blockhash_prime,
    })
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
                "traceId": trace.traceId,
            })),
        )
    })?;
    let compile_transactions_ms = current_time_ms().saturating_sub(compile_started_ms);
    let native = native_artifacts.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": format!(
                    "Native Rust engine does not support launchpad={} mode={} yet. JavaScript compile fallback has been removed.",
                    normalized.launchpad,
                    normalized.mode
                ),
                "traceId": trace.traceId,
            })),
        )
    })?;
    let NativeLaunchArtifacts {
        mut compiled_transactions,
        creation_transactions,
        deferred_setup_transactions,
        setup_bundles,
        setup_transactions,
        bags_launch_follow,
        bags_config_key,
        bags_metadata_uri,
        bags_prepare_launch_ms,
        bags_metadata_upload_ms,
        bags_fee_recipient_resolve_ms,
        report,
        text,
        compile_timings: compile_breakdown,
        mint: compiled_mint,
        launch_creator: compiled_launch_creator,
        vanity_reservation,
        bags_fee_estimate: _,
    } = native;
    let mut compiled_transaction_wallet_keys =
        vec![selected_wallet_key.clone(); compiled_transactions.len()];
    let creation_transaction_wallet_keys =
        vec![selected_wallet_key.clone(); creation_transactions.len()];
    let mut report_value = report;
    let text_value = Value::String(text);
    let assembly_executor = "rust-native".to_string();
    if let Some(warning) = same_time_fee_guard_warning.as_deref() {
        append_execution_warning(&mut report_value, warning);
    }
    for note in &auto_fee_resolution.notes {
        append_execution_note(&mut report_value, note);
    }
    if normalized.launchpad == "bagsapp"
        && (!setup_bundles.is_empty() || !setup_transactions.is_empty())
    {
        append_execution_note(
            &mut report_value,
            "LaunchDeck-created Bags trading is pinned to the canonical local DBC/DAMM path; hosted Bags trade fallback is disabled for this launch.",
        );
    }
    if benchmark_mode == BenchmarkMode::Full {
        set_execution_value(
            &mut report_value,
            "autoFee",
            serde_json::to_value(&auto_fee_resolution.report).unwrap_or_else(|_| json!({})),
        );
    }
    set_report_timing(
        &mut report_value,
        "compileTransactionsMs",
        compile_transactions_ms,
    );
    set_report_timing(
        &mut report_value,
        "launchpadWarmContextBuildMs",
        warm_report.build_ms,
    );
    set_report_timing(
        &mut report_value,
        "launchpadWarmBlockhashPrimeMs",
        warm_report.blockhash_fetch_ms,
    );
    if let Some(exec) = report_value.get_mut("execution") {
        exec["launchpadBackend"] = json!(launchpad_action_backend(
            &normalized.launchpad,
            "build-launch"
        ));
        exec["launchpadRolloutState"] = json!(launchpad_action_rollout_state(
            &normalized.launchpad,
            "build-launch"
        ));
        exec["launchpadWarmContextEnabled"] = json!(warm_report.warm_context_enabled);
        exec["launchpadWarmParallelFetchEnabled"] = json!(warm_report.parallel_enabled);
        exec["launchpadWarmMaxParallelFetches"] = json!(warm_report.max_parallel_warm_fetches);
    }
    set_report_timing(
        &mut report_value,
        "compileLaunchCreatorPrepMs",
        compile_breakdown.launch_creator_prep_ms,
    );
    set_report_timing(
        &mut report_value,
        "compileAltLoadMs",
        compile_breakdown.alt_load_ms,
    );
    set_report_timing(
        &mut report_value,
        "compileBlockhashFetchMs",
        compile_breakdown.blockhash_fetch_ms,
    );
    set_optional_report_timing(
        &mut report_value,
        "compileGlobalFetchMs",
        compile_breakdown.global_fetch_ms,
    );
    set_optional_report_timing(
        &mut report_value,
        "compileFollowUpPrepMs",
        compile_breakdown.follow_up_prep_ms,
    );
    set_report_timing(
        &mut report_value,
        "compileTxSerializeMs",
        compile_breakdown.tx_serialize_ms,
    );
    set_optional_report_timing(
        &mut report_value,
        "compileLaunchSerializeMs",
        compile_breakdown.launch_serialize_ms,
    );
    set_optional_report_timing(
        &mut report_value,
        "compileFollowUpSerializeMs",
        compile_breakdown.follow_up_serialize_ms,
    );
    set_optional_report_timing(
        &mut report_value,
        "compileTipSerializeMs",
        compile_breakdown.tip_serialize_ms,
    );
    set_optional_report_timing(
        &mut report_value,
        "bagsPrepareLaunchMs",
        bags_prepare_launch_ms,
    );
    set_optional_report_timing(
        &mut report_value,
        "bagsMetadataUploadMs",
        bags_metadata_upload_ms,
    );
    set_optional_report_timing(
        &mut report_value,
        "bagsFeeRecipientResolveMs",
        bags_fee_recipient_resolve_ms,
    );

    if action == "simulate" {
        let mut simulation_transactions = compiled_transactions.clone();
        if normalized.launchpad == "bagsapp"
            && (!setup_bundles.is_empty() || !setup_transactions.is_empty())
        {
            let launch_compiled = compile_bags_launch_transaction(
                &configured_rpc_url(),
                &normalized,
                &wallet_secret,
                &compiled_mint,
                &bags_config_key,
                &bags_metadata_uri,
            )
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "error": format!("Bags launch transaction build failed: {error}"),
                        "traceId": trace.traceId,
                    })),
                )
            })?;
            let launch_transaction = maybe_wrap_launch_dev_buy_transaction(
                &configured_rpc_url(),
                &normalized,
                &wallet_secret,
                launch_compiled.compiled_transaction,
            )
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "error": format!("Bags launch wrapper failed: {error}"),
                        "traceId": trace.traceId,
                    })),
                )
            })?;
            if let Some(transactions) = report_value
                .get_mut("transactions")
                .and_then(Value::as_array_mut)
            {
                let mut launch_summaries = serde_json::to_value(summarize_bags_transactions(
                    std::slice::from_ref(&launch_transaction),
                    normalized.tx.dumpBase64,
                ))
                .unwrap_or_else(|_| Value::Array(vec![]));
                if let Some(items) = launch_summaries.as_array_mut() {
                    transactions.append(items);
                }
            }
            simulation_transactions.push(launch_transaction);
        }
        let simulate_started_ms = current_time_ms();
        let (simulation, warnings) = simulate_transactions(
            &configured_rpc_url(),
            &simulation_transactions,
            &normalized.execution.commitment,
        )
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                    "traceId": trace.traceId,
                })),
            )
        })?;
        let mut report = report_value;
        set_report_benchmark_mode(&mut report, benchmark_mode);
        set_optional_report_timing(
            &mut report,
            "prepareRequestPayloadMs",
            prepare_request_payload_ms,
        );
        if let Some(value) = form_to_raw_config_ms {
            set_report_timing(&mut report, "formToRawConfigMs", value);
        }
        set_report_timing(&mut report, "normalizeConfigMs", normalize_config_ms);
        set_report_timing(&mut report, "walletLoadMs", wallet_load_ms);
        set_report_timing(&mut report, "transportPlanBuildMs", transport_plan_build_ms);
        set_report_timing(&mut report, "autoFeeResolveMs", auto_fee_resolve_ms);
        set_report_timing(&mut report, "sameTimeFeeGuardMs", same_time_fee_guard_ms);
        set_report_timing(
            &mut report,
            "compileTransactionsMs",
            compile_transactions_ms,
        );
        set_report_timing(
            &mut report,
            "compileLaunchCreatorPrepMs",
            compile_breakdown.launch_creator_prep_ms,
        );
        set_report_timing(
            &mut report,
            "compileAltLoadMs",
            compile_breakdown.alt_load_ms,
        );
        set_report_timing(
            &mut report,
            "compileBlockhashFetchMs",
            compile_breakdown.blockhash_fetch_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileGlobalFetchMs",
            compile_breakdown.global_fetch_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileFollowUpPrepMs",
            compile_breakdown.follow_up_prep_ms,
        );
        set_report_timing(
            &mut report,
            "compileTxSerializeMs",
            compile_breakdown.tx_serialize_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileLaunchSerializeMs",
            compile_breakdown.launch_serialize_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileFollowUpSerializeMs",
            compile_breakdown.follow_up_serialize_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileTipSerializeMs",
            compile_breakdown.tip_serialize_ms,
        );
        set_report_timing(
            &mut report,
            "launchpadWarmContextBuildMs",
            warm_report.build_ms,
        );
        set_report_timing(
            &mut report,
            "launchpadWarmBlockhashPrimeMs",
            warm_report.blockhash_fetch_ms,
        );
        set_report_timing(
            &mut report,
            "simulateMs",
            current_time_ms().saturating_sub(simulate_started_ms),
        );
        if let Some(execution) = report.get_mut("execution") {
            execution["launchpadWarmContextEnabled"] = json!(warm_report.warm_context_enabled);
            execution["launchpadWarmParallelFetchEnabled"] = json!(warm_report.parallel_enabled);
            execution["launchpadWarmMaxParallelFetches"] =
                json!(warm_report.max_parallel_warm_fetches);
            execution["simulation"] =
                serde_json::to_value(simulation).unwrap_or(Value::Array(vec![]));
            let mut existing_warnings = execution
                .get("warnings")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            existing_warnings.extend(warnings.into_iter().map(Value::String));
            execution["warnings"] = Value::Array(existing_warnings);
        }
        let send_log_path = if should_persist_report {
            let persist_started = Instant::now();
            let path = persist_launch_report(&trace.traceId, &action, &transport_plan, &report)
                .map_err(|error| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "ok": false,
                            "error": error,
                            "traceId": trace.traceId,
                        })),
                    )
                })?;
            set_report_timing(
                &mut report,
                "persistReportMs",
                persist_started.elapsed().as_millis(),
            );
            set_report_timing(
                &mut report,
                "persistInitialSnapshotMs",
                persist_started.elapsed().as_millis(),
            );
            report["outPath"] = Value::String(path.clone());
            Some(path)
        } else {
            None
        };
        let backend_elapsed_ms = action_started.elapsed().as_millis();
        set_report_timing(&mut report, "executionTotalMs", backend_elapsed_ms);
        set_report_timing(&mut report, "backendTotalElapsedMs", backend_elapsed_ms);
        set_report_timing(&mut report, "totalElapsedMs", backend_elapsed_ms);
        refresh_report_benchmark(&mut report);
        if let Some(path) = send_log_path.as_ref() {
            let finalize_started = Instant::now();
            update_persisted_launch_report(
                path,
                &trace.traceId,
                &action,
                &transport_plan,
                &report,
            )
            .map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "error": format!("Action completed but failed to finalize persisted report: {error}"),
                        "traceId": trace.traceId,
                    })),
                )
            })?;
            set_report_timing(
                &mut report,
                "persistFinalReportUpdateMs",
                finalize_started.elapsed().as_millis(),
            );
        }
        log_event(
            "engine_action_completed",
            &trace.traceId,
            json!({
                "action": action,
                "executor": "rust-rpc",
                "assemblyExecutor": assembly_executor,
            }),
        );
        return Ok(json!({
            "ok": true,
            "service": "launchdeck-engine",
            "action": action,
            "implemented": true,
            "executionImplemented": true,
            "executor": "rust-rpc",
            "message": "Rust engine validated the request, compiled transactions natively, and simulated them through native Rust RPC.",
            "traceId": trace.traceId,
            "elapsedMs": current_time_ms().saturating_sub(trace.startedAtMs),
            "receivedForm": payload.form.is_some(),
            "receivedRawConfig": true,
            "normalizedConfig": redacted_normalized_config(&normalized),
            "transportPlan": transport_plan,
            "assemblyExecutor": assembly_executor,
            "report": report,
            "sendLogPath": send_log_path,
            "text": render_report_value(&report),
            "metadataUri": response_metadata_uri(prepared_metadata_uri, &report),
            "metadataWarning": prepared_metadata_warning,
        }));
    }

    if action == "send" {
        let execution_class = transport_plan.executionClass.clone();
        let secure_hellomoon_bundle_transport = transport_plan.transportType == "hellomoon-bundle";
        let use_phased_follow_pipeline = matches!(normalized.launchpad.as_str(), "pump" | "bonk");
        let (same_time_snipes, deferred_follow_launch) =
            split_same_time_snipes(&normalized.followLaunch);
        let all_same_time_retry_enabled = same_time_snipes.iter().all(|snipe| snipe.retryOnFailure);
        let follow_daemon_transport = configured_follow_daemon_transport().map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                    "traceId": trace.traceId,
                })),
            )
        })?;
        let effective_deferred_setup_transactions = if secure_hellomoon_bundle_transport {
            vec![]
        } else {
            deferred_setup_transactions.clone()
        };
        let should_reserve_deferred_follow_job = should_reserve_follow_job(
            &deferred_follow_launch,
            &effective_deferred_setup_transactions,
        );
        let follow_daemon_client = if should_reserve_deferred_follow_job {
            Some(FollowDaemonClient::new(&configured_follow_daemon_base_url()))
        } else {
            None
        };
        let mut reserved_follow_job: Option<FollowJobResponse> = None;
        let mut armed_follow_job: Option<FollowJobResponse> = None;
        let mut same_time_sniper_compile_ms = 0u128;
        let mut same_time_independent_compiled: Vec<CompiledTransaction> = Vec::new();
        let mut same_time_independent_wallet_keys: Vec<String> = Vec::new();
        let mut same_time_transport_plan: Option<TransportPlan> = None;
        let mut same_time_sent: Vec<crate::rpc::SentResult> = Vec::new();
        let mut post_send_warnings: Vec<String> = Vec::new();
        let mut send_phase_errors: Vec<String> = Vec::new();
        let mut launch_confirmed = false;
        let mut same_time_failure_count = 0usize;
        let mut report_persisted = !should_persist_report;
        let mut report_finalized = !should_persist_report;
        let bags_same_time_compile_after_launch =
            normalized.launchpad == "bagsapp" && !same_time_snipes.is_empty();
        let rpc_url = configured_rpc_url();
        let mut bags_setup_sent: Vec<crate::rpc::SentResult> = Vec::new();
        let mut bags_setup_warnings: Vec<String> = Vec::new();
        let mut bags_setup_submit_ms = 0u128;
        let mut bags_setup_gate_ms = 0u128;
        let mut bags_launch_build_ms = 0u128;
        let bags_confirm_timeout_secs = configured_bags_setup_confirm_timeout_secs();
        let bags_setup_gate_commitment = configured_bags_setup_gate_commitment();
        let mut launch_transport_confirm_ms = 0u128;
        let mut follow_reserve_ms = 0u128;
        let mut follow_arm_ms = 0u128;
        let must_complete_follow_reserve_before_send = deferred_follow_launch
            .devAutoSell
            .as_ref()
            .and_then(|sell| sell.marketCap.as_ref())
            .is_some();
        let prebuilt_follow_actions =
            if use_phased_follow_pipeline && deferred_follow_launch.enabled {
                Some(
                    compile_presigned_follow_actions(
                        &rpc_url,
                        &normalized,
                        &compiled_mint,
                        &compiled_launch_creator,
                        true,
                    )
                    .await
                    .map_err(|error| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "ok": false,
                                "error": format!("Pre-signing follow actions failed: {error}"),
                                "traceId": trace.traceId,
                            })),
                        )
                    })?,
                )
            } else {
                None
            };
        let mut reserve_follow_job_task = if let Some(client) = follow_daemon_client.as_ref() {
            let client = client.clone();
            let trace_id = trace.traceId.clone();
            let launchpad = normalized.launchpad.clone();
            let quote_asset = normalized.quoteAsset.clone();
            let launch_mode = normalized.mode.clone();
            let selected_wallet_key = normalized.selectedWalletKey.clone();
            let follow_launch = deferred_follow_launch.clone();
            let execution = normalized.execution.clone();
            let token_mayhem_mode = normalized.token.mayhemMode;
            let jito_tip_account = normalized.tx.jitoTipAccount.clone();
            let buy_tip_account = pick_tip_account_for_provider(&normalized.execution.buyProvider);
            let sell_tip_account =
                pick_tip_account_for_provider(&normalized.execution.sellProvider);
            let prefer_post_setup_creator_vault_for_sell =
                prefers_post_setup_creator_vault_for_follow(&normalized);
            let bags_launch = bags_launch_follow.clone();
            let prebuilt_actions = prebuilt_follow_actions.clone().unwrap_or_default();
            let deferred_setup_transactions = effective_deferred_setup_transactions.clone();
            Some(tokio::spawn(async move {
                let follow_reserve_started = Instant::now();
                let reserved = client
                    .reserve(&FollowReserveRequest {
                        traceId: trace_id,
                        launchpad,
                        quoteAsset: quote_asset,
                        launchMode: launch_mode,
                        selectedWalletKey: selected_wallet_key,
                        followLaunch: follow_launch,
                        execution,
                        tokenMayhemMode: token_mayhem_mode,
                        jitoTipAccount: jito_tip_account,
                        buyTipAccount: buy_tip_account,
                        sellTipAccount: sell_tip_account,
                        preferPostSetupCreatorVaultForSell:
                            prefer_post_setup_creator_vault_for_sell,
                        wrapperDefaultFeeBps: normalized.wrapperDefaultFeeBps,
                        bagsLaunch: bags_launch,
                        prebuiltActions: prebuilt_actions,
                        deferredSetupTransactions: deferred_setup_transactions,
                    })
                    .await?;
                Ok::<_, String>((reserved, follow_reserve_started.elapsed().as_millis()))
            }))
        } else {
            None
        };
        if normalized.launchpad == "bagsapp"
            && (!setup_bundles.is_empty() || !setup_transactions.is_empty())
        {
            let bags_submit_transport_plan = standard_rpc_transport_plan(&transport_plan, &rpc_url);
            for bundle in &setup_bundles {
                let (mut bundle_sent, bundle_warnings, bundle_timing) =
                    match send_transactions_sequential_for_transport(
                        &rpc_url,
                        &bags_submit_transport_plan,
                        bundle,
                        &bags_setup_gate_commitment,
                        false,
                        normalized.execution.trackSendBlockHeight,
                        Some(bags_confirm_timeout_secs),
                    )
                    .await
                    {
                        Ok(value) => value,
                        Err(error) => {
                            let mut diagnostics = Vec::new();
                            for bundle_transaction in bundle {
                                diagnostics.push(format!(
                                    "{} => {}",
                                    bundle_transaction.label,
                                    collect_bags_setup_transaction_diagnostics(
                                        &rpc_url,
                                        &bags_setup_gate_commitment,
                                        bundle_transaction,
                                        None,
                                    )
                                    .await
                                ));
                            }
                            let diagnostics = diagnostics.join(" || ");
                            cancel_reserved_follow_job_on_launch_failure(
                                follow_daemon_client.as_ref(),
                                &mut reserve_follow_job_task,
                                &trace.traceId,
                                &format!(
                                    "Bags setup bundle execution failed: {error} | diagnostics: {diagnostics}",
                                ),
                            )
                            .await;
                            return Err((
                                StatusCode::BAD_REQUEST,
                                Json(json!({
                                    "ok": false,
                                    "error": format!(
                                        "Bags setup bundle execution failed: {error} | diagnostics: {diagnostics}"
                                    ),
                                    "traceId": trace.traceId,
                                })),
                            ));
                        }
                    };
                bags_setup_submit_ms += bundle_timing.submit_ms;
                bags_setup_gate_ms += bundle_timing.confirm_ms;
                bags_setup_warnings.extend(bundle_warnings);
                bags_setup_sent.append(&mut bundle_sent);
            }
            if !setup_transactions.is_empty() {
                let setup_batch = setup_transactions.clone();
                let setup_labels = setup_batch
                    .iter()
                    .map(|transaction| transaction.label.clone())
                    .collect::<Vec<_>>();
                let setup_label_summary = setup_labels.join(", ");
                let (mut setup_sent, setup_warnings, setup_timing) =
                    match send_transactions_sequential_for_transport(
                        &rpc_url,
                        &bags_submit_transport_plan,
                        &setup_batch,
                        &bags_setup_gate_commitment,
                        false,
                        normalized.execution.trackSendBlockHeight,
                        Some(bags_confirm_timeout_secs),
                    )
                    .await
                    {
                        Ok(value) => value,
                        Err(error) => {
                            let mut diagnostics = Vec::new();
                            for setup_transaction in &setup_batch {
                                diagnostics.push(format!(
                                    "{} => {}",
                                    setup_transaction.label,
                                    collect_bags_setup_transaction_diagnostics(
                                        &rpc_url,
                                        &bags_setup_gate_commitment,
                                        setup_transaction,
                                        None,
                                    )
                                    .await
                                ));
                            }
                            let diagnostics = diagnostics.join(" || ");
                            cancel_reserved_follow_job_on_launch_failure(
                                follow_daemon_client.as_ref(),
                                &mut reserve_follow_job_task,
                                &trace.traceId,
                                &format!(
                                    "Bags sequential setup execution failed for [{setup_label_summary}]: {error} | diagnostics: {diagnostics}",
                                ),
                            )
                            .await;
                            return Err((
                                StatusCode::BAD_REQUEST,
                                Json(json!({
                                    "ok": false,
                                    "error": format!(
                                        "Bags sequential setup execution failed for [{setup_label_summary}]: {error} | diagnostics: {diagnostics}"
                                    ),
                                    "traceId": trace.traceId,
                                })),
                            ));
                        }
                    };
                for warning in &setup_warnings {
                    if warning.contains("Websocket batch confirmation failed via") {
                        record_warn(
                            "bags-send",
                            format!(
                                "Bags setup gate fell back to RPC polling for [{}] at {}",
                                setup_label_summary, bags_setup_gate_commitment
                            ),
                            Some(json!({
                                "traceId": trace.traceId,
                                "labels": setup_labels,
                                "gateCommitment": bags_setup_gate_commitment,
                                "watchEndpoint": transport_plan.watchEndpoint.as_deref().or_else(|| {
                                    transport_plan.watchEndpoints.first().map(String::as_str)
                                }),
                                "warning": warning,
                            })),
                        );
                    }
                }
                bags_setup_submit_ms += setup_timing.submit_ms;
                bags_setup_gate_ms += setup_timing.confirm_ms;
                bags_setup_warnings.extend(setup_warnings);
                if setup_sent.is_empty() {
                    let mut diagnostics = Vec::new();
                    for setup_transaction in &setup_batch {
                        diagnostics.push(format!(
                            "{} => {}",
                            setup_transaction.label,
                            collect_bags_setup_transaction_diagnostics(
                                &rpc_url,
                                &bags_setup_gate_commitment,
                                setup_transaction,
                                None,
                            )
                            .await
                        ));
                    }
                    let diagnostics = diagnostics.join(" || ");
                    cancel_reserved_follow_job_on_launch_failure(
                        follow_daemon_client.as_ref(),
                        &mut reserve_follow_job_task,
                        &trace.traceId,
                        &format!(
                            "Bags sequential setup execution failed for [{setup_label_summary}]: no transactions were returned after sequential send | diagnostics: {}",
                            diagnostics
                        ),
                    )
                    .await;
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "ok": false,
                            "error": format!(
                                "Bags sequential setup execution failed for [{setup_label_summary}]: no transactions were returned after sequential send | diagnostics: {}",
                                diagnostics
                            ),
                            "traceId": trace.traceId,
                        })),
                    ));
                }
                bags_setup_sent.append(&mut setup_sent);
            }
            let launch_compiled = match compile_bags_launch_transaction(
                &rpc_url,
                &normalized,
                &wallet_secret,
                &compiled_mint,
                &bags_config_key,
                &bags_metadata_uri,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => {
                    cancel_reserved_follow_job_on_launch_failure(
                        follow_daemon_client.as_ref(),
                        &mut reserve_follow_job_task,
                        &trace.traceId,
                        &format!("Bags launch transaction build failed: {error}"),
                    )
                    .await;
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "ok": false,
                            "error": format!("Bags launch transaction build failed: {error}"),
                            "traceId": trace.traceId,
                        })),
                    ));
                }
            };
            bags_launch_build_ms = launch_compiled.launch_build_ms.unwrap_or_default();
            let launch_transaction = match maybe_wrap_launch_dev_buy_transaction(
                &rpc_url,
                &normalized,
                &wallet_secret,
                launch_compiled.compiled_transaction,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => {
                    cancel_reserved_follow_job_on_launch_failure(
                        follow_daemon_client.as_ref(),
                        &mut reserve_follow_job_task,
                        &trace.traceId,
                        &format!("Bags launch wrapper failed: {error}"),
                    )
                    .await;
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "ok": false,
                            "error": format!("Bags launch wrapper failed: {error}"),
                            "traceId": trace.traceId,
                        })),
                    ));
                }
            };
            compiled_transactions = vec![launch_transaction];
            compiled_transaction_wallet_keys =
                vec![selected_wallet_key.clone(); compiled_transactions.len()];
            if let Some(transactions) = report_value
                .get_mut("transactions")
                .and_then(Value::as_array_mut)
            {
                let mut launch_summaries = serde_json::to_value(summarize_bags_transactions(
                    &compiled_transactions,
                    normalized.tx.dumpBase64,
                ))
                .unwrap_or_else(|_| Value::Array(vec![]));
                if let Some(items) = launch_summaries.as_array_mut() {
                    transactions.append(items);
                }
            }
        }
        if use_phased_follow_pipeline && !secure_hellomoon_bundle_transport {
            compiled_transactions = creation_transactions.clone();
            compiled_transaction_wallet_keys = creation_transaction_wallet_keys.clone();
        }
        if !same_time_snipes.is_empty() && !bags_same_time_compile_after_launch {
            let same_time_compile_started_ms = current_time_ms();
            let same_time_compiled = compile_same_time_snipes(
                &rpc_url,
                &normalized,
                &compiled_mint,
                &compiled_launch_creator,
                &same_time_snipes,
                action == "send",
                None,
            )
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "error": format!("Same-time sniper compile failed: {error}"),
                        "traceId": trace.traceId,
                    })),
                )
            })?;
            same_time_sniper_compile_ms =
                current_time_ms().saturating_sub(same_time_compile_started_ms);
            let same_time_plan =
                build_buy_transport_plan(&normalized.execution, same_time_compiled.len());
            if transport_plan.transportType == "jito-bundle"
                && same_time_plan.transportType == "jito-bundle"
            {
                compiled_transaction_wallet_keys.extend(
                    same_time_snipes
                        .iter()
                        .map(|snipe| snipe.walletEnvKey.clone()),
                );
                compiled_transactions.extend(same_time_compiled);
            } else {
                same_time_transport_plan = Some(same_time_plan);
                same_time_independent_wallet_keys = same_time_snipes
                    .iter()
                    .map(|snipe| snipe.walletEnvKey.clone())
                    .collect();
                same_time_independent_compiled = same_time_compiled;
            }
        }
        if must_complete_follow_reserve_before_send {
            if let Some(task) = reserve_follow_job_task.take() {
                let (reserved, elapsed_ms) = task
                    .await
                    .map_err(|error| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "ok": false,
                                "error": format!("Follow daemon reservation task failed: {error}"),
                                "traceId": trace.traceId,
                            })),
                        )
                    })?
                    .map_err(|error| {
                        record_error(
                            "follow-client",
                            "Follow daemon reservation failed.",
                            Some(json!({
                                "traceId": trace.traceId,
                                "message": error,
                            })),
                        );
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "ok": false,
                                "error": format!("Follow daemon reservation failed: {error}"),
                                "traceId": trace.traceId,
                            })),
                        )
                    })?;
                follow_reserve_ms = elapsed_ms;
                reserved_follow_job = Some(reserved);
            }
        }
        let launch_transaction_subscribe_account_required = compiled_transactions
            .first()
            .map(|transaction| {
                derive_helius_transaction_subscribe_account_required(&transaction.serializedBase64)
            })
            .unwrap_or_default();
        mark_vanity_reservation_used(vanity_reservation.as_ref(), None).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": format!("Failed to mark queued vanity mint as used before submit: {error}"),
                    "traceId": trace.traceId.clone(),
                })),
            )
        })?;
        let submit_started_ms = current_time_ms();
        let (mut launch_sent, mut warnings, submit_ms) = if same_time_independent_compiled
            .is_empty()
        {
            if normalized.launchpad == "bagsapp" {
                match send_transactions_sequential_for_transport(
                    &rpc_url,
                    &transport_plan,
                    &compiled_transactions,
                    &normalized.execution.commitment,
                    false,
                    normalized.execution.trackSendBlockHeight,
                    Some(bags_confirm_timeout_secs),
                )
                .await
                {
                    Ok((sent, warnings, timing)) => {
                        launch_confirmed = true;
                        launch_transport_confirm_ms = timing.confirm_ms;
                        (sent, warnings, timing.submit_ms)
                    }
                    Err(error) => {
                        cancel_reserved_follow_job_on_launch_failure(
                            follow_daemon_client.as_ref(),
                            &mut reserve_follow_job_task,
                            &trace.traceId,
                            &format!("Launch submit failed: {error}"),
                        )
                        .await;
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "ok": false,
                                "error": error,
                                "traceId": trace.traceId,
                            })),
                        ));
                    }
                }
            } else {
                match submit_transactions_for_transport(
                    &rpc_url,
                    &transport_plan,
                    &compiled_transactions,
                    &normalized.execution.commitment,
                    normalized.execution.skipPreflight,
                    normalized.execution.trackSendBlockHeight,
                )
                .await
                {
                    Ok(value) => value,
                    Err(error) => {
                        cancel_reserved_follow_job_on_launch_failure(
                            follow_daemon_client.as_ref(),
                            &mut reserve_follow_job_task,
                            &trace.traceId,
                            &format!("Launch submit failed: {error}"),
                        )
                        .await;
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "ok": false,
                                "error": error,
                                "traceId": trace.traceId,
                            })),
                        ));
                    }
                }
            }
        } else if matches!(normalized.launchpad.as_str(), "bonk" | "pump") {
            let (launch_sent, mut launch_warnings, _launch_submit_ms) =
                match submit_transactions_for_transport(
                    &rpc_url,
                    &transport_plan,
                    &compiled_transactions,
                    &normalized.execution.commitment,
                    normalized.execution.skipPreflight,
                    normalized.execution.trackSendBlockHeight,
                )
                .await
                {
                    Ok(value) => value,
                    Err(error) => {
                        cancel_reserved_follow_job_on_launch_failure(
                            follow_daemon_client.as_ref(),
                            &mut reserve_follow_job_task,
                            &trace.traceId,
                            &format!("Launch submit failed: {error}"),
                        )
                        .await;
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "ok": false,
                                "error": error,
                                "traceId": trace.traceId,
                            })),
                        ));
                    }
                };
            launch_warnings.push(format!(
                "{} same-time sniper buys are submitted immediately after the launch transaction on non-bundle transports so the mint exists before buy execution.",
                normalized.launchpad
            ));
            match submit_independent_transactions_for_transport(
                &rpc_url,
                same_time_transport_plan.as_ref().unwrap_or(&transport_plan),
                &same_time_independent_compiled,
                &normalized.execution.commitment,
                normalized.execution.skipPreflight,
                normalized.execution.trackSendBlockHeight,
            )
            .await
            {
                Ok((sent, same_time_warnings, _same_time_submit_ms)) => {
                    same_time_sent = sent;
                    launch_warnings.extend(same_time_warnings);
                }
                Err(error) if all_same_time_retry_enabled => {
                    launch_warnings.push(format!(
                        "Same-time sniper submit failed after launch submit; daemon retry is armed for one retry attempt: {error}"
                    ));
                }
                Err(error) => {
                    same_time_failure_count = same_time_failure_count.saturating_add(1);
                    launch_warnings.push(format!(
                        "Same-time sniper submit failed after launch submit; at least one same-time snipe has no daemon retry fallback: {error}"
                    ));
                }
            }
            (
                launch_sent,
                launch_warnings,
                current_time_ms().saturating_sub(submit_started_ms),
            )
        } else {
            let launch_submit = submit_transactions_for_transport(
                &rpc_url,
                &transport_plan,
                &compiled_transactions,
                &normalized.execution.commitment,
                normalized.execution.skipPreflight,
                normalized.execution.trackSendBlockHeight,
            );
            let same_time_submit = submit_independent_transactions_for_transport(
                &rpc_url,
                same_time_transport_plan.as_ref().unwrap_or(&transport_plan),
                &same_time_independent_compiled,
                &normalized.execution.commitment,
                normalized.execution.skipPreflight,
                normalized.execution.trackSendBlockHeight,
            );
            let (launch_result, same_time_result) = tokio::join!(launch_submit, same_time_submit);
            let (launch_sent, mut launch_warnings, _launch_submit_ms) = match launch_result {
                Ok(value) => value,
                Err(error) => {
                    cancel_reserved_follow_job_on_launch_failure(
                        follow_daemon_client.as_ref(),
                        &mut reserve_follow_job_task,
                        &trace.traceId,
                        &format!("Launch submit failed: {error}"),
                    )
                    .await;
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "ok": false,
                            "error": error,
                            "traceId": trace.traceId,
                        })),
                    ));
                }
            };
            match same_time_result {
                Ok((sent, same_time_warnings, _same_time_submit_ms)) => {
                    same_time_sent = sent;
                    launch_warnings.extend(same_time_warnings);
                }
                Err(error) if all_same_time_retry_enabled => {
                    launch_warnings.push(format!(
                        "Same-time sniper submit failed; daemon retry is armed for one retry attempt: {error}"
                    ));
                }
                Err(error) => {
                    same_time_failure_count = same_time_failure_count.saturating_add(1);
                    launch_warnings.push(format!(
                        "Same-time sniper submit failed; at least one same-time snipe has no daemon retry fallback: {error}"
                    ));
                }
            }
            (
                launch_sent,
                launch_warnings,
                current_time_ms().saturating_sub(submit_started_ms),
            )
        };
        if !bags_setup_warnings.is_empty() {
            let launch_warnings = std::mem::take(&mut warnings);
            warnings = bags_setup_warnings;
            warnings.extend(launch_warnings);
        }
        let launch_signature = match launch_sent
            .first()
            .and_then(|result| result.signature.clone())
        {
            Some(signature) => signature,
            None => {
                cancel_reserved_follow_job_on_launch_failure(
                    follow_daemon_client.as_ref(),
                    &mut reserve_follow_job_task,
                    &trace.traceId,
                    "Launch submit completed without a signature, so follow actions cannot be armed safely.",
                )
                .await;
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "error": "Launch submit completed without a signature, so follow actions cannot be armed safely.",
                        "traceId": trace.traceId,
                    })),
                ));
            }
        };
        let launch_submit_at_ms = submit_started_ms.saturating_add(submit_ms);
        let launch_send_observed_slot = launch_sent
            .first()
            .and_then(|result| result.sendObservedSlot);
        if should_reserve_deferred_follow_job && reserved_follow_job.is_none() {
            if let Some(task) = reserve_follow_job_task.take() {
                match task.await {
                    Ok(Ok((reserved, elapsed_ms))) => {
                        follow_reserve_ms = elapsed_ms;
                        reserved_follow_job = Some(reserved);
                    }
                    Ok(Err(error)) => {
                        record_error(
                            "follow-client",
                            "Follow daemon reservation failed.",
                            Some(json!({
                                "traceId": trace.traceId,
                                "message": error,
                            })),
                        );
                        let warning = format!(
                            "Launch was submitted, but follow daemon reservation failed so follow actions were not armed: {error}"
                        );
                        let _ = cancel_follow_job_best_effort(
                            follow_daemon_client.as_ref(),
                            &trace.traceId,
                            &warning,
                        )
                        .await;
                        post_send_warnings.push(warning.clone());
                        send_phase_errors.push(warning);
                    }
                    Err(error) => {
                        record_error(
                            "follow-client",
                            "Follow daemon reservation task failed after launch submit.",
                            Some(json!({
                                "traceId": trace.traceId,
                                "message": error.to_string(),
                            })),
                        );
                        let warning = format!(
                            "Launch was submitted, but follow daemon reservation task failed so follow actions were not armed: {error}"
                        );
                        let _ = cancel_follow_job_best_effort(
                            follow_daemon_client.as_ref(),
                            &trace.traceId,
                            &warning,
                        )
                        .await;
                        post_send_warnings.push(warning.clone());
                        send_phase_errors.push(warning);
                    }
                }
            }
        }
        if let Some(client) = follow_daemon_client.as_ref()
            && reserved_follow_job.is_some()
        {
            let follow_arm_started = Instant::now();
            match client
                .arm(&FollowArmRequest {
                    traceId: trace.traceId.clone(),
                    mint: compiled_mint.clone(),
                    launchCreator: compiled_launch_creator.clone(),
                    launchSignature: launch_signature.clone(),
                    launchTransactionSubscribeAccountRequired:
                        launch_transaction_subscribe_account_required.clone(),
                    submitAtMs: launch_submit_at_ms,
                    sendObservedSlot: launch_send_observed_slot,
                    confirmedObservedSlot: None,
                    reportPath: None,
                    transportPlan: transport_plan.clone(),
                })
                .await
            {
                Ok(response) => {
                    follow_arm_ms =
                        follow_arm_ms.saturating_add(follow_arm_started.elapsed().as_millis());
                    armed_follow_job = Some(response);
                }
                Err(error) => {
                    follow_arm_ms =
                        follow_arm_ms.saturating_add(follow_arm_started.elapsed().as_millis());
                    let warning = format!(
                        "Follow daemon early arm failed after launch submit; retrying after confirmation: {error}"
                    );
                    record_warn(
                        "follow-client",
                        "Follow daemon early arm failed.",
                        Some(json!({
                            "traceId": trace.traceId,
                            "message": error,
                        })),
                    );
                    post_send_warnings.push(warning);
                }
            }
        }
        if bags_same_time_compile_after_launch {
            let same_time_compile_started_ms = current_time_ms();
            match compile_same_time_snipes(
                &rpc_url,
                &normalized,
                &compiled_mint,
                &compiled_launch_creator,
                &same_time_snipes,
                true,
                bags_launch_follow.as_ref(),
            )
            .await
            {
                Ok(same_time_compiled) => {
                    let same_time_plan =
                        build_buy_transport_plan(&normalized.execution, same_time_compiled.len());
                    same_time_transport_plan = Some(same_time_plan.clone());
                    same_time_independent_wallet_keys = same_time_snipes
                        .iter()
                        .map(|snipe| snipe.walletEnvKey.clone())
                        .collect();
                    same_time_sniper_compile_ms = same_time_sniper_compile_ms.saturating_add(
                        current_time_ms().saturating_sub(same_time_compile_started_ms),
                    );
                    match submit_independent_transactions_for_transport(
                        &rpc_url,
                        &same_time_plan,
                        &same_time_compiled,
                        &normalized.execution.commitment,
                        normalized.execution.skipPreflight,
                        normalized.execution.trackSendBlockHeight,
                    )
                    .await
                    {
                        Ok((sent, same_time_warnings, _same_time_submit_ms)) => {
                            same_time_sent = sent;
                            if let Some(execution) = report_value.get_mut("execution") {
                                let mut existing_warnings = execution
                                    .get("warnings")
                                    .and_then(Value::as_array)
                                    .cloned()
                                    .unwrap_or_default();
                                existing_warnings.push(Value::String(
                                    "Bags same-time snipes are compiled immediately after the launch transaction is confirmed so the trade route can resolve against the live mint."
                                        .to_string(),
                                ));
                                existing_warnings
                                    .extend(same_time_warnings.into_iter().map(Value::String));
                                execution["warnings"] = Value::Array(existing_warnings);
                            }
                        }
                        Err(error) if all_same_time_retry_enabled => {
                            append_execution_warning(
                                &mut report_value,
                                &format!(
                                    "Same-time Bags sniper submit failed after launch confirmation; daemon retry is armed for one retry attempt: {error}"
                                ),
                            );
                        }
                        Err(error) => {
                            same_time_failure_count = same_time_failure_count.saturating_add(1);
                            append_execution_warning(
                                &mut report_value,
                                &format!(
                                    "Same-time Bags sniper submit failed after launch confirmation; at least one same-time snipe has no daemon retry fallback: {error}"
                                ),
                            );
                        }
                    }
                }
                Err(error) => {
                    same_time_failure_count = same_time_failure_count.saturating_add(1);
                    append_execution_warning(
                        &mut report_value,
                        &format!(
                            "Same-time Bags sniper compile failed after launch confirmation: {error}"
                        ),
                    );
                }
            }
        }
        let mut report = report_value;
        set_report_benchmark_mode(&mut report, benchmark_mode);
        set_optional_report_timing(
            &mut report,
            "prepareRequestPayloadMs",
            prepare_request_payload_ms,
        );
        if let Some(value) = form_to_raw_config_ms {
            set_report_timing(&mut report, "formToRawConfigMs", value);
        }
        set_report_timing(&mut report, "normalizeConfigMs", normalize_config_ms);
        set_report_timing(&mut report, "walletLoadMs", wallet_load_ms);
        set_report_timing(&mut report, "transportPlanBuildMs", transport_plan_build_ms);
        set_report_timing(&mut report, "autoFeeResolveMs", auto_fee_resolve_ms);
        set_report_timing(&mut report, "sameTimeFeeGuardMs", same_time_fee_guard_ms);
        set_report_timing(
            &mut report,
            "compileTransactionsMs",
            compile_transactions_ms.saturating_add(same_time_sniper_compile_ms),
        );
        set_report_timing(
            &mut report,
            "compileLaunchCreatorPrepMs",
            compile_breakdown.launch_creator_prep_ms,
        );
        set_report_timing(
            &mut report,
            "compileAltLoadMs",
            compile_breakdown.alt_load_ms,
        );
        set_report_timing(
            &mut report,
            "compileBlockhashFetchMs",
            compile_breakdown.blockhash_fetch_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileGlobalFetchMs",
            compile_breakdown.global_fetch_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileFollowUpPrepMs",
            compile_breakdown.follow_up_prep_ms,
        );
        set_report_timing(
            &mut report,
            "compileTxSerializeMs",
            compile_breakdown.tx_serialize_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileLaunchSerializeMs",
            compile_breakdown.launch_serialize_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileFollowUpSerializeMs",
            compile_breakdown.follow_up_serialize_ms,
        );
        set_optional_report_timing(
            &mut report,
            "compileTipSerializeMs",
            compile_breakdown.tip_serialize_ms,
        );
        set_report_timing(
            &mut report,
            "sendMs",
            bags_setup_submit_ms
                .saturating_add(bags_setup_gate_ms)
                .saturating_add(bags_launch_build_ms)
                .saturating_add(submit_ms)
                .saturating_add(launch_transport_confirm_ms),
        );
        set_report_timing(
            &mut report,
            "sendSubmitMs",
            bags_setup_submit_ms
                .saturating_add(bags_launch_build_ms)
                .saturating_add(submit_ms),
        );
        set_report_timing(&mut report, "sendTransportSubmitMs", submit_ms);
        set_report_timing(&mut report, "sendConfirmMs", launch_transport_confirm_ms);
        set_report_timing(
            &mut report,
            "sendTransportConfirmMs",
            launch_transport_confirm_ms,
        );
        set_optional_report_timing(
            &mut report,
            "followDaemonReserveMs",
            Some(follow_reserve_ms),
        );
        if bags_setup_submit_ms > 0 || bags_setup_gate_ms > 0 {
            set_report_timing(&mut report, "bagsSetupSubmitMs", bags_setup_submit_ms);
            set_report_timing(&mut report, "bagsSetupConfirmMs", bags_setup_gate_ms);
            set_report_timing(&mut report, "bagsSetupGateMs", bags_setup_gate_ms);
        }
        if bags_launch_build_ms > 0 {
            set_report_timing(&mut report, "bagsLaunchBuildMs", bags_launch_build_ms);
        }
        attach_follow_daemon_report(
            &mut report,
            if should_reserve_deferred_follow_job {
                Some(follow_daemon_transport.as_str())
            } else {
                None
            },
            reserved_follow_job.as_ref(),
            None,
            None,
            Some(&normalized.followLaunch),
        );
        let send_log_path = if should_persist_report {
            let persist_started = Instant::now();
            match persist_launch_report(&trace.traceId, &action, &transport_plan, &report) {
                Ok(path) => {
                    report_persisted = true;
                    set_report_timing(
                        &mut report,
                        "persistReportMs",
                        persist_started.elapsed().as_millis(),
                    );
                    set_report_timing(
                        &mut report,
                        "persistInitialSnapshotMs",
                        persist_started.elapsed().as_millis(),
                    );
                    report["outPath"] = Value::String(path.clone());
                    Some(path)
                }
                Err(error) => {
                    let warning = format!(
                        "Launch was submitted, but the initial report could not be persisted: {error}"
                    );
                    post_send_warnings.push(warning.clone());
                    send_phase_errors.push(warning);
                    None
                }
            }
        } else {
            None
        };
        let (mut confirm_warnings, mut confirm_ms) = if launch_confirmed {
            (vec![], launch_transport_confirm_ms)
        } else {
            match confirm_submitted_transactions_for_transport(
                &rpc_url,
                &transport_plan,
                &mut launch_sent,
                &normalized.execution.commitment,
                normalized.execution.trackSendBlockHeight,
            )
            .await
            {
                Ok(value) => {
                    launch_confirmed = true;
                    value
                }
                Err(error) => {
                    if let Some(client) = follow_daemon_client.as_ref() {
                        let _ = client
                            .cancel(&FollowCancelRequest {
                                traceId: trace.traceId.clone(),
                                actionId: None,
                                note: Some(format!("Launch confirmation failed: {error}")),
                            })
                            .await;
                    }
                    let warning = format!(
                        "Launch was submitted, but confirmation failed or remained incomplete: {error}"
                    );
                    post_send_warnings.push(warning.clone());
                    send_phase_errors.push(warning.clone());
                    (vec![warning], 0)
                }
            }
        };
        if launch_confirmed {
            if let Some(client) = follow_daemon_client.as_ref()
                && reserved_follow_job.is_some()
            {
                let confirmed_follow_slot = launch_sent
                    .first()
                    .and_then(|result| result.confirmedSlot.or(result.confirmedObservedSlot));
                let follow_arm_started = Instant::now();
                match client
                    .arm(&FollowArmRequest {
                        traceId: trace.traceId.clone(),
                        mint: compiled_mint.clone(),
                        launchCreator: compiled_launch_creator.clone(),
                        launchSignature: launch_signature.clone(),
                        launchTransactionSubscribeAccountRequired:
                            launch_transaction_subscribe_account_required.clone(),
                        submitAtMs: launch_submit_at_ms,
                        sendObservedSlot: launch_send_observed_slot,
                        confirmedObservedSlot: confirmed_follow_slot,
                        reportPath: send_log_path.clone(),
                        transportPlan: transport_plan.clone(),
                    })
                    .await
                {
                    Ok(response) => {
                        follow_arm_ms =
                            follow_arm_ms.saturating_add(follow_arm_started.elapsed().as_millis());
                        armed_follow_job = Some(response);
                    }
                    Err(error) => {
                        follow_arm_ms =
                            follow_arm_ms.saturating_add(follow_arm_started.elapsed().as_millis());
                        let warning = format!(
                            "Follow daemon confirm-slot update failed after launch confirmation: {error}"
                        );
                        record_warn(
                            "follow-client",
                            "Follow daemon post-confirm arm update failed.",
                            Some(json!({
                                "traceId": trace.traceId,
                                "message": error,
                            })),
                        );
                        if armed_follow_job.is_none()
                            && cancel_follow_job_best_effort(
                                follow_daemon_client.as_ref(),
                                &trace.traceId,
                                &warning,
                            )
                            .await
                        {
                            reserved_follow_job = None;
                        }
                        post_send_warnings.push(warning.clone());
                        send_phase_errors.push(warning);
                    }
                }
            }
        }
        if should_reserve_deferred_follow_job {
            attach_follow_daemon_report(
                &mut report,
                Some(follow_daemon_transport.as_str()),
                reserved_follow_job.as_ref(),
                armed_follow_job.as_ref(),
                None,
                Some(&normalized.followLaunch),
            );
        }
        if !same_time_sent.is_empty() {
            match confirm_submitted_transactions_for_transport(
                &rpc_url,
                same_time_transport_plan.as_ref().unwrap_or(&transport_plan),
                &mut same_time_sent,
                &normalized.execution.commitment,
                normalized.execution.trackSendBlockHeight,
            )
            .await
            {
                Ok((same_time_confirm_warnings, same_time_confirm_ms)) => {
                    confirm_warnings.extend(same_time_confirm_warnings);
                    confirm_ms = confirm_ms.saturating_add(same_time_confirm_ms);
                }
                Err(error) if all_same_time_retry_enabled => {
                    confirm_warnings.push(format!(
                        "Same-time sniper confirmation failed; daemon retry will attempt one fallback buy: {error}"
                    ));
                }
                Err(error) => {
                    if let Some(client) = follow_daemon_client.as_ref() {
                        let _ = client
                            .cancel(&FollowCancelRequest {
                                traceId: trace.traceId.clone(),
                                actionId: None,
                                note: Some(format!(
                                    "Same-time sniper confirmation failed: {error}"
                                )),
                            })
                            .await;
                    }
                    same_time_failure_count = same_time_failure_count.saturating_add(1);
                    confirm_warnings.push(format!(
                        "Same-time sniper confirmation failed or remained incomplete: {error}"
                    ));
                }
            }
        }
        record_launchdeck_coin_trades_best_effort(
            &trace.traceId,
            &launch_sent,
            &compiled_transaction_wallet_keys,
            &compiled_mint,
            "launch-send",
        )
        .await;
        record_launchdeck_coin_trades_best_effort(
            &trace.traceId,
            &same_time_sent,
            &same_time_independent_wallet_keys,
            &compiled_mint,
            "same-time-send",
        )
        .await;
        let mut sent = bags_setup_sent;
        sent.append(&mut launch_sent);
        sent.append(&mut same_time_sent);
        warnings.extend(confirm_warnings);
        let response_warning_count = warnings.len().saturating_add(post_send_warnings.len());
        set_report_timing(
            &mut report,
            "sendMs",
            bags_setup_submit_ms
                .saturating_add(bags_setup_gate_ms)
                .saturating_add(bags_launch_build_ms)
                .saturating_add(submit_ms)
                .saturating_add(confirm_ms),
        );
        set_report_timing(
            &mut report,
            "sendSubmitMs",
            bags_setup_submit_ms
                .saturating_add(bags_launch_build_ms)
                .saturating_add(submit_ms),
        );
        set_report_timing(&mut report, "sendTransportSubmitMs", submit_ms);
        set_report_timing(&mut report, "sendConfirmMs", confirm_ms);
        set_report_timing(&mut report, "sendTransportConfirmMs", confirm_ms);
        set_report_timing(&mut report, "sendCreationSubmitMs", submit_ms);
        set_report_timing(&mut report, "sendCreationConfirmMs", confirm_ms);
        if bags_setup_submit_ms > 0 || bags_setup_gate_ms > 0 {
            set_report_timing(&mut report, "bagsSetupSubmitMs", bags_setup_submit_ms);
            set_report_timing(&mut report, "bagsSetupConfirmMs", bags_setup_gate_ms);
            set_report_timing(&mut report, "bagsSetupGateMs", bags_setup_gate_ms);
        }
        if bags_launch_build_ms > 0 {
            set_report_timing(&mut report, "bagsLaunchBuildMs", bags_launch_build_ms);
        }
        if use_phased_follow_pipeline {
            if secure_hellomoon_bundle_transport {
                append_execution_note(
                    &mut report,
                    "Secure Hello Moon creation bundled the phased setup transactions into the creation bundle instead of deferring them to the daemon.",
                );
            } else {
                append_execution_note(
                    &mut report,
                    "Pump/Bonk phased send path reserved pre-signed follow actions in the daemon and moved setup into deferred post-confirm execution.",
                );
                if !deferred_setup_transactions.is_empty() {
                    append_execution_note(
                        &mut report,
                        &format!(
                            "Deferred post-confirm setup prepared {} transaction(s) for daemon-managed submission.",
                            deferred_setup_transactions.len()
                        ),
                    );
                }
            }
        }
        set_optional_report_timing(
            &mut report,
            "followDaemonReserveMs",
            Some(follow_reserve_ms),
        );
        set_optional_report_timing(&mut report, "followDaemonArmMs", Some(follow_arm_ms));
        if let Some(execution) = report.get_mut("execution") {
            execution["sent"] = serde_json::to_value(&sent).unwrap_or(Value::Array(vec![]));
            if let Some(actual_responder) = actual_helius_sender_endpoint(&sent) {
                execution["heliusSenderEndpoint"] = Value::String(actual_responder);
            }
            let mut existing_warnings = execution
                .get("warnings")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            existing_warnings.extend(warnings.into_iter().map(Value::String));
            existing_warnings.extend(post_send_warnings.iter().cloned().map(Value::String));
            execution["warnings"] = Value::Array(existing_warnings);
        }
        let backend_elapsed_ms = action_started.elapsed().as_millis();
        set_report_timing(&mut report, "executionTotalMs", backend_elapsed_ms);
        set_report_timing(&mut report, "backendTotalElapsedMs", backend_elapsed_ms);
        set_report_timing(&mut report, "totalElapsedMs", backend_elapsed_ms);
        attach_follow_daemon_report(
            &mut report,
            if deferred_follow_launch.enabled {
                Some(follow_daemon_transport.as_str())
            } else {
                None
            },
            reserved_follow_job.as_ref(),
            armed_follow_job.as_ref(),
            None,
            Some(&normalized.followLaunch),
        );
        refresh_report_benchmark(&mut report);
        if let Some(path) = send_log_path.as_ref() {
            let finalize_started = Instant::now();
            match update_persisted_launch_report(
                path,
                &trace.traceId,
                &action,
                &transport_plan,
                &report,
            ) {
                Ok(()) => {
                    report_finalized = true;
                    set_report_timing(
                        &mut report,
                        "persistFinalReportUpdateMs",
                        finalize_started.elapsed().as_millis(),
                    );
                }
                Err(error) => {
                    let warning = format!(
                        "Launch completed, but the persisted report could not be finalized: {error}"
                    );
                    post_send_warnings.push(warning.clone());
                    send_phase_errors.push(warning);
                }
            }
        }
        if let Some(execution) = report.get_mut("execution") {
            let mut existing_warnings = execution
                .get("warnings")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for warning in &post_send_warnings {
                let value = Value::String(warning.clone());
                if !existing_warnings.contains(&value) {
                    existing_warnings.push(value);
                }
            }
            execution["warnings"] = Value::Array(existing_warnings);
        }
        log_event(
            "engine_action_completed",
            &trace.traceId,
            json!({
                "action": action,
                "executor": "rust-rpc",
                "assemblyExecutor": assembly_executor,
                "executionClass": execution_class,
            }),
        );
        return Ok(json!({
            "ok": true,
            "service": "launchdeck-engine",
            "action": action,
            "implemented": true,
            "executionImplemented": true,
            "executor": "rust-rpc",
            "message": "Rust engine validated the request, compiled transactions natively, and sent them through native Rust RPC/Jito transport.",
            "traceId": trace.traceId,
            "elapsedMs": current_time_ms().saturating_sub(trace.startedAtMs),
            "receivedForm": payload.form.is_some(),
            "receivedRawConfig": true,
            "normalizedConfig": redacted_normalized_config(&normalized),
            "transportPlan": transport_plan,
            "assemblyExecutor": assembly_executor,
            "report": report,
            "sendLogPath": send_log_path,
            "plainSummary": build_send_plain_summary(
                launch_confirmed,
                same_time_failure_count,
                report_persisted,
                report_finalized,
                reserved_follow_job.is_some(),
                armed_follow_job.is_some(),
                response_warning_count,
            ),
            "sendOutcome": {
                "launchSubmitted": true,
                "launchSignature": launch_signature,
                "launchConfirmed": launch_confirmed,
                "sameTimeFailureCount": same_time_failure_count,
                "reportPersisted": report_persisted,
                "reportFinalized": report_finalized,
                "followDaemonReserved": reserved_follow_job.is_some(),
                "followDaemonArmed": armed_follow_job.is_some(),
                "warnings": post_send_warnings,
                "errors": send_phase_errors,
            },
            "followDaemonTransport": if deferred_follow_launch.enabled {
                Some(follow_daemon_transport)
            } else {
                None::<String>
            },
            "followDaemonReserved": reserved_follow_job,
            "followDaemonArmed": armed_follow_job,
            "text": render_report_value(&report),
            "metadataUri": response_metadata_uri(prepared_metadata_uri, &report),
            "metadataWarning": prepared_metadata_warning,
        }));
    }

    log_event(
        "engine_action_completed",
        &trace.traceId,
        json!({
            "action": action,
            "executor": assembly_executor,
        }),
    );
    Ok(json!({
        "ok": true,
        "service": "launchdeck-engine",
        "action": action,
        "implemented": true,
        "executionImplemented": true,
        "executor": assembly_executor,
        "message": "Rust engine validated the request and compiled the supported execution path natively.",
        "traceId": trace.traceId,
        "elapsedMs": current_time_ms().saturating_sub(trace.startedAtMs),
        "receivedForm": payload.form.is_some(),
        "receivedRawConfig": true,
        "normalizedConfig": redacted_normalized_config(&normalized),
        "transportPlan": transport_plan,
        "assemblyExecutor": assembly_executor,
        "report": report_value,
        "text": text_value,
        "metadataUri": response_metadata_uri(prepared_metadata_uri, &report_value),
        "metadataWarning": prepared_metadata_warning,
    }))
}

async fn engine_action(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<EngineRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    authorize(&headers, &state)?;
    execute_engine_action_payload(&state, payload)
        .await
        .map(Json)
}

fn build_settings_payload() -> Value {
    json!({
        "ok": true,
        "config": read_persistent_config(),
        "defaults": create_default_persistent_config(),
        "regionRouting": build_region_routing_payload(),
        "strategies": strategy_registry(),
        "engine": {
            "backend": "rust",
            "url": configured_base_url(),
        },
    })
}

fn bags_api_base_url() -> String {
    std::env::var("BAGS_API_BASE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://public-api-v2.bags.fm/api/v1".to_string())
}

fn read_bags_credentials_file(path: std::path::PathBuf) -> BagsStoredCredentials {
    let raw = fs::read_to_string(path).unwrap_or_default();
    if raw.trim().is_empty() {
        BagsStoredCredentials::default()
    } else {
        serde_json::from_str::<BagsStoredCredentials>(&raw).unwrap_or_default()
    }
}

fn write_bags_credentials_file(
    path: std::path::PathBuf,
    credentials: &BagsStoredCredentials,
) -> Result<(), String> {
    atomic_write(
        &path,
        &serde_json::to_vec_pretty(credentials).map_err(|error| error.to_string())?,
    )
}

fn read_persisted_bags_credentials() -> BagsStoredCredentials {
    read_bags_credentials_file(paths::bags_credentials_path())
}

fn read_session_bags_credentials() -> BagsStoredCredentials {
    read_bags_credentials_file(paths::bags_session_path())
}

fn read_active_bags_credentials() -> BagsStoredCredentials {
    let persisted = read_persisted_bags_credentials();
    let session = read_session_bags_credentials();
    BagsStoredCredentials {
        api_key: if session.api_key.trim().is_empty() {
            persisted.api_key
        } else {
            session.api_key
        },
        auth_token: if session.auth_token.trim().is_empty() {
            persisted.auth_token
        } else {
            session.auth_token
        },
        agent_username: if session.agent_username.trim().is_empty() {
            persisted.agent_username
        } else {
            session.agent_username
        },
        verified_wallet: if session.verified_wallet.trim().is_empty() {
            persisted.verified_wallet
        } else {
            session.verified_wallet
        },
    }
}

fn persist_bags_credentials(
    credentials: BagsStoredCredentials,
    save_api_key: bool,
) -> Result<(), String> {
    let session_path = paths::bags_session_path();
    let persisted_path = paths::bags_credentials_path();
    if save_api_key {
        write_bags_credentials_file(persisted_path, &credentials)?;
        if session_path.exists() {
            let _ = fs::remove_file(session_path);
        }
        return Ok(());
    }
    write_bags_credentials_file(session_path, &credentials)
}

fn clear_bags_session_credentials() {
    let path = paths::bags_session_path();
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

fn redacted_normalized_config(config: &NormalizedConfig) -> Value {
    let mut value = serde_json::to_value(config).unwrap_or_else(|_| Value::Null);
    if let Some(object) = value.as_object_mut() {
        if let Some(signer) = object.get_mut("signer").and_then(Value::as_object_mut) {
            signer.insert(
                "secretKey".to_string(),
                Value::String("[redacted]".to_string()),
            );
        }
        if let Some(bags) = object.get_mut("bags").and_then(Value::as_object_mut) {
            if bags.contains_key("authToken") {
                bags.insert(
                    "authToken".to_string(),
                    Value::String("[redacted]".to_string()),
                );
            }
        }
        if object.contains_key("vanityPrivateKey") {
            object.insert(
                "vanityPrivateKey".to_string(),
                Value::String("[redacted]".to_string()),
            );
        }
    }
    value
}

fn build_send_plain_summary(
    launch_confirmed: bool,
    same_time_failures: usize,
    report_persisted: bool,
    report_finalized: bool,
    follow_reserved: bool,
    follow_armed: bool,
    warning_count: usize,
) -> String {
    let mut parts = vec![if launch_confirmed {
        "Launch submitted and confirmed.".to_string()
    } else {
        "Launch submitted, but confirmation is incomplete or failed.".to_string()
    }];
    if same_time_failures > 0 {
        parts.push(format!(
            "{same_time_failures} same-time sniper stage(s) failed or remain unconfirmed."
        ));
    }
    if !report_persisted {
        parts.push("Initial report persistence failed.".to_string());
    } else if !report_finalized {
        parts.push("Report persistence completed only partially.".to_string());
    }
    if follow_reserved && !follow_armed {
        parts.push("Follow automation was reserved but not armed.".to_string());
    }
    if warning_count > 0 {
        parts.push(format!("{warning_count} warning(s) were recorded."));
    }
    parts.join(" ")
}

fn resolve_wallet_public_key_for_bags(wallet_env_key: &str) -> Result<String, String> {
    let key = wallet_env_key.trim();
    if key.is_empty() {
        return Ok(String::new());
    }
    let secret = load_solana_wallet_by_env_key(key)?;
    public_key_from_secret(&secret)
}

fn extract_string_field(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or_default()
        .trim()
        .to_string()
}

async fn bags_api_request(
    method: reqwest::Method,
    route: &str,
    api_key: &str,
    auth_token: Option<&str>,
    body: Option<Value>,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| format!("Failed to build Bags HTTP client: {error}"))?;
    let url = format!("{}{}", bags_api_base_url(), route);
    let mut request = client
        .request(method, &url)
        .header("x-api-key", api_key)
        .header("content-type", "application/json");
    if let Some(token) = auth_token.filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(token.trim());
    }
    if let Some(payload) = body {
        request = request.json(&payload);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("Bags API request failed: {error}"))?;
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("Failed to decode Bags API response: {error}"))?;
    if payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(status.is_success())
    {
        Ok(payload.get("response").cloned().unwrap_or(payload))
    } else {
        Err(payload
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Bags API request failed.")
            .to_string())
    }
}

async fn bags_identity_wallet_matches(
    api_key: &str,
    auth_token: &str,
    expected_wallet: &str,
) -> Result<(bool, String), String> {
    if expected_wallet.trim().is_empty() {
        return Ok((false, String::new()));
    }
    let payload = bags_api_request(
        reqwest::Method::GET,
        "/agent/wallet/list",
        api_key,
        Some(auth_token),
        None,
    )
    .await?;
    let wallets = if let Some(array) = payload.as_array() {
        array.clone()
    } else {
        payload
            .get("wallets")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let mut resolved_username = String::new();
    for wallet in wallets {
        if !resolved_username.trim().is_empty() {
            // Keep the first useful username while still checking all wallets.
        } else {
            resolved_username =
                extract_string_field(&wallet, &["providerUsername", "agentUsername", "username"]);
        }
        let wallet_address = extract_string_field(&wallet, &["wallet", "address", "publicKey"]);
        if !wallet_address.is_empty() && wallet_address == expected_wallet.trim() {
            let username =
                extract_string_field(&wallet, &["providerUsername", "agentUsername", "username"]);
            return Ok((
                true,
                if username.is_empty() {
                    resolved_username
                } else {
                    username
                },
            ));
        }
    }
    Ok((false, resolved_username))
}

async fn api_bags_identity_status(
    Query(query): Query<BagsIdentityStatusQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let active = read_active_bags_credentials();
    let wallet_env_key = query.wallet.unwrap_or_default();
    let wallet = resolve_wallet_public_key_for_bags(&wallet_env_key).unwrap_or_default();
    let mut verified = false;
    let mut resolved_username = active.agent_username.clone();
    if !wallet.is_empty()
        && !active.api_key.trim().is_empty()
        && !active.auth_token.trim().is_empty()
    {
        if let Ok((matched, username)) =
            bags_identity_wallet_matches(&active.api_key, &active.auth_token, &wallet).await
        {
            verified = matched;
            if !username.trim().is_empty() {
                resolved_username = username;
            }
        }
    }
    Ok(Json(attach_timing(
        json!({
            "ok": true,
            "configuredApiKey": !active.api_key.trim().is_empty(),
            "verified": verified,
            "mode": if verified { "linked" } else { "wallet-only" },
            "agentUsername": resolved_username,
            "verifiedWallet": if verified { wallet } else { String::new() },
            "authToken": if verified { active.auth_token } else { String::new() },
        }),
        started_at_ms,
    )))
}

async fn api_bags_identity_init(
    Json(payload): Json<BagsIdentityInitRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let current = read_active_bags_credentials();
    let api_key = if payload.api_key.trim().is_empty() {
        current.api_key
    } else {
        payload.api_key.trim().to_string()
    };
    if api_key.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "Bags API key is required." })),
        ));
    }
    let response = bags_api_request(
        reqwest::Method::POST,
        "/agent/auth/init",
        &api_key,
        None,
        Some(json!({
            "agentUsername": payload.agent_username.trim(),
        })),
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": error })),
        )
    })?;
    let init_agent_username = {
        let resolved = extract_string_field(&response, &["agentUsername", "providerUsername"]);
        if resolved.trim().is_empty() {
            payload.agent_username.trim().to_string()
        } else {
            resolved
        }
    };
    persist_bags_credentials(
        BagsStoredCredentials {
            api_key: api_key.clone(),
            auth_token: current.auth_token,
            agent_username: init_agent_username.clone(),
            verified_wallet: current.verified_wallet,
        },
        payload.save_api_key,
    )
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": error })),
        )
    })?;
    Ok(Json(attach_timing(
        json!({
            "ok": true,
            "configuredApiKey": true,
            "agentUsername": init_agent_username,
            "publicIdentifier": extract_string_field(&response, &["publicIdentifier"]),
            "secret": extract_string_field(&response, &["secret"]),
            "verificationPostContent": extract_string_field(&response, &["verificationPostContent", "content", "message"]),
        }),
        started_at_ms,
    )))
}

async fn api_bags_identity_verify(
    Json(payload): Json<BagsIdentityVerifyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let current = read_active_bags_credentials();
    let api_key = if payload.api_key.trim().is_empty() {
        current.api_key
    } else {
        payload.api_key.trim().to_string()
    };
    if api_key.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "Bags API key is required." })),
        ));
    }
    let wallet = resolve_wallet_public_key_for_bags(&payload.wallet_env_key).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": error })),
        )
    })?;
    let login_response = bags_api_request(
        reqwest::Method::POST,
        "/agent/auth/login",
        &api_key,
        None,
        Some(json!({
            "agentUsername": payload.agent_username.trim(),
            "publicIdentifier": payload.public_identifier.trim(),
            "secret": payload.secret.trim(),
            "postId": payload.post_id.trim(),
        })),
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": error })),
        )
    })?;
    let auth_token = extract_string_field(&login_response, &["token", "authToken", "jwt"]);
    if auth_token.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "Bags login did not return an auth token." })),
        ));
    }
    let (verified, resolved_username) =
        bags_identity_wallet_matches(&api_key, &auth_token, &wallet)
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "ok": false, "error": error })),
                )
            })?;
    if !verified {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "Linked Bags identity requires the selected LaunchDeck wallet to belong to the authenticated Bags account.",
            })),
        ));
    }
    let agent_username = if resolved_username.trim().is_empty() {
        payload.agent_username.trim().to_string()
    } else {
        resolved_username
    };
    persist_bags_credentials(
        BagsStoredCredentials {
            api_key: api_key,
            auth_token: auth_token.clone(),
            agent_username: agent_username.clone(),
            verified_wallet: wallet.clone(),
        },
        payload.save_api_key,
    )
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": error })),
        )
    })?;
    Ok(Json(attach_timing(
        json!({
            "ok": true,
            "configuredApiKey": true,
            "verified": true,
            "agentUsername": agent_username,
            "authToken": auth_token,
            "verifiedWallet": wallet,
        }),
        started_at_ms,
    )))
}

async fn api_bags_identity_clear() -> Json<Value> {
    clear_bags_session_credentials();
    Json(json!({ "ok": true }))
}

async fn api_bags_fee_recipient_lookup(
    Query(query): Query<BagsFeeRecipientLookupQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let provider = query
        .provider
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if provider.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "Bags recipient provider is required." })),
        ));
    }
    let username = query.username.unwrap_or_default().trim().to_string();
    let github_user_id = query.github_user_id.unwrap_or_default().trim().to_string();
    if username.is_empty() && github_user_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": if provider == "github" {
                    "Bags GitHub recipient lookup requires a username or user id."
                } else {
                    "Bags recipient lookup requires a username."
                },
            })),
        ));
    }
    let rpc_url = configured_rpc_url();
    let result = lookup_fee_recipient(FeeRecipientLookupRequest {
        launchpad: "bagsapp",
        rpc_url: &rpc_url,
        provider: &provider,
        username: &username,
        github_user_id: &github_user_id,
    })
    .await
    .and_then(|value| {
        value.ok_or_else(|| {
            "Launchpad runtime did not return a Bags fee recipient lookup response.".to_string()
        })
    })
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": error })),
        )
    })?;
    Ok(Json(attach_timing(
        json!({
            "ok": true,
            "lookup": result,
            "backend": launchpad_action_backend("bagsapp", "fee-recipient-lookup"),
            "rolloutState": launchpad_action_rollout_state("bagsapp", "fee-recipient-lookup"),
        }),
        started_at_ms,
    )))
}

async fn api_bootstrap_fast(
    Query(query): Query<StatusQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let requested_wallet = query.wallet.unwrap_or_default();
    let mut payload = build_bootstrap_fast_payload(&requested_wallet, false).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    payload["backend"] = Value::String("rust".to_string());
    payload["signerSource"] = Value::String(resolve_signer_source(
        payload
            .get("selectedWalletKey")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    ));
    Ok(Json(attach_timing(payload, started_at_ms)))
}

async fn api_bootstrap(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StatusQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let requested_wallet = query.wallet.unwrap_or_default();
    let mut payload = build_status_payload(&state, &requested_wallet, false)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })?;
    payload["backend"] = Value::String("rust".to_string());
    payload["signerSource"] = Value::String(resolve_signer_source(
        payload
            .get("selectedWalletKey")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    ));
    payload["config"] = read_persistent_config();
    Ok(Json(attach_timing(payload, started_at_ms)))
}

async fn api_lookup_tables_warm() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let rpc_url = configured_rpc_url();
    let loaded = warm_default_lookup_tables(&rpc_url)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "loaded": loaded,
    })))
}

async fn api_pump_global_warm() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let rpc_url = configured_rpc_url();
    warm_pump_global_state(&rpc_url).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    Ok(Json(json!({
        "ok": true,
    })))
}

async fn api_startup_warm(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WarmActivityRequest>,
) -> Json<Value> {
    let started_at_ms = current_time_ms();
    if !configured_startup_warm_enabled() {
        return Json(attach_timing(
            json!({
                "ok": true,
                "skipped": true,
                "startupWarm": {
                    "enabled": false,
                    "reason": "disabled-by-env",
                },
            }),
            started_at_ms,
        ));
    }
    let main_rpc_url = configured_rpc_url();
    // Read-heavy startup targets (LUTs, pump global, bonk) may use WARM_RPC_URL to spare
    // the primary endpoint. Per-launch blockhash priming uses `main_rpc_url` only so the blockhash
    // cache matches `try_compile_native_launchpad` / Bags (see `launchpad_warm`).
    let rpc_url = configured_warm_rpc_url(&main_rpc_url);
    let selected_routes = startup_warm_routes(&payload);
    {
        let mut warm = state
            .warm
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        warm.selected_routes = selected_routes.clone();
    }
    let (launchpad_warm, fee_market) = tokio::join!(
        warm_launchpads_for_startup(&rpc_url),
        fetch_fee_market_snapshot(&main_rpc_url),
    );
    let StartupWarmLaunchpadPayloads {
        lookup_tables: lookup_tables_payload,
        pump_global: pump_global_payload,
        bonk_state: bonk_state_payload,
        bags_helper: bags_helper_payload,
    } = match launchpad_warm {
        Ok(payloads) => payloads,
        Err(error) => {
            let error_payload = json!({
                "ok": false,
                "error": error,
            });
            StartupWarmLaunchpadPayloads {
                lookup_tables: error_payload.clone(),
                pump_global: error_payload.clone(),
                bonk_state: error_payload.clone(),
                bags_helper: error_payload,
            }
        }
    };
    let fee_market_payload = match fee_market {
        Ok(snapshot) => json!({
            "ok": true,
            "heliusPriorityLamports": snapshot.helius_priority_lamports,
            "heliusLaunchPriorityLamports": snapshot.launch_priority_lamports(),
            "heliusTradePriorityLamports": snapshot.trade_priority_lamports(),
            "jitoTipP99Lamports": snapshot.jito_tip_p99_lamports,
        }),
        Err(error) => json!({
            "ok": false,
            "error": error,
        }),
    };
    let (startup_endpoint_outcomes, startup_watch_outcomes_nested) = tokio::join!(
        join_all(endpoint_warm_probe_futures(&selected_routes, &main_rpc_url)),
        join_all(watch_warm_probe_futures(&selected_routes)),
    );
    let startup_endpoint_attempts = startup_endpoint_outcomes
        .into_iter()
        .map(|outcome| outcome.attempt)
        .collect::<Vec<_>>();
    let startup_watch_attempts = startup_watch_outcomes_nested
        .into_iter()
        .flatten()
        .map(|outcome| outcome.attempt)
        .collect::<Vec<_>>();
    let startup_endpoint_payload = startup_endpoint_attempts
        .iter()
        .map(|attempt| {
            json!({
                "provider": attempt.provider,
                "label": attempt.label,
                "target": attempt.target,
                "ok": matches!(attempt.result, WarmAttemptResult::Success),
                "rateLimited": matches!(attempt.result, WarmAttemptResult::RateLimited(_)),
                "error": match &attempt.result {
                    WarmAttemptResult::Success => None::<String>,
                    WarmAttemptResult::RateLimited(message) => Some(message.clone()),
                    WarmAttemptResult::Error(error) => Some(error.clone()),
                },
            })
        })
        .collect::<Vec<_>>();
    let startup_watch_payload = startup_watch_attempts
        .iter()
        .map(|attempt| {
            json!({
                "label": attempt.label,
                "target": attempt.target,
                "ok": matches!(attempt.result, WarmAttemptResult::Success),
                "rateLimited": matches!(attempt.result, WarmAttemptResult::RateLimited(_)),
                "error": match &attempt.result {
                    WarmAttemptResult::Success => None::<String>,
                    WarmAttemptResult::RateLimited(message) => Some(message.clone()),
                    WarmAttemptResult::Error(error) => Some(error.clone()),
                },
            })
        })
        .collect::<Vec<_>>();
    let startup_state_entries = vec![
        (
            "Lookup tables",
            lookup_tables_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            lookup_tables_payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        (
            "Pump global",
            pump_global_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            pump_global_payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        (
            "Bonk state",
            bonk_state_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            bonk_state_payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        (
            "Bags state",
            bags_helper_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            bags_helper_payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        (
            "Fee market",
            fee_market_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            fee_market_payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
    ];
    let startup_state_total = startup_state_entries.len();
    let startup_state_healthy = startup_state_entries
        .iter()
        .filter(|(_, ok, _)| *ok)
        .count();
    let startup_state_failures = startup_state_entries
        .iter()
        .filter(|(_, ok, _)| !*ok)
        .map(|(label, _, error)| {
            json!({
                "label": label,
                "error": error.clone().unwrap_or_else(|| "startup warm target failed".to_string()),
            })
        })
        .collect::<Vec<_>>();
    let startup_endpoint_total = startup_endpoint_payload.len();
    let startup_endpoint_healthy = startup_endpoint_payload
        .iter()
        .filter(|entry| entry.get("ok").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let startup_endpoint_failures = startup_endpoint_payload
        .iter()
        .filter(|entry| !entry.get("ok").and_then(Value::as_bool).unwrap_or(false))
        .map(|entry| {
            json!({
                "label": entry.get("label").and_then(Value::as_str).unwrap_or("Endpoint warm"),
                "error": entry.get("error").and_then(Value::as_str).unwrap_or("startup warm target failed"),
            })
        })
        .collect::<Vec<_>>();
    let startup_watch_total = startup_watch_payload.len();
    let startup_watch_healthy = startup_watch_payload
        .iter()
        .filter(|entry| entry.get("ok").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let startup_watch_failures = startup_watch_payload
        .iter()
        .filter(|entry| !entry.get("ok").and_then(Value::as_bool).unwrap_or(false))
        .map(|entry| {
            json!({
                "label": entry.get("label").and_then(Value::as_str).unwrap_or("Watcher WS warm"),
                "error": entry.get("error").and_then(Value::as_str).unwrap_or("startup warm target failed"),
            })
        })
        .collect::<Vec<_>>();
    let mut startup_attempts = vec![
        build_warm_target_attempt(
            "state",
            None,
            "Lookup tables",
            rpc_url.clone(),
            match lookup_tables_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                true => WarmAttemptResult::Success,
                false => WarmAttemptResult::Error(
                    lookup_tables_payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("startup warm target failed")
                        .to_string(),
                ),
            },
        ),
        build_warm_target_attempt(
            "state",
            None,
            "Pump global",
            rpc_url.clone(),
            match pump_global_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                true => WarmAttemptResult::Success,
                false => WarmAttemptResult::Error(
                    pump_global_payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("startup warm target failed")
                        .to_string(),
                ),
            },
        ),
        build_warm_target_attempt(
            "state",
            None,
            "Bonk state",
            rpc_url.clone(),
            match bonk_state_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                true => WarmAttemptResult::Success,
                false => WarmAttemptResult::Error(
                    bonk_state_payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("startup warm target failed")
                        .to_string(),
                ),
            },
        ),
        build_warm_target_attempt(
            "state",
            None,
            "Bags state",
            "bags-launchpad-state".to_string(),
            match bags_helper_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                true => WarmAttemptResult::Success,
                false => WarmAttemptResult::Error(
                    bags_helper_payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("startup warm target failed")
                        .to_string(),
                ),
            },
        ),
        build_warm_target_attempt(
            "state",
            None,
            "Fee Market",
            main_rpc_url.clone(),
            match fee_market_payload
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                true => WarmAttemptResult::Success,
                false => WarmAttemptResult::Error(
                    fee_market_payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("startup warm target failed")
                        .to_string(),
                ),
            },
        ),
    ];
    let warm_startup_state_failed = startup_attempts.iter().any(|attempt| {
        attempt.category == "state"
            && attempt.target == rpc_url
            && matches!(attempt.result, WarmAttemptResult::Error(_))
    });
    if (warm_startup_state_failed && main_rpc_url.trim() != rpc_url.trim())
        || should_prewarm_primary_rpc_separately(&main_rpc_url, &rpc_url)
    {
        let primary_rpc_result = prewarm_rpc_endpoint(&main_rpc_url).await;
        startup_attempts.push(build_warm_target_attempt(
            "state",
            None,
            "Primary RPC",
            main_rpc_url.clone(),
            match primary_rpc_result {
                Ok(()) => WarmAttemptResult::Success,
                Err(error) => WarmAttemptResult::Error(error),
            },
        ));
    }
    startup_attempts.extend(startup_endpoint_attempts.iter().cloned());
    startup_attempts.extend(startup_watch_attempts.iter().cloned());
    let attempt_at_ms = current_time_ms() as u64;
    {
        let mut warm = state
            .warm
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        record_startup_warm_attempts(&mut warm, &startup_attempts, attempt_at_ms);
    }
    Json(attach_timing(
        json!({
            "ok": true,
            "startupWarm": {
                "enabled": true,
                "stateTargets": {
                    "total": startup_state_total,
                    "healthy": startup_state_healthy,
                    "failing": startup_state_total.saturating_sub(startup_state_healthy),
                },
                "endpointTargets": {
                    "total": startup_endpoint_total,
                    "healthy": startup_endpoint_healthy,
                    "failing": startup_endpoint_total.saturating_sub(startup_endpoint_healthy),
                    "label": "Endpoint warm",
                },
                "watchTargets": {
                    "total": startup_watch_total,
                    "healthy": startup_watch_healthy,
                    "failing": startup_watch_total.saturating_sub(startup_watch_healthy),
                    "label": "Watcher WS warm",
                },
                "stateFailures": startup_state_failures,
                "endpointFailures": startup_endpoint_failures,
                "watchFailures": startup_watch_failures,
            },
            "launchpadBackends": {
                "pump": {
                    "backend": launchpad_action_backend("pump", "startup-warm"),
                    "rolloutState": launchpad_action_rollout_state("pump", "startup-warm"),
                },
                "bonk": {
                    "backend": launchpad_action_backend("bonk", "startup-warm"),
                    "rolloutState": launchpad_action_rollout_state("bonk", "startup-warm"),
                },
                "bagsapp": {
                    "backend": launchpad_action_backend("bagsapp", "startup-warm"),
                    "rolloutState": launchpad_action_rollout_state("bagsapp", "startup-warm"),
                }
            },
            "lookupTables": lookup_tables_payload,
            "pumpGlobal": pump_global_payload,
            "bonkState": bonk_state_payload,
            "bagsState": bags_helper_payload.clone(),
            "bagsHelper": bags_helper_payload,
            "feeMarket": fee_market_payload,
            "endpointResults": startup_endpoint_payload,
            "watchResults": startup_watch_payload,
        }),
        started_at_ms,
    ))
}

async fn api_wallet_status(
    Query(query): Query<StatusQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let wallet = query.wallet.unwrap_or_default();
    let mut payload = build_wallet_status_payload(&wallet, true)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })?;
    payload["backend"] = Value::String("rust".to_string());
    payload["signerSource"] = Value::String(resolve_signer_source(
        payload
            .get("selectedWalletKey")
            .and_then(Value::as_str)
            .unwrap_or(wallet.as_str()),
    ));
    Ok(Json(attach_timing(payload, started_at_ms)))
}

async fn api_runtime_status(State(state): State<Arc<AppState>>) -> Json<Value> {
    let started_at_ms = current_time_ms();
    Json(attach_timing(
        build_runtime_status_payload(&state).await,
        started_at_ms,
    ))
}

async fn api_warm_activity(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WarmActivityRequest>,
) -> Json<Value> {
    let started_at_ms = current_time_ms();
    let should_rewarm_now = mark_operator_activity(&state, activity_routes(&payload));
    if should_rewarm_now {
        execute_continuous_warm_pass(&state).await;
    }
    let follow_active_jobs = follow_active_jobs_count().await;
    Json(attach_timing(
        json!({
            "ok": true,
            "warm": warm_state_payload(&state, follow_active_jobs),
            "rpcTraffic": rpc_traffic_snapshot(),
        }),
        started_at_ms,
    ))
}

async fn api_warm_presence(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WarmPresenceRequest>,
) -> Json<Value> {
    let started_at_ms = current_time_ms();
    mark_browser_presence(&state, payload.active, &payload.reason);
    let follow_active_jobs = follow_active_jobs_count().await;
    Json(attach_timing(
        json!({
            "ok": true,
            "warm": warm_state_payload(&state, follow_active_jobs),
        }),
        started_at_ms,
    ))
}

fn follow_jobs_payload(response: FollowJobResponse, started_at_ms: u128) -> Json<Value> {
    let FollowJobResponse {
        schemaVersion,
        ok,
        job,
        jobs,
        health,
    } = response;
    Json(attach_timing(
        json!({
            "ok": ok,
            "schemaVersion": schemaVersion,
            "job": job,
            "jobs": jobs,
            "health": health,
        }),
        started_at_ms,
    ))
}

async fn api_follow_jobs() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let client = follow_daemon_browser_client().map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    client
        .list()
        .await
        .map(|response| follow_jobs_payload(response, started_at_ms))
        .map_err(|error| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })
}

async fn api_follow_cancel(
    Json(payload): Json<FollowCancelApiRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let client = follow_daemon_browser_client().map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    client
        .cancel(&FollowCancelRequest {
            traceId: payload.trace_id,
            actionId: payload.action_id,
            note: payload.note,
        })
        .await
        .map(|response| follow_jobs_payload(response, started_at_ms))
        .map_err(|error| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })
}

async fn api_follow_stop_all(
    Json(payload): Json<FollowStopAllApiRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let client = follow_daemon_browser_client().map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    client
        .stop_all(&FollowStopAllRequest { note: payload.note })
        .await
        .map(|response| follow_jobs_payload(response, started_at_ms))
        .map_err(|error| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })
}

async fn api_status(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StatusQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let wallet = query.wallet.unwrap_or_default();
    let mut payload = build_status_payload(&state, &wallet, true)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })?;
    payload["backend"] = Value::String("rust".to_string());
    payload["signerSource"] = Value::String(resolve_signer_source(
        payload
            .get("selectedWalletKey")
            .and_then(Value::as_str)
            .unwrap_or(wallet.as_str()),
    ));
    payload["config"] = read_persistent_config();
    Ok(Json(attach_timing(payload, started_at_ms)))
}

async fn api_run(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RunRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let _warm_guard = mark_in_flight_engine_request(&state);
    let client_pre_request_ms = payload.client_pre_request_ms.map(u128::from);
    let action = payload.action.unwrap_or_else(|| "build".to_string());
    if !["build", "simulate", "send"].contains(&action.trim().to_lowercase().as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": format!("Unsupported action: {}", action.trim()),
            })),
        ));
    }
    let selected_wallet_key = payload
        .form
        .as_ref()
        .and_then(|value| value.get("selectedWalletKey"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut response = match execute_engine_action_payload(
        &state,
        EngineRequest {
            action: Some(action.clone()),
            form: payload.form,
            raw_config: None,
            prepare_request_payload_ms: payload.prepare_request_payload_ms,
        },
    )
    .await
    {
        Ok(response) => response,
        Err((status, Json(error_payload))) => {
            record_warn(
                "launchdeck-engine",
                format!(
                    "LaunchDeck /api/run rejected action {} with status {}: {}",
                    action,
                    status,
                    error_payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                ),
                Some(error_payload.clone()),
            );
            return Err((status, Json(error_payload)));
        }
    };
    let persisted_path = response
        .get("sendLogPath")
        .and_then(Value::as_str)
        .map(str::to_string);
    let response_trace_id = response
        .get("traceId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let response_transport_plan = response.get("transportPlan").cloned();
    let mut rendered_text: Option<Value> = None;
    let mut persisted_report_snapshot: Option<Value> = None;
    if let Some(report) = response.get_mut("report") {
        if let Some(pre_request_ms) = client_pre_request_ms {
            let backend_total_ms = report
                .get("execution")
                .and_then(|execution| execution.get("timings"))
                .and_then(|timings| timings.get("totalElapsedMs"))
                .and_then(Value::as_u64)
                .map(u128::from);
            set_optional_report_timing(report, "backendTotalElapsedMs", backend_total_ms);
            set_report_timing(
                report,
                "totalElapsedMs",
                backend_total_ms
                    .unwrap_or_default()
                    .saturating_add(pre_request_ms),
            );
            set_report_timing(report, "clientPreRequestMs", pre_request_ms);
        }
        refresh_report_benchmark(report);
        let render_started = Instant::now();
        rendered_text = Some(render_report_value(report));
        set_report_timing(
            report,
            "reportRenderMs",
            render_started.elapsed().as_millis(),
        );
        refresh_report_benchmark(report);
        persisted_report_snapshot = Some(report.clone());
    }
    if let Some(text) = rendered_text {
        response["text"] = text;
    }
    if let (Some(path), Some(trace_id), Some(transport_plan_value), Some(report_snapshot)) = (
        persisted_path.as_deref(),
        response_trace_id.as_deref(),
        response_transport_plan,
        persisted_report_snapshot,
    ) && let Ok(transport_plan) =
        serde_json::from_value::<crate::transport::TransportPlan>(transport_plan_value)
    {
        let _ = update_persisted_launch_report(
            path,
            trace_id,
            &action,
            &transport_plan,
            &report_snapshot,
        );
    }
    let run_warm_routes = response
        .get("normalizedConfig")
        .map(warm_routes_from_normalized_config_value)
        .unwrap_or_default();
    if !run_warm_routes.is_empty() {
        let should_rewarm_now = mark_operator_activity(&state, run_warm_routes);
        if should_rewarm_now {
            execute_continuous_warm_pass(&state).await;
        }
    }
    response["backend"] = Value::String("rust".to_string());
    response["metadataUri"] = response
        .get("metadataUri")
        .cloned()
        .unwrap_or(Value::String(String::new()));
    response["signerSource"] = Value::String(resolve_signer_source(&selected_wallet_key));
    Ok(Json(response))
}

async fn api_engine_health() -> Json<Value> {
    let engine = health().await;
    let follow_daemon = follow_daemon_status_payload().await;
    Json(json!({
        "ok": true,
        "backend": "rust",
        "url": configured_base_url(),
        "engine": engine.0,
        "followDaemon": follow_daemon,
    }))
}

async fn api_quote(
    Query(query): Query<QuoteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let launchpad = query.launchpad.unwrap_or_else(|| "pump".to_string());
    let quote_asset = query.quoteAsset.unwrap_or_else(|| "sol".to_string());
    let launch_mode = query.launchMode.unwrap_or_default();
    let mode = query.mode.unwrap_or_default();
    let amount = query.amount.unwrap_or_default();
    if mode.trim().is_empty() || amount.trim().is_empty() {
        return Ok(Json(attach_timing(
            json!({
                "ok": true,
                "quote": Value::Null,
            }),
            started_at_ms,
        )));
    }
    let quote = quote_launch(LaunchQuoteRequest {
        rpc_url: &configured_rpc_url(),
        launchpad: &launchpad,
        quote_asset: &quote_asset,
        launch_mode: &launch_mode,
        mode: &mode,
        amount: &amount,
    })
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    Ok(Json(attach_timing(
        json!({
            "ok": true,
            "quote": quote,
            "backend": launchpad_action_backend(&launchpad, "quote"),
            "rolloutState": launchpad_action_rollout_state(&launchpad, "quote"),
        }),
        started_at_ms,
    )))
}

async fn api_settings_get() -> Json<Value> {
    let started_at_ms = current_time_ms();
    Json(attach_timing(build_settings_payload(), started_at_ms))
}

async fn api_settings_save(
    Json(payload): Json<SettingsSaveRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let config = payload
        .config
        .unwrap_or_else(create_default_persistent_config);
    let saved_path = write_persistent_config(config).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    let mut response = build_settings_payload();
    response["savedPath"] = Value::String(saved_path);
    Ok(Json(attach_timing(response, started_at_ms)))
}

async fn api_reports_list(Query(query): Query<ReportsQuery>) -> Json<Value> {
    let started_at_ms = current_time_ms();
    let sort = if query
        .sort
        .unwrap_or_else(|| "newest".to_string())
        .trim()
        .eq_ignore_ascii_case("oldest")
    {
        "oldest"
    } else {
        "newest"
    };
    Json(attach_timing(
        json!({
            "ok": true,
            "sort": sort,
            "reports": list_persisted_reports(sort),
        }),
        started_at_ms,
    ))
}

async fn api_logs(Query(query): Query<LogsQuery>) -> Json<Value> {
    let started_at_ms = current_time_ms();
    let normalized_view = query
        .view
        .unwrap_or_else(|| "live".to_string())
        .trim()
        .to_ascii_lowercase();
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let logs = if normalized_view == "errors" {
        list_error_logs(Some(limit))
    } else {
        list_live_logs(limit.min(100))
    };
    Json(attach_timing(
        json!({
            "ok": true,
            "view": if normalized_view == "errors" { "errors" } else { "live" },
            "logs": logs,
        }),
        started_at_ms,
    ))
}

async fn api_reports_view(
    Query(query): Query<ReportViewQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let id = query.id.unwrap_or_default();
    let (entry, text, payload) = read_persisted_report_bundle(&id).map_err(|error| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    Ok(Json(attach_timing(
        json!({
            "ok": true,
            "entry": entry,
            "text": text,
            "payload": payload,
        }),
        started_at_ms,
    )))
}

async fn api_upload_image(
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let mut bytes = Vec::new();
    let mut filename = "image".to_string();
    let mut content_type = String::new();
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error.to_string(),
            })),
        )
    })? {
        if field.name().unwrap_or_default() != "file" {
            continue;
        }
        filename = field.file_name().unwrap_or("image").to_string();
        content_type = field.content_type().unwrap_or_default().to_string();
        bytes = field
            .bytes()
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "error": error.to_string(),
                    })),
                )
            })?
            .to_vec();
        break;
    }
    if bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "Image file is required.",
            })),
        ));
    }
    let extension = match content_type.trim().to_ascii_lowercase().as_str() {
        "image/png" => ".png",
        "image/avif" => ".avif",
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": "Only png, jpg/jpeg, avif, webp, and gif images are supported.",
                })),
            ));
        }
    };
    let record = save_image_bytes(&bytes, extension, &filename, None).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    Ok(Json(attach_timing(
        json!({
            "ok": true,
            "id": record.id,
            "fileName": record.fileName,
            "name": record.name,
            "tags": record.tags,
            "category": record.category,
            "isFavorite": record.isFavorite,
            "createdAt": record.createdAt,
            "updatedAt": record.updatedAt,
            "previewUrl": record.previewUrl,
        }),
        started_at_ms,
    )))
}

async fn api_metadata_upload(
    Json(payload): Json<MetadataUploadRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let form = payload.form.unwrap_or(Value::Null);
    let metadata_outcome = upload_metadata_from_form(form).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    Ok(Json(attach_timing(
        json!({
            "ok": true,
            "metadataUri": metadata_outcome.metadata_uri,
            "metadataWarning": metadata_outcome.warning,
        }),
        started_at_ms,
    )))
}

async fn api_images_list(
    Query(query): Query<ImagesQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let started_at_ms = current_time_ms();
    let payload = build_image_library_payload(
        query.search.as_deref().unwrap_or_default(),
        query.category.as_deref().unwrap_or_default(),
        query
            .favoritesOnly
            .as_deref()
            .unwrap_or_default()
            .eq_ignore_ascii_case("true"),
    )
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    Ok(Json(attach_timing(
        serde_json::to_value(payload).unwrap_or(json!({"ok": false})),
        started_at_ms,
    )))
}

async fn api_image_update(
    Json(payload): Json<ImageUpdateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let tags = payload.tags.map(|value| match value {
        Value::Array(entries) => entries
            .into_iter()
            .filter_map(|entry| entry.as_str().map(|value| value.to_string()))
            .collect::<Vec<_>>(),
        Value::String(raw) => raw
            .split(',')
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>(),
        _ => vec![],
    });
    let (library_payload, image) = update_image(
        payload.id.as_deref().unwrap_or_default(),
        payload.name.as_deref(),
        tags,
        payload.category.as_deref(),
        payload.isFavorite,
    )
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    let mut response = serde_json::to_value(library_payload).unwrap_or(json!({"ok": true}));
    response["image"] = serde_json::to_value(image).unwrap_or(Value::Null);
    Ok(Json(response))
}

async fn api_image_category_create(
    Json(payload): Json<ImageCategoryRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (library_payload, category) = create_category(payload.name.as_deref().unwrap_or_default())
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })?;
    let mut response = serde_json::to_value(library_payload).unwrap_or(json!({"ok": true}));
    response["category"] = Value::String(category);
    Ok(Json(response))
}

async fn api_image_delete(
    Json(payload): Json<ImageDeleteRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let library_payload =
        delete_image(payload.id.as_deref().unwrap_or_default()).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })?;
    Ok(Json(
        serde_json::to_value(library_payload).unwrap_or(json!({"ok": true})),
    ))
}

async fn api_vamp_import(
    Json(payload): Json<VampRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let contract_address = payload.contractAddress.unwrap_or_default();
    if contract_address.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "Contract address is required.",
            })),
        ));
    }
    let mint = solana_sdk::pubkey::Pubkey::try_from(contract_address.trim()).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "Invalid Solana contract address.",
            })),
        )
    })?;
    let rpc_url = configured_rpc_url();
    let imported = fetch_imported_token_metadata(&mint.to_string(), &rpc_url)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": error,
                })),
            )
        })?;
    let mut image = Value::Null;
    let mut warning = String::new();
    let mut image_candidates = Vec::new();
    for candidate in &imported.imageCandidates {
        let normalized = candidate.trim();
        if !normalized.is_empty() && !image_candidates.iter().any(|entry| entry == normalized) {
            image_candidates.push(normalized.to_string());
        }
    }
    if !imported.imageUrl.trim().is_empty() {
        let primary = imported.imageUrl.trim().to_string();
        if !image_candidates.iter().any(|entry| entry == &primary) {
            image_candidates.push(primary);
        }
    }
    if !image_candidates.is_empty() {
        for candidate in image_candidates {
            match import_remote_image_to_library(
                &candidate,
                &format!(
                    "{}-vamp",
                    if !imported.symbol.is_empty() {
                        imported.symbol.clone()
                    } else if !imported.name.is_empty() {
                        imported.name.clone()
                    } else {
                        mint.to_string()
                    }
                ),
                if !imported.name.is_empty() {
                    &imported.name
                } else if !imported.symbol.is_empty() {
                    &imported.symbol
                } else {
                    "Imported token image"
                },
            )
            .await
            {
                Ok(Some(record)) => {
                    image = serde_json::to_value(record).unwrap_or(Value::Null);
                    warning.clear();
                    break;
                }
                Ok(None) => {}
                Err(error) => warning = error,
            }
        }
    }
    Ok(Json(json!({
        "ok": true,
        "source": if imported.source.is_empty() { "metadata" } else { &imported.source },
        "token": {
            "name": imported.name,
            "symbol": imported.symbol,
            "description": imported.description,
            "website": imported.website,
            "twitter": imported.twitter,
            "telegram": imported.telegram,
            "launchpad": imported.launchpad,
            "mode": imported.mode,
            "quoteAsset": imported.quoteAsset,
            "routes": imported.routes,
            "detection": imported.detection,
        },
        "image": image,
        "warning": warning,
    })))
}

async fn api_vanity_validate(
    Json(payload): Json<VanityValidateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let private_key = payload.privateKey.unwrap_or_default();
    if private_key.trim().is_empty() {
        return Ok(Json(json!({ "ok": true })));
    }
    let bytes = read_keypair_bytes(private_key.trim()).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": format!("Invalid vanity private key: {error}"),
            })),
        )
    })?;
    let keypair = solana_sdk::signature::Keypair::try_from(bytes.as_slice()).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": format!("Invalid vanity private key: {error}"),
            })),
        )
    })?;
    let keypair_bytes = keypair.to_bytes();
    let public_key = bs58::encode(&keypair_bytes[32..]).into_string();
    match crate::rpc::fetch_account_data(&configured_rpc_url(), &public_key, "confirmed").await {
        Ok(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": format!("This vanity address has already been used on-chain. Generate a fresh one. ({public_key})"),
                })),
            ));
        }
        Err(error) if error.contains("was not found.") => {}
        Err(error) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "error": format!("Unable to verify vanity private key availability: {error}"),
                })),
            ));
        }
    }
    Ok(Json(json!({
        "ok": true,
        "normalizedPrivateKey": bs58::encode(keypair_bytes).into_string(),
        "publicKey": public_key,
    })))
}

async fn api_vanity_status() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    vanity_pool_status_payload().map(Json).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })
}

async fn static_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(requested): AxumPath<String>,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    let normalized = requested.trim().trim_matches('/').to_string();
    if normalized.is_empty() || normalized == "index.html" {
        return launchdeck_index_response(&state);
    }
    if normalized == "launchdeck" || normalized == "launchdeck/index.html" {
        return launchdeck_index_response(&state);
    }
    if normalized == "legacy" || normalized.starts_with("legacy/") {
        return Err(static_not_found());
    }
    if normalized.starts_with("uploads/") {
        let file_name = std::path::Path::new(normalized.trim_start_matches("uploads/"))
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        return file_response(paths::uploads_dir().join(file_name));
    }
    let Some(relative_path) = safe_ui_relative_path(&requested) else {
        return Err(static_not_found());
    };
    if let Some(response) = try_file_response(paths::ui_dir().join(&relative_path)) {
        return response;
    }
    if let Some(response) = try_file_response(paths::ui_dir().join("images").join(&relative_path)) {
        return response;
    }
    Err(static_not_found())
}

async fn runtime_action(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RuntimeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    authorize(&headers, &state)?;
    let worker = payload.worker.unwrap_or_else(|| "default".to_string());
    let path = headers
        .get("x-launchdeck-runtime-action")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let response = match path.as_str() {
        "start" => start_worker(&state.runtime, &worker, payload.config).await,
        "stop" => stop_worker(&state.runtime, &worker, payload.note.clone()).await,
        "heartbeat" => heartbeat_worker(&state.runtime, &worker, payload.note.clone()).await,
        "fail" => fail_worker(&state.runtime, &worker, payload.note.clone()).await,
        _ => RuntimeResponse {
            ok: true,
            worker: Some(worker),
            active: None,
            state: None,
            workers: list_workers(&state.runtime).await,
        },
    };
    Ok(Json(json!({
        "ok": response.ok,
        "service": "launchdeck-engine",
        "implemented": true,
        "runtime": response,
    })))
}

async fn runtime_start(
    state: State<Arc<AppState>>,
    mut headers: HeaderMap,
    payload: Json<RuntimeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    headers.insert(
        "x-launchdeck-runtime-action",
        "start".parse().expect("valid header"),
    );
    runtime_action(state, headers, payload).await
}

async fn runtime_stop(
    state: State<Arc<AppState>>,
    mut headers: HeaderMap,
    payload: Json<RuntimeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    headers.insert(
        "x-launchdeck-runtime-action",
        "stop".parse().expect("valid header"),
    );
    runtime_action(state, headers, payload).await
}

async fn runtime_status(
    state: State<Arc<AppState>>,
    mut headers: HeaderMap,
    payload: Json<RuntimeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    headers.insert(
        "x-launchdeck-runtime-action",
        "status".parse().expect("valid header"),
    );
    runtime_action(state, headers, payload).await
}

async fn runtime_heartbeat(
    state: State<Arc<AppState>>,
    mut headers: HeaderMap,
    payload: Json<RuntimeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    headers.insert(
        "x-launchdeck-runtime-action",
        "heartbeat".parse().expect("valid header"),
    );
    runtime_action(state, headers, payload).await
}

async fn runtime_fail(
    state: State<Arc<AppState>>,
    mut headers: HeaderMap,
    payload: Json<RuntimeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    headers.insert(
        "x-launchdeck-runtime-action",
        "fail".parse().expect("valid header"),
    );
    runtime_action(state, headers, payload).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::follow::{FOLLOW_RESPONSE_SCHEMA_VERSION, FollowJobRecord, FollowJobState};
    use crate::report::FollowActionTimings;
    use shared_fee_market::parse_helius_priority_estimate_result;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn test_state(warm: WarmControlState) -> Arc<AppState> {
        Arc::new(AppState {
            auth: None,
            runtime: Arc::new(RuntimeRegistry::new(
                "/tmp/launchdeck-test-runtime.json".into(),
            )),
            warm: Arc::new(Mutex::new(warm)),
        })
    }

    fn sample_execution() -> crate::config::NormalizedExecution {
        serde_json::from_value(json!({
            "simulate": false,
            "send": true,
            "txFormat": "legacy",
            "commitment": "confirmed",
            "skipPreflight": true,
            "trackSendBlockHeight": true,
            "provider": "helius-sender",
            "endpointProfile": "ams",
            "mevProtect": false,
            "mevMode": "off",
            "jitodontfront": false,
            "autoGas": true,
            "autoMode": "balanced",
            "priorityFeeSol": "0",
            "tipSol": "0",
            "maxPriorityFeeSol": "0",
            "maxTipSol": "0",
            "buyProvider": "hellomoon",
            "buyEndpointProfile": "fra",
            "buyMevProtect": true,
            "buyMevMode": "secure",
            "buyJitodontfront": false,
            "buyAutoGas": true,
            "buyAutoMode": "balanced",
            "buyPriorityFeeSol": "0",
            "buyTipSol": "0",
            "buySlippagePercent": "90",
            "buyMaxPriorityFeeSol": "0",
            "buyMaxTipSol": "0",
            "sellAutoGas": true,
            "sellAutoMode": "balanced",
            "sellProvider": "jito-bundle",
            "sellEndpointProfile": "ewr",
            "sellMevProtect": false,
            "sellMevMode": "off",
            "sellJitodontfront": false,
            "sellPriorityFeeSol": "0",
            "sellTipSol": "0",
            "sellSlippagePercent": "90",
            "sellMaxPriorityFeeSol": "0",
            "sellMaxTipSol": "0"
        }))
        .expect("sample execution")
    }

    #[test]
    fn regular_pump_with_custom_fee_recipients_prefers_post_setup_creator_vault() {
        assert!(launch_prefers_post_setup_creator_vault_for_follow(
            "pump", "regular", true, true
        ));
    }

    #[test]
    fn regular_pump_without_fee_recipient_follow_up_keeps_default_creator_vault() {
        assert!(!launch_prefers_post_setup_creator_vault_for_follow(
            "pump", "regular", false, true
        ));
        assert!(!launch_prefers_post_setup_creator_vault_for_follow(
            "pump", "regular", true, false
        ));
    }

    #[test]
    fn same_time_split_preserves_launch_buy_and_retry_fallback() {
        let follow_launch = NormalizedFollowLaunch {
            enabled: true,
            source: "test".to_string(),
            schemaVersion: 1,
            snipes: vec![crate::config::NormalizedFollowLaunchSnipe {
                actionId: "snipe-1-buy".to_string(),
                enabled: true,
                walletEnvKey: "SOLANA_PRIVATE_KEY2".to_string(),
                buyAmountSol: "0.1".to_string(),
                submitWithLaunch: true,
                retryOnFailure: true,
                submitDelayMs: 0,
                targetBlockOffset: None,
                jitterMs: 0,
                feeJitterBps: 0,
                skipIfTokenBalancePositive: false,
                postBuySell: None,
            }],
            devAutoSell: None,
            constraints: crate::config::NormalizedFollowLaunchConstraints {
                pumpOnly: true,
                retryBudget: 0,
                requireDaemonReadiness: true,
                blockOnRequiredPrechecks: true,
            },
        };

        let (same_time, deferred) = split_same_time_snipes(&follow_launch);

        assert_eq!(same_time.len(), 1);
        assert!(same_time[0].submitWithLaunch);
        assert_eq!(deferred.snipes.len(), 1);
        assert!(!deferred.snipes[0].submitWithLaunch);
        assert_eq!(deferred.snipes[0].submitDelayMs, 450);
        assert!(deferred.snipes[0].skipIfTokenBalancePositive);
    }

    #[test]
    fn build_buy_transport_plan_uses_buy_route_settings() {
        let execution = sample_execution();
        let plan = build_buy_transport_plan(&execution, 2);

        assert_eq!(plan.requestedProvider, "hellomoon");
        assert_eq!(plan.resolvedProvider, "hellomoon");
        assert_eq!(plan.requestedEndpointProfile, "fra");
        assert_eq!(plan.resolvedEndpointProfile, "fra");
    }

    #[test]
    fn standard_rpc_transport_plan_overrides_only_bags_setup_route() {
        let execution = sample_execution();
        let launch_plan = build_transport_plan(&execution, 1);
        let setup_plan =
            standard_rpc_transport_plan(&launch_plan, "https://mainnet.helius-rpc.com");

        assert_eq!(launch_plan.transportType, "helius-sender");
        assert_eq!(setup_plan.transportType, "standard-rpc-fanout");
        assert!(setup_plan.heliusSenderEndpoints.is_empty());
    }

    fn sample_follow_job(state: FollowJobState) -> FollowJobRecord {
        FollowJobRecord {
            schemaVersion: FOLLOW_RESPONSE_SCHEMA_VERSION,
            traceId: "trace".to_string(),
            jobId: "job".to_string(),
            state,
            createdAtMs: 1,
            updatedAtMs: 1,
            launchpad: "pump".to_string(),
            quoteAsset: "sol".to_string(),
            launchMode: String::new(),
            selectedWalletKey: "wallet".to_string(),
            execution: sample_execution(),
            tokenMayhemMode: false,
            wrapperDefaultFeeBps: 10,
            jitoTipAccount: String::new(),
            buyTipAccount: String::new(),
            sellTipAccount: String::new(),
            preferPostSetupCreatorVaultForSell: false,
            mint: None,
            launchCreator: None,
            launchSignature: None,
            launchTransactionSubscribeAccountRequired: vec![],
            submitAtMs: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            reportPath: None,
            transportPlan: None,
            bagsLaunch: None,
            followLaunch: NormalizedFollowLaunch {
                enabled: false,
                source: String::new(),
                schemaVersion: 1,
                snipes: vec![],
                devAutoSell: None,
                constraints: crate::config::NormalizedFollowLaunchConstraints {
                    pumpOnly: false,
                    retryBudget: 0,
                    requireDaemonReadiness: false,
                    blockOnRequiredPrechecks: false,
                },
            },
            actions: vec![],
            reservedPayloadFingerprint: String::new(),
            deferredSetup: None,
            cancelRequested: false,
            lastError: None,
            timings: FollowJobTimings::default(),
        }
    }

    fn sample_sent_result(transport_type: &str, endpoint: Option<&str>) -> crate::rpc::SentResult {
        crate::rpc::SentResult {
            label: "launch".to_string(),
            format: "legacy".to_string(),
            signature: Some("sig-1".to_string()),
            explorerUrl: None,
            transportType: transport_type.to_string(),
            endpoint: endpoint.map(|value| value.to_string()),
            attemptedEndpoints: endpoint
                .map(|value| vec![value.to_string()])
                .unwrap_or_default(),
            skipPreflight: true,
            maxRetries: 0,
            confirmationStatus: None,
            confirmationSource: None,
            submittedAtMs: None,
            firstObservedStatus: None,
            firstObservedSlot: None,
            firstObservedAtMs: None,
            confirmedAtMs: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedSlot: None,
            computeUnitLimit: None,
            computeUnitPriceMicroLamports: None,
            inlineTipLamports: None,
            inlineTipAccount: None,
            bundleId: None,
            attemptedBundleIds: vec![],
            transactionSubscribeAccountRequired: vec![],
            postTokenBalances: vec![],
            confirmedTokenBalanceRaw: None,
            balanceWatchAccount: None,
            capturePostTokenBalances: false,
            requestFullTransactionDetails: false,
        }
    }

    #[test]
    fn refresh_report_benchmark_keeps_explicit_off_mode_payload() {
        let mut report = json!({
            "execution": {
                "timings": {
                    "benchmarkMode": "off",
                    "totalElapsedMs": 123
                },
                "sent": [{
                    "label": "launch",
                    "confirmationStatus": "confirmed",
                    "sendObservedSlot": 100,
                    "confirmedObservedSlot": 101,
                    "confirmedSlot": 101
                }]
            }
        });
        refresh_report_benchmark(&mut report);
        assert!(report["execution"]["timings"].is_null());
        assert_eq!(
            report["benchmark"]["mode"],
            Value::String("off".to_string())
        );
        assert_eq!(report["benchmark"]["timingGroups"], Value::Array(vec![]));
        assert_eq!(report["benchmark"]["sent"], Value::Array(vec![]));
    }

    #[test]
    fn actual_helius_sender_endpoint_ignores_non_helius_transports() {
        let sent = vec![sample_sent_result(
            "standard-rpc-sequential",
            Some("https://rpc.example"),
        )];

        assert_eq!(actual_helius_sender_endpoint(&sent), None);
    }

    #[test]
    fn actual_helius_sender_endpoint_uses_helius_send_result_endpoint() {
        let sent = vec![sample_sent_result(
            "helius-sender",
            Some("http://fra-sender.helius-rpc.com/fast"),
        )];

        assert_eq!(
            actual_helius_sender_endpoint(&sent),
            Some("http://fra-sender.helius-rpc.com/fast".to_string())
        );
    }

    #[test]
    fn immediate_presign_candidate_excludes_delayed_and_offset_actions() {
        let base = crate::follow::FollowActionRecord {
            actionId: "dev-sell".to_string(),
            kind: crate::follow::FollowActionKind::DevAutoSell,
            walletEnvKey: "WALLET_A".to_string(),
            state: crate::follow::FollowActionState::Queued,
            buyAmountSol: None,
            sellPercent: Some(100),
            submitDelayMs: None,
            targetBlockOffset: Some(0),
            delayMs: None,
            marketCap: None,
            jitterMs: None,
            feeJitterBps: None,
            precheckRequired: false,
            requireConfirmation: true,
            skipIfTokenBalancePositive: false,
            attemptCount: 0,
            scheduledForMs: None,
            eligibleAtMs: None,
            submitStartedAtMs: None,
            submittedAtMs: None,
            confirmedAtMs: None,
            provider: None,
            endpointProfile: None,
            transportType: None,
            watcherMode: None,
            watcherFallbackReason: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedTokenBalanceRaw: None,
            eligibilityObservedSlot: None,
            slotsToConfirm: None,
            signature: None,
            explorerUrl: None,
            endpoint: None,
            bundleId: None,
            lastError: None,
            triggerKey: Some("slot:0".to_string()),
            orderIndex: 0,
            preSignedTransactions: vec![],
            poolId: None,
            primaryTxIndex: None,
            timings: FollowActionTimings::default(),
        };
        assert!(follow_action_is_immediate_presign_candidate(&base));

        let mut delayed = base.clone();
        delayed.delayMs = Some(1000);
        assert!(!follow_action_is_immediate_presign_candidate(&delayed));

        let mut offset = base.clone();
        offset.targetBlockOffset = Some(1);
        assert!(!follow_action_is_immediate_presign_candidate(&offset));

        let mut market_cap = base;
        market_cap.marketCap = Some(crate::follow::FollowMarketCapTrigger {
            direction: "above".to_string(),
            threshold: "100".to_string(),
            scanTimeoutSeconds: 30,
            timeoutAction: "stop".to_string(),
        });
        assert!(!follow_action_is_immediate_presign_candidate(&market_cap));
    }

    #[tokio::test]
    async fn health_reports_rust_native_only_mode() {
        let response = health().await;
        assert!(response.ok);
        assert_eq!(response.service, "launchdeck-engine");
        assert_eq!(response.mode, "rust-native-only");
    }

    #[test]
    fn runtime_status_backend_payload_reports_bags_launch_follow_and_snapshot_as_rust_owned() {
        let payload = build_launchpad_backend_status_payload();
        assert_eq!(
            payload
                .get("bagsapp")
                .and_then(|value| value.get("market-snapshot"))
                .and_then(|value| value.get("backend"))
                .and_then(Value::as_str),
            Some("rust-native")
        );
        assert_eq!(
            payload
                .get("bagsapp")
                .and_then(|value| value.get("follow-buy"))
                .and_then(|value| value.get("backend"))
                .and_then(Value::as_str),
            Some("rust-native")
        );
        assert_eq!(
            payload
                .get("bagsapp")
                .and_then(|value| value.get("follow-sell"))
                .and_then(|value| value.get("backend"))
                .and_then(Value::as_str),
            Some("rust-native")
        );
        assert_eq!(
            payload
                .get("bagsapp")
                .and_then(|value| value.get("prepare-launch"))
                .and_then(|value| value.get("backend"))
                .and_then(Value::as_str),
            Some("rust-native")
        );
        assert_eq!(
            payload
                .get("bagsapp")
                .and_then(|value| value.get("build-launch"))
                .and_then(|value| value.get("backend"))
                .and_then(Value::as_str),
            Some("rust-native")
        );
    }

    #[test]
    fn reserves_follow_job_for_deferred_setup_without_follow_actions() {
        let follow_launch = NormalizedFollowLaunch {
            enabled: false,
            source: "legacy-postLaunch".to_string(),
            schemaVersion: 1,
            snipes: vec![],
            devAutoSell: None,
            constraints: crate::config::NormalizedFollowLaunchConstraints {
                pumpOnly: true,
                retryBudget: 1,
                requireDaemonReadiness: true,
                blockOnRequiredPrechecks: true,
            },
        };
        let deferred_setup_transactions = vec![CompiledTransaction {
            label: "follow-up".to_string(),
            format: "v0".to_string(),
            blockhash: "blockhash".to_string(),
            lastValidBlockHeight: 123,
            serializedBase64: "AQ==".to_string(),
            signature: Some("sig".to_string()),
            lookupTablesUsed: vec![],
            computeUnitLimit: None,
            computeUnitPriceMicroLamports: None,
            inlineTipLamports: None,
            inlineTipAccount: None,
        }];

        assert!(should_reserve_follow_job(
            &follow_launch,
            &deferred_setup_transactions
        ));
    }

    #[test]
    fn operator_activity_requests_immediate_rewarm_when_idle() {
        let idle_ms = u128::from(configured_idle_warm_timeout_ms()) + 5_000;
        let now_ms = current_time_ms();
        let state = test_state(WarmControlState {
            last_activity_at_ms: now_ms.saturating_sub(idle_ms),
            current_reason: "suspended-idle".to_string(),
            ..WarmControlState::default()
        });

        let immediate = mark_operator_activity(
            &state,
            vec![WarmRouteSelection {
                provider: "standard-rpc".to_string(),
                endpoint_profile: String::new(),
                hellomoon_mev_mode: String::new(),
            }],
        );
        let warm = state
            .warm
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        assert!(immediate);
        assert_eq!(warm.current_reason, "active-operator-activity");
        assert_eq!(
            warm.selected_routes,
            vec![WarmRouteSelection {
                provider: "standard-rpc".to_string(),
                endpoint_profile: String::new(),
                hellomoon_mev_mode: String::new(),
            }]
        );
    }

    #[test]
    fn operator_activity_requests_immediate_rewarm_when_routes_change_while_active() {
        let now_ms = current_time_ms();
        let state = test_state(WarmControlState {
            last_activity_at_ms: now_ms.saturating_sub(1_000),
            last_warm_success_at_ms: Some(now_ms.saturating_sub(1_000)),
            current_reason: "active-operator-activity".to_string(),
            selected_routes: vec![WarmRouteSelection {
                provider: "helius-sender".to_string(),
                endpoint_profile: "fra".to_string(),
                hellomoon_mev_mode: String::new(),
            }],
            continuous_active: true,
            ..WarmControlState::default()
        });

        let immediate = mark_operator_activity(
            &state,
            vec![WarmRouteSelection {
                provider: "helius-sender".to_string(),
                endpoint_profile: "ams".to_string(),
                hellomoon_mev_mode: String::new(),
            }],
        );
        let warm = state
            .warm
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        assert!(immediate);
        assert_eq!(
            warm.selected_routes,
            vec![WarmRouteSelection {
                provider: "helius-sender".to_string(),
                endpoint_profile: "ams".to_string(),
                hellomoon_mev_mode: String::new(),
            }]
        );
    }

    #[test]
    fn operator_activity_requests_immediate_rewarm_after_idle_suspend_with_inactive_targets() {
        let idle_ms = u128::from(configured_idle_warm_timeout_ms()) + 5_000;
        let now_ms = current_time_ms();
        let mut warm_targets = HashMap::new();
        warm_targets.insert(
            "endpoint:hellomoon:https://example.invalid".to_string(),
            WarmTargetStatus {
                id: "endpoint:hellomoon:https://example.invalid".to_string(),
                category: "endpoint".to_string(),
                provider: Some("hellomoon".to_string()),
                label: "Hello Moon QUIC".to_string(),
                target: "https://example.invalid".to_string(),
                active: false,
                last_attempt_at_ms: Some(
                    (now_ms.saturating_sub(1_000)).min(u128::from(u64::MAX)) as u64
                ),
                status: WarmTargetHealth::Healthy,
                last_success_at_ms: Some(
                    (now_ms.saturating_sub(1_000)).min(u128::from(u64::MAX)) as u64
                ),
                last_rate_limited_at_ms: None,
                last_rate_limit_message: None,
                last_error: None,
                last_error_at_ms: None,
                last_recovered_at_ms: None,
                last_recovered_error: None,
                consecutive_failures: 0,
            },
        );
        let routes = vec![WarmRouteSelection {
            provider: "hellomoon".to_string(),
            endpoint_profile: "ams".to_string(),
            hellomoon_mev_mode: String::new(),
        }];
        let state = test_state(WarmControlState {
            last_activity_at_ms: now_ms.saturating_sub(idle_ms),
            last_warm_success_at_ms: Some(now_ms.saturating_sub(1_000)),
            current_reason: "suspended-idle".to_string(),
            selected_routes: routes.clone(),
            warm_targets,
            ..WarmControlState::default()
        });

        let immediate = mark_operator_activity(&state, routes.clone());
        let warm = state
            .warm
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        assert!(immediate);
        assert_eq!(warm.current_reason, "active-operator-activity");
        assert_eq!(warm.selected_routes, routes);
    }

    #[test]
    fn operator_activity_skips_immediate_rewarm_when_routes_match_and_warm_is_active() {
        let now_ms = current_time_ms();
        let routes = vec![WarmRouteSelection {
            provider: "hellomoon".to_string(),
            endpoint_profile: "ewr".to_string(),
            hellomoon_mev_mode: "secure".to_string(),
        }];
        let state = test_state(WarmControlState {
            last_activity_at_ms: now_ms.saturating_sub(1_000),
            last_warm_success_at_ms: Some(now_ms.saturating_sub(1_000)),
            current_reason: "active-operator-activity".to_string(),
            selected_routes: routes.clone(),
            continuous_active: true,
            ..WarmControlState::default()
        });

        let immediate = mark_operator_activity(&state, routes.clone());
        let warm = state
            .warm
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        assert!(!immediate);
        assert_eq!(warm.selected_routes, routes);
    }

    #[test]
    fn activity_routes_include_profiles_and_normalize_aliases() {
        let routes = activity_routes(&WarmActivityRequest {
            creation_provider: Some("helius-sender".to_string()),
            creation_endpoint_profile: Some("fra,ams".to_string()),
            creation_mev_mode: None,
            buy_provider: Some("jito-bundle".to_string()),
            buy_endpoint_profile: Some("ny".to_string()),
            buy_mev_mode: None,
            sell_provider: Some("standard-rpc".to_string()),
            sell_endpoint_profile: Some("eu".to_string()),
            sell_mev_mode: None,
        });
        assert_eq!(
            routes,
            vec![
                WarmRouteSelection {
                    provider: "helius-sender".to_string(),
                    endpoint_profile: "fra,ams".to_string(),
                    hellomoon_mev_mode: String::new(),
                },
                WarmRouteSelection {
                    provider: "jito-bundle".to_string(),
                    endpoint_profile: "ewr".to_string(),
                    hellomoon_mev_mode: String::new(),
                },
                WarmRouteSelection {
                    provider: "standard-rpc".to_string(),
                    endpoint_profile: String::new(),
                    hellomoon_mev_mode: String::new(),
                },
            ]
        );
    }

    #[test]
    fn activity_routes_keep_distinct_hellomoon_secure_and_non_secure_routes() {
        let routes = activity_routes(&WarmActivityRequest {
            creation_provider: Some("hellomoon".to_string()),
            creation_endpoint_profile: Some("ewr".to_string()),
            creation_mev_mode: Some("reduced".to_string()),
            buy_provider: Some("hellomoon".to_string()),
            buy_endpoint_profile: Some("ewr".to_string()),
            buy_mev_mode: Some("secure".to_string()),
            sell_provider: None,
            sell_endpoint_profile: None,
            sell_mev_mode: None,
        });
        assert_eq!(
            routes,
            vec![
                WarmRouteSelection {
                    provider: "hellomoon".to_string(),
                    endpoint_profile: "ewr".to_string(),
                    hellomoon_mev_mode: String::new(),
                },
                WarmRouteSelection {
                    provider: "hellomoon".to_string(),
                    endpoint_profile: "ewr".to_string(),
                    hellomoon_mev_mode: "secure".to_string(),
                },
            ]
        );
    }

    #[test]
    fn warm_state_payload_uses_fresh_reason_not_cached_reason() {
        let idle_ms = u128::from(configured_idle_warm_timeout_ms()) + 5_000;
        let now_ms = current_time_ms();
        let state = test_state(WarmControlState {
            last_activity_at_ms: now_ms.saturating_sub(idle_ms),
            current_reason: "stale-old-reason".to_string(),
            ..WarmControlState::default()
        });

        let payload = warm_state_payload(&state, 0);
        assert_eq!(
            payload.get("reason").and_then(Value::as_str),
            Some("suspended-idle")
        );
    }

    #[test]
    fn warm_state_payload_derives_immediate_suspend_telemetry() {
        let idle_timeout_ms = u128::from(configured_idle_warm_timeout_ms());
        let idle_ms = idle_timeout_ms + 5_000;
        let now_ms = current_time_ms();
        let mut warm_targets = HashMap::new();
        warm_targets.insert(
            "state:https://warm".to_string(),
            WarmTargetStatus {
                id: "state:https://warm".to_string(),
                category: "state".to_string(),
                provider: None,
                label: "Warm RPC".to_string(),
                target: "https://warm".to_string(),
                active: true,
                last_attempt_at_ms: Some(10),
                status: WarmTargetHealth::Healthy,
                last_success_at_ms: Some(10),
                last_rate_limited_at_ms: None,
                last_rate_limit_message: None,
                last_error: None,
                last_error_at_ms: None,
                last_recovered_at_ms: None,
                last_recovered_error: None,
                consecutive_failures: 0,
            },
        );
        let state = test_state(WarmControlState {
            last_activity_at_ms: now_ms.saturating_sub(idle_ms),
            continuous_active: true,
            warm_targets,
            ..WarmControlState::default()
        });

        let payload = warm_state_payload(&state, 0);
        assert_eq!(payload.get("active").and_then(Value::as_bool), Some(false));
        assert_eq!(
            payload.get("lastSuspendAtMs").and_then(Value::as_u64),
            Some(
                (now_ms
                    .saturating_sub(idle_ms)
                    .saturating_add(idle_timeout_ms)) as u64
            )
        );
        let state_targets = payload
            .get("stateTargets")
            .and_then(Value::as_array)
            .expect("stateTargets array");
        assert_eq!(
            state_targets[0].get("active").and_then(Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn warm_state_payload_includes_per_target_telemetry() {
        let mut warm_targets = HashMap::new();
        warm_targets.insert(
            "state:https://warm".to_string(),
            WarmTargetStatus {
                id: "state:https://warm".to_string(),
                category: "state".to_string(),
                provider: None,
                label: "Warm RPC".to_string(),
                target: "https://warm".to_string(),
                active: true,
                last_attempt_at_ms: Some(10),
                status: WarmTargetHealth::Healthy,
                last_success_at_ms: Some(10),
                last_rate_limited_at_ms: None,
                last_rate_limit_message: None,
                last_error: None,
                last_error_at_ms: None,
                last_recovered_at_ms: None,
                last_recovered_error: None,
                consecutive_failures: 0,
            },
        );
        warm_targets.insert(
            "endpoint:standard-rpc:https://send".to_string(),
            WarmTargetStatus {
                id: "endpoint:standard-rpc:https://send".to_string(),
                category: "endpoint".to_string(),
                provider: Some("standard-rpc".to_string()),
                label: "Standard RPC".to_string(),
                target: "https://send".to_string(),
                active: true,
                last_attempt_at_ms: Some(20),
                status: WarmTargetHealth::Error,
                last_success_at_ms: None,
                last_rate_limited_at_ms: None,
                last_rate_limit_message: None,
                last_error: Some("timeout".to_string()),
                last_error_at_ms: None,
                last_recovered_at_ms: None,
                last_recovered_error: None,
                consecutive_failures: 2,
            },
        );
        let state = test_state(WarmControlState {
            warm_targets,
            ..WarmControlState::default()
        });

        let payload = warm_state_payload(&state, 0);
        let state_targets = payload
            .get("stateTargets")
            .and_then(Value::as_array)
            .expect("stateTargets array");
        let endpoint_targets = payload
            .get("endpointTargets")
            .and_then(Value::as_array)
            .expect("endpointTargets array");

        assert_eq!(state_targets.len(), 1);
        assert_eq!(
            state_targets[0].get("label").and_then(Value::as_str),
            Some("Warm RPC")
        );
        assert_eq!(endpoint_targets.len(), 1);
        assert_eq!(
            endpoint_targets[0].get("provider").and_then(Value::as_str),
            Some("standard-rpc")
        );
        assert_eq!(
            endpoint_targets[0].get("lastError").and_then(Value::as_str),
            Some("timeout")
        );
        assert_eq!(
            endpoint_targets[0].get("status").and_then(Value::as_str),
            Some("error")
        );
    }

    #[test]
    fn warm_target_telemetry_prunes_stale_entries_after_retention() {
        let mut warm = WarmControlState::default();
        apply_warm_target_attempts(
            &mut warm,
            &[build_warm_target_attempt(
                "state",
                None,
                "Warm RPC",
                "https://warm.example/rpc",
                WarmAttemptResult::Success,
            )],
            1_000,
        );
        assert_eq!(warm.warm_targets.len(), 1);
        let later_ms = 1_000 + WARM_TARGET_STALE_RETENTION_MS + 60_000;
        apply_warm_target_attempts(&mut warm, &[], later_ms);
        assert!(
            warm.warm_targets.is_empty(),
            "stale target should be removed after retention"
        );
    }

    #[test]
    fn warm_target_telemetry_marks_rate_limited_targets_as_degraded() {
        let mut warm = WarmControlState::default();
        apply_warm_target_attempts(
            &mut warm,
            &[build_warm_target_attempt(
                "endpoint",
                Some("jito-bundle"),
                "Jito Bundle",
                "frankfurt.mainnet.block-engine.jito.wtf",
                WarmAttemptResult::RateLimited("rate limit exceeded".to_string()),
            )],
            5_000,
        );
        let payload = warm_state_payload(&test_state(warm), 0);
        let endpoint_targets = payload
            .get("endpointTargets")
            .and_then(Value::as_array)
            .expect("endpointTargets array");
        assert_eq!(
            endpoint_targets[0].get("status").and_then(Value::as_str),
            Some("rate-limited")
        );
        assert_eq!(
            endpoint_targets[0]
                .get("lastRateLimitMessage")
                .and_then(Value::as_str),
            Some("rate limit exceeded")
        );
        assert_eq!(
            endpoint_targets[0]
                .get("lastRateLimitedAtMs")
                .and_then(Value::as_u64),
            Some(5_000)
        );
        assert_eq!(
            endpoint_targets[0]
                .get("lastSuccessAtMs")
                .and_then(Value::as_u64),
            Some(5_000)
        );
        assert!(endpoint_targets[0].get("lastError").is_none());
    }

    #[test]
    fn watch_target_diagnostics_are_provider_warnings_not_warm_ws() {
        let warm = json!({
            "stateTargets": [
                {
                    "label": "Primary RPC",
                    "status": "healthy",
                    "target": "https://rpc.example.test",
                    "lastError": null
                }
            ],
            "watchTargets": [
                {
                    "label": "Watcher WS",
                    "status": "degraded",
                    "target": "wss://watch.example.test/path?api-key=secret",
                    "lastError": "connect failed",
                    "lastErrorAtMs": 42
                }
            ]
        });

        let diagnostics = runtime_diagnostics_from_warm_payload(&warm);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].get("endpointKind").and_then(Value::as_str),
            Some("provider")
        );
        assert_eq!(
            diagnostics[0].get("envVar").unwrap_or(&Value::Null),
            &Value::Null
        );
        assert_eq!(
            diagnostics[0]
                .get("restartRequired")
                .and_then(Value::as_bool),
            Some(false)
        );
        let fingerprint = diagnostics[0]
            .get("fingerprint")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(!fingerprint.contains("secret"));
    }

    #[test]
    fn primary_rpc_diagnostic_is_not_reported_as_warm_rpc() {
        let warm = json!({
            "stateTargets": [
                {
                    "label": "Primary RPC",
                    "status": "degraded",
                    "target": "https://rpc.example.test/path?api-key=secret",
                    "lastError": "request failed",
                    "lastErrorAtMs": 42,
                    "active": true
                }
            ]
        });

        let diagnostics = runtime_diagnostics_from_warm_payload(&warm);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].get("endpointKind").and_then(Value::as_str),
            Some("provider")
        );
        assert_eq!(
            diagnostics[0].get("envVar").unwrap_or(&Value::Null),
            &Value::Null
        );
        let fingerprint = diagnostics[0]
            .get("fingerprint")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(!fingerprint.contains("secret"));
    }

    #[test]
    fn inactive_warm_targets_do_not_emit_runtime_diagnostics() {
        let warm = json!({
            "stateTargets": [
                {
                    "label": "Warm RPC",
                    "status": "degraded",
                    "target": "https://warm.example.test",
                    "lastError": "old failure",
                    "lastErrorAtMs": 42,
                    "active": false
                }
            ]
        });

        let diagnostics = runtime_diagnostics_from_warm_payload(&warm);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn inactive_primary_target_does_not_downgrade_warm_rpc_diagnostic() {
        let warm = json!({
            "stateTargets": [
                {
                    "label": "Warm RPC",
                    "status": "degraded",
                    "target": "https://warm.example.test",
                    "lastError": "warm failed",
                    "lastErrorAtMs": 42,
                    "active": true
                },
                {
                    "label": "Primary RPC",
                    "status": "healthy",
                    "target": "https://rpc.example.test",
                    "lastError": null,
                    "active": false
                }
            ]
        });

        let diagnostics = runtime_diagnostics_from_warm_payload(&warm);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].get("severity").and_then(Value::as_str),
            Some("critical")
        );
        assert_eq!(
            diagnostics[0].get("code").and_then(Value::as_str),
            Some("warm_endpoint_failed")
        );
    }

    #[test]
    fn stale_in_flight_warm_pass_is_detected() {
        let timeout_ms = u128::from(configured_continuous_warm_pass_timeout_ms());
        let now_ms = current_time_ms();
        let warm = WarmControlState {
            warm_pass_in_flight: true,
            last_warm_attempt_at_ms: Some(now_ms.saturating_sub(timeout_ms + 1_000)),
            ..WarmControlState::default()
        };
        assert!(warm_pass_in_flight_is_stale(&warm, now_ms));
    }

    #[test]
    fn recent_in_flight_warm_pass_is_not_stale() {
        let timeout_ms = u128::from(configured_continuous_warm_pass_timeout_ms());
        let now_ms = current_time_ms();
        let warm = WarmControlState {
            warm_pass_in_flight: true,
            last_warm_attempt_at_ms: Some(now_ms.saturating_sub(timeout_ms.saturating_sub(1_000))),
            ..WarmControlState::default()
        };
        assert!(!warm_pass_in_flight_is_stale(&warm, now_ms));
    }

    #[test]
    fn background_request_gate_stays_active_for_follow_jobs() {
        let now_ms = current_time_ms();
        let warm = WarmControlState {
            last_activity_at_ms: now_ms
                .saturating_sub(u128::from(configured_idle_warm_timeout_ms()) + 5_000),
            follow_jobs_active: true,
            ..WarmControlState::default()
        };
        assert!(background_request_gate_active(&warm, now_ms));
    }

    #[test]
    fn startup_warm_success_rearms_operator_activity() {
        let mut warm = WarmControlState::default();
        let attempt_at_ms = 5_000;
        record_startup_warm_attempts(
            &mut warm,
            &[build_warm_target_attempt(
                "state",
                None,
                "Warm RPC",
                "https://warm.example/rpc",
                WarmAttemptResult::Success,
            )],
            attempt_at_ms,
        );
        assert_eq!(warm.last_activity_at_ms, u128::from(attempt_at_ms));
        assert_eq!(
            warm.last_warm_success_at_ms,
            Some(u128::from(attempt_at_ms))
        );
        assert!(warm.browser_active);
        assert!(warm.continuous_active);
        assert_eq!(warm.current_reason, "active-operator-activity");
    }

    #[test]
    fn sync_follow_job_warm_state_collects_active_job_routes() {
        let mut warm = WarmControlState::default();
        crate::follow_controlplane::sync_follow_job_warm_state(
            &mut warm,
            1,
            &[sample_follow_job(FollowJobState::Running)],
        );
        assert!(warm.follow_jobs_active);
        assert_eq!(warm.follow_job_routes.len(), 3);
        assert!(
            warm.follow_job_routes.iter().any(|route| {
                route.provider == "helius-sender" && route.endpoint_profile == "ams"
            })
        );
        assert!(warm.follow_job_routes.iter().any(|route| {
            route.provider == "hellomoon"
                && route.endpoint_profile == "fra"
                && route.hellomoon_mev_mode == "secure"
        }));
        assert!(
            warm.follow_job_routes.iter().any(|route| {
                route.provider == "jito-bundle" && route.endpoint_profile == "ewr"
            })
        );
    }

    #[test]
    fn helius_sender_ping_endpoint_rewrites_fast_path() {
        assert_eq!(
            helius_sender_ping_endpoint_url("http://fra-sender.helius-rpc.com/fast"),
            "http://fra-sender.helius-rpc.com/ping"
        );
        assert_eq!(
            helius_sender_ping_endpoint_url("https://sender.helius-rpc.com/fast"),
            "https://sender.helius-rpc.com/ping"
        );
        assert_eq!(
            helius_sender_ping_endpoint_url("http://ams-sender.helius-rpc.com/ping"),
            "http://ams-sender.helius-rpc.com/ping"
        );
    }

    #[test]
    fn fee_market_snapshot_prefers_launch_specific_estimate_for_creation() {
        let snapshot = FeeMarketSnapshot {
            helius_priority_lamports: Some(10),
            helius_launch_priority_lamports: Some(25),
            helius_trade_priority_lamports: Some(40),
            jito_tip_p99_lamports: Some(5),
        };
        assert_eq!(snapshot.launch_priority_lamports(), Some(25));
        assert_eq!(snapshot.trade_priority_lamports(), Some(40));
    }

    #[test]
    fn fee_market_snapshot_falls_back_to_generic_estimate_when_template_missing() {
        let snapshot = FeeMarketSnapshot {
            helius_priority_lamports: Some(10),
            helius_launch_priority_lamports: None,
            helius_trade_priority_lamports: None,
            jito_tip_p99_lamports: Some(5),
        };
        assert_eq!(snapshot.launch_priority_lamports(), Some(10));
        assert_eq!(snapshot.trade_priority_lamports(), Some(10));
    }

    #[test]
    fn hellomoon_auto_fee_policy_uses_both_priority_and_tip() {
        assert!(provider_uses_auto_fee_priority(
            "hellomoon",
            "single",
            "creation"
        ));
        assert!(provider_uses_auto_fee_tip("hellomoon", "creation"));
        assert!(provider_uses_auto_fee_priority(
            "hellomoon",
            "single",
            "buy"
        ));
        assert!(provider_uses_auto_fee_tip("hellomoon", "buy"));
        assert!(provider_uses_auto_fee_priority(
            "hellomoon",
            "single",
            "sell"
        ));
        assert!(provider_uses_auto_fee_tip("hellomoon", "sell"));
    }

    #[test]
    fn jito_bundle_auto_fee_policy_skips_priority_only_for_bundle_creation() {
        assert!(!provider_uses_auto_fee_priority(
            "jito-bundle",
            "bundle",
            "creation"
        ));
        assert!(provider_uses_auto_fee_tip("jito-bundle", "creation"));
        assert!(provider_uses_auto_fee_priority(
            "jito-bundle",
            "bundle",
            "buy"
        ));
        assert!(provider_uses_auto_fee_priority(
            "jito-bundle",
            "bundle",
            "sell"
        ));
    }

    #[test]
    fn helius_priority_estimate_parser_uses_selected_level_then_fallbacks() {
        let payload = json!({
            "priorityFeeLevels": {
                "medium": 222,
                "high": 1234,
                "veryHigh": 5678
            },
            "priorityFeeEstimate": 4321
        });
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "Medium"),
            Some(222)
        );
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "High"),
            Some(1234)
        );
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "veryhigh"),
            Some(5678)
        );
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "unsafeMax"),
            Some(5678)
        );
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "recommended"),
            Some(4321)
        );
    }

    #[test]
    fn clamp_auto_fee_tip_raises_zero_estimate_to_provider_minimum() {
        assert_eq!(
            shared_fee_market::clamp_auto_fee_tip_to_provider_minimum(0, "hellomoon", None, "Buy",)
                .unwrap(),
            1_000_000
        );
        assert_eq!(
            shared_fee_market::clamp_auto_fee_tip_to_provider_minimum(
                0,
                "helius-sender",
                None,
                "Buy",
            )
            .unwrap(),
            200_000
        );
    }

    #[test]
    fn clamp_auto_fee_tip_raises_subminimum_to_provider_floor() {
        assert_eq!(
            shared_fee_market::clamp_auto_fee_tip_to_provider_minimum(
                50_000,
                "hellomoon",
                None,
                "Sell",
            )
            .unwrap(),
            1_000_000
        );
    }

    #[test]
    fn clamp_auto_fee_tip_leaves_at_or_above_minimum_unchanged() {
        assert_eq!(
            shared_fee_market::clamp_auto_fee_tip_to_provider_minimum(
                2_000_000,
                "hellomoon",
                None,
                "Creation",
            )
            .unwrap(),
            2_000_000
        );
    }

    #[test]
    fn clamp_auto_fee_tip_errors_when_cap_below_minimum() {
        let err = shared_fee_market::clamp_auto_fee_tip_to_provider_minimum(
            50_000,
            "hellomoon",
            Some(100_000),
            "Buy",
        )
        .expect_err("cap below minimum");
        assert!(err.contains("max auto fee is below"));
        assert!(err.contains("hellomoon"));
    }

    #[test]
    fn clamp_auto_fee_tip_no_provider_minimum_passes_through() {
        assert_eq!(
            shared_fee_market::clamp_auto_fee_tip_to_provider_minimum(
                0,
                "standard-rpc",
                None,
                "Buy",
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn total_auto_fee_cap_is_shared_between_priority_and_tip() {
        let (priority, tip) = resolve_auto_fee_components_with_total_cap(
            Some(700_000),
            Some(1_500_000),
            Some(1_100_000),
            "standard-rpc",
            "Creation",
        )
        .unwrap();
        assert_eq!(priority, Some(330_000));
        assert_eq!(tip, Some(770_000));
    }

    #[test]
    fn total_auto_fee_cap_respects_tip_floor_when_ratio_is_too_low() {
        let (priority, tip) = resolve_auto_fee_components_with_total_cap(
            Some(700_000),
            Some(200_000),
            Some(1_200_000),
            "hellomoon",
            "Creation",
        )
        .unwrap();
        assert_eq!(priority, Some(200_000));
        assert_eq!(tip, Some(1_000_000));
    }

    #[test]
    fn total_auto_fee_cap_redistributes_unused_tip_share_to_priority() {
        let (priority, tip) = resolve_auto_fee_components_with_total_cap(
            Some(900_000),
            Some(100_000),
            Some(500_000),
            "standard-rpc",
            "Creation",
        )
        .unwrap();
        assert_eq!(priority, Some(400_000));
        assert_eq!(tip, Some(100_000));
    }

    #[test]
    fn total_auto_fee_cap_allows_exact_provider_tip_floor() {
        let (priority, tip) = resolve_auto_fee_components_with_total_cap(
            Some(700_000),
            Some(200_000),
            Some(1_000_000),
            "hellomoon",
            "Creation",
        )
        .expect("cap equal to provider minimum tip should resolve");
        assert_eq!(priority, Some(1));
        assert_eq!(tip, Some(1_000_000));
    }

    #[tokio::test]
    async fn bags_fee_recipient_lookup_requires_provider() {
        let result = api_bags_fee_recipient_lookup(Query(BagsFeeRecipientLookupQuery {
            provider: None,
            username: Some("launchdeck".to_string()),
            github_user_id: None,
        }))
        .await;
        let (status, Json(payload)) = result.expect_err("missing provider should fail");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(false));
        assert!(
            payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("provider"),
        );
    }

    #[tokio::test]
    async fn bags_fee_recipient_lookup_requires_username_or_github_id() {
        let result = api_bags_fee_recipient_lookup(Query(BagsFeeRecipientLookupQuery {
            provider: Some("github".to_string()),
            username: None,
            github_user_id: None,
        }))
        .await;
        let (status, Json(payload)) = result.expect_err("missing identifier should fail");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(false));
        assert!(
            payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("username or user id"),
        );
    }
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    crypto::install_rustls_crypto_provider();
    shared_transaction_submit::observability::configure_outbound_provider_http_request_hook(
        record_outbound_provider_http_request,
    );
    clear_bags_session_credentials();
    let rpc_url = configured_rpc_url();
    let auth = match AuthManager::new() {
        Ok(manager) => Some(Arc::new(manager)),
        Err(error) => {
            eprintln!("launchdeck-engine startup failed: {error}");
            std::process::exit(1);
        }
    };
    let state = Arc::new(AppState {
        auth,
        runtime: Arc::new(RuntimeRegistry::new(configured_runtime_state_path())),
        warm: Arc::new(Mutex::new(WarmControlState {
            selected_routes: configured_active_warm_routes(),
            current_reason: "idle-awaiting-browser-activity".to_string(),
            ..WarmControlState::default()
        })),
    });
    spawn_fee_market_snapshot_refresh_task(state.clone(), rpc_url.clone());
    spawn_engine_blockhash_refresh_task(state.clone(), rpc_url.clone(), "confirmed");
    spawn_vanity_pool_refresh_task(rpc_url);
    spawn_follow_job_activity_refresh_task(state.clone());
    spawn_continuous_warm_task(state.clone());
    spawn_startup_outbox_flush_task("launchdeck-engine");
    let restored_workers = restore_workers(&state.runtime).await;
    let protected_api = Router::new()
        .route("/api/bootstrap-fast", get(api_bootstrap_fast))
        .route("/api/bootstrap", get(api_bootstrap))
        .route("/api/startup-warm", post(api_startup_warm))
        .route("/api/lookup-tables/warm", post(api_lookup_tables_warm))
        .route("/api/pump-global/warm", post(api_pump_global_warm))
        .route("/api/wallet-status", get(api_wallet_status))
        .route("/api/runtime-status", get(api_runtime_status))
        .route("/api/warm/activity", post(api_warm_activity))
        .route("/api/warm/presence", post(api_warm_presence))
        .route("/api/follow/jobs", get(api_follow_jobs))
        .route("/api/follow/cancel", post(api_follow_cancel))
        .route("/api/follow/stop-all", post(api_follow_stop_all))
        .route("/api/status", get(api_status))
        .route("/api/run", post(api_run))
        .route("/api/engine/health", get(api_engine_health))
        .route("/api/quote", get(api_quote))
        .route("/api/bags/identity/status", get(api_bags_identity_status))
        .route("/api/bags/identity/init", post(api_bags_identity_init))
        .route("/api/bags/identity/verify", post(api_bags_identity_verify))
        .route("/api/bags/identity/clear", post(api_bags_identity_clear))
        .route(
            "/api/bags/fee-recipient-lookup",
            get(api_bags_fee_recipient_lookup),
        )
        .route(
            "/api/settings",
            get(api_settings_get).post(api_settings_save),
        )
        .route("/api/settings/save", post(api_settings_save))
        .route("/api/reports", get(api_reports_list))
        .route("/api/logs", get(api_logs))
        .route("/api/reports/view", get(api_reports_view))
        .route("/api/upload-image", post(api_upload_image))
        .route("/api/metadata/upload", post(api_metadata_upload))
        .route("/api/images", get(api_images_list))
        .route("/api/images/update", post(api_image_update))
        .route("/api/images/categories", post(api_image_category_create))
        .route("/api/images/delete", post(api_image_delete))
        .route("/api/vamp", post(api_vamp_import))
        .route("/api/vanity/validate", post(api_vanity_validate))
        .route("/api/vanity/status", get(api_vanity_status))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_authorized_api_request,
        ));
    let protected_engine = Router::new()
        .route("/engine/status", post(engine_status))
        .route("/engine/quote", post(engine_action))
        .route("/engine/build", post(engine_action))
        .route("/engine/simulate", post(engine_action))
        .route("/engine/send", post(engine_action))
        .route("/engine/runtime/start", post(runtime_start))
        .route("/engine/runtime/stop", post(runtime_stop))
        .route("/engine/runtime/status", post(runtime_status))
        .route("/engine/runtime/heartbeat", post(runtime_heartbeat))
        .route("/engine/runtime/fail", post(runtime_fail))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_authorized_api_request,
        ));
    let app = Router::new()
        .route("/health", get(health))
        .route("/", get(serve_launchdeck_index))
        .route("/index.html", get(serve_launchdeck_index))
        .merge(protected_api)
        .merge(protected_engine)
        .route("/{*path}", get(static_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], configured_engine_port()));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind LaunchDeck engine listener");
    if !restored_workers.is_empty() {
        record_info(
            "engine",
            format!(
                "Restored {} runtime worker(s) from disk",
                restored_workers.len()
            ),
            None,
        );
        println!(
            "Restored {} runtime worker(s) from disk.",
            restored_workers.len()
        );
    }
    record_info(
        "engine",
        format!("LaunchDeck engine listening at http://{addr}"),
        Some(json!({
            "address": addr.to_string(),
        })),
    );
    println!("LaunchDeck engine running at http://{}", addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("LaunchDeck engine server failed");
}
