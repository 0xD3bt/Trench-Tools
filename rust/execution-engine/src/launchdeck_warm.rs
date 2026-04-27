use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use shared_extension_runtime::{
    resource_mode::idle_suspension_enabled as global_idle_suspension_enabled,
    warm_manager::{
        WarmActivityRequest, WarmControlState, WarmLifecycleMode, WarmRouteSelection,
        WarmTargetHealth, WarmTargetStatus, WatchWarmTarget, WatchWarmTransport,
    },
};
use shared_transaction_submit::{
    JitoWarmResult, prewarm_helius_transaction_subscribe_endpoint,
    prewarm_hellomoon_bundle_endpoint, prewarm_hellomoon_quic_endpoint,
    prewarm_jito_bundle_endpoint, prewarm_rpc_endpoint, prewarm_watch_websocket_endpoint,
};
use tokio::time::{Duration, sleep, timeout};

use crate::endpoint_profile::parse_config_endpoint_profile;
use crate::rpc_client::{configured_rpc_url, configured_warm_rpc_url};
use crate::transport::{
    configured_enable_helius_transaction_subscribe, configured_helius_sender_endpoints_for_profile,
    configured_hellomoon_bundle_endpoints_for_profile, configured_hellomoon_mev_protect,
    configured_hellomoon_quic_endpoints_for_profile, configured_jito_bundle_endpoints_for_profile,
    configured_standard_rpc_submit_endpoints, configured_watch_endpoints_for_provider,
    default_endpoint_profile_for_provider, prefers_helius_transaction_subscribe_path,
    resolved_helius_transaction_subscribe_ws_url, transport_environment_snapshot,
};

const DEFAULT_WALLET_STATUS_REFRESH_INTERVAL_MS: u64 = 15_000;
const WARM_TARGET_STALE_RETENTION_MS: u64 = 60 * 60 * 1000;
const RECENT_WARM_SUCCESS_REUSE_MS: u64 = 30 * 1000;

#[derive(Debug, Default)]
pub struct LaunchdeckWarmRegistry {
    pub control: WarmControlState,
    pub default_routes: Vec<WarmRouteSelection>,
    pub last_startup_warm_payload: Option<Value>,
}

pub type SharedLaunchdeckWarmRegistry = Arc<Mutex<LaunchdeckWarmRegistry>>;

