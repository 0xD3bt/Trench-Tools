use crate::{
    DEFAULT_AUTO_FEE_HELIUS_PRIORITY_LEVEL, DEFAULT_AUTO_FEE_JITO_TIP_PERCENTILE,
    FeeMarketSnapshot, extract_jito_tip_floor_lamports, helius_fee_estimate_options,
    normalize_helius_priority_level, normalize_jito_tip_percentile, parse_auto_fee_cap_lamports,
    parse_helius_priority_estimate_result, parse_sol_decimal_to_lamports,
    provider_uses_auto_fee_priority, provider_uses_auto_fee_tip,
    resolve_auto_fee_components_with_total_cap,
};
use fs2::FileExt;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared_execution_routing::provider_tip::provider_required_tip_lamports;
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    hash::{Hash, Hasher},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_HELIUS_PRIORITY_REFRESH_INTERVAL_MS: u64 = 15_000;
pub const DEFAULT_HELIUS_PRIORITY_STALE_MS: u64 = 60_000;
pub const DEFAULT_JITO_TIP_REFRESH_INTERVAL_MS: u64 = 2_000;
pub const DEFAULT_JITO_TIP_STALE_MS: u64 = 45_000;
pub const DEFAULT_AUTO_FEE_BUFFER_PERCENT: f64 = 10.0;
pub const DEFAULT_AUTO_FEE_FALLBACK_LAMPORTS: u64 = 1_000_000;
/// How long the worker should back off after a failed refresh before
/// retrying. Short enough to recover quickly from transient Helius
/// hiccups without hammering the endpoint.
pub const HELIUS_REFRESH_RETRY_BACKOFF_MS: u64 = 3_000;

const CACHE_SCHEMA_VERSION: u64 = 1;
const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 10;
const DEFAULT_JITO_TIP_STREAM_ENDPOINT: &str = "wss://bundles.jito.wtf/api/v1/bundles/tip_stream";
const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(60);

#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedFeeMarketCacheFile {
    pub schemaVersion: u64,
    pub primaryRpcUrl: String,
    pub heliusPriorityRpcUrl: String,
    pub heliusPriorityLevel: String,
    pub jitoTipPercentile: String,
    pub updatedAtUnixMs: u64,
    pub heliusUpdatedAtUnixMs: Option<u64>,
    pub jitoUpdatedAtUnixMs: Option<u64>,
    #[serde(default)]
    pub heliusLastError: Option<String>,
    #[serde(default)]
    pub jitoLastError: Option<String>,
    pub snapshot: FeeMarketSnapshot,
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Default, Serialize)]
pub struct SharedFeeMarketLeaseStatus {
    pub owner: Option<String>,
    pub expiresAtUnixMs: Option<u64>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SharedFeeMarketSnapshotStatus {
    pub snapshot: FeeMarketSnapshot,
    pub helius_fresh: bool,
    pub jito_fresh: bool,
    pub cache_age_ms: Option<u64>,
    pub helius_age_ms: Option<u64>,
    pub jito_age_ms: Option<u64>,
    pub helius_last_error: Option<String>,
    pub jito_last_error: Option<String>,
    pub helius_lease: SharedFeeMarketLeaseStatus,
    pub jito_lease: SharedFeeMarketLeaseStatus,
}

/// Outcome of a single refresh attempt by the background worker. Lets
/// the loop pick a retry cadence: tight after [`RefreshOutcome::Failed`],
/// normal after [`RefreshOutcome::Refreshed`] or
/// [`RefreshOutcome::Skipped`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// We owned the lease and successfully wrote a fresh snapshot.
    Refreshed,
    /// Another process owns the lease, so we deferred to it.
    Skipped,
    /// We owned the lease but the upstream call returned an error.
    Failed,
}

#[derive(Debug, Clone)]
struct CachedFeeMarketSnapshot {
    snapshot: FeeMarketSnapshot,
    fetched_at: Instant,
}

fn memory_cache() -> &'static Mutex<HashMap<String, CachedFeeMarketSnapshot>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedFeeMarketSnapshot>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Per-`cache_key` guard that prevents N concurrent trade requests from
/// each spawning their own background refresh when the snapshot is
/// stale. The first caller flips the flag, performs the refresh, then
/// resets the flag.
fn opportunistic_refresh_guard(cache_key: &str) -> Arc<AtomicBool> {
    static GUARDS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    let guards = GUARDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = guards
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.entry(cache_key.to_string())
        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

#[derive(Clone, Debug)]
pub struct SharedFeeMarketConfig {
    pub cache_path: PathBuf,
    pub primary_rpc_url: String,
    pub helius_priority_rpc_url: String,
    pub owner: String,
    pub helius_priority_level: String,
    pub jito_tip_percentile: String,
    pub helius_refresh_interval: Duration,
    pub helius_max_age: Duration,
    pub jito_max_age: Duration,
    pub lease_ttl: Duration,
    pub jito_reconnect_delay: Duration,
    pub jito_tip_stream_endpoint: String,
    pub launch_account_keys: Vec<String>,
}

impl SharedFeeMarketConfig {
    pub fn new(
        cache_path: PathBuf,
        primary_rpc_url: String,
        helius_priority_rpc_url: String,
        owner: String,
        launch_account_keys: Vec<String>,
    ) -> Self {
        Self {
            cache_path,
            primary_rpc_url,
            helius_priority_rpc_url,
            owner,
            helius_priority_level: configured_helius_priority_level(),
            jito_tip_percentile: configured_jito_tip_percentile(),
            helius_refresh_interval: configured_helius_priority_refresh_interval(),
            helius_max_age: configured_helius_priority_stale_duration(),
            jito_max_age: configured_jito_tip_stale_duration(),
            lease_ttl: DEFAULT_LEASE_TTL,
            jito_reconnect_delay: configured_jito_tip_refresh_interval(),
            jito_tip_stream_endpoint: DEFAULT_JITO_TIP_STREAM_ENDPOINT.to_string(),
            launch_account_keys,
        }
    }

    fn cache_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.helius_priority_rpc_url, self.helius_priority_level, self.jito_tip_percentile
        )
    }

    fn lease_key(&self, kind: &str) -> String {
        let input = match kind {
            "helius" => format!(
                "helius|{}|{}|{}",
                self.helius_priority_rpc_url,
                self.helius_priority_level,
                self.launch_account_keys.join(",")
            ),
            "jito" => format!(
                "jito|{}|{}",
                self.jito_tip_stream_endpoint, self.jito_tip_percentile
            ),
            _ => self.cache_key(),
        };
        stable_hash_hex(&input)
    }

    fn lease_path(&self, kind: &str) -> PathBuf {
        let stem = self
            .cache_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("shared-fee-market");
        let key = self.lease_key(kind);
        self.cache_path
            .with_file_name(format!("{stem}.{kind}.{key}.lease"))
    }

    fn matches_cache_file(&self, cache: &SharedFeeMarketCacheFile) -> bool {
        cache.schemaVersion == CACHE_SCHEMA_VERSION
            && cache.heliusPriorityRpcUrl == self.helius_priority_rpc_url
            && cache.heliusPriorityLevel == self.helius_priority_level
            && cache.jitoTipPercentile == self.jito_tip_percentile
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoFeeDegradation {
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct AutoFeeResolutionInput<'a> {
    pub provider: &'a str,
    pub execution_class: &'a str,
    pub action: &'a str,
    pub action_label: &'a str,
    pub max_fee_sol: &'a str,
    pub fallback_priority_fee_sol: &'a str,
    pub fallback_tip_sol: &'a str,
    pub snapshot_status: Option<SharedFeeMarketSnapshotStatus>,
    pub allow_unavailable_fallback: bool,
}

#[derive(Debug, Clone)]
pub struct AutoFeeResolutionOutput {
    pub priority_lamports: Option<u64>,
    pub tip_lamports: Option<u64>,
    pub priority_estimate_lamports: Option<u64>,
    pub tip_estimate_lamports: Option<u64>,
    pub priority_source: String,
    pub tip_source: String,
    pub cap_lamports: Option<u64>,
    pub degradations: Vec<AutoFeeDegradation>,
}

#[derive(Debug, Deserialize, Serialize)]
struct FeeMarketLeaseFile {
    owner: String,
    expires_at_unix_ms: u64,
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn duration_ms_u64(value: Duration) -> u64 {
    value.as_millis().min(u128::from(u64::MAX)) as u64
}

fn stable_hash_hex(value: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn shared_fee_market_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS))
            .build()
            .expect("shared fee market client")
    })
}

