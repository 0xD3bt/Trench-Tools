#![allow(non_snake_case, dead_code)]

use crate::paths;
use serde::Serialize;
use serde_json::Value;
use std::{fs, time::UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct ReportSummaryEntry {
    pub id: String,
    pub fileName: String,
    pub action: String,
    pub traceId: String,
    pub mint: String,
    pub writtenAtMs: u128,
    pub displayTime: String,
    pub provider: String,
    pub transportType: String,
    pub signatureCount: usize,
}

fn format_report_time(written_at_ms: u128) -> String {
    if written_at_ms == 0 {
        return "Unknown time".to_string();
    }
    written_at_ms.to_string()
}

fn safe_json_parse(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::Object(Default::default()))
}

pub fn build_report_summary_entry(file_name: &str) -> Result<ReportSummaryEntry, String> {
    let file_path = paths::reports_dir().join(file_name);
    let stat = fs::metadata(&file_path).map_err(|error| error.to_string())?;
    let raw = fs::read_to_string(&file_path).map_err(|error| error.to_string())?;
    let payload = safe_json_parse(&raw);
    let report = payload.get("report").cloned().unwrap_or(Value::Null);
    let execution = report.get("execution").cloned().unwrap_or(Value::Null);
    let written_at_ms = payload
        .get("writtenAtMs")
        .and_then(Value::as_u64)
        .map(|value| value as u128)
        .unwrap_or_else(|| {
            stat.modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_millis())
                .unwrap_or(0)
        });
    Ok(ReportSummaryEntry {
        id: file_name.to_string(),
        fileName: file_name.to_string(),
        action: payload
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .trim()
            .to_string(),
        traceId: payload
            .get("traceId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        mint: payload
            .get("mint")
            .and_then(Value::as_str)
            .or_else(|| report.get("mint").and_then(Value::as_str))
            .unwrap_or_default()
            .trim()
            .to_string(),
        writtenAtMs: written_at_ms,
        displayTime: format_report_time(written_at_ms),
        provider: execution
            .get("resolvedProvider")
            .and_then(Value::as_str)
            .or_else(|| execution.get("provider").and_then(Value::as_str))
            .unwrap_or_default()
            .trim()
            .to_string(),
        transportType: execution
            .get("transportType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        signatureCount: payload
            .get("signatures")
            .and_then(Value::as_array)
            .map(|entries| entries.len())
            .unwrap_or(0),
    })
}

pub fn list_persisted_reports(sort: &str) -> Vec<ReportSummaryEntry> {
    let dir = paths::reports_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };
    let mut reports = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if !file_name.to_ascii_lowercase().ends_with(".json") {
                return None;
            }
            build_report_summary_entry(&file_name).ok()
        })
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| {
        if sort == "oldest" {
            left.writtenAtMs.cmp(&right.writtenAtMs)
        } else {
            right.writtenAtMs.cmp(&left.writtenAtMs)
        }
    });
    reports
}

