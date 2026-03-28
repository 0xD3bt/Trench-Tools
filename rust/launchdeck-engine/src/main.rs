mod config;
mod observability;
mod providers;
mod pump_native;
mod report;
mod rpc;
mod runtime;
mod transport;
mod wallet;

use crate::{
    config::{RawConfig, normalize_raw_config},
    observability::{log_event, new_trace_context},
    providers::provider_registry,
    pump_native::try_compile_native_pump,
    report::{build_report, render_report},
    rpc::{
        send_transactions_bundle, send_transactions_sequential, simulate_transactions,
    },
    runtime::{
        RuntimeRegistry, RuntimeRequest, RuntimeResponse, fail_worker, heartbeat_worker,
        list_workers, restore_workers, start_worker, stop_worker,
    },
    transport::{build_transport_plan, configured_jito_bundle_endpoints},
    wallet::{
        list_solana_env_wallets, load_solana_wallet_by_env_key, public_key_from_secret,
        selected_wallet_key_or_default,
    },
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
struct AppState {
    auth_token: Option<String>,
    runtime: Arc<RuntimeRegistry>,
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
}

fn configured_engine_port() -> u16 {
    env::var("LAUNCHDECK_ENGINE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8790)
}

fn configured_auth_token() -> Option<String> {
    let token = env::var("LAUNCHDECK_ENGINE_AUTH_TOKEN").unwrap_or_default();
    let trimmed = token.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn configured_runtime_state_path() -> PathBuf {
    if let Ok(explicit) = env::var("LAUNCHDECK_ENGINE_RUNTIME_PATH") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".local")
        .join("engine-runtime.json")
}

fn authorize(headers: &HeaderMap, state: &AppState) -> Result<(), (StatusCode, Json<Value>)> {
    let Some(expected) = &state.auth_token else {
        return Ok(());
    };
    let actual = headers
        .get("x-launchdeck-engine-auth")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if actual == expected {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "ok": false,
                "error": "Unauthorized engine request.",
            })),
        ))
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "launchdeck-engine",
        version: env!("CARGO_PKG_VERSION"),
        mode: "rust-native-only",
    })
}

fn configured_rpc_url() -> String {
    if let Ok(explicit) = env::var("SOLANA_RPC_URL") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Ok(explicit) = env::var("HELIUS_RPC_URL") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Ok(api_key) = env::var("HELIUS_API_KEY") {
        let trimmed = api_key.trim();
        if !trimmed.is_empty() {
            return format!("https://mainnet.helius-rpc.com/?api-key={trimmed}");
        }
    }
    "http://127.0.0.1:8899".to_string()
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

fn synthetic_mint_address(trace_id: &str) -> String {
    let clean = trace_id.replace('-', "");
    let mut bytes = Vec::with_capacity(32);
    while bytes.len() < 32 {
        bytes.extend_from_slice(clean.as_bytes());
    }
    bytes.truncate(32);
    bs58::encode(bytes).into_string()
}

async fn engine_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    authorize(&headers, &state)?;
    let wallets = list_solana_env_wallets();
    let selected_wallet_key = selected_wallet_key_or_default("");
    let runtime_workers = list_workers(&state.runtime).await;
    Ok(Json(json!({
        "ok": true,
        "service": "launchdeck-engine",
        "engineBackend": "rust",
        "implemented": true,
        "executionMode": "rust-native-only",
        "message": "Rust engine is online with native Pump execution, native RPC transport, and native runtime workers. Unsupported requests fail explicitly instead of falling back to JavaScript.",
        "rpcUrl": configured_rpc_url(),
        "wallets": wallets,
        "selectedWalletKey": selected_wallet_key,
        "providers": provider_registry(),
        "transport": {
            "jitoBundleEndpoints": configured_jito_bundle_endpoints(),
        },
        "runtime": {
            "statePath": state.runtime.storage_path,
            "workerCount": runtime_workers.len(),
        },
        "runtimeWorkers": runtime_workers,
    })))
}

