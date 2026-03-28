#![allow(non_snake_case, dead_code)]

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
