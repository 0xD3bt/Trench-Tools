#![allow(non_snake_case, dead_code)]

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address;
use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    path::PathBuf,
    str::FromStr,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Default)]
pub struct WalletRuntimeConfig {
    ata_cache_path: Option<PathBuf>,
    before_rpc_request: Option<fn()>,
}

impl WalletRuntimeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_ata_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.ata_cache_path = Some(path.into());
        self
    }

    pub fn with_before_rpc_request(mut self, hook: fn()) -> Self {
        self.before_rpc_request = Some(hook);
        self
    }
}

pub fn configure_wallet_runtime(config: WalletRuntimeConfig) {
    let _ = wallet_runtime_config().set(config);
}

fn wallet_runtime_config() -> &'static OnceLock<WalletRuntimeConfig> {
    static CONFIG: OnceLock<WalletRuntimeConfig> = OnceLock::new();
    &CONFIG
}

fn configured_ata_cache_path() -> Option<PathBuf> {
    wallet_runtime_config()
        .get()
        .and_then(|config| config.ata_cache_path.clone())
}

fn before_rpc_request_hook() -> Option<fn()> {
    wallet_runtime_config()
        .get()
        .and_then(|config| config.before_rpc_request)
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletSummary {
    pub envKey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customName: Option<String>,
    pub publicKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletStatusSummary {
    pub envKey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customName: Option<String>,
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

const TOKEN_ACCOUNT_AMOUNT_OFFSET: usize = 64;
const TOKEN_ACCOUNT_AMOUNT_LEN: usize = 8;
const USD1_DECIMALS_FACTOR: f64 = 1_000_000.0;
const MAX_MULTIPLE_ACCOUNTS_BATCH_SIZE: usize = 100;
const WALLET_ATA_CACHE_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WalletAtaCachePayload {
    #[serde(default)]
    schemaVersion: u8,
    #[serde(default)]
    entries: BTreeMap<String, String>,
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
    match bs58::decode(trimmed).into_vec() {
        Ok(bytes) => Ok(bytes),
        Err(base58_error) => BASE64.decode(trimmed).map_err(|base64_error| {
            format!("Invalid keypair encoding. Base58: {base58_error}; Base64: {base64_error}")
        }),
    }
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

fn split_wallet_secret_and_name(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    if trimmed.starts_with('[')
        && let Some(end_index) = trimmed.rfind(']')
    {
        let secret = trimmed[..=end_index].trim().to_string();
        let remainder = trimmed[end_index + 1..].trim();
        if let Some(name) = remainder.strip_prefix(',').map(str::trim) {
            return (
                secret,
                if name.is_empty() {
                    None
                } else {
                    Some(name.to_string())
                },
            );
        }
        return (secret, None);
    }
    if let Some((secret, name)) = trimmed.split_once(',') {
        let secret = secret.trim().to_string();
        let name = name.trim();
        return (
            secret,
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            },
        );
    }
    (trimmed.to_string(), None)
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
            let raw_value = env::var(&env_key).unwrap_or_default();
            let (secret, custom_name) = split_wallet_secret_and_name(&raw_value);
            match read_keypair_bytes(&secret).and_then(|bytes| public_key_from_secret_bytes(&bytes))
            {
                Ok(public_key) => WalletSummary {
                    envKey: env_key,
                    customName: custom_name,
                    publicKey: Some(public_key),
                    error: None,
                },
                Err(error) => WalletSummary {
                    envKey: env_key,
                    customName: custom_name,
                    publicKey: None,
                    error: Some(error),
                },
            }
        })
        .collect()
}

pub fn selected_wallet_key_or_default(requested_key: &str) -> Option<String> {
    selected_wallet_key_or_default_from_wallets(requested_key, &list_solana_env_wallets())
}

pub fn selected_wallet_key_or_default_from_wallets(
    requested_key: &str,
    wallets: &[WalletSummary],
) -> Option<String> {
    if !requested_key.trim().is_empty() {
        return Some(requested_key.trim().to_string());
    }
    wallets
        .iter()
        .find(|wallet| wallet.error.is_none())
        .map(|wallet| wallet.envKey.clone())
}

pub fn load_solana_wallet_by_env_key(env_key: &str) -> Result<Vec<u8>, String> {
    if !is_solana_wallet_env_key(env_key) {
        return Err(format!("Invalid Solana wallet env key: {env_key}"));
    }
    let raw_value = env::var(env_key).map_err(|_| format!("Missing env value for {env_key}"))?;
    let (secret, _) = split_wallet_secret_and_name(&raw_value);
    read_keypair_bytes(&secret)
}

fn wallet_rpc_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| format!("Failed to build wallet RPC client: {error}"))
}

