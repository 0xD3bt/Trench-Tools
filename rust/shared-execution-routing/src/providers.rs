#![allow(non_snake_case)]

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderMeta {
    pub id: &'static str,
    pub label: &'static str,
    pub verified: bool,
    pub supportsSingle: bool,
    pub supportsSequential: bool,
    pub supportsBundle: bool,
    pub supportsEndpointProfiles: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAvailability {
    pub provider: String,
    pub available: bool,
    pub verified: bool,
    pub supportState: String,
    pub supportsSingle: bool,
    pub supportsSequential: bool,
    pub supportsBundle: bool,
    pub supportsEndpointProfiles: bool,
    pub reason: String,
}

pub fn provider_registry() -> Vec<ProviderMeta> {
    vec![
        ProviderMeta {
            id: "helius-sender",
            label: "Helius Sender",
            verified: true,
            supportsSingle: true,
            supportsSequential: true,
            supportsBundle: false,
            supportsEndpointProfiles: true,
        },
        ProviderMeta {
            id: "hellomoon",
            label: "Hello Moon QUIC",
            verified: true,
            supportsSingle: true,
            supportsSequential: true,
            supportsBundle: true,
            supportsEndpointProfiles: true,
        },
        ProviderMeta {
            id: "jito-bundle",
            label: "Jito Bundle",
            verified: true,
            supportsSingle: true,
            supportsSequential: false,
            supportsBundle: true,
            supportsEndpointProfiles: true,
        },
        ProviderMeta {
            id: "standard-rpc",
            label: "Standard RPC",
            verified: true,
            supportsSingle: true,
            supportsSequential: true,
            supportsBundle: false,
            supportsEndpointProfiles: false,
        },
    ]
}

pub fn provider_availability_registry(
    solana_rpc_configured: bool,
    hellomoon_api_key_configured: bool,
) -> BTreeMap<String, ProviderAvailability> {
    provider_registry()
        .into_iter()
        .map(|provider| {
            let (available, reason) = match provider.id {
                "helius-sender" if !solana_rpc_configured => (
                    true,
                    "Using the default Helius Sender endpoint with the localhost RPC fallback; set SOLANA_RPC_URL for a dedicated confirmation RPC.".to_string(),
                ),
                "hellomoon" if !hellomoon_api_key_configured => (
                    false,
                    "Set HELLOMOON_API_KEY to enable Hello Moon QUIC submission.".to_string(),
                ),
                "hellomoon" if !solana_rpc_configured => (
                    true,
                    "Hello Moon QUIC is configured, but SOLANA_RPC_URL is still recommended for confirmations; Shyft is a good pairing here.".to_string(),
                ),
                _ => (true, String::new()),
            };
            (
                provider.id.to_string(),
                ProviderAvailability {
                    provider: provider.id.to_string(),
                    available,
                    verified: provider.verified,
                    supportState: if available {
                        if provider.verified {
                            "verified".to_string()
                        } else {
                            "unverified".to_string()
                        }
                    } else {
                        "unavailable".to_string()
                    },
                    supportsSingle: provider.supportsSingle,
                    supportsSequential: provider.supportsSequential,
                    supportsBundle: provider.supportsBundle,
                    supportsEndpointProfiles: provider.supportsEndpointProfiles,
                    reason,
                },
            )
        })
        .collect()
}

pub fn get_provider_meta(provider: &str) -> ProviderMeta {
    let normalized = if provider.trim().is_empty() {
        "helius-sender"
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
                .find(|entry| entry.id == "helius-sender")
                .expect("helius-sender provider must exist")
        })
}
