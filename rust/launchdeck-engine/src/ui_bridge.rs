#![allow(non_snake_case)]

use crate::{
    config::{
        RawAgent, RawAutomaticDevSell, RawConfig, RawCreatorFee, RawExecution, RawFeeSharing,
        RawPostLaunch, RawPresets, RawRecipient, RawSnipeOwnLaunch, RawToken, RawTx,
    },
    paths,
    pump_native::{LaunchQuote, quote_launch},
    wallet::selected_wallet_key_or_default,
};
use reqwest::{Client, multipart};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, path::Path, time::{SystemTime, UNIX_EPOCH}};

const FIXED_COMPUTE_UNIT_LIMIT: u64 = 1_000_000;
const HELIUS_SENDER_TIP_ACCOUNTS: [&str; 3] = [
    "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
    "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
    "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
];
const JITO_TIP_ACCOUNTS: [&str; 8] = [
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
    "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
    "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
    "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];

#[derive(Debug, Deserialize, Default)]
pub struct QuoteForm {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub amount: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct UiForm {
    #[serde(default)]
    pub selectedWalletKey: String,
    #[serde(default)]
    pub activePresetId: String,
    #[serde(default)]
    pub launchpad: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub website: String,
    #[serde(default)]
    pub twitter: String,
    #[serde(default)]
    pub telegram: String,
    #[serde(default)]
    pub metadataUri: String,
    #[serde(default)]
    pub mayhemMode: bool,
    #[serde(default)]
    pub agentAuthority: String,
    #[serde(default)]
    pub buybackPercent: String,
    #[serde(default)]
    pub agentSplitRecipients: Vec<UiRecipientInput>,
    #[serde(default)]
    pub devBuyMode: String,
    #[serde(default)]
    pub devBuyAmount: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub endpointProfile: String,
    #[serde(default)]
    pub policy: String,
    #[serde(default)]
    pub priorityFeeSol: String,
    #[serde(default)]
    pub creationTipSol: String,
    #[serde(default)]
    pub buyProvider: String,
    #[serde(default)]
    pub buyEndpointProfile: String,
    #[serde(default)]
    pub buyPolicy: String,
    #[serde(default)]
    pub buyPriorityFeeSol: String,
    #[serde(default)]
    pub buyTipSol: String,
    #[serde(default)]
    pub buySlippagePercent: String,
    #[serde(default)]
    pub sellProvider: String,
    #[serde(default)]
    pub sellEndpointProfile: String,
    #[serde(default)]
    pub sellPolicy: String,
    #[serde(default)]
    pub sellPriorityFeeSol: String,
    #[serde(default)]
    pub sellTipSol: String,
    #[serde(default)]
    pub sellSlippagePercent: String,
    #[serde(default)]
    pub skipPreflight: bool,
    #[serde(default)]
    pub trackSendBlockHeight: bool,
    #[serde(default)]
    pub feeSplitEnabled: bool,
    #[serde(default)]
    pub feeSplitRecipients: Vec<UiRecipientInput>,
    #[serde(default)]
    pub postLaunchStrategy: String,
    #[serde(default)]
    pub snipeBuyAmountSol: String,
    #[serde(default)]
    pub automaticDevSellEnabled: bool,
    #[serde(default)]
    pub automaticDevSellPercent: String,
    #[serde(default)]
    pub automaticDevSellDelaySeconds: String,
    #[serde(default)]
    pub imageFileName: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct UiRecipientInput {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub githubUsername: String,
    #[serde(default)]
    pub githubUserId: String,
    #[serde(default)]
    pub shareBps: Option<i64>,
}

fn uploads_dir() -> std::path::PathBuf {
    paths::uploads_dir()
}

fn sanitize_provider(value: &str) -> String {
    let trimmed = value.trim().to_lowercase();
    if trimmed.is_empty() {
        "helius-sender".to_string()
    } else {
        trimmed
    }
}

fn tip_supported(provider: &str) -> bool {
    matches!(provider, "helius-sender" | "jito-bundle")
}

fn pick_tip_account(provider: &str) -> String {
    let accounts = if provider == "helius-sender" {
        &HELIUS_SENDER_TIP_ACCOUNTS[..]
    } else {
        &JITO_TIP_ACCOUNTS[..]
    };
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or_default();
    accounts[seed % accounts.len()].to_string()
}

fn parse_decimal_to_u64(value: &str, decimals: u32, label: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    if !trimmed
        .chars()
        .all(|char| char.is_ascii_digit() || char == '.')
    {
        return Err(format!(
            "{label} must be a positive decimal string. Got: {value}"
        ));
    }
    let mut parts = trimmed.split('.');
    let whole = parts.next().unwrap_or_default();
    let fractional = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err(format!(
            "{label} must be a positive decimal string. Got: {value}"
        ));
    }
    if fractional.len() > decimals as usize {
        return Err(format!(
            "{label} supports at most {decimals} decimal places. Got: {value}"
        ));
    }
    let normalized = format!("{whole}{fractional:0<width$}", width = decimals as usize);
    let digits = normalized.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(0);
    }
    digits.parse::<u64>().map_err(|error| error.to_string())
}

fn lamports_to_priority_fee_micro_lamports(priority_fee_lamports: u64) -> u64 {
    if priority_fee_lamports == 0 {
        0
    } else {
        (priority_fee_lamports.saturating_mul(1_000_000)) / FIXED_COMPUTE_UNIT_LIMIT
    }
}

fn buyback_percent_to_bps(raw_value: &str) -> Result<Option<i64>, String> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let numeric = trimmed
        .parse::<f64>()
        .map_err(|_| format!("buyback percentage must be between 0 and 100. Got: {raw_value}"))?;
    if !(0.0..=100.0).contains(&numeric) {
        return Err(format!(
            "buyback percentage must be between 0 and 100. Got: {raw_value}"
        ));
    }
    Ok(Some((numeric * 100.0).round() as i64))
}

