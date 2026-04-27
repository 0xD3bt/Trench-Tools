#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

use crate::{
    endpoint_profile::{
        metro_token_canonical, normalize_user_region, parse_config_endpoint_profile,
    },
    providers::get_provider_meta,
};

const DEFAULT_HELIUS_SENDER_ENDPOINT: &str = "https://sender.helius-rpc.com/fast";
const DEFAULT_HELIUS_SENDER_REGIONAL_ENDPOINTS: [(&str, &str); 7] = [
    ("slc", "http://slc-sender.helius-rpc.com/fast"),
    ("ewr", "http://ewr-sender.helius-rpc.com/fast"),
    ("lon", "http://lon-sender.helius-rpc.com/fast"),
    ("fra", "http://fra-sender.helius-rpc.com/fast"),
    ("ams", "http://ams-sender.helius-rpc.com/fast"),
    ("sg", "http://sg-sender.helius-rpc.com/fast"),
    ("tyo", "http://tyo-sender.helius-rpc.com/fast"),
];
const DEFAULT_HELLOMOON_GLOBAL_QUIC_ENDPOINT: &str = "lunar-lander.hellomoon.io:16888";
const DEFAULT_HELLOMOON_REGIONAL_QUIC_ENDPOINTS: [(&str, &str); 5] = [
    ("fra", "fra.lunar-lander.hellomoon.io:16888"),
    ("ams", "ams.lunar-lander.hellomoon.io:16888"),
    ("nyc", "nyc.lunar-lander.hellomoon.io:16888"),
    ("ash", "ash.lunar-lander.hellomoon.io:16888"),
    ("tyo", "tyo.lunar-lander.hellomoon.io:16888"),
];
const DEFAULT_HELLOMOON_GLOBAL_BUNDLE_ENDPOINT: &str =
    "http://lunar-lander.hellomoon.io/sendBundle";
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
const DEFAULT_ENDPOINT_PROFILE: &str = "global";

#[derive(Debug, Clone, Default)]
pub struct ProviderRegionConfig {
    pub helius_sender: String,
    pub hellomoon: String,
    pub jito_bundle: String,
}

