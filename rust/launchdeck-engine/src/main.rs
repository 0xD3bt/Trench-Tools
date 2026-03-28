mod config;
mod image_library;
mod launchpads;
mod observability;
mod paths;
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
mod wallet;

use crate::{
    config::{RawConfig, normalize_raw_config},
    image_library::{
        build_image_library_payload, create_category, delete_image, save_data_url_image,
        update_image,
    },
    launchpads::launchpad_registry,
    observability::{
        log_event, new_trace_context, persist_launch_report, update_persisted_launch_report,
    },
    providers::{provider_availability_registry, provider_registry},
    pump_native::{try_compile_native_pump, warm_default_lookup_tables, warm_pump_global_state},
    report::{LaunchReport, build_report, render_report},
    reports_browser::{list_persisted_reports, read_persisted_report},
    rpc::{send_transactions_for_transport, simulate_transactions, spawn_blockhash_refresh_task},
    runtime::{
        RuntimeRegistry, RuntimeRequest, RuntimeResponse, fail_worker, heartbeat_worker,
        list_workers, restore_workers, start_worker, stop_worker,
    },
    strategies::strategy_registry,
    transport::{
        build_transport_plan, configured_helius_sender_endpoint, configured_jito_bundle_endpoints,
        estimate_transaction_count,
    },
    ui_bridge::{build_raw_config_from_form, quote_from_form, upload_metadata_from_form},
    ui_config::{
        create_default_persistent_config, read_persistent_config, write_persistent_config,
    },
    vamp::{fetch_imported_token_metadata, import_remote_image_to_library},
    wallet::{
        enrich_wallet_statuses, list_solana_env_wallets, load_solana_wallet_by_env_key,
        public_key_from_secret, selected_wallet_key_or_default,
    },
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, Response, StatusCode, header},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
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
}

#[derive(Deserialize, Default)]
struct MetadataUploadRequest {
    form: Option<Value>,
}

#[derive(Deserialize, Default)]
struct SettingsSaveRequest {
    config: Option<Value>,
}

#[derive(Deserialize, Default)]
struct ReportsQuery {
    sort: Option<String>,
}

#[derive(Deserialize, Default)]
struct ReportViewQuery {
    id: Option<String>,
}

#[derive(Deserialize, Default)]
struct QuoteQuery {
    mode: Option<String>,
    amount: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Default)]
struct UploadImageRequest {
    dataUrl: Option<String>,
    filename: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Default)]
struct VampRequest {
    contractAddress: Option<String>,
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
        .or_else(|| {
            std::env::var("LAUNCHDECK_ENGINE_PORT")
                .ok()
                .and_then(|value| value.parse::<u16>().ok())
        })
        .unwrap_or(8789)
}

