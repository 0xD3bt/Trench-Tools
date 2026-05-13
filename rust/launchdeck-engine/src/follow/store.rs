use super::contract::{
    DeferredSetupRecord, DeferredSetupState, FOLLOW_JOB_SCHEMA_VERSION, FOLLOW_SCHEMA_VERSION,
    FollowActionRecord, FollowActionState, FollowArmRequest, FollowCancelRequest,
    FollowDaemonHealth, FollowDaemonStateFile, FollowJobRecord, FollowJobState,
    FollowReserveRequest, FollowWatcherHealth, build_action_records,
    canonicalize_action_trigger_key, canonicalize_job_trigger_keys, json_values_equal, now_ms,
    should_prune_job_on_restart, stable_action_identity, stable_reserve_payload_fingerprint,
};
use crate::{
    fs_utils::{atomic_write, quarantine_corrupt_file},
    report::{FollowJobTimings, configured_benchmark_mode},
};
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, RwLock},
    time::sleep,
};

#[derive(Debug, Clone)]
pub struct FollowDaemonStore {
    pub state_path: PathBuf,
    pub inner: Arc<RwLock<FollowDaemonStateFile>>,
    persist_lock: Arc<Mutex<()>>,
}

fn is_retryable_persist_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32 | 33 | 1224))
        || matches!(
            error.kind(),
            std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::PermissionDenied
        )
}

impl FollowDaemonStore {
    pub fn load_or_default(state_path: PathBuf) -> Self {
        let mut state = match fs::read_to_string(&state_path) {
            Ok(raw) => serde_json::from_str::<FollowDaemonStateFile>(&raw).unwrap_or_else(|_| {
                let _ = quarantine_corrupt_file(&state_path, "follow daemon state");
                Self::default_state(state_path.clone())
            }),
            Err(_) => Self::default_state(state_path.clone()),
        };
        for job in &mut state.jobs {
            canonicalize_job_trigger_keys(job);
        }
        Self {
            state_path,
            inner: Arc::new(RwLock::new(state)),
            persist_lock: Arc::new(Mutex::new(())),
        }
    }

    fn default_state(state_path: PathBuf) -> FollowDaemonStateFile {
        FollowDaemonStateFile {
            schemaVersion: FOLLOW_SCHEMA_VERSION,
            health: FollowDaemonHealth {
                running: false,
                statePath: state_path,
                version: env!("CARGO_PKG_VERSION").to_string(),
                pid: None,
                startedAtMs: None,
                controlTransport: "local-http".to_string(),
                controlUrl: None,
                updatedAtMs: now_ms(),
                queueDepth: 0,
                activeJobs: 0,
                maxActiveJobs: None,
                maxConcurrentCompiles: None,
                maxConcurrentSends: None,
                availableCompileSlots: None,
                availableSendSlots: None,
                slotWatcher: FollowWatcherHealth::Healthy,
                slotWatcherMode: None,
                signatureWatcher: FollowWatcherHealth::Healthy,
                signatureWatcherMode: None,
                marketWatcher: FollowWatcherHealth::Healthy,
                marketWatcherMode: None,
                lastError: None,
                watchEndpoint: None,
            },
            jobs: vec![],
        }
    }

    fn active_job_count(jobs: &[FollowJobRecord]) -> usize {
        jobs.iter()
            .filter(|job| {
                matches!(
                    job.state,
                    FollowJobState::Reserved | FollowJobState::Armed | FollowJobState::Running
                )
            })
            .count()
    }

    fn has_live_watcher_work(jobs: &[FollowJobRecord]) -> bool {
        jobs.iter()
            .any(|job| matches!(job.state, FollowJobState::Armed | FollowJobState::Running))
    }

    fn normalize_idle_health(state: &mut FollowDaemonStateFile) {
        if Self::has_live_watcher_work(&state.jobs) {
            return;
        }
        state.health.slotWatcher = FollowWatcherHealth::Healthy;
        state.health.slotWatcherMode = None;
        state.health.signatureWatcher = FollowWatcherHealth::Healthy;
        state.health.signatureWatcherMode = None;
        state.health.marketWatcher = FollowWatcherHealth::Healthy;
        state.health.marketWatcherMode = None;
        state.health.watchEndpoint = None;
        state.health.lastError = None;
    }

