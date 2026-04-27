use std::future::Future;

pub use shared_execution_routing::transport::{
    JitoBundleEndpoint, TransportEnvironment, TransportPlan,
};

tokio::task_local! {
    static TRANSPORT_ENVIRONMENT: TransportEnvironment;
}

pub async fn with_transport_environment<F, T>(environment: TransportEnvironment, future: F) -> T
where
    F: Future<Output = T>,
{
    TRANSPORT_ENVIRONMENT.scope(environment, future).await
}

pub fn configured_enable_helius_transaction_subscribe() -> bool {
    TRANSPORT_ENVIRONMENT
        .try_with(|environment| environment.enable_helius_transaction_subscribe)
        .unwrap_or(true)
}

pub fn configured_hellomoon_api_key() -> String {
    TRANSPORT_ENVIRONMENT
        .try_with(|environment| {
            environment
                .hellomoon_api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

pub fn configured_watch_endpoints_for_provider(
    provider: &str,
    endpoint_profile: &str,
) -> Vec<String> {
    TRANSPORT_ENVIRONMENT
        .try_with(|environment| {
            shared_execution_routing::transport::configured_watch_endpoints_for_provider(
                environment,
                provider,
                endpoint_profile,
            )
        })
        .unwrap_or_default()
}

pub fn prefers_helius_transaction_subscribe_path(
    helius_subscribe_enabled: bool,
    base_watch_endpoint: Option<&str>,
) -> bool {
    TRANSPORT_ENVIRONMENT
        .try_with(|environment| {
            shared_execution_routing::transport::prefers_helius_transaction_subscribe_path(
                helius_subscribe_enabled,
                environment,
                base_watch_endpoint,
            )
        })
        .unwrap_or(false)
}

pub fn resolved_helius_transaction_subscribe_ws_url(
    base_watch_endpoint: Option<&str>,
) -> Option<String> {
    TRANSPORT_ENVIRONMENT
        .try_with(|environment| {
            shared_execution_routing::transport::resolved_helius_transaction_subscribe_ws_url(
                environment,
                base_watch_endpoint,
            )
        })
        .ok()
        .flatten()
}