#[derive(Debug, Clone)]
pub struct StartupStateResult {
    pub label: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WarmTargetAttempt {
    id: String,
    category: String,
    provider: Option<String>,
    label: String,
    target: String,
    result: WarmAttemptResult,
}

#[derive(Debug, Clone)]
enum WarmAttemptResult {
    Success,
    RateLimited(String),
    Error(String),
}

pub fn new_registry(default_routes: Vec<WarmRouteSelection>) -> SharedLaunchdeckWarmRegistry {
    let mut registry = LaunchdeckWarmRegistry::default();
    registry.default_routes = default_routes.clone();
    registry.control.selected_routes = default_routes;
    Arc::new(Mutex::new(registry))
}

pub fn configured_startup_warm_enabled() -> bool {
    if let Ok(value) = std::env::var("LAUNCHDECK_ENABLE_STARTUP_WARM") {
        return parse_env_bool_flag(&value, true);
    }
    !parse_env_bool_flag(
        &std::env::var("LAUNCHDECK_DISABLE_STARTUP_WARM").unwrap_or_default(),
        false,
    )
}

pub fn configured_wallet_status_refresh_interval_ms() -> u64 {
    std::env::var("LAUNCHDECK_WALLET_STATUS_REFRESH_INTERVAL_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_WALLET_STATUS_REFRESH_INTERVAL_MS)
}

pub fn configured_active_warm_routes_from_config(config: &Value) -> Vec<WarmRouteSelection> {
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

pub fn activity_routes(payload: &WarmActivityRequest) -> Vec<WarmRouteSelection> {
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

pub fn startup_warm_routes(
    payload: &WarmActivityRequest,
    default_routes: &[WarmRouteSelection],
) -> Vec<WarmRouteSelection> {
    let routes = activity_routes(payload);
    if routes.is_empty() {
        if default_routes.is_empty() {
            let mut fallback = Vec::new();
            push_unique_warm_route(&mut fallback, "helius-sender", "", "");
            fallback
        } else {
            default_routes.to_vec()
        }
    } else {
        routes
    }
}

pub fn build_startup_state_result(
    label: &str,
    ok: bool,
    error: Option<String>,
) -> StartupStateResult {
    StartupStateResult {
        label: label.to_string(),
        ok,
        error,
    }
}

pub fn build_startup_summary_payload(
    state_results: &[StartupStateResult],
    attempts: &[WarmTargetAttempt],
) -> Value {
    let endpoint_attempts = attempts
        .iter()
        .filter(|attempt| attempt.category == "endpoint")
        .collect::<Vec<_>>();
    let watch_attempts = attempts
        .iter()
        .filter(|attempt| attempt.category == "watch-endpoint")
        .collect::<Vec<_>>();
    let state_failures = state_results
        .iter()
        .filter(|result| !result.ok)
        .map(|result| {
            json!({
                "label": result.label,
                "error": result.error.clone().unwrap_or_else(|| "startup warm target failed".to_string()),
            })
        })
        .collect::<Vec<_>>();
    let endpoint_failures = endpoint_attempts
        .iter()
        .filter(|attempt| !attempt_succeeded(&attempt.result))
        .map(|attempt| {
            json!({
                "label": attempt.label,
                "error": attempt_error_message(&attempt.result),
            })
        })
        .collect::<Vec<_>>();
    let watch_failures = watch_attempts
        .iter()
        .filter(|attempt| !attempt_succeeded(&attempt.result))
        .map(|attempt| {
            json!({
                "label": attempt.label,
                "error": attempt_error_message(&attempt.result),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "enabled": configured_startup_warm_enabled(),
        "stateTargets": {
            "label": "State warm",
            "total": state_results.len(),
            "healthy": state_results.iter().filter(|result| result.ok).count(),
        },
        "endpointTargets": {
            "label": "Endpoint warm",
            "total": endpoint_attempts.len(),
            "healthy": endpoint_attempts.iter().filter(|attempt| attempt_succeeded(&attempt.result)).count(),
        },
        "watchTargets": {
            "label": "Watcher WS warm",
            "total": watch_attempts.len(),
            "healthy": watch_attempts.iter().filter(|attempt| attempt_succeeded(&attempt.result)).count(),
        },
        "stateFailures": state_failures,
        "endpointFailures": endpoint_failures,
        "watchFailures": watch_failures,
    })
}

pub fn record_startup_warm(
    registry: &SharedLaunchdeckWarmRegistry,
    selected_routes: Vec<WarmRouteSelection>,
    startup_payload: Value,
    attempts: &[WarmTargetAttempt],
    attempt_at_ms: u64,
) {
    let mut registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.control.selected_routes = selected_routes;
    registry.last_startup_warm_payload = Some(startup_payload);
    record_startup_warm_attempts(&mut registry.control, attempts, attempt_at_ms);
}

pub fn update_default_routes(
    registry: &SharedLaunchdeckWarmRegistry,
    default_routes: Vec<WarmRouteSelection>,
) {
    let mut registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.default_routes = default_routes;
}

pub fn record_startup_warm_error(
    registry: &SharedLaunchdeckWarmRegistry,
    startup_payload: Option<Value>,
    error: &str,
) {
    let mut registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(payload) = startup_payload {
        registry.last_startup_warm_payload = Some(payload);
    }
    registry.control.last_error = Some(error.trim().to_string());
}

pub fn mark_operator_activity(
    registry: &SharedLaunchdeckWarmRegistry,
    routes: Vec<WarmRouteSelection>,
) -> bool {
    let now = now_unix_ms();
    let mut registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let warm = &mut registry.control;
    let route_matches_current = routes.is_empty() || routes == warm.selected_routes;
    let warm_is_active = warm_gate_state(warm, now, 0).0;
    let should_trigger_immediate_rewarm = if !route_matches_current {
        true
    } else if !warm_is_active && warm_targets_need_resume_pass(warm) {
        true
    } else {
        !warm_is_active && !warm_succeeded_recently(warm, now)
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

pub fn warm_runtime_payload(registry: &SharedLaunchdeckWarmRegistry) -> Value {
    let now_ms = now_unix_ms();
    let registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let warm = &registry.control;
    let follow_active_jobs = 0u64;
    let (active, browser_active, reason) = warm_gate_state(warm, now_ms, follow_active_jobs);
    let mode = if active {
        WarmLifecycleMode::Active
    } else if browser_active || follow_active_jobs > 0 || warm.warm_pass_in_flight {
        WarmLifecycleMode::Maintenance
    } else {
        WarmLifecycleMode::Idle
    };
    let selected_routes = effective_warm_routes(&registry);
    let selected_providers = warm_route_providers(&selected_routes);
    let state_targets = payload_warm_targets(warm, "state", active);
    let endpoint_targets = payload_warm_targets(warm, "endpoint", active);
    let watch_targets = payload_warm_targets(warm, "watch-endpoint", active);
    json!({
        "startupEnabled": configured_startup_warm_enabled(),
        "continuousEnabled": configured_continuous_warm_enabled(),
        "idleSuspendEnabled": configured_idle_warm_suspend_enabled(),
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
        "lastSuspendAtMs": effective_suspend_at_ms(warm, active, &reason, now_ms),
        "passInFlight": warm.warm_pass_in_flight,
        "lastWarmAttemptAtMs": warm.last_warm_attempt_at_ms.map(|value| value as u64),
        "lastWarmSuccessAtMs": warm.last_warm_success_at_ms.map(|value| value as u64),
        "lastError": warm.last_error.clone(),
        "stateTargets": state_targets,
        "endpointTargets": endpoint_targets,
        "watchTargets": watch_targets,
        "sessionCount": if active || browser_active { 1 } else { 0 },
        "sessions": [],
        "startupWarm": registry.last_startup_warm_payload,
    })
}

pub fn spawn_continuous_warm_task(registry: SharedLaunchdeckWarmRegistry) {
    tokio::spawn(async move {
        loop {
            let pass_timeout_ms = configured_continuous_warm_pass_timeout_ms();
            if timeout(
                Duration::from_millis(pass_timeout_ms),
                execute_continuous_warm_pass(registry.clone()),
            )
            .await
            .is_err()
            {
                let mut registry = registry
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                registry.control.warm_pass_in_flight = false;
                registry.control.last_error = Some(format!(
                    "Continuous warm pass timed out after {}ms and was reset.",
                    pass_timeout_ms
                ));
                if registry.control.continuous_active {
                    set_all_warm_targets_inactive(&mut registry.control);
                }
            }
            sleep(Duration::from_millis(
                configured_continuous_warm_interval_ms(),
            ))
            .await;
        }
    });
}

pub async fn execute_immediate_warm_pass(registry: SharedLaunchdeckWarmRegistry) {
    execute_continuous_warm_pass(registry).await;
}

pub async fn collect_startup_runtime_attempts(
    routes: &[WarmRouteSelection],
) -> Vec<WarmTargetAttempt> {
    let main_rpc_url = configured_rpc_url();
    let warm_rpc_url = configured_warm_rpc_url();
    let mut attempts = collect_state_warm_attempts(&main_rpc_url, &warm_rpc_url).await;
    attempts.extend(collect_endpoint_warm_attempts(routes, &main_rpc_url).await);
    attempts.extend(collect_watch_warm_attempts(routes).await);
    attempts
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
        .unwrap_or(600_000)
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
    parse_config_endpoint_profile(trimmed)
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

fn effective_warm_routes(registry: &LaunchdeckWarmRegistry) -> Vec<WarmRouteSelection> {
    let warm = &registry.control;
    let mut routes = registry.default_routes.clone();
    for route in &warm.selected_routes {
        push_unique_warm_route(
            &mut routes,
            &route.provider,
            &route.endpoint_profile,
            &route.hellomoon_mev_mode,
        );
    }
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

fn set_all_warm_targets_inactive(warm: &mut WarmControlState) {
    for target in warm.warm_targets.values_mut() {
        target.active = false;
    }
}

fn apply_warm_target_attempts(
    warm: &mut WarmControlState,
    attempts: &[WarmTargetAttempt],
    attempt_at_ms: u64,
) {
    let active_ids = attempts
        .iter()
        .map(|attempt| attempt.id.clone())
        .collect::<HashSet<_>>();
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
                entry.last_error = None;
                entry.last_error_at_ms = None;
                entry.consecutive_failures = 0;
            }
            WarmAttemptResult::RateLimited(message) => {
                entry.status = WarmTargetHealth::RateLimited;
                entry.last_success_at_ms = Some(attempt_at_ms);
                entry.last_rate_limited_at_ms = Some(attempt_at_ms);
                entry.last_rate_limit_message = Some(message.clone());
                entry.last_error = None;
                entry.last_error_at_ms = None;
                entry.consecutive_failures = 0;
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
            }
        }
    }
    prune_stale_warm_targets_after_attempt(warm, &active_ids, attempt_at_ms);
}

fn record_startup_warm_attempts(
    warm: &mut WarmControlState,
    attempts: &[WarmTargetAttempt],
    attempt_at_ms: u64,
) {
    apply_warm_target_attempts(warm, attempts, attempt_at_ms);
    warm.last_warm_attempt_at_ms = Some(u128::from(attempt_at_ms));
    if attempts
        .iter()
        .any(|attempt| attempt_succeeded(&attempt.result))
    {
        let attempt_at_ms_u128 = u128::from(attempt_at_ms);
        warm.last_warm_success_at_ms = Some(attempt_at_ms_u128);
        warm.last_activity_at_ms = attempt_at_ms_u128;
        warm.browser_active = true;
        warm.continuous_active = true;
        warm.current_reason = "active-operator-activity".to_string();
        warm.last_error = None;
    } else if !attempts.is_empty() {
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

fn attempt_succeeded(result: &WarmAttemptResult) -> bool {
    matches!(
        result,
        WarmAttemptResult::Success | WarmAttemptResult::RateLimited(_)
    )
}

fn attempt_error_message(result: &WarmAttemptResult) -> String {
    match result {
        WarmAttemptResult::Success => String::new(),
        WarmAttemptResult::RateLimited(message) => message.clone(),
        WarmAttemptResult::Error(message) => message.clone(),
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

async fn warm_attempt_result<F>(future: F) -> WarmAttemptResult
where
    F: std::future::Future<Output = Result<(), String>>,
{
    let timeout_ms = configured_warm_probe_timeout_ms();
    match timeout(Duration::from_millis(timeout_ms), future).await {
        Ok(Ok(())) => WarmAttemptResult::Success,
        Ok(Err(error)) => WarmAttemptResult::Error(error),
        Err(_) => WarmAttemptResult::Error(format!("timed out after {timeout_ms}ms")),
    }
}

async fn collect_state_warm_attempts(
    main_rpc_url: &str,
    warm_rpc_url: &str,
) -> Vec<WarmTargetAttempt> {
    let mut attempts = Vec::new();
    let warm_rpc_target = warm_rpc_url.to_string();
    let warm_rpc_result =
        warm_attempt_result(async move { prewarm_rpc_endpoint(&warm_rpc_target).await }).await;
    attempts.push(build_warm_target_attempt(
        "state",
        None,
        "Warm RPC",
        warm_rpc_url.to_string(),
        warm_rpc_result,
    ));
    if should_prewarm_primary_rpc_separately(main_rpc_url, warm_rpc_url) {
        let primary_target = main_rpc_url.to_string();
        let primary_result =
            warm_attempt_result(async move { prewarm_rpc_endpoint(&primary_target).await }).await;
        attempts.push(build_warm_target_attempt(
            "state",
            None,
            "Primary RPC",
            main_rpc_url.to_string(),
            primary_result,
        ));
    }
    attempts
}

async fn collect_endpoint_warm_attempts(
    routes: &[WarmRouteSelection],
    main_rpc_url: &str,
) -> Vec<WarmTargetAttempt> {
    let mut attempts = Vec::new();
    let transport_environment = transport_environment_snapshot();
    if routes.iter().any(|route| route.provider == "standard-rpc") {
        for endpoint in configured_standard_rpc_warm_endpoints(main_rpc_url) {
            let endpoint_target = endpoint.clone();
            let result =
                warm_attempt_result(async move { prewarm_rpc_endpoint(&endpoint_target).await })
                    .await;
            attempts.push(build_warm_target_attempt(
                "endpoint",
                Some("standard-rpc"),
                "Standard RPC",
                endpoint,
                result,
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
                let result = warm_attempt_result(async move {
                    prewarm_helius_sender_endpoint(&endpoint_target).await
                })
                .await;
                attempts.push(build_warm_target_attempt(
                    "endpoint",
                    Some("helius-sender"),
                    "Helius Sender",
                    endpoint,
                    result,
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
                let result = if route.hellomoon_mev_mode == "secure" {
                    let environment = transport_environment.clone();
                    warm_attempt_result(async move {
                        prewarm_hellomoon_bundle_endpoint(&endpoint_target, &environment).await
                    })
                    .await
                } else {
                    let environment = transport_environment.clone();
                    warm_attempt_result(async move {
                        prewarm_hellomoon_quic_endpoint(&endpoint_target, mev_protect, &environment)
                            .await
                    })
                    .await
                };
                attempts.push(build_warm_target_attempt(
                    "endpoint",
                    Some("hellomoon"),
                    if route.hellomoon_mev_mode == "secure" {
                        "Hello Moon Bundle"
                    } else {
                        "Hello Moon QUIC"
                    },
                    endpoint,
                    result,
                ));
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
                let endpoint_name = endpoint.name.clone();
                let result = warm_attempt_result(async move {
                    match prewarm_jito_bundle_endpoint(&endpoint).await {
                        Ok(JitoWarmResult::Warmed) => Ok(()),
                        Ok(JitoWarmResult::RateLimited(message)) => {
                            Err(format!("rate-limited: {message}"))
                        }
                        Err(error) => Err(error),
                    }
                })
                .await;
                let result = match result {
                    WarmAttemptResult::Error(message) if message.starts_with("rate-limited: ") => {
                        WarmAttemptResult::RateLimited(
                            message.trim_start_matches("rate-limited: ").to_string(),
                        )
                    }
                    other => other,
                };
                attempts.push(build_warm_target_attempt(
                    "endpoint",
                    Some("jito-bundle"),
                    "Jito Bundle",
                    endpoint_name,
                    result,
                ));
            }
        }
    }
    attempts
}

async fn collect_watch_warm_attempts(routes: &[WarmRouteSelection]) -> Vec<WarmTargetAttempt> {
    let mut attempts = Vec::new();
    for target in configured_watch_warm_targets(routes) {
        match target.transport {
            WatchWarmTransport::StandardWs => {
                let endpoint = target.target.clone();
                let result = warm_attempt_result(async move {
                    prewarm_watch_websocket_endpoint(&endpoint).await
                })
                .await;
                attempts.push(build_warm_target_attempt(
                    "watch-endpoint",
                    None,
                    &target.label,
                    target.target.clone(),
                    result,
                ));
            }
            WatchWarmTransport::HeliusTransactionSubscribe => {
                let endpoint = target.target.clone();
                let primary_result = warm_attempt_result(async move {
                    prewarm_helius_transaction_subscribe_endpoint(&endpoint).await
                })
                .await;
                let primary_failed = matches!(primary_result, WarmAttemptResult::Error(_));
                attempts.push(build_warm_target_attempt(
                    "watch-endpoint",
                    None,
                    &target.label,
                    target.target.clone(),
                    primary_result,
                ));
                if primary_failed {
                    if let Some(fallback_endpoint) = target.fallback_target.as_deref() {
                        let fallback_target = fallback_endpoint.to_string();
                        let fallback_result = warm_attempt_result(async move {
                            prewarm_watch_websocket_endpoint(&fallback_target).await
                        })
                        .await;
                        attempts.push(build_warm_target_attempt(
                            "watch-endpoint",
                            None,
                            "Watcher WS",
                            fallback_endpoint.to_string(),
                            fallback_result,
                        ));
                    }
                }
            }
        }
    }
    attempts
}

async fn execute_continuous_warm_pass(registry: SharedLaunchdeckWarmRegistry) {
    let follow_active_jobs = 0u64;
    let (routes, attempt_started_at_ms) = {
        let now_ms = now_unix_ms();
        let mut registry = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let routes = effective_warm_routes(&registry);
        let warm = &mut registry.control;
        warm.follow_jobs_active = false;
        let (should_warm, browser_active, reason) =
            warm_gate_state(warm, now_ms, follow_active_jobs);
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
            set_all_warm_targets_inactive(warm);
            return;
        }
        if warm.warm_pass_in_flight {
            if warm_pass_in_flight_is_stale(warm, now_ms) {
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
    let warm_rpc_url = configured_warm_rpc_url();
    let mut attempts = collect_state_warm_attempts(&main_rpc_url, &warm_rpc_url).await;
    attempts.extend(collect_endpoint_warm_attempts(&routes, &main_rpc_url).await);
    attempts.extend(collect_watch_warm_attempts(&routes).await);

    let now_ms = now_unix_ms();
    let mut registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let warm = &mut registry.control;
    warm.warm_pass_in_flight = false;
    apply_warm_target_attempts(warm, &attempts, attempt_started_at_ms);
    if attempts
        .iter()
        .any(|attempt| attempt_succeeded(&attempt.result))
    {
        warm.last_warm_success_at_ms = Some(now_ms);
        warm.last_error = None;
    } else if !attempts.is_empty() {
        let errors = attempts
            .iter()
            .filter_map(|attempt| match &attempt.result {
                WarmAttemptResult::Error(error) => {
                    Some(format!("{} {}: {}", attempt.label, attempt.target, error))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if !errors.is_empty() {
            warm.last_error = Some(errors.join(" | "));
        }
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn helius_sender_ping_endpoint_url(rpc_url: &str) -> String {
    let trimmed = rpc_url.trim_end_matches('/');
    if trimmed.ends_with("/ping") {
        return trimmed.to_string();
    }
    format!("{trimmed}/ping")
}

/// Canonical Helius Sender prewarm helper. Reuses the process-wide shared
/// HTTP client from `rpc_client` so TLS sessions / DNS resolution / keepalive
/// connections are shared with every other execution-engine HTTP request
/// instead of sitting in a second parallel pool.
async fn prewarm_helius_sender_endpoint(rpc_url: &str) -> Result<(), String> {
    let ping_url = helius_sender_ping_endpoint_url(rpc_url);
    let response = crate::rpc_client::shared_rpc_http_client()
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_fallback_defaults_to_helius_sender() {
        let routes = configured_active_warm_routes_from_config(&json!({}));
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].provider, "helius-sender");
    }

    #[test]
    fn startup_fallback_defaults_to_helius_sender() {
        let routes = startup_warm_routes(&WarmActivityRequest::default(), &[]);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].provider, "helius-sender");
    }

    #[test]
    fn effective_routes_merge_defaults_selected_and_follow_jobs() {
        let mut registry = LaunchdeckWarmRegistry {
            default_routes: vec![WarmRouteSelection {
                provider: "helius-sender".to_string(),
                endpoint_profile: "fra".to_string(),
                hellomoon_mev_mode: String::new(),
            }],
            ..LaunchdeckWarmRegistry::default()
        };
        registry.control.selected_routes = vec![WarmRouteSelection {
            provider: "hellomoon".to_string(),
            endpoint_profile: "ams".to_string(),
            hellomoon_mev_mode: "secure".to_string(),
        }];
        registry.control.follow_job_routes = vec![WarmRouteSelection {
            provider: "jito-bundle".to_string(),
            endpoint_profile: "ny".to_string(),
            hellomoon_mev_mode: String::new(),
        }];

        let routes = effective_warm_routes(&registry);
        assert!(
            routes.iter().any(|route| {
                route.provider == "helius-sender" && route.endpoint_profile == "fra"
            })
        );
        assert!(routes.iter().any(|route| {
            route.provider == "hellomoon"
                && route.endpoint_profile == "ams"
                && route.hellomoon_mev_mode == "secure"
        }));
        assert!(
            routes.iter().any(|route| {
                route.provider == "jito-bundle" && route.endpoint_profile == "ewr"
            })
        );
    }
}