fn parse_recipients(
    entries: &[UiRecipientInput],
    allow_agent: bool,
) -> Result<Vec<RawRecipient>, String> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let entry_type = if entry.r#type.trim().is_empty() {
                "wallet".to_string()
            } else {
                entry.r#type.trim().to_lowercase()
            };
            let share_bps = entry.shareBps.ok_or_else(|| {
                format!("Fee split recipient {} must have a positive share.", index + 1)
            })?;
            if share_bps <= 0 {
                return Err(format!(
                    "Fee split recipient {} must have a positive share.",
                    index + 1
                ));
            }
            if allow_agent && entry_type == "agent" {
                return Ok(RawRecipient {
                    r#type: "agent".to_string(),
                    shareBps: Some(json!(share_bps)),
                    ..RawRecipient::default()
                });
            }
            if entry_type == "wallet" {
                let address = entry.address.trim();
                if address.is_empty() {
                    return Err(format!(
                        "Fee split recipient {} is missing a wallet address.",
                        index + 1
                    ));
                }
                return Ok(RawRecipient {
                    address: address.to_string(),
                    shareBps: Some(json!(share_bps)),
                    ..RawRecipient::default()
                });
            }
            if entry_type == "github" {
                let username = entry.githubUsername.trim();
                if username.is_empty() {
                    return Err(format!(
                        "Fee split recipient {} is missing a GitHub username.",
                        index + 1
                    ));
                }
                return Ok(RawRecipient {
                    r#type: "github".to_string(),
                    githubUsername: username.to_string(),
                    githubUserId: entry.githubUserId.trim().to_string(),
                    shareBps: Some(json!(share_bps)),
                    ..RawRecipient::default()
                });
            }
            Err(format!("Unsupported fee split recipient type: {entry_type}"))
        })
        .collect()
}

async fn resolve_github_user(username: &str) -> Result<(String, String), String> {
    let trimmed = username.trim();
    if trimmed.is_empty() {
        return Err("GitHub username is required.".to_string());
    }
    let response = Client::new()
        .get(format!("https://api.github.com/users/{trimmed}"))
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "launchdeck-rust-engine")
        .send()
        .await
        .map_err(|error| format!("Failed to look up GitHub user @{trimmed}: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub user @{trimmed} was not found (status {}).",
            response.status()
        ));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse GitHub response for @{trimmed}: {error}"))?;
    let login = payload
        .get("login")
        .and_then(Value::as_str)
        .unwrap_or(trimmed)
        .trim()
        .to_string();
    let user_id = payload
        .get("id")
        .and_then(Value::as_i64)
        .map(|value| value.to_string())
        .ok_or_else(|| format!("GitHub user @{trimmed} did not include a numeric id."))?;
    Ok((login, user_id))
}

