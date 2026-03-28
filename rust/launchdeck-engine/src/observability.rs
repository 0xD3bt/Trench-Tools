#![allow(non_snake_case, dead_code)]

use serde::Serialize;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct TraceContext {
    pub traceId: String,
    pub startedAtMs: u128,
}

pub fn new_trace_context() -> TraceContext {
    let started_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    TraceContext {
        traceId: Uuid::new_v4().to_string(),
        startedAtMs: started_at_ms,
    }
}

pub fn log_event(event: &str, trace_id: &str, payload: Value) {
    let line = json!({
        "event": event,
        "traceId": trace_id,
        "payload": payload,
    });
    println!("{line}");
}
