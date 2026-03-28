#![allow(non_snake_case, dead_code)]

use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::env;

#[derive(Debug, Clone, Serialize)]
pub struct WalletSummary {
    pub envKey: String,
    pub publicKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletStatusSummary {
    pub envKey: String,
    pub publicKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balanceLamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balanceSol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usd1Balance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balanceError: Option<String>,
}

pub fn is_solana_wallet_env_key(key: &str) -> bool {
    let key = key.trim();
    key == "SOLANA_PRIVATE_KEY"
        || (key.starts_with("SOLANA_PRIVATE_KEY")
            && key["SOLANA_PRIVATE_KEY".len()..]
                .chars()
                .all(|c| c.is_ascii_digit()))
}

pub fn read_keypair_bytes(raw: &str) -> Result<Vec<u8>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Keypair value was empty.".to_string());
    }
    if trimmed.starts_with('[') {
        let parsed: Value = serde_json::from_str(trimmed).map_err(|error| error.to_string())?;
        let array = parsed
            .as_array()
            .ok_or_else(|| "Keypair JSON must be an array of bytes.".to_string())?;
        let mut bytes = Vec::with_capacity(array.len());
        for item in array {
            let byte = item
                .as_u64()
                .ok_or_else(|| "Keypair byte array contained a non-integer value.".to_string())?;
            if byte > 255 {
                return Err("Keypair byte array contained a value above 255.".to_string());
            }
            bytes.push(byte as u8);
        }
        return Ok(bytes);
    }
    bs58::decode(trimmed)
        .into_vec()
        .map_err(|error| error.to_string())
}

fn public_key_from_secret_bytes(bytes: &[u8]) -> Result<String, String> {
    match bytes.len() {
        64 => Ok(bs58::encode(&bytes[32..64]).into_string()),
        32 => {
            Err("32-byte private keys are not yet supported by the Rust wallet parser.".to_string())
        }
        other => Err(format!("Unsupported keypair length: {other} bytes.")),
    }
}

pub fn public_key_from_secret(bytes: &[u8]) -> Result<String, String> {
    public_key_from_secret_bytes(bytes)
}

pub fn list_solana_env_wallets() -> Vec<WalletSummary> {
    let mut keys: Vec<String> = env::vars()
        .map(|(key, _)| key)
        .filter(|key| is_solana_wallet_env_key(key))
        .collect();
    keys.sort_by_key(|key| {
        key.strip_prefix("SOLANA_PRIVATE_KEY")
            .and_then(|suffix| {
                if suffix.is_empty() {
                    Some(1)
                } else {
                    suffix.parse::<usize>().ok()
                }
            })
            .unwrap_or(usize::MAX)
    });
    keys.into_iter()
        .map(|env_key| {
            let secret = env::var(&env_key).unwrap_or_default();
            match read_keypair_bytes(&secret).and_then(|bytes| public_key_from_secret_bytes(&bytes))
            {
                Ok(public_key) => WalletSummary {
                    envKey: env_key,
                    publicKey: Some(public_key),
                    error: None,
                },
                Err(error) => WalletSummary {
                    envKey: env_key,
                    publicKey: None,
                    error: Some(error),
                },
            }
        })
        .collect()
}

pub fn selected_wallet_key_or_default(requested_key: &str) -> Option<String> {
    if !requested_key.trim().is_empty() {
        return Some(requested_key.trim().to_string());
    }
    list_solana_env_wallets()
        .into_iter()
        .find(|wallet| wallet.error.is_none())
        .map(|wallet| wallet.envKey)
}

pub fn load_solana_wallet_by_env_key(env_key: &str) -> Result<Vec<u8>, String> {
    if !is_solana_wallet_env_key(env_key) {
        return Err(format!("Invalid Solana wallet env key: {env_key}"));
    }
    let secret = env::var(env_key).map_err(|_| format!("Missing env value for {env_key}"))?;
    read_keypair_bytes(&secret)
}