async fn resolve_recipient_github_ids(recipients: &mut [RawRecipient]) -> Result<(), String> {
    for recipient in recipients {
        if recipient.githubUsername.trim().is_empty() || !recipient.githubUserId.trim().is_empty() {
            continue;
        }
        let (username, user_id) = resolve_github_user(&recipient.githubUsername).await?;
        recipient.githubUsername = username;
        recipient.githubUserId = user_id;
    }
    Ok(())
}

fn image_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}

async fn upload_metadata_to_pump_fun(config: &RawConfig) -> Result<String, String> {
    let image_file_name = Path::new(&config.imageLocalPath)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if image_file_name.is_empty() {
        return Err("Image is required before launching.".to_string());
    }
    let image_path = uploads_dir().join(&image_file_name);
    let image_bytes = fs::read(&image_path)
        .map_err(|error| format!("Failed to read uploaded image {}: {error}", image_path.display()))?;
    let image_part = multipart::Part::bytes(image_bytes)
        .file_name(image_file_name)
        .mime_str(image_mime(&image_path))
        .map_err(|error| format!("Failed to prepare uploaded image: {error}"))?;
    let mut metadata = serde_json::Map::new();
    if !config.token.website.trim().is_empty() {
        metadata.insert("website".to_string(), Value::String(config.token.website.clone()));
    }
    if !config.token.twitter.trim().is_empty() {
        metadata.insert("twitter".to_string(), Value::String(config.token.twitter.clone()));
    }
    if !config.token.telegram.trim().is_empty() {
        metadata.insert("telegram".to_string(), Value::String(config.token.telegram.clone()));
    }
    let form = multipart::Form::new()
        .text("name", config.token.name.clone())
        .text("symbol", config.token.symbol.clone())
        .text("description", config.token.description.clone())
        .text("showName", "true")
        .text("metadata", Value::Object(metadata).to_string())
        .part("file", image_part);
    let response = Client::new()
        .post("https://pump.fun/api/ipfs")
        .multipart(form)
        .send()
        .await
        .map_err(|error| format!("Metadata upload failed: {error}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Metadata upload failed with status {status}: {}",
            if body.trim().is_empty() {
                "empty response".to_string()
            } else {
                body
            }
        ));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse metadata upload response: {error}"))?;
    payload
        .get("metadataUri")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Metadata upload did not return metadataUri.".to_string())
}

pub async fn quote_from_form(rpc_url: &str, form_value: Value) -> Result<Option<LaunchQuote>, String> {
    let form: QuoteForm = serde_json::from_value(form_value)
        .map_err(|error| format!("Invalid quote form payload: {error}"))?;
    quote_launch(rpc_url, &form.mode, &form.amount).await
}

