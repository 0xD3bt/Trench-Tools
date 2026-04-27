use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use solana_sdk::signature::{Keypair, Signer};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, RwLock},
    time::UNIX_EPOCH,
};

const DEFAULT_RPC_URL: &str = "http://127.0.0.1:8899";
const SHARED_ENV_PATH_ENV_KEY: &str = "EXECUTION_ENGINE_SHARED_ENV_PATH";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedRpcConfig {
    pub rpc_url: String,
    pub ws_url: String,
    pub warm_rpc_url: String,
    pub shared_region: String,
    pub helius_rpc_url: String,
    pub helius_ws_url: String,
    #[serde(default)]
    pub standard_rpc_send_urls: Vec<String>,
    pub helius_sender_region: String,
}

#[derive(Debug, Clone)]
pub struct SharedWalletEntry {
    pub key: String,
    pub label: String,
    pub public_key: String,
    pub secret: String,
}

#[derive(Debug, Clone)]
pub struct SharedConfigSnapshot {
    pub env_path: PathBuf,
    pub env_modified_unix_ms: u128,
    pub values: BTreeMap<String, String>,
    pub wallets: Vec<SharedWalletEntry>,
    pub rpc: SharedRpcConfig,
}

pub struct SharedConfigManager {
    env_path: PathBuf,
    snapshot: RwLock<SharedConfigSnapshot>,
    write_lock: Mutex<()>,
}

pub fn shared_env_path() -> PathBuf {
    if let Ok(explicit) = std::env::var(SHARED_ENV_PATH_ENV_KEY) {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".env")
}

pub fn shared_config_manager() -> &'static SharedConfigManager {
    static STORE: OnceLock<SharedConfigManager> = OnceLock::new();
    STORE.get_or_init(SharedConfigManager::new)
}

impl SharedConfigManager {
    pub fn new() -> Self {
        let env_path = shared_env_path();
        let snapshot =
            load_snapshot_from_path(&env_path).unwrap_or_else(|_| SharedConfigSnapshot {
                env_path: env_path.clone(),
                env_modified_unix_ms: 0,
                values: BTreeMap::new(),
                wallets: Vec::new(),
                rpc: SharedRpcConfig {
                    rpc_url: DEFAULT_RPC_URL.to_string(),
                    ws_url: String::new(),
                    warm_rpc_url: DEFAULT_RPC_URL.to_string(),
                    shared_region: String::new(),
                    helius_rpc_url: String::new(),
                    helius_ws_url: String::new(),
                    standard_rpc_send_urls: Vec::new(),
                    helius_sender_region: String::new(),
                },
            });
        Self {
            env_path,
            snapshot: RwLock::new(snapshot),
            write_lock: Mutex::new(()),
        }
    }

    pub fn env_path(&self) -> PathBuf {
        self.env_path.clone()
    }

    pub fn current_snapshot(&self) -> SharedConfigSnapshot {
        let _ = self.refresh_if_needed();
        self.snapshot.read().unwrap().clone()
    }

    pub fn refresh_if_needed(&self) -> Result<SharedConfigSnapshot, String> {
        let current_modified = modified_unix_ms(&self.env_path).unwrap_or(0);
        let known_modified = self.snapshot.read().unwrap().env_modified_unix_ms;
        if current_modified == known_modified {
            return Ok(self.snapshot.read().unwrap().clone());
        }
        self.force_reload()
    }

    pub fn force_reload(&self) -> Result<SharedConfigSnapshot, String> {
        let snapshot = load_snapshot_from_path(&self.env_path)?;
        *self.snapshot.write().unwrap() = snapshot.clone();
        Ok(snapshot)
    }

    pub fn wallet_secret(&self, wallet_key: &str) -> Result<String, String> {
        self.current_snapshot()
            .wallets
            .into_iter()
            .find(|wallet| wallet.key == wallet_key)
            .map(|wallet| wallet.secret)
            .ok_or_else(|| format!("Missing shared wallet secret for {wallet_key}"))
    }