pub fn configured_helius_priority_level() -> String {
    let value = std::env::var("HELIUS_PRIORITY_LEVEL")
        .or_else(|_| std::env::var("TRENCH_AUTO_FEE_HELIUS_PRIORITY_LEVEL"))
        .or_else(|_| std::env::var("LAUNCHDECK_AUTO_FEE_HELIUS_PRIORITY_LEVEL"))
        .unwrap_or_else(|_| DEFAULT_AUTO_FEE_HELIUS_PRIORITY_LEVEL.to_string());
    normalize_helius_priority_level(&value)
}

pub fn configured_jito_tip_percentile() -> String {
    let value = std::env::var("JITO_TIP_PERCENTILE")
        .or_else(|_| std::env::var("TRENCH_AUTO_FEE_JITO_TIP_PERCENTILE"))
        .or_else(|_| std::env::var("LAUNCHDECK_AUTO_FEE_JITO_TIP_PERCENTILE"))
        .unwrap_or_else(|_| DEFAULT_AUTO_FEE_JITO_TIP_PERCENTILE.to_string());
    normalize_jito_tip_percentile(&value)
}

fn configured_duration_ms(env_names: &[&str], default_ms: u64) -> Duration {
    Duration::from_millis(
        env_names
            .iter()
            .find_map(|name| std::env::var(name).ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(default_ms),
    )
}

pub fn configured_helius_priority_refresh_interval() -> Duration {
    configured_duration_ms(
        &[
            "HELIUS_PRIORITY_REFRESH_INTERVAL_MS",
            "TRENCH_HELIUS_PRIORITY_REFRESH_INTERVAL_MS",
            "LAUNCHDECK_HELIUS_PRIORITY_REFRESH_INTERVAL_MS",
        ],
        DEFAULT_HELIUS_PRIORITY_REFRESH_INTERVAL_MS,
    )
}

pub fn configured_helius_priority_stale_duration() -> Duration {
    configured_duration_ms(
        &[
            "HELIUS_PRIORITY_STALE_MS",
            "TRENCH_HELIUS_PRIORITY_STALE_MS",
        ],
        DEFAULT_HELIUS_PRIORITY_STALE_MS,
    )
}

pub fn configured_jito_tip_refresh_interval() -> Duration {
    configured_duration_ms(
        &[
            "JITO_TIP_REFRESH_INTERVAL_MS",
            "TRENCH_JITO_TIP_REFRESH_INTERVAL_MS",
        ],
        DEFAULT_JITO_TIP_REFRESH_INTERVAL_MS,
    )
}

pub fn configured_jito_tip_stale_duration() -> Duration {
    configured_duration_ms(
        &["JITO_TIP_STALE_MS", "TRENCH_JITO_TIP_STALE_MS"],
        DEFAULT_JITO_TIP_STALE_MS,
    )
}

pub fn configured_auto_fee_buffer_bps() -> u64 {
    std::env::var("AUTO_FEE_BUFFER_PERCENT")
        .or_else(|_| std::env::var("TRENCH_AUTO_FEE_BUFFER_PERCENT"))
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| (value.min(1_000.0) * 100.0).round() as u64)
        .unwrap_or((DEFAULT_AUTO_FEE_BUFFER_PERCENT * 100.0) as u64)
}

pub fn apply_auto_fee_estimate_buffer(value: u64) -> u64 {
    let buffer_bps = configured_auto_fee_buffer_bps();
    let denominator = 10_000u128;
    let multiplier = denominator.saturating_add(u128::from(buffer_bps));
    let buffered = u128::from(value)
        .saturating_mul(multiplier)
        .saturating_add(denominator.saturating_sub(1))
        / denominator;
    buffered.min(u128::from(u64::MAX)) as u64
}

fn cache_file_from_snapshot(
    config: &SharedFeeMarketConfig,
    snapshot: FeeMarketSnapshot,
    helius_updated_at: Option<u64>,
    jito_updated_at: Option<u64>,
) -> SharedFeeMarketCacheFile {
    let updated_at = unix_ms_now();
    let has_helius_value = snapshot.helius_priority_lamports.is_some()
        || snapshot.helius_launch_priority_lamports.is_some()
        || snapshot.helius_trade_priority_lamports.is_some();
    let has_jito_value = snapshot.jito_tip_p99_lamports.is_some();
    SharedFeeMarketCacheFile {
        schemaVersion: CACHE_SCHEMA_VERSION,
        primaryRpcUrl: config.primary_rpc_url.clone(),
        heliusPriorityRpcUrl: config.helius_priority_rpc_url.clone(),
        heliusPriorityLevel: config.helius_priority_level.clone(),
        jitoTipPercentile: config.jito_tip_percentile.clone(),
        updatedAtUnixMs: updated_at,
        heliusUpdatedAtUnixMs: if has_helius_value {
            helius_updated_at.or(Some(updated_at))
        } else {
            None
        },
        jitoUpdatedAtUnixMs: if has_jito_value {
            jito_updated_at.or(Some(updated_at))
        } else {
            None
        },
        heliusLastError: None,
        jitoLastError: None,
        snapshot,
    }
}

fn read_cache_file(path: &Path) -> Option<SharedFeeMarketCacheFile> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<SharedFeeMarketCacheFile>(&text).ok()
}

fn write_cache_file(path: &Path, cache: &SharedFeeMarketCacheFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create fee-market cache directory: {error}"))?;
    }
    let tmp_path = path.with_extension("tmp");
    let text = serde_json::to_string_pretty(cache)
        .map_err(|error| format!("Failed to encode fee-market cache: {error}"))?;
    fs::write(&tmp_path, text)
        .map_err(|error| format!("Failed to write fee-market cache temp file: {error}"))?;
    fs::rename(&tmp_path, path)
        .map_err(|error| format!("Failed to replace fee-market cache file: {error}"))
}

fn cache_source_fresh(timestamp_ms: Option<u64>, max_age: Duration) -> bool {
    let Some(timestamp_ms) = timestamp_ms else {
        return false;
    };
    unix_ms_now().saturating_sub(timestamp_ms) <= duration_ms_u64(max_age)
}

fn cache_source_age_ms(timestamp_ms: Option<u64>) -> Option<u64> {
    timestamp_ms.map(|timestamp_ms| unix_ms_now().saturating_sub(timestamp_ms))
}

fn read_lease_status(config: &SharedFeeMarketConfig, kind: &str) -> SharedFeeMarketLeaseStatus {
    let now = unix_ms_now();
    let Some(text) = fs::read_to_string(config.lease_path(kind)).ok() else {
        return SharedFeeMarketLeaseStatus {
            owner: None,
            expiresAtUnixMs: None,
            active: false,
        };
    };
    let Ok(lease) = serde_json::from_str::<FeeMarketLeaseFile>(&text) else {
        return SharedFeeMarketLeaseStatus {
            owner: None,
            expiresAtUnixMs: None,
            active: false,
        };
    };
    SharedFeeMarketLeaseStatus {
        owner: Some(lease.owner),
        expiresAtUnixMs: Some(lease.expires_at_unix_ms),
        active: lease.expires_at_unix_ms > now,
    }
}

