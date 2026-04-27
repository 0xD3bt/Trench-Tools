use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub const DEFAULT_SHARED_DATA_ROOT: &str = ".local/trench-tools";
pub const SHARED_DATA_ROOT_ENV: &str = "TRENCH_TOOLS_DATA_ROOT";
pub const PROJECT_ROOT_ENV: &str = "TRENCH_TOOLS_PROJECT_ROOT";

const AUTH_STATE_VERSION: &str = "v1";
const DEFAULT_TOKEN_LABEL: &str = "Default bundled access";
const AUTH_STATE_FILE: &str = "auth-state.json";
const DEFAULT_TOKEN_FILE: &str = "default-engine-token.txt";
const LEGACY_EXECUTION_ENGINE_DATA_ROOT: &str = ".local/execution-engine";
const FILE_LOCK_RETRY_DELAY_MS: u64 = 50;
const FILE_LOCK_WAIT_TIMEOUT_MS: u64 = 15_000;
const LAST_USED_PERSIST_INTERVAL_MS: u128 = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthTokenRecord {
    id: String,
    label: String,
    token_hash: String,
    created_at_unix_ms: u128,
    #[serde(default)]
    last_used_at_unix_ms: Option<u128>,
    #[serde(default)]
    expires_at_unix_ms: Option<u128>,
    #[serde(default)]
    revoked_at_unix_ms: Option<u128>,
    #[serde(default)]
    is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthStateFile {
    version: String,
    #[serde(default)]
    tokens: Vec<AuthTokenRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthTokenSummary {
    pub id: String,
    pub label: String,
    pub created_at_unix_ms: u128,
    pub last_used_at_unix_ms: Option<u128>,
    pub expires_at_unix_ms: Option<u128>,
    pub revoked_at_unix_ms: Option<u128>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedAuthToken {
    pub token: String,
    pub token_summary: AuthTokenSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthBootstrapInfo {
    pub auth_required: bool,
    pub token_file_path: String,
    pub default_token_label: String,
    pub remote_secure_transport_required: bool,
}

/// Real OS-level advisory file lock. Uses `flock` on Unix and `LockFileEx`
/// on Windows via `fs2`. The lock is released when the guard is dropped
/// (including on panic) and on abnormal process exit, which means a crashed
/// holder does not leave the lock permanently held — unlike a sentinel-file
/// approach that relies on timed stale detection.
pub struct ExclusiveFileLock {
    file: Option<File>,
}

impl Drop for ExclusiveFileLock {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = FileExt::unlock(&file);
        }
    }
}

pub fn acquire_exclusive_file_lock(
    lock_path: &Path,
    label: &str,
) -> Result<ExclusiveFileLock, String> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|error| {
            format!(
                "Failed to open {label} lock file {}: {error}",
                lock_path.display()
            )
        })?;
    restrict_file_permissions(lock_path);
    let wait_started_at = Instant::now();
    loop {
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => {
                return Ok(ExclusiveFileLock { file: Some(file) });
            }
            Err(error) => {
                let kind = error.kind();
                // `try_lock_exclusive` returns `WouldBlock` (or the platform
                // equivalent) when another holder still owns the lock.
                if kind != ErrorKind::WouldBlock
                    && error.raw_os_error() != Some(libc_ewouldblock_or_eagain())
                {
                    return Err(format!(
                        "Failed to acquire {label} lock {}: {error}",
                        lock_path.display()
                    ));
                }
                if wait_started_at.elapsed() >= Duration::from_millis(FILE_LOCK_WAIT_TIMEOUT_MS) {
                    return Err(format!(
                        "Timed out waiting for {label} lock {}.",
                        lock_path.display()
                    ));
                }
                thread::sleep(Duration::from_millis(FILE_LOCK_RETRY_DELAY_MS));
            }
        }
    }
}

#[cfg(unix)]
const fn libc_ewouldblock_or_eagain() -> i32 {
    // On all Unix targets Rust supports, EAGAIN == EWOULDBLOCK, so checking
    // either is equivalent. Hard-coding 11 (EAGAIN on most glibc/musl) is a
    // best-effort shortcut; the primary check uses `ErrorKind::WouldBlock`
    // which maps across platforms.
    11
}

