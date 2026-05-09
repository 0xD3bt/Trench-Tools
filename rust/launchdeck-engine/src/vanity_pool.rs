#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solana_sdk::{
    hash::hash,
    signature::{Keypair, Signer},
};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    paths,
    rpc::{fetch_account_exists, fetch_multiple_account_exists},
};

#[allow(dead_code)]
const STALE_RESERVATION_MS: u64 = 30 * 60 * 1000;
#[cfg(test)]
static DISABLE_SUFFIX_CHECK_FOR_TESTS: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VanityLaunchpad {
    Pump,
    Bonk,
}

impl VanityLaunchpad {
    pub fn all() -> [Self; 2] {
        [Self::Pump, Self::Bonk]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pump => "pump",
            Self::Bonk => "bonk",
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Self::Pump => "pump",
            Self::Bonk => "bonk",
        }
    }
}

#[derive(Debug)]
pub struct ReservedVanityMint {
    pub keypair: Keypair,
    pub reservation: VanityReservation,
}

#[derive(Debug, Clone)]
pub struct VanityReservation {
    inner: Arc<Mutex<VanityReservationInner>>,
}

#[derive(Debug)]
struct VanityReservationInner {
    root: PathBuf,
    launchpad: VanityLaunchpad,
    public_key: String,
    key_hash: String,
    reservation_id: String,
    completed: bool,
}

impl VanityReservation {
    fn new(root: PathBuf, record: &VanityCandidate, reservation_id: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VanityReservationInner {
                root,
                launchpad: record.launchpad,
                public_key: record.public_key.clone(),
                key_hash: record.key_hash.clone(),
                reservation_id,
                completed: false,
            })),
        }
    }

    pub fn launchpad(&self) -> VanityLaunchpad {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .launchpad
    }

    pub fn public_key(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .public_key
            .clone()
    }

    pub fn key_hash(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .key_hash
            .clone()
    }

    pub fn reservation_id(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .reservation_id
            .clone()
    }

    pub fn mark_used(&self, signature: Option<&str>, error: Option<&str>) -> Result<(), String> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if inner.completed {
            return Ok(());
        }
        let _lock = LockGuard::acquire(&inner.root, inner.launchpad)?;
        let record = VanityStateRecord::new(
            inner.launchpad,
            &inner.public_key,
            &inner.key_hash,
            "used",
            Some(&inner.reservation_id),
            signature,
            error,
        );
        append_state_record_at(&inner.root, inner.launchpad, &record)?;
        compact_active_file_at(
            &inner.root,
            inner.launchpad,
            &inner.public_key,
            &inner.key_hash,
        )?;
        inner.completed = true;
        Ok(())
    }

    pub fn release(&self, error: Option<&str>) -> Result<(), String> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if inner.completed {
            return Ok(());
        }
        let _lock = LockGuard::acquire(&inner.root, inner.launchpad)?;
        let record = VanityStateRecord::new(
            inner.launchpad,
            &inner.public_key,
            &inner.key_hash,
            "released",
            Some(&inner.reservation_id),
            None,
            error,
        );
        append_state_record_at(&inner.root, inner.launchpad, &record)?;
        inner.completed = true;
        Ok(())
    }
}

