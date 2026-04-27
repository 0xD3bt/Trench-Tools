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
const TRADE_TIMEOUT_SECS: u64 = 15;
const TRADE_REAP_INTERVAL_SECS: u64 = 5;
const EVENT_BUS_CAPACITY: usize = 512;

#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub ws_url: String,
    pub rpc_url: String,
    pub usd1_mint: String,
    pub commitment: String,
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
            usd1_mint: usd1_mint.into(),
            commitment: "confirmed".to_string(),
        }
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
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamEvent {
    Balance(BalanceEventPayload),
    Trade(TradeEventPayload),
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

    let worker_config = config.clone();
    let worker_event_tx = event_tx.clone();
    let worker_snapshot = snapshot.clone();
    let worker_connection = connection.clone();
    tokio::spawn(async move {
        run_stream_worker(
            worker_config,
            command_rx,
            worker_event_tx,
            worker_snapshot,
            worker_connection,
        )
        .await;
    });

    BalanceStreamHandle {
        inner: Arc::new(StreamInner {
            command_tx,
            event_tx,
            snapshot,
            connection,
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
    commitment: String,
    balance_demand_active: bool,
    /// Active non-USD1 mints whose per-wallet ATAs are subscribed.
    active_mints: HashSet<String>,
    /// Subscription id -> (kind, key)
    account_subs: HashMap<u64, AccountSubKind>,
    /// Pending `accountSubscribe` acks keyed by request id.
    pending_account_acks: HashMap<i64, PendingAccountAck>,
    /// subscription id -> (client_request_id, batch_id, signature)
    trade_subs: HashMap<u64, PendingTrade>,
    pending_trade_acks: HashMap<i64, PendingTrade>,
    next_request_id: i64,
}

#[derive(Clone)]
enum AccountSubKind {
    WalletSol { env_key: String },
    Usd1Ata { env_key: String },
    TokenAta { env_key: String, mint: String },
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

async fn run_stream_worker(
    config: StreamConfig,
    mut command_rx: mpsc::UnboundedReceiver<StreamCommand>,
    event_tx: broadcast::Sender<StreamEvent>,
    snapshot: Arc<RwLock<SnapshotStore>>,
    connection: Arc<RwLock<ConnectionState>>,
) {
    let mut state = SubscriptionState {
        wallets: Vec::new(),
        usd1_mint: config.usd1_mint.clone(),
        commitment: config.commitment.clone(),
        balance_demand_active: !idle_suspension_enabled(),
        active_mints: HashSet::new(),
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
            seed_initial_snapshot(&config, &snapshot, &event_tx).await;
            seeded = true;
        }
        update_connection(&connection, ConnectionState::Connecting).await;
        broadcast_connection(&event_tx, "connecting", None);
        let socket_result = connect_async(&config.ws_url).await;
        let mut ws = match socket_result {
            Ok((ws, _resp)) => ws,
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
    snapshot: &Arc<RwLock<SnapshotStore>>,
    event_tx: &broadcast::Sender<StreamEvent>,
) {
    let wallets = crate::wallet::list_solana_env_wallets();
    if wallets.is_empty() {
        return;
    }
    let seed = tokio::time::timeout(
        Duration::from_secs(INITIAL_SNAPSHOT_TIMEOUT_SECS),
        enrich_wallet_statuses_with_options(&config.rpc_url, &config.usd1_mint, &wallets, false),
    )
    .await
    .unwrap_or_default();
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
                    Some(StreamCommand::SetDemand(active)) => {
                        if !idle_suspension_enabled() {
                            state.balance_demand_active = true;
                            continue;
                        }
                        if state.balance_demand_active == active {
                            continue;
                        }
                        state.balance_demand_active = active;
                        if active {
                            if let Err(error) = install_wallet_subscriptions(ws, state).await {
                                return SessionExit::Disconnected(error);
                            }
                        } else {
                            if let Err(error) = unsubscribe_account_subscriptions(ws, state).await {
                                return SessionExit::Disconnected(error);
                            }
                            if state.trade_subs.is_empty() && state.pending_trade_acks.is_empty() && pending_trades.is_empty() {
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
    let params = json!([
        public_key,
        { "commitment": state.commitment, "encoding": "base64" }
    ]);
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
    let params = json!([
        ata.to_string(),
        { "commitment": state.commitment, "encoding": "jsonParsed" }
    ]);
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
    let ata = get_associated_token_address(&owner, &mint_pk);
    let request_id = next_request_id(state);
    let params = json!([
        ata.to_string(),
        { "commitment": state.commitment, "encoding": "jsonParsed" }
    ]);
    state.pending_account_acks.insert(
        request_id,
        PendingAccountAck {
            kind: AccountSubKind::TokenAta {
                env_key: wallet.envKey.clone(),
                mint: mint.to_string(),
            },
        },
    );
    send_jsonrpc(ws, request_id, "accountSubscribe", params).await
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
            .filter(
                |(_, kind)| matches!(kind, AccountSubKind::TokenAta { mint, .. } if removed.contains(mint)),
            )
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
    state.pending_account_acks.clear();
    Ok(())
}

async fn install_trade_subscription(
    ws: &mut WsStream,
    state: &mut SubscriptionState,
    trade: PendingTrade,
) -> Result<(), String> {
    let request_id = next_request_id(state);
    let params = json!([
        trade.signature,
        { "commitment": state.commitment }
    ]);
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
                | AccountSubKind::TokenAta { env_key, .. } => removed_keys.contains(env_key),
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
        handle_jsonrpc_ack(request_id, &payload, state)?;
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

fn handle_jsonrpc_ack(
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
                    at_ms: now_ms(),
                }));
            }
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
                    Some(StreamCommand::SetDemand(active)) => {
                        if !idle_suspension_enabled() {
                            state.balance_demand_active = true;
                            return false;
                        }
                        state.balance_demand_active = active;
                        if active {
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
            Some(StreamCommand::SetDemand(active)) => {
                if !idle_suspension_enabled() {
                    state.balance_demand_active = true;
                    return false;
                }
                state.balance_demand_active = active;
                if active {
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