#[cfg(windows)]
const fn libc_ewouldblock_or_eagain() -> i32 {
    // ERROR_LOCK_VIOLATION is the typical raw OS error from LockFileEx when
    // another process holds the lock. Keep this in sync with the WouldBlock
    // ErrorKind mapping in the lock loop above.
    33
}

#[cfg(not(any(unix, windows)))]
const fn libc_ewouldblock_or_eagain() -> i32 {
    0
}

pub struct AuthManager {
    state_path: PathBuf,
    default_token_path: PathBuf,
    state: Mutex<AuthStateFile>,
    last_used_persist: Mutex<u128>,
}

impl AuthManager {
    pub fn new() -> Result<Self, String> {
        let root = shared_data_root();
        fs::create_dir_all(&root)
            .map_err(|error| format!("Failed to create auth root {}: {error}", root.display()))?;
        restrict_dir_permissions(&root);
        let state_path = root.join(AUTH_STATE_FILE);
        let default_token_path = root.join(DEFAULT_TOKEN_FILE);
        migrate_legacy_execution_engine_auth(&state_path, &default_token_path)?;
        // Sweep stray `tmp-*` temp files that a previous process may have left
        // behind if it crashed between opening the temp file and the final
        // rename. These are safe to delete: if another process is writing
        // concurrently it still holds the file open via OS-level locking.
        cleanup_stale_temp_files(&root);
        let state = load_existing_state_or_default(&state_path)?;
        let manager = Self {
            state_path,
            default_token_path,
            state: Mutex::new(state),
            last_used_persist: Mutex::new(0),
        };
        manager.ensure_default_token()?;
        restrict_file_permissions(&manager.state_path);
        restrict_file_permissions(&manager.default_token_path);
        Ok(manager)
    }

    pub fn bootstrap_info(&self) -> AuthBootstrapInfo {
        AuthBootstrapInfo {
            auth_required: true,
            token_file_path: self.default_token_path.display().to_string(),
            default_token_label: DEFAULT_TOKEN_LABEL.to_string(),
            remote_secure_transport_required: true,
        }
    }

    pub fn default_token(&self) -> Result<String, String> {
        read_token_from_file(&self.default_token_path).ok_or_else(|| {
            format!(
                "Failed to read default auth token from {}.",
                self.default_token_path.display()
            )
        })
    }