fn configured_auth_token() -> Option<String> {
    let token = std::env::var("LAUNCHDECK_ENGINE_AUTH_TOKEN").unwrap_or_default();
    let trimmed = token.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn configured_runtime_state_path() -> std::path::PathBuf {
    paths::runtime_state_path()
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
    if let Ok(explicit) = std::env::var("HELIUS_RPC_URL") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Ok(api_key) = std::env::var("HELIUS_API_KEY") {
        let trimmed = api_key.trim();
        if !trimmed.is_empty() {
            return format!("https://mainnet.helius-rpc.com/?api-key={trimmed}");
        }
    }
    "http://127.0.0.1:8899".to_string()
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
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
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
        .header(header::CACHE_CONTROL, "no-store")
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

fn render_report_value(report: &Value) -> Value {
    serde_json::from_value::<LaunchReport>(report.clone())
        .map(|parsed| Value::String(render_report(&parsed)))
        .unwrap_or_else(|_| Value::String(String::new()))
}

fn set_report_timing(report: &mut Value, key: &str, value_ms: u128) {
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

fn refresh_report_benchmark(report: &mut Value) {
    let timings = report
        .get("execution")
        .and_then(|value| value.get("timings"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let sent_items = report
        .get("execution")
        .and_then(|value| value.get("sent"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let sent = sent_items
        .into_iter()
        .map(|item| {
            let send_height = item.get("sendObservedBlockHeight").and_then(Value::as_u64);
            let confirmed_height = item
                .get("confirmedObservedBlockHeight")
                .and_then(Value::as_u64);
            json!({
                "label": item.get("label").cloned().unwrap_or_else(|| Value::String("(unknown)".to_string())),
                "signature": item.get("signature").cloned().unwrap_or(Value::Null),
                "confirmationStatus": item.get("confirmationStatus").cloned().unwrap_or(Value::Null),
                "sendBlockHeight": send_height,
                "confirmedBlockHeight": confirmed_height,
                "blocksToConfirm": match (send_height, confirmed_height) {
                    (Some(send_height), Some(confirmed_height)) => Some(confirmed_height.saturating_sub(send_height)),
                    _ => None,
                },
                "confirmedSlot": item.get("confirmedSlot").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    report["benchmark"] = json!({
        "timings": timings,
        "sent": sent,
    });
}

async fn build_status_payload(
    state: &Arc<AppState>,
    requested_wallet: &str,
) -> Result<Value, String> {
    let raw_wallets = list_solana_env_wallets();
    if !requested_wallet.trim().is_empty()
        && !raw_wallets
            .iter()
            .any(|wallet| wallet.envKey == requested_wallet.trim())
    {
        return Err(format!(
            "Unknown wallet env key: {}",
            requested_wallet.trim()
        ));
    }
    let selected_wallet_key = selected_wallet_key_or_default(requested_wallet);
    let rpc_url = configured_rpc_url();
    let wallets = enrich_wallet_statuses(&rpc_url, USD1_MINT, &raw_wallets).await;
    let selected_wallet = selected_wallet_key.as_ref().and_then(|env_key| {
        wallets
            .iter()
            .find(|wallet| wallet.envKey == *env_key)
            .cloned()
    });
    let runtime_workers = list_workers(&state.runtime).await;
    Ok(json!({
        "ok": true,
        "service": "launchdeck-engine",
        "engineBackend": "rust",
        "implemented": true,
        "executionMode": "rust-native-only",
        "message": "Rust engine is online with native Pump execution, native RPC transport, and native runtime workers. Unsupported requests fail explicitly instead of falling back to JavaScript.",
        "rpcUrl": rpc_url,
        "connected": selected_wallet.is_some(),
        "wallets": wallets,
        "selectedWalletKey": selected_wallet_key,
        "wallet": selected_wallet.as_ref().and_then(|wallet| wallet.publicKey.clone()),
        "balanceLamports": selected_wallet.as_ref().and_then(|wallet| wallet.balanceLamports),
        "balanceSol": selected_wallet.as_ref().and_then(|wallet| wallet.balanceSol),
        "usd1Balance": selected_wallet.as_ref().and_then(|wallet| wallet.usd1Balance),
        "providers": provider_availability_registry(),
        "providerRegistry": provider_registry(),
        "launchpads": launchpad_registry(),
        "transport": {
            "heliusSenderEndpoint": configured_helius_sender_endpoint(),
            "jitoBundleEndpoints": configured_jito_bundle_endpoints(),
        },
        "runtime": {
            "statePath": state.runtime.storage_path,
            "workerCount": runtime_workers.len(),
        },
        "runtimeWorkers": runtime_workers,
    }))
}

async fn engine_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<StatusRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    authorize(&headers, &state)?;
    build_status_payload(&state, &payload.wallet.unwrap_or_default())
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
    let action_started_ms = trace.startedAtMs;
    let action = payload.action.unwrap_or_else(|| "unknown".to_string());
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
    let form_prepare_started_ms = current_time_ms();
    let (raw_config_value, prepared_metadata_uri, form_to_raw_config_ms) =
        if let Some(raw_config_value) = payload.raw_config {
            (raw_config_value, None, None)
        } else if let Some(form_value) = payload.form.clone() {
            let (raw_config, metadata_uri) = build_raw_config_from_form(&action, form_value)
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
                Some(current_time_ms().saturating_sub(form_prepare_started_ms)),
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
    let normalize_started_ms = current_time_ms();
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
    let normalize_config_ms = current_time_ms().saturating_sub(normalize_started_ms);
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
    let wallet_load_started_ms = current_time_ms();
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
    let wallet_load_ms = current_time_ms().saturating_sub(wallet_load_started_ms);
    let preview_agent_authority = if normalized.mode == "regular" || normalized.mode == "cashback" {
        None
    } else if !normalized.agent.authority.trim().is_empty() {
        Some(normalized.agent.authority.clone())
    } else {
        Some(creator_public_key.clone())
    };
    let transport_plan = build_transport_plan(
        &normalized.execution,
        estimate_transaction_count(&normalized),
    );
    let should_persist_report = normalized.tx.writeReport || action == "send";
    if action == "build" {
        let report_build_started_ms = current_time_ms();
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
        let report_build_ms = current_time_ms().saturating_sub(report_build_started_ms);
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
        if let Some(value) = form_to_raw_config_ms {
            set_report_timing(&mut report_value, "formToRawConfigMs", value);
        }
        set_report_timing(&mut report_value, "normalizeConfigMs", normalize_config_ms);
        set_report_timing(&mut report_value, "walletLoadMs", wallet_load_ms);
        set_report_timing(&mut report_value, "reportBuildMs", report_build_ms);
        let send_log_path = if should_persist_report {
            let persist_started_ms = current_time_ms();
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
                current_time_ms().saturating_sub(persist_started_ms),
            );
            report_value["outPath"] = Value::String(path.clone());
            Some(path)
        } else {
            None
        };
        set_report_timing(
            &mut report_value,
            "totalElapsedMs",
            current_time_ms().saturating_sub(action_started_ms),
        );
        refresh_report_benchmark(&mut report_value);
        if let Some(path) = send_log_path.as_ref() {
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
            "normalizedConfig": normalized,
            "transportPlan": transport_plan,
            "report": report_value,
            "sendLogPath": send_log_path,
            "text": render_report_value(&report_value),
            "metadataUri": prepared_metadata_uri,
        }));
    }
    let compile_started_ms = current_time_ms();
    let native_artifacts = try_compile_native_pump(
        &configured_rpc_url(),
        &normalized,
        &transport_plan,
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
    let (compiled_transactions, mut report_value, text_value, assembly_executor, compile_breakdown) = (
        native.compiled_transactions,
        native.report,
        Value::String(native.text),
        "rust-native".to_string(),
        native.compile_timings,
    );
    set_report_timing(
        &mut report_value,
        "compileTransactionsMs",
        compile_transactions_ms,
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

    if action == "simulate" {
        let simulate_started_ms = current_time_ms();
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
        if let Some(value) = form_to_raw_config_ms {
            set_report_timing(&mut report, "formToRawConfigMs", value);
        }
        set_report_timing(&mut report, "normalizeConfigMs", normalize_config_ms);
        set_report_timing(&mut report, "walletLoadMs", wallet_load_ms);
        set_report_timing(
            &mut report,
            "compileTransactionsMs",
            compile_transactions_ms,
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
        set_report_timing(
            &mut report,
            "simulateMs",
            current_time_ms().saturating_sub(simulate_started_ms),
        );
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
        let send_log_path = if should_persist_report {
            let persist_started_ms = current_time_ms();
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
                current_time_ms().saturating_sub(persist_started_ms),
            );
            report["outPath"] = Value::String(path.clone());
            Some(path)
        } else {
            None
        };
        set_report_timing(
            &mut report,
            "totalElapsedMs",
            current_time_ms().saturating_sub(action_started_ms),
        );
        refresh_report_benchmark(&mut report);
        if let Some(path) = send_log_path.as_ref() {
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
            "normalizedConfig": normalized,
            "transportPlan": transport_plan,
            "assemblyExecutor": assembly_executor,
            "report": report,
            "sendLogPath": send_log_path,
            "text": render_report_value(&report),
            "metadataUri": prepared_metadata_uri,
        }));
    }

    if action == "send" {
        let execution_class = transport_plan.executionClass.clone();
        let (sent, warnings, send_timing) = send_transactions_for_transport(
            &configured_rpc_url(),
            &transport_plan,
            &compiled_transactions,
            &normalized.execution.commitment,
            normalized.execution.skipPreflight,
            normalized.execution.trackSendBlockHeight,
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
        if let Some(value) = form_to_raw_config_ms {
            set_report_timing(&mut report, "formToRawConfigMs", value);
        }
        set_report_timing(&mut report, "normalizeConfigMs", normalize_config_ms);
        set_report_timing(&mut report, "walletLoadMs", wallet_load_ms);
        set_report_timing(
            &mut report,
            "compileTransactionsMs",
            compile_transactions_ms,
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
        set_report_timing(
            &mut report,
            "sendMs",
            send_timing.submit_ms.saturating_add(send_timing.confirm_ms),
        );
        set_report_timing(&mut report, "sendSubmitMs", send_timing.submit_ms);
        set_report_timing(&mut report, "sendConfirmMs", send_timing.confirm_ms);
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
        let send_log_path = if should_persist_report {
            let persist_started_ms = current_time_ms();
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
                current_time_ms().saturating_sub(persist_started_ms),
            );
            report["outPath"] = Value::String(path.clone());
            Some(path)
        } else {
            None
        };
        set_report_timing(
            &mut report,
            "totalElapsedMs",
            current_time_ms().saturating_sub(action_started_ms),
        );
        refresh_report_benchmark(&mut report);
        if let Some(path) = send_log_path.as_ref() {
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
            "normalizedConfig": normalized,
            "transportPlan": transport_plan,
            "assemblyExecutor": assembly_executor,
            "report": report,
            "sendLogPath": send_log_path,
            "text": render_report_value(&report),
            "metadataUri": prepared_metadata_uri,
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
        "normalizedConfig": normalized,
        "transportPlan": transport_plan,
        "assemblyExecutor": assembly_executor,
        "report": report_value,
        "text": text_value,
        "metadataUri": prepared_metadata_uri,
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
        "strategies": strategy_registry(),
        "engine": {
            "backend": "rust",
            "url": configured_base_url(),
        },
    })
}

async fn api_bootstrap(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut payload = build_status_payload(&state, "").await.map_err(|error| {
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
    Ok(Json(payload))
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

async fn api_status(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StatusQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let wallet = query.wallet.unwrap_or_default();
    let mut payload = build_status_payload(&state, &wallet)
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
    Ok(Json(payload))
}

async fn api_run(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RunRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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
    let mut response = execute_engine_action_payload(
        &state,
        EngineRequest {
            action: Some(action.clone()),
            form: payload.form,
            raw_config: None,
        },
    )
    .await?;
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
                backend_total_ms.unwrap_or_default().saturating_add(pre_request_ms),
            );
            set_report_timing(report, "clientPreRequestMs", pre_request_ms);
        }
        refresh_report_benchmark(report);
        rendered_text = Some(render_report_value(report));
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
    Json(json!({
        "ok": true,
        "backend": "rust",
        "url": configured_base_url(),
        "engine": engine.0,
    }))
}

async fn api_quote(
    Query(query): Query<QuoteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mode = query.mode.unwrap_or_default();
    let amount = query.amount.unwrap_or_default();
    if mode.trim().is_empty() || amount.trim().is_empty() {
        return Ok(Json(json!({
            "ok": true,
            "quote": Value::Null,
        })));
    }
    let quote = quote_from_form(
        &configured_rpc_url(),
        json!({ "mode": mode, "amount": amount }),
    )
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
        "quote": quote,
    })))
}

async fn api_settings_get() -> Json<Value> {
    Json(build_settings_payload())
}

async fn api_settings_save(
    Json(payload): Json<SettingsSaveRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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
    Ok(Json(response))
}

async fn api_reports_list(Query(query): Query<ReportsQuery>) -> Json<Value> {
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
    Json(json!({
        "ok": true,
        "sort": sort,
        "reports": list_persisted_reports(sort),
    }))
}

async fn api_reports_view(
    Query(query): Query<ReportViewQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id = query.id.unwrap_or_default();
    let (entry, text) = read_persisted_report(&id).map_err(|error| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": error,
            })),
        )
    })?;
    Ok(Json(json!({
        "ok": true,
        "entry": entry,
        "text": text,
    })))
}

async fn api_upload_image(
    Json(payload): Json<UploadImageRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let record = save_data_url_image(
        payload.dataUrl.as_deref().unwrap_or_default(),
        payload.filename.as_deref().unwrap_or("image"),
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
    Ok(Json(json!({
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
    })))
}

async fn api_metadata_upload(
    Json(payload): Json<MetadataUploadRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let form = payload.form.unwrap_or(Value::Null);
    let metadata_uri = upload_metadata_from_form(form).await.map_err(|error| {
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
        "metadataUri": metadata_uri,
    })))
}