fn load_wallet_ata_cache_from_disk() -> BTreeMap<String, String> {
    let Some(path) = configured_ata_cache_path() else {
        return BTreeMap::new();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(payload) = serde_json::from_str::<WalletAtaCachePayload>(&raw) else {
        return BTreeMap::new();
    };
    if payload.schemaVersion != WALLET_ATA_CACHE_SCHEMA_VERSION {
        return BTreeMap::new();
    }
    payload.entries
}

fn wallet_ata_cache_store() -> &'static Mutex<BTreeMap<String, String>> {
    static STORE: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(load_wallet_ata_cache_from_disk()))
}

fn wallet_ata_cache_key(owner: &str, mint: &str) -> String {
    format!("{owner}:{mint}")
}

fn write_wallet_cache_bytes(path: &PathBuf, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let temp_path = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| format!("{value}."))
            .unwrap_or_default()
    ));
    if fs::write(&temp_path, bytes).is_err() {
        return;
    }
    if fs::rename(&temp_path, path).is_err() {
        let _ = fs::remove_file(path);
        let _ = fs::rename(&temp_path, path);
    }
    let _ = fs::remove_file(&temp_path);
}

fn persist_wallet_ata_cache(entries: &BTreeMap<String, String>) {
    let Some(path) = configured_ata_cache_path() else {
        return;
    };
    let payload = WalletAtaCachePayload {
        schemaVersion: WALLET_ATA_CACHE_SCHEMA_VERSION,
        entries: entries.clone(),
    };
    let Ok(serialized) = serde_json::to_vec_pretty(&payload) else {
        return;
    };
    write_wallet_cache_bytes(&path, &serialized);
}

fn resolve_cached_associated_token_accounts(
    owners: &[String],
    mint: &Pubkey,
) -> Result<Vec<String>, String> {
    let mint_string = mint.to_string();
    let cache = wallet_ata_cache_store();
    let mut guard = match cache.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let mut changed = false;
    let mut addresses = Vec::with_capacity(owners.len());
    for owner_string in owners {
        let cache_key = wallet_ata_cache_key(owner_string, &mint_string);
        if let Some(cached) = guard.get(&cache_key) {
            addresses.push(cached.clone());
            continue;
        }
        let owner = Pubkey::from_str(owner_string)
            .map_err(|error| format!("Invalid wallet public key {owner_string}: {error}"))?;
        let ata = get_associated_token_address(&owner, mint).to_string();
        guard.insert(cache_key, ata.clone());
        addresses.push(ata);
        changed = true;
    }
    if changed {
        persist_wallet_ata_cache(&guard);
    }
    Ok(addresses)
}