    pub fn list_tokens(&self) -> Vec<AuthTokenSummary> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let cached = state.clone();
        match load_auth_state_with_diagnostics(&self.state_path) {
            Ok(Some(mut latest)) => {
                for token in &mut latest.tokens {
                    if let Some(cached_token) =
                        cached.tokens.iter().find(|entry| entry.id == token.id)
                    {
                        if cached_token.last_used_at_unix_ms > token.last_used_at_unix_ms {
                            token.last_used_at_unix_ms = cached_token.last_used_at_unix_ms;
                        }
                    }
                }
                *state = latest;
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!(
                    "[shared-auth] failed to reload auth state from {}: {error}",
                    self.state_path.display()
                );
            }
        }
        state
            .tokens
            .iter()
            .cloned()
            .map(summary_from_record)
            .collect()
    }

    pub fn create_token(&self, label: &str) -> Result<CreatedAuthToken, String> {
        self.create_token_internal(label, false)
    }

    pub fn revoke_token(&self, token_id: &str) -> Result<AuthTokenSummary, String> {
        self.with_locked_state("revoke auth token", |state| {
            let index = state
                .tokens
                .iter()
                .position(|token| token.id == token_id)
                .ok_or_else(|| format!("Unknown auth token {token_id}"))?;
            if state.tokens[index].revoked_at_unix_ms.is_none() {
                state.tokens[index].revoked_at_unix_ms = Some(now_unix_ms());
            }
            Ok(summary_from_record(state.tokens[index].clone()))
        })
    }

    pub fn verify_token(&self, raw_token: &str) -> Result<AuthTokenSummary, String> {
        let normalized = raw_token.trim();
        if normalized.is_empty() {
            return Err("Missing bearer token.".to_string());
        }
        let hashed = hash_token(normalized);
        let _lock_guard =
            acquire_exclusive_file_lock(&state_lock_path(&self.state_path), "verify auth token")?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Reload under the same cross-process lock so a peer that revoked the
        // token cannot be observed as still-valid here.
        *state = load_existing_state_or_default(&self.state_path)?;
        let now = now_unix_ms();
        let index = state
            .tokens
            .iter()
            .position(|token| {
                constant_time_hex_eq(&token.token_hash, &hashed)
                    && token.revoked_at_unix_ms.is_none()
            })
            .ok_or_else(|| "Invalid or revoked auth token.".to_string())?;
        if state.tokens[index]
            .expires_at_unix_ms
            .is_some_and(|expires_at| expires_at <= now)
        {
            return Err("Auth token has expired.".to_string());
        }
        state.tokens[index].last_used_at_unix_ms = Some(now);
        let summary = summary_from_record(state.tokens[index].clone());
        // Throttle persistence so every request does not pay for a disk write.
        if self.should_persist_last_used(now) {
            if let Err(error) = persist_auth_state(&self.state_path, &state) {
                eprintln!(
                    "[shared-auth] failed to persist last-used timestamp to {}: {error}",
                    self.state_path.display()
                );
            }
        }
        Ok(summary)
    }

    fn should_persist_last_used(&self, now: u128) -> bool {
        let mut last = self
            .last_used_persist
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *last == 0 || now.saturating_sub(*last) >= LAST_USED_PERSIST_INTERVAL_MS {
            *last = now;
            true
        } else {
            false
        }
    }

    fn ensure_default_token(&self) -> Result<(), String> {
        self.with_locked_state("ensure default auth token", |state| {
            let final_token = if let Some(raw_token) =
                read_token_from_file(&self.default_token_path)
            {
                raw_token
            } else if let Some(raw_token) = read_token_from_file(&legacy_default_token_file_path())
            {
                self.write_default_token_with_race_recovery(&raw_token)?;
                read_token_from_file(&self.default_token_path).unwrap_or(raw_token)
            } else {
                let raw_token = generate_token();
                self.write_default_token_with_race_recovery(&raw_token)?;
                read_token_from_file(&self.default_token_path).unwrap_or(raw_token)
            };
            ensure_token_record_in_state(state, &final_token, true, DEFAULT_TOKEN_LABEL);
            Ok(())
        })
    }

    fn create_token_internal(
        &self,
        label: &str,
        is_default: bool,
    ) -> Result<CreatedAuthToken, String> {
        let normalized_label = if label.trim().is_empty() {
            if is_default {
                DEFAULT_TOKEN_LABEL.to_string()
            } else {
                format!("Connection {}", now_unix_ms())
            }
        } else {
            label.trim().to_string()
        };
        let raw_token = generate_token();
        self.with_locked_state("create auth token", |state| {
            if is_default {
                for token in &mut state.tokens {
                    token.is_default = false;
                }
            }
            let record = AuthTokenRecord {
                id: Uuid::new_v4().simple().to_string(),
                label: normalized_label,
                token_hash: hash_token(&raw_token),
                created_at_unix_ms: now_unix_ms(),
                last_used_at_unix_ms: None,
                expires_at_unix_ms: None,
                revoked_at_unix_ms: None,
                is_default,
            };
            let summary = summary_from_record(record.clone());
            state.tokens.push(record);
            Ok(CreatedAuthToken {
                token: raw_token,
                token_summary: summary,
            })
        })
    }

    fn write_default_token_with_race_recovery(&self, raw_token: &str) -> Result<(), String> {
        match create_new_file_with_contents(
            &self.default_token_path,
            format!("{raw_token}\n").as_bytes(),
        ) {
            Ok(true) => Ok(()),
            Ok(false) => read_token_from_file(&self.default_token_path)
                .map(|_| ())
                .ok_or_else(|| {
                    format!(
                        "Default auth token file {} already existed but could not be read.",
                        self.default_token_path.display()
                    )
                }),
            Err(error) => Err(error),
        }
    }

    fn with_locked_state<T, F>(&self, label: &str, action: F) -> Result<T, String>
    where
        F: FnOnce(&mut AuthStateFile) -> Result<T, String>,
    {
        let _lock_guard = acquire_exclusive_file_lock(&state_lock_path(&self.state_path), label)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state = load_existing_state_or_default(&self.state_path)?;
        let output = action(&mut state)?;
        persist_auth_state(&self.state_path, &state)?;
        Ok(output)
    }
}