    pub fn wallet_keypair(&self, wallet_key: &str) -> Result<Keypair, String> {
        let secret = self.wallet_secret(wallet_key)?;
        let bytes = read_keypair_bytes(&secret)?;
        Keypair::try_from(bytes.as_slice())
            .map_err(|error| format!("Invalid wallet secret for {wallet_key}: {error}"))
    }

    pub fn create_wallet(&self, secret: &str, label: &str) -> Result<SharedWalletEntry, String> {
        // Recover from poisoning rather than hard-fail: the write lock
        // serializes mutations but doesn't carry invariants a panicked
        // writer could have corrupted. Pushing through on a poisoned
        // lock keeps the host alive through transient panic events.
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let latest = self.force_reload()?;
        let normalized_secret = secret.trim().to_string();
        let normalized_label = trimmed_or_empty(label);
        let public_key = public_key_from_secret(&normalized_secret)?;
        let wallet_key = next_wallet_env_key(latest.values.keys().map(String::as_str));
        let mut updates = BTreeMap::new();
        updates.insert(
            wallet_key.clone(),
            Some(join_wallet_secret_and_name(
                &normalized_secret,
                &normalized_label,
            )),
        );
        let updated = self.write_updates(&updates)?;
        updated
            .wallets
            .into_iter()
            .find(|wallet| wallet.key == wallet_key)
            .or_else(|| {
                Some(SharedWalletEntry {
                    key: wallet_key,
                    label: normalized_label,
                    public_key,
                    secret: normalized_secret,
                })
            })
            .ok_or_else(|| "Wallet was created but could not be reloaded.".to_string())
    }

    pub fn update_wallet(
        &self,
        wallet_key: &str,
        next_secret: Option<&str>,
        next_label: Option<&str>,
    ) -> Result<SharedWalletEntry, String> {
        // Recover from poisoning rather than hard-fail: the write lock
        // serializes mutations but doesn't carry invariants a panicked
        // writer could have corrupted. Pushing through on a poisoned
        // lock keeps the host alive through transient panic events.
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let latest = self.force_reload()?;
        let existing = latest
            .wallets
            .iter()
            .find(|wallet| wallet.key == wallet_key)
            .cloned()
            .ok_or_else(|| format!("Unknown wallet {wallet_key}"))?;
        let normalized_secret = next_secret
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or(existing.secret);
        let normalized_label = next_label.map(trimmed_or_empty).unwrap_or(existing.label);
        let mut updates = BTreeMap::new();
        updates.insert(
            wallet_key.to_string(),
            Some(join_wallet_secret_and_name(
                &normalized_secret,
                &normalized_label,
            )),
        );
        let updated = self.write_updates(&updates)?;
        updated
            .wallets
            .into_iter()
            .find(|wallet| wallet.key == wallet_key)
            .ok_or_else(|| format!("Wallet {wallet_key} disappeared after update"))
    }

    pub fn delete_wallet(&self, wallet_key: &str) -> Result<(), String> {
        // Recover from poisoning rather than hard-fail: the write lock
        // serializes mutations but doesn't carry invariants a panicked
        // writer could have corrupted. Pushing through on a poisoned
        // lock keeps the host alive through transient panic events.
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let latest = self.force_reload()?;
        if !latest.wallets.iter().any(|wallet| wallet.key == wallet_key) {
            return Err(format!("Unknown wallet {wallet_key}"));
        }
        let mut updates = BTreeMap::new();
        updates.insert(wallet_key.to_string(), None);
        self.write_updates(&updates)?;
        Ok(())
    }

