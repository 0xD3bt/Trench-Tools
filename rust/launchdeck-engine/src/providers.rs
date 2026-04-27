use std::{collections::BTreeMap, env};

use shared_execution_routing::transport::ExecutionTransportInput;

use crate::config::NormalizedExecution;

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

#[allow(dead_code)]
pub fn get_resolved_provider(execution: &NormalizedExecution, _transaction_count: usize) -> String {
    shared_execution_routing::transport::resolved_provider(&ExecutionTransportInput {
        provider: execution.provider.clone(),
        endpoint_profile: execution.endpointProfile.clone(),
        mev_protect: execution.mevProtect,
        mev_mode: execution.mevMode.clone(),
        skip_preflight: execution.skipPreflight,
    })
}

#[allow(dead_code)]
pub fn get_execution_class(execution: &NormalizedExecution, transaction_count: usize) -> String {
    shared_execution_routing::transport::execution_class(
        &ExecutionTransportInput {
            provider: execution.provider.clone(),
            endpoint_profile: execution.endpointProfile.clone(),
            mev_protect: execution.mevProtect,
            mev_mode: execution.mevMode.clone(),
            skip_preflight: execution.skipPreflight,
        },
        transaction_count,
    )
}