impl Drop for VanityReservation {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) != 1 {
            return;
        }
        let _ = self.release(Some("reservation dropped before submit"));
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VanityStatusPayload {
    pub ok: bool,
    pub root: String,
    pub launchpads: Vec<VanityLaunchpadStatus>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct VanityLaunchpadStatus {
    pub launchpad: String,
    pub path: String,
    pub available: usize,
    pub reserved: usize,
    pub used: usize,
    pub invalid: usize,
    pub duplicates: usize,
    pub onChainUsed: usize,
    pub diagnostics: Vec<VanityLineDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VanityLineDiagnostic {
    pub launchpad: String,
    pub line: usize,
    pub publicKey: Option<String>,
    pub keyHash: Option<String>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
struct VanityCandidate {
    launchpad: VanityLaunchpad,
    line_number: usize,
    public_key: String,
    key_hash: String,
}

#[derive(Debug, Default, Clone)]
struct VanityPoolState {
    candidates: HashMap<VanityLaunchpad, Vec<VanityCandidate>>,
    status: VanityStatusPayload,
}

#[derive(Debug)]
pub struct VanityPoolManager {
    root: PathBuf,
    state: Mutex<VanityPoolState>,
}

impl VanityPoolManager {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: Mutex::new(VanityPoolState {
                candidates: HashMap::new(),
                status: VanityStatusPayload {
                    ok: true,
                    root: String::new(),
                    launchpads: vec![],
                },
            }),
        }
    }

    pub fn global() -> &'static Self {
        static MANAGER: OnceLock<VanityPoolManager> = OnceLock::new();
        MANAGER.get_or_init(|| VanityPoolManager::new(paths::vanity_dir()))
    }

    pub fn refresh(&self) -> Result<VanityStatusPayload, String> {
        ensure_templates_at(&self.root)?;
        let parsed = parse_all_launchpads(&self.root)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.candidates = parsed.candidates;
        state.status = parsed.status;
        Ok(state.status.clone())
    }

    #[allow(dead_code)]
    pub async fn refresh_with_rpc(&self, rpc_url: &str) -> Result<VanityStatusPayload, String> {
        ensure_templates_at(&self.root)?;
        recover_stale_reservations(&self.root, rpc_url).await?;
        let parsed = parse_all_launchpads(&self.root)?;
        mark_on_chain_used_candidates(&self.root, rpc_url, &parsed).await?;
        self.refresh()
    }

    #[allow(dead_code)]
    pub fn status_payload(&self) -> Result<Value, String> {
        let status = self.refresh()?;
        serde_json::to_value(status).map_err(|error| error.to_string())
    }

    pub async fn reserve_next(
        &self,
        launchpad: VanityLaunchpad,
        rpc_url: &str,
    ) -> Result<Option<ReservedVanityMint>, String> {
        ensure_templates_at(&self.root)?;
        let (mut candidates, refresh_released_reservations) = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let candidates = state
                .candidates
                .get(&launchpad)
                .cloned()
                .unwrap_or_default();
            let has_reserved = state
                .status
                .launchpads
                .iter()
                .find(|status| status.launchpad == launchpad.as_str())
                .is_some_and(|status| status.reserved > 0);
            (candidates, has_reserved)
        };
        if candidates.is_empty() {
            if !refresh_released_reservations {
                return Ok(None);
            }
            self.refresh()?;
            candidates = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .candidates
                .get(&launchpad)
                .cloned()
                .unwrap_or_default();
            if candidates.is_empty() {
                return Ok(None);
            }
        }

        for candidate in candidates {
            let _lock = LockGuard::acquire(&self.root, launchpad)?;
            let latest = read_latest_state_at(&self.root, launchpad)?;
            if matches!(
                latest
                    .get(&candidate.key_hash)
                    .map(|record| record.status.as_str()),
                Some("reserved" | "used" | "on-chain-used")
            ) {
                continue;
            }
            let Some(parsed) = find_candidate_in_file(&self.root, &candidate)? else {
                continue;
            };
            match fetch_account_exists(rpc_url, &parsed.public_key, "confirmed").await {
                Ok(true) => {
                    let record = VanityStateRecord::new(
                        launchpad,
                        &parsed.public_key,
                        &parsed.key_hash,
                        "on-chain-used",
                        None,
                        None,
                        Some("mint account already exists"),
                    );
                    append_state_record_at(&self.root, launchpad, &record)?;
                    compact_active_file_at(
                        &self.root,
                        launchpad,
                        &parsed.public_key,
                        &parsed.key_hash,
                    )?;
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    return Err(format!(
                        "Unable to verify queued vanity mint {} availability: {error}",
                        parsed.public_key
                    ));
                }
            }

            let reservation_id = uuid::Uuid::new_v4().to_string();
            let record = VanityStateRecord::new(
                launchpad,
                &parsed.public_key,
                &parsed.key_hash,
                "reserved",
                Some(&reservation_id),
                None,
                None,
            );
            append_state_record_at(&self.root, launchpad, &record)?;
            self.refresh()?;
            return Ok(Some(ReservedVanityMint {
                keypair: parsed.keypair,
                reservation: VanityReservation::new(self.root.clone(), &candidate, reservation_id),
            }));
        }
        self.refresh()?;
        Ok(None)
    }
}

