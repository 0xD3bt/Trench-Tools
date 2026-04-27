#![allow(non_snake_case, dead_code)]

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
pub struct LaunchpadRuntimeCapabilities {
    pub compileLaunch: bool,
    pub quote: bool,
    pub startupWarm: bool,
    pub marketSnapshot: bool,
    pub importContext: bool,
    pub followBuy: bool,
    pub followSell: bool,
    pub atomicFollowBuy: bool,
    pub prelaunchSetup: bool,
    pub requestWarmBlockhashPrime: bool,
    pub helperBackedCompile: bool,
    pub helperBackedQuote: bool,
    pub helperBackedWarm: bool,
    pub helperBackedMarketSnapshot: bool,
    pub helperBackedImportContext: bool,
    pub helperBackedFollow: bool,
    pub supportsQuoteAssets: Vec<&'static str>,
}

pub fn launchpad_runtime_capabilities(launchpad: &str) -> Option<LaunchpadRuntimeCapabilities> {
    match launchpad.trim().to_ascii_lowercase().as_str() {
        "pump" => Some(LaunchpadRuntimeCapabilities {
            compileLaunch: true,
            quote: true,
            startupWarm: true,
            marketSnapshot: true,
            importContext: false,
            followBuy: true,
            followSell: true,
            atomicFollowBuy: true,
            prelaunchSetup: false,
            requestWarmBlockhashPrime: true,
            helperBackedCompile: false,
            helperBackedQuote: false,
            helperBackedWarm: false,
            helperBackedMarketSnapshot: false,
            helperBackedImportContext: false,
            helperBackedFollow: false,
            supportsQuoteAssets: vec!["sol"],
        }),
        "bonk" => Some(LaunchpadRuntimeCapabilities {
            compileLaunch: true,
            quote: true,
            startupWarm: true,
            marketSnapshot: true,
            importContext: true,
            followBuy: true,
            followSell: true,
            atomicFollowBuy: true,
            prelaunchSetup: false,
            requestWarmBlockhashPrime: false,
            helperBackedCompile: false,
            helperBackedQuote: false,
            helperBackedWarm: false,
            helperBackedMarketSnapshot: false,
            helperBackedImportContext: false,
            helperBackedFollow: false,
            supportsQuoteAssets: vec!["sol", "usd1"],
        }),
        "bagsapp" => Some(LaunchpadRuntimeCapabilities {
            compileLaunch: true,
            quote: true,
            startupWarm: true,
            marketSnapshot: true,
            importContext: true,
            followBuy: true,
            followSell: true,
            atomicFollowBuy: true,
            prelaunchSetup: true,
            requestWarmBlockhashPrime: true,
            helperBackedCompile: false,
            helperBackedQuote: false,
            helperBackedWarm: false,
            helperBackedMarketSnapshot: false,
            helperBackedImportContext: false,
            helperBackedFollow: false,
            supportsQuoteAssets: vec!["sol"],
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchpadAvailability {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub supportState: String,
    pub runtimeCapabilities: LaunchpadRuntimeCapabilities,
    pub tokenMetadata: TokenMetadataLimits,
    pub supportsStrategies: StrategySupport,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub officialSdk: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LaunchpadAvailabilityInputs {
    pub bags_configured: bool,
}

pub fn launchpad_registry(
    inputs: LaunchpadAvailabilityInputs,
) -> BTreeMap<String, LaunchpadAvailability> {
    let bags_configured = inputs.bags_configured;
    [
        (
            "pump".to_string(),
            LaunchpadAvailability {
                id: "pump".to_string(),
                label: "Pump".to_string(),
                available: true,
                supportState: "verified".to_string(),
                runtimeCapabilities: launchpad_runtime_capabilities("pump")
                    .expect("pump runtime capabilities"),
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
                supportState: "verified".to_string(),
                runtimeCapabilities: launchpad_runtime_capabilities("bonk")
                    .expect("bonk runtime capabilities"),
                tokenMetadata: TokenMetadataLimits {
                    nameMaxLength: 32,
                    symbolMaxLength: 10,
                },
                supportsStrategies: StrategySupport {
                    snipe_own_launch: true,
                    automatic_dev_sell: true,
                    dev_buy: true,
                },
                reason:
                    "Bonk routes through LetsBonk and Bonkers on Raydium LaunchLab with SOL/USD1 quote-asset support, auto USD1 top-up, compile/send, dev-buy, same-time snipers, dev auto-sell, and snipe buy/sell automation."
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
                    "supported".to_string()
                } else {
                    "configured-required".to_string()
                },
                runtimeCapabilities: launchpad_runtime_capabilities("bagsapp")
                    .expect("bagsapp runtime capabilities"),
                tokenMetadata: TokenMetadataLimits {
                    nameMaxLength: 32,
                    symbolMaxLength: 10,
                },
                supportsStrategies: StrategySupport {
                    snipe_own_launch: true,
                    automatic_dev_sell: true,
                    dev_buy: true,
                },
                reason: if bags_configured {
                    "Bags hosted launch flow is enabled with fee-share modes, wallet-only identity, dev-buy, same-time snipers, snipe buy/sell automation, and market-triggered auto-sell.".to_string()
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

pub fn strategy_registry() -> Value {
    json!({
        "none": {
            "id": "none",
            "label": "None",
            "description": "No post-launch automation."
        },
        "dev-buy": {
            "id": "dev-buy",
            "label": "Dev Buy",
            "description": "Include the configured developer buy during launch where supported."
        },
        "snipe-own-launch": {
            "id": "snipe-own-launch",
            "label": "Snipe Own Launch",
            "description": "Submit separate follow-up buy transactions around 1-2 blocks after launch."
        },
        "automatic-dev-sell": {
            "id": "automatic-dev-sell",
            "label": "Automatic Dev Sell",
            "description": "Sell a configured share of the dev wallet after launch with a short delay."
        }
    })
}