#[derive(Debug, Clone, Default)]
pub struct TransportEnvironment {
    pub shared_region: String,
    pub provider_regions: ProviderRegionConfig,
    pub standard_rpc_submit_endpoints: Vec<String>,
    pub solana_rpc_url: Option<String>,
    pub solana_ws_url: Option<String>,
    pub helius_rpc_url: Option<String>,
    pub helius_ws_url: Option<String>,
    pub helius_sender_endpoint: Option<String>,
    pub helius_sender_base_url: Option<String>,
    pub hellomoon_api_key: Option<String>,
    pub hellomoon_mev_protect: bool,
    pub hellomoon_quic_endpoint: Option<String>,
    pub jito_send_bundle_endpoint: Option<String>,
    pub jito_bundle_status_endpoint: Option<String>,
    pub jito_bundle_base_urls: Vec<String>,
    pub enable_helius_transaction_subscribe: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionTransportInput {
    pub provider: String,
    pub endpoint_profile: String,
    pub mev_protect: bool,
    pub mev_mode: String,
    pub skip_preflight: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitoBundleEndpoint {
    pub name: String,
    pub send: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportPlan {
    pub requestedProvider: String,
    pub resolvedProvider: String,
    pub requestedEndpointProfile: String,
    pub resolvedEndpointProfile: String,
    pub executionClass: String,
    pub transportType: String,
    pub ordering: String,
    pub verified: bool,
    pub supportsBundle: bool,
    pub requiresInlineTip: bool,
    pub requiresPriorityFee: bool,
    pub separateTipTransaction: bool,
    pub skipPreflight: bool,
    pub maxRetries: u32,
    pub standardRpcSubmitEndpoints: Vec<String>,
    pub helloMoonApiKeyConfigured: bool,
    pub helloMoonMevProtect: bool,
    pub helloMoonQuicEndpoint: Option<String>,
    pub helloMoonQuicEndpoints: Vec<String>,
    #[serde(default)]
    pub helloMoonBundleEndpoint: Option<String>,
    #[serde(default)]
    pub helloMoonBundleEndpoints: Vec<String>,
    pub heliusSenderEndpoint: Option<String>,
    pub heliusSenderEndpoints: Vec<String>,
    pub watchEndpoint: Option<String>,
    pub watchEndpoints: Vec<String>,
    pub jitoBundleEndpoints: Vec<JitoBundleEndpoint>,
    pub warnings: Vec<String>,
}

fn normalize_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "" => "helius-sender".to_string(),
        "helius" => "helius-sender".to_string(),
        "rpc" => "standard-rpc".to_string(),
        value => value.to_string(),
    }
}

pub fn default_endpoint_profile_from_user_region(user_region: &str) -> String {
    normalize_user_region(user_region).unwrap_or_else(|| DEFAULT_ENDPOINT_PROFILE.to_string())
}

pub fn resolve_default_endpoint_profile_for_provider(
    provider: &str,
    provider_region: &str,
    shared_region: &str,
) -> String {
    if normalize_provider(provider) == "standard-rpc" {
        return String::new();
    }
    normalize_user_region(provider_region)
        .or_else(|| normalize_user_region(shared_region))
        .unwrap_or_else(|| DEFAULT_ENDPOINT_PROFILE.to_string())
}

pub fn default_endpoint_profile_for_provider(
    provider: &str,
    environment: &TransportEnvironment,
) -> String {
    let provider_region = match normalize_provider(provider).as_str() {
        "helius-sender" => environment.provider_regions.helius_sender.as_str(),
        "hellomoon" => environment.provider_regions.hellomoon.as_str(),
        "jito-bundle" => environment.provider_regions.jito_bundle.as_str(),
        _ => "",
    };
    resolve_default_endpoint_profile_for_provider(
        provider,
        provider_region,
        &environment.shared_region,
    )
}

fn normalize_endpoint_profile(
    provider: &str,
    endpoint_profile: &str,
    environment: &TransportEnvironment,
) -> String {
    let normalized_provider = normalize_provider(provider);
    if normalized_provider == "standard-rpc" {
        return String::new();
    }
    let trimmed = endpoint_profile.trim();
    if trimmed.is_empty() {
        return default_endpoint_profile_for_provider(provider, environment);
    }
    parse_config_endpoint_profile(trimmed)
        .unwrap_or_else(|_| default_endpoint_profile_for_provider(provider, environment))
}

fn configured_helius_sender_override(environment: &TransportEnvironment) -> Option<String> {
    if let Some(explicit) = environment
        .helius_sender_endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(explicit.to_string());
    }
    environment
        .helius_sender_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|base| format!("{}/fast", base.trim_end_matches('/')))
}

pub fn helius_sender_endpoint_override_active(environment: &TransportEnvironment) -> bool {
    configured_helius_sender_override(environment).is_some()
}

pub fn hellomoon_api_key_configured(environment: &TransportEnvironment) -> bool {
    environment
        .hellomoon_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
}