pub fn read_shared_fee_market_snapshot(
    config: &SharedFeeMarketConfig,
) -> Option<SharedFeeMarketSnapshotStatus> {
    let cache = read_cache_file(&config.cache_path)?;
    if !config.matches_cache_file(&cache) {
        return None;
    }
    let now = unix_ms_now();
    Some(SharedFeeMarketSnapshotStatus {
        snapshot: cache.snapshot,
        helius_fresh: cache_source_fresh(cache.heliusUpdatedAtUnixMs, config.helius_max_age),
        jito_fresh: cache_source_fresh(cache.jitoUpdatedAtUnixMs, config.jito_max_age),
        cache_age_ms: Some(now.saturating_sub(cache.updatedAtUnixMs)),
        helius_age_ms: cache_source_age_ms(cache.heliusUpdatedAtUnixMs),
        jito_age_ms: cache_source_age_ms(cache.jitoUpdatedAtUnixMs),
        helius_last_error: cache.heliusLastError,
        jito_last_error: cache.jitoLastError,
        helius_lease: read_lease_status(config, "helius"),
        jito_lease: read_lease_status(config, "jito"),
    })
}

fn get_memory_cache_snapshot(config: &SharedFeeMarketConfig) -> Option<FeeMarketSnapshot> {
    let cache = memory_cache().lock().ok()?;
    let entry = cache.get(&config.cache_key())?;
    if entry.fetched_at.elapsed() > config.helius_max_age.min(config.jito_max_age) {
        return None;
    }
    Some(entry.snapshot.clone())
}

fn put_memory_cache_snapshot(config: &SharedFeeMarketConfig, snapshot: FeeMarketSnapshot) {
    if let Ok(mut cache) = memory_cache().lock() {
        cache.insert(
            config.cache_key(),
            CachedFeeMarketSnapshot {
                snapshot,
                fetched_at: Instant::now(),
            },
        );
    }
}

fn update_shared_cache<F>(config: &SharedFeeMarketConfig, updater: F) -> Result<(), String>
where
    F: FnOnce(&mut SharedFeeMarketCacheFile, u64),
{
    let now = unix_ms_now();
    let mut cache = read_cache_file(&config.cache_path)
        .filter(|cache| config.matches_cache_file(cache))
        .unwrap_or_else(|| {
            cache_file_from_snapshot(config, FeeMarketSnapshot::default(), None, None)
        });
    updater(&mut cache, now);
    cache.updatedAtUnixMs = now;
    write_cache_file(&config.cache_path, &cache)?;
    put_memory_cache_snapshot(config, cache.snapshot);
    Ok(())
}

fn acquire_lease(config: &SharedFeeMarketConfig, kind: &str, replace_other_owner: bool) -> bool {
    let path = config.lease_path(kind);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
    else {
        return false;
    };
    if file.lock_exclusive().is_err() {
        return false;
    }
    let mut text = String::new();
    let _ = file.read_to_string(&mut text);
    let now = unix_ms_now();
    if let Ok(existing) = serde_json::from_str::<FeeMarketLeaseFile>(&text) {
        if !replace_other_owner
            && existing.expires_at_unix_ms > now
            && existing.owner != config.owner
        {
            let _ = file.unlock();
            return false;
        }
    }
    let lease = FeeMarketLeaseFile {
        owner: config.owner.clone(),
        expires_at_unix_ms: now.saturating_add(duration_ms_u64(config.lease_ttl)),
    };
    let Ok(lease_text) = serde_json::to_string(&lease) else {
        let _ = file.unlock();
        return false;
    };
    if file.set_len(0).is_err()
        || file.seek(SeekFrom::Start(0)).is_err()
        || file.write_all(lease_text.as_bytes()).is_err()
        || file.sync_all().is_err()
    {
        let _ = file.unlock();
        return false;
    }
    let _ = file.unlock();
    true
}

fn try_acquire_lease(config: &SharedFeeMarketConfig, kind: &str) -> bool {
    acquire_lease(config, kind, false)
}

fn force_acquire_lease(config: &SharedFeeMarketConfig, kind: &str) -> bool {
    acquire_lease(config, kind, true)
}

pub struct SharedFeeMarketRuntime {
    config: SharedFeeMarketConfig,
}