pub fn shared_data_root() -> PathBuf {
    let configured = std::env::var(SHARED_DATA_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_SHARED_DATA_ROOT.to_string());
    resolve_configured_path(&configured)
}

pub fn default_token_file_path() -> PathBuf {
    shared_data_root().join(DEFAULT_TOKEN_FILE)
}

fn default_auth_state() -> AuthStateFile {
    AuthStateFile {
        version: AUTH_STATE_VERSION.to_string(),
        tokens: Vec::new(),
    }
}

fn resolve_configured_path(value: &str) -> PathBuf {
    let candidate = PathBuf::from(value.trim());
    if candidate.is_absolute() {
        candidate
    } else {
        relative_path_base().join(candidate)
    }
}

fn relative_path_base() -> PathBuf {
    if let Ok(configured_root) = env::var(PROJECT_ROOT_ENV) {
        let trimmed = configured_root.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.is_absolute() {
                return path;
            }
            // A non-empty relative PROJECT_ROOT is resolved against the
            // process CWD so scripted layouts that pass a relative value get
            // a predictable base instead of silently falling through to the
            // workspace-root fallback.
            if let Ok(cwd) = env::current_dir() {
                return cwd.join(path);
            }
        }
    }
    infer_workspace_root()
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn infer_workspace_root() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| find_workspace_root(path.as_path()))
        .or_else(|| {
            env::current_dir()
                .ok()
                .and_then(|path| find_workspace_root(path.as_path()))
        })
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let start_dir = if start.is_dir() {
        start
    } else {
        start.parent()?
    };
    start_dir.ancestors().find_map(|candidate| {
        (candidate.join("Cargo.toml").is_file() && candidate.join("rust").is_dir())
            .then(|| candidate.to_path_buf())
    })
}

fn legacy_execution_engine_root() -> PathBuf {
    resolve_configured_path(LEGACY_EXECUTION_ENGINE_DATA_ROOT)
}

fn legacy_default_token_file_path() -> PathBuf {
    legacy_execution_engine_root().join(DEFAULT_TOKEN_FILE)
}

fn legacy_auth_state_file_path() -> PathBuf {
    legacy_execution_engine_root().join(AUTH_STATE_FILE)
}

fn migrate_legacy_execution_engine_auth(
    shared_state_path: &Path,
    shared_token_path: &Path,
) -> Result<(), String> {
    if !shared_state_path.exists() {
        let legacy_state_path = legacy_auth_state_file_path();
        if legacy_state_path.exists() {
            let raw = fs::read(&legacy_state_path).map_err(|error| {
                format!(
                    "Failed to read legacy auth state {}: {error}",
                    legacy_state_path.display()
                )
            })?;
            atomic_write(shared_state_path, &raw)?;
        }
    }

    if !shared_token_path.exists() {
        let legacy_token_path = legacy_default_token_file_path();
        if legacy_token_path.exists() {
            let raw = fs::read(&legacy_token_path).map_err(|error| {
                format!(
                    "Failed to read legacy default token {}: {error}",
                    legacy_token_path.display()
                )
            })?;
            atomic_write(shared_token_path, &raw)?;
        }
    }

    Ok(())
}

fn load_auth_state_with_diagnostics(path: &Path) -> Result<Option<AuthStateFile>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Failed to read auth state {}: {error}",
                path.display()
            ));
        }
    };
    serde_json::from_str::<AuthStateFile>(&raw)
        .map(Some)
        .map_err(|error| {
            format!(
                "Auth state file {} contained invalid JSON: {error}",
                path.display()
            )
        })
}

fn load_existing_state_or_default(path: &Path) -> Result<AuthStateFile, String> {
    Ok(load_auth_state_with_diagnostics(path)?.unwrap_or_else(default_auth_state))
}