    fn refresh_counts(state: &mut FollowDaemonStateFile) {
        state.health.queueDepth = state.jobs.len();
        state.health.activeJobs = Self::active_job_count(&state.jobs);
        Self::normalize_idle_health(state);
    }

    pub async fn persist(&self) -> Result<(), String> {
        let state = self.inner.read().await.clone();
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let payload = serde_json::to_vec_pretty(&state).map_err(|error| error.to_string())?;
        let _persist_guard = self.persist_lock.lock().await;
        let mut last_error = None;
        for attempt in 0..5 {
            match atomic_write(&self.state_path, &payload) {
                Ok(()) => return Ok(()),
                Err(error)
                    if is_retryable_persist_error(&std::io::Error::other(error.clone()))
                        && attempt < 4 =>
                {
                    last_error = Some(std::io::Error::other(error));
                    sleep(Duration::from_millis(20 * (attempt + 1) as u64)).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "Failed to persist follow daemon state.".to_string()))
    }

    pub async fn health(&self) -> FollowDaemonHealth {
        self.inner.read().await.health.clone()
    }

    pub async fn list_jobs(&self) -> Vec<FollowJobRecord> {
        self.inner.read().await.jobs.clone()
    }

    pub async fn get_job(&self, trace_id: &str) -> Option<FollowJobRecord> {
        self.inner
            .read()
            .await
            .jobs
            .iter()
            .find(|job| job.traceId == trace_id)
            .cloned()
    }

    pub async fn reserve_job(
        &self,
        request: FollowReserveRequest,
    ) -> Result<FollowJobRecord, String> {
        let mut state = self.inner.write().await;
        let now = now_ms();
        let mut requested_actions = if request.prebuiltActions.is_empty() {
            build_action_records(&request.followLaunch)
        } else {
            request.prebuiltActions.clone()
        };
        for action in &mut requested_actions {
            canonicalize_action_trigger_key(action);
        }
        let requested_action_identity = requested_actions
            .iter()
            .map(stable_action_identity)
            .collect::<Vec<_>>();
        let requested_payload_fingerprint = stable_reserve_payload_fingerprint(
            &requested_action_identity,
            &request.deferredSetupTransactions,
        )?;
        if let Some(existing) = state
            .jobs
            .iter_mut()
            .find(|job| job.traceId == request.traceId)
        {
            let existing_action_identity = existing
                .actions
                .iter()
                .map(stable_action_identity)
                .collect::<Vec<_>>();
            let existing_payload_fingerprint = if existing.reservedPayloadFingerprint.is_empty() {
                let existing_deferred_setup_transactions = existing
                    .deferredSetup
                    .as_ref()
                    .map(|setup| setup.transactions.clone())
                    .unwrap_or_default();
                stable_reserve_payload_fingerprint(
                    &existing_action_identity,
                    &existing_deferred_setup_transactions,
                )?
            } else {
                existing.reservedPayloadFingerprint.clone()
            };
            if existing.launchpad != request.launchpad
                || existing.quoteAsset != request.quoteAsset
                || existing.launchMode != request.launchMode
                || existing.selectedWalletKey != request.selectedWalletKey
                || existing.tokenMayhemMode != request.tokenMayhemMode
                || existing.jitoTipAccount != request.jitoTipAccount
                || existing.buyTipAccount != request.buyTipAccount
                || existing.sellTipAccount != request.sellTipAccount
                || existing.preferPostSetupCreatorVaultForSell
                    != request.preferPostSetupCreatorVaultForSell
                || !json_values_equal(&existing.followLaunch, &request.followLaunch)
                || !json_values_equal(&existing.execution, &request.execution)
                || !json_values_equal(&existing.bagsLaunch, &request.bagsLaunch)
                || existing_payload_fingerprint != requested_payload_fingerprint
            {
                return Err(format!(
                    "Conflicting follow reserve request for traceId {}. Reused traceIds must keep the same follow-launch payload.",
                    request.traceId
                ));
            }
            if existing.reservedPayloadFingerprint.is_empty() {
                existing.reservedPayloadFingerprint = requested_payload_fingerprint;
            }
            existing.updatedAtMs = now;
            state.health.updatedAtMs = now;
            Self::refresh_counts(&mut state);
            drop(state);
            self.persist().await?;
            return Ok(self
                .inner
                .read()
                .await
                .jobs
                .iter()
                .find(|job| job.traceId == request.traceId)
                .cloned()
                .expect("reserved job should exist"));
        }
        let actions = requested_actions;
        let job = FollowJobRecord {
            schemaVersion: FOLLOW_JOB_SCHEMA_VERSION,
            traceId: request.traceId.clone(),
            jobId: format!("follow-{}", request.traceId.replace('-', "")),
            state: FollowJobState::Reserved,
            createdAtMs: now,
            updatedAtMs: now,
            launchpad: request.launchpad,
            quoteAsset: request.quoteAsset,
            launchMode: request.launchMode,
            selectedWalletKey: request.selectedWalletKey,
            execution: request.execution,
            tokenMayhemMode: request.tokenMayhemMode,
            wrapperDefaultFeeBps: request.wrapperDefaultFeeBps,
            jitoTipAccount: request.jitoTipAccount,
            buyTipAccount: request.buyTipAccount,
            sellTipAccount: request.sellTipAccount,
            preferPostSetupCreatorVaultForSell: request.preferPostSetupCreatorVaultForSell,
            mint: None,
            launchCreator: None,
            launchSignature: None,
            launchTransactionSubscribeAccountRequired: vec![],
            submitAtMs: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            reportPath: None,
            transportPlan: None,
            bagsLaunch: request.bagsLaunch,
            followLaunch: request.followLaunch,
            actions,
            reservedPayloadFingerprint: requested_payload_fingerprint,
            deferredSetup: if request.deferredSetupTransactions.is_empty() {
                None
            } else {
                Some(DeferredSetupRecord {
                    transactions: request.deferredSetupTransactions,
                    state: DeferredSetupState::Queued,
                    attemptCount: 0,
                    signatures: vec![],
                    submittedAtMs: None,
                    confirmedAtMs: None,
                    lastError: None,
                })
            },
            cancelRequested: false,
            lastError: None,
            timings: FollowJobTimings {
                benchmarkMode: Some(configured_benchmark_mode().as_str().to_string()),
                ..FollowJobTimings::default()
            },
        };
        state.jobs.push(job.clone());
        state.health.updatedAtMs = now;
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await?;
        Ok(job)
    }

    pub async fn arm_job(&self, request: FollowArmRequest) -> Result<FollowJobRecord, String> {
        let mut state = self.inner.write().await;
        let now = now_ms();
        let job = state
            .jobs
            .iter_mut()
            .find(|job| job.traceId == request.traceId)
            .ok_or_else(|| format!("Unknown follow job traceId: {}", request.traceId))?;
        if let Some(existing_mint) = &job.mint
            && existing_mint != &request.mint
        {
            return Err(format!(
                "Conflicting follow arm request for traceId {}: mint changed from {} to {}.",
                request.traceId, existing_mint, request.mint
            ));
        }
        if let Some(existing_signature) = &job.launchSignature
            && existing_signature != &request.launchSignature
        {
            return Err(format!(
                "Conflicting follow arm request for traceId {}: signature changed from {} to {}.",
                request.traceId, existing_signature, request.launchSignature
            ));
        }
        if let Some(existing_launch_creator) = &job.launchCreator
            && existing_launch_creator != &request.launchCreator
        {
            return Err(format!(
                "Conflicting follow arm request for traceId {}: launch creator changed from {} to {}.",
                request.traceId, existing_launch_creator, request.launchCreator
            ));
        }
        let mut metadata_touched = false;
        if job.mint.is_none() {
            job.mint = Some(request.mint.clone());
            metadata_touched = true;
        }
        if job.launchCreator.is_none() {
            job.launchCreator = Some(request.launchCreator.clone());
            metadata_touched = true;
        }
        if job.launchSignature.is_none() {
            job.launchSignature = Some(request.launchSignature.clone());
            metadata_touched = true;
        }
        if !request.launchTransactionSubscribeAccountRequired.is_empty() {
            job.launchTransactionSubscribeAccountRequired =
                request.launchTransactionSubscribeAccountRequired.clone();
            metadata_touched = true;
        }
        if job.submitAtMs.is_none() {
            job.submitAtMs = Some(request.submitAtMs);
            metadata_touched = true;
        }
        if let Some(send_observed_slot) = request.sendObservedSlot {
            job.sendObservedSlot = Some(send_observed_slot);
            metadata_touched = true;
        }
        if let Some(confirmed_observed_slot) = request.confirmedObservedSlot {
            job.confirmedObservedSlot = Some(confirmed_observed_slot);
            metadata_touched = true;
        }
        if let Some(report_path) = request.reportPath.clone() {
            job.reportPath = Some(report_path);
            metadata_touched = true;
        }
        if matches!(
            job.state,
            FollowJobState::Completed
                | FollowJobState::CompletedWithFailures
                | FollowJobState::Cancelled
                | FollowJobState::Failed
        ) {
            if metadata_touched {
                job.updatedAtMs = now;
            }
            let snapshot = job.clone();
            drop(state);
            if metadata_touched {
                self.persist().await?;
            }
            return Ok(snapshot);
        }
        job.state = if matches!(job.state, FollowJobState::Running) {
            FollowJobState::Running
        } else {
            FollowJobState::Armed
        };
        job.updatedAtMs = now;
        job.transportPlan = Some(request.transportPlan);
        for action in &mut job.actions {
            if matches!(
                action.state,
                FollowActionState::Queued | FollowActionState::Armed
            ) {
                action.state = FollowActionState::Armed;
            }
            if action.scheduledForMs.is_none() {
                if let Some(delay_ms) = action.submitDelayMs {
                    action.scheduledForMs =
                        Some(request.submitAtMs.saturating_add(u128::from(delay_ms)));
                } else if let Some(delay_ms) = action.delayMs {
                    action.scheduledForMs =
                        Some(request.submitAtMs.saturating_add(u128::from(delay_ms)));
                }
            }
        }
        state.health.updatedAtMs = now;
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await?;
        Ok(self
            .inner
            .read()
            .await
            .jobs
            .iter()
            .find(|job| job.traceId == request.traceId)
            .cloned()
            .expect("armed job should exist"))
    }

    pub async fn cancel_job(
        &self,
        request: FollowCancelRequest,
    ) -> Result<FollowJobRecord, String> {
        let mut state = self.inner.write().await;
        let now = now_ms();
        let job = state
            .jobs
            .iter_mut()
            .find(|job| job.traceId == request.traceId)
            .ok_or_else(|| format!("Unknown follow job traceId: {}", request.traceId))?;
        job.updatedAtMs = now;
        if let Some(action_id) = request.actionId.as_deref() {
            if let Some(action) = job
                .actions
                .iter_mut()
                .find(|action| action.actionId == action_id)
            {
                action.state = FollowActionState::Cancelled;
                action.lastError = request.note.clone();
            }
        } else {
            job.cancelRequested = true;
            job.state = FollowJobState::Cancelled;
            for action in &mut job.actions {
                if !matches!(
                    action.state,
                    FollowActionState::Confirmed
                        | FollowActionState::Stopped
                        | FollowActionState::Failed
                        | FollowActionState::Cancelled
                ) {
                    action.state = FollowActionState::Cancelled;
                    action.lastError = request.note.clone();
                }
            }
        }
        state.health.updatedAtMs = now;
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await?;
        Ok(self
            .inner
            .read()
            .await
            .jobs
            .iter()
            .find(|job| job.traceId == request.traceId)
            .cloned()
            .expect("cancelled job should exist"))
    }

    pub async fn cancel_all_jobs(
        &self,
        note: Option<String>,
    ) -> Result<Vec<FollowJobRecord>, String> {
        let mut state = self.inner.write().await;
        let now = now_ms();
        let mut snapshots = Vec::new();
        for job in &mut state.jobs {
            if matches!(
                job.state,
                FollowJobState::Completed
                    | FollowJobState::CompletedWithFailures
                    | FollowJobState::Cancelled
                    | FollowJobState::Failed
            ) {
                continue;
            }
            job.cancelRequested = true;
            job.state = FollowJobState::Cancelled;
            job.updatedAtMs = now;
            for action in &mut job.actions {
                if !matches!(
                    action.state,
                    FollowActionState::Confirmed
                        | FollowActionState::Stopped
                        | FollowActionState::Failed
                        | FollowActionState::Cancelled
                ) {
                    action.state = FollowActionState::Cancelled;
                    action.lastError = note.clone();
                }
            }
            snapshots.push(job.clone());
        }
        state.health.updatedAtMs = now;
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await?;
        Ok(snapshots)
    }

    pub async fn update_health(
        &self,
        watch_endpoint: Option<String>,
        slot_watcher: FollowWatcherHealth,
        slot_watcher_mode: Option<String>,
        signature_watcher: FollowWatcherHealth,
        signature_watcher_mode: Option<String>,
        market_watcher: FollowWatcherHealth,
        market_watcher_mode: Option<String>,
        last_error: Option<String>,
    ) -> Result<(), String> {
        let mut state = self.inner.write().await;
        state.health.updatedAtMs = now_ms();
        state.health.watchEndpoint = watch_endpoint;
        state.health.slotWatcher = slot_watcher;
        state.health.slotWatcherMode = slot_watcher_mode;
        state.health.signatureWatcher = signature_watcher;
        state.health.signatureWatcherMode = signature_watcher_mode;
        state.health.marketWatcher = market_watcher;
        state.health.marketWatcherMode = market_watcher_mode;
        state.health.lastError = last_error;
        drop(state);
        self.persist().await
    }

    pub async fn update_supervision(
        &self,
        running: bool,
        pid: Option<u32>,
        control_transport: Option<String>,
        control_url: Option<String>,
        last_error: Option<String>,
    ) -> Result<(), String> {
        let mut state = self.inner.write().await;
        let now = now_ms();
        state.health.running = running;
        state.health.pid = pid;
        if running {
            state.health.startedAtMs.get_or_insert(now);
        }
        if let Some(value) = control_transport {
            state.health.controlTransport = value;
        }
        if control_url.is_some() {
            state.health.controlUrl = control_url;
        }
        state.health.lastError = last_error;
        state.health.updatedAtMs = now;
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await
    }

    pub async fn recover_jobs_for_restart(&self) -> Result<Vec<FollowJobRecord>, String> {
        let mut state = self.inner.write().await;
        let now = now_ms();
        state
            .jobs
            .retain(|job| !should_prune_job_on_restart(job, now));
        let mut recovered = Vec::new();
        for job in &mut state.jobs {
            let should_resume =
                matches!(job.state, FollowJobState::Armed | FollowJobState::Running)
                    && !job.cancelRequested;
            if matches!(job.state, FollowJobState::Running) {
                job.state = FollowJobState::Armed;
            }
            let mut touched = false;
            for action in &mut job.actions {
                if matches!(
                    action.state,
                    FollowActionState::Running | FollowActionState::Sent
                ) {
                    action.state = FollowActionState::Failed;
                    action.lastError = Some(
                        "Follow daemon restarted while the action was in flight; automatic resend was skipped to avoid duplication."
                            .to_string(),
                    );
                    touched = true;
                }
            }
            if touched {
                job.updatedAtMs = now;
            }
            if should_resume {
                recovered.push(job.clone());
            }
        }
        state.health.updatedAtMs = now;
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await?;
        Ok(recovered)
    }

    pub async fn clear_jobs_for_restart(&self) -> Result<usize, String> {
        let mut state = self.inner.write().await;
        let removed = state.jobs.len();
        let now = now_ms();
        state.jobs.clear();
        state.health.updatedAtMs = now;
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await?;
        Ok(removed)
    }

    pub async fn update_action(
        &self,
        trace_id: &str,
        action_id: &str,
        mutator: impl FnOnce(&mut FollowActionRecord),
    ) -> Result<FollowJobRecord, String> {
        let mut state = self.inner.write().await;
        let now = now_ms();
        state.health.updatedAtMs = now;
        let job = state
            .jobs
            .iter_mut()
            .find(|job| job.traceId == trace_id)
            .ok_or_else(|| format!("Unknown follow job traceId: {trace_id}"))?;
        let action = job
            .actions
            .iter_mut()
            .find(|action| action.actionId == action_id)
            .ok_or_else(|| format!("Unknown follow action: {action_id}"))?;
        mutator(action);
        job.updatedAtMs = now;
        let snapshot = job.clone();
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await?;
        Ok(snapshot)
    }

    pub async fn update_job(
        &self,
        trace_id: &str,
        mutator: impl FnOnce(&mut FollowJobRecord),
    ) -> Result<FollowJobRecord, String> {
        let mut state = self.inner.write().await;
        let now = now_ms();
        state.health.updatedAtMs = now;
        let job = state
            .jobs
            .iter_mut()
            .find(|job| job.traceId == trace_id)
            .ok_or_else(|| format!("Unknown follow job traceId: {trace_id}"))?;
        mutator(job);
        job.updatedAtMs = now;
        let snapshot = job.clone();
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await?;
        Ok(snapshot)
    }

    pub async fn finalize_job_state(
        &self,
        trace_id: &str,
        state_value: FollowJobState,
        last_error: Option<String>,
    ) -> Result<FollowJobRecord, String> {
        let mut state = self.inner.write().await;
        let now = now_ms();
        state.health.updatedAtMs = now;
        let job = state
            .jobs
            .iter_mut()
            .find(|job| job.traceId == trace_id)
            .ok_or_else(|| format!("Unknown follow job traceId: {trace_id}"))?;
        job.state = state_value;
        job.lastError = last_error;
        job.updatedAtMs = now;
        let snapshot = job.clone();
        Self::refresh_counts(&mut state);
        drop(state);
        self.persist().await?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{
            NormalizedExecution, NormalizedFollowLaunch, NormalizedFollowLaunchConstraints,
            NormalizedFollowLaunchSnipe,
        },
        follow::{FollowArmRequest, FollowReserveRequest},
        transport::TransportPlan,
    };
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_state_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("launchdeck-follow-store-{label}-{unique}.json"))
    }

    fn sample_execution() -> NormalizedExecution {
        serde_json::from_value(json!({
            "simulate": false,
            "send": true,
            "txFormat": "v0",
            "commitment": "confirmed",
            "skipPreflight": false,
            "trackSendBlockHeight": true,
            "provider": "standard-rpc",
            "endpointProfile": "default",
            "mevProtect": false,
            "mevMode": "reduced",
            "jitodontfront": false,
            "autoGas": false,
            "autoMode": "manual",
            "priorityFeeSol": "0",
            "tipSol": "0",
            "maxPriorityFeeSol": "0",
            "maxTipSol": "0",
            "buyProvider": "standard-rpc",
            "buyEndpointProfile": "default",
            "buyMevProtect": false,
            "buyMevMode": "reduced",
            "buyJitodontfront": false,
            "buyAutoGas": false,
            "buyAutoMode": "manual",
            "buyPriorityFeeSol": "0",
            "buyTipSol": "0",
            "buySlippagePercent": "5",
            "buyMaxPriorityFeeSol": "0",
            "buyMaxTipSol": "0",
            "sellAutoGas": false,
            "sellAutoMode": "manual",
            "sellProvider": "standard-rpc",
            "sellEndpointProfile": "default",
            "sellMevProtect": false,
            "sellMevMode": "reduced",
            "sellJitodontfront": false,
            "sellPriorityFeeSol": "0",
            "sellTipSol": "0",
            "sellSlippagePercent": "5",
            "sellMaxPriorityFeeSol": "0",
            "sellMaxTipSol": "0"
        }))
        .expect("sample execution")
    }

    fn sample_transport_plan() -> TransportPlan {
        TransportPlan {
            requestedProvider: "standard-rpc".to_string(),
            resolvedProvider: "standard-rpc".to_string(),
            requestedEndpointProfile: "default".to_string(),
            resolvedEndpointProfile: "default".to_string(),
            executionClass: "direct".to_string(),
            transportType: "standard-rpc".to_string(),
            ordering: "sequential".to_string(),
            verified: true,
            supportsBundle: false,
            requiresInlineTip: false,
            requiresPriorityFee: false,
            separateTipTransaction: false,
            skipPreflight: false,
            maxRetries: 0,
            standardRpcSubmitEndpoints: vec![],
            helloMoonApiKeyConfigured: false,
            helloMoonMevProtect: false,
            helloMoonQuicEndpoint: None,
            helloMoonQuicEndpoints: vec![],
            helloMoonBundleEndpoint: None,
            helloMoonBundleEndpoints: vec![],
            heliusSenderEndpoint: None,
            heliusSenderEndpoints: vec![],
            watchEndpoint: None,
            watchEndpoints: vec![],
            jitoBundleEndpoints: vec![],
            warnings: vec![],
        }
    }

    fn sample_follow_launch_with_submit_delay(delay_ms: u64) -> NormalizedFollowLaunch {
        NormalizedFollowLaunch {
            enabled: true,
            source: "test".to_string(),
            schemaVersion: 1,
            snipes: vec![NormalizedFollowLaunchSnipe {
                actionId: "snipe-a".to_string(),
                enabled: true,
                walletEnvKey: "WALLET_A".to_string(),
                buyAmountSol: "0.1".to_string(),
                submitWithLaunch: false,
                retryOnFailure: false,
                submitDelayMs: delay_ms,
                targetBlockOffset: None,
                jitterMs: 0,
                feeJitterBps: 0,
                skipIfTokenBalancePositive: false,
                postBuySell: None,
            }],
            devAutoSell: None,
            constraints: NormalizedFollowLaunchConstraints {
                pumpOnly: true,
                retryBudget: 0,
                requireDaemonReadiness: true,
                blockOnRequiredPrechecks: true,
            },
        }
    }

    #[tokio::test]
    async fn arm_job_schedules_on_submit_action_from_submit_timestamp() {
        let state_path = test_state_path("on-submit");
        let store = FollowDaemonStore::load_or_default(state_path.clone());
        let trace_id = "trace-on-submit-delay".to_string();

        store
            .reserve_job(FollowReserveRequest {
                traceId: trace_id.clone(),
                launchpad: "pump".to_string(),
                quoteAsset: "sol".to_string(),
                launchMode: "normal".to_string(),
                selectedWalletKey: "WALLET_A".to_string(),
                followLaunch: sample_follow_launch_with_submit_delay(25),
                execution: sample_execution(),
                tokenMayhemMode: false,
                wrapperDefaultFeeBps: 100,
                jitoTipAccount: String::new(),
                buyTipAccount: String::new(),
                sellTipAccount: String::new(),
                preferPostSetupCreatorVaultForSell: false,
                bagsLaunch: None,
                prebuiltActions: vec![],
                deferredSetupTransactions: vec![],
            })
            .await
            .expect("reserve follow job");

        let armed = store
            .arm_job(FollowArmRequest {
                traceId: trace_id,
                mint: "mint".to_string(),
                launchCreator: "creator".to_string(),
                launchSignature: "sig".to_string(),
                launchTransactionSubscribeAccountRequired: vec![],
                submitAtMs: 1_000,
                sendObservedSlot: Some(10),
                confirmedObservedSlot: None,
                reportPath: None,
                transportPlan: sample_transport_plan(),
            })
            .await
            .expect("arm follow job");

        assert_eq!(armed.submitAtMs, Some(1_000));
        assert_eq!(armed.actions[0].submitDelayMs, Some(25));
        assert_eq!(armed.actions[0].scheduledForMs, Some(1_025));

        let _ = std::fs::remove_file(state_path);
    }
}
