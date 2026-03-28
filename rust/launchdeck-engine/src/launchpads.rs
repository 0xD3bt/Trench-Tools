#![allow(non_snake_case, dead_code)]

use serde::Serialize;
use std::{collections::BTreeMap, env};

#[derive(Debug, Clone, Serialize)]
pub struct TokenMetadataLimits {
    pub nameMaxLength: usize,
    pub symbolMaxLength: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategySupport {
    #[serde(rename = "snipe-own-launch")]
    pub snipe_own_launch: bool,
    #[serde(rename = "automatic-dev-sell")]
    pub automatic_dev_sell: bool,
    #[serde(rename = "dev-buy")]
    pub dev_buy: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchpadAvailability {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub supportState: String,
    pub tokenMetadata: TokenMetadataLimits,
    pub supportsStrategies: StrategySupport,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub officialSdk: Option<String>,
}

pub fn launchpad_registry() -> BTreeMap<String, LaunchpadAvailability> {
    let bags_configured = env::var("BAGS_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    [
        (
            "pump".to_string(),
            LaunchpadAvailability {
                id: "pump".to_string(),
                label: "Pump".to_string(),
                available: true,
                supportState: "verified".to_string(),
                tokenMetadata: TokenMetadataLimits {
                    nameMaxLength: 32,
                    symbolMaxLength: 10,
                },
                supportsStrategies: StrategySupport {
                    snipe_own_launch: true,
                    automatic_dev_sell: true,
                    dev_buy: true,
                },
                reason: String::new(),
                officialSdk: None,
            },
        ),
        (
            "bonk".to_string(),
            LaunchpadAvailability {
                id: "bonk".to_string(),
                label: "Bonk".to_string(),
                available: true,
                supportState: "unverified".to_string(),
                tokenMetadata: TokenMetadataLimits {
                    nameMaxLength: 32,
                    symbolMaxLength: 10,
                },
                supportsStrategies: StrategySupport {
                    snipe_own_launch: true,
                    automatic_dev_sell: true,
                    dev_buy: true,
                },
                reason: "Official Raydium-backed integration path still needs live validation."
                    .to_string(),
                officialSdk: Some("@raydium-io/raydium-sdk-v2".to_string()),
            },
        ),
        (
            "bagsapp".to_string(),
            LaunchpadAvailability {
                id: "bagsapp".to_string(),
                label: "Bagsapp".to_string(),
                available: bags_configured,
                supportState: if bags_configured {
                    "unverified".to_string()
                } else {
                    "configured-required".to_string()
                },
                tokenMetadata: TokenMetadataLimits {
                    nameMaxLength: 32,
                    symbolMaxLength: 10,
                },
                supportsStrategies: StrategySupport {
                    snipe_own_launch: false,
                    automatic_dev_sell: false,
                    dev_buy: true,
                },
                reason: if bags_configured {
                    "Bags integration is wired for the documented launch flow but still needs live validation.".to_string()
                } else {
                    "Missing BAGS_API_KEY.".to_string()
                },
                officialSdk: None,
            },
        ),
    ]
    .into_iter()
    .collect()
}