async fn api_images_list(
    Query(query): Query<ImagesQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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
    Ok(Json(
        serde_json::to_value(payload).unwrap_or(json!({"ok": false})),
    ))
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
    let imported = fetch_imported_token_metadata(&mint.to_string())
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
    if !imported.imageUrl.is_empty() {
        match import_remote_image_to_library(
            &imported.imageUrl,
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
            }
            Ok(None) => {}
            Err(error) => warning = error,
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
        },
        "image": image,
        "warning": warning,
    })))
}

async fn static_handler(
    AxumPath(requested): AxumPath<String>,
) -> Result<Response<Body>, (StatusCode, Json<Value>)> {
    if requested.is_empty() || requested == "index.html" {
        return file_response(paths::ui_dir().join("index.html"));
    }
    if requested.starts_with("uploads/") {
        let file_name = std::path::Path::new(requested.trim_start_matches("uploads/"))
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        return file_response(paths::uploads_dir().join(file_name));
    }
    let safe_name = std::path::Path::new(&requested)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    if safe_name.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": "Not found",
            })),
        ));
    }
    file_response(paths::ui_dir().join(safe_name))
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
    let rpc_url = configured_rpc_url();
    let state = Arc::new(AppState {
        auth_token: configured_auth_token(),
        runtime: Arc::new(RuntimeRegistry::new(configured_runtime_state_path())),
    });
    spawn_blockhash_refresh_task(rpc_url, "confirmed");
    let restored_workers = restore_workers(&state.runtime).await;
    let app = Router::new()
        .route("/health", get(health))
        .route(
            "/",
            get(|| async { file_response(paths::ui_dir().join("index.html")) }),
        )
        .route(
            "/index.html",
            get(|| async { file_response(paths::ui_dir().join("index.html")) }),
        )
        .route("/api/bootstrap", get(api_bootstrap))
        .route("/api/lookup-tables/warm", post(api_lookup_tables_warm))
        .route("/api/pump-global/warm", post(api_pump_global_warm))
        .route("/api/status", get(api_status))
        .route("/api/run", post(api_run))
        .route("/api/engine/health", get(api_engine_health))
        .route("/api/quote", get(api_quote))
        .route(
            "/api/settings",
            get(api_settings_get).post(api_settings_save),
        )
        .route("/api/settings/save", post(api_settings_save))
        .route("/api/reports", get(api_reports_list))
        .route("/api/reports/view", get(api_reports_view))
        .route("/api/upload-image", post(api_upload_image))
        .route("/api/metadata/upload", post(api_metadata_upload))
        .route("/api/images", get(api_images_list))
        .route("/api/images/update", post(api_image_update))
        .route("/api/images/categories", post(api_image_category_create))
        .route("/api/images/delete", post(api_image_delete))
        .route("/api/vamp", post(api_vamp_import))
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
        .route("/{*path}", get(static_handler))
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