fn persist_auth_state(path: &Path, state: &AuthStateFile) -> Result<(), String> {
    let serialized = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("Failed to encode auth state: {error}"))?;
    atomic_write(path, &serialized)
}

fn cleanup_stale_temp_files(root: &Path) {
    // Stale-temp threshold: only delete files older than this window so we
    // never stomp on a write from a concurrent, still-running process.
    const STALE_AFTER: Duration = Duration::from_secs(60 * 60);
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let is_stray = name_str.contains(".tmp-")
            || name_str.contains(".rewrite-")
            || name_str.ends_with(".tmp");
        if !is_stray {
            continue;
        }
        let path = entry.path();
        let is_old = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .map(|modified| {
                SystemTime::now()
                    .duration_since(modified)
                    .map(|elapsed| elapsed >= STALE_AFTER)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if is_old {
            let _ = fs::remove_file(&path);
        }
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }
    let temp_path = path.with_extension(format!("tmp-{}-{}", std::process::id(), now_unix_ms()));
    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|error| format!("Failed to create {}: {error}", temp_path.display()))?;
        file.write_all(contents)
            .map_err(|error| format!("Failed to write {}: {error}", temp_path.display()))?;
        file.sync_all().ok();
    }
    restrict_file_permissions(&temp_path);
    // `fs::rename` atomically replaces an existing file on both Unix and
    // Windows (`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`), so we must
    // not `remove_file` first: a failure between the delete and the rename
    // would leave the canonical file missing entirely.
    fs::rename(&temp_path, path)
        .map_err(|error| format!("Failed to replace {}: {error}", path.display()))?;
    restrict_file_permissions(path);
    Ok(())
}

fn state_lock_path(state_path: &Path) -> PathBuf {
    state_path.with_extension("lock")
}

fn create_new_file_with_contents(path: &Path, contents: &[u8]) -> Result<bool, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(contents)
                .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
            file.sync_all().ok();
            drop(file);
            restrict_file_permissions(path);
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(format!("Failed to create {}: {error}", path.display())),
    }
}

fn read_token_from_file(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    raw.lines().find_map(|line| {
        let value = line.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn summary_from_record(record: AuthTokenRecord) -> AuthTokenSummary {
    AuthTokenSummary {
        id: record.id,
        label: record.label,
        created_at_unix_ms: record.created_at_unix_ms,
        last_used_at_unix_ms: record.last_used_at_unix_ms,
        expires_at_unix_ms: record.expires_at_unix_ms,
        revoked_at_unix_ms: record.revoked_at_unix_ms,
        is_default: record.is_default,
    }
}

fn ensure_token_record_in_state(
    state: &mut AuthStateFile,
    raw_token: &str,
    is_default: bool,
    label: &str,
) {
    let hashed = hash_token(raw_token);
    let now = now_unix_ms();
    if is_default {
        for token in &mut state.tokens {
            token.is_default = false;
        }
    }
    if let Some(existing) = state
        .tokens
        .iter_mut()
        .find(|token| constant_time_hex_eq(&token.token_hash, &hashed))
    {
        existing.is_default = is_default;
        existing.revoked_at_unix_ms = None;
        if existing.label.trim().is_empty() {
            existing.label = label.to_string();
        }
    } else {
        state.tokens.push(AuthTokenRecord {
            id: Uuid::new_v4().simple().to_string(),
            label: label.to_string(),
            token_hash: hashed,
            created_at_unix_ms: now,
            last_used_at_unix_ms: None,
            expires_at_unix_ms: None,
            revoked_at_unix_ms: None,
            is_default,
        });
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn generate_token() -> String {
    format!("tt_{}_{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn hash_token(raw_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Constant-time comparison for the fixed-length hex strings we store as
/// token hashes. Defends against local side-channel timing probes by making
/// the loop do the same amount of work regardless of where the first
/// mismatch appears.
fn constant_time_hex_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.as_bytes().iter().zip(b.as_bytes().iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        eprintln!(
            "[shared-auth] failed to restrict permissions on {}: {error}",
            path.display()
        );
    }
}

#[cfg(not(unix))]
fn restrict_file_permissions(path: &Path) {
    restrict_windows_path_permissions(path, false);
}

#[cfg(unix)]
fn restrict_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o700)) {
        eprintln!(
            "[shared-auth] failed to restrict directory permissions on {}: {error}",
            path.display()
        );
    }
}

#[cfg(not(unix))]
fn restrict_dir_permissions(path: &Path) {
    restrict_windows_path_permissions(path, true);
}

#[cfg(windows)]
fn restrict_windows_path_permissions(path: &Path, is_dir: bool) {
    let Some(user) = current_windows_user() else {
        return;
    };
    let grant = if is_dir {
        format!("{user}:(OI)(CI)F")
    } else {
        format!("{user}:F")
    };
    match std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(grant)
        .output()
    {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "[shared-auth] failed to restrict Windows ACLs on {}: {}",
                path.display(),
                stderr.trim()
            );
        }
        Err(error) => {
            eprintln!(
                "[shared-auth] failed to invoke icacls for {}: {error}",
                path.display()
            );
        }
    }
}

#[cfg(windows)]
fn current_windows_user() -> Option<String> {
    let username = env::var("USERNAME").ok()?.trim().to_string();
    if username.is_empty() {
        return None;
    }
    let domain = env::var("USERDOMAIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Some(match domain {
        Some(domain) => format!("{domain}\\{username}"),
        None => username,
    })
}

