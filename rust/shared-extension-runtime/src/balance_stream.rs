#![allow(non_snake_case, dead_code)]

//! Process-wide balance & trade subscription manager.
//!
//! One instance per engine process. Owns a single persistent WSS connection to
//! the Solana RPC endpoint and fans out `accountSubscribe` / `signatureSubscribe`
//! notifications to extension surfaces via a `tokio::sync::broadcast` channel.
//!
//! The HTTP balance path (`enrich_wallet_statuses`) remains the cold-start seed
//! and the fallback when the stream is disconnected. When the stream is live,
//! the `BalanceSnapshot` exposed by this module is the source of truth.

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{RwLock, broadcast, mpsc},
    time::sleep,
};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use crate::{
    resource_mode::idle_suspension_enabled,
    wallet::{WalletStatusSummary, WalletSummary, enrich_wallet_statuses_with_options},
};

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const RECONNECT_BASE_MS: u64 = 1_000;
const RECONNECT_CAP_MS: u64 = 30_000;
const INITIAL_SNAPSHOT_TIMEOUT_SECS: u64 = 10;
const WARM_RPC_RECOVERY_CHECK_SECS: u64 = 30;
const TRADE_TIMEOUT_SECS: u64 = 15;
const TRADE_REAP_INTERVAL_SECS: u64 = 5;
const EVENT_BUS_CAPACITY: usize = 512;
const DISPLAY_ACCOUNT_COMMITMENT: &str = "processed";
const ACCOUNTING_COMMITMENT: &str = "confirmed";

#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub ws_url: String,
    pub rpc_url: String,
    pub fallback_ws_url: Option<String>,
    pub fallback_rpc_url: Option<String>,
    pub initial_wallets: Vec<WalletSummary>,
    pub usd1_mint: String,
    pub account_commitment: String,
    pub signature_commitment: String,
}

impl StreamConfig {
    pub fn new(
        ws_url: impl Into<String>,
        rpc_url: impl Into<String>,
        usd1_mint: impl Into<String>,
    ) -> Self {
        Self {
            ws_url: ws_url.into(),
            rpc_url: rpc_url.into(),
            fallback_ws_url: None,
            fallback_rpc_url: None,
            initial_wallets: Vec::new(),
            usd1_mint: usd1_mint.into(),
            account_commitment: DISPLAY_ACCOUNT_COMMITMENT.to_string(),
            signature_commitment: ACCOUNTING_COMMITMENT.to_string(),
        }
    }

    pub fn with_initial_wallets(mut self, wallets: Vec<WalletSummary>) -> Self {
        self.initial_wallets = wallets;
        self
    }

    pub fn with_fallbacks(
        mut self,
        fallback_ws_url: impl Into<String>,
        fallback_rpc_url: impl Into<String>,
    ) -> Self {
        let fallback_ws_url = fallback_ws_url.into().trim().to_string();
        if !fallback_ws_url.is_empty() && fallback_ws_url != self.ws_url {
            self.fallback_ws_url = Some(fallback_ws_url);
        }
        let fallback_rpc_url = fallback_rpc_url.into().trim().to_string();
        if !fallback_rpc_url.is_empty() && fallback_rpc_url != self.rpc_url {
            self.fallback_rpc_url = Some(fallback_rpc_url);
        }
        self
    }

    pub fn with_account_commitment(mut self, commitment: impl Into<String>) -> Self {
        let commitment = commitment.into().trim().to_string();
        self.account_commitment = if commitment.is_empty() {
            DISPLAY_ACCOUNT_COMMITMENT.to_string()
        } else {
            commitment
        };
        self
    }

    pub fn with_signature_commitment(mut self, commitment: impl Into<String>) -> Self {
        let commitment = commitment.into().trim().to_string();
        self.signature_commitment = if commitment.is_empty() {
            ACCOUNTING_COMMITMENT.to_string()
        } else {
            commitment
        };
        self
    }
}

#[derive(Clone)]
pub struct BalanceStreamHandle {
    inner: Arc<StreamInner>,
}