impl Default for VanityStatusPayload {
    fn default() -> Self {
        Self {
            ok: true,
            root: String::new(),
            launchpads: vec![],
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedPools {
    candidates: HashMap<VanityLaunchpad, Vec<VanityCandidate>>,
    status: VanityStatusPayload,
}

#[derive(Debug)]
struct ParsedLine {
    keypair: Keypair,
    public_key: String,
    key_hash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct VanityStateRecord {
    ts_ms: u64,
    launchpad: String,
    publicKey: String,
    keyHash: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reservationId: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl VanityStateRecord {
    fn new(
        launchpad: VanityLaunchpad,
        public_key: &str,
        key_hash: &str,
        status: &str,
        reservation_id: Option<&str>,
        signature: Option<&str>,
        error: Option<&str>,
    ) -> Self {
        Self {
            ts_ms: current_time_ms(),
            launchpad: launchpad.as_str().to_string(),
            publicKey: public_key.to_string(),
            keyHash: key_hash.to_string(),
            status: status.to_string(),
            reservationId: reservation_id.map(str::to_string),
            signature: signature.map(str::to_string),
            error: error.map(str::to_string),
        }
    }
}

struct LockGuard {
    path: PathBuf,
}

impl LockGuard {
    fn acquire(root: &Path, launchpad: VanityLaunchpad) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|error| {
            format!(
                "Failed to create vanity directory {}: {error}",
                root.display()
            )
        })?;
        let path = root.join(format!("{}.lock", launchpad.as_str()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let _ = writeln!(file, "pid={}", std::process::id());
                Ok(Self { path })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if lock_is_stale(&path) {
                    let _ = fs::remove_file(&path);
                    return Self::acquire(root, launchpad);
                }
                Err(format!(
                    "Vanity queue for {} is currently locked.",
                    launchpad.as_str()
                ))
            }
            Err(error) => Err(format!(
                "Failed to lock vanity queue {}: {error}",
                launchpad.as_str()
            )),
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|age| age > Duration::from_secs(30))
        .unwrap_or(false)
}

#[allow(dead_code)]
pub fn preload_vanity_pool() -> Result<Value, String> {
    VanityPoolManager::global().status_payload()
}

#[allow(dead_code)]
pub async fn refresh_vanity_pool_with_rpc(rpc_url: &str) -> Result<Value, String> {
    let status = VanityPoolManager::global()
        .refresh_with_rpc(rpc_url)
        .await?;
    serde_json::to_value(status).map_err(|error| error.to_string())
}

#[allow(dead_code)]
pub fn vanity_pool_status_payload() -> Result<Value, String> {
    VanityPoolManager::global().status_payload()
}

pub async fn reserve_vanity_mint(
    launchpad: VanityLaunchpad,
    rpc_url: &str,
) -> Result<Option<ReservedVanityMint>, String> {
    VanityPoolManager::global()
        .reserve_next(launchpad, rpc_url)
        .await
}

pub fn mark_vanity_reservation_used(
    reservation: Option<&VanityReservation>,
    signature: Option<&str>,
) -> Result<(), String> {
    if let Some(reservation) = reservation {
        reservation.mark_used(signature, None)?;
    }
    Ok(())
}

pub fn append_vanity_report_note(report: &mut Value, reservation: Option<&VanityReservation>) {
    let Some(reservation) = reservation else {
        return;
    };
    let note = json!({
        "source": "file-queue",
        "launchpad": reservation.launchpad().as_str(),
        "publicKey": reservation.public_key(),
        "keyHash": reservation.key_hash(),
        "reservationId": reservation.reservation_id(),
    });
    report["vanityMint"] = note;
}

fn parse_all_launchpads(root: &Path) -> Result<ParsedPools, String> {
    let mut candidates: HashMap<VanityLaunchpad, Vec<VanityCandidate>> = HashMap::new();
    let mut statuses = Vec::new();
    let mut seen_public_keys = HashSet::new();
    let mut seen_hashes = HashSet::new();

    for launchpad in VanityLaunchpad::all() {
        let path = queue_path_at(root, launchpad);
        let latest = read_latest_state_at(root, launchpad)?;
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut status = VanityLaunchpadStatus {
            launchpad: launchpad.as_str().to_string(),
            path: path.display().to_string(),
            ..Default::default()
        };
        let mut launchpad_candidates = Vec::new();
        for (index, line) in content.lines().enumerate() {
            let line_number = index + 1;
            let Some(_raw) = extract_key_text(line) else {
                continue;
            };
            let parsed = match parse_key_line(line, launchpad) {
                Ok(value) => value,
                Err(message) => {
                    status.invalid = status.invalid.saturating_add(1);
                    status.diagnostics.push(diagnostic(
                        launchpad,
                        line_number,
                        None,
                        None,
                        "invalid",
                        message,
                    ));
                    continue;
                }
            };
            if !seen_public_keys.insert(parsed.public_key.clone())
                || !seen_hashes.insert(parsed.key_hash.clone())
            {
                status.duplicates = status.duplicates.saturating_add(1);
                status.diagnostics.push(diagnostic(
                    launchpad,
                    line_number,
                    Some(parsed.public_key),
                    Some(parsed.key_hash),
                    "duplicate",
                    "Duplicate vanity key/public key; only the first occurrence can be selected.",
                ));
                continue;
            }
            match latest
                .get(&parsed.key_hash)
                .map(|record| record.status.as_str())
            {
                Some("used") => {
                    status.used = status.used.saturating_add(1);
                    continue;
                }
                Some("on-chain-used") => {
                    status.onChainUsed = status.onChainUsed.saturating_add(1);
                    continue;
                }
                Some("reserved") => {
                    status.reserved = status.reserved.saturating_add(1);
                    continue;
                }
                _ => {}
            }
            status.available = status.available.saturating_add(1);
            launchpad_candidates.push(VanityCandidate {
                launchpad,
                line_number,
                public_key: parsed.public_key,
                key_hash: parsed.key_hash,
            });
        }
        candidates.insert(launchpad, launchpad_candidates);
        statuses.push(status);
    }

    Ok(ParsedPools {
        candidates,
        status: VanityStatusPayload {
            ok: true,
            root: root.display().to_string(),
            launchpads: statuses,
        },
    })
}

async fn mark_on_chain_used_candidates(
    root: &Path,
    rpc_url: &str,
    parsed: &ParsedPools,
) -> Result<(), String> {
    for launchpad in VanityLaunchpad::all() {
        let candidates = parsed
            .candidates
            .get(&launchpad)
            .cloned()
            .unwrap_or_default();
        if candidates.is_empty() {
            continue;
        }
        let accounts = candidates
            .iter()
            .map(|candidate| candidate.public_key.clone())
            .collect::<Vec<_>>();
        let exists = fetch_multiple_account_exists(rpc_url, &accounts, "confirmed").await?;
        for (candidate, exists) in candidates.iter().zip(exists.into_iter()) {
            if !exists {
                continue;
            }
            let _lock = LockGuard::acquire(root, launchpad)?;
            let latest = read_latest_state_at(root, launchpad)?;
            if matches!(
                latest
                    .get(&candidate.key_hash)
                    .map(|record| record.status.as_str()),
                Some("reserved" | "used" | "on-chain-used")
            ) {
                continue;
            }
            let record = VanityStateRecord::new(
                launchpad,
                &candidate.public_key,
                &candidate.key_hash,
                "on-chain-used",
                None,
                None,
                Some("mint account already exists"),
            );
            append_state_record_at(root, launchpad, &record)?;
            compact_active_file_at(root, launchpad, &candidate.public_key, &candidate.key_hash)?;
        }
    }
    Ok(())
}

fn diagnostic(
    launchpad: VanityLaunchpad,
    line: usize,
    public_key: Option<String>,
    key_hash: Option<String>,
    code: &str,
    message: impl Into<String>,
) -> VanityLineDiagnostic {
    VanityLineDiagnostic {
        launchpad: launchpad.as_str().to_string(),
        line,
        publicKey: public_key,
        keyHash: key_hash,
        code: code.to_string(),
        message: message.into(),
    }
}

fn parse_key_line(line: &str, launchpad: VanityLaunchpad) -> Result<ParsedLine, String> {
    let raw = extract_key_text(line).ok_or_else(|| "Line is empty.".to_string())?;
    if raw.chars().any(char::is_whitespace) {
        return Err(
            "Queue entries must be one base58 keypair token before any comment.".to_string(),
        );
    }
    if raw.starts_with('[') {
        return Err("JSON byte-array keypairs are not accepted in queue files; paste the base58 64-byte keypair string.".to_string());
    }
    let bytes = bs58::decode(raw)
        .into_vec()
        .map_err(|error| format!("Invalid base58 full keypair/private key: {error}"))?;
    if bytes.len() != 64 {
        return Err(format!(
            "Expected a 64-byte Solana keypair, got {} bytes.",
            bytes.len()
        ));
    }
    let keypair = Keypair::try_from(bytes.as_slice())
        .map_err(|error| format!("Invalid Solana keypair: {error}"))?;
    let public_key = keypair.pubkey().to_string();
    if !suffix_check_disabled_for_tests() && !public_key.ends_with(launchpad.suffix()) {
        return Err(format!(
            "Derived public key {public_key} does not end with required suffix {}.",
            launchpad.suffix()
        ));
    }
    let key_hash = redacted_key_hash(&bytes);
    Ok(ParsedLine {
        keypair,
        public_key,
        key_hash,
    })
}

fn find_candidate_in_file(
    root: &Path,
    candidate: &VanityCandidate,
) -> Result<Option<ParsedLine>, String> {
    let content = fs::read_to_string(queue_path_at(root, candidate.launchpad)).unwrap_or_default();
    for (index, line) in content.lines().enumerate() {
        if index + 1 != candidate.line_number && !line.contains(&candidate.public_key) {
            // Fast path for unchanged files, while still allowing comment-free lines below.
        }
        let parsed = match parse_key_line(line, candidate.launchpad) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if parsed.public_key == candidate.public_key || parsed.key_hash == candidate.key_hash {
            return Ok(Some(parsed));
        }
    }
    Ok(None)
}

fn extract_key_text(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    for (index, ch) in line.char_indices() {
        if ch == '#' {
            let before = &line[..index];
            if before.chars().last().is_some_and(char::is_whitespace) {
                let key = before.trim();
                return (!key.is_empty()).then_some(key);
            }
        }
    }
    Some(trimmed)
}

fn redacted_key_hash(bytes: &[u8]) -> String {
    hash(bytes).to_string().chars().take(16).collect()
}

fn read_latest_state_at(
    root: &Path,
    launchpad: VanityLaunchpad,
) -> Result<HashMap<String, VanityStateRecord>, String> {
    let path = used_path_at(root, launchpad);
    let content = fs::read_to_string(&path).unwrap_or_default();
    let mut latest = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<VanityStateRecord>(trimmed) else {
            continue;
        };
        latest.insert(record.keyHash.clone(), record);
    }
    Ok(latest)
}

fn append_state_record_at(
    root: &Path,
    launchpad: VanityLaunchpad,
    record: &VanityStateRecord,
) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| {
        format!(
            "Failed to create vanity directory {}: {error}",
            root.display()
        )
    })?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(used_path_at(root, launchpad))
        .map_err(|error| format!("Failed to open vanity used state: {error}"))?;
    let line = serde_json::to_string(record).map_err(|error| error.to_string())?;
    writeln!(file, "{line}").map_err(|error| format!("Failed to append vanity state: {error}"))
}

fn compact_active_file_at(
    root: &Path,
    launchpad: VanityLaunchpad,
    public_key: &str,
    key_hash: &str,
) -> Result<(), String> {
    let path = queue_path_at(root, launchpad);
    let content = fs::read_to_string(&path).unwrap_or_default();
    let had_trailing_newline = content.ends_with('\n');
    let mut removed = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        if !removed
            && let Ok(parsed) = parse_key_line(line, launchpad)
            && (parsed.public_key == public_key || parsed.key_hash == key_hash)
        {
            removed = true;
            continue;
        }
        lines.push(line.to_string());
    }
    if !removed {
        return Ok(());
    }
    let mut next = lines.join("\n");
    if had_trailing_newline && !next.is_empty() {
        next.push('\n');
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, next).map_err(|error| {
        format!(
            "Failed to write compacted vanity queue {}: {error}",
            tmp.display()
        )
    })?;
    fs::rename(&tmp, &path).map_err(|error| {
        format!(
            "Failed to replace compacted vanity queue {}: {error}",
            path.display()
        )
    })
}

