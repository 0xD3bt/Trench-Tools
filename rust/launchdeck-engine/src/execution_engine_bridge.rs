use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use shared_auth::{acquire_exclusive_file_lock, default_token_file_path, shared_data_root};
use tokio::time::sleep;

use crate::rpc::SentResult;

const OUTBOX_FILE_NAME: &str = "launchdeck-pending-ledger.jsonl";
const OUTBOX_FLUSH_CHUNK_SIZE: usize = 50;
const OUTBOX_MAX_ENTRIES: usize = 10_000;
const OUTBOX_OVERFLOW_ARCHIVE_PREFIX: &str = "launchdeck-pending-ledger-archive-";
const OUTBOX_FLUSHING_SUFFIX: &str = "flushing";
const OUTBOX_REWRITE_SUFFIX: &str = "rewrite";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeErrorKind {
    Transient,
    Permanent,
}

#[derive(Debug, Clone)]
struct BridgeError {
    kind: BridgeErrorKind,
    message: String,
    retry_signatures: Vec<String>,
}

impl BridgeError {
    fn transient(message: impl Into<String>) -> Self {
        Self {
            kind: BridgeErrorKind::Transient,
            message: message.into(),
            retry_signatures: Vec::new(),
        }
    }

    fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: BridgeErrorKind::Permanent,
            message: message.into(),
            retry_signatures: Vec::new(),
        }
    }

    fn is_retryable(&self) -> bool {
        matches!(self.kind, BridgeErrorKind::Transient)
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEngineConfirmedTradeRecord {
    pub wallet_key: String,
    pub mint: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionEngineConfirmedTradesRequest {
    trades: Vec<ExecutionEngineConfirmedTradeRecord>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionEngineConfirmedTradesResponse {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    errors: Vec<String>,
    #[serde(default)]
    recorded_count: u32,
    #[serde(default)]
    duplicate_count: u32,
    #[serde(default)]
    ignored_count: u32,
    #[serde(default)]
    transient_failures: Vec<ExecutionEngineConfirmedTradeFailure>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionEngineConfirmedTradeFailure {
    #[serde(default)]
    signature: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingTradeRecord {
    #[serde(flatten)]
    trade: ExecutionEngineConfirmedTradeRecord,
    enqueued_at_unix_ms: u128,
}

fn normalized_base_url_from_env(var_name: &str) -> Option<String> {
    std::env::var(var_name)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

fn configured_execution_engine_port() -> u16 {
    std::env::var("EXECUTION_ENGINE_PORT")
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(8788)
}

fn configured_execution_engine_base_url() -> Option<String> {
    normalized_base_url_from_env("LAUNCHDECK_EXECUTION_ENGINE_BASE_URL")
        .or_else(|| normalized_base_url_from_env("EXECUTION_ENGINE_BASE_URL"))
        .or_else(|| {
            Some(format!(
                "http://127.0.0.1:{}",
                configured_execution_engine_port()
            ))
        })
}

fn execution_engine_bridge_enabled() -> bool {
    configured_execution_engine_base_url().is_some()
}

fn shared_execution_engine_http_client() -> &'static Client {
    // Panic here if the reqwest client cannot be constructed: it means the
    // platform lacks TLS support, which is a deployment bug we want to surface
    // immediately rather than silently swapping in a client without a timeout.
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client for execution-engine bridge")
    })
}

fn outbox_path() -> PathBuf {
    shared_data_root().join(OUTBOX_FILE_NAME)
}

fn outbox_lock_path() -> PathBuf {
    outbox_path().with_extension("lock")
}

fn with_outbox_file_lock<T, F>(label: &str, action: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let _lock_guard = acquire_exclusive_file_lock(&outbox_lock_path(), label)?;
    action()
}

fn pending_file_path(prefix: &str) -> PathBuf {
    outbox_path().with_file_name(format!(
        "{OUTBOX_FILE_NAME}.{prefix}-{}-{}.jsonl",
        std::process::id(),
        now_unix_ms()
    ))
}

fn outbox_flushing_file_prefix() -> String {
    format!("{OUTBOX_FILE_NAME}.{OUTBOX_FLUSHING_SUFFIX}-")
}

fn is_outbox_flushing_file(name: &str) -> bool {
    name.starts_with(&outbox_flushing_file_prefix()) && name.ends_with(".jsonl")
}

fn outbox_overflow_archive_path() -> PathBuf {
    outbox_path().with_file_name(format!(
        "{OUTBOX_OVERFLOW_ARCHIVE_PREFIX}{}.jsonl",
        unix_day_stamp()
    ))
}

fn unix_day_stamp() -> String {
    let ms = now_unix_ms();
    let days = ms / 86_400_000;
    format!("{days}")
}

fn default_execution_engine_token_path() -> PathBuf {
    default_token_file_path()
}

fn read_token_from_file(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    raw.lines().find_map(|line| {
        let value = line.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn load_execution_engine_bearer_token() -> Option<String> {
    // Cache the resolved token briefly. Flush bursts issue back-to-back HTTP
    // requests; each request reading the token file from disk adds syscalls
    // and creates a race with an in-place token rotation. A short TTL keeps
    // rotations visible within a second while collapsing steady-state reads
    // into the cache.
    use std::sync::Mutex;
    use std::time::Instant;

    if let Ok(value) = std::env::var("LAUNCHDECK_EXECUTION_ENGINE_TOKEN") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    static CACHE: OnceLock<Mutex<Option<(Instant, Option<String>)>>> = OnceLock::new();
    const CACHE_TTL: Duration = Duration::from_secs(1);

    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let now = Instant::now();
    {
        let guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((cached_at, token)) = guard.as_ref() {
            if now.duration_since(*cached_at) < CACHE_TTL {
                return token.clone();
            }
        }
    }
    let explicit_path = std::env::var("LAUNCHDECK_EXECUTION_ENGINE_TOKEN_FILE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let token = explicit_path
        .as_ref()
        .and_then(|path| read_token_from_file(path))
        .or_else(|| read_token_from_file(&default_execution_engine_token_path()));
    let mut guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some((now, token.clone()));
    token
}

pub fn confirmed_trade_record_from_sent_result(
    result: &SentResult,
    wallet_key: &str,
    mint: &str,
    client_request_id: Option<&str>,
    batch_id: Option<&str>,
) -> Option<ExecutionEngineConfirmedTradeRecord> {
    if !matches!(
        result.confirmationStatus.as_deref(),
        Some("confirmed") | Some("finalized")
    ) {
        return None;
    }
    let signature = result.signature.as_deref()?.trim();
    let normalized_wallet_key = wallet_key.trim();
    let normalized_mint = mint.trim();
    if signature.is_empty() || normalized_wallet_key.is_empty() || normalized_mint.is_empty() {
        return None;
    }
    Some(ExecutionEngineConfirmedTradeRecord {
        wallet_key: normalized_wallet_key.to_string(),
        mint: normalized_mint.to_string(),
        signature: signature.to_string(),
        client_request_id: client_request_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        batch_id: batch_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

pub fn spawn_startup_outbox_flush_task(task_label: &'static str) {
    if !execution_engine_bridge_enabled() {
        return;
    }
    tokio::spawn(async move {
        sleep(Duration::from_secs(3)).await;
        if let Err(error) = flush_outbox().await {
            eprintln!(
                "[launchdeck][execution-engine-bridge][{task_label}] startup outbox flush failed: {error}"
            );
        }
    });
}

pub async fn record_confirmed_trades(
    trades: &[ExecutionEngineConfirmedTradeRecord],
) -> Result<(), String> {
    if trades.is_empty() || !execution_engine_bridge_enabled() {
        return Ok(());
    }
    if let Err(error) = flush_outbox().await {
        if error.is_retryable() {
            eprintln!(
                "[launchdeck][execution-engine-bridge] failed to flush pending ledger records before posting new trades: {error}"
            );
        } else {
            return Err(error.to_string());
        }
    }
    match post_confirmed_trades(trades).await {
        Ok(()) => Ok(()),
        Err(error) if error.is_retryable() => {
            let retryable_trades =
                filter_trades_for_transient_retry(trades, &error.retry_signatures);
            append_records_to_outbox(&retryable_trades).await?;
            eprintln!(
                "[launchdeck][execution-engine-bridge] execution-engine unreachable; queued {} retryable trade record(s) locally: {error}",
                retryable_trades.len()
            );
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

async fn flush_outbox() -> Result<(), BridgeError> {
    if !execution_engine_bridge_enabled() {
        return Ok(());
    }
    // Under the outbox lock, first recover any orphaned flushing files left by a
    // previously crashed flush (they would otherwise be invisible to future
    // flushes), then claim the current canonical outbox by renaming it.
    let outbox = outbox_path();
    let flushing_path =
        with_outbox_file_lock("claim pending execution-engine ledger outbox", || {
            recover_orphaned_flushing_files_locked(&outbox)?;
            rename_pending_file(&outbox, OUTBOX_FLUSHING_SUFFIX)
        })
        .map_err(BridgeError::permanent)?;
    let flushing_path = match flushing_path {
        Some(path) => path,
        None => return Ok(()),
    };
    let pending = read_pending_records(&flushing_path).map_err(BridgeError::permanent)?;
    if pending.is_empty() {
        let _ = fs::remove_file(&flushing_path);
        return Ok(());
    }
    let trades: Vec<ExecutionEngineConfirmedTradeRecord> =
        pending.iter().map(|entry| entry.trade.clone()).collect();
    let mut delivered_records = 0usize;
    for chunk in trades.chunks(OUTBOX_FLUSH_CHUNK_SIZE) {
        if let Err(error) = post_confirmed_trades(chunk).await {
            // Requeue only the portion of the current chunk the execution-engine
            // explicitly classified as transient, plus every chunk we have not
            // attempted yet.
            let chunk_end = delivered_records + chunk.len();
            let mut remainder = filter_pending_records_for_transient_retry(
                &pending[delivered_records..chunk_end],
                &error.retry_signatures,
            );
            remainder.extend(pending[chunk_end..].iter().cloned());
            let flushing_path_for_cleanup = flushing_path.clone();
            with_outbox_file_lock(
                "requeue pending execution-engine ledger outbox",
                move || {
                    merge_pending_records_into_outbox(&remainder)?;
                    // Remove the flushing file inside the same locked section so a
                    // crash after merge cannot re-queue the same records again.
                    match fs::remove_file(&flushing_path_for_cleanup) {
                        Ok(()) => Ok(()),
                        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
                        Err(err) => Err(format!(
                            "Failed to clear pending execution-engine ledger outbox {}: {err}",
                            flushing_path_for_cleanup.display()
                        )),
                    }
                },
            )
            .map_err(|merge_error| {
                BridgeError::permanent(format!(
                    "Execution-engine trade ledger flush failed and pending records could not be requeued: {merge_error}"
                ))
            })?;
            return Err(BridgeError {
                kind: error.kind,
                message: format!(
                    "Execution-engine trade ledger flush failed after delivering {delivered_records}/{total} record(s): {error}",
                    total = pending.len(),
                ),
                retry_signatures: error.retry_signatures.clone(),
            });
        }
        delivered_records = delivered_records.saturating_add(chunk.len());
    }
    fs::remove_file(&flushing_path).map_err(|error| {
        BridgeError::permanent(format!(
            "Failed to clear pending execution-engine ledger outbox {}: {error}",
            flushing_path.display()
        ))
    })?;
    Ok(())
}

fn recover_orphaned_flushing_files_locked(outbox: &Path) -> Result<(), String> {
    let parent = match outbox.parent() {
        Some(parent) => parent,
        None => return Ok(()),
    };
    if !parent.exists() {
        return Ok(());
    }
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Failed to scan pending execution-engine ledger directory {}: {error}",
                parent.display()
            ));
        }
    };
    let mut orphans: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        if let Some(name) = file_name.to_str() {
            if is_outbox_flushing_file(name) {
                orphans.push(entry.path());
            }
        }
    }
    if orphans.is_empty() {
        return Ok(());
    }
    let mut recovered: Vec<PendingTradeRecord> = Vec::new();
    let mut recovered_orphans: Vec<PathBuf> = Vec::new();
    let mut unreadable_orphans: Vec<PathBuf> = Vec::new();
    for orphan in &orphans {
        match read_pending_records(orphan) {
            Ok(records) => {
                recovered.extend(records);
                recovered_orphans.push(orphan.clone());
            }
            Err(error) => {
                eprintln!(
                    "[launchdeck][execution-engine-bridge] skipping unreadable orphan flushing file {}: {error}",
                    orphan.display()
                );
                unreadable_orphans.push(orphan.clone());
            }
        }
    }
    if !recovered.is_empty() {
        merge_pending_records_into_outbox(&recovered)?;
    }
    for orphan in &recovered_orphans {
        if let Err(error) = fs::remove_file(orphan) {
            if error.kind() != ErrorKind::NotFound {
                eprintln!(
                    "[launchdeck][execution-engine-bridge] failed to remove orphan flushing file {}: {error}",
                    orphan.display()
                );
            }
        }
    }
    if !unreadable_orphans.is_empty() {
        eprintln!(
            "[launchdeck][execution-engine-bridge] left {} unreadable orphan flushing file(s) in place for manual recovery.",
            unreadable_orphans.len()
        );
    }
    eprintln!(
        "[launchdeck][execution-engine-bridge] recovered {} orphan pending ledger record(s) from {} flushing file(s).",
        recovered.len(),
        recovered_orphans.len()
    );
    Ok(())
}

async fn post_confirmed_trades(
    trades: &[ExecutionEngineConfirmedTradeRecord],
) -> Result<(), BridgeError> {
    let Some(base_url) = configured_execution_engine_base_url() else {
        return Ok(());
    };
    let token = load_execution_engine_bearer_token().ok_or_else(|| {
        BridgeError::permanent(
            "Execution-engine bearer token unavailable (set LAUNCHDECK_EXECUTION_ENGINE_TOKEN or ensure the shared default token file exists).",
        )
    })?;
    let url = reqwest::Url::parse(&format!("{base_url}/api/launchdeck/trade-ledger/record"))
        .map_err(|error| {
            BridgeError::permanent(format!(
                "Execution-engine trade ledger URL was invalid ({base_url}): {error}"
            ))
        })?;
    let response = shared_execution_engine_http_client()
        .post(url)
        .bearer_auth(token)
        .json(&ExecutionEngineConfirmedTradesRequest {
            trades: trades.to_vec(),
        })
        .send()
        .await
        .map_err(|error| {
            BridgeError::transient(format!(
                "Execution-engine trade ledger request failed: {error}"
            ))
        })?;
    let status = response.status();
    let response_text = response.text().await.map_err(|error| {
        bridge_error_for_status(
            status,
            format!("Execution-engine trade ledger response could not be read: {error}"),
        )
    })?;
    let payload: ExecutionEngineConfirmedTradesResponse = serde_json::from_str(&response_text)
        .map_err(|error| {
            bridge_error_for_status(
                status,
                format!(
                    "Execution-engine trade ledger response was invalid: {error}. Body: {}",
                    trim_error_body(&response_text)
                ),
            )
        })?;
    if status.is_success() && payload.ok {
        return Ok(());
    }
    if status.is_success() {
        // Partial success. The execution engine splits row failures into two
        // buckets:
        //   * `errors` = permanent (malformed fields, unknown wallet,
        //     signature belonged to a different wallet, etc.). Retrying will
        //     not change the outcome, so we log them and drop them.
        //   * `transient_failures` = transient (RPC timeout, signature not yet
        //     confirmed, disk I/O hiccup). Only those rows should stay queued;
        //     successful rows and permanent failures should not be retried.
        if !payload.errors.is_empty() {
            eprintln!(
                "[launchdeck][execution-engine-bridge] execution-engine rejected {failed}/{total} ledger row(s) permanently (recorded={recorded}, duplicate={duplicate}, ignored={ignored}): {messages}",
                failed = payload.errors.len(),
                total = trades.len(),
                recorded = payload.recorded_count,
                duplicate = payload.duplicate_count,
                ignored = payload.ignored_count,
                messages = payload.errors.join(" | ")
            );
        }
        if !payload.transient_failures.is_empty() {
            let messages: Vec<String> = payload
                .transient_failures
                .iter()
                .map(|failure| failure.message.clone())
                .collect();
            let retry_signatures: Vec<String> = payload
                .transient_failures
                .iter()
                .map(|failure| failure.signature.trim().to_string())
                .filter(|signature| !signature.is_empty())
                .collect();
            return Err(BridgeError {
                kind: BridgeErrorKind::Transient,
                message: format!(
                    "Execution-engine reported {failed}/{total} transient ledger-record failure(s) (recorded={recorded}, duplicate={duplicate}, ignored={ignored}): {messages}",
                    failed = messages.len(),
                    total = trades.len(),
                    recorded = payload.recorded_count,
                    duplicate = payload.duplicate_count,
                    ignored = payload.ignored_count,
                    messages = messages.join(" | ")
                ),
                retry_signatures,
            });
        }
        return Ok(());
    }
    let message = if payload.errors.is_empty() {
        format!(
            "Execution-engine trade ledger record failed with HTTP {}.",
            status
        )
    } else {
        payload.errors.join(" | ")
    };
    Err(bridge_error_for_status(status, message))
}

fn filter_trades_for_transient_retry(
    trades: &[ExecutionEngineConfirmedTradeRecord],
    retry_signatures: &[String],
) -> Vec<ExecutionEngineConfirmedTradeRecord> {
    if retry_signatures.is_empty() {
        return trades.to_vec();
    }
    let retry_signature_set: HashSet<&str> = retry_signatures
        .iter()
        .map(String::as_str)
        .filter(|signature| !signature.is_empty())
        .collect();
    if retry_signature_set.is_empty() {
        return trades.to_vec();
    }
    let filtered: Vec<ExecutionEngineConfirmedTradeRecord> = trades
        .iter()
        .filter(|trade| retry_signature_set.contains(trade.signature.as_str()))
        .cloned()
        .collect();
    if filtered.is_empty() {
        eprintln!(
            "[launchdeck][execution-engine-bridge] execution-engine reported transient retry signatures that did not map back to the posted trade batch; requeueing the full batch."
        );
        return trades.to_vec();
    }
    filtered
}

fn filter_pending_records_for_transient_retry(
    records: &[PendingTradeRecord],
    retry_signatures: &[String],
) -> Vec<PendingTradeRecord> {
    if retry_signatures.is_empty() {
        return records.to_vec();
    }
    let retry_signature_set: HashSet<&str> = retry_signatures
        .iter()
        .map(String::as_str)
        .filter(|signature| !signature.is_empty())
        .collect();
    if retry_signature_set.is_empty() {
        return records.to_vec();
    }
    let filtered: Vec<PendingTradeRecord> = records
        .iter()
        .filter(|record| retry_signature_set.contains(record.trade.signature.as_str()))
        .cloned()
        .collect();
    if filtered.is_empty() {
        eprintln!(
            "[launchdeck][execution-engine-bridge] execution-engine reported transient retry signatures that did not map back to the pending outbox chunk; requeueing the full chunk."
        );
        return records.to_vec();
    }
    filtered
}

async fn append_records_to_outbox(
    trades: &[ExecutionEngineConfirmedTradeRecord],
) -> Result<(), String> {
    with_outbox_file_lock("append pending execution-engine ledger outbox", || {
        let path = outbox_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed to create pending execution-engine ledger outbox directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let file_already_exists = path.exists();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| {
                format!(
                    "Failed to open pending execution-engine ledger outbox {}: {error}",
                    path.display()
                )
            })?;
        let enqueued_at = now_unix_ms();
        for trade in trades {
            let line = serde_json::to_string(&PendingTradeRecord {
                trade: trade.clone(),
                enqueued_at_unix_ms: enqueued_at,
            })
            .map_err(|error| {
                format!("Failed to encode pending execution-engine trade record: {error}")
            })?;
            let serialized = format!("{line}\n");
            file.write_all(serialized.as_bytes()).map_err(|error| {
                format!(
                    "Failed to append pending execution-engine trade record to {}: {error}",
                    path.display()
                )
            })?;
        }
        drop(file);
        if !file_already_exists {
            restrict_file_permissions(&path);
        }
        if let Err(error) = trim_outbox_to_cap_locked(&path) {
            eprintln!(
                "[launchdeck][execution-engine-bridge] failed to trim pending ledger outbox to {} entries: {error}",
                OUTBOX_MAX_ENTRIES
            );
        }
        Ok(())
    })
}

fn trim_outbox_to_cap_locked(path: &Path) -> Result<(), String> {
    let mut pending = read_pending_records(path)?;
    let overflow = pending.len().saturating_sub(OUTBOX_MAX_ENTRIES);
    if overflow == 0 {
        return Ok(());
    }
    let overflow_records: Vec<PendingTradeRecord> = pending.drain(0..overflow).collect();
    archive_overflow_records(&overflow_records)?;
    write_pending_records(path, &pending)?;
    eprintln!(
        "[launchdeck][execution-engine-bridge] outbox exceeded {OUTBOX_MAX_ENTRIES} entries; archived {overflow} oldest pending ledger record(s) to {}.",
        outbox_overflow_archive_path().display()
    );
    Ok(())
}

fn merge_pending_records_into_outbox(records: &[PendingTradeRecord]) -> Result<(), String> {
    if records.is_empty() {
        return Ok(());
    }
    let path = outbox_path();
    let mut merged = records.to_vec();
    if path.exists() {
        merged.extend(read_pending_records(&path)?);
    }
    let overflow = merged.len().saturating_sub(OUTBOX_MAX_ENTRIES);
    if overflow > 0 {
        let overflow_records: Vec<PendingTradeRecord> = merged.drain(0..overflow).collect();
        archive_overflow_records(&overflow_records)?;
        eprintln!(
            "[launchdeck][execution-engine-bridge] outbox exceeded {OUTBOX_MAX_ENTRIES} entries during requeue; archived {overflow} oldest pending ledger record(s) to {}.",
            outbox_overflow_archive_path().display()
        );
    }
    write_pending_records(&path, &merged)
}

fn archive_overflow_records(records: &[PendingTradeRecord]) -> Result<(), String> {
    if records.is_empty() {
        return Ok(());
    }
    let path = outbox_overflow_archive_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create pending ledger archive directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| {
            format!(
                "Failed to open pending ledger archive {}: {error}",
                path.display()
            )
        })?;
    for record in records {
        let line = serde_json::to_string(record)
            .map_err(|error| format!("Failed to encode archived pending ledger record: {error}"))?;
        let serialized = format!("{line}\n");
        file.write_all(serialized.as_bytes()).map_err(|error| {
            format!(
                "Failed to append archived pending ledger record to {}: {error}",
                path.display()
            )
        })?;
    }
    drop(file);
    restrict_file_permissions(&path);
    Ok(())
}

fn rename_pending_file(path: &Path, prefix: &str) -> Result<Option<PathBuf>, String> {
    let next_path = pending_file_path(prefix);
    match fs::rename(path, &next_path) {
        Ok(()) => Ok(Some(next_path)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Failed to rename pending execution-engine ledger outbox {}: {error}",
            path.display()
        )),
    }
}

fn read_pending_records(path: &Path) -> Result<Vec<PendingTradeRecord>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path).map_err(|error| {
        format!(
            "Failed to read pending execution-engine ledger outbox {}: {error}",
            path.display()
        )
    })?;
    let mut pending = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record = serde_json::from_str::<PendingTradeRecord>(trimmed).map_err(|error| {
            format!(
                "Pending execution-engine ledger outbox {} contained invalid JSON on line {}: {error}",
                path.display(),
                index + 1
            )
        })?;
        pending.push(record);
    }
    Ok(pending)
}

fn write_pending_records(path: &Path, records: &[PendingTradeRecord]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create pending execution-engine ledger directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let temp_path = pending_file_path(OUTBOX_REWRITE_SUFFIX);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temp_path)
        .map_err(|error| {
            format!(
                "Failed to open temporary pending execution-engine ledger file {}: {error}",
                temp_path.display()
            )
        })?;
    for record in records {
        let line = serde_json::to_string(record).map_err(|error| {
            format!(
                "Failed to encode pending execution-engine trade record for {}: {error}",
                path.display()
            )
        })?;
        let serialized = format!("{line}\n");
        file.write_all(serialized.as_bytes()).map_err(|error| {
            format!(
                "Failed to write temporary pending execution-engine ledger file {}: {error}",
                temp_path.display()
            )
        })?;
    }
    drop(file);
    restrict_file_permissions(&temp_path);
    // `fs::rename` atomically replaces an existing file on both Unix and
    // Windows; the previous pre-delete created a window in which a failure
    // would leave the outbox missing entirely, so we skip it.
    fs::rename(&temp_path, path).map_err(|error| {
        format!(
            "Failed to replace pending execution-engine ledger outbox {}: {error}",
            path.display()
        )
    })
}

fn bridge_error_for_status(status: reqwest::StatusCode, message: impl Into<String>) -> BridgeError {
    if status.is_server_error() || matches!(status.as_u16(), 408 | 425 | 429) {
        BridgeError::transient(message)
    } else {
        BridgeError::permanent(message)
    }
}

fn trim_error_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        "<empty>".to_string()
    } else if trimmed.chars().count() > 240 {
        format!("{}...", trimmed.chars().take(240).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        eprintln!(
            "[launchdeck][execution-engine-bridge] failed to restrict permissions on {}: {error}",
            path.display()
        );
    }
}

#[cfg(not(unix))]
fn restrict_file_permissions(_path: &Path) {}
