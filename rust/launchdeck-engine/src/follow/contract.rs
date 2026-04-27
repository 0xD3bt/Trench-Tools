use crate::{
    config::{NormalizedExecution, NormalizedFollowLaunch},
    report::{FollowActionTimings, FollowJobTimings},
    rpc::CompiledTransaction,
    transport::TransportPlan,
};
use serde::{Deserialize, Serialize};
pub use shared_extension_runtime::follow_contract::BagsLaunchMetadata;
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const FOLLOW_SCHEMA_VERSION: u32 = 1;
pub const FOLLOW_JOB_SCHEMA_VERSION: u32 = 2;
pub const FOLLOW_RESPONSE_SCHEMA_VERSION: u32 = 1;
const RESTART_STALE_JOB_MAX_AGE_MS: u128 = 5 * 60 * 1000;

fn follow_job_schema_version() -> u32 {
    FOLLOW_JOB_SCHEMA_VERSION
}

fn follow_response_schema_version() -> u32 {
    FOLLOW_RESPONSE_SCHEMA_VERSION
}

fn default_wrapper_fee_bps() -> u16 {
    10
}

pub(super) fn json_values_equal<T: Serialize>(left: &T, right: &T) -> bool {
    match (serde_json::to_value(left), serde_json::to_value(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct StableFollowActionIdentity {
    actionId: String,
    kind: FollowActionKind,
    walletEnvKey: String,
    buyAmountSol: Option<String>,
    sellPercent: Option<u8>,
    submitDelayMs: Option<u64>,
    targetBlockOffset: Option<u8>,
    delayMs: Option<u64>,
    marketCap: Option<FollowMarketCapTrigger>,
    jitterMs: Option<u64>,
    feeJitterBps: Option<u16>,
    precheckRequired: bool,
    requireConfirmation: bool,
    skipIfTokenBalancePositive: bool,
    triggerKey: Option<String>,
    orderIndex: u32,
    poolId: Option<String>,
    preSignedTransactions: Vec<CompiledTransaction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    primaryTxIndex: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct StableFollowReservePayloadIdentity {
    actions: Vec<StableFollowActionIdentity>,
    deferredSetupTransactions: Vec<CompiledTransaction>,
}

fn stable_identity_trigger_key(action: &FollowActionRecord) -> Option<String> {
    if matches!(action.kind, FollowActionKind::SniperSell) {
        return Some(trigger_key_for_action(action));
    }
    normalized_trigger_key_value(action.triggerKey.as_deref(), action.targetBlockOffset)
}

pub(super) fn stable_action_identity(action: &FollowActionRecord) -> StableFollowActionIdentity {
    StableFollowActionIdentity {
        actionId: action.actionId.clone(),
        kind: action.kind.clone(),
        walletEnvKey: action.walletEnvKey.clone(),
        buyAmountSol: action.buyAmountSol.clone(),
        sellPercent: action.sellPercent,
        submitDelayMs: action.submitDelayMs,
        targetBlockOffset: action.targetBlockOffset,
        delayMs: action.delayMs,
        marketCap: action.marketCap.clone(),
        jitterMs: action.jitterMs,
        feeJitterBps: action.feeJitterBps,
        precheckRequired: action.precheckRequired,
        requireConfirmation: action.requireConfirmation,
        skipIfTokenBalancePositive: action.skipIfTokenBalancePositive,
        triggerKey: stable_identity_trigger_key(action),
        orderIndex: action.orderIndex,
        poolId: action.poolId.clone(),
        preSignedTransactions: action.preSignedTransactions.clone(),
        primaryTxIndex: action.primaryTxIndex,
    }
}

pub(super) fn stable_reserve_payload_fingerprint(
    action_identity: &[StableFollowActionIdentity],
    deferred_setup_transactions: &[CompiledTransaction],
) -> Result<String, String> {
    serde_json::to_string(&StableFollowReservePayloadIdentity {
        actions: action_identity.to_vec(),
        deferredSetupTransactions: deferred_setup_transactions.to_vec(),
    })
    .map_err(|error| format!("Failed to serialize follow reserve payload identity: {error}"))
}

pub(super) fn should_prune_job_on_restart(job: &FollowJobRecord, now: u128) -> bool {
    let freshest_ms = job.updatedAtMs.max(job.createdAtMs);
    now.saturating_sub(freshest_ms) > RESTART_STALE_JOB_MAX_AGE_MS
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FollowJobState {
    Reserved,
    Armed,
    Running,
    Completed,
    CompletedWithFailures,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FollowActionState {
    Queued,
    Armed,
    Eligible,
    Running,
    Sent,
    Confirmed,
    Stopped,
    Failed,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FollowActionKind {
    SniperBuy,
    DevAutoSell,
    SniperSell,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FollowWatcherHealth {
    Healthy,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowMarketCapTrigger {
    pub direction: String,
    pub threshold: String,
    pub scanTimeoutSeconds: u64,
    pub timeoutAction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DeferredSetupState {
    Queued,
    Running,
    Sent,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredSetupRecord {
    pub transactions: Vec<CompiledTransaction>,
    pub state: DeferredSetupState,
    #[serde(default)]
    pub attemptCount: u32,
    #[serde(default)]
    pub signatures: Vec<String>,
    #[serde(default)]
    pub submittedAtMs: Option<u128>,
    #[serde(default)]
    pub confirmedAtMs: Option<u128>,
    #[serde(default)]
    pub lastError: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowActionRecord {
    pub actionId: String,
    pub kind: FollowActionKind,
    pub walletEnvKey: String,
    pub state: FollowActionState,
    pub buyAmountSol: Option<String>,
    pub sellPercent: Option<u8>,
    pub submitDelayMs: Option<u64>,
    pub targetBlockOffset: Option<u8>,
    pub delayMs: Option<u64>,
    pub marketCap: Option<FollowMarketCapTrigger>,
    pub jitterMs: Option<u64>,
    pub feeJitterBps: Option<u16>,
    pub precheckRequired: bool,
    pub requireConfirmation: bool,
    #[serde(default)]
    pub skipIfTokenBalancePositive: bool,
    pub attemptCount: u32,
    pub scheduledForMs: Option<u128>,
    #[serde(default)]
    pub eligibleAtMs: Option<u128>,
    pub submitStartedAtMs: Option<u128>,
    pub submittedAtMs: Option<u128>,
    pub confirmedAtMs: Option<u128>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub endpointProfile: Option<String>,
    #[serde(default)]
    pub transportType: Option<String>,
    #[serde(default)]
    pub watcherMode: Option<String>,
    #[serde(default)]
    pub watcherFallbackReason: Option<String>,
    #[serde(default, alias = "sendObservedBlockHeight")]
    pub sendObservedSlot: Option<u64>,
    #[serde(default, alias = "confirmedObservedBlockHeight")]
    pub confirmedObservedSlot: Option<u64>,
    #[serde(default)]
    pub confirmedTokenBalanceRaw: Option<String>,
    #[serde(default, alias = "eligibilityObservedBlockHeight")]
    pub eligibilityObservedSlot: Option<u64>,
    #[serde(default, alias = "blocksToConfirm")]
    pub slotsToConfirm: Option<u64>,
    pub signature: Option<String>,
    pub explorerUrl: Option<String>,
    pub endpoint: Option<String>,
    pub bundleId: Option<String>,
    pub lastError: Option<String>,
    #[serde(default)]
    pub triggerKey: Option<String>,
    #[serde(default)]
    pub orderIndex: u32,
    #[serde(default)]
    pub preSignedTransactions: Vec<CompiledTransaction>,
    #[serde(default)]
    pub poolId: Option<String>,
    #[serde(default)]
    pub primaryTxIndex: Option<usize>,
    #[serde(default)]
    pub timings: FollowActionTimings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowJobRecord {
    #[serde(default = "follow_job_schema_version")]
    pub schemaVersion: u32,
    pub traceId: String,
    pub jobId: String,
    pub state: FollowJobState,
    pub createdAtMs: u128,
    pub updatedAtMs: u128,
    pub launchpad: String,
    pub quoteAsset: String,
    #[serde(default)]
    pub launchMode: String,
    pub selectedWalletKey: String,
    pub execution: NormalizedExecution,
    pub tokenMayhemMode: bool,
    #[serde(default = "default_wrapper_fee_bps")]
    pub wrapperDefaultFeeBps: u16,
    pub jitoTipAccount: String,
    #[serde(default)]
    pub buyTipAccount: String,
    #[serde(default)]
    pub sellTipAccount: String,
    #[serde(default)]
    pub preferPostSetupCreatorVaultForSell: bool,
    pub mint: Option<String>,
    pub launchCreator: Option<String>,
    pub launchSignature: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launchTransactionSubscribeAccountRequired: Vec<String>,
    pub submitAtMs: Option<u128>,
    #[serde(default, alias = "sendObservedBlockHeight")]
    pub sendObservedSlot: Option<u64>,
    #[serde(default, alias = "confirmedObservedBlockHeight")]
    pub confirmedObservedSlot: Option<u64>,
    pub reportPath: Option<String>,
    pub transportPlan: Option<TransportPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bagsLaunch: Option<BagsLaunchMetadata>,
    pub followLaunch: NormalizedFollowLaunch,
    pub actions: Vec<FollowActionRecord>,
    #[serde(default)]
    pub reservedPayloadFingerprint: String,
    #[serde(default)]
    pub deferredSetup: Option<DeferredSetupRecord>,
    pub cancelRequested: bool,
    pub lastError: Option<String>,
    #[serde(default)]
    pub timings: FollowJobTimings,
}

fn should_use_post_setup_creator_vault(
    job_prefers_post_setup_creator_vault: bool,
    action: &FollowActionRecord,
    mev_mode: &str,
) -> bool {
    job_prefers_post_setup_creator_vault
        && (action.targetBlockOffset.unwrap_or_default() > 0
            || mev_mode.trim().eq_ignore_ascii_case("secure"))
}

pub fn should_use_post_setup_creator_vault_for_sell(
    job_prefers_post_setup_creator_vault: bool,
    action: &FollowActionRecord,
    mev_mode: &str,
) -> bool {
    should_use_post_setup_creator_vault(job_prefers_post_setup_creator_vault, action, mev_mode)
}

pub fn should_use_post_setup_creator_vault_for_buy(
    job_prefers_post_setup_creator_vault: bool,
    action: &FollowActionRecord,
    mev_mode: &str,
) -> bool {
    should_use_post_setup_creator_vault(job_prefers_post_setup_creator_vault, action, mev_mode)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowDaemonHealth {
    pub running: bool,
    pub statePath: PathBuf,
    pub version: String,
    pub pid: Option<u32>,
    pub startedAtMs: Option<u128>,
    pub controlTransport: String,
    pub controlUrl: Option<String>,
    pub updatedAtMs: u128,
    pub queueDepth: usize,
    pub activeJobs: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxActiveJobs: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxConcurrentCompiles: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxConcurrentSends: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availableCompileSlots: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availableSendSlots: Option<usize>,
    pub slotWatcher: FollowWatcherHealth,
    #[serde(default)]
    pub slotWatcherMode: Option<String>,
    pub signatureWatcher: FollowWatcherHealth,
    #[serde(default)]
    pub signatureWatcherMode: Option<String>,
    pub marketWatcher: FollowWatcherHealth,
    #[serde(default)]
    pub marketWatcherMode: Option<String>,
    pub lastError: Option<String>,
    pub watchEndpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowDaemonStateFile {
    pub schemaVersion: u32,
    pub health: FollowDaemonHealth,
    pub jobs: Vec<FollowJobRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowReserveRequest {
    pub traceId: String,
    pub launchpad: String,
    pub quoteAsset: String,
    #[serde(default)]
    pub launchMode: String,
    pub selectedWalletKey: String,
    pub followLaunch: NormalizedFollowLaunch,
    pub execution: NormalizedExecution,
    pub tokenMayhemMode: bool,
    #[serde(default = "default_wrapper_fee_bps")]
    pub wrapperDefaultFeeBps: u16,
    pub jitoTipAccount: String,
    #[serde(default)]
    pub buyTipAccount: String,
    #[serde(default)]
    pub sellTipAccount: String,
    #[serde(default)]
    pub preferPostSetupCreatorVaultForSell: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bagsLaunch: Option<BagsLaunchMetadata>,
    #[serde(default)]
    pub prebuiltActions: Vec<FollowActionRecord>,
    #[serde(default)]
    pub deferredSetupTransactions: Vec<CompiledTransaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowArmRequest {
    pub traceId: String,
    pub mint: String,
    pub launchCreator: String,
    pub launchSignature: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launchTransactionSubscribeAccountRequired: Vec<String>,
    pub submitAtMs: u128,
    #[serde(default, alias = "sendObservedBlockHeight")]
    pub sendObservedSlot: Option<u64>,
    #[serde(default, alias = "confirmedObservedBlockHeight")]
    pub confirmedObservedSlot: Option<u64>,
    pub reportPath: Option<String>,
    pub transportPlan: TransportPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowCancelRequest {
    pub traceId: String,
    pub actionId: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FollowStopAllRequest {
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowReadyRequest {
    pub followLaunch: NormalizedFollowLaunch,
    pub quoteAsset: String,
    pub execution: NormalizedExecution,
    pub watchEndpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowReadyResponse {
    pub ok: bool,
    pub ready: bool,
    pub watchEndpoint: Option<String>,
    pub requiredWebsocket: bool,
    pub reason: Option<String>,
    pub health: FollowDaemonHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowJobResponse {
    #[serde(default = "follow_response_schema_version")]
    pub schemaVersion: u32,
    pub ok: bool,
    pub job: Option<FollowJobRecord>,
    pub jobs: Vec<FollowJobRecord>,
    pub health: FollowDaemonHealth,
}

pub(super) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn base_trigger_key_for_action(action: &FollowActionRecord) -> String {
    if let Some(trigger) = &action.marketCap {
        return format!(
            "market:{}:{}:{}:{}",
            trigger.direction, trigger.threshold, trigger.scanTimeoutSeconds, trigger.timeoutAction
        );
    }
    if let Some(offset) = action.targetBlockOffset {
        return format!("slot:{offset}");
    }
    if action.requireConfirmation {
        return "confirm".to_string();
    }
    if let Some(delay_ms) = action.delayMs {
        return format!("delay:{delay_ms}");
    }
    if let Some(delay_ms) = action.submitDelayMs {
        return format!("submit:{delay_ms}");
    }
    "submit:0".to_string()
}

fn trigger_key_for_action(action: &FollowActionRecord) -> String {
    let base = base_trigger_key_for_action(action);
    if matches!(action.kind, FollowActionKind::SniperSell) {
        return format!("sniper-sell:{}:{base}", action.actionId);
    }
    base
}

fn normalized_trigger_key_value(
    trigger_key: Option<&str>,
    target_block_offset: Option<u8>,
) -> Option<String> {
    if let Some(trigger_key) = trigger_key {
        let trimmed = trigger_key.trim();
        if let Some(offset) = trimmed.strip_prefix("block:") {
            return Some(format!("slot:{offset}"));
        }
        if trimmed.is_empty() {
            return target_block_offset.map(|offset| format!("slot:{offset}"));
        }
        return Some(trimmed.to_string());
    }
    target_block_offset.map(|offset| format!("slot:{offset}"))
}

pub(super) fn canonicalize_action_trigger_key(action: &mut FollowActionRecord) {
    action.triggerKey = if matches!(action.kind, FollowActionKind::SniperSell) {
        Some(trigger_key_for_action(action))
    } else {
        normalized_trigger_key_value(action.triggerKey.as_deref(), action.targetBlockOffset)
            .or_else(|| Some(trigger_key_for_action(action)))
    };
}

pub(super) fn canonicalize_job_trigger_keys(job: &mut FollowJobRecord) {
    for action in &mut job.actions {
        canonicalize_action_trigger_key(action);
    }
}

pub fn build_action_records(follow: &NormalizedFollowLaunch) -> Vec<FollowActionRecord> {
    let mut actions = follow
        .snipes
        .iter()
        .enumerate()
        .filter(|(_, snipe)| snipe.enabled)
        .map(|(index, snipe)| {
            let mut action = FollowActionRecord {
                actionId: snipe.actionId.clone(),
                kind: FollowActionKind::SniperBuy,
                walletEnvKey: snipe.walletEnvKey.clone(),
                state: FollowActionState::Queued,
                buyAmountSol: Some(snipe.buyAmountSol.clone()),
                sellPercent: None,
                submitDelayMs: Some(snipe.submitDelayMs),
                targetBlockOffset: snipe.targetBlockOffset,
                delayMs: None,
                marketCap: None,
                jitterMs: Some(snipe.jitterMs),
                feeJitterBps: Some(snipe.feeJitterBps),
                precheckRequired: follow.constraints.blockOnRequiredPrechecks,
                requireConfirmation: false,
                skipIfTokenBalancePositive: snipe.skipIfTokenBalancePositive,
                attemptCount: 0,
                scheduledForMs: None,
                eligibleAtMs: None,
                submitStartedAtMs: None,
                submittedAtMs: None,
                confirmedAtMs: None,
                provider: None,
                endpointProfile: None,
                transportType: None,
                watcherMode: None,
                watcherFallbackReason: None,
                sendObservedSlot: None,
                confirmedObservedSlot: None,
                confirmedTokenBalanceRaw: None,
                eligibilityObservedSlot: None,
                slotsToConfirm: None,
                signature: None,
                explorerUrl: None,
                endpoint: None,
                bundleId: None,
                lastError: None,
                triggerKey: None,
                orderIndex: index as u32,
                preSignedTransactions: vec![],
                poolId: None,
                primaryTxIndex: None,
                timings: FollowActionTimings::default(),
            };
            action.triggerKey = Some(trigger_key_for_action(&action));
            action
        })
        .collect::<Vec<_>>();
    if let Some(dev_auto_sell) = &follow.devAutoSell
        && dev_auto_sell.enabled
    {
        let mut action = FollowActionRecord {
            actionId: dev_auto_sell.actionId.clone(),
            kind: FollowActionKind::DevAutoSell,
            walletEnvKey: dev_auto_sell.walletEnvKey.clone(),
            state: FollowActionState::Queued,
            buyAmountSol: None,
            sellPercent: Some(dev_auto_sell.percent),
            submitDelayMs: None,
            targetBlockOffset: dev_auto_sell.targetBlockOffset,
            delayMs: dev_auto_sell.delayMs,
            marketCap: dev_auto_sell
                .marketCap
                .as_ref()
                .map(|trigger| FollowMarketCapTrigger {
                    direction: trigger.direction.clone(),
                    threshold: trigger.threshold.clone(),
                    scanTimeoutSeconds: trigger.scanTimeoutSeconds,
                    timeoutAction: trigger.timeoutAction.clone(),
                }),
            jitterMs: None,
            feeJitterBps: None,
            precheckRequired: dev_auto_sell.precheckRequired,
            requireConfirmation: dev_auto_sell.requireConfirmation,
            skipIfTokenBalancePositive: false,
            attemptCount: 0,
            scheduledForMs: None,
            eligibleAtMs: None,
            submitStartedAtMs: None,
            submittedAtMs: None,
            confirmedAtMs: None,
            provider: None,
            endpointProfile: None,
            transportType: None,
            watcherMode: None,
            watcherFallbackReason: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedTokenBalanceRaw: None,
            eligibilityObservedSlot: None,
            slotsToConfirm: None,
            signature: None,
            explorerUrl: None,
            endpoint: None,
            bundleId: None,
            lastError: None,
            triggerKey: None,
            orderIndex: 0,
            preSignedTransactions: vec![],
            poolId: None,
            primaryTxIndex: None,
            timings: FollowActionTimings::default(),
        };
        action.triggerKey = Some(trigger_key_for_action(&action));
        actions.push(action);
    }
    for (index, snipe) in follow.snipes.iter().enumerate() {
        if !snipe.enabled {
            continue;
        }
        if let Some(sell) = &snipe.postBuySell
            && sell.enabled
        {
            let mut action = FollowActionRecord {
                actionId: sell.actionId.clone(),
                kind: FollowActionKind::SniperSell,
                walletEnvKey: sell.walletEnvKey.clone(),
                state: FollowActionState::Queued,
                buyAmountSol: None,
                sellPercent: Some(sell.percent),
                submitDelayMs: None,
                targetBlockOffset: sell.targetBlockOffset,
                delayMs: sell.delayMs,
                marketCap: sell
                    .marketCap
                    .as_ref()
                    .map(|trigger| FollowMarketCapTrigger {
                        direction: trigger.direction.clone(),
                        threshold: trigger.threshold.clone(),
                        scanTimeoutSeconds: trigger.scanTimeoutSeconds,
                        timeoutAction: trigger.timeoutAction.clone(),
                    }),
                jitterMs: None,
                feeJitterBps: None,
                precheckRequired: sell.precheckRequired,
                requireConfirmation: sell.requireConfirmation,
                skipIfTokenBalancePositive: false,
                attemptCount: 0,
                scheduledForMs: None,
                eligibleAtMs: None,
                submitStartedAtMs: None,
                submittedAtMs: None,
                confirmedAtMs: None,
                provider: None,
                endpointProfile: None,
                transportType: None,
                watcherMode: None,
                watcherFallbackReason: None,
                sendObservedSlot: None,
                confirmedObservedSlot: None,
                confirmedTokenBalanceRaw: None,
                eligibilityObservedSlot: None,
                slotsToConfirm: None,
                signature: None,
                explorerUrl: None,
                endpoint: None,
                bundleId: None,
                lastError: None,
                triggerKey: None,
                orderIndex: index as u32,
                preSignedTransactions: vec![],
                poolId: None,
                primaryTxIndex: None,
                timings: FollowActionTimings::default(),
            };
            action.triggerKey = Some(trigger_key_for_action(&action));
            actions.push(action);
        }
    }
    actions
}

pub fn follow_job_response(
    health: FollowDaemonHealth,
    job: Option<FollowJobRecord>,
    jobs: Vec<FollowJobRecord>,
) -> FollowJobResponse {
    FollowJobResponse {
        schemaVersion: FOLLOW_RESPONSE_SCHEMA_VERSION,
        ok: true,
        job,
        jobs,
        health,
    }
}

pub fn follow_ready_response(
    health: FollowDaemonHealth,
    watch_endpoint: Option<String>,
    required_websocket: bool,
    ready: bool,
    reason: Option<String>,
) -> FollowReadyResponse {
    FollowReadyResponse {
        ok: true,
        ready,
        watchEndpoint: watch_endpoint,
        requiredWebsocket: required_websocket,
        reason,
        health,
    }
}

pub fn path_exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        NormalizedFollowLaunchConstraints, NormalizedFollowLaunchSell, NormalizedFollowLaunchSnipe,
    };
    use crate::follow::FollowDaemonStore;
    use crate::transport::TransportPlan;
    use serde_json::json;

    fn sample_compiled_transaction(label: &str, blockhash: &str) -> CompiledTransaction {
        CompiledTransaction {
            label: label.to_string(),
            format: "v0".to_string(),
            blockhash: blockhash.to_string(),
            lastValidBlockHeight: 123,
            serializedBase64: format!("base64-{label}-{blockhash}"),
            signature: None,
            lookupTablesUsed: vec![],
            computeUnitLimit: None,
            computeUnitPriceMicroLamports: None,
            inlineTipLamports: None,
            inlineTipAccount: None,
        }
    }

    fn sample_follow_launch() -> NormalizedFollowLaunch {
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
                submitDelayMs: 0,
                targetBlockOffset: Some(0),
                jitterMs: 0,
                feeJitterBps: 0,
                skipIfTokenBalancePositive: false,
                postBuySell: None,
            }],
            devAutoSell: Some(NormalizedFollowLaunchSell {
                actionId: "dev-sell".to_string(),
                enabled: true,
                walletEnvKey: "WALLET_A".to_string(),
                percent: 100,
                delayMs: None,
                targetBlockOffset: Some(0),
                marketCap: None,
                precheckRequired: false,
                requireConfirmation: false,
            }),
            constraints: NormalizedFollowLaunchConstraints {
                pumpOnly: false,
                retryBudget: 1,
                requireDaemonReadiness: false,
                blockOnRequiredPrechecks: true,
            },
        }
    }

    fn sample_follow_launch_with_sniper_sell() -> NormalizedFollowLaunch {
        let mut follow = sample_follow_launch();
        follow.snipes[0].postBuySell = Some(NormalizedFollowLaunchSell {
            actionId: "snipe-a-sell".to_string(),
            enabled: true,
            walletEnvKey: "WALLET_A".to_string(),
            percent: 50,
            delayMs: None,
            targetBlockOffset: Some(1),
            marketCap: None,
            precheckRequired: false,
            requireConfirmation: true,
        });
        follow
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

    #[test]
    fn build_action_records_sets_trigger_keys_and_order() {
        let actions = build_action_records(&sample_follow_launch());
        let snipe = actions
            .iter()
            .find(|action| action.actionId == "snipe-a")
            .expect("snipe action");
        let dev_sell = actions
            .iter()
            .find(|action| action.actionId == "dev-sell")
            .expect("dev sell action");
        assert_eq!(snipe.triggerKey.as_deref(), Some("slot:0"));
        assert_eq!(dev_sell.triggerKey.as_deref(), Some("slot:0"));
        assert_eq!(snipe.orderIndex, 0);
        assert!(snipe.preSignedTransactions.is_empty());
    }

    #[test]
    fn stable_action_identity_normalizes_legacy_block_trigger_keys() {
        let mut legacy = build_action_records(&sample_follow_launch())
            .into_iter()
            .find(|action| action.actionId == "dev-sell")
            .expect("dev sell action");
        let mut current = legacy.clone();
        legacy.triggerKey = Some("block:0".to_string());
        current.triggerKey = Some("slot:0".to_string());
        assert!(json_values_equal(
            &stable_action_identity(&legacy),
            &stable_action_identity(&current)
        ));
    }

    #[test]
    fn sniper_sell_trigger_keys_are_action_specific() {
        let sell = build_action_records(&sample_follow_launch_with_sniper_sell())
            .into_iter()
            .find(|action| action.kind == FollowActionKind::SniperSell)
            .expect("sniper sell action");
        assert_eq!(
            sell.triggerKey.as_deref(),
            Some("sniper-sell:snipe-a-sell:slot:1")
        );
    }

    #[test]
    fn stable_action_identity_rewrites_legacy_sniper_sell_trigger_keys() {
        let mut legacy = build_action_records(&sample_follow_launch_with_sniper_sell())
            .into_iter()
            .find(|action| action.kind == FollowActionKind::SniperSell)
            .expect("sniper sell action");
        let mut current = legacy.clone();
        legacy.triggerKey = Some("slot:1".to_string());
        current.triggerKey = Some("sniper-sell:snipe-a-sell:slot:1".to_string());
        assert!(json_values_equal(
            &stable_action_identity(&legacy),
            &stable_action_identity(&current)
        ));
    }

    #[test]
    fn trigger_key_uses_market_cap_shape_when_present() {
        let action = FollowActionRecord {
            actionId: "sell-a".to_string(),
            kind: FollowActionKind::SniperSell,
            walletEnvKey: "WALLET_A".to_string(),
            state: FollowActionState::Queued,
            buyAmountSol: None,
            sellPercent: Some(50),
            submitDelayMs: None,
            targetBlockOffset: None,
            delayMs: None,
            marketCap: Some(FollowMarketCapTrigger {
                direction: "above".to_string(),
                threshold: "100".to_string(),
                scanTimeoutSeconds: 30,
                timeoutAction: "cancel".to_string(),
            }),
            jitterMs: None,
            feeJitterBps: None,
            precheckRequired: false,
            requireConfirmation: false,
            skipIfTokenBalancePositive: false,
            attemptCount: 0,
            scheduledForMs: None,
            eligibleAtMs: None,
            submitStartedAtMs: None,
            submittedAtMs: None,
            confirmedAtMs: None,
            provider: None,
            endpointProfile: None,
            transportType: None,
            watcherMode: None,
            watcherFallbackReason: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedTokenBalanceRaw: None,
            eligibilityObservedSlot: None,
            slotsToConfirm: None,
            signature: None,
            explorerUrl: None,
            endpoint: None,
            bundleId: None,
            lastError: None,
            triggerKey: None,
            orderIndex: 0,
            preSignedTransactions: vec![],
            poolId: None,
            primaryTxIndex: None,
            timings: FollowActionTimings::default(),
        };
        assert_eq!(
            trigger_key_for_action(&action),
            "sniper-sell:sell-a:market:above:100:30:cancel".to_string()
        );
    }

    #[test]
    fn creator_vault_rule_keeps_confirmation_zero_on_deployer_path() {
        let action = FollowActionRecord {
            actionId: "sell-a".to_string(),
            kind: FollowActionKind::DevAutoSell,
            walletEnvKey: "WALLET_A".to_string(),
            state: FollowActionState::Queued,
            buyAmountSol: None,
            sellPercent: Some(100),
            submitDelayMs: None,
            targetBlockOffset: Some(0),
            delayMs: None,
            marketCap: None,
            jitterMs: None,
            feeJitterBps: None,
            precheckRequired: false,
            requireConfirmation: true,
            skipIfTokenBalancePositive: false,
            attemptCount: 0,
            scheduledForMs: None,
            eligibleAtMs: None,
            submitStartedAtMs: None,
            submittedAtMs: None,
            confirmedAtMs: None,
            provider: None,
            endpointProfile: None,
            transportType: None,
            watcherMode: None,
            watcherFallbackReason: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedTokenBalanceRaw: None,
            eligibilityObservedSlot: None,
            slotsToConfirm: None,
            signature: None,
            explorerUrl: None,
            endpoint: None,
            bundleId: None,
            lastError: None,
            triggerKey: None,
            orderIndex: 0,
            preSignedTransactions: vec![],
            poolId: None,
            primaryTxIndex: None,
            timings: FollowActionTimings::default(),
        };
        assert!(!should_use_post_setup_creator_vault_for_sell(
            true, &action, "reduced"
        ));
    }

    #[test]
    fn creator_vault_rule_allows_post_setup_path_after_confirmation_zero() {
        let action = FollowActionRecord {
            actionId: "sell-b".to_string(),
            kind: FollowActionKind::DevAutoSell,
            walletEnvKey: "WALLET_A".to_string(),
            state: FollowActionState::Queued,
            buyAmountSol: None,
            sellPercent: Some(100),
            submitDelayMs: None,
            targetBlockOffset: Some(1),
            delayMs: None,
            marketCap: None,
            jitterMs: None,
            feeJitterBps: None,
            precheckRequired: false,
            requireConfirmation: true,
            skipIfTokenBalancePositive: false,
            attemptCount: 0,
            scheduledForMs: None,
            eligibleAtMs: None,
            submitStartedAtMs: None,
            submittedAtMs: None,
            confirmedAtMs: None,
            provider: None,
            endpointProfile: None,
            transportType: None,
            watcherMode: None,
            watcherFallbackReason: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedTokenBalanceRaw: None,
            eligibilityObservedSlot: None,
            slotsToConfirm: None,
            signature: None,
            explorerUrl: None,
            endpoint: None,
            bundleId: None,
            lastError: None,
            triggerKey: None,
            orderIndex: 0,
            preSignedTransactions: vec![],
            poolId: None,
            primaryTxIndex: None,
            timings: FollowActionTimings::default(),
        };
        assert!(should_use_post_setup_creator_vault_for_sell(
            true, &action, "reduced"
        ));
    }

    #[test]
    fn creator_vault_rule_uses_post_setup_path_immediately_for_secure_buy() {
        let action = FollowActionRecord {
            actionId: "buy-a".to_string(),
            kind: FollowActionKind::SniperBuy,
            walletEnvKey: "WALLET_A".to_string(),
            state: FollowActionState::Queued,
            buyAmountSol: Some("0.001".to_string()),
            sellPercent: None,
            submitDelayMs: None,
            targetBlockOffset: Some(0),
            delayMs: None,
            marketCap: None,
            jitterMs: None,
            feeJitterBps: None,
            precheckRequired: false,
            requireConfirmation: true,
            skipIfTokenBalancePositive: false,
            attemptCount: 0,
            scheduledForMs: None,
            eligibleAtMs: None,
            submitStartedAtMs: None,
            submittedAtMs: None,
            confirmedAtMs: None,
            provider: None,
            endpointProfile: None,
            transportType: None,
            watcherMode: None,
            watcherFallbackReason: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedTokenBalanceRaw: None,
            eligibilityObservedSlot: None,
            slotsToConfirm: None,
            signature: None,
            explorerUrl: None,
            endpoint: None,
            bundleId: None,
            lastError: None,
            triggerKey: None,
            orderIndex: 0,
            preSignedTransactions: vec![],
            poolId: None,
            primaryTxIndex: None,
            timings: FollowActionTimings::default(),
        };
        assert!(should_use_post_setup_creator_vault_for_buy(
            true, &action, "secure"
        ));
    }

    #[test]
    fn creator_vault_rule_uses_post_setup_path_immediately_for_secure_sell() {
        let action = FollowActionRecord {
            actionId: "sell-secure".to_string(),
            kind: FollowActionKind::DevAutoSell,
            walletEnvKey: "WALLET_A".to_string(),
            state: FollowActionState::Queued,
            buyAmountSol: None,
            sellPercent: Some(100),
            submitDelayMs: None,
            targetBlockOffset: Some(0),
            delayMs: None,
            marketCap: None,
            jitterMs: None,
            feeJitterBps: None,
            precheckRequired: false,
            requireConfirmation: true,
            skipIfTokenBalancePositive: false,
            attemptCount: 0,
            scheduledForMs: None,
            eligibleAtMs: None,
            submitStartedAtMs: None,
            submittedAtMs: None,
            confirmedAtMs: None,
            provider: None,
            endpointProfile: None,
            transportType: None,
            watcherMode: None,
            watcherFallbackReason: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedTokenBalanceRaw: None,
            eligibilityObservedSlot: None,
            slotsToConfirm: None,
            signature: None,
            explorerUrl: None,
            endpoint: None,
            bundleId: None,
            lastError: None,
            triggerKey: None,
            orderIndex: 0,
            preSignedTransactions: vec![],
            poolId: None,
            primaryTxIndex: None,
            timings: FollowActionTimings::default(),
        };
        assert!(should_use_post_setup_creator_vault_for_sell(
            true, &action, "secure"
        ));
    }

    #[test]
    fn creator_vault_rule_keeps_non_secure_buy_on_deployer_path_at_zero_offset() {
        let action = FollowActionRecord {
            actionId: "buy-b".to_string(),
            kind: FollowActionKind::SniperBuy,
            walletEnvKey: "WALLET_A".to_string(),
            state: FollowActionState::Queued,
            buyAmountSol: Some("0.001".to_string()),
            sellPercent: None,
            submitDelayMs: None,
            targetBlockOffset: Some(0),
            delayMs: None,
            marketCap: None,
            jitterMs: None,
            feeJitterBps: None,
            precheckRequired: false,
            requireConfirmation: true,
            skipIfTokenBalancePositive: false,
            attemptCount: 0,
            scheduledForMs: None,
            eligibleAtMs: None,
            submitStartedAtMs: None,
            submittedAtMs: None,
            confirmedAtMs: None,
            provider: None,
            endpointProfile: None,
            transportType: None,
            watcherMode: None,
            watcherFallbackReason: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedTokenBalanceRaw: None,
            eligibilityObservedSlot: None,
            slotsToConfirm: None,
            signature: None,
            explorerUrl: None,
            endpoint: None,
            bundleId: None,
            lastError: None,
            triggerKey: None,
            orderIndex: 0,
            preSignedTransactions: vec![],
            poolId: None,
            primaryTxIndex: None,
            timings: FollowActionTimings::default(),
        };
        assert!(!should_use_post_setup_creator_vault_for_buy(
            true, &action, "reduced"
        ));
    }

    #[tokio::test]
    async fn reserve_job_remains_idempotent_after_arm_and_runtime_mutation() {
        let state_path = std::env::temp_dir().join(format!(
            "launchdeck-follow-store-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = FollowDaemonStore::load_or_default(state_path.clone());
        let reserve_request = FollowReserveRequest {
            traceId: "trace-123".to_string(),
            launchpad: "pump".to_string(),
            quoteAsset: "sol".to_string(),
            launchMode: "launch".to_string(),
            selectedWalletKey: "WALLET_A".to_string(),
            followLaunch: sample_follow_launch(),
            execution: sample_execution(),
            tokenMayhemMode: false,
            wrapperDefaultFeeBps: 10,
            jitoTipAccount: String::new(),
            buyTipAccount: String::new(),
            sellTipAccount: String::new(),
            preferPostSetupCreatorVaultForSell: false,
            bagsLaunch: None,
            prebuiltActions: vec![],
            deferredSetupTransactions: vec![],
        };
        store
            .reserve_job(reserve_request.clone())
            .await
            .expect("initial reserve");
        store
            .arm_job(FollowArmRequest {
                traceId: reserve_request.traceId.clone(),
                mint: "mint".to_string(),
                launchCreator: "creator".to_string(),
                launchSignature: "sig".to_string(),
                launchTransactionSubscribeAccountRequired: vec!["payer".to_string()],
                submitAtMs: 1,
                sendObservedSlot: Some(100),
                confirmedObservedSlot: Some(101),
                reportPath: None,
                transportPlan: sample_transport_plan(),
            })
            .await
            .expect("arm");
        store
            .update_action(&reserve_request.traceId, "snipe-a", |record| {
                record.state = FollowActionState::Running;
                record.attemptCount = 1;
                record.provider = Some("standard-rpc".to_string());
                record.transportType = Some("standard-rpc".to_string());
                record.signature = Some("sig-follow".to_string());
                record.sendObservedSlot = Some(102);
            })
            .await
            .expect("runtime mutation");
        store
            .reserve_job(reserve_request)
            .await
            .expect("reserve should remain idempotent");
        let _ = std::fs::remove_file(state_path);
    }

    #[tokio::test]
    async fn repeat_arm_updates_confirm_slot_and_report_path() {
        let state_path = std::env::temp_dir().join(format!(
            "launchdeck-follow-store-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = FollowDaemonStore::load_or_default(state_path.clone());
        let reserve_request = FollowReserveRequest {
            traceId: "trace-arm-update".to_string(),
            launchpad: "pump".to_string(),
            quoteAsset: "sol".to_string(),
            launchMode: "launch".to_string(),
            selectedWalletKey: "WALLET_A".to_string(),
            followLaunch: sample_follow_launch(),
            execution: sample_execution(),
            tokenMayhemMode: false,
            wrapperDefaultFeeBps: 10,
            jitoTipAccount: String::new(),
            buyTipAccount: String::new(),
            sellTipAccount: String::new(),
            preferPostSetupCreatorVaultForSell: false,
            bagsLaunch: None,
            prebuiltActions: vec![],
            deferredSetupTransactions: vec![],
        };
        store
            .reserve_job(reserve_request.clone())
            .await
            .expect("reserve");
        store
            .arm_job(FollowArmRequest {
                traceId: reserve_request.traceId.clone(),
                mint: "mint".to_string(),
                launchCreator: "creator".to_string(),
                launchSignature: "sig".to_string(),
                launchTransactionSubscribeAccountRequired: vec!["payer".to_string()],
                submitAtMs: 1,
                sendObservedSlot: Some(100),
                confirmedObservedSlot: None,
                reportPath: None,
                transportPlan: sample_transport_plan(),
            })
            .await
            .expect("initial arm");
        let updated = store
            .arm_job(FollowArmRequest {
                traceId: reserve_request.traceId.clone(),
                mint: "mint".to_string(),
                launchCreator: "creator".to_string(),
                launchSignature: "sig".to_string(),
                launchTransactionSubscribeAccountRequired: vec![
                    "payer".to_string(),
                    "mint".to_string(),
                ],
                submitAtMs: 1,
                sendObservedSlot: Some(100),
                confirmedObservedSlot: Some(101),
                reportPath: Some("report.json".to_string()),
                transportPlan: sample_transport_plan(),
            })
            .await
            .expect("repeat arm");
        assert_eq!(updated.confirmedObservedSlot, Some(101));
        assert_eq!(updated.reportPath.as_deref(), Some("report.json"));
        assert_eq!(
            updated.launchTransactionSubscribeAccountRequired,
            vec!["payer".to_string(), "mint".to_string()]
        );
        let _ = std::fs::remove_file(state_path);
    }

    #[tokio::test]
    async fn reserve_job_rejects_changed_presigned_payloads_for_same_trace() {
        let state_path = std::env::temp_dir().join(format!(
            "launchdeck-follow-store-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = FollowDaemonStore::load_or_default(state_path.clone());
        let mut initial_request = FollowReserveRequest {
            traceId: "trace-presigned".to_string(),
            launchpad: "pump".to_string(),
            quoteAsset: "sol".to_string(),
            launchMode: "launch".to_string(),
            selectedWalletKey: "WALLET_A".to_string(),
            followLaunch: sample_follow_launch(),
            execution: sample_execution(),
            tokenMayhemMode: false,
            wrapperDefaultFeeBps: 10,
            jitoTipAccount: String::new(),
            buyTipAccount: String::new(),
            sellTipAccount: String::new(),
            preferPostSetupCreatorVaultForSell: false,
            bagsLaunch: None,
            prebuiltActions: build_action_records(&sample_follow_launch()),
            deferredSetupTransactions: vec![],
        };
        initial_request.prebuiltActions[0].preSignedTransactions =
            vec![sample_compiled_transaction("snipe-a", "blockhash-a")];
        store
            .reserve_job(initial_request.clone())
            .await
            .expect("initial reserve");
        let mut conflicting_request = initial_request.clone();
        conflicting_request.prebuiltActions[0].preSignedTransactions =
            vec![sample_compiled_transaction("snipe-a", "blockhash-b")];
        let error = store
            .reserve_job(conflicting_request)
            .await
            .expect_err("changed presigned payload should conflict");
        assert!(error.contains("Conflicting follow reserve request"));
        let _ = std::fs::remove_file(state_path);
    }

    #[tokio::test]
    async fn reserve_job_accepts_legacy_fingerprint_without_primary_tx_index() {
        #[derive(Serialize)]
        struct LegacyStableFollowActionIdentity {
            actionId: String,
            kind: FollowActionKind,
            walletEnvKey: String,
            buyAmountSol: Option<String>,
            sellPercent: Option<u8>,
            submitDelayMs: Option<u64>,
            targetBlockOffset: Option<u8>,
            delayMs: Option<u64>,
            marketCap: Option<FollowMarketCapTrigger>,
            jitterMs: Option<u64>,
            feeJitterBps: Option<u16>,
            precheckRequired: bool,
            requireConfirmation: bool,
            skipIfTokenBalancePositive: bool,
            triggerKey: Option<String>,
            orderIndex: u32,
            poolId: Option<String>,
            preSignedTransactions: Vec<CompiledTransaction>,
        }

        #[derive(Serialize)]
        struct LegacyStableFollowReservePayloadIdentity {
            actions: Vec<LegacyStableFollowActionIdentity>,
            deferredSetupTransactions: Vec<CompiledTransaction>,
        }

        fn legacy_stable_action_identity(
            action: &FollowActionRecord,
        ) -> LegacyStableFollowActionIdentity {
            LegacyStableFollowActionIdentity {
                actionId: action.actionId.clone(),
                kind: action.kind.clone(),
                walletEnvKey: action.walletEnvKey.clone(),
                buyAmountSol: action.buyAmountSol.clone(),
                sellPercent: action.sellPercent,
                submitDelayMs: action.submitDelayMs,
                targetBlockOffset: action.targetBlockOffset,
                delayMs: action.delayMs,
                marketCap: action.marketCap.clone(),
                jitterMs: action.jitterMs,
                feeJitterBps: action.feeJitterBps,
                precheckRequired: action.precheckRequired,
                requireConfirmation: action.requireConfirmation,
                skipIfTokenBalancePositive: action.skipIfTokenBalancePositive,
                triggerKey: stable_identity_trigger_key(action),
                orderIndex: action.orderIndex,
                poolId: action.poolId.clone(),
                preSignedTransactions: action.preSignedTransactions.clone(),
            }
        }

        fn legacy_reserve_payload_fingerprint(
            actions: &[FollowActionRecord],
            deferred_setup_transactions: &[CompiledTransaction],
        ) -> String {
            serde_json::to_string(&LegacyStableFollowReservePayloadIdentity {
                actions: actions.iter().map(legacy_stable_action_identity).collect(),
                deferredSetupTransactions: deferred_setup_transactions.to_vec(),
            })
            .expect("legacy reserve payload fingerprint")
        }

        let state_path = std::env::temp_dir().join(format!(
            "launchdeck-follow-store-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = FollowDaemonStore::load_or_default(state_path.clone());
        let mut request = FollowReserveRequest {
            traceId: "trace-legacy-primary-index".to_string(),
            launchpad: "pump".to_string(),
            quoteAsset: "sol".to_string(),
            launchMode: "launch".to_string(),
            selectedWalletKey: "WALLET_A".to_string(),
            followLaunch: sample_follow_launch(),
            execution: sample_execution(),
            tokenMayhemMode: false,
            wrapperDefaultFeeBps: 10,
            jitoTipAccount: String::new(),
            buyTipAccount: String::new(),
            sellTipAccount: String::new(),
            preferPostSetupCreatorVaultForSell: false,
            bagsLaunch: None,
            prebuiltActions: build_action_records(&sample_follow_launch()),
            deferredSetupTransactions: vec![],
        };
        request.prebuiltActions[0].preSignedTransactions = vec![
            sample_compiled_transaction("topup", "blockhash-a"),
            sample_compiled_transaction("buy", "blockhash-b"),
        ];
        store
            .reserve_job(request.clone())
            .await
            .expect("initial reserve");

        let legacy_fingerprint = legacy_reserve_payload_fingerprint(
            &request.prebuiltActions,
            &request.deferredSetupTransactions,
        );
        {
            let mut state = store.inner.write().await;
            let job = state
                .jobs
                .iter_mut()
                .find(|job| job.traceId == request.traceId)
                .expect("reserved job");
            job.reservedPayloadFingerprint = legacy_fingerprint;
        }

        store
            .reserve_job(request)
            .await
            .expect("legacy fingerprint should still match");
        let _ = std::fs::remove_file(state_path);
    }

    #[tokio::test]
    async fn reserve_job_rejects_changed_deferred_setup_transactions_for_same_trace() {
        let state_path = std::env::temp_dir().join(format!(
            "launchdeck-follow-store-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = FollowDaemonStore::load_or_default(state_path.clone());
        let initial_request = FollowReserveRequest {
            traceId: "trace-deferred-setup".to_string(),
            launchpad: "pump".to_string(),
            quoteAsset: "sol".to_string(),
            launchMode: "launch".to_string(),
            selectedWalletKey: "WALLET_A".to_string(),
            followLaunch: sample_follow_launch(),
            execution: sample_execution(),
            tokenMayhemMode: false,
            wrapperDefaultFeeBps: 10,
            jitoTipAccount: String::new(),
            buyTipAccount: String::new(),
            sellTipAccount: String::new(),
            preferPostSetupCreatorVaultForSell: false,
            bagsLaunch: None,
            prebuiltActions: vec![],
            deferredSetupTransactions: vec![sample_compiled_transaction(
                "deferred-setup",
                "blockhash-a",
            )],
        };
        store
            .reserve_job(initial_request.clone())
            .await
            .expect("initial reserve");
        let mut conflicting_request = initial_request.clone();
        conflicting_request.deferredSetupTransactions =
            vec![sample_compiled_transaction("deferred-setup", "blockhash-b")];
        let error = store
            .reserve_job(conflicting_request)
            .await
            .expect_err("changed deferred setup payload should conflict");
        assert!(error.contains("Conflicting follow reserve request"));
        let _ = std::fs::remove_file(state_path);
    }

    #[tokio::test]
    async fn repeat_arm_rejects_changed_launch_creator() {
        let state_path = std::env::temp_dir().join(format!(
            "launchdeck-follow-store-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = FollowDaemonStore::load_or_default(state_path.clone());
        let reserve_request = FollowReserveRequest {
            traceId: "trace-arm-creator".to_string(),
            launchpad: "pump".to_string(),
            quoteAsset: "sol".to_string(),
            launchMode: "launch".to_string(),
            selectedWalletKey: "WALLET_A".to_string(),
            followLaunch: sample_follow_launch(),
            execution: sample_execution(),
            tokenMayhemMode: false,
            wrapperDefaultFeeBps: 10,
            jitoTipAccount: String::new(),
            buyTipAccount: String::new(),
            sellTipAccount: String::new(),
            preferPostSetupCreatorVaultForSell: false,
            bagsLaunch: None,
            prebuiltActions: vec![],
            deferredSetupTransactions: vec![],
        };
        store
            .reserve_job(reserve_request.clone())
            .await
            .expect("reserve");
        store
            .arm_job(FollowArmRequest {
                traceId: reserve_request.traceId.clone(),
                mint: "mint".to_string(),
                launchCreator: "creator-a".to_string(),
                launchSignature: "sig".to_string(),
                launchTransactionSubscribeAccountRequired: vec!["payer".to_string()],
                submitAtMs: 1,
                sendObservedSlot: Some(100),
                confirmedObservedSlot: None,
                reportPath: None,
                transportPlan: sample_transport_plan(),
            })
            .await
            .expect("initial arm");
        let error = store
            .arm_job(FollowArmRequest {
                traceId: reserve_request.traceId.clone(),
                mint: "mint".to_string(),
                launchCreator: "creator-b".to_string(),
                launchSignature: "sig".to_string(),
                launchTransactionSubscribeAccountRequired: vec!["payer".to_string()],
                submitAtMs: 1,
                sendObservedSlot: Some(100),
                confirmedObservedSlot: Some(101),
                reportPath: None,
                transportPlan: sample_transport_plan(),
            })
            .await
            .expect_err("launch creator changes should conflict");
        assert!(error.contains("launch creator changed"));
        let _ = std::fs::remove_file(state_path);
    }
}