async fn rpc_request(rpc_url: &str, method: &str, params: Value) -> Result<Value, String> {
    let response = Client::new()
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .map_err(|error| format!("RPC {method} request failed: {error}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let payload: Value = serde_json::from_str(&body)
        .map_err(|error| format!("RPC {method} returned invalid JSON: {error}"))?;
    if !status.is_success() {
        return Err(format!("RPC {method} failed with status {status}: {body}"));
    }
    if let Some(message) = payload
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
    {
        return Err(format!("RPC {method} failed: {message}"));
    }
    payload
        .get("result")
        .cloned()
        .ok_or_else(|| format!("RPC {method} did not return a result."))
}

async fn fetch_balance_lamports(rpc_url: &str, public_key: &str) -> Result<u64, String> {
    rpc_request(
        rpc_url,
        "getBalance",
        serde_json::json!([public_key, "confirmed"]),
    )
    .await?
    .get("value")
    .and_then(Value::as_u64)
    .ok_or_else(|| format!("RPC getBalance did not return a numeric balance for {public_key}."))
}

async fn fetch_token_balance(rpc_url: &str, public_key: &str, mint: &str) -> Result<f64, String> {
    let result = rpc_request(
        rpc_url,
        "getTokenAccountsByOwner",
        serde_json::json!([
            public_key,
            { "mint": mint },
            { "encoding": "jsonParsed", "commitment": "confirmed" }
        ]),
    )
    .await?;
    let accounts = result
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "RPC getTokenAccountsByOwner returned invalid account data.".to_string())?;
    Ok(accounts.iter().fold(0.0, |sum, entry| {
        let token_amount = entry
            .get("account")
            .and_then(|value| value.get("data"))
            .and_then(|value| value.get("parsed"))
            .and_then(|value| value.get("info"))
            .and_then(|value| value.get("tokenAmount"));
        let ui_amount_string = token_amount
            .and_then(|value| value.get("uiAmountString"))
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<f64>().ok());
        let ui_amount = token_amount
            .and_then(|value| value.get("uiAmount"))
            .and_then(Value::as_f64);
        sum + ui_amount_string.or(ui_amount).unwrap_or(0.0)
    }))
}

pub async fn enrich_wallet_statuses(
    rpc_url: &str,
    usd1_mint: &str,
    wallets: &[WalletSummary],
) -> Vec<WalletStatusSummary> {
    let mut enriched = Vec::with_capacity(wallets.len());
    for wallet in wallets {
        if wallet.error.is_some() || wallet.publicKey.is_none() {
            enriched.push(WalletStatusSummary {
                envKey: wallet.envKey.clone(),
                publicKey: wallet.publicKey.clone(),
                error: wallet.error.clone(),
                balanceLamports: None,
                balanceSol: None,
                usd1Balance: None,
                balanceError: None,
            });
            continue;
        }
        let public_key = wallet.publicKey.clone().unwrap_or_default();
        match (
            fetch_balance_lamports(rpc_url, &public_key).await,
            fetch_token_balance(rpc_url, &public_key, usd1_mint).await,
        ) {
            (Ok(balance_lamports), Ok(usd1_balance)) => enriched.push(WalletStatusSummary {
                envKey: wallet.envKey.clone(),
                publicKey: wallet.publicKey.clone(),
                error: wallet.error.clone(),
                balanceLamports: Some(balance_lamports),
                balanceSol: Some(balance_lamports as f64 / 1_000_000_000.0),
                usd1Balance: Some(usd1_balance),
                balanceError: None,
            }),
            (balance_result, token_result) => {
                let balance_error = balance_result
                    .err()
                    .or_else(|| token_result.err())
                    .unwrap_or_else(|| "Unknown wallet balance error.".to_string());
                enriched.push(WalletStatusSummary {
                    envKey: wallet.envKey.clone(),
                    publicKey: wallet.publicKey.clone(),
                    error: wallet.error.clone(),
                    balanceLamports: None,
                    balanceSol: None,
                    usd1Balance: None,
                    balanceError: Some(balance_error),
                });
            }
        }
    }
    enriched
}
