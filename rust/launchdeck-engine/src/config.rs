#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

const TOKEN_NAME_MAX_LENGTH: usize = 32;
const TOKEN_SYMBOL_MAX_LENGTH: usize = 10;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawConfig {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub launchpad: String,
    #[serde(default)]
    pub token: RawToken,
    #[serde(default)]
    pub signer: RawSigner,
    #[serde(default)]
    pub agent: RawAgent,
    #[serde(default)]
    pub tx: RawTx,
    #[serde(default)]
    pub feeSharing: RawFeeSharing,
    #[serde(default)]
    pub creatorFee: RawCreatorFee,
    #[serde(default)]
    pub execution: RawExecution,
    #[serde(default)]
    pub initialBuySol: String,
    #[serde(default)]
    pub initialBuyTokens: String,
    #[serde(default)]
    pub devBuy: Option<RawDevBuy>,
    #[serde(default)]
    pub postLaunch: RawPostLaunch,
    #[serde(default)]
    pub presets: RawPresets,
    #[serde(default)]
    pub imageLocalPath: String,
    #[serde(default)]
    pub selectedWalletKey: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawToken {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub website: String,
    #[serde(default)]
    pub twitter: String,
    #[serde(default)]
    pub telegram: String,
    #[serde(default)]
    pub mayhemMode: Option<Value>,
    #[serde(default)]
    pub cashback: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawSigner {
    #[serde(default)]
    pub keypairFile: String,
    #[serde(default)]
    pub secretKey: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawAgent {
    #[serde(default)]
    pub authority: String,
    #[serde(default)]
    pub buybackBps: Option<Value>,
    #[serde(default)]
    pub splitAgentInit: Option<Value>,
    #[serde(default)]
    pub feeReceiver: String,
    #[serde(default)]
    pub feeRecipients: Vec<RawRecipient>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawTx {
    #[serde(default)]
    pub computeUnitLimit: Option<Value>,
    #[serde(default)]
    pub computeUnitPriceMicroLamports: Option<Value>,
    #[serde(default)]
    pub jitoTipLamports: Option<Value>,
    #[serde(default)]
    pub jitoTipAccount: String,
    #[serde(default)]
    pub lookupTables: Vec<String>,
    #[serde(default)]
    pub useDefaultLookupTables: Option<Value>,
    #[serde(default)]
    pub dumpBase64: Option<Value>,
    #[serde(default)]
    pub writeReport: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawFeeSharing {
    #[serde(default)]
    pub generateLaterSetup: Option<Value>,
    #[serde(default)]
    pub recipients: Vec<RawRecipient>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawCreatorFee {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub githubUsername: String,
    #[serde(default)]
    pub githubUserId: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawExecution {
    #[serde(default)]
    pub simulate: Option<Value>,
    #[serde(default)]
    pub send: Option<Value>,
    #[serde(default)]
    pub txFormat: String,
    #[serde(default)]
    pub commitment: String,
    #[serde(default)]
    pub skipPreflight: Option<Value>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub policy: String,
    #[serde(default)]
    pub autoGas: Option<Value>,
    #[serde(default)]
    pub autoMode: String,
    #[serde(default)]
    pub priorityFeeSol: String,
    #[serde(default)]
    pub tipSol: String,
    #[serde(default)]
    pub maxPriorityFeeSol: String,
    #[serde(default)]
    pub maxTipSol: String,
    #[serde(default)]
    pub buyProvider: String,
    #[serde(default)]
    pub buyPolicy: String,
    #[serde(default)]
    pub buyAutoGas: Option<Value>,
    #[serde(default)]
    pub buyAutoMode: String,
    #[serde(default)]
    pub buyPriorityFeeSol: String,
    #[serde(default)]
    pub buyTipSol: String,
    #[serde(default)]
    pub buySlippagePercent: String,
    #[serde(default)]
    pub buyMaxPriorityFeeSol: String,
    #[serde(default)]
    pub buyMaxTipSol: String,
    #[serde(default)]
    pub sellProvider: String,
    #[serde(default)]
    pub sellPolicy: String,
    #[serde(default)]
    pub sellPriorityFeeSol: String,
    #[serde(default)]
    pub sellTipSol: String,
    #[serde(default)]
    pub sellSlippagePercent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawDevBuy {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawPostLaunch {
    #[serde(default)]
    pub strategy: String,
    #[serde(default)]
    pub snipeOwnLaunch: RawSnipeOwnLaunch,
    #[serde(default)]
    pub automaticDevSell: RawAutomaticDevSell,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawSnipeOwnLaunch {
    #[serde(default)]
    pub buyAmountSol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawAutomaticDevSell {
    #[serde(default)]
    pub enabled: Option<Value>,
    #[serde(default)]
    pub percent: Option<Value>,
    #[serde(default)]
    pub delaySeconds: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawPresets {
    #[serde(default)]
    pub activePresetId: String,
    #[serde(default)]
    pub selectedLaunchPresetId: String,
    #[serde(default)]
    pub selectedSniperPresetId: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawRecipient {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub githubUsername: String,
    #[serde(default)]
    pub githubUserId: String,
    #[serde(default)]
    pub shareBps: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedConfig {
    pub mode: String,
    pub launchpad: String,
    pub token: NormalizedToken,
    pub signer: NormalizedSigner,
    pub agent: NormalizedAgent,
    pub tx: NormalizedTx,
    pub feeSharing: NormalizedFeeSharing,
    pub creatorFee: NormalizedCreatorFee,
    pub execution: NormalizedExecution,
    pub devBuy: Option<NormalizedDevBuy>,
    pub postLaunch: NormalizedPostLaunch,
    pub presets: NormalizedPresets,
    pub imageLocalPath: String,
    pub selectedWalletKey: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedToken {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub description: String,
    pub website: String,
    pub twitter: String,
    pub telegram: String,
    pub mayhemMode: bool,
    pub cashback: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedSigner {
    pub keypairFile: String,
    pub secretKey: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedAgent {
    pub authority: String,
    pub buybackBps: Option<i64>,
    pub splitAgentInit: bool,
    pub feeReceiver: String,
    pub feeRecipients: Vec<NormalizedRecipient>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedTx {
    pub computeUnitLimit: Option<i64>,
    pub computeUnitPriceMicroLamports: Option<i64>,
    pub jitoTipLamports: i64,
    pub jitoTipAccount: String,
    pub lookupTables: Vec<String>,
    pub useDefaultLookupTables: bool,
    pub dumpBase64: bool,
    pub writeReport: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedFeeSharing {
    pub generateLaterSetup: bool,
    pub recipients: Vec<NormalizedRecipient>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedCreatorFee {
    pub mode: String,
    pub address: String,
    pub githubUsername: String,
    pub githubUserId: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedExecution {
    pub simulate: bool,
    pub send: bool,
    pub txFormat: String,
    pub commitment: String,
    pub skipPreflight: bool,
    pub provider: String,
    pub policy: String,
    pub autoGas: bool,
    pub autoMode: String,
    pub priorityFeeSol: String,
    pub tipSol: String,
    pub maxPriorityFeeSol: String,
    pub maxTipSol: String,
    pub buyProvider: String,
    pub buyPolicy: String,
    pub buyAutoGas: bool,
    pub buyAutoMode: String,
    pub buyPriorityFeeSol: String,
    pub buyTipSol: String,
    pub buySlippagePercent: String,
    pub buyMaxPriorityFeeSol: String,
    pub buyMaxTipSol: String,
    pub sellProvider: String,
    pub sellPolicy: String,
    pub sellPriorityFeeSol: String,
    pub sellTipSol: String,
    pub sellSlippagePercent: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedDevBuy {
    pub mode: String,
    pub amount: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedPostLaunch {
    pub strategy: String,
    pub snipeOwnLaunch: NormalizedSnipeOwnLaunch,
    pub automaticDevSell: NormalizedAutomaticDevSell,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedSnipeOwnLaunch {
    pub enabled: bool,
    pub buyAmountSol: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedAutomaticDevSell {
    pub enabled: bool,
    pub percent: i64,
    pub delaySeconds: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedPresets {
    pub activePresetId: String,
    pub selectedLaunchPresetId: String,
    pub selectedSniperPresetId: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedRecipient {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    pub address: String,
    pub githubUserId: String,
    pub githubUsername: String,
    pub shareBps: i64,
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

fn parse_bool(value: &Option<Value>, fallback: bool) -> bool {
    match value {
        None => fallback,
        Some(Value::Bool(v)) => *v,
        Some(Value::String(v)) => match v.trim().to_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => !v.trim().is_empty(),
        },
        Some(Value::Number(n)) => n.as_i64().unwrap_or_default() != 0,
        Some(Value::Null) => fallback,
        Some(_) => fallback,
    }
}

fn parse_int(
    value: &Option<Value>,
    label: &str,
    min: Option<i64>,
    max: Option<i64>,
    fallback: Option<i64>,
) -> Result<Option<i64>, ConfigError> {
    let Some(value) = value else {
        return Ok(fallback);
    };
    if matches!(value, Value::Null) {
        return Ok(fallback);
    }
    if let Value::String(s) = value
        && s.trim().is_empty()
    {
        return Ok(fallback);
    }
    let parsed = match value {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    let Some(parsed) = parsed else {
        return Err(ConfigError::Message(format!(
            "{label} must be an integer. Got: {value}"
        )));
    };
    if let Some(min) = min
        && parsed < min
    {
        return Err(ConfigError::Message(format!(
            "{label} must be >= {min}. Got: {parsed}"
        )));
    }
    if let Some(max) = max
        && parsed > max
    {
        return Err(ConfigError::Message(format!(
            "{label} must be <= {max}. Got: {parsed}"
        )));
    }
    Ok(Some(parsed))
}

fn parse_choice(
    value: &str,
    label: &str,
    allowed: &[&str],
    fallback: &str,
) -> Result<String, ConfigError> {
    let normalized = if is_blank(value) { fallback } else { value }
        .trim()
        .to_lowercase();
    if allowed.iter().any(|entry| *entry == normalized) {
        Ok(normalized)
    } else {
        Err(ConfigError::Message(format!(
            "{label} must be one of {}. Got: {value}",
            allowed.join(", ")
        )))
    }
}

fn parse_limited_string(
    value: &str,
    label: &str,
    max_length: usize,
    required: bool,
) -> Result<String, ConfigError> {
    let normalized = value.trim().to_string();
    if required && normalized.is_empty() {
        return Err(ConfigError::Message(format!("{label} is required.")));
    }
    if normalized.len() > max_length {
        return Err(ConfigError::Message(format!(
            "{label} must be at most {max_length} characters."
        )));
    }
    Ok(normalized)
}

fn normalize_recipients(
    entries: &[RawRecipient],
    label_prefix: &str,
    allow_agent: bool,
) -> Result<Vec<NormalizedRecipient>, ConfigError> {
    let mut normalized = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.shareBps.is_none() {
            continue;
        }
        let share_bps = parse_int(
            &entry.shareBps,
            &format!("{label_prefix} shareBps at index {index}"),
            Some(1),
            Some(10_000),
            None,
        )?
        .unwrap_or_default();
        let entry_type = if entry.r#type.trim().is_empty() {
            "wallet".to_string()
        } else {
            entry.r#type.trim().to_lowercase()
        };
        let address = entry.address.trim().to_string();
        let github_user_id = entry.githubUserId.trim().to_string();
        let github_username = entry.githubUsername.trim().to_string();

        if allow_agent && entry_type == "agent" {
            normalized.push(NormalizedRecipient {
                r#type: Some(entry_type),
                address: String::new(),
                githubUserId: String::new(),
                githubUsername: String::new(),
                shareBps: share_bps,
            });
            continue;
        }

        if address.is_empty() && github_user_id.is_empty() {
            return Err(ConfigError::Message(format!(
                "{label_prefix} at index {index} must provide either address or githubUserId."
            )));
        }

        normalized.push(NormalizedRecipient {
            r#type: if allow_agent { Some(entry_type) } else { None },
            address,
            githubUserId: github_user_id,
            githubUsername: github_username,
            shareBps: share_bps,
        });
    }

    if normalized.is_empty() {
        return Ok(normalized);
    }
    let total: i64 = normalized.iter().map(|entry| entry.shareBps).sum();
    if total != 10_000 {
        return Err(ConfigError::Message(format!(
            "{}s must total 10000 bps. Got: {}",
            label_prefix, total
        )));
    }

    Ok(normalized)
}

fn normalize_creator_fee(
    raw: &RawCreatorFee,
    mode: &str,
) -> Result<NormalizedCreatorFee, ConfigError> {
    match mode {
        "agent-locked" => {
            return Ok(NormalizedCreatorFee {
                mode: "agent-escrow".to_string(),
                address: String::new(),
                githubUsername: String::new(),
                githubUserId: String::new(),
            });
        }
        "agent-custom" | "agent-unlocked" => {
            return Ok(NormalizedCreatorFee {
                mode: "deployer".to_string(),
                address: String::new(),
                githubUsername: String::new(),
                githubUserId: String::new(),
            });
        }
        "cashback" => {
            return Ok(NormalizedCreatorFee {
                mode: "cashback".to_string(),
                address: String::new(),
                githubUsername: String::new(),
                githubUserId: String::new(),
            });
        }
        _ => {}
    }

    let normalized_mode = if is_blank(&raw.mode) {
        "deployer".to_string()
    } else {
        raw.mode.trim().to_lowercase()
    };
    if !["deployer", "wallet", "github"].contains(&normalized_mode.as_str()) {
        return Err(ConfigError::Message(format!(
            "creatorFee.mode must be one of deployer, wallet, github. Got: {}",
            normalized_mode
        )));
    }
    let normalized = NormalizedCreatorFee {
        mode: normalized_mode,
        address: raw.address.trim().to_string(),
        githubUsername: raw.githubUsername.trim().to_string(),
        githubUserId: raw.githubUserId.trim().to_string(),
    };
    if normalized.mode == "wallet" && normalized.address.is_empty() {
        return Err(ConfigError::Message(
            "creatorFee.address is required when creatorFee.mode is wallet.".to_string(),
        ));
    }
    if normalized.mode == "github" && normalized.githubUserId.is_empty() {
        return Err(ConfigError::Message(
            "creatorFee.githubUserId is required when creatorFee.mode is github.".to_string(),
        ));
    }
    Ok(normalized)
}

fn normalize_dev_buy(raw: &RawConfig) -> Result<Option<NormalizedDevBuy>, ConfigError> {
    let object_mode = raw
        .devBuy
        .as_ref()
        .map(|entry| entry.mode.trim().to_string())
        .unwrap_or_default();
    let object_amount = raw
        .devBuy
        .as_ref()
        .map(|entry| entry.amount.trim().to_string())
        .unwrap_or_default();
    let shorthand_sol = raw.initialBuySol.trim().to_string();
    let shorthand_tokens = raw.initialBuyTokens.trim().to_string();

    if !shorthand_sol.is_empty() && !shorthand_tokens.is_empty() {
        return Err(ConfigError::Message(
            "Use only one of initialBuySol or initialBuyTokens.".to_string(),
        ));
    }
    if !object_mode.is_empty() || !object_amount.is_empty() {
        if !shorthand_sol.is_empty() || !shorthand_tokens.is_empty() {
            return Err(ConfigError::Message(
                "Use either devBuy object form or shorthand initialBuySol/initialBuyTokens, not both."
                    .to_string(),
            ));
        }
        if object_mode.is_empty() || object_amount.is_empty() {
            return Err(ConfigError::Message(
                "devBuy.mode and devBuy.amount must both be set.".to_string(),
            ));
        }
        let mode = object_mode.to_lowercase();
        if mode != "sol" && mode != "tokens" {
            return Err(ConfigError::Message(format!(
                "devBuy.mode must be 'sol' or 'tokens'. Got: {}",
                object_mode
            )));
        }
        return Ok(Some(NormalizedDevBuy {
            mode,
            amount: object_amount,
            source: "object".to_string(),
        }));
    }
    if !shorthand_sol.is_empty() {
        return Ok(Some(NormalizedDevBuy {
            mode: "sol".to_string(),
            amount: shorthand_sol,
            source: "shorthand".to_string(),
        }));
    }
    if !shorthand_tokens.is_empty() {
        return Ok(Some(NormalizedDevBuy {
            mode: "tokens".to_string(),
            amount: shorthand_tokens,
            source: "shorthand".to_string(),
        }));
    }
    Ok(None)
}

pub fn normalize_raw_config(raw: RawConfig) -> Result<NormalizedConfig, ConfigError> {
    let requested_mode = if raw.mode.trim().is_empty() {
        "regular".to_string()
    } else {
        raw.mode.trim().to_lowercase()
    };
    let mode = match requested_mode.as_str() {
        "custom" | "agent-custom-live" => "agent-custom".to_string(),
        _ => requested_mode,
    };
    if ![
        "regular",
        "cashback",
        "agent-custom",
        "agent-unlocked",
        "agent-locked",
    ]
    .contains(&mode.as_str())
    {
        return Err(ConfigError::Message(format!(
            "mode must be one of regular, cashback, agent-custom, agent-unlocked, agent-locked. Got: {mode}"
        )));
    }

    let launchpad = parse_choice(
        &raw.launchpad,
        "launchpad",
        &["pump", "bonk", "bagsapp"],
        "pump",
    )?;
    let post_launch_strategy = parse_choice(
        &raw.postLaunch.strategy,
        "postLaunch.strategy",
        &["none", "dev-buy", "snipe-own-launch", "automatic-dev-sell"],
        "none",
    )?;

    let fee_recipients =
        normalize_recipients(&raw.feeSharing.recipients, "feeSharing recipient", false)?;
    let mut agent_fee_recipients =
        normalize_recipients(&raw.agent.feeRecipients, "agent recipient", true)?;

    let mut normalized = NormalizedConfig {
        mode: mode.clone(),
        launchpad,
        token: NormalizedToken {
            name: parse_limited_string(&raw.token.name, "token.name", TOKEN_NAME_MAX_LENGTH, true)?,
            symbol: parse_limited_string(
                &raw.token.symbol,
                "token.symbol",
                TOKEN_SYMBOL_MAX_LENGTH,
                true,
            )?,
            uri: raw.token.uri.trim().to_string(),
            description: raw.token.description.trim().to_string(),
            website: raw.token.website.trim().to_string(),
            twitter: raw.token.twitter.trim().to_string(),
            telegram: raw.token.telegram.trim().to_string(),
            mayhemMode: parse_bool(&raw.token.mayhemMode, false),
            cashback: if mode == "cashback" {
                true
            } else {
                parse_bool(&raw.token.cashback, false)
            },
        },
        signer: NormalizedSigner {
            keypairFile: raw.signer.keypairFile.trim().to_string(),
            secretKey: raw.signer.secretKey.trim().to_string(),
        },
        agent: NormalizedAgent {
            authority: raw.agent.authority.trim().to_string(),
            buybackBps: parse_int(
                &raw.agent.buybackBps,
                "buybackBps",
                Some(0),
                Some(10_000),
                None,
            )?,
            splitAgentInit: parse_bool(&raw.agent.splitAgentInit, false),
            feeReceiver: raw.agent.feeReceiver.trim().to_string(),
            feeRecipients: agent_fee_recipients.clone(),
        },
        tx: NormalizedTx {
            computeUnitLimit: parse_int(
                &raw.tx.computeUnitLimit,
                "computeUnitLimit",
                Some(1),
                None,
                None,
            )?,
            computeUnitPriceMicroLamports: parse_int(
                &raw.tx.computeUnitPriceMicroLamports,
                "computeUnitPriceMicroLamports",
                Some(0),
                None,
                None,
            )?,
            jitoTipLamports: parse_int(
                &raw.tx.jitoTipLamports,
                "jitoTipLamports",
                Some(0),
                None,
                Some(0),
            )?
            .unwrap_or(0),
            jitoTipAccount: raw.tx.jitoTipAccount.trim().to_string(),
            lookupTables: raw
                .tx
                .lookupTables
                .iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect(),
            useDefaultLookupTables: parse_bool(&raw.tx.useDefaultLookupTables, true),
            dumpBase64: parse_bool(&raw.tx.dumpBase64, false),
            writeReport: parse_bool(&raw.tx.writeReport, false),
        },
        feeSharing: NormalizedFeeSharing {
            generateLaterSetup: parse_bool(&raw.feeSharing.generateLaterSetup, false),
            recipients: fee_recipients,
        },
        creatorFee: normalize_creator_fee(&raw.creatorFee, &mode)?,
        execution: NormalizedExecution {
            simulate: parse_bool(&raw.execution.simulate, false),
            send: parse_bool(&raw.execution.send, false),
            txFormat: parse_choice(
                &raw.execution.txFormat,
                "txFormat",
                &["auto", "v0-alt", "v0", "legacy"],
                "auto",
            )?,
            commitment: parse_choice(
                &raw.execution.commitment,
                "commitment",
                &["processed", "confirmed", "finalized"],
                "confirmed",
            )?,
            skipPreflight: parse_bool(&raw.execution.skipPreflight, false),
            provider: parse_choice(
                &raw.execution.provider,
                "execution.provider",
                &[
                    "auto",
                    "helius",
                    "jito",
                    "astralane",
                    "bloxroute",
                    "hellomoon",
                ],
                "auto",
            )?,
            policy: parse_choice(
                &raw.execution.policy,
                "execution.policy",
                &["fast", "safe"],
                "fast",
            )?,
            autoGas: parse_bool(&raw.execution.autoGas, true),
            autoMode: if is_blank(&raw.execution.autoMode) {
                "launchAuto".to_string()
            } else {
                raw.execution.autoMode.trim().to_string()
            },
            priorityFeeSol: raw.execution.priorityFeeSol.trim().to_string(),
            tipSol: raw.execution.tipSol.trim().to_string(),
            maxPriorityFeeSol: raw.execution.maxPriorityFeeSol.trim().to_string(),
            maxTipSol: raw.execution.maxTipSol.trim().to_string(),
            buyProvider: parse_choice(
                &raw.execution.buyProvider,
                "execution.buyProvider",
                &[
                    "auto",
                    "helius",
                    "jito",
                    "astralane",
                    "bloxroute",
                    "hellomoon",
                ],
                "auto",
            )?,
            buyPolicy: parse_choice(
                &raw.execution.buyPolicy,
                "execution.buyPolicy",
                &["fast", "safe"],
                "fast",
            )?,
            buyAutoGas: parse_bool(&raw.execution.buyAutoGas, true),
            buyAutoMode: if is_blank(&raw.execution.buyAutoMode) {
                "buyAuto".to_string()
            } else {
                raw.execution.buyAutoMode.trim().to_string()
            },
            buyPriorityFeeSol: raw.execution.buyPriorityFeeSol.trim().to_string(),
            buyTipSol: raw.execution.buyTipSol.trim().to_string(),
            buySlippagePercent: raw.execution.buySlippagePercent.trim().to_string(),
            buyMaxPriorityFeeSol: raw.execution.buyMaxPriorityFeeSol.trim().to_string(),
            buyMaxTipSol: raw.execution.buyMaxTipSol.trim().to_string(),
            sellProvider: parse_choice(
                &raw.execution.sellProvider,
                "execution.sellProvider",
                &[
                    "auto",
                    "helius",
                    "jito",
                    "astralane",
                    "bloxroute",
                    "hellomoon",
                ],
                "helius",
            )?,
            sellPolicy: parse_choice(
                &raw.execution.sellPolicy,
                "execution.sellPolicy",
                &["fast", "safe"],
                "safe",
            )?,
            sellPriorityFeeSol: raw.execution.sellPriorityFeeSol.trim().to_string(),
            sellTipSol: raw.execution.sellTipSol.trim().to_string(),
            sellSlippagePercent: raw.execution.sellSlippagePercent.trim().to_string(),
        },
        devBuy: normalize_dev_buy(&raw)?,
        postLaunch: NormalizedPostLaunch {
            strategy: post_launch_strategy.clone(),
            snipeOwnLaunch: NormalizedSnipeOwnLaunch {
                enabled: post_launch_strategy == "snipe-own-launch",
                buyAmountSol: raw
                    .postLaunch
                    .snipeOwnLaunch
                    .buyAmountSol
                    .trim()
                    .to_string(),
            },
            automaticDevSell: NormalizedAutomaticDevSell {
                enabled: post_launch_strategy == "automatic-dev-sell"
                    || parse_bool(&raw.postLaunch.automaticDevSell.enabled, false),
                percent: parse_int(
                    &raw.postLaunch.automaticDevSell.percent,
                    "postLaunch.automaticDevSell.percent",
                    Some(0),
                    Some(100),
                    Some(0),
                )?
                .unwrap_or(0),
                delaySeconds: parse_int(
                    &raw.postLaunch.automaticDevSell.delaySeconds,
                    "postLaunch.automaticDevSell.delaySeconds",
                    Some(0),
                    Some(10),
                    Some(0),
                )?
                .unwrap_or(0),
            },
        },
        presets: NormalizedPresets {
            activePresetId: raw.presets.activePresetId.trim().to_string(),
            selectedLaunchPresetId: raw.presets.selectedLaunchPresetId.trim().to_string(),
            selectedSniperPresetId: raw.presets.selectedSniperPresetId.trim().to_string(),
        },
        imageLocalPath: raw.imageLocalPath.trim().to_string(),
        selectedWalletKey: raw.selectedWalletKey.trim().to_string(),
    };

    if normalized.token.uri.is_empty() {
        return Err(ConfigError::Message("token.uri is required.".to_string()));
    }

    if mode == "regular" || mode == "cashback" {
        normalized.agent.buybackBps = None;
        normalized.agent.feeRecipients = vec![];
    }
    if ["agent-custom", "agent-unlocked", "agent-locked"].contains(&mode.as_str())
        && normalized.agent.buybackBps.is_none()
    {
        return Err(ConfigError::Message(format!(
            "buybackBps is required for {mode} mode."
        )));
    }
    if mode == "agent-unlocked" || mode == "agent-locked" {
        normalized.agent.feeRecipients = vec![];
    }
    if (mode == "regular" || mode == "cashback") && normalized.agent.splitAgentInit {
        normalized.agent.splitAgentInit = false;
    }
    if mode != "regular" && normalized.feeSharing.generateLaterSetup {
        return Err(ConfigError::Message(
            "feeSharing.generateLaterSetup is only supported in regular mode.".to_string(),
        ));
    }
    if mode == "regular"
        && normalized.feeSharing.generateLaterSetup
        && normalized.creatorFee.mode != "deployer"
    {
        return Err(ConfigError::Message(
            "Later fee-sharing setup is only supported when the regular-mode creator fee receiver is the deployer."
                .to_string(),
        ));
    }
    if normalized.feeSharing.generateLaterSetup && normalized.feeSharing.recipients.is_empty() {
        return Err(ConfigError::Message(
            "feeSharing.recipients is required when feeSharing.generateLaterSetup is true."
                .to_string(),
        ));
    }
    if normalized.tx.jitoTipLamports > 0 && normalized.tx.jitoTipAccount.trim().is_empty() {
        return Err(ConfigError::Message(
            "jitoTipAccount is required when jitoTipLamports is set.".to_string(),
        ));
    }

    // Keep variable live for future parity extensions.
    agent_fee_recipients.clear();

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_raw_config() -> RawConfig {
        serde_json::from_value(json!({
            "mode": "regular",
            "launchpad": "pump",
            "token": {
                "name": "LaunchDeck",
                "symbol": "LDECK",
                "uri": "ipfs://test",
                "description": "demo"
            },
            "agent": {
                "splitAgentInit": false,
                "feeRecipients": []
            },
            "tx": {
                "jitoTipLamports": 0,
                "useDefaultLookupTables": true,
                "dumpBase64": false,
                "writeReport": true
            },
            "feeSharing": {
                "generateLaterSetup": false,
                "recipients": []
            },
            "creatorFee": {
                "mode": "deployer"
            },
            "execution": {
                "simulate": false,
                "send": false,
                "txFormat": "auto",
                "commitment": "confirmed",
                "skipPreflight": false,
                "provider": "auto",
                "policy": "fast",
                "autoGas": true,
                "autoMode": "launchAuto",
                "buyProvider": "auto",
                "buyPolicy": "fast",
                "buyAutoGas": true,
                "buyAutoMode": "buyAuto"
            },
            "postLaunch": {
                "strategy": "none",
                "snipeOwnLaunch": { "buyAmountSol": "" },
                "automaticDevSell": {
                    "enabled": false,
                    "percent": 0,
                    "delaySeconds": 0
                }
            },
            "presets": {}
        }))
        .expect("sample config should deserialize")
    }

    #[test]
    fn normalizes_minimal_regular_config() {
        let normalized =
            normalize_raw_config(sample_raw_config()).expect("config should normalize");
        assert_eq!(normalized.mode, "regular");
        assert_eq!(normalized.launchpad, "pump");
        assert_eq!(normalized.token.name, "LaunchDeck");
        assert_eq!(normalized.execution.provider, "auto");
        assert_eq!(normalized.execution.buyProvider, "auto");
        assert!(normalized.devBuy.is_none());
    }

    #[test]
    fn requires_jito_tip_account_when_tip_is_set() {
        let mut raw = sample_raw_config();
        raw.tx.jitoTipLamports = Some(json!(1000));
        let error = normalize_raw_config(raw).expect_err("jito tip account should be required");
        assert_eq!(
            error.to_string(),
            "jitoTipAccount is required when jitoTipLamports is set."
        );
    }
}
