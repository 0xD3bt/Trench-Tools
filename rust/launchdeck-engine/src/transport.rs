#![allow(non_snake_case, dead_code)]

use serde::Serialize;
use std::env;

use crate::{config::NormalizedExecution, providers::get_provider_meta};

const DEFAULT_JITO_BUNDLE_BASE_URLS: [&str; 9] = [
    "https://ny.mainnet.block-engine.jito.wtf",
    "https://frankfurt.mainnet.block-engine.jito.wtf",
    "https://amsterdam.mainnet.block-engine.jito.wtf",
    "https://london.mainnet.block-engine.jito.wtf",
    "https://slc.mainnet.block-engine.jito.wtf",
    "https://mainnet.block-engine.jito.wtf",
    "https://singapore.mainnet.block-engine.jito.wtf",
    "https://tokyo.mainnet.block-engine.jito.wtf",
    "https://dublin.mainnet.block-engine.jito.wtf",
];

#[derive(Debug, Clone, Serialize)]
pub struct JitoBundleEndpoint {
    pub name: String,
    pub send: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransportPlan {
    pub requestedProvider: String,
    pub resolvedProvider: String,
    pub executionClass: String,
    pub verified: bool,
    pub supportsBundle: bool,
    pub jitoBundleEndpoints: Vec<JitoBundleEndpoint>,
    pub warnings: Vec<String>,
}

fn normalize_provider(provider: &str) -> String {
    if provider.trim().is_empty() {
        "auto".to_string()
    } else {
        provider.trim().to_lowercase()
    }
}

pub fn resolved_provider(execution: &NormalizedExecution, transaction_count: usize) -> String {
    let requested = normalize_provider(&execution.provider);
    if requested != "auto" {
        return requested;
    }
    if transaction_count > 1 && execution.policy.trim().eq_ignore_ascii_case("safe") {
        return "jito".to_string();
    }
    "helius".to_string()
}

pub fn execution_class(execution: &NormalizedExecution, transaction_count: usize) -> String {
    let provider = resolved_provider(execution, transaction_count);
    let meta = get_provider_meta(&provider);
    if transaction_count <= 1 {
        return "single".to_string();
    }
    if execution.policy.trim().eq_ignore_ascii_case("safe")
        && provider == "jito"
        && meta.supportsBundle
    {
        return "bundle".to_string();
    }
    "sequential".to_string()
}

pub fn configured_jito_bundle_endpoints() -> Vec<JitoBundleEndpoint> {
    let explicit_send = env::var("JITO_SEND_BUNDLE_ENDPOINT")
        .unwrap_or_default()
        .trim()
        .to_string();
    let explicit_status = env::var("JITO_BUNDLE_STATUS_ENDPOINT")
        .unwrap_or_default()
        .trim()
        .to_string();
    if !explicit_send.is_empty() || !explicit_status.is_empty() {
        if !explicit_send.is_empty() && !explicit_status.is_empty() {
            return vec![JitoBundleEndpoint {
                name: "custom".to_string(),
                send: explicit_send,
                status: explicit_status,
            }];
        }
        return vec![];
    }

    let configured_bases = env::var("JITO_BUNDLE_BASE_URLS")
        .or_else(|_| env::var("JITO_BUNDLE_BASE_URL"))
        .unwrap_or_default();
    let bases: Vec<String> = if configured_bases.trim().is_empty() {
        DEFAULT_JITO_BUNDLE_BASE_URLS
            .iter()
            .map(|entry| entry.to_string())
            .collect()
    } else {
        configured_bases
            .split(',')
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect()
    };
    bases
        .into_iter()
        .map(|base| JitoBundleEndpoint {
            name: base
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .to_string(),
            send: format!("{base}/api/v1/bundles"),
            status: format!("{base}/api/v1/getBundleStatuses"),
        })
        .collect()
}

pub fn build_transport_plan(
    execution: &NormalizedExecution,
    transaction_count: usize,
) -> TransportPlan {
    let requested = normalize_provider(&execution.provider);
    let resolved = resolved_provider(execution, transaction_count);
    let class = execution_class(execution, transaction_count);
    let meta = get_provider_meta(&resolved);
    let mut warnings = Vec::new();
    if !meta.verified {
        warnings.push(format!(
            "Provider {} is currently marked unverified in this environment.",
            resolved
        ));
    }
    if execution.policy.trim().eq_ignore_ascii_case("safe")
        && meta.supportsBundle
        && resolved != "jito"
    {
        warnings.push(format!(
            "Provider {} safe bundle execution is not wired natively yet; sequential fallback is currently expected.",
            resolved
        ));
    }
    if class == "bundle" && configured_jito_bundle_endpoints().is_empty() {
        warnings.push(
            "Bundle execution selected but no Jito bundle endpoints are configured.".to_string(),
        );
    }

    TransportPlan {
        requestedProvider: requested,
        resolvedProvider: resolved,
        executionClass: class,
        verified: meta.verified,
        supportsBundle: meta.supportsBundle,
        jitoBundleEndpoints: configured_jito_bundle_endpoints(),
        warnings,
    }
}