fn configured_hellomoon_quic_override(environment: &TransportEnvironment) -> Option<String> {
    environment
        .hellomoon_quic_endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn configured_helius_sender_endpoints_for_profile(
    environment: &TransportEnvironment,
    endpoint_profile: &str,
) -> Vec<String> {
    if let Some(override_endpoint) = configured_helius_sender_override(environment) {
        return vec![override_endpoint];
    }
    let resolved_endpoint_profile =
        normalize_endpoint_profile("helius-sender", endpoint_profile, environment);
    let global_endpoint = DEFAULT_HELIUS_SENDER_ENDPOINT.to_string();
    let regional = |codes: &[&str]| {
        DEFAULT_HELIUS_SENDER_REGIONAL_ENDPOINTS
            .iter()
            .filter(|(code, _)| codes.iter().any(|candidate| *candidate == *code))
            .map(|(_, endpoint)| endpoint.to_string())
            .collect::<Vec<_>>()
    };
    if resolved_endpoint_profile.contains(',') {
        let codes: Vec<&str> = resolved_endpoint_profile
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        let endpoints = regional(&codes);
        return if endpoints.is_empty() {
            vec![global_endpoint]
        } else {
            endpoints
        };
    }
    match resolved_endpoint_profile.as_str() {
        "us" => regional(&["slc", "ewr"]),
        "eu" => regional(&["fra", "ams"]),
        "asia" => regional(&["sg", "tyo"]),
        code if metro_token_canonical(code).is_some() => regional(&[code]),
        _ => vec![global_endpoint],
    }
}

fn hellomoon_profile_tokens(
    endpoint_profile: &str,
    environment: &TransportEnvironment,
) -> Vec<String> {
    let resolved = normalize_endpoint_profile("hellomoon", endpoint_profile, environment);
    let map_token = |token: &str| match token {
        "global" => vec!["global".to_string()],
        "us" => vec!["nyc".to_string(), "ash".to_string()],
        "eu" => vec!["fra".to_string(), "ams".to_string()],
        "asia" => vec!["tyo".to_string()],
        "ewr" | "slc" => vec!["nyc".to_string(), "ash".to_string()],
        "fra" => vec!["fra".to_string()],
        "ams" => vec!["ams".to_string()],
        "lon" => vec!["fra".to_string(), "ams".to_string()],
        "sg" | "tyo" => vec!["tyo".to_string()],
        _ => vec!["global".to_string()],
    };
    if resolved.contains(',') {
        let mut out = Vec::new();
        for token in resolved.split(',').map(str::trim) {
            for mapped in map_token(token) {
                if !out.iter().any(|existing| existing == &mapped) {
                    out.push(mapped);
                }
            }
        }
        return out;
    }
    map_token(&resolved)
}

pub fn configured_hellomoon_quic_endpoints_for_profile(
    environment: &TransportEnvironment,
    endpoint_profile: &str,
) -> Vec<String> {
    if let Some(override_endpoint) = configured_hellomoon_quic_override(environment) {
        return vec![override_endpoint];
    }
    let profile_tokens = hellomoon_profile_tokens(endpoint_profile, environment);
    if profile_tokens.iter().any(|token| token == "global") {
        return vec![DEFAULT_HELLOMOON_GLOBAL_QUIC_ENDPOINT.to_string()];
    }
    let mut endpoints = Vec::new();
    for token in profile_tokens {
        if let Some((_, endpoint)) = DEFAULT_HELLOMOON_REGIONAL_QUIC_ENDPOINTS
            .iter()
            .find(|(name, _)| *name == token)
        {
            if !endpoints.iter().any(|existing| existing == endpoint) {
                endpoints.push((*endpoint).to_string());
            }
        }
    }
    if endpoints.is_empty() {
        vec![DEFAULT_HELLOMOON_GLOBAL_QUIC_ENDPOINT.to_string()]
    } else {
        endpoints
    }
}

pub fn configured_hellomoon_bundle_endpoints_for_profile(
    environment: &TransportEnvironment,
    endpoint_profile: &str,
) -> Vec<String> {
    let profile_tokens = hellomoon_profile_tokens(endpoint_profile, environment);
    if profile_tokens.iter().any(|token| token == "global") {
        return vec![DEFAULT_HELLOMOON_GLOBAL_BUNDLE_ENDPOINT.to_string()];
    }
    let mut endpoints = Vec::new();
    for token in profile_tokens {
        let endpoint = match token.as_str() {
            "fra" => Some("http://fra.lunar-lander.hellomoon.io/sendBundle"),
            "ams" => Some("http://ams.lunar-lander.hellomoon.io/sendBundle"),
            "nyc" => Some("http://nyc.lunar-lander.hellomoon.io/sendBundle"),
            "ash" => Some("http://ash.lunar-lander.hellomoon.io/sendBundle"),
            "tyo" => Some("http://tyo.lunar-lander.hellomoon.io/sendBundle"),
            _ => None,
        };
        if let Some(endpoint) = endpoint {
            if !endpoints.iter().any(|existing| existing == endpoint) {
                endpoints.push(endpoint.to_string());
            }
        }
    }
    if endpoints.is_empty() {
        vec![DEFAULT_HELLOMOON_GLOBAL_BUNDLE_ENDPOINT.to_string()]
    } else {
        endpoints
    }
}

pub fn resolved_helius_priority_fee_rpc_url(environment: &TransportEnvironment) -> String {
    environment
        .helius_rpc_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            environment
                .solana_rpc_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn derived_solana_ws_url_from_rpc_url(environment: &TransportEnvironment) -> Option<String> {
    let rpc_url = environment.solana_rpc_url.as_deref()?.trim();
    if rpc_url.is_empty() {
        return None;
    }
    if let Some(rest) = rpc_url.strip_prefix("https://") {
        return Some(format!("wss://{rest}"));
    }
    if let Some(rest) = rpc_url.strip_prefix("http://") {
        if let Some(host) = rest.strip_suffix(":8899") {
            return Some(format!("ws://{host}:8900"));
        }
        return Some(format!("ws://{rest}"));
    }
    if rpc_url.starts_with("wss://") || rpc_url.starts_with("ws://") {
        return Some(rpc_url.to_string());
    }
    None
}

pub fn resolved_helius_transaction_subscribe_ws_url(
    environment: &TransportEnvironment,
    base_watch_endpoint: Option<&str>,
) -> Option<String> {
    if let Some(url) = environment
        .helius_ws_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(url.to_string());
    }
    base_watch_endpoint
        .filter(|endpoint| endpoint.trim().to_ascii_lowercase().contains("helius"))
        .map(|endpoint| endpoint.trim().to_string())
}

pub fn prefers_helius_transaction_subscribe_path(
    helius_subscribe_enabled: bool,
    environment: &TransportEnvironment,
    base_watch_endpoint: Option<&str>,
) -> bool {
    helius_subscribe_enabled
        && resolved_helius_transaction_subscribe_ws_url(environment, base_watch_endpoint).is_some()
}

pub fn configured_watch_endpoints_for_provider(
    environment: &TransportEnvironment,
    provider: &str,
    endpoint_profile: &str,
) -> Vec<String> {
    let _ = normalize_provider(provider);
    let _ = normalize_endpoint_profile(provider, endpoint_profile, environment);
    if let Some(explicit_ws) = environment
        .solana_ws_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return vec![explicit_ws.to_string()];
    }
    if let Some(url) = environment
        .helius_ws_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return vec![url.to_string()];
    }
    derived_solana_ws_url_from_rpc_url(environment)
        .into_iter()
        .collect()
}