#[allow(dead_code)]
async fn recover_stale_reservations(root: &Path, rpc_url: &str) -> Result<(), String> {
    for launchpad in VanityLaunchpad::all() {
        let _lock = match LockGuard::acquire(root, launchpad) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let latest = read_latest_state_at(root, launchpad)?;
        for record in latest.values() {
            if record.status != "reserved" {
                continue;
            }
            if current_time_ms().saturating_sub(record.ts_ms) < STALE_RESERVATION_MS {
                continue;
            }
            let exists = fetch_account_exists(rpc_url, &record.publicKey, "confirmed").await?;
            let status = if exists { "on-chain-used" } else { "released" };
            let error = if exists {
                Some("stale reservation recovered as on-chain used")
            } else {
                Some("stale reservation released")
            };
            let next = VanityStateRecord::new(
                launchpad,
                &record.publicKey,
                &record.keyHash,
                status,
                record.reservationId.as_deref(),
                None,
                error,
            );
            append_state_record_at(root, launchpad, &next)?;
            if exists {
                compact_active_file_at(root, launchpad, &record.publicKey, &record.keyHash)?;
            }
        }
    }
    Ok(())
}

fn ensure_templates_at(root: &Path) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| {
        format!(
            "Failed to create vanity directory {}: {error}",
            root.display()
        )
    })?;
    for launchpad in VanityLaunchpad::all() {
        let path = queue_path_at(root, launchpad);
        if path.exists() {
            continue;
        }
        fs::write(&path, template_for(launchpad)).map_err(|error| {
            format!(
                "Failed to create vanity queue template {}: {error}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn template_for(launchpad: VanityLaunchpad) -> String {
    format!(
        "# LaunchDeck {} vanity mint queue\n\
         # Paste one base58-encoded 64-byte Solana keypair per line. JSON arrays, base64, seed phrases, and public keys are rejected.\n\
         # Required public key suffix: {}\n\
         # Blank lines and lines starting with # are ignored.\n\
         # No commas, brackets, quotes, or labels are required.\n\
         # Correct shape:\n\
         # <base58_64_byte_keypair_private_key>\n\
         # <base58_64_byte_keypair_private_key>\n\
         # Optional comments are allowed after whitespace plus #:\n\
         # <base58_64_byte_keypair_private_key> # optional derived public mint address\n",
        launchpad.as_str(),
        launchpad.suffix()
    )
}

fn queue_path_at(root: &Path, launchpad: VanityLaunchpad) -> PathBuf {
    root.join(format!("{}.txt", launchpad.as_str()))
}

fn used_path_at(root: &Path, launchpad: VanityLaunchpad) -> PathBuf {
    root.join(format!("{}.used.jsonl", launchpad.as_str()))
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
fn suffix_check_disabled_for_tests() -> bool {
    DISABLE_SUFFIX_CHECK_FOR_TESTS.load(Ordering::SeqCst)
}

#[cfg(not(test))]
fn suffix_check_disabled_for_tests() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn suffix_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "launchdeck-vanity-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn with_suffix_check_disabled(test: impl FnOnce()) {
        let _guard = suffix_test_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        DISABLE_SUFFIX_CHECK_FOR_TESTS.store(true, Ordering::SeqCst);
        test();
        DISABLE_SUFFIX_CHECK_FOR_TESTS.store(false, Ordering::SeqCst);
    }

    #[test]
    fn parses_comments_and_valid_suffixes() {
        with_suffix_check_disabled(|| {
            let root = temp_root("parse");
            ensure_templates_at(&root).expect("templates");
            let keypair = Keypair::new();
            fs::write(
                queue_path_at(&root, VanityLaunchpad::Pump),
                format!(
                    "# comment\n{}\n\n{} # {}\n",
                    bs58::encode(keypair.to_bytes()).into_string(),
                    bs58::encode(keypair.to_bytes()).into_string(),
                    keypair.pubkey()
                ),
            )
            .expect("write queue");

            let status = VanityPoolManager::new(root.clone())
                .refresh()
                .expect("status");
            let pump = status
                .launchpads
                .iter()
                .find(|entry| entry.launchpad == "pump")
                .expect("pump status");
            assert_eq!(pump.available, 1);
            assert_eq!(pump.duplicates, 1);
            let _ = fs::remove_dir_all(root);
        });
    }

    #[test]
    fn rejects_wrong_suffix() {
        let _guard = suffix_test_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        DISABLE_SUFFIX_CHECK_FOR_TESTS.store(false, Ordering::SeqCst);
        let root = temp_root("suffix");
        ensure_templates_at(&root).expect("templates");
        let keypair = Keypair::new();
        fs::write(
            queue_path_at(&root, VanityLaunchpad::Pump),
            format!("{}\n", bs58::encode(keypair.to_bytes()).into_string()),
        )
        .expect("write queue");

        let status = VanityPoolManager::new(root.clone())
            .refresh()
            .expect("status");
        let pump = status
            .launchpads
            .iter()
            .find(|entry| entry.launchpad == "pump")
            .expect("pump status");
        assert_eq!(pump.available, 0);
        assert_eq!(pump.invalid, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_json_array_queue_entries() {
        let _guard = suffix_test_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        DISABLE_SUFFIX_CHECK_FOR_TESTS.store(false, Ordering::SeqCst);
        let root = temp_root("json-array");
        ensure_templates_at(&root).expect("templates");
        fs::write(
            queue_path_at(&root, VanityLaunchpad::Pump),
            "[1,2,3,4] # not accepted in queue files\n",
        )
        .expect("write queue");

        let status = VanityPoolManager::new(root.clone())
            .refresh()
            .expect("status");
        let pump = status
            .launchpads
            .iter()
            .find(|entry| entry.launchpad == "pump")
            .expect("pump status");
        assert_eq!(pump.available, 0);
        assert_eq!(pump.invalid, 1);
        assert!(
            pump.diagnostics
                .first()
                .map(|entry| entry.message.contains("JSON byte-array"))
                .unwrap_or(false)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn used_reservation_removes_only_consumed_line() {
        with_suffix_check_disabled(|| {
            let root = temp_root("used");
            ensure_templates_at(&root).expect("templates");
            let keypair = Keypair::new();
            let encoded = bs58::encode(keypair.to_bytes()).into_string();
            fs::write(
                queue_path_at(&root, VanityLaunchpad::Pump),
                format!("# keep\n{encoded} # {}\n# keep2\n", keypair.pubkey()),
            )
            .expect("write queue");
            let parsed = parse_key_line(&encoded, VanityLaunchpad::Pump).expect("parsed");
            let candidate = VanityCandidate {
                launchpad: VanityLaunchpad::Pump,
                line_number: 2,
                public_key: parsed.public_key,
                key_hash: parsed.key_hash,
            };
            let reservation =
                VanityReservation::new(root.clone(), &candidate, "reservation".to_string());
            reservation.mark_used(Some("sig"), None).expect("used");
            let content =
                fs::read_to_string(queue_path_at(&root, VanityLaunchpad::Pump)).expect("queue");
            assert!(content.contains("# keep"));
            assert!(content.contains("# keep2"));
            assert!(!content.contains(&encoded));
            let _ = fs::remove_dir_all(root);
        });
    }
}