impl SharedFeeMarketRuntime {
    pub fn new(config: SharedFeeMarketConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &SharedFeeMarketConfig {
        &self.config
    }

    pub fn read_snapshot_status(&self) -> Option<SharedFeeMarketSnapshotStatus> {
        read_shared_fee_market_snapshot(&self.config)
    }

    pub async fn fetch_helius_priority_snapshot_live(&self) -> Result<FeeMarketSnapshot, String> {
        let generic_priority = self.fetch_helius_priority_estimate(None).await;
        let launch_priority = if self.config.launch_account_keys.is_empty() {
            Ok(None)
        } else {
            self.fetch_helius_priority_estimate(Some(&self.config.launch_account_keys))
                .await
        };
        let helius_priority_lamports = generic_priority?;
        let helius_launch_priority_lamports = launch_priority.unwrap_or(None);
        Ok(FeeMarketSnapshot {
            helius_priority_lamports,
            helius_launch_priority_lamports,
            helius_trade_priority_lamports: None,
            jito_tip_p99_lamports: None,
        })
    }

    pub async fn fetch_jito_tip_floor_live(&self) -> Result<Option<u64>, String> {
        let response = shared_fee_market_http_client()
            .get("https://bundles.jito.wtf/api/v1/bundles/tip_floor")
            .send()
            .await
            .map_err(|error| format!("Jito tip floor request failed: {error}"))?;
        let payload = response
            .json::<Value>()
            .await
            .map_err(|error| format!("Failed to decode Jito tip floor: {error}"))?;
        Ok(extract_jito_tip_floor_lamports(
            &payload,
            &self.config.jito_tip_percentile,
        ))
    }

    async fn fetch_helius_priority_estimate(
        &self,
        account_keys: Option<&[String]>,
    ) -> Result<Option<u64>, String> {
        let mut params = json!({
            "options": helius_fee_estimate_options(&self.config.helius_priority_level)
        });
        if let Some(account_keys) = account_keys.filter(|keys| !keys.is_empty()) {
            params["accountKeys"] = Value::Array(
                account_keys
                    .iter()
                    .map(|value| Value::String(value.clone()))
                    .collect(),
            );
        }
        let payload = shared_fee_market_http_client()
            .post(&self.config.helius_priority_rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "shared-fee-market-helius-priority-estimate",
                "method": "getPriorityFeeEstimate",
                "params": [params]
            }))
            .send()
            .await
            .map_err(|error| format!("Helius priority estimate request failed: {error}"))?
            .json::<Value>()
            .await
            .map_err(|error| format!("Failed to decode Helius fee estimate: {error}"))?;
        if let Some(error) = payload.get("error") {
            return Err(format!("Helius priority estimate failed: {error}"));
        }
        let result = payload.get("result").unwrap_or(&payload);
        Ok(parse_helius_priority_estimate_result(
            result,
            &self.config.helius_priority_level,
        ))
    }

    pub async fn fetch_fee_market_snapshot_live(&self) -> Result<FeeMarketSnapshot, String> {
        let (helius_snapshot, jito_tip_p99_lamports) = tokio::join!(
            self.fetch_helius_priority_snapshot_live(),
            self.fetch_jito_tip_floor_live()
        );
        let helius_snapshot = helius_snapshot.map_err(|error| {
            self.record_helius_error(error.clone());
            error
        })?;
        self.update_helius_snapshot(&helius_snapshot)?;
        match jito_tip_p99_lamports {
            Ok(Some(tip_lamports)) => self.update_jito_tip_snapshot(Some(tip_lamports))?,
            Ok(None) => self.record_jito_error(format!(
                "Jito tip floor response did not include {}",
                self.config.jito_tip_percentile
            )),
            Err(error) => self.record_jito_error(error),
        }
        self.read_snapshot_status()
            .map(|status| status.snapshot)
            .ok_or_else(|| "live fee-market refresh did not write a readable snapshot".to_string())
    }

    pub async fn fetch_fee_market_snapshot(&self) -> Result<FeeMarketSnapshot, String> {
        if let Some(snapshot) = get_memory_cache_snapshot(&self.config) {
            return Ok(snapshot);
        }
        if let Some(status) = self.read_snapshot_status() {
            if status.helius_fresh && status.jito_fresh {
                put_memory_cache_snapshot(&self.config, status.snapshot.clone());
                return Ok(status.snapshot);
            }
        }
        if try_acquire_lease(&self.config, "live") {
            return self.fetch_fee_market_snapshot_live().await;
        }
        if let Some(status) = self.read_snapshot_status() {
            return Ok(status.snapshot);
        }
        Err("Fee market snapshot unavailable and another process owns the fetch lease.".to_string())
    }

    pub async fn initialize_fee_market_snapshot(
        &self,
    ) -> Result<SharedFeeMarketSnapshotStatus, String> {
        let _ = force_acquire_lease(&self.config, "live");
        let _ = force_acquire_lease(&self.config, "helius");
        let _ = force_acquire_lease(&self.config, "jito");

        let (helius_result, jito_result) = tokio::join!(
            self.fetch_helius_priority_snapshot_live(),
            self.fetch_jito_tip_floor_live()
        );
        let mut refresh_errors = Vec::new();

        match helius_result {
            Ok(snapshot) => {
                if let Err(error) = self.update_helius_snapshot(&snapshot) {
                    refresh_errors.push(format!("helius cache update failed: {error}"));
                }
            }
            Err(error) => {
                self.record_helius_error(error.clone());
                refresh_errors.push(format!("helius refresh failed: {error}"));
            }
        }

        match jito_result {
            Ok(Some(tip_lamports)) => {
                if let Err(error) = self.update_jito_tip_snapshot(Some(tip_lamports)) {
                    refresh_errors.push(format!("jito cache update failed: {error}"));
                }
            }
            Ok(None) => {
                let error = format!(
                    "Jito tip floor response did not include {}",
                    self.config.jito_tip_percentile
                );
                self.record_jito_error(error.clone());
                refresh_errors.push(format!("jito refresh failed: {error}"));
            }
            Err(error) => {
                self.record_jito_error(error.clone());
                refresh_errors.push(format!("jito refresh failed: {error}"));
            }
        }

        let status = self.read_snapshot_status().ok_or_else(|| {
            format!(
                "fee-market startup refresh did not write a readable snapshot: {}",
                refresh_errors.join("; ")
            )
        })?;
        if !status.helius_fresh || !status.jito_fresh {
            return Err(format!(
                "fee-market startup refresh did not produce a fresh complete snapshot: helius_fresh={} helius_age_ms={:?} jito_fresh={} jito_age_ms={:?} refresh_errors={}",
                status.helius_fresh,
                status.helius_age_ms,
                status.jito_fresh,
                status.jito_age_ms,
                refresh_errors.join("; ")
            ));
        }
        Ok(status)
    }

    pub fn cache_full_snapshot(&self, snapshot: FeeMarketSnapshot) -> Result<(), String> {
        let now = unix_ms_now();
        let cache = cache_file_from_snapshot(&self.config, snapshot.clone(), Some(now), Some(now));
        write_cache_file(&self.config.cache_path, &cache)?;
        put_memory_cache_snapshot(&self.config, snapshot);
        Ok(())
    }

    pub fn update_helius_snapshot(&self, snapshot: &FeeMarketSnapshot) -> Result<(), String> {
        update_shared_cache(&self.config, |cache, now| {
            let mut updated = false;
            if snapshot.helius_priority_lamports.is_some() {
                cache.snapshot.helius_priority_lamports = snapshot.helius_priority_lamports;
                updated = true;
            }
            if snapshot.helius_launch_priority_lamports.is_some() {
                cache.snapshot.helius_launch_priority_lamports =
                    snapshot.helius_launch_priority_lamports;
                updated = true;
            }
            if snapshot.helius_trade_priority_lamports.is_some() {
                cache.snapshot.helius_trade_priority_lamports =
                    snapshot.helius_trade_priority_lamports;
                updated = true;
            }
            if updated {
                cache.heliusUpdatedAtUnixMs = Some(now);
                cache.heliusLastError = None;
            }
        })
    }

    pub fn update_jito_tip_snapshot(&self, jito_tip_lamports: Option<u64>) -> Result<(), String> {
        update_shared_cache(&self.config, |cache, now| {
            cache.snapshot.jito_tip_p99_lamports = jito_tip_lamports;
            if jito_tip_lamports.is_some() {
                cache.jitoUpdatedAtUnixMs = Some(now);
                cache.jitoLastError = None;
            } else {
                cache.jitoUpdatedAtUnixMs = None;
            }
        })
    }

    fn record_helius_error(&self, error: String) {
        // Surface to stderr so operators can see why auto-fee is degraded;
        // previously this was only persisted to `heliusLastError` in the
        // cache file and was effectively invisible during incidents.
        eprintln!(
            "[shared-fee-market][helius] priority refresh failed owner={} error={}",
            self.config.owner, error
        );
        let _ = update_shared_cache(&self.config, |cache, _now| {
            cache.heliusLastError = Some(error);
        });
    }

    fn record_jito_error(&self, error: String) {
        eprintln!(
            "[shared-fee-market][jito] tip refresh failed owner={} error={}",
            self.config.owner, error
        );
        let _ = update_shared_cache(&self.config, |cache, _now| {
            cache.jitoLastError = Some(error);
        });
    }

    /// Attempt a Helius priority refresh if this process owns the lease.
    /// Returns the resulting [`RefreshOutcome`] so the driving loop can
    /// schedule a tighter retry on transient failures.
    pub async fn refresh_helius_if_leased(&self) -> RefreshOutcome {
        if !try_acquire_lease(&self.config, "helius") {
            return RefreshOutcome::Skipped;
        }
        match self.fetch_helius_priority_snapshot_live().await {
            Ok(snapshot) => {
                let _ = self.update_helius_snapshot(&snapshot);
                RefreshOutcome::Refreshed
            }
            Err(error) => {
                self.record_helius_error(error);
                RefreshOutcome::Failed
            }
        }
    }

    /// Attempt a Jito tip refresh if this process owns the lease. Returns
    /// the same [`RefreshOutcome`] semantics as the Helius variant.
    pub async fn refresh_jito_if_leased(&self) -> RefreshOutcome {
        if !try_acquire_lease(&self.config, "jito") {
            return RefreshOutcome::Skipped;
        }
        self.refresh_jito_http_fallback_if_stale().await;
        match self.consume_jito_tip_stream_updates().await {
            Ok(()) => RefreshOutcome::Refreshed,
            Err(error) => {
                self.record_jito_error(error);
                self.refresh_jito_http_fallback_if_stale().await;
                RefreshOutcome::Failed
            }
        }
    }

    /// If the shared snapshot's Helius half is stale, spawn a single
    /// best-effort background refresh and return immediately. Multiple
    /// callers racing this method per snapshot collapse into one in-flight
    /// fetch via a per-cache `AtomicBool` guard, so trade hot paths can
    /// call it freely without amplifying Helius load. The current trade
    /// will not see the new value, but the next one (within ~1s) will.
    /// Silently no-ops when invoked outside a Tokio runtime so non-async
    /// unit tests can exercise the resolver path.
    pub fn maybe_spawn_helius_refresh_if_stale(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let needs_refresh = self
            .read_snapshot_status()
            .map(|status| !status.helius_fresh)
            .unwrap_or(true);
        if !needs_refresh {
            return;
        }
        let guard = opportunistic_refresh_guard(&self.config.cache_key());
        if guard
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let config = self.config.clone();
        tokio::spawn(async move {
            let runtime = SharedFeeMarketRuntime::new(config);
            let _ = runtime.refresh_helius_if_leased().await;
            opportunistic_refresh_guard(&runtime.config.cache_key()).store(false, Ordering::Release);
        });
    }

    async fn refresh_jito_http_fallback_if_stale(&self) {
        let needs_fallback = self
            .read_snapshot_status()
            .map(|status| !status.jito_fresh)
            .unwrap_or(true);
        if !needs_fallback {
            return;
        }
        match self.fetch_jito_tip_floor_live().await {
            Ok(Some(jito_tip_p99_lamports)) => {
                let _ = self.update_jito_tip_snapshot(Some(jito_tip_p99_lamports));
            }
            Ok(None) => {
                self.record_jito_error(format!(
                    "Jito tip floor response did not include {}",
                    self.config.jito_tip_percentile
                ));
            }
            Err(error) => self.record_jito_error(error),
        }
    }

    pub async fn consume_jito_tip_stream_updates(&self) -> Result<(), String> {
        let mut ws = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async(self.config.jito_tip_stream_endpoint.as_str()),
        )
        .await
        .map_err(|_| "Timed out connecting to Jito tip stream.".to_string())?
        .map(|(stream, _)| stream)
        .map_err(|error| format!("Failed to connect to Jito tip stream: {error}"))?;
        let mut last_message_at = Instant::now();
        loop {
            let message = tokio::select! {
                message = ws.next() => message,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if last_message_at.elapsed() >= self.config.jito_max_age {
                        return Err("Timed out waiting for Jito tip stream update.".to_string());
                    }
                    continue;
                }
            };
            let Some(message) = message else {
                return Err("Jito tip stream closed.".to_string());
            };
            let message =
                message.map_err(|error| format!("Jito tip stream read failed: {error}"))?;
            last_message_at = Instant::now();
            match message {
                tokio_tungstenite::tungstenite::protocol::Message::Text(text) => {
                    if let Ok(payload) = serde_json::from_str::<Value>(&text) {
                        if let Some(value) = extract_jito_tip_floor_lamports(
                            &payload,
                            &self.config.jito_tip_percentile,
                        ) {
                            let _ = self.update_jito_tip_snapshot(Some(value));
                            let _ = try_acquire_lease(&self.config, "jito");
                        }
                    }
                }
                tokio_tungstenite::tungstenite::protocol::Message::Binary(bytes) => {
                    if let Ok(payload) = serde_json::from_slice::<Value>(&bytes) {
                        if let Some(value) = extract_jito_tip_floor_lamports(
                            &payload,
                            &self.config.jito_tip_percentile,
                        ) {
                            let _ = self.update_jito_tip_snapshot(Some(value));
                            let _ = try_acquire_lease(&self.config, "jito");
                        }
                    }
                }
                tokio_tungstenite::tungstenite::protocol::Message::Ping(payload) => {
                    ws.send(tokio_tungstenite::tungstenite::protocol::Message::Pong(
                        payload,
                    ))
                    .await
                    .map_err(|error| format!("Jito tip stream pong failed: {error}"))?;
                }
                tokio_tungstenite::tungstenite::protocol::Message::Pong(_) => {}
                tokio_tungstenite::tungstenite::protocol::Message::Frame(_) => {}
                tokio_tungstenite::tungstenite::protocol::Message::Close(_) => {
                    return Err("Jito tip stream closed.".to_string());
                }
            }
        }
    }
}