    pub fn update_rpc_config(&self, rpc: &SharedRpcConfig) -> Result<SharedConfigSnapshot, String> {
        // Recover from poisoning rather than hard-fail: the write lock
        // serializes mutations but doesn't carry invariants a panicked
        // writer could have corrupted. Pushing through on a poisoned
        // lock keeps the host alive through transient panic events.
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut updates = BTreeMap::new();
        updates.insert(
            "SOLANA_RPC_URL".to_string(),
            optional_env_value(&rpc.rpc_url),
        );
        updates.insert("SOLANA_WS_URL".to_string(), optional_env_value(&rpc.ws_url));
        updates.insert(
            "WARM_RPC_URL".to_string(),
            optional_env_value(&rpc.warm_rpc_url),
        );
        updates.insert(
            "USER_REGION".to_string(),
            optional_env_value(&rpc.shared_region),
        );
        updates.insert(
            "HELIUS_RPC_URL".to_string(),
            optional_env_value(&rpc.helius_rpc_url),
        );
        updates.insert(
            "HELIUS_WS_URL".to_string(),
            optional_env_value(&rpc.helius_ws_url),
        );
        updates.insert(
            "LAUNCHDECK_EXTRA_STANDARD_RPC_SEND_URLS".to_string(),
            optional_env_value(&rpc.standard_rpc_send_urls.join(",")),
        );
        updates.insert(
            "USER_REGION_HELIUS_SENDER".to_string(),
            optional_env_value(&rpc.helius_sender_region),
        );
        self.write_updates(&updates)
    }

    fn write_updates(
        &self,
        updates: &BTreeMap<String, Option<String>>,
    ) -> Result<SharedConfigSnapshot, String> {
        let latest_contents = fs::read_to_string(&self.env_path).unwrap_or_default();
        let next_contents = apply_env_updates(&latest_contents, updates);
        atomic_write(&self.env_path, next_contents.as_bytes())?;
        self.force_reload()
    }
}

pub fn is_solana_wallet_env_key(key: &str) -> bool {
    let key = key.trim();
    key == "SOLANA_PRIVATE_KEY"
        || (key.starts_with("SOLANA_PRIVATE_KEY")
            && key["SOLANA_PRIVATE_KEY".len()..]
                .chars()
                .all(|c| c.is_ascii_digit()))
}

pub fn wallet_display_label(wallet_key: &str) -> String {
    let key = wallet_key.trim();
    if key.is_empty() {
        return "#?".to_string();
    }
    let snapshot = shared_config_manager().current_snapshot();
    if let Some(wallet) = snapshot.wallets.iter().find(|wallet| wallet.key == key) {
        let label = wallet.label.trim();
        if !label.is_empty() && label != key {
            return label.to_string();
        }
    }
    wallet_slot_label(key).unwrap_or_else(|| key.to_string())
}

fn wallet_slot_label(wallet_key: &str) -> Option<String> {
    let suffix = wallet_key.strip_prefix("SOLANA_PRIVATE_KEY")?;
    if suffix.is_empty() {
        return Some("#1".to_string());
    }
    suffix
        .parse::<usize>()
        .ok()
        .filter(|slot| *slot > 0)
        .map(|slot| format!("#{slot}"))
}

pub fn read_keypair_bytes(raw: &str) -> Result<Vec<u8>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Keypair value was empty.".to_string());
    }
    if trimmed.starts_with('[') {
        let parsed: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|error| error.to_string())?;
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

pub fn split_wallet_secret_and_name(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    if trimmed.starts_with('[') {
        if let Some(end_index) = trimmed.rfind(']') {
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
    }
    if let Some((secret, name)) = trimmed.split_once(',') {
        let name = name.trim();
        return (
            secret.trim().to_string(),
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            },
        );
    }
    (trimmed.to_string(), None)
}

pub fn join_wallet_secret_and_name(secret: &str, name: &str) -> String {
    let trimmed_secret = secret.trim();
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        trimmed_secret.to_string()
    } else {
        format!("{trimmed_secret},{trimmed_name}")
    }
}