fn jito_name_matches_metro_token(name: &str, token: &str) -> bool {
    match token.trim().to_lowercase().as_str() {
        "slc" => name.contains("slc."),
        "ewr" | "ny" => name.contains("ny."),
        "fra" => name.contains("frankfurt."),
        "ams" => name.contains("amsterdam."),
        "lon" => name.contains("london."),
        "sg" => name.contains("singapore."),
        "tyo" => name.contains("tokyo."),
        _ => false,
    }
}

fn jito_endpoint_matches_profile(endpoint: &JitoBundleEndpoint, endpoint_profile: &str) -> bool {
    let name = endpoint.name.to_lowercase();
    if endpoint_profile.contains(',') {
        return endpoint_profile
            .split(',')
            .any(|token| jito_name_matches_metro_token(&name, token.trim()));
    }
    match endpoint_profile {
        "us" => name.contains("ny.") || name.contains("slc."),
        "eu" => {
            name.contains("frankfurt.")
                || name.contains("amsterdam.")
                || name.contains("london.")
                || name.contains("dublin.")
        }
        "asia" => name.contains("singapore.") || name.contains("tokyo."),
        "global" => true,
        other => jito_name_matches_metro_token(&name, other),
    }
}

pub fn configured_jito_bundle_endpoints_for_profile(
    environment: &TransportEnvironment,
    endpoint_profile: &str,
) -> Vec<JitoBundleEndpoint> {
    let resolved_endpoint_profile =
        normalize_endpoint_profile("jito-bundle", endpoint_profile, environment);
    let explicit_send = environment
        .jito_send_bundle_endpoint
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let explicit_status = environment
        .jito_bundle_status_endpoint
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
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
    let bases: Vec<String> = if environment.jito_bundle_base_urls.is_empty() {
        DEFAULT_JITO_BUNDLE_BASE_URLS
            .iter()
            .map(|entry| entry.to_string())
            .collect()
    } else {
        environment.jito_bundle_base_urls.clone()
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
        .filter(|endpoint| jito_endpoint_matches_profile(endpoint, &resolved_endpoint_profile))
        .collect()
}

pub fn jito_bundle_endpoint_override_active(environment: &TransportEnvironment) -> bool {
    environment
        .jito_send_bundle_endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
        || environment
            .jito_bundle_status_endpoint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
}

pub fn resolved_provider(input: &ExecutionTransportInput) -> String {
    normalize_provider(&input.provider)
}

pub fn execution_class(input: &ExecutionTransportInput, transaction_count: usize) -> String {
    let provider = resolved_provider(input);
    if provider == "jito-bundle"
        || (provider == "hellomoon" && input.mev_mode.trim().eq_ignore_ascii_case("secure"))
    {
        return "bundle".to_string();
    }
    if transaction_count <= 1 {
        return "single".to_string();
    }
    "sequential".to_string()
}

pub fn transport_type(input: &ExecutionTransportInput, _transaction_count: usize) -> String {
    let provider = resolved_provider(input);
    match provider.as_str() {
        "standard-rpc" => "standard-rpc-fanout".to_string(),
        "helius-sender" => "helius-sender".to_string(),
        "hellomoon" => {
            if input.mev_mode.trim().eq_ignore_ascii_case("secure") {
                "hellomoon-bundle".to_string()
            } else {
                "hellomoon-quic".to_string()
            }
        }
        "jito-bundle" => "jito-bundle".to_string(),
        _ => "standard-rpc-fanout".to_string(),
    }
}

pub fn transport_ordering(input: &ExecutionTransportInput, transaction_count: usize) -> String {
    match execution_class(input, transaction_count).as_str() {
        "bundle" => "bundle".to_string(),
        "single" => "single".to_string(),
        _ => "sequential".to_string(),
    }
}

pub fn build_transport_plan(
    input: &ExecutionTransportInput,
    transaction_count: usize,
    environment: &TransportEnvironment,
) -> TransportPlan {
    let requested = normalize_provider(&input.provider);
    let resolved = resolved_provider(input);
    let requested_endpoint_profile =
        normalize_endpoint_profile(&input.provider, &input.endpoint_profile, environment);
    let resolved_endpoint_profile =
        normalize_endpoint_profile(&resolved, &input.endpoint_profile, environment);
    let class = execution_class(input, transaction_count);
    let transport = transport_type(input, transaction_count);
    let ordering = transport_ordering(input, transaction_count);
    let meta = get_provider_meta(&resolved);
    let helius_sender_endpoints = if transport == "helius-sender" {
        configured_helius_sender_endpoints_for_profile(environment, &resolved_endpoint_profile)
    } else {
        vec![]
    };
    let hello_moon_quic_endpoints = if transport == "hellomoon-quic" {
        configured_hellomoon_quic_endpoints_for_profile(environment, &resolved_endpoint_profile)
    } else {
        vec![]
    };
    let hello_moon_bundle_endpoints = if transport == "hellomoon-bundle" {
        configured_hellomoon_bundle_endpoints_for_profile(environment, &resolved_endpoint_profile)
    } else {
        vec![]
    };
    let standard_rpc_submit_endpoints = if resolved == "standard-rpc" {
        environment.standard_rpc_submit_endpoints.clone()
    } else {
        vec![]
    };
    let jito_bundle_endpoints = if transport == "jito-bundle" {
        configured_jito_bundle_endpoints_for_profile(environment, &resolved_endpoint_profile)
    } else {
        vec![]
    };
    let watch_endpoints =
        configured_watch_endpoints_for_provider(environment, &resolved, &resolved_endpoint_profile);
    let mut warnings = Vec::new();
    if !meta.verified {
        warnings.push(format!(
            "Provider {} is currently marked unverified in this environment.",
            resolved
        ));
    }
    if transport == "jito-bundle" && jito_bundle_endpoints.is_empty() {
        warnings.push(
            "Bundle execution selected but no Jito bundle endpoints are configured.".to_string(),
        );
    }
    if resolved == "jito-bundle" && transaction_count > 5 {
        warnings.push("Jito bundle transport supports at most 5 transactions.".to_string());
    }
    if transport == "hellomoon-bundle" && transaction_count > 4 {
        warnings.push("Hello Moon bundle transport supports at most 4 transactions.".to_string());
    }
    if resolved == "helius-sender" && !input.skip_preflight {
        warnings.push(
            "Helius Sender requires skipPreflight=true and will hard-fail if it is disabled."
                .to_string(),
        );
    }
    if resolved == "hellomoon" && !input.skip_preflight {
        warnings.push(
            "Hello Moon QUIC runs as a low-latency fire-and-forget path and expects skipPreflight=true."
                .to_string(),
        );
    }
    if resolved == "helius-sender" {
        if let Some(override_endpoint) = configured_helius_sender_override(environment) {
            warnings.push(format!(
                "HELIUS_SENDER endpoint override is active ({override_endpoint}); endpoint profile fanout is bypassed."
            ));
        }
    }
    if resolved == "hellomoon" {
        if !hellomoon_api_key_configured(environment) {
            warnings.push("Hello Moon QUIC requires HELLOMOON_API_KEY.".to_string());
        }
        if let Some(override_endpoint) = configured_hellomoon_quic_override(environment) {
            warnings.push(format!(
                "HELLOMOON_QUIC_ENDPOINT override is active ({override_endpoint}); endpoint profile fanout is bypassed."
            ));
        }
    }

    TransportPlan {
        requestedProvider: requested,
        resolvedProvider: resolved,
        requestedEndpointProfile: requested_endpoint_profile,
        resolvedEndpointProfile: resolved_endpoint_profile,
        executionClass: class,
        transportType: transport.clone(),
        ordering,
        verified: meta.verified,
        supportsBundle: meta.supportsBundle,
        requiresInlineTip: matches!(
            transport.as_str(),
            "helius-sender" | "hellomoon-quic" | "hellomoon-bundle"
        ),
        requiresPriorityFee: matches!(
            transport.as_str(),
            "helius-sender" | "hellomoon-quic" | "hellomoon-bundle"
        ),
        separateTipTransaction: transport == "jito-bundle",
        skipPreflight: matches!(
            transport.as_str(),
            "helius-sender"
                | "hellomoon-quic"
                | "hellomoon-bundle"
                | "standard-rpc-fanout"
                | "jito-bundle"
        ) || input.skip_preflight,
        maxRetries: if matches!(
            transport.as_str(),
            "helius-sender" | "hellomoon-quic" | "hellomoon-bundle" | "standard-rpc-fanout"
        ) {
            0
        } else {
            3
        },
        standardRpcSubmitEndpoints: standard_rpc_submit_endpoints,
        helloMoonApiKeyConfigured: hellomoon_api_key_configured(environment),
        helloMoonMevProtect: transport == "hellomoon-quic" && input.mev_protect,
        helloMoonQuicEndpoint: if transport == "hellomoon-quic" {
            hello_moon_quic_endpoints.first().cloned()
        } else {
            None
        },
        helloMoonQuicEndpoints: hello_moon_quic_endpoints,
        helloMoonBundleEndpoint: if transport == "hellomoon-bundle" {
            hello_moon_bundle_endpoints.first().cloned()
        } else {
            None
        },
        helloMoonBundleEndpoints: hello_moon_bundle_endpoints,
        heliusSenderEndpoint: if transport == "helius-sender" {
            helius_sender_endpoints.first().cloned()
        } else {
            None
        },
        heliusSenderEndpoints: helius_sender_endpoints,
        watchEndpoint: watch_endpoints.first().cloned(),
        watchEndpoints: watch_endpoints,
        jitoBundleEndpoints: jito_bundle_endpoints,
        warnings,
    }
}
