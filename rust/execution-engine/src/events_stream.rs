#![allow(dead_code)]

//! SSE endpoint that streams live balance + trade events to extension surfaces.
//!
//! The extension opens a single `EventSource` per service-worker lifetime. The
//! first event is always a `snapshot` containing the full wallet state, followed
//! by incremental `balance` / `trade` / `connectionState` events pushed from
//! `shared_extension_runtime::balance_stream`.

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shared_extension_runtime::balance_stream::StreamEvent;
use std::{convert::Infallible, time::Duration};
use tokio_stream::wrappers::BroadcastStream;

use crate::extension_api::AppState;

const SSE_HEARTBEAT_SECS: u64 = 15;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveMintEntry {
    pub wallet_key: String,
    pub mint: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveMintRequest {
    #[serde(default)]
    pub entries: Vec<ActiveMintEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalancePresenceRequest {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub reason: String,
}

/// `GET /api/extension/events/stream`
///
/// This route sits behind the standard authenticated extension middleware, so
/// callers must present the same bearer token used by the rest of
/// `/api/extension/*`.
pub async fn events_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>> + Send + 'static> {
    let handle = state.balance_stream();

    let initial_snapshot = handle.snapshot().await;
    let snapshot_data =
        serde_json::to_string(&initial_snapshot).unwrap_or_else(|_| String::from("{}"));

    let snapshot_event = Event::default().event("snapshot").data(snapshot_data);

    let receiver = handle.subscribe_events();
    let live = BroadcastStream::new(receiver).filter_map(|result| async move {
        let event = match result {
            Ok(event) => event,
            Err(_) => return None,
        };
        let (name, data) = match event {
            StreamEvent::Balance(payload) => ("balance", serde_json::to_string(&payload).ok()?),
            StreamEvent::Trade(payload) => ("trade", serde_json::to_string(&payload).ok()?),
            StreamEvent::ConnectionState {
                state: connection_state,
                error,
            } => (
                "connectionState",
                serde_json::to_string(&ConnectionStatePayload {
                    state: connection_state,
                    error,
                })
                .ok()?,
            ),
        };
        Some(Ok::<_, Infallible>(Event::default().event(name).data(data)))
    });

    let initial = stream::once(async move { Ok::<_, Infallible>(snapshot_event) });
    let combined = initial.chain(live);

    Sse::new(combined).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(SSE_HEARTBEAT_SECS))
            .text("ping"),
    )
}

/// `POST /api/extension/events/active-mint`
///
/// Registers the union of mints the extension surfaces are currently viewing.
/// Rust diffs against its current set and adjusts the `accountSubscribe`
/// registrations for every wallet × mint ATA pair on the Solana WS. The
/// request body replaces the set (it is not a delta).
pub async fn set_active_mints(
    State(state): State<AppState>,
    Json(request): Json<ActiveMintRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mints: Vec<String> = request
        .entries
        .into_iter()
        .map(|entry| entry.mint.trim().to_string())
        .filter(|mint| !mint.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    state.balance_stream().set_active_mints(mints);
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /api/extension/events/presence`
///
/// Explicitly tells the engine whether any extension surface currently needs
/// live balance/account subscriptions. The SSE channel may stay connected while
/// the Solana websocket is paused.
pub async fn set_balance_presence(
    State(state): State<AppState>,
    Json(request): Json<BalancePresenceRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let _reason = request.reason.trim();
    state.balance_stream().set_demand(request.active);
    Ok(Json(
        serde_json::json!({ "ok": true, "active": request.active }),
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionStatePayload {
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}