fn build_report_text(file_name: &str, payload: &Value, fallback_raw: &str) -> String {
    let report = payload.get("report").cloned().unwrap_or(Value::Null);
    let execution = report.get("execution").cloned().unwrap_or(Value::Null);
    let mut lines = vec![
        format!(
            "[{}] {}",
            payload
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_uppercase(),
            format_report_time(payload.get("writtenAtMs").and_then(Value::as_u64).unwrap_or(0) as u128)
        ),
        format!("File: {file_name}"),
        format!(
            "Trace: {}",
            payload.get("traceId").and_then(Value::as_str).unwrap_or("(missing)")
        ),
        format!(
            "Mint: {}",
            payload
                .get("mint")
                .and_then(Value::as_str)
                .or_else(|| report.get("mint").and_then(Value::as_str))
                .unwrap_or("(missing)")
        ),
    ];
    if let Some(provider) = execution
        .get("resolvedProvider")
        .and_then(Value::as_str)
        .or_else(|| execution.get("provider").and_then(Value::as_str))
    {
        if !provider.is_empty() {
            lines.push(format!("Provider: {provider}"));
        }
    }
    if let Some(transport) = execution.get("transportType").and_then(Value::as_str) {
        if !transport.is_empty() {
            lines.push(format!("Transport: {transport}"));
        }
    }
    if let Some(profile) = execution.get("resolvedEndpointProfile").and_then(Value::as_str) {
        if !profile.is_empty() {
            lines.push(format!("Endpoint profile: {profile}"));
        }
    }
    if let Some(signatures) = payload.get("signatures").and_then(Value::as_array) {
        if !signatures.is_empty() {
            lines.push(format!(
                "Signatures: {}",
                signatures
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if let Some(sent_items) = execution.get("sent").and_then(Value::as_array) {
        if !sent_items.is_empty() {
            lines.push(String::new());
            lines.push("Sent:".to_string());
            for sent in sent_items {
                let mut summary = format!(
                    "- {}: signature={} | status={}",
                    sent.get("label").and_then(Value::as_str).unwrap_or("(unknown)"),
                    sent.get("signature").and_then(Value::as_str).unwrap_or("(missing)"),
                    sent.get("confirmationStatus").and_then(Value::as_str).unwrap_or("(pending)")
                );
                if let Some(block_height) =
                    sent.get("sendObservedBlockHeight").and_then(Value::as_u64)
                {
                    summary.push_str(&format!(" | send block height={block_height}"));
                }
                if let Some(block_height) = sent
                    .get("confirmedObservedBlockHeight")
                    .and_then(Value::as_u64)
                {
                    summary.push_str(&format!(" | confirmed block height={block_height}"));
                }
                if let (Some(send_height), Some(confirmed_height)) = (
                    sent.get("sendObservedBlockHeight").and_then(Value::as_u64),
                    sent.get("confirmedObservedBlockHeight").and_then(Value::as_u64),
                ) {
                    summary.push_str(&format!(
                        " | blocks to confirm={}",
                        confirmed_height.saturating_sub(send_height)
                    ));
                }
                if let Some(slot) = sent.get("confirmedSlot").and_then(Value::as_u64) {
                    summary.push_str(&format!(" | confirmed slot={slot}"));
                }
                lines.push(summary);
            }
        }
    }
    if let Some(benchmark) = report.get("benchmark").and_then(Value::as_object) {
        lines.push("Benchmark:".to_string());
        if let Some(timings) = benchmark.get("timings").and_then(Value::as_object) {
            let mut timing_parts = Vec::new();
            let push_timing = |parts: &mut Vec<String>, key: &str, label: &str| {
                if let Some(value) = timings.get(key).and_then(Value::as_u64) {
                    parts.push(format!("{label}={value}ms"));
                }
            };
            push_timing(&mut timing_parts, "totalElapsedMs", "total");
            push_timing(&mut timing_parts, "formToRawConfigMs", "form");
            push_timing(&mut timing_parts, "normalizeConfigMs", "normalize");
            push_timing(&mut timing_parts, "walletLoadMs", "wallet");
            push_timing(&mut timing_parts, "reportBuildMs", "report");
            push_timing(&mut timing_parts, "compileTransactionsMs", "compile");
            push_timing(&mut timing_parts, "simulateMs", "simulate");
            push_timing(&mut timing_parts, "sendMs", "send");
            push_timing(&mut timing_parts, "persistReportMs", "persist");
            if !timing_parts.is_empty() {
                lines.push(format!("  Timings: {}", timing_parts.join(" | ")));
            }
        }
        if let Some(sent_items) = benchmark.get("sent").and_then(Value::as_array) {
            for sent in sent_items {
                let label = sent.get("label").and_then(Value::as_str).unwrap_or("(unknown)");
                let mut sent_parts = Vec::new();
                if let Some(value) = sent.get("sendBlockHeight").and_then(Value::as_u64) {
                    sent_parts.push(format!("send block height={value}"));
                }
                if let Some(value) = sent.get("confirmedBlockHeight").and_then(Value::as_u64) {
                    sent_parts.push(format!("confirmed block height={value}"));
                }
                if let Some(value) = sent.get("blocksToConfirm").and_then(Value::as_u64) {
                    sent_parts.push(format!("blocks to confirm={value}"));
                }
                if let Some(value) = sent.get("confirmedSlot").and_then(Value::as_u64) {
                    sent_parts.push(format!("confirmed slot={value}"));
                }
                if !sent_parts.is_empty() {
                    lines.push(format!("  {}: {}", label, sent_parts.join(" | ")));
                }
            }
        }
    }
    lines.push(String::new());
    lines.push("--- Report JSON ---".to_string());
    lines.push(
        serde_json::to_string_pretty(if report.is_null() { payload } else { &report })
            .unwrap_or_default(),
    );
    if report.is_null() {
        lines.push(String::new());
        lines.push("--- Raw File ---".to_string());
        lines.push(fallback_raw.to_string());
    }
    lines.join("\n")
}

pub fn read_persisted_report(file_name: &str) -> Result<(ReportSummaryEntry, String), String> {
    let safe_file_name = std::path::Path::new(file_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if safe_file_name.is_empty() {
        return Err("Report id is required.".to_string());
    }
    let file_path = paths::reports_dir().join(&safe_file_name);
    if !file_path.exists() {
        return Err("Report not found.".to_string());
    }
    let raw = fs::read_to_string(&file_path).map_err(|error| error.to_string())?;
    let payload = safe_json_parse(&raw);
    Ok((
        build_report_summary_entry(&safe_file_name)?,
        build_report_text(&safe_file_name, &payload, &raw),
    ))
}