#[cfg(not(any(unix, windows)))]
fn restrict_windows_path_permissions(_path: &Path, _is_dir: bool) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn with_temp_auth_root<F>(test: F)
    where
        F: FnOnce(&Path),
    {
        static ENV_GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("lock env guard");
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("shared-auth-root");
        fs::create_dir_all(&root).expect("create auth root");
        unsafe {
            env::set_var(SHARED_DATA_ROOT_ENV, &root);
            env::set_var(PROJECT_ROOT_ENV, dir.path());
        }
        test(root.as_path());
        unsafe {
            env::remove_var(SHARED_DATA_ROOT_ENV);
            env::remove_var(PROJECT_ROOT_ENV);
        }
    }

    #[test]
    fn hashes_tokens_deterministically() {
        assert_eq!(hash_token("abc"), hash_token("abc"));
    }

    #[test]
    fn constant_time_hex_eq_matches_equal_strings() {
        assert!(constant_time_hex_eq("deadbeef", "deadbeef"));
        assert!(!constant_time_hex_eq("deadbeef", "deadbeee"));
        assert!(!constant_time_hex_eq("deadbeef", "deadbee"));
    }

    #[test]
    fn acquire_exclusive_file_lock_blocks_concurrent_holders() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.lock");
        let first = acquire_exclusive_file_lock(&path, "test").expect("first lock");
        let path_clone = path.clone();
        let handle = std::thread::spawn(move || acquire_exclusive_file_lock(&path_clone, "test"));
        std::thread::sleep(Duration::from_millis(100));
        drop(first);
        let second = handle.join().expect("join").expect("second lock");
        drop(second);
    }

    #[test]
    fn auth_manager_new_rejects_invalid_auth_state() {
        with_temp_auth_root(|root| {
            fs::write(root.join(AUTH_STATE_FILE), "{not-json").expect("write invalid auth state");

            let error = AuthManager::new()
                .err()
                .expect("invalid auth state should fail");
            assert!(error.contains("invalid JSON"), "{error}");
        });
    }

    #[test]
    fn mutating_auth_paths_do_not_overwrite_invalid_auth_state() {
        with_temp_auth_root(|root| {
            let manager = AuthManager::new().expect("create manager");
            fs::write(root.join(AUTH_STATE_FILE), "{not-json").expect("corrupt auth state");

            let error = manager
                .create_token("manual")
                .expect_err("mutating auth path should fail on invalid state");
            assert!(error.contains("invalid JSON"), "{error}");

            let raw =
                fs::read_to_string(root.join(AUTH_STATE_FILE)).expect("read corrupted state back");
            assert_eq!(raw, "{not-json");
        });
    }
}