async fn engine_action(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<EngineRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    authorize(&headers, &state)?;
    let trace = new_trace_context();
    let action = payload.action.unwrap_or_else(|| "unknown".to_string());
    let raw_config_value = payload.raw_config.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "rawConfig is required.",
                "traceId": trace.traceId,
            })),
        )
    })?;
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
    let normalized = normalize_raw_config(parsed).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": error.to_string(),
                "traceId": trace.traceId,
            })),
        )
    })?;
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
    let preview_agent_authority = if normalized.mode == "regular" || normalized.mode == "cashback" {
        None
    } else if !normalized.agent.authority.trim().is_empty() {
        Some(normalized.agent.authority.clone())
    } else {
        Some(creator_public_key.clone())
    };
    let preview_report = build_report(
        &normalized,
        now_timestamp_string(),
        configured_rpc_url(),
        creator_public_key.clone(),
        synthetic_mint_address(&trace.traceId),
        preview_agent_authority.clone(),
        Some("Rust native preview".to_string()),
    );
    let transport_plan =
        build_transport_plan(&normalized.execution, preview_report.transactions.len());
    if action == "build" {
        let report = build_report(
            &normalized,
            now_timestamp_string(),
            configured_rpc_url(),
            creator_public_key.clone(),
            synthetic_mint_address(&trace.traceId),
            preview_agent_authority,
            Some("Rust native build".to_string()),
        );
        let text = render_report(&report);
        log_event(
            "engine_action_completed",
            &trace.traceId,
            json!({
                "action": action,
                "executor": "rust-native",
            }),
        );
        return Ok(Json(json!({
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
            "normalizedConfig": normalized,
            "transportPlan": transport_plan,
            "report": report,
            "text": text,
        })));
    }
    let native_artifacts = try_compile_native_pump(
        &configured_rpc_url(),
        &normalized,
        &wallet_secret,
        now_timestamp_string(),
        creator_public_key.clone(),
        Some("Rust native compile".to_string()),
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
    let (compiled_transactions, report_value, text_value, assembly_executor) = (
        native.compiled_transactions,
        native.report,
        Value::String(native.text),
        "rust-native".to_string(),
    );

    if action == "simulate" {
        let (simulation, warnings) = simulate_transactions(
            &configured_rpc_url(),
            &compiled_transactions,
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
        if let Some(execution) = report.get_mut("execution") {
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
        log_event(
            "engine_action_completed",
            &trace.traceId,
            json!({
                "action": action,
                "executor": "rust-rpc",
                "assemblyExecutor": assembly_executor,
            }),
        );
        return Ok(Json(json!({
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
            "normalizedConfig": normalized,
            "transportPlan": transport_plan,
            "assemblyExecutor": assembly_executor,
            "report": report,
            "text": text_value,
        })));
    }

    if action == "send" {
        let execution_class = transport_plan.executionClass.clone();
        let (sent, warnings) = if execution_class == "bundle" {
            send_transactions_bundle(
                &transport_plan.jitoBundleEndpoints,
                &compiled_transactions,
                &normalized.execution.commitment,
            )
            .await
        } else {
            send_transactions_sequential(
                &configured_rpc_url(),
                &compiled_transactions,
                &normalized.execution.commitment,
                normalized.execution.skipPreflight,
            )
            .await
        }
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
        if let Some(execution) = report.get_mut("execution") {
            execution["sent"] = serde_json::to_value(sent).unwrap_or(Value::Array(vec![]));
            let mut existing_warnings = execution
                .get("warnings")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            existing_warnings.extend(warnings.into_iter().map(Value::String));
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
        return Ok(Json(json!({
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
            "normalizedConfig": normalized,
            "transportPlan": transport_plan,
            "assemblyExecutor": assembly_executor,
            "report": report,
            "text": text_value,
        })));
    }

    log_event(
        "engine_action_completed",
        &trace.traceId,
        json!({
            "action": action,
            "executor": assembly_executor,
        }),
    );
    Ok(Json(json!({
        "ok": true,
        "service": "launchdeck-engine",
        "action": action,
        "implemented": true,
        "executionImplemented": true,
        "executor": assembly_executor,
        "message": if assembly_executor == "rust-native" {
            "Rust engine validated the request and compiled the supported execution path natively."
        } else {
            "Rust engine validated the request and used the JS compile bridge for the remaining unported execution path."
        },
        "traceId": trace.traceId,
        "elapsedMs": current_time_ms().saturating_sub(trace.startedAtMs),
        "receivedForm": payload.form.is_some(),
        "receivedRawConfig": true,
        "normalizedConfig": normalized,
        "transportPlan": transport_plan,
        "assemblyExecutor": assembly_executor,
        "report": report_value,
        "text": text_value,
    })))
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

    #[tokio::test]
    async fn health_reports_rust_native_only_mode() {
        let response = health().await;
        assert!(response.ok);
        assert_eq!(response.service, "launchdeck-engine");
        assert_eq!(response.mode, "rust-native-only");
    }
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let state = Arc::new(AppState {
        auth_token: configured_auth_token(),
        runtime: Arc::new(RuntimeRegistry::new(configured_runtime_state_path())),
    });
    let restored_workers = restore_workers(&state.runtime).await;
    let app = Router::new()
        .route("/health", get(health))
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
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], configured_engine_port()));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind LaunchDeck engine listener");
    if !restored_workers.is_empty() {
        println!(
            "Restored {} runtime worker(s) from disk.",
            restored_workers.len()
        );
    }
    println!("LaunchDeck engine running at http://{}", addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("LaunchDeck engine server failed");
}