struct StreamInner {
    command_tx: mpsc::UnboundedSender<StreamCommand>,
    event_tx: broadcast::Sender<StreamEvent>,
    snapshot: Arc<RwLock<SnapshotStore>>,
    connection: Arc<RwLock<ConnectionState>>,
    diagnostics: Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceEventPayload {
    pub env_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usd1_balance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_balance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_balance_raw: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_decimals: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commitment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    pub at_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBalanceCacheEventPayload {
    pub env_key: String,
    pub token_mint: String,
    pub token_balance: f64,
    pub commitment: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    pub at_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeEventPayload {
    pub client_request_id: String,
    pub batch_id: String,
    pub signature: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub err: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger_applied: Option<bool>,
    pub at_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchStatusEventPayload {
    pub batch_id: String,
    pub client_request_id: String,
    pub revision: u64,
    pub snapshot: Value,
    pub reason: String,
    pub at_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkEventPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    pub mark_revision: u64,
    pub mint: String,
    pub wallet_keys: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_balance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_balance_raw: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holding_value_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holding: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnl_gross: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnl_net: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnl_percent_gross: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnl_percent_net: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commitment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    pub at_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarketAccountEventPayload {
    pub account: String,
    pub slot: Option<u64>,
    pub at_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEventPayload {
    pub fingerprint: String,
    pub severity: String,
    pub source: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub active: bool,
    pub restart_required: bool,
    pub at_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamEvent {
    Balance(BalanceEventPayload),
    TokenBalanceCache(TokenBalanceCacheEventPayload),
    Trade(TradeEventPayload),
    BatchStatus(BatchStatusEventPayload),
    Mark(MarkEventPayload),
    MarketAccount(MarketAccountEventPayload),
    Diagnostic(DiagnosticEventPayload),
    ConnectionState {
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum ConnectionState {
    Paused { reason: String, at_ms: u128 },
    Connecting,
    Live { since_ms: u128 },
    Disconnected { reason: String, at_ms: u128 },
}

impl ConnectionState {
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionState::Paused { .. } => "paused",
            ConnectionState::Connecting => "connecting",
            ConnectionState::Live { .. } => "live",
            ConnectionState::Disconnected { .. } => "disconnected",
        }
    }

    pub fn error_message(&self) -> Option<String> {
        match self {
            ConnectionState::Paused { reason, .. } => Some(reason.clone()),
            ConnectionState::Disconnected { reason, .. } => Some(reason.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceSnapshot {
    pub wallets: Vec<WalletStatusSummary>,
    pub revision: u128,
    pub connection: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_error: Option<String>,
    pub last_event_at_ms: u128,
}

enum StreamCommand {
    ResyncWallets(Vec<WalletSummary>),
    SetActiveMints(Vec<String>),
    SetMarkAccounts(Vec<String>),
    SetDemand(bool),
    RegisterTrade {
        client_request_id: String,
        batch_id: String,
        signature: String,
    },
    Shutdown,
}

pub fn spawn(config: StreamConfig) -> BalanceStreamHandle {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
    let snapshot = Arc::new(RwLock::new(SnapshotStore::new()));
    let connection = Arc::new(RwLock::new(ConnectionState::Connecting));
    let diagnostics = Arc::new(RwLock::new(HashMap::new()));

    let worker_config = config.clone();
    let worker_event_tx = event_tx.clone();
    let worker_snapshot = snapshot.clone();
    let worker_connection = connection.clone();
    let worker_diagnostics = diagnostics.clone();
    tokio::spawn(async move {
        run_stream_worker(
            worker_config,
            command_rx,
            worker_event_tx,
            worker_snapshot,
            worker_connection,
            worker_diagnostics,
        )
        .await;
    });

    BalanceStreamHandle {
        inner: Arc::new(StreamInner {
            command_tx,
            event_tx,
            snapshot,
            connection,
            diagnostics,
        }),
    }
}

impl BalanceStreamHandle {
    pub fn subscribe_events(&self) -> broadcast::Receiver<StreamEvent> {
        self.inner.event_tx.subscribe()
    }

    pub fn resync_wallets(&self, wallets: Vec<WalletSummary>) {
        let _ = self
            .inner
            .command_tx
            .send(StreamCommand::ResyncWallets(wallets));
    }

    /// Replace the set of active non-USD1 mints whose per-wallet ATAs should be
    /// subscribed. The stream diffs against the current set and only issues
    /// `accountSubscribe` / `accountUnsubscribe` for the deltas.
    pub fn set_active_mints(&self, mints: Vec<String>) {
        let _ = self
            .inner
            .command_tx
            .send(StreamCommand::SetActiveMints(mints));
    }

    pub fn set_mark_accounts(&self, accounts: Vec<String>) {
        let _ = self
            .inner
            .command_tx
            .send(StreamCommand::SetMarkAccounts(accounts));
    }

    pub fn set_demand(&self, active: bool) {
        let _ = self.inner.command_tx.send(StreamCommand::SetDemand(active));
    }

    pub fn register_trade(
        &self,
        client_request_id: impl Into<String>,
        batch_id: impl Into<String>,
        signature: impl Into<String>,
    ) {
        let _ = self.inner.command_tx.send(StreamCommand::RegisterTrade {
            client_request_id: client_request_id.into(),
            batch_id: batch_id.into(),
            signature: signature.into(),
        });
    }

    pub fn publish_trade_event(&self, payload: TradeEventPayload) {
        let _ = self.inner.event_tx.send(StreamEvent::Trade(payload));
    }

    pub fn publish_batch_status_event(&self, payload: BatchStatusEventPayload) {
        let _ = self.inner.event_tx.send(StreamEvent::BatchStatus(payload));
    }

    pub fn publish_balance_event(&self, payload: BalanceEventPayload) {
        let _ = self.inner.event_tx.send(StreamEvent::Balance(payload));
    }

    pub fn publish_mark_event(&self, payload: MarkEventPayload) {
        let _ = self.inner.event_tx.send(StreamEvent::Mark(payload));
    }

    pub fn shutdown(&self) {
        let _ = self.inner.command_tx.send(StreamCommand::Shutdown);
    }

    pub async fn snapshot(&self) -> BalanceSnapshot {
        let snapshot = self.inner.snapshot.read().await;
        let connection = self.inner.connection.read().await;
        BalanceSnapshot {
            wallets: snapshot.wallets(),
            revision: snapshot.revision,
            connection: connection.label().to_string(),
            connection_error: connection.error_message(),
            last_event_at_ms: snapshot.last_event_at_ms,
        }
    }

    pub async fn connection_state(&self) -> ConnectionState {
        self.inner.connection.read().await.clone()
    }

    pub async fn diagnostics_snapshot(&self) -> Vec<DiagnosticEventPayload> {
        let mut diagnostics = self
            .inner
            .diagnostics
            .read()
            .await
            .values()
            .filter(|diagnostic| diagnostic.active)
            .cloned()
            .collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| {
            left.severity
                .cmp(&right.severity)
                .then(left.source.cmp(&right.source))
                .then(left.code.cmp(&right.code))
                .then(left.fingerprint.cmp(&right.fingerprint))
        });
        diagnostics
    }

    /// Hydrate the snapshot with a full list of `WalletStatusSummary` rows
    /// (for example, after a cold-start HTTP fetch or a piggy-back mint fetch).
    pub async fn hydrate_statuses(&self, statuses: &[WalletStatusSummary]) {
        if statuses.is_empty() {
            return;
        }
        let mut store = self.inner.snapshot.write().await;
        for status in statuses {
            store.upsert_full(status.clone());
        }
    }
}

#[derive(Default)]
struct SnapshotStore {
    entries: HashMap<String, WalletStatusSummary>,
    owner_to_env: HashMap<String, String>,
    ata_to_env: HashMap<String, String>,
    revision: u128,
    last_event_at_ms: u128,
}

impl SnapshotStore {
    fn new() -> Self {
        Self::default()
    }

    fn wallets(&self) -> Vec<WalletStatusSummary> {
        self.entries.values().cloned().collect()
    }

    fn upsert_full(&mut self, status: WalletStatusSummary) {
        if let Some(ref public_key) = status.publicKey {
            self.owner_to_env
                .insert(public_key.clone(), status.envKey.clone());
        }
        self.entries.insert(status.envKey.clone(), status);
        self.revision = now_ms();
    }

    fn set_sol(&mut self, env_key: &str, lamports: u64) {
        let entry =
            self.entries
                .entry(env_key.to_string())
                .or_insert_with(|| WalletStatusSummary {
                    envKey: env_key.to_string(),
                    customName: None,
                    publicKey: None,
                    error: None,
                    balanceLamports: None,
                    balanceSol: None,
                    usd1Balance: None,
                    balanceError: None,
                });
        entry.balanceLamports = Some(lamports);
        entry.balanceSol = Some(lamports as f64 / 1_000_000_000.0);
        entry.balanceError = None;
        self.revision = now_ms();
        self.last_event_at_ms = self.revision;
    }

    fn set_usd1(&mut self, env_key: &str, amount: f64) {
        let entry =
            self.entries
                .entry(env_key.to_string())
                .or_insert_with(|| WalletStatusSummary {
                    envKey: env_key.to_string(),
                    customName: None,
                    publicKey: None,
                    error: None,
                    balanceLamports: None,
                    balanceSol: None,
                    usd1Balance: None,
                    balanceError: None,
                });
        entry.usd1Balance = Some(amount);
        entry.balanceError = None;
        self.revision = now_ms();
        self.last_event_at_ms = self.revision;
    }

    fn remove(&mut self, env_key: &str) {
        if let Some(existing) = self.entries.remove(env_key)
            && let Some(public_key) = existing.publicKey
        {
            self.owner_to_env.remove(&public_key);
            self.ata_to_env
                .retain(|_, mapped| mapped.as_str() != env_key);
        }
        self.revision = now_ms();
    }
}

struct SubscriptionState {
    wallets: Vec<WalletSummary>,
    usd1_mint: String,
    account_commitment: String,
    signature_commitment: String,
    balance_demand_active: bool,
    /// Active non-USD1 mints whose per-wallet ATAs are subscribed.
    active_mints: HashSet<String>,
    /// Active route/market accounts whose changes should trigger live mark recomputation.
    mark_accounts: HashSet<String>,
    /// Subscription id -> (kind, key)
    account_subs: HashMap<u64, AccountSubKind>,
    /// Pending `accountSubscribe` acks keyed by request id.
    pending_account_acks: HashMap<i64, PendingAccountAck>,
    /// subscription id -> (client_request_id, batch_id, signature)
    trade_subs: HashMap<u64, PendingTrade>,
    pending_trade_acks: HashMap<i64, PendingTrade>,
    next_request_id: i64,
}

#[derive(Clone, PartialEq, Eq)]
enum AccountSubKind {
    WalletSol { env_key: String },
    Usd1Ata { env_key: String },
    TokenAta { env_key: String, mint: String },
    TokenAtaCache { env_key: String, mint: String },
    MarkAccount { account: String },
}

#[derive(Clone)]
struct PendingAccountAck {
    kind: AccountSubKind,
}

#[derive(Clone)]
struct PendingTrade {
    client_request_id: String,
    batch_id: String,
    signature: String,
    registered_at: Instant,
}

fn endpoint_host(endpoint: &str) -> Option<String> {
    reqwest::Url::parse(endpoint.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .or_else(|| Some("<invalid-url>".to_string()))
}

fn diagnostic_fingerprint(
    endpoint_kind: &str,
    env_var: &str,
    host: Option<&str>,
    code: &str,
) -> String {
    format!(
        "execution-engine:{}:{}:{}:{}",
        endpoint_kind,
        env_var,
        host.unwrap_or(""),
        code
    )
}

async fn publish_diagnostic(
    event_tx: &broadcast::Sender<StreamEvent>,
    diagnostics: &Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
    payload: DiagnosticEventPayload,
) {
    if payload.active {
        diagnostics
            .write()
            .await
            .insert(payload.fingerprint.clone(), payload.clone());
    } else {
        diagnostics.write().await.remove(&payload.fingerprint);
    }
    let _ = event_tx.send(StreamEvent::Diagnostic(payload));
}

async fn clear_warm_diagnostic(
    event_tx: &broadcast::Sender<StreamEvent>,
    diagnostics: &Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
    endpoint_kind: &str,
    env_var: &str,
    endpoint: &str,
    code: &str,
) {
    let host = endpoint_host(endpoint);
    let fingerprint = diagnostic_fingerprint(endpoint_kind, env_var, host.as_deref(), code);
    let existed = diagnostics.write().await.remove(&fingerprint).is_some();
    if existed {
        let _ = event_tx.send(StreamEvent::Diagnostic(DiagnosticEventPayload {
            fingerprint,
            severity: "info".to_string(),
            source: "execution-engine".to_string(),
            code: code.to_string(),
            message: "Runtime diagnostic recovered.".to_string(),
            detail: None,
            env_var: Some(env_var.to_string()),
            endpoint_kind: Some(endpoint_kind.to_string()),
            host,
            active: false,
            restart_required: false,
            at_ms: now_ms(),
        }));
    }
}

async fn clear_warm_rpc_diagnostics(
    event_tx: &broadcast::Sender<StreamEvent>,
    diagnostics: &Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
    endpoint: &str,
) {
    clear_warm_diagnostic(
        event_tx,
        diagnostics,
        "warm-rpc",
        "WARM_RPC_URL",
        endpoint,
        "warm_rpc_failed_primary_used",
    )
    .await;
    clear_warm_diagnostic(
        event_tx,
        diagnostics,
        "warm-rpc",
        "WARM_RPC_URL",
        endpoint,
        "warm_rpc_failed_primary_failed",
    )
    .await;
}

async fn emit_warm_ws_diagnostic(
    event_tx: &broadcast::Sender<StreamEvent>,
    diagnostics: &Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
    config: &StreamConfig,
    severity: &str,
    code: &str,
    message: &str,
    detail: Option<String>,
) {
    let host = endpoint_host(&config.ws_url);
    publish_diagnostic(
        event_tx,
        diagnostics,
        DiagnosticEventPayload {
            fingerprint: diagnostic_fingerprint("warm-ws", "WARM_WS_URL", host.as_deref(), code),
            severity: severity.to_string(),
            source: "execution-engine".to_string(),
            code: code.to_string(),
            message: message.to_string(),
            detail,
            env_var: Some("WARM_WS_URL".to_string()),
            endpoint_kind: Some("warm-ws".to_string()),
            host,
            active: true,
            restart_required: true,
            at_ms: now_ms(),
        },
    )
    .await;
}

async fn emit_warm_rpc_diagnostic(
    event_tx: &broadcast::Sender<StreamEvent>,
    diagnostics: &Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
    warm_rpc_url: &str,
    severity: &str,
    code: &str,
    message: &str,
    detail: Option<String>,
) {
    let host = endpoint_host(warm_rpc_url);
    publish_diagnostic(
        event_tx,
        diagnostics,
        DiagnosticEventPayload {
            fingerprint: diagnostic_fingerprint("warm-rpc", "WARM_RPC_URL", host.as_deref(), code),
            severity: severity.to_string(),
            source: "execution-engine".to_string(),
            code: code.to_string(),
            message: message.to_string(),
            detail,
            env_var: Some("WARM_RPC_URL".to_string()),
            endpoint_kind: Some("warm-rpc".to_string()),
            host,
            active: true,
            restart_required: true,
            at_ms: now_ms(),
        },
    )
    .await;
}

fn status_has_balance_error(status: &WalletStatusSummary) -> bool {
    status.error.is_none()
        && status
            .balanceError
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
}

fn status_has_usable_balance(status: &WalletStatusSummary) -> bool {
    status.balanceError.is_none()
        && (status.balanceLamports.is_some()
            || status.balanceSol.is_some()
            || status.usd1Balance.is_some())
}

async fn warm_rpc_diagnostic_is_active(
    diagnostics: &Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
    endpoint: &str,
) -> bool {
    let host = endpoint_host(endpoint);
    let used = diagnostic_fingerprint(
        "warm-rpc",
        "WARM_RPC_URL",
        host.as_deref(),
        "warm_rpc_failed_primary_used",
    );
    let failed = diagnostic_fingerprint(
        "warm-rpc",
        "WARM_RPC_URL",
        host.as_deref(),
        "warm_rpc_failed_primary_failed",
    );
    let diagnostics = diagnostics.read().await;
    diagnostics.contains_key(&used) || diagnostics.contains_key(&failed)
}

fn spawn_warm_rpc_recovery_probe(
    config: &StreamConfig,
    wallets: &[WalletSummary],
    event_tx: &broadcast::Sender<StreamEvent>,
    diagnostics: &Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
) {
    if config.fallback_rpc_url.is_none() {
        return;
    }
    let config = config.clone();
    let wallets = wallets.to_vec();
    let event_tx = event_tx.clone();
    let diagnostics = Arc::clone(diagnostics);
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(WARM_RPC_RECOVERY_CHECK_SECS)).await;
            if !warm_rpc_diagnostic_is_active(&diagnostics, &config.rpc_url).await {
                break;
            }
            if wallets.is_empty() {
                break;
            }
            let seed_result = tokio::time::timeout(
                Duration::from_secs(INITIAL_SNAPSHOT_TIMEOUT_SECS),
                enrich_wallet_statuses_with_options(
                    &config.rpc_url,
                    &config.usd1_mint,
                    &wallets,
                    false,
                ),
            )
            .await;
            let Ok(seed) = seed_result else {
                continue;
            };
            let recovered = !seed.is_empty()
                && seed.iter().any(status_has_usable_balance)
                && !seed.iter().any(status_has_balance_error);
            if recovered {
                clear_warm_rpc_diagnostics(&event_tx, &diagnostics, &config.rpc_url).await;
                break;
            }
        }
    });
}

fn merge_seed_fallback(
    warm_seed: Vec<WalletStatusSummary>,
    fallback_seed: Vec<WalletStatusSummary>,
) -> Vec<WalletStatusSummary> {
    if warm_seed.is_empty() {
        return fallback_seed;
    }
    let fallback_by_key = fallback_seed
        .into_iter()
        .map(|status| (status.envKey.clone(), status))
        .collect::<HashMap<_, _>>();
    warm_seed
        .into_iter()
        .map(|warm| {
            if !status_has_balance_error(&warm) {
                return warm;
            }
            fallback_by_key
                .get(&warm.envKey)
                .filter(|status| status_has_usable_balance(status))
                .cloned()
                .unwrap_or(warm)
        })
        .collect()
}

fn seed_fallback_recovered_warm_errors(
    warm_seed: &[WalletStatusSummary],
    fallback_seed: &[WalletStatusSummary],
    expected_wallets: &[WalletSummary],
) -> bool {
    if warm_seed.is_empty() {
        if expected_wallets.is_empty() {
            return false;
        }
        let fallback_by_key = fallback_seed
            .iter()
            .map(|status| (status.envKey.as_str(), status))
            .collect::<HashMap<_, _>>();
        return expected_wallets.iter().all(|wallet| {
            fallback_by_key
                .get(wallet.envKey.as_str())
                .is_some_and(|fallback| status_has_usable_balance(fallback))
        });
    }
    let fallback_by_key = fallback_seed
        .iter()
        .map(|status| (status.envKey.as_str(), status))
        .collect::<HashMap<_, _>>();
    warm_seed
        .iter()
        .filter(|status| status_has_balance_error(status))
        .all(|warm| {
            fallback_by_key
                .get(warm.envKey.as_str())
                .is_some_and(|fallback| status_has_usable_balance(fallback))
        })
}

fn ordered_ws_targets(config: &StreamConfig, prefer_fallback_once: bool) -> Vec<(String, bool)> {
    let primary = (config.ws_url.clone(), true);
    let Some(fallback) = config.fallback_ws_url.clone() else {
        return vec![primary];
    };
    if prefer_fallback_once {
        vec![(fallback, false), primary]
    } else {
        vec![primary, (fallback, false)]
    }
}

async fn connect_stream_socket(
    config: &StreamConfig,
    event_tx: &broadcast::Sender<StreamEvent>,
    diagnostics: &Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
    prefer_fallback_once: bool,
) -> Result<(WsStream, bool), String> {
    let mut last_error = None;
    let mut warm_error = None;
    for (endpoint, is_warm) in ordered_ws_targets(config, prefer_fallback_once) {
        match connect_async(&endpoint).await {
            Ok((ws, _resp)) => {
                if !is_warm {
                    clear_warm_diagnostic(
                        event_tx,
                        diagnostics,
                        "warm-ws",
                        "WARM_WS_URL",
                        &config.ws_url,
                        "warm_ws_failed_primary_failed",
                    )
                    .await;
                    emit_warm_ws_diagnostic(
                        event_tx,
                        diagnostics,
                        config,
                        "warning",
                        "warm_ws_failed_primary_used",
                        "Warm WS failed; using primary WS fallback.",
                        warm_error.clone(),
                    )
                    .await;
                } else {
                    clear_warm_diagnostic(
                        event_tx,
                        diagnostics,
                        "warm-ws",
                        "WARM_WS_URL",
                        &config.ws_url,
                        "warm_ws_failed_primary_used",
                    )
                    .await;
                    clear_warm_diagnostic(
                        event_tx,
                        diagnostics,
                        "warm-ws",
                        "WARM_WS_URL",
                        &config.ws_url,
                        "warm_ws_failed_primary_failed",
                    )
                    .await;
                }
                return Ok((ws, is_warm));
            }
            Err(_error) => {
                let message = "connect failed".to_string();
                if is_warm {
                    warm_error = Some(
                        "Warm WS connect failed; primary WS fallback was attempted.".to_string(),
                    );
                }
                last_error = Some(message);
            }
        }
    }
    if config.fallback_ws_url.is_some() {
        clear_warm_diagnostic(
            event_tx,
            diagnostics,
            "warm-ws",
            "WARM_WS_URL",
            &config.ws_url,
            "warm_ws_failed_primary_used",
        )
        .await;
        emit_warm_ws_diagnostic(
            event_tx,
            diagnostics,
            config,
            "critical",
            "warm_ws_failed_primary_failed",
            "Warm and primary WS failed for live balances.",
            last_error.clone(),
        )
        .await;
    }
    Err(last_error.unwrap_or_else(|| "connect failed".to_string()))
}

async fn run_stream_worker(
    config: StreamConfig,
    mut command_rx: mpsc::UnboundedReceiver<StreamCommand>,
    event_tx: broadcast::Sender<StreamEvent>,
    snapshot: Arc<RwLock<SnapshotStore>>,
    connection: Arc<RwLock<ConnectionState>>,
    diagnostics: Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
) {
    let mut state = SubscriptionState {
        wallets: config.initial_wallets.clone(),
        usd1_mint: config.usd1_mint.clone(),
        account_commitment: config.account_commitment.clone(),
        signature_commitment: config.signature_commitment.clone(),
        balance_demand_active: !idle_suspension_enabled(),
        active_mints: HashSet::new(),
        mark_accounts: HashSet::new(),
        account_subs: HashMap::new(),
        pending_account_acks: HashMap::new(),
        trade_subs: HashMap::new(),
        pending_trade_acks: HashMap::new(),
        next_request_id: 100,
    };

    let mut pending_trades: Vec<PendingTrade> = Vec::new();
    let mut reconnect_attempt: u32 = 0;
    let mut shutdown = false;
    let mut seeded = false;
    let mut prefer_primary_ws_once = false;

    while !shutdown {
        if idle_suspension_enabled() && !state.balance_demand_active && pending_trades.is_empty() {
            update_connection(
                &connection,
                ConnectionState::Paused {
                    reason: "idle-no-balance-demand".to_string(),
                    at_ms: now_ms(),
                },
            )
            .await;
            broadcast_connection(
                &event_tx,
                "paused",
                Some("idle-no-balance-demand".to_string()),
            );
            shutdown =
                wait_for_demand_or_shutdown(&mut command_rx, &mut state, &mut pending_trades).await;
            continue;
        }
        if state.balance_demand_active && !seeded {
            seed_initial_snapshot(&config, &state.wallets, &snapshot, &event_tx, &diagnostics)
                .await;
            seeded = true;
        }
        update_connection(&connection, ConnectionState::Connecting).await;
        broadcast_connection(&event_tx, "connecting", None);
        let socket_result =
            connect_stream_socket(&config, &event_tx, &diagnostics, prefer_primary_ws_once).await;
        prefer_primary_ws_once = false;
        let (mut ws, using_warm_ws) = match socket_result {
            Ok(result) => result,
            Err(error) => {
                let reason = format!("connect failed: {error}");
                update_connection(
                    &connection,
                    ConnectionState::Disconnected {
                        reason: reason.clone(),
                        at_ms: now_ms(),
                    },
                )
                .await;
                broadcast_connection(&event_tx, "disconnected", Some(reason));
                shutdown = wait_for_reconnect_or_command(
                    reconnect_attempt,
                    &mut command_rx,
                    &mut state,
                    &mut pending_trades,
                )
                .await;
                reconnect_attempt = reconnect_attempt.saturating_add(1);
                continue;
            }
        };

        let session_result = run_session(
            &mut ws,
            &mut state,
            &mut pending_trades,
            &mut command_rx,
            &event_tx,
            &snapshot,
            &connection,
            &mut reconnect_attempt,
        )
        .await;

        match session_result {
            SessionExit::Shutdown => shutdown = true,
            SessionExit::Paused => {}
            SessionExit::Disconnected(reason) => {
                if using_warm_ws && config.fallback_ws_url.is_some() {
                    prefer_primary_ws_once = true;
                    emit_warm_ws_diagnostic(
                        &event_tx,
                        &diagnostics,
                        &config,
                        "warning",
                        "warm_ws_failed_primary_used",
                        "Warm WS disconnected; using primary WS fallback.",
                        Some(
                            "Warm WS session disconnected; primary WS fallback was selected."
                                .to_string(),
                        ),
                    )
                    .await;
                }
                update_connection(
                    &connection,
                    ConnectionState::Disconnected {
                        reason: reason.clone(),
                        at_ms: now_ms(),
                    },
                )
                .await;
                broadcast_connection(&event_tx, "disconnected", Some(reason));
                // Clear subscription ids (they're invalid after disconnect).
                state.account_subs.clear();
                state.pending_account_acks.clear();
                // Preserve trade registrations so we can resubscribe after reconnect.
                let still_pending: Vec<_> = state.trade_subs.values().cloned().collect();
                pending_trades.extend(still_pending);
                state.trade_subs.clear();
                state.pending_trade_acks.clear();
                shutdown = wait_for_reconnect_or_command(
                    reconnect_attempt,
                    &mut command_rx,
                    &mut state,
                    &mut pending_trades,
                )
                .await;
                reconnect_attempt = reconnect_attempt.saturating_add(1);
            }
        }
    }
}

async fn seed_initial_snapshot(
    config: &StreamConfig,
    wallets: &[WalletSummary],
    snapshot: &Arc<RwLock<SnapshotStore>>,
    event_tx: &broadcast::Sender<StreamEvent>,
    diagnostics: &Arc<RwLock<HashMap<String, DiagnosticEventPayload>>>,
) {
    if wallets.is_empty() {
        return;
    }
    let warm_seed_result = tokio::time::timeout(
        Duration::from_secs(INITIAL_SNAPSHOT_TIMEOUT_SECS),
        enrich_wallet_statuses_with_options(&config.rpc_url, &config.usd1_mint, &wallets, false),
    )
    .await;
    let warm_seed_timed_out = warm_seed_result.is_err();
    let mut seed = warm_seed_result.unwrap_or_default();
    let warm_seed_has_errors = seed.iter().any(|status| {
        status.error.is_none()
            && status
                .balanceError
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
    });
    let warm_seed_should_fallback = warm_seed_timed_out || seed.is_empty() || warm_seed_has_errors;
    if warm_seed_should_fallback && let Some(fallback_rpc_url) = config.fallback_rpc_url.as_deref()
    {
        let fallback_seed = tokio::time::timeout(
            Duration::from_secs(INITIAL_SNAPSHOT_TIMEOUT_SECS),
            enrich_wallet_statuses_with_options(
                fallback_rpc_url,
                &config.usd1_mint,
                &wallets,
                true,
            ),
        )
        .await
        .unwrap_or_default();
        let fallback_recovered =
            seed_fallback_recovered_warm_errors(&seed, &fallback_seed, wallets);
        let merged_seed = merge_seed_fallback(seed, fallback_seed);
        if fallback_recovered {
            emit_warm_rpc_diagnostic(
                event_tx,
                diagnostics,
                &config.rpc_url,
                "warning",
                "warm_rpc_failed_primary_used",
                "Warm RPC failed; using primary RPC fallback.",
                Some("Initial balance stream snapshot recovered through primary RPC.".to_string()),
            )
            .await;
            clear_warm_diagnostic(
                event_tx,
                diagnostics,
                "warm-rpc",
                "WARM_RPC_URL",
                &config.rpc_url,
                "warm_rpc_failed_primary_failed",
            )
            .await;
            spawn_warm_rpc_recovery_probe(config, wallets, event_tx, diagnostics);
            seed = merged_seed;
        } else {
            emit_warm_rpc_diagnostic(
                event_tx,
                diagnostics,
                &config.rpc_url,
                "critical",
                "warm_rpc_failed_primary_failed",
                "Warm and primary RPC failed for balance snapshots.",
                Some(
                    "Initial balance stream snapshot could not recover through primary RPC."
                        .to_string(),
                ),
            )
            .await;
            clear_warm_diagnostic(
                event_tx,
                diagnostics,
                "warm-rpc",
                "WARM_RPC_URL",
                &config.rpc_url,
                "warm_rpc_failed_primary_used",
            )
            .await;
            spawn_warm_rpc_recovery_probe(config, wallets, event_tx, diagnostics);
            seed = merged_seed;
        }
    } else if !warm_seed_should_fallback {
        clear_warm_rpc_diagnostics(event_tx, diagnostics, &config.rpc_url).await;
    }
    if seed.is_empty() {
        return;
    }
    let mut store = snapshot.write().await;
    for status in &seed {
        store.upsert_full(status.clone());
    }
    store.last_event_at_ms = now_ms();
    let at_ms = now_ms();
    for status in &seed {
        let event = BalanceEventPayload {
            env_key: status.envKey.clone(),
            balance_sol: status.balanceSol,
            balance_lamports: status.balanceLamports,
            usd1_balance: status.usd1Balance,
            token_mint: None,
            token_balance: None,
            token_balance_raw: None,
            token_decimals: None,
            commitment: Some(ACCOUNTING_COMMITMENT.to_string()),
            source: Some("walletStatusSnapshot".to_string()),
            slot: None,
            at_ms,
        };
        let _ = event_tx.send(StreamEvent::Balance(event));
    }
}

enum SessionExit {
    Shutdown,
    Paused,
    Disconnected(String),
}

async fn run_session(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    pending_trades: &mut Vec<PendingTrade>,
    command_rx: &mut mpsc::UnboundedReceiver<StreamCommand>,
    event_tx: &broadcast::Sender<StreamEvent>,
    snapshot: &Arc<RwLock<SnapshotStore>>,
    connection: &Arc<RwLock<ConnectionState>>,
    reconnect_attempt: &mut u32,
) -> SessionExit {
    update_connection(connection, ConnectionState::Live { since_ms: now_ms() }).await;
    broadcast_connection(event_tx, "live", None);
    *reconnect_attempt = 0;

    if state.balance_demand_active {
        if let Err(error) = install_wallet_subscriptions(ws, state).await {
            return SessionExit::Disconnected(error);
        }
        if let Err(error) = install_mark_account_subscriptions(ws, state).await {
            return SessionExit::Disconnected(error);
        }
    }
    if !pending_trades.is_empty() {
        let drained = std::mem::take(pending_trades);
        for trade in drained {
            if let Err(error) = install_trade_subscription(ws, state, trade).await {
                return SessionExit::Disconnected(error);
            }
        }
    }

    loop {
        tokio::select! {
            message = ws.next() => {
                let Some(message) = message else {
                    return SessionExit::Disconnected("ws ended".to_string());
                };
                let message = match message {
                    Ok(msg) => msg,
                    Err(error) => return SessionExit::Disconnected(error.to_string()),
                };
                if let Err(error) = handle_ws_message(ws, message, state, event_tx, snapshot).await {
                    return SessionExit::Disconnected(error);
                }
            }
            command = command_rx.recv() => {
                match command {
                    Some(StreamCommand::Shutdown) => {
                        let _ = ws.send(Message::Close(None)).await;
                        return SessionExit::Shutdown;
                    }
                    Some(StreamCommand::ResyncWallets(wallets)) => {
                        if state.balance_demand_active {
                            if let Err(error) = resync_wallet_subscriptions(ws, state, wallets, snapshot).await {
                                return SessionExit::Disconnected(error);
                            }
                        } else {
                            state.wallets = wallets;
                        }
                    }
                    Some(StreamCommand::SetActiveMints(mints)) => {
                        if state.balance_demand_active {
                            if let Err(error) = resync_active_mint_subscriptions(ws, state, mints).await {
                                return SessionExit::Disconnected(error);
                            }
                        } else {
                            state.active_mints = normalize_active_mints(mints, &state.usd1_mint);
                        }
                    }
                    Some(StreamCommand::SetMarkAccounts(accounts)) => {
                        if state.balance_demand_active {
                            if let Err(error) = resync_mark_account_subscriptions(ws, state, accounts).await {
                                return SessionExit::Disconnected(error);
                            }
                        } else {
                            state.mark_accounts = normalize_mark_accounts(accounts);
                        }
                    }
                    Some(StreamCommand::SetDemand(active)) => {
                        if !idle_suspension_enabled() {
                            state.balance_demand_active = true;
                            continue;
                        }
                        let next_active = active || !state.mark_accounts.is_empty();
                        if state.balance_demand_active == next_active {
                            continue;
                        }
                        state.balance_demand_active = next_active;
                        if next_active {
                            if let Err(error) = install_wallet_subscriptions(ws, state).await {
                                return SessionExit::Disconnected(error);
                            }
                            if let Err(error) = install_mark_account_subscriptions(ws, state).await {
                                return SessionExit::Disconnected(error);
                            }
                        } else {
                            if let Err(error) = unsubscribe_account_subscriptions(ws, state).await {
                                return SessionExit::Disconnected(error);
                            }
                            if state.trade_subs.is_empty() && state.pending_trade_acks.is_empty() && pending_trades.is_empty() {
                                state.pending_account_acks.clear();
                                let _ = ws.send(Message::Close(None)).await;
                                return SessionExit::Paused;
                            }
                        }
                    }
                    Some(StreamCommand::RegisterTrade { client_request_id, batch_id, signature }) => {
                        let trade = PendingTrade {
                            client_request_id,
                            batch_id,
                            signature,
                            registered_at: Instant::now(),
                        };
                        if let Err(error) = install_trade_subscription(ws, state, trade).await {
                            return SessionExit::Disconnected(error);
                        }
                    }
                    None => return SessionExit::Shutdown,
                }
            }
            _ = sleep(Duration::from_secs(TRADE_REAP_INTERVAL_SECS)) => {
                let expired: Vec<u64> = state
                    .trade_subs
                    .iter()
                    .filter(|(_, t)| t.registered_at.elapsed() > Duration::from_secs(TRADE_TIMEOUT_SECS))
                    .map(|(id, _)| *id)
                    .collect();
                for sub_id in expired {
                    state.trade_subs.remove(&sub_id);
                    let request_id = next_request_id(state);
                    if let Err(error) =
                        send_jsonrpc(ws, request_id, "signatureUnsubscribe", json!([sub_id])).await
                    {
                        return SessionExit::Disconnected(error);
                    }
                }
                if idle_suspension_enabled()
                    && !state.balance_demand_active
                    && state.trade_subs.is_empty()
                    && state.pending_trade_acks.is_empty()
                    && pending_trades.is_empty()
                {
                    state.pending_account_acks.clear();
                    let _ = ws.send(Message::Close(None)).await;
                    return SessionExit::Paused;
                }
            }
        }
    }
}

async fn install_wallet_subscriptions(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
) -> Result<(), String> {
    let wallets = state.wallets.clone();
    for wallet in &wallets {
        install_wallet_sol_sub(ws, state, wallet).await?;
        install_wallet_usd1_sub(ws, state, wallet).await?;
    }
    // Re-install any active-mint ATA subscriptions that were desired before the
    // last disconnect. We iterate a snapshot of the active mints so we can
    // mutate `state` inside the loop.
    let mints: Vec<String> = state.active_mints.iter().cloned().collect();
    for mint in mints {
        for wallet in &wallets {
            install_wallet_token_sub(ws, state, wallet, &mint).await?;
        }
    }
    Ok(())
}

async fn install_wallet_sol_sub(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    wallet: &WalletSummary,
) -> Result<(), String> {
    let Some(public_key) = wallet.publicKey.as_ref() else {
        return Ok(());
    };
    let request_id = next_request_id(state);
    let params = account_subscribe_params(public_key, &state.account_commitment, "base64");
    state.pending_account_acks.insert(
        request_id,
        PendingAccountAck {
            kind: AccountSubKind::WalletSol {
                env_key: wallet.envKey.clone(),
            },
        },
    );
    send_jsonrpc(ws, request_id, "accountSubscribe", params).await
}

async fn install_wallet_usd1_sub(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    wallet: &WalletSummary,
) -> Result<(), String> {
    let Some(public_key) = wallet.publicKey.as_ref() else {
        return Ok(());
    };
    let owner = match Pubkey::from_str(public_key) {
        Ok(owner) => owner,
        Err(_) => return Ok(()),
    };
    let mint = match Pubkey::from_str(&state.usd1_mint) {
        Ok(mint) => mint,
        Err(_) => return Ok(()),
    };
    let ata = get_associated_token_address(&owner, &mint);
    let request_id = next_request_id(state);
    let params = account_subscribe_params(ata.to_string(), &state.account_commitment, "jsonParsed");
    state.pending_account_acks.insert(
        request_id,
        PendingAccountAck {
            kind: AccountSubKind::Usd1Ata {
                env_key: wallet.envKey.clone(),
            },
        },
    );
    send_jsonrpc(ws, request_id, "accountSubscribe", params).await
}

async fn install_wallet_token_sub(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    wallet: &WalletSummary,
    mint: &str,
) -> Result<(), String> {
    let display_commitment = state.account_commitment.clone();
    install_wallet_token_account_sub(ws, state, wallet, mint, &display_commitment, false).await?;
    if display_commitment != ACCOUNTING_COMMITMENT {
        install_wallet_token_account_sub(ws, state, wallet, mint, ACCOUNTING_COMMITMENT, true)
            .await?;
    }
    Ok(())
}

async fn install_wallet_token_account_sub(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    wallet: &WalletSummary,
    mint: &str,
    commitment: &str,
    cache_only: bool,
) -> Result<(), String> {
    let Some(public_key) = wallet.publicKey.as_ref() else {
        return Ok(());
    };
    let owner = match Pubkey::from_str(public_key) {
        Ok(owner) => owner,
        Err(_) => return Ok(()),
    };
    let mint_pk = match Pubkey::from_str(mint) {
        Ok(mint_pk) => mint_pk,
        Err(_) => return Ok(()),
    };
    let kind = if cache_only {
        AccountSubKind::TokenAtaCache {
            env_key: wallet.envKey.clone(),
            mint: mint.to_string(),
        }
    } else {
        AccountSubKind::TokenAta {
            env_key: wallet.envKey.clone(),
            mint: mint.to_string(),
        }
    };
    if account_sub_kind_already_registered(state, &kind) {
        return Ok(());
    }
    let ata = get_associated_token_address(&owner, &mint_pk);
    let request_id = next_request_id(state);
    let params = account_subscribe_params(ata.to_string(), commitment, "jsonParsed");
    state
        .pending_account_acks
        .insert(request_id, PendingAccountAck { kind });
    send_jsonrpc(ws, request_id, "accountSubscribe", params).await
}

async fn install_mark_account_sub(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    account: &str,
) -> Result<(), String> {
    let account = account.trim();
    if account.is_empty() {
        return Ok(());
    }
    let already_subscribed = state
        .account_subs
        .values()
        .any(|kind| matches!(kind, AccountSubKind::MarkAccount { account: existing } if existing == account));
    let already_pending = state
        .pending_account_acks
        .values()
        .any(|pending| matches!(&pending.kind, AccountSubKind::MarkAccount { account: existing } if existing == account));
    if already_subscribed || already_pending {
        return Ok(());
    }
    let request_id = next_request_id(state);
    let params = account_subscribe_params(account, &state.account_commitment, "base64");
    state.pending_account_acks.insert(
        request_id,
        PendingAccountAck {
            kind: AccountSubKind::MarkAccount {
                account: account.to_string(),
            },
        },
    );
    send_jsonrpc(ws, request_id, "accountSubscribe", params).await
}

async fn install_mark_account_subscriptions(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
) -> Result<(), String> {
    let accounts: Vec<String> = state.mark_accounts.iter().cloned().collect();
    for account in accounts {
        install_mark_account_sub(ws, state, &account).await?;
    }
    Ok(())
}

async fn resync_active_mint_subscriptions(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    mints: Vec<String>,
) -> Result<(), String> {
    let desired = normalize_active_mints(mints, &state.usd1_mint);
    let removed: Vec<String> = state.active_mints.difference(&desired).cloned().collect();
    let added: Vec<String> = desired.difference(&state.active_mints).cloned().collect();
    state.active_mints = desired;

    if !removed.is_empty() {
        let sub_ids_to_drop: Vec<u64> = state
            .account_subs
            .iter()
            .filter(|(_, kind)| match kind {
                AccountSubKind::TokenAta { mint, .. }
                | AccountSubKind::TokenAtaCache { mint, .. } => removed.contains(mint),
                _ => false,
            })
            .map(|(id, _)| *id)
            .collect();
        for sub_id in sub_ids_to_drop {
            state.account_subs.remove(&sub_id);
            let request_id = next_request_id(state);
            send_jsonrpc(ws, request_id, "accountUnsubscribe", json!([sub_id])).await?;
        }
    }

    if !added.is_empty() {
        let wallets = state.wallets.clone();
        for mint in added {
            for wallet in &wallets {
                install_wallet_token_sub(ws, state, wallet, &mint).await?;
            }
        }
    }
    Ok(())
}

fn normalize_active_mints(mints: Vec<String>, usd1_mint: &str) -> HashSet<String> {
    mints
        .into_iter()
        .map(|mint| mint.trim().to_string())
        .filter(|mint| !mint.is_empty() && mint != usd1_mint)
        .collect()
}

fn normalize_mark_accounts(accounts: Vec<String>) -> HashSet<String> {
    accounts
        .into_iter()
        .map(|account| account.trim().to_string())
        .filter(|account| !account.is_empty())
        .collect()
}

fn account_sub_kind_already_registered(state: &SubscriptionState, kind: &AccountSubKind) -> bool {
    state.account_subs.values().any(|existing| existing == kind)
}

fn account_sub_kind_is_desired(state: &SubscriptionState, kind: &AccountSubKind) -> bool {
    match kind {
        AccountSubKind::WalletSol { env_key } | AccountSubKind::Usd1Ata { env_key } => {
            state.balance_demand_active && wallet_env_is_active(state, env_key)
        }
        AccountSubKind::TokenAta { env_key, mint }
        | AccountSubKind::TokenAtaCache { env_key, mint } => {
            state.balance_demand_active
                && wallet_env_is_active(state, env_key)
                && state.active_mints.contains(mint)
        }
        AccountSubKind::MarkAccount { account } => {
            state.balance_demand_active && state.mark_accounts.contains(account)
        }
    }
}

fn wallet_env_is_active(state: &SubscriptionState, env_key: &str) -> bool {
    state.wallets.iter().any(|wallet| wallet.envKey == env_key)
}

async fn resync_mark_account_subscriptions(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    accounts: Vec<String>,
) -> Result<(), String> {
    let desired = normalize_mark_accounts(accounts);
    let removed: Vec<String> = state.mark_accounts.difference(&desired).cloned().collect();
    let added: Vec<String> = desired.difference(&state.mark_accounts).cloned().collect();
    state.mark_accounts = desired;

    if !removed.is_empty() {
        let sub_ids_to_drop: Vec<u64> = state
            .account_subs
            .iter()
            .filter(
                |(_, kind)| matches!(kind, AccountSubKind::MarkAccount { account } if removed.contains(account)),
            )
            .map(|(id, _)| *id)
            .collect();
        for sub_id in sub_ids_to_drop {
            state.account_subs.remove(&sub_id);
            let request_id = next_request_id(state);
            send_jsonrpc(ws, request_id, "accountUnsubscribe", json!([sub_id])).await?;
        }
    }

    for account in added {
        install_mark_account_sub(ws, state, &account).await?;
    }
    Ok(())
}

fn account_subscribe_params(account: impl Into<String>, commitment: &str, encoding: &str) -> Value {
    json!([
        account.into(),
        { "commitment": commitment, "encoding": encoding }
    ])
}

fn signature_subscribe_params(signature: impl Into<String>, commitment: &str) -> Value {
    json!([
        signature.into(),
        { "commitment": commitment }
    ])
}

async fn unsubscribe_account_subscriptions(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
) -> Result<(), String> {
    let sub_ids_to_drop: Vec<u64> = state.account_subs.keys().copied().collect();
    for sub_id in sub_ids_to_drop {
        state.account_subs.remove(&sub_id);
        let request_id = next_request_id(state);
        send_jsonrpc(ws, request_id, "accountUnsubscribe", json!([sub_id])).await?;
    }
    Ok(())
}

async fn install_trade_subscription(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    trade: PendingTrade,
) -> Result<(), String> {
    let request_id = next_request_id(state);
    let params = signature_subscribe_params(trade.signature.clone(), &state.signature_commitment);
    state.pending_trade_acks.insert(request_id, trade);
    send_jsonrpc(ws, request_id, "signatureSubscribe", params).await
}

async fn resync_wallet_subscriptions(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    next_wallets: Vec<WalletSummary>,
    snapshot: &Arc<RwLock<SnapshotStore>>,
) -> Result<(), String> {
    let old_keys: HashSet<String> = state.wallets.iter().map(|w| w.envKey.clone()).collect();
    let new_keys: HashSet<String> = next_wallets.iter().map(|w| w.envKey.clone()).collect();
    let removed_keys: Vec<String> = old_keys.difference(&new_keys).cloned().collect();
    let added: Vec<WalletSummary> = next_wallets
        .iter()
        .filter(|w| !old_keys.contains(&w.envKey))
        .cloned()
        .collect();
    state.wallets = next_wallets;

    // Unsubscribe all subs whose env_key is no longer present.
    if !removed_keys.is_empty() {
        let sub_ids_to_drop: Vec<u64> = state
            .account_subs
            .iter()
            .filter(|(_, kind)| match kind {
                AccountSubKind::WalletSol { env_key }
                | AccountSubKind::Usd1Ata { env_key }
                | AccountSubKind::TokenAta { env_key, .. }
                | AccountSubKind::TokenAtaCache { env_key, .. } => removed_keys.contains(env_key),
                AccountSubKind::MarkAccount { .. } => false,
            })
            .map(|(id, _)| *id)
            .collect();
        for sub_id in sub_ids_to_drop {
            state.account_subs.remove(&sub_id);
            let request_id = next_request_id(state);
            send_jsonrpc(ws, request_id, "accountUnsubscribe", json!([sub_id])).await?;
        }
        let mut store = snapshot.write().await;
        for env_key in &removed_keys {
            store.remove(env_key);
        }
    }

    for wallet in &added {
        install_wallet_sol_sub(ws, state, wallet).await?;
        install_wallet_usd1_sub(ws, state, wallet).await?;
        let mints: Vec<String> = state.active_mints.iter().cloned().collect();
        for mint in mints {
            install_wallet_token_sub(ws, state, wallet, &mint).await?;
        }
    }

    Ok(())
}

fn next_request_id(state: &mut SubscriptionState) -> i64 {
    let id = state.next_request_id;
    state.next_request_id = state.next_request_id.wrapping_add(1);
    id
}

async fn send_jsonrpc(
    ws: &mut WsStream,
    request_id: i64,
    method: &str,
    params: Value,
) -> Result<(), String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
        "params": params,
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|error| error.to_string())
}

async fn handle_ws_message(
    ws: &mut WsStream,
    message: Message,
    state: &mut SubscriptionState,
    event_tx: &broadcast::Sender<StreamEvent>,
    snapshot: &Arc<RwLock<SnapshotStore>>,
) -> Result<(), String> {
    let text = match message {
        Message::Text(text) => text.to_string(),
        Message::Binary(bytes) => String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())?,
        Message::Ping(payload) => {
            ws.send(Message::Pong(payload))
                .await
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        Message::Pong(_) | Message::Frame(_) => return Ok(()),
        Message::Close(_) => return Err("remote close".to_string()),
    };
    let payload: Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid WS json: {e}"))?;

    if let Some(request_id) = payload.get("id").and_then(Value::as_i64) {
        handle_jsonrpc_ack(ws, request_id, &payload, state).await?;
        return Ok(());
    }

    let method = payload
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match method {
        "accountNotification" => {
            handle_account_notification(&payload, state, event_tx, snapshot).await
        }
        "signatureNotification" => {
            handle_signature_notification(&payload, state, event_tx, snapshot).await
        }
        _ => Ok(()),
    }
}

async fn handle_jsonrpc_ack(
    ws: &mut WsStream,
    request_id: i64,
    payload: &Value,
    state: &mut SubscriptionState,
) -> Result<(), String> {
    if payload.get("error").is_some() {
        state.pending_account_acks.remove(&request_id);
        state.pending_trade_acks.remove(&request_id);
        return Ok(());
    }
    if let Some(pending) = state.pending_account_acks.remove(&request_id) {
        let Some(sub_id) = payload.get("result").and_then(Value::as_u64) else {
            return Ok(());
        };
        if !account_sub_kind_is_desired(state, &pending.kind)
            || state
                .account_subs
                .values()
                .any(|existing| existing == &pending.kind)
        {
            let unsubscribe_id = next_request_id(state);
            send_jsonrpc(ws, unsubscribe_id, "accountUnsubscribe", json!([sub_id])).await?;
            return Ok(());
        }
        state.account_subs.insert(sub_id, pending.kind);
        return Ok(());
    }
    if let Some(pending) = state.pending_trade_acks.remove(&request_id) {
        let Some(sub_id) = payload.get("result").and_then(Value::as_u64) else {
            return Ok(());
        };
        state.trade_subs.insert(sub_id, pending);
        return Ok(());
    }
    Ok(())
}

async fn handle_account_notification(
    payload: &Value,
    state: &mut SubscriptionState,
    event_tx: &broadcast::Sender<StreamEvent>,
    snapshot: &Arc<RwLock<SnapshotStore>>,
) -> Result<(), String> {
    let Some(params) = payload.get("params") else {
        return Ok(());
    };
    let Some(subscription_id) = params.get("subscription").and_then(Value::as_u64) else {
        return Ok(());
    };
    let Some(kind) = state.account_subs.get(&subscription_id).cloned() else {
        return Ok(());
    };
    let value = params
        .get("result")
        .and_then(|r| r.get("value"))
        .cloned()
        .unwrap_or(Value::Null);
    let slot = params
        .get("result")
        .and_then(|r| r.get("context"))
        .and_then(|c| c.get("slot"))
        .and_then(Value::as_u64);

    match kind {
        AccountSubKind::WalletSol { env_key } => {
            let Some(lamports) = value.get("lamports").and_then(Value::as_u64) else {
                return Ok(());
            };
            {
                let mut store = snapshot.write().await;
                store.set_sol(&env_key, lamports);
            }
            let _ = event_tx.send(StreamEvent::Balance(BalanceEventPayload {
                env_key,
                balance_sol: Some(lamports as f64 / 1_000_000_000.0),
                balance_lamports: Some(lamports),
                usd1_balance: None,
                token_mint: None,
                token_balance: None,
                token_balance_raw: None,
                token_decimals: None,
                commitment: Some(state.account_commitment.clone()),
                source: Some("accountSubscribe".to_string()),
                slot,
                at_ms: now_ms(),
            }));
        }
        AccountSubKind::Usd1Ata { env_key } => {
            let amount = parse_token_ui_amount(&value);
            if let Some(amount) = amount {
                {
                    let mut store = snapshot.write().await;
                    store.set_usd1(&env_key, amount);
                }
                let _ = event_tx.send(StreamEvent::Balance(BalanceEventPayload {
                    env_key,
                    balance_sol: None,
                    balance_lamports: None,
                    usd1_balance: Some(amount),
                    token_mint: None,
                    token_balance: None,
                    token_balance_raw: None,
                    token_decimals: None,
                    commitment: Some(state.account_commitment.clone()),
                    source: Some("accountSubscribe".to_string()),
                    slot,
                    at_ms: now_ms(),
                }));
            }
        }
        AccountSubKind::TokenAta { env_key, mint } => {
            // We don't cache non-USD1 token balances in the snapshot store;
            // they're ephemeral per-mint views consumed by the active panel.
            // Emit the balance event directly so the panel can render it.
            let amount = parse_token_ui_amount(&value);
            if let Some(amount) = amount {
                let _ = event_tx.send(StreamEvent::Balance(BalanceEventPayload {
                    env_key,
                    balance_sol: None,
                    balance_lamports: None,
                    usd1_balance: None,
                    token_mint: Some(mint),
                    token_balance: Some(amount),
                    token_balance_raw: None,
                    token_decimals: None,
                    commitment: Some(state.account_commitment.clone()),
                    source: Some("accountSubscribe".to_string()),
                    slot,
                    at_ms: now_ms(),
                }));
            }
        }
        AccountSubKind::TokenAtaCache { env_key, mint } => {
            let amount = parse_token_ui_amount(&value);
            if let Some(amount) = amount {
                let _ = event_tx.send(StreamEvent::TokenBalanceCache(
                    TokenBalanceCacheEventPayload {
                        env_key,
                        token_mint: mint,
                        token_balance: amount,
                        commitment: ACCOUNTING_COMMITMENT.to_string(),
                        source: Some("accountSubscribe".to_string()),
                        slot,
                        at_ms: now_ms(),
                    },
                ));
            }
        }
        AccountSubKind::MarkAccount { account } => {
            let _ = event_tx.send(StreamEvent::MarketAccount(MarketAccountEventPayload {
                account,
                slot,
                at_ms: now_ms(),
            }));
        }
    }
    Ok(())
}

fn parse_token_ui_amount(value: &Value) -> Option<f64> {
    // jsonParsed shape: value.data.parsed.info.tokenAmount.{uiAmount,uiAmountString}
    let amount = value
        .pointer("/data/parsed/info/tokenAmount/uiAmount")
        .and_then(Value::as_f64);
    if amount.is_some() {
        return amount;
    }
    value
        .pointer("/data/parsed/info/tokenAmount/uiAmountString")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())
}

async fn handle_signature_notification(
    payload: &Value,
    state: &mut SubscriptionState,
    event_tx: &broadcast::Sender<StreamEvent>,
    _snapshot: &Arc<RwLock<SnapshotStore>>,
) -> Result<(), String> {
    let Some(params) = payload.get("params") else {
        return Ok(());
    };
    let Some(subscription_id) = params.get("subscription").and_then(Value::as_u64) else {
        return Ok(());
    };
    let Some(trade) = state.trade_subs.remove(&subscription_id) else {
        return Ok(());
    };
    let slot = params
        .get("result")
        .and_then(|r| r.get("context"))
        .and_then(|c| c.get("slot"))
        .and_then(Value::as_u64);
    let err_value = params
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.get("err"))
        .cloned()
        .filter(|v| !v.is_null());
    let status = if err_value.is_some() {
        "failed".to_string()
    } else {
        "confirmed".to_string()
    };
    let _ = event_tx.send(StreamEvent::Trade(TradeEventPayload {
        client_request_id: trade.client_request_id,
        batch_id: trade.batch_id,
        signature: trade.signature,
        status,
        slot,
        err: err_value,
        ledger_applied: None,
        at_ms: now_ms(),
    }));
    Ok(())
}

async fn wait_for_reconnect_or_command(
    attempt: u32,
    command_rx: &mut mpsc::UnboundedReceiver<StreamCommand>,
    state: &mut SubscriptionState,
    pending_trades: &mut Vec<PendingTrade>,
) -> bool {
    let delay_ms = reconnect_delay_ms(attempt);
    let deadline = tokio::time::Instant::now() + Duration::from_millis(delay_ms);
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return false,
            command = command_rx.recv() => {
                match command {
                    Some(StreamCommand::Shutdown) | None => return true,
                    Some(StreamCommand::ResyncWallets(wallets)) => {
                        state.wallets = wallets;
                    }
                    Some(StreamCommand::SetActiveMints(mints)) => {
                        // Defer actual ATA subscriptions until the next session
                        // is live. Simply update the desired set.
                        state.active_mints = normalize_active_mints(mints, &state.usd1_mint);
                    }
                    Some(StreamCommand::SetMarkAccounts(accounts)) => {
                        state.mark_accounts = normalize_mark_accounts(accounts);
                        if !state.mark_accounts.is_empty() {
                            state.balance_demand_active = true;
                            return false;
                        }
                    }
                    Some(StreamCommand::SetDemand(active)) => {
                        if !idle_suspension_enabled() {
                            state.balance_demand_active = true;
                            return false;
                        }
                        state.balance_demand_active = active || !state.mark_accounts.is_empty();
                        if state.balance_demand_active {
                            return false;
                        }
                    }
                    Some(StreamCommand::RegisterTrade { client_request_id, batch_id, signature }) => {
                        pending_trades.push(PendingTrade {
                            client_request_id,
                            batch_id,
                            signature,
                            registered_at: Instant::now(),
                        });
                        return false;
                    }
                }
            }
        }
    }
}

async fn wait_for_demand_or_shutdown(
    command_rx: &mut mpsc::UnboundedReceiver<StreamCommand>,
    state: &mut SubscriptionState,
    pending_trades: &mut Vec<PendingTrade>,
) -> bool {
    loop {
        match command_rx.recv().await {
            Some(StreamCommand::Shutdown) | None => return true,
            Some(StreamCommand::ResyncWallets(wallets)) => {
                state.wallets = wallets;
            }
            Some(StreamCommand::SetActiveMints(mints)) => {
                state.active_mints = normalize_active_mints(mints, &state.usd1_mint);
            }
            Some(StreamCommand::SetMarkAccounts(accounts)) => {
                state.mark_accounts = normalize_mark_accounts(accounts);
                if !state.mark_accounts.is_empty() {
                    state.balance_demand_active = true;
                    return false;
                }
            }
            Some(StreamCommand::SetDemand(active)) => {
                if !idle_suspension_enabled() {
                    state.balance_demand_active = true;
                    return false;
                }
                state.balance_demand_active = active || !state.mark_accounts.is_empty();
                if state.balance_demand_active {
                    return false;
                }
            }
            Some(StreamCommand::RegisterTrade {
                client_request_id,
                batch_id,
                signature,
            }) => {
                pending_trades.push(PendingTrade {
                    client_request_id,
                    batch_id,
                    signature,
                    registered_at: Instant::now(),
                });
                return false;
            }
        }
    }
}

fn reconnect_delay_ms(attempt: u32) -> u64 {
    let capped_attempt = attempt.min(10) as u64;
    let delay = RECONNECT_BASE_MS.saturating_mul(1u64 << capped_attempt);
    delay.min(RECONNECT_CAP_MS)
}

async fn update_connection(store: &Arc<RwLock<ConnectionState>>, next: ConnectionState) {
    let mut guard = store.write().await;
    *guard = next;
}

fn broadcast_connection(
    event_tx: &broadcast::Sender<StreamEvent>,
    state: &str,
    error: Option<String>,
) {
    let _ = event_tx.send(StreamEvent::ConnectionState {
        state: state.to_string(),
        error,
    });
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(key: &str, lamports: Option<u64>, error: Option<&str>) -> WalletStatusSummary {
        WalletStatusSummary {
            envKey: key.to_string(),
            customName: None,
            publicKey: Some(format!("{key}Pubkey")),
            error: None,
            balanceLamports: lamports,
            balanceSol: lamports.map(|value| value as f64 / 1_000_000_000.0),
            usd1Balance: None,
            balanceError: error.map(str::to_string),
        }
    }

    fn wallet(key: &str) -> WalletSummary {
        WalletSummary {
            envKey: key.to_string(),
            customName: None,
            publicKey: Some(format!("{key}Pubkey")),
            error: None,
        }
    }

    fn subscription_state(wallets: Vec<WalletSummary>) -> SubscriptionState {
        SubscriptionState {
            wallets,
            usd1_mint: "USD1".to_string(),
            account_commitment: DISPLAY_ACCOUNT_COMMITMENT.to_string(),
            signature_commitment: ACCOUNTING_COMMITMENT.to_string(),
            balance_demand_active: true,
            active_mints: HashSet::new(),
            mark_accounts: HashSet::new(),
            account_subs: HashMap::new(),
            pending_account_acks: HashMap::new(),
            trade_subs: HashMap::new(),
            pending_trade_acks: HashMap::new(),
            next_request_id: 100,
        }
    }

    #[test]
    fn token_account_ack_is_stale_after_active_mint_removed() {
        let mut state = subscription_state(vec![wallet("SOLANA_PRIVATE_KEY")]);
        state.active_mints.insert("Mint111".to_string());
        let display_kind = AccountSubKind::TokenAta {
            env_key: "SOLANA_PRIVATE_KEY".to_string(),
            mint: "Mint111".to_string(),
        };
        let cache_kind = AccountSubKind::TokenAtaCache {
            env_key: "SOLANA_PRIVATE_KEY".to_string(),
            mint: "Mint111".to_string(),
        };

        assert!(account_sub_kind_is_desired(&state, &display_kind));
        assert!(account_sub_kind_is_desired(&state, &cache_kind));

        state.active_mints.clear();

        assert!(!account_sub_kind_is_desired(&state, &display_kind));
        assert!(!account_sub_kind_is_desired(&state, &cache_kind));
    }

    #[test]
    fn wallet_account_ack_is_stale_after_wallet_removed() {
        let mut state = subscription_state(vec![wallet("SOLANA_PRIVATE_KEY")]);
        state.active_mints.insert("Mint111".to_string());
        let sol_kind = AccountSubKind::WalletSol {
            env_key: "SOLANA_PRIVATE_KEY".to_string(),
        };
        let usd1_kind = AccountSubKind::Usd1Ata {
            env_key: "SOLANA_PRIVATE_KEY".to_string(),
        };
        let token_kind = AccountSubKind::TokenAta {
            env_key: "SOLANA_PRIVATE_KEY".to_string(),
            mint: "Mint111".to_string(),
        };

        assert!(account_sub_kind_is_desired(&state, &sol_kind));
        assert!(account_sub_kind_is_desired(&state, &usd1_kind));
        assert!(account_sub_kind_is_desired(&state, &token_kind));

        state.wallets.clear();

        assert!(!account_sub_kind_is_desired(&state, &sol_kind));
        assert!(!account_sub_kind_is_desired(&state, &usd1_kind));
        assert!(!account_sub_kind_is_desired(&state, &token_kind));
    }

    #[test]
    fn seed_merge_replaces_only_warm_balance_errors() {
        let warm = vec![
            status("SOLANA_PRIVATE_KEY", None, Some("warm failed")),
            status("SOLANA_PRIVATE_KEY2", Some(7), None),
        ];
        let fallback = vec![
            status("SOLANA_PRIVATE_KEY", Some(8), None),
            status("SOLANA_PRIVATE_KEY2", Some(9), None),
        ];

        let merged = merge_seed_fallback(warm, fallback);

        assert_eq!(merged[0].balanceLamports, Some(8));
        assert_eq!(merged[0].balanceError, None);
        assert_eq!(merged[1].balanceLamports, Some(7));
    }

    #[test]
    fn seed_merge_uses_fallback_when_warm_seed_is_empty() {
        let fallback = vec![status("SOLANA_PRIVATE_KEY", Some(8), None)];

        let merged = merge_seed_fallback(Vec::new(), fallback);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].balanceLamports, Some(8));
    }

    #[test]
    fn seed_fallback_requires_every_warm_error_to_recover() {
        let warm = vec![
            status("SOLANA_PRIVATE_KEY", None, Some("warm failed")),
            status("SOLANA_PRIVATE_KEY2", None, Some("warm failed")),
            status("SOLANA_PRIVATE_KEY3", Some(7), None),
        ];
        let partial_fallback = vec![
            status("SOLANA_PRIVATE_KEY", Some(8), None),
            status("SOLANA_PRIVATE_KEY2", None, Some("primary failed")),
        ];
        let full_fallback = vec![
            status("SOLANA_PRIVATE_KEY", Some(8), None),
            status("SOLANA_PRIVATE_KEY2", Some(9), None),
        ];

        let expected = vec![
            wallet("SOLANA_PRIVATE_KEY"),
            wallet("SOLANA_PRIVATE_KEY2"),
            wallet("SOLANA_PRIVATE_KEY3"),
        ];

        assert!(!seed_fallback_recovered_warm_errors(
            &warm,
            &partial_fallback,
            &expected
        ));
        assert!(seed_fallback_recovered_warm_errors(
            &warm,
            &full_fallback,
            &expected
        ));
    }

    #[test]
    fn empty_warm_seed_requires_all_expected_wallets_to_recover() {
        let expected = vec![wallet("SOLANA_PRIVATE_KEY"), wallet("SOLANA_PRIVATE_KEY2")];
        let partial_fallback = vec![status("SOLANA_PRIVATE_KEY", Some(8), None)];
        let full_fallback = vec![
            status("SOLANA_PRIVATE_KEY", Some(8), None),
            status("SOLANA_PRIVATE_KEY2", Some(9), None),
        ];

        assert!(!seed_fallback_recovered_warm_errors(
            &[],
            &partial_fallback,
            &expected
        ));
        assert!(seed_fallback_recovered_warm_errors(
            &[],
            &full_fallback,
            &expected
        ));
    }

    #[test]
    fn stream_config_splits_display_and_accounting_commitments() {
        let config =
            StreamConfig::new("wss://rpc.example.test", "https://rpc.example.test", "USD1");

        assert_eq!(config.account_commitment, DISPLAY_ACCOUNT_COMMITMENT);
        assert_eq!(config.signature_commitment, ACCOUNTING_COMMITMENT);
    }

    #[test]
    fn subscription_params_use_split_commitments() {
        let account_params = account_subscribe_params(
            "Wallet111111111111111111111111111111111",
            "processed",
            "base64",
        );
        let signature_params = signature_subscribe_params(
            "Sig111111111111111111111111111111111111111111111111",
            "confirmed",
        );

        assert_eq!(
            account_params,
            json!([
                "Wallet111111111111111111111111111111111",
                { "commitment": "processed", "encoding": "base64" }
            ])
        );
        assert_eq!(
            signature_params,
            json!([
                "Sig111111111111111111111111111111111111111111111111",
                { "commitment": "confirmed" }
            ])
        );
    }

    #[test]
    fn balance_event_serializes_display_metadata() {
        let event = BalanceEventPayload {
            env_key: "SOLANA_PRIVATE_KEY".to_string(),
            balance_sol: Some(1.0),
            balance_lamports: Some(1_000_000_000),
            usd1_balance: None,
            token_mint: None,
            token_balance: None,
            token_balance_raw: None,
            token_decimals: None,
            commitment: Some("processed".to_string()),
            source: Some("accountSubscribe".to_string()),
            slot: Some(123),
            at_ms: 456,
        };

        let payload = serde_json::to_value(event).expect("serialize balance event");

        assert_eq!(payload.get("commitment"), Some(&json!("processed")));
        assert_eq!(payload.get("source"), Some(&json!("accountSubscribe")));
        assert_eq!(payload.get("slot"), Some(&json!(123)));
    }

    #[test]
    fn token_balance_cache_event_serializes_accounting_metadata() {
        let event = TokenBalanceCacheEventPayload {
            env_key: "SOLANA_PRIVATE_KEY".to_string(),
            token_mint: "Mint111".to_string(),
            token_balance: 42.0,
            commitment: "confirmed".to_string(),
            source: Some("accountSubscribe".to_string()),
            slot: Some(123),
            at_ms: 456,
        };

        let payload = serde_json::to_value(event).expect("serialize token cache event");

        assert_eq!(payload.get("commitment"), Some(&json!("confirmed")));
        assert_eq!(payload.get("tokenBalance"), Some(&json!(42.0)));
        assert_eq!(payload.get("slot"), Some(&json!(123)));
    }

    #[test]
    fn mark_event_serializes_live_pnl_fields() {
        let event = MarkEventPayload {
            surface_id: Some("content:test".to_string()),
            mark_revision: 1,
            mint: "Mint111111111111111111111111111111111111111".to_string(),
            wallet_keys: vec!["SOLANA_PRIVATE_KEY".to_string()],
            wallet_group_id: None,
            token_balance: Some(42.0),
            token_balance_raw: Some(42_000_000),
            holding_value_sol: Some(1.25),
            holding: Some(1.25),
            pnl_gross: Some(0.25),
            pnl_net: Some(0.24),
            pnl_percent_gross: Some(25.0),
            pnl_percent_net: Some(24.0),
            quote_source: Some("live-mark:pump-amm".to_string()),
            commitment: Some("processed".to_string()),
            slot: Some(321),
            at_ms: 654,
        };

        let payload = serde_json::to_value(event).expect("serialize mark event");

        assert_eq!(payload.get("holdingValueSol"), Some(&json!(1.25)));
        assert_eq!(payload.get("surfaceId"), Some(&json!("content:test")));
        assert_eq!(payload.get("markRevision"), Some(&json!(1)));
        assert_eq!(payload.get("pnlGross"), Some(&json!(0.25)));
        assert_eq!(payload.get("commitment"), Some(&json!("processed")));
        assert_eq!(payload.get("slot"), Some(&json!(321)));
    }

    #[test]
    fn diagnostic_fingerprint_is_query_safe() {
        let host = endpoint_host("wss://rpc.example.test/path?api-key=secret");
        let fingerprint =
            diagnostic_fingerprint("warm-ws", "WARM_WS_URL", host.as_deref(), "warm_ws_failed");

        assert_eq!(host.as_deref(), Some("rpc.example.test"));
        assert!(fingerprint.contains("rpc.example.test"));
        assert!(!fingerprint.contains("secret"));
    }
}