pub fn resolve_buffered_auto_fee_components(
    input: AutoFeeResolutionInput<'_>,
) -> Result<AutoFeeResolutionOutput, String> {
    let cap_lamports = parse_auto_fee_cap_lamports(input.max_fee_sol);
    let fallback_priority_lamports =
        positive_sol_fallback_lamports(input.fallback_priority_fee_sol);
    let fallback_tip_lamports = positive_sol_fallback_lamports(input.fallback_tip_sol);
    let uses_priority =
        provider_uses_auto_fee_priority(input.provider, input.execution_class, input.action);
    let uses_tip = provider_uses_auto_fee_tip(input.provider, input.action);
    let mut degradations = Vec::new();
    let mut priority_source = "off".to_string();
    let mut tip_source = "off".to_string();
    let mut priority_estimate = None;
    let mut tip_estimate = None;
    let mut priority_used_fallback = false;
    let mut tip_used_fallback = false;

    if uses_priority {
        if let Some(status) = input
            .snapshot_status
            .as_ref()
            .filter(|status| status.helius_fresh)
        {
            priority_estimate = match input.action {
                "creation" => status.snapshot.launch_priority_lamports(),
                _ => status.snapshot.trade_priority_lamports(),
            }
            .map(apply_auto_fee_estimate_buffer);
            priority_source = if priority_estimate.is_some() {
                "shared-helius-buffered".to_string()
            } else {
                "missing".to_string()
            };
        }
        if priority_estimate.is_none() && input.allow_unavailable_fallback {
            priority_estimate = Some(fallback_priority_lamports);
            priority_used_fallback = true;
            priority_source = if fallback_priority_lamports == DEFAULT_AUTO_FEE_FALLBACK_LAMPORTS {
                "fallback-0.001".to_string()
            } else {
                "fallback-user-priority".to_string()
            };
            degradations.push(AutoFeeDegradation {
                source: "helius".to_string(),
                message: auto_fee_unavailable_message(
                    "helius",
                    input
                        .snapshot_status
                        .as_ref()
                        .and_then(|status| status.helius_age_ms),
                    "priority fee",
                    fallback_priority_lamports,
                ),
            });
        }
    }

    if uses_tip {
        if let Some(status) = input
            .snapshot_status
            .as_ref()
            .filter(|status| status.jito_fresh)
        {
            tip_estimate = status
                .snapshot
                .jito_tip_p99_lamports
                .map(apply_auto_fee_estimate_buffer);
            tip_source = if tip_estimate.is_some() {
                "shared-jito-buffered".to_string()
            } else {
                "missing".to_string()
            };
        }
        if tip_estimate.is_none() && input.allow_unavailable_fallback {
            tip_estimate = Some(fallback_tip_lamports);
            tip_used_fallback = true;
            tip_source = if fallback_tip_lamports == DEFAULT_AUTO_FEE_FALLBACK_LAMPORTS {
                "fallback-0.001".to_string()
            } else {
                "fallback-user-tip".to_string()
            };
            degradations.push(AutoFeeDegradation {
                source: "jito".to_string(),
                message: auto_fee_unavailable_message(
                    "jito",
                    input
                        .snapshot_status
                        .as_ref()
                        .and_then(|status| status.jito_age_ms),
                    "tip",
                    fallback_tip_lamports,
                ),
            });
        }
    }

    let (priority_lamports, tip_lamports) = if priority_used_fallback || tip_used_fallback {
        let resolved_priority = priority_estimate.map(|estimate| {
            if priority_used_fallback {
                estimate
            } else {
                cap_lamports
                    .map(|cap| estimate.min(cap))
                    .unwrap_or(estimate)
            }
        });
        let resolved_tip = match tip_estimate {
            Some(estimate) => {
                let with_floor =
                    estimate.max(provider_required_tip_lamports(input.provider).unwrap_or(0));
                let resolved = if tip_used_fallback {
                    with_floor
                } else {
                    let capped = cap_lamports
                        .map(|cap| with_floor.min(cap))
                        .unwrap_or(with_floor);
                    crate::clamp_auto_fee_tip_to_provider_minimum(
                        capped,
                        input.provider,
                        cap_lamports,
                        input.action_label,
                    )?
                };
                Some(resolved)
            }
            None => None,
        };
        (resolved_priority, resolved_tip)
    } else {
        resolve_auto_fee_components_with_total_cap(
            priority_estimate,
            tip_estimate,
            cap_lamports,
            input.provider,
            input.action_label,
        )?
    };
    let tip_lamports = match tip_lamports {
        Some(tip) if provider_required_tip_lamports(input.provider).is_some() => Some(tip),
        Some(_) => None,
        None => None,
    };
    if uses_priority && uses_tip {
        if let Some(minimum_tip_lamports) = provider_required_tip_lamports(input.provider) {
            if minimum_tip_lamports > 0
                && cap_lamports == Some(minimum_tip_lamports)
                && priority_lamports == Some(1)
            {
                degradations.push(AutoFeeDegradation {
                    source: "cap".to_string(),
                    message: format!(
                        "Auto Fee cap equals the {} minimum tip of {} SOL, so priority fee was reduced to 1 lamport. Increase max auto fee to include priority fee.",
                        input.provider,
                        crate::format_lamports_to_sol_decimal(minimum_tip_lamports)
                    ),
                });
            }
        }
    }

    Ok(AutoFeeResolutionOutput {
        priority_lamports,
        tip_lamports,
        priority_estimate_lamports: priority_estimate,
        tip_estimate_lamports: tip_estimate,
        priority_source,
        tip_source,
        cap_lamports,
        degradations,
    })
}