pub async fn build_raw_config_from_form(
    action: &str,
    form_value: Value,
) -> Result<(RawConfig, Option<String>), String> {
    let form: UiForm = serde_json::from_value(form_value)
        .map_err(|error| format!("Invalid launch form payload: {error}"))?;
    let mode = if form.mode.trim().is_empty() {
        "regular".to_string()
    } else {
        form.mode.trim().to_lowercase()
    };
    let launchpad = if form.launchpad.trim().is_empty() {
        "pump".to_string()
    } else {
        form.launchpad.trim().to_lowercase()
    };
    let provider = sanitize_provider(&form.provider);
    let endpoint_profile = if provider == "standard-rpc" {
        String::new()
    } else {
        form.endpointProfile.trim().to_lowercase()
    };
    let buy_provider = sanitize_provider(if form.buyProvider.trim().is_empty() {
        &provider
    } else {
        &form.buyProvider
    });
    let buy_endpoint_profile = if buy_provider == "standard-rpc" {
        String::new()
    } else if form.buyEndpointProfile.trim().is_empty() {
        endpoint_profile.clone()
    } else {
        form.buyEndpointProfile.trim().to_lowercase()
    };
    let sell_provider = sanitize_provider(if form.sellProvider.trim().is_empty() {
        &provider
    } else {
        &form.sellProvider
    });
    let sell_endpoint_profile = if sell_provider == "standard-rpc" {
        String::new()
    } else if form.sellEndpointProfile.trim().is_empty() {
        endpoint_profile.clone()
    } else {
        form.sellEndpointProfile.trim().to_lowercase()
    };
    let priority_fee_lamports =
        parse_decimal_to_u64(&form.priorityFeeSol, 9, "creation priority fee")?;
    let tip_lamports = if tip_supported(&provider) {
        parse_decimal_to_u64(&form.creationTipSol, 9, "creation tip")?
    } else {
        0
    };
    let is_agent_complete = matches!(mode.as_str(), "agent-unlocked" | "agent-locked");
    let is_agent_custom = mode == "agent-custom";
    let is_agent_unlocked = mode == "agent-unlocked";
    let fee_split_enabled = mode == "regular" && form.feeSplitEnabled;
    let mut agent_fee_recipients = if is_agent_custom {
        parse_recipients(&form.agentSplitRecipients, true)?
    } else {
        vec![]
    };
    let agent_recipient_share = agent_fee_recipients
        .iter()
        .find(|entry| entry.r#type == "agent")
        .and_then(|entry| entry.shareBps.as_ref())
        .and_then(Value::as_i64);
    let buyback_bps = if is_agent_complete {
        Some(10_000)
    } else if is_agent_custom {
        agent_recipient_share
            .map(Some)
            .unwrap_or(buyback_percent_to_bps(&form.buybackPercent)?)
    } else {
        buyback_percent_to_bps(&form.buybackPercent)?
    };
    let mut fee_sharing_recipients = if fee_split_enabled {
        parse_recipients(&form.feeSplitRecipients, false)?
    } else {
        vec![]
    };
    resolve_recipient_github_ids(&mut fee_sharing_recipients).await?;
    resolve_recipient_github_ids(&mut agent_fee_recipients).await?;

    let image_file_name = Path::new(form.imageFileName.trim())
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let selected_wallet_key = selected_wallet_key_or_default(&form.selectedWalletKey)
        .unwrap_or_default();
    let mut raw = RawConfig {
        mode: mode.clone(),
        launchpad,
        token: RawToken {
            name: form.name.trim().to_string(),
            symbol: form.symbol.trim().to_string(),
            uri: form.metadataUri.trim().to_string(),
            description: form.description.trim().to_string(),
            website: form.website.trim().to_string(),
            twitter: form.twitter.trim().to_string(),
            telegram: form.telegram.trim().to_string(),
            mayhemMode: Some(json!(form.mayhemMode)),
            cashback: if mode == "cashback" {
                Some(json!(true))
            } else {
                None
            },
        },
        signer: Default::default(),
        agent: RawAgent {
            authority: if is_agent_complete {
                String::new()
            } else {
                form.agentAuthority.trim().to_string()
            },
            buybackBps: buyback_bps.map(|value| json!(value)),
            splitAgentInit: Some(json!(is_agent_custom || mode == "agent-locked")),
            feeReceiver: String::new(),
            feeRecipients: if is_agent_complete || is_agent_unlocked {
                vec![]
            } else {
                agent_fee_recipients
            },
        },
        tx: RawTx {
            computeUnitLimit: Some(json!(FIXED_COMPUTE_UNIT_LIMIT)),
            computeUnitPriceMicroLamports: Some(json!(
                lamports_to_priority_fee_micro_lamports(priority_fee_lamports)
            )),
            jitoTipLamports: Some(json!(tip_lamports)),
            jitoTipAccount: if tip_lamports > 0 {
                pick_tip_account(&provider)
            } else {
                String::new()
            },
            lookupTables: vec![],
            useDefaultLookupTables: Some(json!(true)),
            dumpBase64: Some(json!(false)),
            writeReport: Some(json!(true)),
        },
        feeSharing: RawFeeSharing {
            generateLaterSetup: Some(json!(fee_split_enabled)),
            recipients: fee_sharing_recipients,
        },
        creatorFee: RawCreatorFee {
            mode: if mode == "cashback" {
                "cashback".to_string()
            } else if is_agent_complete {
                "agent-escrow".to_string()
            } else {
                "deployer".to_string()
            },
            ..RawCreatorFee::default()
        },
        execution: RawExecution {
            simulate: Some(json!(action == "simulate")),
            send: Some(json!(action == "send")),
            txFormat: "auto".to_string(),
            commitment: "confirmed".to_string(),
            skipPreflight: Some(json!(if provider == "helius-sender" {
                true
            } else {
                form.skipPreflight
            })),
            trackSendBlockHeight: Some(json!(form.trackSendBlockHeight)),
            provider: provider.clone(),
            endpointProfile: endpoint_profile.clone(),
            policy: if form.policy.trim().is_empty() {
                "safe".to_string()
            } else {
                form.policy.trim().to_string()
            },
            autoGas: Some(json!(true)),
            autoMode: "launchAuto".to_string(),
            priorityFeeSol: form.priorityFeeSol.trim().to_string(),
            tipSol: if tip_lamports > 0 {
                form.creationTipSol.trim().to_string()
            } else {
                String::new()
            },
            maxPriorityFeeSol: form.priorityFeeSol.trim().to_string(),
            maxTipSol: if tip_lamports > 0 {
                form.creationTipSol.trim().to_string()
            } else {
                String::new()
            },
            buyProvider: buy_provider,
            buyEndpointProfile: buy_endpoint_profile,
            buyPolicy: if form.buyPolicy.trim().is_empty() {
                "safe".to_string()
            } else {
                form.buyPolicy.trim().to_string()
            },
            buyAutoGas: Some(json!(true)),
            buyAutoMode: "buyAuto".to_string(),
            buyPriorityFeeSol: form.buyPriorityFeeSol.trim().to_string(),
            buyTipSol: if tip_supported(&provider) {
                form.buyTipSol.trim().to_string()
            } else {
                String::new()
            },
            buySlippagePercent: form.buySlippagePercent.trim().to_string(),
            buyMaxPriorityFeeSol: form.buyPriorityFeeSol.trim().to_string(),
            buyMaxTipSol: if tip_supported(&provider) {
                form.buyTipSol.trim().to_string()
            } else {
                String::new()
            },
            sellProvider: sell_provider,
            sellEndpointProfile: sell_endpoint_profile,
            sellPolicy: if form.sellPolicy.trim().is_empty() {
                "safe".to_string()
            } else {
                form.sellPolicy.trim().to_string()
            },
            sellPriorityFeeSol: form.sellPriorityFeeSol.trim().to_string(),
            sellTipSol: if tip_supported(&provider) {
                form.sellTipSol.trim().to_string()
            } else {
                String::new()
            },
            sellSlippagePercent: form.sellSlippagePercent.trim().to_string(),
        },
        initialBuySol: if form.devBuyMode.trim().eq_ignore_ascii_case("sol") {
            form.devBuyAmount.trim().to_string()
        } else {
            String::new()
        },
        initialBuyTokens: if form.devBuyMode.trim().eq_ignore_ascii_case("tokens") {
            form.devBuyAmount.trim().to_string()
        } else {
            String::new()
        },
        devBuy: None,
        postLaunch: RawPostLaunch {
            strategy: form.postLaunchStrategy.trim().to_string(),
            snipeOwnLaunch: RawSnipeOwnLaunch {
                buyAmountSol: form.snipeBuyAmountSol.trim().to_string(),
            },
            automaticDevSell: RawAutomaticDevSell {
                enabled: Some(json!(form.automaticDevSellEnabled)),
                percent: Some(json!(form.automaticDevSellPercent.trim())),
                delaySeconds: Some(json!(form.automaticDevSellDelaySeconds.trim())),
            },
        },
        presets: RawPresets {
            activePresetId: form.activePresetId.trim().to_string(),
            selectedLaunchPresetId: form.activePresetId.trim().to_string(),
            selectedSniperPresetId: form.activePresetId.trim().to_string(),
        },
        imageLocalPath: if image_file_name.is_empty() {
            String::new()
        } else {
            uploads_dir().join(&image_file_name).display().to_string()
        },
        selectedWalletKey: selected_wallet_key,
    };
    let metadata_uri = upload_metadata_to_pump_fun(&raw).await?;
    raw.token.uri = metadata_uri.clone();
    Ok((raw, Some(metadata_uri)))
}