pub fn public_key_from_secret(secret: &str) -> Result<String, String> {
    let bytes = read_keypair_bytes(secret)?;
    match bytes.len() {
        64 => {
            let keypair = Keypair::try_from(bytes.as_slice())
                .map_err(|error| format!("Invalid keypair secret: {error}"))?;
            Ok(keypair.pubkey().to_string())
        }
        32 => {
            Err("32-byte private keys are not yet supported by the Rust wallet parser.".to_string())
        }
        other => Err(format!("Unsupported keypair length: {other} bytes.")),
    }
}

pub fn next_wallet_env_key<'a>(keys: impl Iterator<Item = &'a str>) -> String {
    let mut taken = HashSet::new();
    for key in keys {
        if !is_solana_wallet_env_key(key) {
            continue;
        }
        if key == "SOLANA_PRIVATE_KEY" {
            taken.insert(1usize);
        } else if let Some(suffix) = key.strip_prefix("SOLANA_PRIVATE_KEY") {
            if let Ok(index) = suffix.parse::<usize>() {
                taken.insert(index);
            }
        }
    }
    for index in 1usize.. {
        if !taken.contains(&index) {
            return if index == 1 {
                "SOLANA_PRIVATE_KEY".to_string()
            } else {
                format!("SOLANA_PRIVATE_KEY{index}")
            };
        }
    }
    unreachable!("wallet key space exhausted")
}

pub fn configured_env_value(keys: &[&str]) -> Option<String> {
    let snapshot = shared_config_manager().current_snapshot();
    keys.iter().find_map(|key| {
        snapshot
            .values
            .get(*key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                std::env::var(key)
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
    })
}

fn load_snapshot_from_path(path: &Path) -> Result<SharedConfigSnapshot, String> {
    let contents = fs::read_to_string(path).unwrap_or_default();
    let values = parse_env_values(&contents);
    let wallets = parse_wallets(&values);
    let rpc = parse_rpc_config(&values);
    Ok(SharedConfigSnapshot {
        env_path: path.to_path_buf(),
        env_modified_unix_ms: modified_unix_ms(path).unwrap_or(0),
        values,
        wallets,
        rpc,
    })
}

fn parse_env_values(contents: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        values.insert(key.to_string(), value.trim().to_string());
    }
    values
}

fn parse_wallets(values: &BTreeMap<String, String>) -> Vec<SharedWalletEntry> {
    let mut keys: Vec<String> = values
        .keys()
        .filter(|key| is_solana_wallet_env_key(key))
        .cloned()
        .collect();
    keys.sort_by_key(|key| {
        key.strip_prefix("SOLANA_PRIVATE_KEY")
            .and_then(|suffix| {
                if suffix.is_empty() {
                    Some(1usize)
                } else {
                    suffix.parse::<usize>().ok()
                }
            })
            .unwrap_or(usize::MAX)
    });
    keys.into_iter()
        .filter_map(|key| {
            let raw = values.get(&key)?.to_string();
            let (secret, custom_name) = split_wallet_secret_and_name(&raw);
            let public_key = public_key_from_secret(&secret).ok()?;
            Some(SharedWalletEntry {
                key: key.clone(),
                label: custom_name.unwrap_or_else(|| key.clone()),
                public_key,
                secret,
            })
        })
        .collect()
}

