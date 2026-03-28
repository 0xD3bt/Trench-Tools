#![allow(non_snake_case, dead_code)]

use serde::Serialize;

use crate::config::NormalizedExecution;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderMeta {
    pub id: &'static str,
    pub label: &'static str,
    pub verified: bool,
    pub supportsSingle: bool,
    pub supportsSequential: bool,
    pub supportsBundle: bool,
}

pub fn provider_registry() -> Vec<ProviderMeta> {
    vec![
        ProviderMeta {
            id: "auto",
            label: "Auto",
            verified: true,
            supportsSingle: true,
            supportsSequential: true,
            supportsBundle: true,
        },
        ProviderMeta {
            id: "helius",
            label: "Helius",
            verified: true,
            supportsSingle: true,
            supportsSequential: true,
            supportsBundle: false,
        },
        ProviderMeta {
            id: "jito",
            label: "Jito",
            verified: true,
            supportsSingle: true,
            supportsSequential: true,
            supportsBundle: true,
        },
        ProviderMeta {
            id: "astralane",
            label: "Astralane",
            verified: true,
            supportsSingle: true,
            supportsSequential: true,
            supportsBundle: true,
        },
        ProviderMeta {
            id: "bloxroute",
            label: "bloXroute",
            verified: false,
            supportsSingle: true,
            supportsSequential: true,
            supportsBundle: true,
        },
        ProviderMeta {
            id: "hellomoon",
            label: "Hello Moon",
            verified: false,
            supportsSingle: true,
            supportsSequential: true,
            supportsBundle: true,
        },
    ]
}

pub fn get_provider_meta(provider: &str) -> ProviderMeta {
    let normalized = if provider.trim().is_empty() {
        "auto"
    } else {
        provider.trim()
    }
    .to_lowercase();
    provider_registry()
        .into_iter()
        .find(|entry| entry.id == normalized)
        .unwrap_or_else(|| {
            provider_registry()
                .into_iter()
                .find(|entry| entry.id == "auto")
                .expect("auto provider must exist")
        })
}

pub fn get_resolved_provider(execution: &NormalizedExecution, transaction_count: usize) -> String {
    let requested = if execution.provider.trim().is_empty() {
        "auto".to_string()
    } else {
        execution.provider.trim().to_lowercase()
    };
    if requested != "auto" {
        return requested;
    }
    if transaction_count > 1 && execution.policy.trim().eq_ignore_ascii_case("safe") {
        return "jito".to_string();
    }
    "helius".to_string()
}

pub fn get_execution_class(execution: &NormalizedExecution, transaction_count: usize) -> String {
    let resolved_provider = get_resolved_provider(execution, transaction_count);
    let provider_meta = get_provider_meta(&resolved_provider);
    if transaction_count <= 1 {
        return "single".to_string();
    }
    if execution.policy.trim().eq_ignore_ascii_case("safe")
        && resolved_provider == "jito"
        && provider_meta.supportsBundle
    {
        return "bundle".to_string();
    }
    "sequential".to_string()
}