fn positive_sol_fallback_lamports(value: &str) -> u64 {
    parse_sol_decimal_to_lamports(value)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_AUTO_FEE_FALLBACK_LAMPORTS)
}

fn auto_fee_unavailable_message(
    source: &str,
    age_ms: Option<u64>,
    fallback_label: &str,
    fallback_lamports: u64,
) -> String {
    let reason = age_ms
        .map(|age_ms| format!("stale {}s", age_ms / 1_000))
        .unwrap_or_else(|| "missing".to_string());
    let fallback_sol = crate::format_lamports_to_sol_decimal(fallback_lamports);
    format!(
        "Auto Fee unavailable: {source} {reason}. Defaulted {fallback_label} to {fallback_sol} SOL. Fix Auto Fee or switch it off."
    )
}

pub fn shared_fee_market_status_payload(config: &SharedFeeMarketConfig) -> Value {
    let status = read_shared_fee_market_snapshot(config);
    json!({
        "heliusFresh": status.as_ref().map(|value| value.helius_fresh).unwrap_or(false),
        "heliusAgeMs": status.as_ref().and_then(|value| value.helius_age_ms),
        "heliusLastError": status.as_ref().and_then(|value| value.helius_last_error.clone()),
        "heliusLease": status.as_ref()
            .map(|value| serde_json::to_value(&value.helius_lease).unwrap_or_else(|_| json!({})))
            .unwrap_or_else(|| serde_json::to_value(read_lease_status(config, "helius")).unwrap_or_else(|_| json!({}))),
        "jitoFresh": status.as_ref().map(|value| value.jito_fresh).unwrap_or(false),
        "jitoAgeMs": status.as_ref().and_then(|value| value.jito_age_ms),
        "jitoLastError": status.as_ref().and_then(|value| value.jito_last_error.clone()),
        "jitoLease": status.as_ref()
            .map(|value| serde_json::to_value(&value.jito_lease).unwrap_or_else(|_| json!({})))
            .unwrap_or_else(|| serde_json::to_value(read_lease_status(config, "jito")).unwrap_or_else(|_| json!({}))),
        "config": {
            "heliusPriorityLevel": config.helius_priority_level.clone(),
            "heliusRefreshIntervalMs": duration_ms_u64(config.helius_refresh_interval),
            "heliusStaleMs": duration_ms_u64(config.helius_max_age),
            "jitoTipPercentile": config.jito_tip_percentile.clone(),
            "jitoReconnectIntervalMs": duration_ms_u64(config.jito_reconnect_delay),
            "jitoStaleMs": duration_ms_u64(config.jito_max_age),
            "autoFeeBufferPercent": (configured_auto_fee_buffer_bps() as f64) / 100.0,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_env_vars<F: FnOnce()>(updates: &[(&str, Option<&str>)], f: F) {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let originals = updates
            .iter()
            .map(|(name, _)| (*name, std::env::var(name).ok()))
            .collect::<Vec<_>>();
        for (name, value) in updates {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
        f();
        for (name, value) in originals {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    fn fresh_status(snapshot: FeeMarketSnapshot) -> SharedFeeMarketSnapshotStatus {
        SharedFeeMarketSnapshotStatus {
            snapshot,
            helius_fresh: true,
            jito_fresh: true,
            cache_age_ms: Some(0),
            helius_age_ms: Some(0),
            jito_age_ms: Some(0),
            helius_last_error: None,
            jito_last_error: None,
            helius_lease: SharedFeeMarketLeaseStatus::default(),
            jito_lease: SharedFeeMarketLeaseStatus::default(),
        }
    }

    fn snapshot_status(
        snapshot: FeeMarketSnapshot,
        helius_fresh: bool,
        jito_fresh: bool,
    ) -> SharedFeeMarketSnapshotStatus {
        SharedFeeMarketSnapshotStatus {
            snapshot,
            helius_fresh,
            jito_fresh,
            cache_age_ms: Some(0),
            helius_age_ms: Some(if helius_fresh { 0 } else { 60_000 }),
            jito_age_ms: Some(if jito_fresh { 0 } else { 60_000 }),
            helius_last_error: None,
            jito_last_error: None,
            helius_lease: SharedFeeMarketLeaseStatus::default(),
            jito_lease: SharedFeeMarketLeaseStatus::default(),
        }
    }

    #[test]
    fn auto_fee_buffer_rounds_live_estimates_up_by_ten_percent() {
        with_env_vars(
            &[
                ("AUTO_FEE_BUFFER_PERCENT", None),
                ("TRENCH_AUTO_FEE_BUFFER_PERCENT", None),
            ],
            || {
                assert_eq!(configured_auto_fee_buffer_bps(), 1_000);
                assert_eq!(apply_auto_fee_estimate_buffer(1_000), 1_100);
                assert_eq!(apply_auto_fee_estimate_buffer(1), 2);
            },
        );
    }

    #[test]
    fn auto_fee_buffer_percent_is_configurable() {
        with_env_vars(
            &[
                ("AUTO_FEE_BUFFER_PERCENT", Some("25")),
                ("TRENCH_AUTO_FEE_BUFFER_PERCENT", None),
            ],
            || {
                assert_eq!(configured_auto_fee_buffer_bps(), 2_500);
                assert_eq!(apply_auto_fee_estimate_buffer(1_000), 1_250);
                assert_eq!(apply_auto_fee_estimate_buffer(1), 2);
            },
        );
    }

    #[test]
    fn preferred_auto_fee_env_defaults_and_aliases_are_resolved() {
        with_env_vars(
            &[
                ("JITO_TIP_PERCENTILE", Some("p95")),
                ("LAUNCHDECK_AUTO_FEE_JITO_TIP_PERCENTILE", Some("p25")),
                ("HELIUS_PRIORITY_LEVEL", Some("medium")),
                (
                    "LAUNCHDECK_AUTO_FEE_HELIUS_PRIORITY_LEVEL",
                    Some("unsafeMax"),
                ),
                ("HELIUS_PRIORITY_REFRESH_INTERVAL_MS", Some("12345")),
                (
                    "LAUNCHDECK_HELIUS_PRIORITY_REFRESH_INTERVAL_MS",
                    Some("6000"),
                ),
                ("HELIUS_PRIORITY_STALE_MS", Some("45000")),
                ("JITO_TIP_REFRESH_INTERVAL_MS", Some("2000")),
                ("JITO_TIP_STALE_MS", Some("45000")),
            ],
            || {
                assert_eq!(configured_jito_tip_percentile(), "p95");
                assert_eq!(configured_helius_priority_level(), "Medium");
                assert_eq!(
                    duration_ms_u64(configured_helius_priority_refresh_interval()),
                    12_345
                );
                assert_eq!(
                    duration_ms_u64(configured_helius_priority_stale_duration()),
                    45_000
                );
                assert_eq!(
                    duration_ms_u64(configured_jito_tip_refresh_interval()),
                    2_000
                );
                assert_eq!(
                    duration_ms_u64(configured_jito_tip_stale_duration()),
                    45_000
                );
            },
        );
    }

    #[test]
    fn auto_fee_feed_defaults_match_documented_cadence() {
        with_env_vars(
            &[
                ("HELIUS_PRIORITY_REFRESH_INTERVAL_MS", None),
                ("TRENCH_HELIUS_PRIORITY_REFRESH_INTERVAL_MS", None),
                ("LAUNCHDECK_HELIUS_PRIORITY_REFRESH_INTERVAL_MS", None),
                ("HELIUS_PRIORITY_STALE_MS", None),
                ("TRENCH_HELIUS_PRIORITY_STALE_MS", None),
                ("JITO_TIP_REFRESH_INTERVAL_MS", None),
                ("TRENCH_JITO_TIP_REFRESH_INTERVAL_MS", None),
                ("JITO_TIP_STALE_MS", None),
                ("TRENCH_JITO_TIP_STALE_MS", None),
                ("AUTO_FEE_BUFFER_PERCENT", None),
                ("TRENCH_AUTO_FEE_BUFFER_PERCENT", None),
            ],
            || {
                assert_eq!(
                    duration_ms_u64(configured_helius_priority_refresh_interval()),
                    15_000
                );
                assert_eq!(
                    duration_ms_u64(configured_helius_priority_stale_duration()),
                    60_000
                );
                assert_eq!(
                    duration_ms_u64(configured_jito_tip_refresh_interval()),
                    2_000
                );
                assert_eq!(
                    duration_ms_u64(configured_jito_tip_stale_duration()),
                    45_000
                );
                assert_eq!(configured_auto_fee_buffer_bps(), 1_000);
            },
        );
    }

    #[test]
    fn buffered_resolution_keeps_standard_rpc_priority_only() {
        let output = resolve_buffered_auto_fee_components(AutoFeeResolutionInput {
            provider: "standard-rpc",
            execution_class: "manual",
            action: "buy",
            action_label: "Buy",
            max_fee_sol: "0.001",
            fallback_priority_fee_sol: "",
            fallback_tip_sol: "",
            snapshot_status: Some(fresh_status(FeeMarketSnapshot {
                helius_priority_lamports: Some(500_000),
                helius_launch_priority_lamports: Some(500_000),
                helius_trade_priority_lamports: None,
                jito_tip_p99_lamports: Some(500_000),
            })),
            allow_unavailable_fallback: false,
        })
        .expect("resolution");

        assert_eq!(output.priority_lamports, Some(550_000));
        assert_eq!(output.tip_lamports, None);
        assert_eq!(output.priority_source, "shared-helius-buffered");
        assert!(output.degradations.is_empty());
    }

    #[test]
    fn buffered_resolution_enforces_tip_floor_and_cap() {
        let output = resolve_buffered_auto_fee_components(AutoFeeResolutionInput {
            provider: "helius-sender",
            execution_class: "manual",
            action: "buy",
            action_label: "Buy",
            max_fee_sol: "0.001",
            fallback_priority_fee_sol: "",
            fallback_tip_sol: "",
            snapshot_status: Some(fresh_status(FeeMarketSnapshot {
                helius_priority_lamports: Some(900_000),
                helius_launch_priority_lamports: Some(900_000),
                helius_trade_priority_lamports: None,
                jito_tip_p99_lamports: Some(900_000),
            })),
            allow_unavailable_fallback: false,
        })
        .expect("resolution");

        assert_eq!(
            output.priority_lamports.unwrap() + output.tip_lamports.unwrap(),
            1_000_000
        );
        assert!(output.tip_lamports.unwrap() >= 200_000);
        assert_eq!(output.priority_estimate_lamports, Some(990_000));
        assert_eq!(output.tip_estimate_lamports, Some(990_000));
    }

    #[test]
    fn buffered_resolution_enforces_hellomoon_tip_floor_with_fresh_snapshot() {
        let output = resolve_buffered_auto_fee_components(AutoFeeResolutionInput {
            provider: "hellomoon",
            execution_class: "manual",
            action: "buy",
            action_label: "Buy",
            max_fee_sol: "0.03",
            fallback_priority_fee_sol: "0.001",
            fallback_tip_sol: "0.001",
            snapshot_status: Some(fresh_status(FeeMarketSnapshot {
                helius_priority_lamports: Some(15_000),
                helius_launch_priority_lamports: Some(50_000),
                helius_trade_priority_lamports: None,
                jito_tip_p99_lamports: Some(29_823),
            })),
            allow_unavailable_fallback: true,
        })
        .expect("resolution");

        assert_eq!(output.priority_lamports, Some(16_500));
        assert_eq!(output.tip_lamports, Some(1_000_000));
        assert_eq!(output.priority_source, "shared-helius-buffered");
        assert_eq!(output.tip_source, "shared-jito-buffered");
    }

    #[test]
    fn buffered_resolution_rejects_partial_fallback_below_hellomoon_tip_floor() {
        let error = resolve_buffered_auto_fee_components(AutoFeeResolutionInput {
            provider: "hellomoon",
            execution_class: "manual",
            action: "buy",
            action_label: "Buy",
            max_fee_sol: "0.0005",
            fallback_priority_fee_sol: "0.001",
            fallback_tip_sol: "0.001",
            snapshot_status: Some(snapshot_status(
                FeeMarketSnapshot {
                    helius_priority_lamports: Some(15_000),
                    helius_launch_priority_lamports: Some(50_000),
                    helius_trade_priority_lamports: None,
                    jito_tip_p99_lamports: Some(29_823),
                },
                false,
                true,
            )),
            allow_unavailable_fallback: true,
        })
        .expect_err("cap below minimum tip should fail");

        assert!(error.contains("max auto fee is below"));
        assert!(error.contains("hellomoon"));
    }

    #[test]
    fn buffered_resolution_allows_cap_equal_to_hellomoon_tip_floor() {
        let output = resolve_buffered_auto_fee_components(AutoFeeResolutionInput {
            provider: "hellomoon",
            execution_class: "manual",
            action: "buy",
            action_label: "Buy",
            max_fee_sol: "0.001",
            fallback_priority_fee_sol: "0.001",
            fallback_tip_sol: "0.001",
            snapshot_status: Some(fresh_status(FeeMarketSnapshot {
                helius_priority_lamports: Some(15_000),
                helius_launch_priority_lamports: Some(50_000),
                helius_trade_priority_lamports: None,
                jito_tip_p99_lamports: Some(29_823),
            })),
            allow_unavailable_fallback: true,
        })
        .expect("cap equal to minimum tip should resolve");

        assert_eq!(output.priority_lamports, Some(1));
        assert_eq!(output.tip_lamports, Some(1_000_000));
        assert_eq!(output.degradations.len(), 1);
        assert!(output.degradations[0].message.contains("minimum tip"));
        assert!(output.degradations[0].message.contains("1 lamport"));
    }

    #[test]
    fn unavailable_sources_fallback_to_default_and_emit_metadata() {
        let output = resolve_buffered_auto_fee_components(AutoFeeResolutionInput {
            provider: "helius-sender",
            execution_class: "manual",
            action: "sell",
            action_label: "Sell",
            max_fee_sol: "0.001",
            fallback_priority_fee_sol: "",
            fallback_tip_sol: "",
            snapshot_status: None,
            allow_unavailable_fallback: true,
        })
        .expect("resolution");

        assert_eq!(output.priority_lamports, Some(1_000_000));
        assert_eq!(output.tip_lamports, Some(1_000_000));
        assert_eq!(output.priority_source, "fallback-0.001");
        assert_eq!(output.tip_source, "fallback-0.001");
        assert_eq!(output.degradations.len(), 2);
        assert!(
            output
                .degradations
                .iter()
                .any(|entry| entry.source == "helius")
        );
        assert!(
            output
                .degradations
                .iter()
                .any(|entry| entry.source == "jito")
        );
    }

    #[test]
    fn unavailable_sources_use_user_fallback_values_when_present() {
        let output = resolve_buffered_auto_fee_components(AutoFeeResolutionInput {
            provider: "helius-sender",
            execution_class: "manual",
            action: "sell",
            action_label: "Sell",
            max_fee_sol: "0.001",
            fallback_priority_fee_sol: "0.002",
            fallback_tip_sol: "0.003",
            snapshot_status: None,
            allow_unavailable_fallback: true,
        })
        .expect("resolution");

        assert_eq!(output.priority_lamports, Some(2_000_000));
        assert_eq!(output.tip_lamports, Some(3_000_000));
        assert_eq!(output.priority_source, "fallback-user-priority");
        assert_eq!(output.tip_source, "fallback-user-tip");
        assert!(output.degradations.iter().any(|entry| {
            entry
                .message
                .contains("Defaulted priority fee to 0.002 SOL")
        }));
        assert!(
            output
                .degradations
                .iter()
                .any(|entry| entry.message.contains("Defaulted tip to 0.003 SOL"))
        );
    }

    #[test]
    fn cache_match_ignores_primary_rpc_url_for_cross_product_sharing() {
        let config = SharedFeeMarketConfig::new(
            PathBuf::from("shared-fee-market.json"),
            "https://primary-a.example".to_string(),
            "https://helius.example".to_string(),
            "test".to_string(),
            Vec::new(),
        );
        let cache = SharedFeeMarketCacheFile {
            schemaVersion: CACHE_SCHEMA_VERSION,
            primaryRpcUrl: "https://primary-b.example".to_string(),
            heliusPriorityRpcUrl: "https://helius.example".to_string(),
            heliusPriorityLevel: config.helius_priority_level.clone(),
            jitoTipPercentile: config.jito_tip_percentile.clone(),
            updatedAtUnixMs: 1,
            heliusUpdatedAtUnixMs: Some(1),
            jitoUpdatedAtUnixMs: Some(1),
            heliusLastError: None,
            jitoLastError: None,
            snapshot: FeeMarketSnapshot::default(),
        };

        assert!(config.matches_cache_file(&cache));
    }

    #[test]
    fn lease_paths_are_keyed_by_source_config() {
        let config_a = SharedFeeMarketConfig::new(
            PathBuf::from("shared-fee-market.json"),
            "https://primary.example".to_string(),
            "https://helius-a.example".to_string(),
            "test".to_string(),
            Vec::new(),
        );
        let mut config_b = config_a.clone();
        config_b.helius_priority_rpc_url = "https://helius-b.example".to_string();
        assert_ne!(config_a.lease_path("helius"), config_b.lease_path("helius"));

        config_b = config_a.clone();
        config_b.jito_tip_percentile = "p95".to_string();
        assert_ne!(config_a.lease_path("jito"), config_b.lease_path("jito"));

        config_b = config_a.clone();
        config_b.launch_account_keys = vec!["LaunchTemplateAccount".to_string()];
        assert_ne!(config_a.lease_path("helius"), config_b.lease_path("helius"));
    }

    #[test]
    fn cache_file_does_not_mark_missing_sources_fresh() {
        let config = SharedFeeMarketConfig::new(
            PathBuf::from("shared-fee-market.json"),
            "https://primary.example".to_string(),
            "https://helius.example".to_string(),
            "test".to_string(),
            Vec::new(),
        );
        let cache = cache_file_from_snapshot(
            &config,
            FeeMarketSnapshot {
                helius_priority_lamports: Some(42),
                helius_launch_priority_lamports: None,
                helius_trade_priority_lamports: None,
                jito_tip_p99_lamports: None,
            },
            Some(100),
            Some(100),
        );

        assert_eq!(cache.heliusUpdatedAtUnixMs, Some(100));
        assert_eq!(cache.jitoUpdatedAtUnixMs, None);
    }

    #[test]
    fn helius_incremental_update_preserves_existing_launch_estimate() {
        let path = std::env::temp_dir().join(format!(
            "shared-fee-market-merge-test-{}-{}.json",
            std::process::id(),
            unix_ms_now()
        ));
        let config = SharedFeeMarketConfig::new(
            path.clone(),
            "https://primary.example".to_string(),
            "https://helius.example".to_string(),
            "test".to_string(),
            Vec::new(),
        );
        let runtime = SharedFeeMarketRuntime::new(config);
        runtime
            .cache_full_snapshot(FeeMarketSnapshot {
                helius_priority_lamports: Some(100),
                helius_launch_priority_lamports: Some(200),
                helius_trade_priority_lamports: None,
                jito_tip_p99_lamports: None,
            })
            .expect("cache seed");
        runtime
            .update_helius_snapshot(&FeeMarketSnapshot {
                helius_priority_lamports: Some(300),
                helius_launch_priority_lamports: None,
                helius_trade_priority_lamports: None,
                jito_tip_p99_lamports: None,
            })
            .expect("incremental update");

        let status = runtime.read_snapshot_status().expect("status");
        assert_eq!(status.snapshot.helius_priority_lamports, Some(300));
        assert_eq!(status.snapshot.helius_launch_priority_lamports, Some(200));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn source_errors_preserve_existing_jito_tip_snapshot() {
        let path = std::env::temp_dir().join(format!(
            "shared-fee-market-jito-preserve-test-{}-{}.json",
            std::process::id(),
            unix_ms_now()
        ));
        let config = SharedFeeMarketConfig::new(
            path.clone(),
            "https://primary.example".to_string(),
            "https://helius.example".to_string(),
            "test".to_string(),
            Vec::new(),
        );
        let runtime = SharedFeeMarketRuntime::new(config);
        runtime
            .cache_full_snapshot(FeeMarketSnapshot {
                helius_priority_lamports: Some(100),
                helius_launch_priority_lamports: None,
                helius_trade_priority_lamports: None,
                jito_tip_p99_lamports: Some(200),
            })
            .expect("cache seed");
        runtime.record_jito_error("temporary jito error".to_string());

        let status = runtime.read_snapshot_status().expect("status");
        assert_eq!(status.snapshot.jito_tip_p99_lamports, Some(200));
        assert!(status.jito_fresh);
        assert_eq!(
            status.jito_last_error.as_deref(),
            Some("temporary jito error")
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn snapshot_status_uses_source_specific_stale_thresholds_and_diagnostics() {
        let path = std::env::temp_dir().join(format!(
            "shared-fee-market-test-{}-{}.json",
            std::process::id(),
            unix_ms_now()
        ));
        let mut config = SharedFeeMarketConfig::new(
            path.clone(),
            "https://primary.example".to_string(),
            "https://helius.example".to_string(),
            "test".to_string(),
            Vec::new(),
        );
        config.helius_max_age = Duration::from_millis(20_000);
        config.jito_max_age = Duration::from_millis(10_000);
        let now = unix_ms_now();
        let cache = SharedFeeMarketCacheFile {
            schemaVersion: CACHE_SCHEMA_VERSION,
            primaryRpcUrl: "https://primary.example".to_string(),
            heliusPriorityRpcUrl: "https://helius.example".to_string(),
            heliusPriorityLevel: config.helius_priority_level.clone(),
            jitoTipPercentile: config.jito_tip_percentile.clone(),
            updatedAtUnixMs: now,
            heliusUpdatedAtUnixMs: Some(now.saturating_sub(21_000)),
            jitoUpdatedAtUnixMs: Some(now.saturating_sub(9_000)),
            heliusLastError: Some("old helius error".to_string()),
            jitoLastError: None,
            snapshot: FeeMarketSnapshot {
                helius_priority_lamports: Some(1),
                helius_launch_priority_lamports: None,
                helius_trade_priority_lamports: None,
                jito_tip_p99_lamports: Some(2),
            },
        };
        write_cache_file(&path, &cache).expect("write cache");

        let status = read_shared_fee_market_snapshot(&config).expect("status");
        assert!(!status.helius_fresh);
        assert!(status.jito_fresh);
        assert!(status.helius_age_ms.unwrap_or_default() >= 21_000);
        assert!(status.jito_age_ms.unwrap_or_default() >= 9_000);
        assert_eq!(
            status.helius_last_error.as_deref(),
            Some("old helius error")
        );

        let payload = shared_fee_market_status_payload(&config);
        assert_eq!(payload["heliusFresh"], json!(false));
        assert_eq!(payload["jitoFresh"], json!(true));

        let _ = fs::remove_file(path);
    }
}
