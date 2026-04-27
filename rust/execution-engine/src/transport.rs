use serde::{Deserialize, Serialize};

use shared_execution_routing::transport::{
    ExecutionTransportInput, JitoBundleEndpoint, ProviderRegionConfig, TransportEnvironment,
};

use crate::shared_config::configured_env_value;

#[derive(Debug, Clone)]
pub struct ExecutionTransportConfig {
    pub provider: String,
    pub endpoint_profile: String,
    pub commitment: String,
    pub skip_preflight: bool,
    pub track_send_block_height: bool,
    pub mev_mode: String,
    pub mev_protect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportPlan {
    pub requested_provider: String,
    pub resolved_provider: String,
    pub requested_endpoint_profile: String,
    pub resolved_endpoint_profile: String,
    pub execution_class: String,
    pub transport_type: String,
    pub ordering: String,
    pub verified: bool,
    pub supports_bundle: bool,
    pub requires_inline_tip: bool,
    pub requires_priority_fee: bool,
    pub separate_tip_transaction: bool,
    pub skip_preflight: bool,
    pub max_retries: u32,
    pub commitment: String,
    pub track_send_block_height: bool,
    pub standard_rpc_submit_endpoints: Vec<String>,
    pub hello_moon_api_key_configured: bool,
    pub hello_moon_mev_protect: bool,
    pub hello_moon_quic_endpoint: Option<String>,
    pub hello_moon_quic_endpoints: Vec<String>,
    pub hello_moon_bundle_endpoint: Option<String>,
    pub hello_moon_bundle_endpoints: Vec<String>,
    pub helius_sender_endpoint: Option<String>,
    pub helius_sender_endpoints: Vec<String>,
    pub watch_endpoint: Option<String>,
    pub watch_endpoints: Vec<String>,
    pub jito_bundle_endpoints: Vec<JitoBundleEndpoint>,
    pub warnings: Vec<String>,
}

fn first_non_empty_env(keys: &[&str]) -> String {
    configured_env_value(keys).unwrap_or_default()
}

fn optional_env(keys: &[&str]) -> Option<String> {
    let value = first_non_empty_env(keys);
    if value.is_empty() { None } else { Some(value) }
}

fn csv_env(keys: &[&str]) -> Vec<String> {
    first_non_empty_env(keys)
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_env_bool_flag(value: &str, default: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn transport_environment() -> TransportEnvironment {
    TransportEnvironment {
        shared_region: configured_shared_region(),
        provider_regions: ProviderRegionConfig {
            helius_sender: first_non_empty_env(&[
                "EXECUTION_ENGINE_HELIUS_SENDER_REGION",
                "HELIUS_SENDER_REGION",
                "USER_REGION_HELIUS_SENDER",
                "EXECUTION_ENGINE_USER_REGION",
                "USER_REGION",
            ]),
            hellomoon: first_non_empty_env(&[
                "EXECUTION_ENGINE_HELLOMOON_REGION",
                "USER_REGION_HELLOMOON",
                "EXECUTION_ENGINE_USER_REGION",
                "USER_REGION",
            ]),
            jito_bundle: first_non_empty_env(&[
                "EXECUTION_ENGINE_JITO_BUNDLE_REGION",
                "USER_REGION_JITO_BUNDLE",
                "EXECUTION_ENGINE_USER_REGION",
                "USER_REGION",
            ]),
        },
        standard_rpc_submit_endpoints: configured_standard_rpc_submit_endpoints(),
        solana_rpc_url: optional_env(&["SOLANA_RPC_URL"]),
        solana_ws_url: optional_env(&["EXECUTION_ENGINE_SOLANA_WS_URL", "SOLANA_WS_URL"]),
        helius_rpc_url: optional_env(&["EXECUTION_ENGINE_HELIUS_RPC_URL", "HELIUS_RPC_URL"]),
        helius_ws_url: optional_env(&["EXECUTION_ENGINE_HELIUS_WS_URL", "HELIUS_WS_URL"]),
        helius_sender_endpoint: optional_env(&[
            "EXECUTION_ENGINE_HELIUS_SENDER_ENDPOINT",
            "HELIUS_SENDER_ENDPOINT",
        ]),
        helius_sender_base_url: optional_env(&[
            "EXECUTION_ENGINE_HELIUS_SENDER_BASE_URL",
            "HELIUS_SENDER_BASE_URL",
        ]),
        hellomoon_api_key: optional_env(&["HELLOMOON_API_KEY"]),
        hellomoon_mev_protect: parse_env_bool_flag(
            &first_non_empty_env(&["HELLOMOON_MEV_PROTECT", "LUNAR_LANDER_MEV_PROTECT"]),
            false,
        ),
        hellomoon_quic_endpoint: optional_env(&[
            "EXECUTION_ENGINE_HELLOMOON_QUIC_ENDPOINT",
            "HELLOMOON_QUIC_ENDPOINT",
            "LUNAR_LANDER_QUIC_ENDPOINT",
        ]),
        jito_send_bundle_endpoint: optional_env(&["JITO_SEND_BUNDLE_ENDPOINT"]),
        jito_bundle_status_endpoint: optional_env(&["JITO_BUNDLE_STATUS_ENDPOINT"]),
        jito_bundle_base_urls: csv_env(&["JITO_BUNDLE_BASE_URLS"]),
        enable_helius_transaction_subscribe: parse_env_bool_flag(
            &first_non_empty_env(&[
                "EXECUTION_ENGINE_ENABLE_HELIUS_TRANSACTION_SUBSCRIBE",
                "LAUNCHDECK_ENABLE_HELIUS_TRANSACTION_SUBSCRIBE",
            ]),
            true,
        ),
    }
}

pub fn transport_environment_snapshot() -> TransportEnvironment {
    transport_environment()
}

fn shared_plan_to_local(
    plan: shared_execution_routing::transport::TransportPlan,
    config: &ExecutionTransportConfig,
) -> TransportPlan {
    TransportPlan {
        requested_provider: plan.requestedProvider,
        resolved_provider: plan.resolvedProvider,
        requested_endpoint_profile: plan.requestedEndpointProfile,
        resolved_endpoint_profile: plan.resolvedEndpointProfile,
        execution_class: plan.executionClass,
        transport_type: plan.transportType,
        ordering: plan.ordering,
        verified: plan.verified,
        supports_bundle: plan.supportsBundle,
        requires_inline_tip: plan.requiresInlineTip,
        requires_priority_fee: plan.requiresPriorityFee,
        separate_tip_transaction: plan.separateTipTransaction,
        skip_preflight: plan.skipPreflight,
        max_retries: plan.maxRetries,
        commitment: {
            let normalized = config.commitment.trim().to_lowercase();
            if normalized.is_empty() {
                "confirmed".to_string()
            } else {
                normalized
            }
        },
        track_send_block_height: config.track_send_block_height,
        standard_rpc_submit_endpoints: plan.standardRpcSubmitEndpoints,
        hello_moon_api_key_configured: plan.helloMoonApiKeyConfigured,
        hello_moon_mev_protect: plan.helloMoonMevProtect,
        hello_moon_quic_endpoint: plan.helloMoonQuicEndpoint,
        hello_moon_quic_endpoints: plan.helloMoonQuicEndpoints,
        hello_moon_bundle_endpoint: plan.helloMoonBundleEndpoint,
        hello_moon_bundle_endpoints: plan.helloMoonBundleEndpoints,
        helius_sender_endpoint: plan.heliusSenderEndpoint,
        helius_sender_endpoints: plan.heliusSenderEndpoints,
        watch_endpoint: plan.watchEndpoint,
        watch_endpoints: plan.watchEndpoints,
        jito_bundle_endpoints: plan.jitoBundleEndpoints,
        warnings: plan.warnings,
    }
}

fn to_transport_input(config: &ExecutionTransportConfig) -> ExecutionTransportInput {
    ExecutionTransportInput {
        provider: config.provider.clone(),
        endpoint_profile: config.endpoint_profile.clone(),
        mev_protect: config.mev_protect,
        mev_mode: config.mev_mode.clone(),
        skip_preflight: config.skip_preflight,
    }
}

pub fn configured_shared_region() -> String {
    first_non_empty_env(&["EXECUTION_ENGINE_USER_REGION", "USER_REGION"])
}

pub fn configured_provider_region(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "helius-sender" => first_non_empty_env(&[
            "EXECUTION_ENGINE_HELIUS_SENDER_REGION",
            "HELIUS_SENDER_REGION",
            "USER_REGION_HELIUS_SENDER",
            "EXECUTION_ENGINE_USER_REGION",
            "USER_REGION",
        ]),
        "hellomoon" => first_non_empty_env(&[
            "EXECUTION_ENGINE_HELLOMOON_REGION",
            "USER_REGION_HELLOMOON",
            "EXECUTION_ENGINE_USER_REGION",
            "USER_REGION",
        ]),
        "jito-bundle" => first_non_empty_env(&[
            "EXECUTION_ENGINE_JITO_BUNDLE_REGION",
            "USER_REGION_JITO_BUNDLE",
            "EXECUTION_ENGINE_USER_REGION",
            "USER_REGION",
        ]),
        _ => String::new(),
    }
}

pub fn default_endpoint_profile() -> String {
    shared_execution_routing::transport::default_endpoint_profile_from_user_region(
        &configured_shared_region(),
    )
}

pub fn default_endpoint_profile_for_provider(provider: &str) -> String {
    shared_execution_routing::transport::default_endpoint_profile_for_provider(
        provider,
        &transport_environment(),
    )
}

pub fn configured_standard_rpc_submit_endpoints() -> Vec<String> {
    csv_env(&[
        "EXECUTION_ENGINE_STANDARD_RPC_SEND_URLS",
        "STANDARD_RPC_SEND_URLS",
        "LAUNCHDECK_EXTRA_STANDARD_RPC_SEND_URLS",
        "LAUNCHDECK_STANDARD_RPC_SEND_URLS",
    ])
}

pub fn configured_watch_endpoints() -> Vec<String> {
    shared_execution_routing::transport::configured_watch_endpoints_for_provider(
        &transport_environment(),
        "standard-rpc",
        "",
    )
}

pub fn configured_watch_endpoints_for_provider(
    provider: &str,
    endpoint_profile: &str,
) -> Vec<String> {
    shared_execution_routing::transport::configured_watch_endpoints_for_provider(
        &transport_environment(),
        provider,
        endpoint_profile,
    )
}

pub fn configured_helius_sender_endpoints_for_profile(endpoint_profile: &str) -> Vec<String> {
    shared_execution_routing::transport::configured_helius_sender_endpoints_for_profile(
        &transport_environment(),
        endpoint_profile,
    )
}

pub fn configured_hellomoon_quic_endpoints_for_profile(endpoint_profile: &str) -> Vec<String> {
    shared_execution_routing::transport::configured_hellomoon_quic_endpoints_for_profile(
        &transport_environment(),
        endpoint_profile,
    )
}

pub fn configured_hellomoon_bundle_endpoints_for_profile(endpoint_profile: &str) -> Vec<String> {
    shared_execution_routing::transport::configured_hellomoon_bundle_endpoints_for_profile(
        &transport_environment(),
        endpoint_profile,
    )
}

pub fn configured_jito_bundle_endpoints_for_profile(
    endpoint_profile: &str,
) -> Vec<JitoBundleEndpoint> {
    shared_execution_routing::transport::configured_jito_bundle_endpoints_for_profile(
        &transport_environment(),
        endpoint_profile,
    )
}

pub fn configured_hellomoon_mev_protect() -> bool {
    transport_environment().hellomoon_mev_protect
}

pub fn configured_enable_helius_transaction_subscribe() -> bool {
    transport_environment().enable_helius_transaction_subscribe
}

pub fn resolved_helius_transaction_subscribe_ws_url(
    base_watch_endpoint: Option<&str>,
) -> Option<String> {
    shared_execution_routing::transport::resolved_helius_transaction_subscribe_ws_url(
        &transport_environment(),
        base_watch_endpoint,
    )
}

pub fn prefers_helius_transaction_subscribe_path(
    helius_subscribe_enabled: bool,
    base_watch_endpoint: Option<&str>,
) -> bool {
    shared_execution_routing::transport::prefers_helius_transaction_subscribe_path(
        helius_subscribe_enabled,
        &transport_environment(),
        base_watch_endpoint,
    )
}

pub fn resolved_helius_priority_fee_rpc_url() -> String {
    shared_execution_routing::transport::resolved_helius_priority_fee_rpc_url(
        &transport_environment(),
    )
}

pub fn build_transport_plan(
    config: &ExecutionTransportConfig,
    transaction_count: usize,
) -> TransportPlan {
    shared_plan_to_local(
        shared_execution_routing::transport::build_transport_plan(
            &to_transport_input(config),
            transaction_count,
            &transport_environment(),
        ),
        config,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config(provider: &str, mev_mode: &str) -> ExecutionTransportConfig {
        ExecutionTransportConfig {
            provider: provider.to_string(),
            endpoint_profile: "fra".to_string(),
            commitment: "confirmed".to_string(),
            skip_preflight: false,
            track_send_block_height: true,
            mev_mode: mev_mode.to_string(),
            mev_protect: !mev_mode.eq_ignore_ascii_case("off"),
        }
    }

    #[test]
    fn hellomoon_secure_selects_bundle_transport() {
        let plan = build_transport_plan(&sample_config("hellomoon", "secure"), 1);
        assert_eq!(plan.transport_type, "hellomoon-bundle");
        assert!(plan.supports_bundle);
        assert!(plan.requires_inline_tip);
    }

    #[test]
    fn hellomoon_reduced_selects_quic_transport() {
        let plan = build_transport_plan(&sample_config("hellomoon", "reduced"), 1);
        assert_eq!(plan.transport_type, "hellomoon-quic");
        assert!(plan.requires_priority_fee);
        assert!(plan.hello_moon_mev_protect);
        assert_eq!(plan.requested_provider, "hellomoon");
    }

    #[test]
    fn jito_bundle_transport_preserves_bundle_endpoints() {
        let plan = build_transport_plan(&sample_config("jito-bundle", "off"), 2);
        assert_eq!(plan.transport_type, "jito-bundle");
        assert!(plan.supports_bundle);
        assert!(!plan.jito_bundle_endpoints.is_empty());
    }
}
