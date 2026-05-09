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

use crate::extension_api::{AppState, ExtensionActiveMarkRequest};

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
        let (name, data) = stream_event_to_sse(event)?;
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

fn stream_event_to_sse(event: StreamEvent) -> Option<(&'static str, String)> {
    match event {
        StreamEvent::Balance(payload) => Some(("balance", serde_json::to_string(&payload).ok()?)),
        StreamEvent::TokenBalanceCache(_) => None,
        StreamEvent::Trade(payload) => Some(("trade", serde_json::to_string(&payload).ok()?)),
        StreamEvent::Mark(payload) => Some(("mark", serde_json::to_string(&payload).ok()?)),
        StreamEvent::MarketAccount(_) => None,
        StreamEvent::Diagnostic(payload) => {
            Some(("diagnostic", serde_json::to_string(&payload).ok()?))
        }
        StreamEvent::ConnectionState {
            state: connection_state,
            error,
        } => Some((
            "connectionState",
            serde_json::to_string(&ConnectionStatePayload {
                state: connection_state,
                error,
            })
            .ok()?,
        )),
    }
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

/// `POST /api/extension/events/active-mark`
///
/// Registers the currently visible open position whose PnL display should be
/// marked from Rust-owned route and market data.
pub async fn set_active_mark(
    State(state): State<AppState>,
    Json(request): Json<ExtensionActiveMarkRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state
        .set_active_mark_target(request)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionStatePayload {
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_extension_runtime::balance_stream::{
        MarkEventPayload, MarketAccountEventPayload, TokenBalanceCacheEventPayload,
    };

    #[test]
    fn stream_event_maps_mark_to_sse_mark() {
        let payload = MarkEventPayload {
            surface_id: Some("content:test".to_string()),
            mark_revision: 1,
            mint: "Mint111".to_string(),
            wallet_keys: vec!["wallet1".to_string()],
            wallet_group_id: None,
            token_balance: Some(1.0),
            token_balance_raw: Some(1),
            holding_value_sol: Some(0.5),
            holding: Some(0.5),
            pnl_gross: Some(0.1),
            pnl_net: Some(0.09),
            pnl_percent_gross: Some(10.0),
            pnl_percent_net: Some(9.0),
            quote_source: Some("live-mark:test".to_string()),
            commitment: Some("processed".to_string()),
            slot: Some(7),
            at_ms: 8,
        };

        let (name, data) =
            stream_event_to_sse(StreamEvent::Mark(payload)).expect("mark event maps");

        assert_eq!(name, "mark");
        assert!(data.contains("\"holdingValueSol\":0.5"));
    }

    #[test]
    fn stream_event_does_not_expose_internal_market_account_events() {
        let event = StreamEvent::MarketAccount(MarketAccountEventPayload {
            account: "Account111".to_string(),
            slot: Some(1),
            at_ms: 2,
        });

        assert!(stream_event_to_sse(event).is_none());
    }

    #[test]
    fn stream_event_does_not_expose_internal_token_cache_events() {
        let event = StreamEvent::TokenBalanceCache(TokenBalanceCacheEventPayload {
            env_key: "SOLANA_PRIVATE_KEY".to_string(),
            token_mint: "Mint111".to_string(),
            token_balance: 42.0,
            commitment: "confirmed".to_string(),
            source: Some("accountSubscribe".to_string()),
            slot: Some(1),
            at_ms: 2,
        });

        assert!(stream_event_to_sse(event).is_none());
    }
}