fn parse_rpc_config(values: &BTreeMap<String, String>) -> SharedRpcConfig {
    let rpc_url = values
        .get("SOLANA_RPC_URL")
        .map(String::as_str)
        .unwrap_or(DEFAULT_RPC_URL)
        .trim()
        .to_string();
    let ws_url = values
        .get("SOLANA_WS_URL")
        .map(String::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let warm_rpc_url = values
        .get("WARM_RPC_URL")
        .map(String::as_str)
        .unwrap_or(rpc_url.as_str())
        .trim()
        .to_string();
    let shared_region = values
        .get("USER_REGION")
        .map(String::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let helius_rpc_url = values
        .get("HELIUS_RPC_URL")
        .map(String::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let helius_ws_url = values
        .get("HELIUS_WS_URL")
        .map(String::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let standard_rpc_send_urls = values
        .get("LAUNCHDECK_EXTRA_STANDARD_RPC_SEND_URLS")
        .or_else(|| values.get("LAUNCHDECK_STANDARD_RPC_SEND_URLS"))
        .map(String::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    let helius_sender_region = values
        .get("USER_REGION_HELIUS_SENDER")
        .map(String::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    SharedRpcConfig {
        rpc_url,
        ws_url,
        warm_rpc_url,
        shared_region,
        helius_rpc_url,
        helius_ws_url,
        standard_rpc_send_urls,
        helius_sender_region,
    }
}

fn apply_env_updates(contents: &str, updates: &BTreeMap<String, Option<String>>) -> String {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw_line in contents.lines() {
        let candidate_key = parse_env_key(raw_line);
        if let Some(key) = candidate_key {
            if let Some(next_value) = updates.get(key.as_str()) {
                seen.insert(key.clone());
                if let Some(next_value) = next_value {
                    output.push(format!("{key}={next_value}"));
                }
                continue;
            }
        }
        output.push(raw_line.to_string());
    }
    for (key, value) in updates {
        if seen.contains(key) {
            continue;
        }
        if let Some(value) = value {
            output.push(format!("{key}={value}"));
        }
    }
    let mut serialized = output.join("\n");
    if !serialized.ends_with('\n') {
        serialized.push('\n');
    }
    serialized
}

fn parse_env_key(raw_line: &str) -> Option<String> {
    let trimmed = raw_line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, contents)
        .map_err(|error| format!("Failed to write {}: {error}", temp_path.display()))?;
    fs::rename(&temp_path, path)
        .map_err(|error| format!("Failed to replace {}: {error}", path.display()))?;
    Ok(())
}

fn modified_unix_ms(path: &Path) -> Option<u128> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(
        modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
}

fn optional_env_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn trimmed_or_empty(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        apply_env_updates, join_wallet_secret_and_name, next_wallet_env_key,
        split_wallet_secret_and_name, wallet_slot_label,
    };
    use std::collections::BTreeMap;

    #[test]
    fn splits_wallet_secret_and_label() {
        let (secret, label) = split_wallet_secret_and_name("secret-value,Primary");
        assert_eq!(secret, "secret-value");
        assert_eq!(label.as_deref(), Some("Primary"));
    }

    #[test]
    fn preserves_json_wallet_secret_when_joining() {
        let joined = join_wallet_secret_and_name("[1,2,3]", "Desk");
        assert_eq!(joined, "[1,2,3],Desk");
    }

    #[test]
    fn next_wallet_key_reuses_lowest_open_slot() {
        let next = next_wallet_env_key(["SOLANA_PRIVATE_KEY", "SOLANA_PRIVATE_KEY3"].into_iter());
        assert_eq!(next, "SOLANA_PRIVATE_KEY2");
    }

    #[test]
    fn wallet_slot_label_hides_env_key_names() {
        assert_eq!(
            wallet_slot_label("SOLANA_PRIVATE_KEY").as_deref(),
            Some("#1")
        );
        assert_eq!(
            wallet_slot_label("SOLANA_PRIVATE_KEY2").as_deref(),
            Some("#2")
        );
        assert_eq!(wallet_slot_label("CUSTOM_WALLET").as_deref(), None);
    }

    #[test]
    fn apply_env_updates_preserves_unrelated_lines() {
        let mut updates = BTreeMap::new();
        updates.insert(
            "SOLANA_RPC_URL".to_string(),
            Some("https://rpc.example".to_string()),
        );
        updates.insert("USER_REGION".to_string(), None);
        let updated = apply_env_updates(
            "# comment\nSOLANA_RPC_URL=http://old\nUSER_REGION=EU\nHELLO=world\n",
            &updates,
        );
        assert!(updated.contains("# comment"));
        assert!(updated.contains("SOLANA_RPC_URL=https://rpc.example"));
        assert!(updated.contains("HELLO=world"));
        assert!(!updated.contains("USER_REGION=EU"));
    }
}
