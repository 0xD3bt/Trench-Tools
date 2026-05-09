use std::{collections::BTreeMap, env};

#[allow(unused_imports)]
pub use shared_execution_routing::providers::{
    ProviderAvailability, ProviderMeta, get_provider_meta, provider_registry,
};

#[allow(dead_code)]
pub fn provider_availability_registry() -> BTreeMap<String, ProviderAvailability> {
    let solana_rpc_configured = env::var("SOLANA_RPC_URL")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let hellomoon_api_key_configured = env::var("HELLOMOON_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    shared_execution_routing::providers::provider_availability_registry(
        solana_rpc_configured,
        hellomoon_api_key_configured,
    )
}