async fn rpc_request(
    client: &Client,
    rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    if let Some(hook) = before_rpc_request_hook() {
        hook();
    }
    let response = client
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

async fn fetch_balance_lamports_with_client(
    client: &Client,
    rpc_url: &str,
    public_key: &str,
) -> Result<u64, String> {
    rpc_request(
        client,
        rpc_url,
        "getBalance",
        serde_json::json!([public_key, { "commitment": "confirmed" }]),
    )
    .await?
    .get("value")
    .and_then(Value::as_u64)
    .ok_or_else(|| format!("RPC getBalance did not return a numeric balance for {public_key}."))
}

pub async fn fetch_balance_lamports(rpc_url: &str, public_key: &str) -> Result<u64, String> {
    let client = wallet_rpc_client()?;
    fetch_balance_lamports_with_client(&client, rpc_url, public_key).await
}

async fn fetch_token_balance_with_client(
    client: &Client,
    rpc_url: &str,
    public_key: &str,
    mint: &str,
    commitment: &str,
) -> Result<f64, String> {
    let result = rpc_request(
        client,
        rpc_url,
        "getTokenAccountsByOwner",
        serde_json::json!([
            public_key,
            { "mint": mint },
            { "encoding": "jsonParsed", "commitment": commitment }
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

pub async fn fetch_token_balance(
    rpc_url: &str,
    public_key: &str,
    mint: &str,
    commitment: &str,
) -> Result<f64, String> {
    let client = wallet_rpc_client()?;
    fetch_token_balance_with_client(&client, rpc_url, public_key, mint, commitment).await
}

fn wallet_status_without_balance(
    wallet: &WalletSummary,
    balance_error: Option<String>,
) -> WalletStatusSummary {
    WalletStatusSummary {
        envKey: wallet.envKey.clone(),
        customName: wallet.customName.clone(),
        publicKey: wallet.publicKey.clone(),
        error: wallet.error.clone(),
        balanceLamports: None,
        balanceSol: None,
        usd1Balance: None,
        balanceError: balance_error,
    }
}

async fn fetch_multiple_balance_lamports_with_client(
    client: &Client,
    rpc_url: &str,
    accounts: &[String],
    commitment: &str,
) -> Result<Vec<Option<u64>>, String> {
    if accounts.is_empty() {
        return Ok(vec![]);
    }
    let mut combined = Vec::with_capacity(accounts.len());
    for account_chunk in accounts.chunks(MAX_MULTIPLE_ACCOUNTS_BATCH_SIZE) {
        let result = rpc_request(
            client,
            rpc_url,
            "getMultipleAccounts",
            serde_json::json!([
                account_chunk,
                {
                    "encoding": "base64",
                    "commitment": commitment,
                    "dataSlice": {
                        "offset": 0,
                        "length": 0
                    }
                }
            ]),
        )
        .await?;
        let values = result
            .get("value")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| "RPC getMultipleAccounts did not return a value array.".to_string())?;
        if values.len() != account_chunk.len() {
            return Err(format!(
                "RPC getMultipleAccounts returned {} entries for {} requested accounts.",
                values.len(),
                account_chunk.len()
            ));
        }
        let parsed_chunk = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                if value.is_null() {
                    return Ok(None);
                }
                value
                    .get("lamports")
                    .and_then(Value::as_u64)
                    .map(Some)
                    .ok_or_else(|| {
                        format!(
                            "RPC getMultipleAccounts did not return lamports for {}.",
                            account_chunk[index]
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        combined.extend(parsed_chunk);
    }
    Ok(combined)
}

async fn fetch_multiple_account_data_with_client(
    client: &Client,
    rpc_url: &str,
    accounts: &[String],
    commitment: &str,
) -> Result<Vec<Option<Vec<u8>>>, String> {
    if accounts.is_empty() {
        return Ok(vec![]);
    }
    let mut combined = Vec::with_capacity(accounts.len());
    for account_chunk in accounts.chunks(MAX_MULTIPLE_ACCOUNTS_BATCH_SIZE) {
        let result = rpc_request(
            client,
            rpc_url,
            "getMultipleAccounts",
            serde_json::json!([
                account_chunk,
                {
                    "encoding": "base64",
                    "commitment": commitment,
                }
            ]),
        )
        .await?;
        let values = result
            .get("value")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| "RPC getMultipleAccounts did not return a value array.".to_string())?;
        if values.len() != account_chunk.len() {
            return Err(format!(
                "RPC getMultipleAccounts returned {} entries for {} requested accounts.",
                values.len(),
                account_chunk.len()
            ));
        }
        let parsed_chunk = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                if value.is_null() {
                    return Ok(None);
                }
                let data = value
                    .get("data")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        format!(
                            "RPC getMultipleAccounts returned invalid base64 data for {}.",
                            account_chunk[index]
                        )
                    })?;
                BASE64
                    .decode(data)
                    .map(Some)
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        combined.extend(parsed_chunk);
    }
    Ok(combined)
}

fn parse_token_account_raw_balance(data: &[u8]) -> Result<u64, String> {
    let end = TOKEN_ACCOUNT_AMOUNT_OFFSET + TOKEN_ACCOUNT_AMOUNT_LEN;
    if data.len() < end {
        return Err("Token account data was too short to contain a token amount.".to_string());
    }
    let amount_bytes: [u8; TOKEN_ACCOUNT_AMOUNT_LEN] = data[TOKEN_ACCOUNT_AMOUNT_OFFSET..end]
        .try_into()
        .map_err(|_| "Token account amount bytes were malformed.".to_string())?;
    Ok(u64::from_le_bytes(amount_bytes))
}

async fn enrich_wallet_statuses_individual(
    client: &Client,
    rpc_url: &str,
    usd1_mint: &str,
    wallets: &[WalletSummary],
    include_sol_balance: bool,
    include_usd1_balance: bool,
) -> Vec<WalletStatusSummary> {
    let tasks = wallets.iter().cloned().map(|wallet| {
        let client = client.clone();
        let rpc_url = rpc_url.to_string();
        let usd1_mint = usd1_mint.to_string();
        async move {
            if wallet.error.is_some() || wallet.publicKey.is_none() {
                return wallet_status_without_balance(&wallet, None);
            }
            let public_key = wallet.publicKey.clone().unwrap_or_default();
            let balance_fetch = async {
                if include_sol_balance {
                    fetch_balance_lamports_with_client(&client, &rpc_url, &public_key)
                        .await
                        .map(Some)
                } else {
                    Ok(None)
                }
            };
            let token_fetch = async {
                if include_usd1_balance {
                    fetch_token_balance_with_client(
                        &client,
                        &rpc_url,
                        &public_key,
                        &usd1_mint,
                        "confirmed",
                    )
                    .await
                    .map(Some)
                } else {
                    Ok(None)
                }
            };
            let (balance_result, token_result) = tokio::join!(balance_fetch, token_fetch);
            match (balance_result, token_result) {
                (Ok(balance_lamports), Ok(usd1_balance)) => WalletStatusSummary {
                    envKey: wallet.envKey.clone(),
                    customName: wallet.customName.clone(),
                    publicKey: wallet.publicKey.clone(),
                    error: wallet.error.clone(),
                    balanceLamports: balance_lamports,
                    balanceSol: balance_lamports.map(|lamports| lamports as f64 / 1_000_000_000.0),
                    usd1Balance: usd1_balance,
                    balanceError: None,
                },
                (balance_result, token_result) => {
                    let balance_error = balance_result
                        .err()
                        .or_else(|| token_result.err())
                        .unwrap_or_else(|| "Unknown wallet balance error.".to_string());
                    wallet_status_without_balance(&wallet, Some(balance_error))
                }
            }
        }
    });
    join_all(tasks).await
}

async fn enrich_wallet_statuses_batched(
    client: &Client,
    rpc_url: &str,
    usd1_mint: &str,
    wallets: &[WalletSummary],
    include_sol_balance: bool,
    include_usd1_balance: bool,
) -> Result<Vec<WalletStatusSummary>, String> {
    let mut results = wallets
        .iter()
        .map(|wallet| wallet_status_without_balance(wallet, None))
        .collect::<Vec<_>>();
    let mut valid_indices = Vec::new();
    let mut public_keys = Vec::new();
    for (index, wallet) in wallets.iter().enumerate() {
        if wallet.error.is_some() {
            continue;
        }
        let Some(public_key) = wallet.publicKey.as_ref() else {
            continue;
        };
        valid_indices.push(index);
        public_keys.push(public_key.clone());
    }
    if valid_indices.is_empty() {
        return Ok(results);
    }
    let (balances, usd1_accounts) = match (include_sol_balance, include_usd1_balance) {
        (true, true) => {
            let usd1_mint_pubkey = Pubkey::from_str(usd1_mint)
                .map_err(|error| format!("Invalid USD1 mint {usd1_mint}: {error}"))?;
            let usd1_ata_accounts =
                resolve_cached_associated_token_accounts(&public_keys, &usd1_mint_pubkey)?;
            let (balance_result, usd1_accounts_result) = tokio::join!(
                fetch_multiple_balance_lamports_with_client(
                    client,
                    rpc_url,
                    &public_keys,
                    "confirmed"
                ),
                fetch_multiple_account_data_with_client(
                    client,
                    rpc_url,
                    &usd1_ata_accounts,
                    "confirmed"
                ),
            );
            (balance_result?, usd1_accounts_result?)
        }
        (true, false) => (
            fetch_multiple_balance_lamports_with_client(client, rpc_url, &public_keys, "confirmed")
                .await?,
            vec![None; valid_indices.len()],
        ),
        (false, true) => {
            let usd1_mint_pubkey = Pubkey::from_str(usd1_mint)
                .map_err(|error| format!("Invalid USD1 mint {usd1_mint}: {error}"))?;
            let usd1_ata_accounts =
                resolve_cached_associated_token_accounts(&public_keys, &usd1_mint_pubkey)?;
            (
                vec![None; valid_indices.len()],
                fetch_multiple_account_data_with_client(
                    client,
                    rpc_url,
                    &usd1_ata_accounts,
                    "confirmed",
                )
                .await?,
            )
        }
        (false, false) => (
            vec![None; valid_indices.len()],
            vec![None; valid_indices.len()],
        ),
    };
    if balances.len() != valid_indices.len() || usd1_accounts.len() != valid_indices.len() {
        return Err("Batched wallet balance results did not match the wallet count.".to_string());
    }
    let mut fallback_wallet_positions = Vec::new();
    for (position, wallet_index) in valid_indices.iter().copied().enumerate() {
        let wallet = &wallets[wallet_index];
        let balance_lamports = balances[position];
        let usd1_balance = if include_usd1_balance {
            match usd1_accounts[position].as_ref() {
                Some(account_data) => match parse_token_account_raw_balance(account_data) {
                    Ok(amount) => Some(amount as f64 / USD1_DECIMALS_FACTOR),
                    Err(_error) => {
                        fallback_wallet_positions.push(position);
                        Some(0.0)
                    }
                },
                None => {
                    fallback_wallet_positions.push(position);
                    Some(0.0)
                }
            }
        } else {
            None
        };
        results[wallet_index] = WalletStatusSummary {
            envKey: wallet.envKey.clone(),
            customName: wallet.customName.clone(),
            publicKey: wallet.publicKey.clone(),
            error: wallet.error.clone(),
            balanceLamports: balance_lamports,
            balanceSol: balance_lamports.map(|lamports| lamports as f64 / 1_000_000_000.0),
            usd1Balance: usd1_balance,
            balanceError: None,
        };
    }
    if !fallback_wallet_positions.is_empty() {
        let fallback_tasks = fallback_wallet_positions.iter().copied().map(|position| {
            let client = client.clone();
            let rpc_url = rpc_url.to_string();
            let public_key = public_keys[position].clone();
            let usd1_mint = usd1_mint.to_string();
            async move {
                (
                    position,
                    fetch_token_balance_with_client(
                        &client,
                        &rpc_url,
                        &public_key,
                        &usd1_mint,
                        "confirmed",
                    )
                    .await,
                )
            }
        });
        for (position, fallback_result) in join_all(fallback_tasks).await {
            let wallet_index = valid_indices[position];
            match fallback_result {
                Ok(usd1_balance) => {
                    results[wallet_index].usd1Balance = Some(usd1_balance);
                }
                Err(error) => {
                    results[wallet_index].usd1Balance = None;
                    results[wallet_index].balanceError = Some(error);
                }
            }
        }
    }
    Ok(results)
}

const WALLET_BALANCE_CACHE_TTL_MS: u128 = 2000;

#[derive(Clone)]
struct CachedBalance {
    status: WalletStatusSummary,
    cached_at_ms: u128,
}

static WALLET_BALANCE_CACHE: OnceLock<Mutex<HashMap<String, CachedBalance>>> = OnceLock::new();

fn wallet_balance_cache() -> &'static Mutex<HashMap<String, CachedBalance>> {
    WALLET_BALANCE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn wallet_balance_cache_now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn wallet_balance_cache_key(
    rpc_url: &str,
    usd1_mint: &str,
    env_key: &str,
    include_sol_balance: bool,
    include_usd1_balance: bool,
) -> String {
    format!("{rpc_url}|{usd1_mint}|sol={include_sol_balance}|usd1={include_usd1_balance}|{env_key}")
}

fn wallet_balance_cache_get(
    rpc_url: &str,
    usd1_mint: &str,
    env_key: &str,
    include_sol_balance: bool,
    include_usd1_balance: bool,
    now_ms: u128,
) -> Option<WalletStatusSummary> {
    let guard = wallet_balance_cache().lock().ok()?;
    let entry = guard.get(&wallet_balance_cache_key(
        rpc_url,
        usd1_mint,
        env_key,
        include_sol_balance,
        include_usd1_balance,
    ))?;
    if now_ms.saturating_sub(entry.cached_at_ms) > WALLET_BALANCE_CACHE_TTL_MS {
        return None;
    }
    Some(entry.status.clone())
}

fn wallet_balance_cache_put(
    rpc_url: &str,
    usd1_mint: &str,
    status: &WalletStatusSummary,
    include_sol_balance: bool,
    include_usd1_balance: bool,
    now_ms: u128,
) {
    // Only cache successful balance fetches; transient errors should be retried on the next call.
    if status.balanceError.is_some() {
        return;
    }
    if status.balanceLamports.is_none() && status.usd1Balance.is_none() {
        return;
    }
    if let Ok(mut guard) = wallet_balance_cache().lock() {
        guard.insert(
            wallet_balance_cache_key(
                rpc_url,
                usd1_mint,
                &status.envKey,
                include_sol_balance,
                include_usd1_balance,
            ),
            CachedBalance {
                status: status.clone(),
                cached_at_ms: now_ms,
            },
        );
    }
}

/// Invalidate cached balances for specific env keys (pass an empty slice to clear everything).
pub fn invalidate_wallet_balance_cache(env_keys: &[String]) {
    let Ok(mut guard) = wallet_balance_cache().lock() else {
        return;
    };
    if env_keys.is_empty() {
        guard.clear();
        return;
    }
    let suffixes: Vec<String> = env_keys.iter().map(|k| format!("|{k}")).collect();
    guard.retain(|key, _| !suffixes.iter().any(|suffix| key.ends_with(suffix)));
}

pub async fn enrich_wallet_statuses(
    rpc_url: &str,
    usd1_mint: &str,
    wallets: &[WalletSummary],
) -> Vec<WalletStatusSummary> {
    enrich_wallet_statuses_with_options(rpc_url, usd1_mint, wallets, false).await
}

pub async fn enrich_wallet_statuses_with_options(
    rpc_url: &str,
    usd1_mint: &str,
    wallets: &[WalletSummary],
    force_refresh: bool,
) -> Vec<WalletStatusSummary> {
    enrich_wallet_statuses_with_balance_options(
        rpc_url,
        usd1_mint,
        wallets,
        force_refresh,
        true,
        true,
    )
    .await
}

pub async fn enrich_wallet_statuses_with_balance_options(
    rpc_url: &str,
    usd1_mint: &str,
    wallets: &[WalletSummary],
    force_refresh: bool,
    include_sol_balance: bool,
    include_usd1_balance: bool,
) -> Vec<WalletStatusSummary> {
    if wallets.is_empty() {
        return Vec::new();
    }

    if !include_sol_balance && !include_usd1_balance {
        return wallets
            .iter()
            .map(|wallet| wallet_status_without_balance(wallet, None))
            .collect();
    }

    let now_ms = wallet_balance_cache_now_ms();
    let mut results: Vec<Option<WalletStatusSummary>> = vec![None; wallets.len()];
    let mut wallets_to_fetch: Vec<WalletSummary> = Vec::new();
    let mut fetch_result_indices: Vec<usize> = Vec::new();

    for (index, wallet) in wallets.iter().enumerate() {
        if !force_refresh
            && let Some(cached) = wallet_balance_cache_get(
                rpc_url,
                usd1_mint,
                &wallet.envKey,
                include_sol_balance,
                include_usd1_balance,
                now_ms,
            )
        {
            results[index] = Some(cached);
            continue;
        }
        wallets_to_fetch.push(wallet.clone());
        fetch_result_indices.push(index);
    }

    if wallets_to_fetch.is_empty() {
        return results.into_iter().map(|status| status.unwrap()).collect();
    }

    let client = match wallet_rpc_client() {
        Ok(client) => client,
        Err(error) => {
            for (local_index, original_index) in fetch_result_indices.iter().enumerate() {
                let wallet = &wallets_to_fetch[local_index];
                results[*original_index] = Some(WalletStatusSummary {
                    envKey: wallet.envKey.clone(),
                    customName: wallet.customName.clone(),
                    publicKey: wallet.publicKey.clone(),
                    error: wallet.error.clone(),
                    balanceLamports: None,
                    balanceSol: None,
                    usd1Balance: None,
                    balanceError: if wallet.error.is_some() {
                        None
                    } else {
                        Some(error.clone())
                    },
                });
            }
            return results.into_iter().map(|status| status.unwrap()).collect();
        }
    };

    let fresh = match enrich_wallet_statuses_batched(
        &client,
        rpc_url,
        usd1_mint,
        &wallets_to_fetch,
        include_sol_balance,
        include_usd1_balance,
    )
    .await
    {
        Ok(wallets) => wallets,
        Err(_error) => {
            enrich_wallet_statuses_individual(
                &client,
                rpc_url,
                usd1_mint,
                &wallets_to_fetch,
                include_sol_balance,
                include_usd1_balance,
            )
            .await
        }
    };

    let fresh_now_ms = wallet_balance_cache_now_ms();
    for (local_index, original_index) in fetch_result_indices.iter().enumerate() {
        let status = fresh
            .get(local_index)
            .cloned()
            .unwrap_or_else(|| wallet_status_without_balance(&wallets_to_fetch[local_index], None));
        wallet_balance_cache_put(
            rpc_url,
            usd1_mint,
            &status,
            include_sol_balance,
            include_usd1_balance,
            fresh_now_ms,
        );
        results[*original_index] = Some(status);
    }

    results.into_iter().map(|status| status.unwrap()).collect()
}

#[cfg(test)]
mod tests {
    use super::read_keypair_bytes;
    use base64::Engine as _;

    #[test]
    fn read_keypair_bytes_accepts_base64_secret() {
        let secret = vec![7u8; 64];
        let encoded = base64::engine::general_purpose::STANDARD.encode(&secret);
        let decoded = read_keypair_bytes(&encoded).expect("base64 secret should decode");
        assert_eq!(decoded, secret);
    }
}
