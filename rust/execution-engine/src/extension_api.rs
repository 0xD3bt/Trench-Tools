use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shared_fee_market::{
    AutoFeeResolutionInput, DEFAULT_AUTO_FEE_FALLBACK_LAMPORTS, SharedFeeMarketConfig,
    SharedFeeMarketRuntime, format_lamports_to_sol_decimal, parse_sol_decimal_to_lamports,
    read_shared_fee_market_snapshot, resolve_buffered_auto_fee_components,
    shared_fee_market_status_payload,
};
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use tokio::{sync::RwLock, task::JoinSet};
use uuid::Uuid;

use shared_extension_runtime::{
    balance_stream::{
        BalanceEventPayload, BalanceStreamHandle, BatchStatusEventPayload, DiagnosticEventPayload,
        MarkEventPayload, StreamConfig as BalanceStreamConfig, StreamEvent, TradeEventPayload,
        spawn as spawn_balance_stream,
    },
    catalog::{
        LaunchpadAvailability, LaunchpadAvailabilityInputs,
        launchpad_registry as build_launchpad_registry, strategy_registry,
    },
    crypto::install_rustls_crypto_provider,
    wallet::{
        WalletRuntimeConfig, WalletStatusSummary, WalletSummary as RuntimeWalletSummary,
        configure_wallet_runtime, enrich_wallet_statuses_with_balance_options,
        invalidate_wallet_balance_cache,
    },
};

use crate::auth::{AuthBootstrapInfo, AuthManager, AuthTokenSummary, CreatedAuthToken};
use crate::batch_store::{
    batch_history_path, history_entries, load_batch_history, persist_batch_history,
};
use crate::canonical_config::{
    CANONICAL_CONFIG_SCHEMA_VERSION, canonical_config_from_legacy,
    config_allow_non_canonical_pool_trades, config_default_buy_funding_policy,
    config_default_sell_settlement_policy, config_track_send_block_height,
    config_wrapper_default_fee_bps, default_canonical_config, normalize_canonical_config,
    remove_legacy_preset, route_bool_field, route_mev_mode, route_string_field,
    set_allow_non_canonical_pool_trades, set_track_send_block_height,
    set_wrapper_default_fee_bps_in_config, upsert_legacy_preset,
};
use crate::executor::{
    ExecutedTrade, ExecutionExecutor, ExecutionPolicy, SellIntent, WalletTradeRequest,
};
use crate::launchdeck_warm::{
    SharedLaunchdeckWarmRegistry, configured_active_warm_routes_from_config,
    execute_immediate_warm_pass, mark_operator_activity,
    new_registry as new_launchdeck_warm_registry, spawn_continuous_warm_task,
    update_default_routes, warm_runtime_payload,
};
use crate::providers::{
    ProviderAvailability as LaunchdeckProviderAvailability, ProviderMeta as LaunchdeckProviderMeta,
    provider_availability_registry, provider_registry,
};
use crate::rewards::{
    RewardWallet, RewardsClaimRequest, RewardsClaimResponse, RewardsExecutionConfig,
    RewardsSummaryRequest, RewardsSummaryResponse, claim_rewards, summarize_rewards,
};
use crate::rollout::family_execution_enabled;
use crate::rollout::family_guard_warning;
use crate::rpc_client::configured_rpc_url;
use crate::shared_config::{SharedRpcConfig, shared_config_manager};
use crate::token_distribution::{
    TokenConsolidateRequest, TokenDistributionExecutionConfig, TokenDistributionResponse,
    TokenSplitRequest, execute_consolidate as execute_token_consolidate,
    execute_split as execute_token_split,
};
use crate::trade_dispatch::resolve_trade_plan;
use crate::trade_ledger::{
    EventProvenance, ExplicitFeeBreakdown, ForceCloseMarkerEvent,
    IncompleteBalanceAdjustmentMarkerEvent, JournalEntry, PlatformTag, RecordConfirmedTradeParams,
    StoredEntryPreference, TradeLedgerPaths, aggregate_trade_ledger, append_confirmed_trade_event,
    append_force_close_marker, append_incomplete_balance_adjustment_marker, append_reset_marker,
    force_close_trade_ledger_position, load_trade_ledger, load_trade_ledger_known_event_ids,
    persist_trade_ledger, platform_tag_from_label, read_journal_entries, record_confirmed_trade,
    reset_trade_ledger_position, trade_ledger_paths,
};
use crate::trade_planner::{
    LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerRuntimeBundle, TradeVenueFamily,
};
use crate::trade_runtime::{
    RuntimeExecutionPolicy, RuntimeSellIntent, TradeRuntimeRequest, compile_wallet_trade,
};
use crate::transport::{
    configured_provider_region as execution_configured_provider_region,
    default_endpoint_profile_for_provider as execution_default_endpoint_profile_for_provider,
    resolved_helius_priority_fee_rpc_url,
};
use crate::warming_service::shared_warming_service;

const EXTENSION_CONTRACT_VERSION: &str = "v3-engine-authoritative";
const EXECUTION_RUNTIME_MODE: &str = "engine_authoritative";
const EXECUTION_AUTHORITY: &str = "engine";
const HOST_BIND_HOST: &str = "127.0.0.1";
const DEFAULT_HOST_PORT: u16 = 8788;
const IDEMPOTENCY_WINDOW_MS: u64 = 15_000;
const HELLOMOON_BATCH_WALLET_TIMEOUT_MS: u64 = 10_000;
const TOKEN_DISTRIBUTION_STALE_MS: u64 = 10_000;
const DEFAULT_DATA_ROOT: &str = ".local/execution-engine";
const DEFAULT_STATE_FILE: &str = "engine-state.json";
const CURRENT_ENGINE_STATE_VERSION: &str = "0.3.0";
const AUTH_SCHEME_BEARER: &str = "Bearer ";
const CANONICAL_CONFIG_VERSION: &str = "v1";
const MAX_TRANSACTION_DELAY_MS: u64 = 2_000;
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLCsAEpksdQSJNy2C";
const USD1_MINT: &str = "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB";
const TRADE_BALANCE_GATE_TIMEOUT_MS: u64 = 100;
const TRADE_BALANCE_GATE_FRONTEND_REFRESH_WALLET_LIMIT: usize = 50;
const TRADE_BALANCE_GATE_SIGNATURE_FEE_LAMPORTS: u64 = 5_000;
const TRADE_BALANCE_GATE_FIXED_BUY_BUFFER_LAMPORTS: u64 = 10_000;
const TRADE_BALANCE_GATE_FIRST_BUY_RENT_BUFFER_LAMPORTS: u64 = 2_100_000;

const RPC_RESYNC_PAGE_SIZE: usize = 1000;
const HELIUS_RESYNC_PAGE_SIZE: usize = 100;
const RPC_RESYNC_MAX_PAGES: usize = 10;
const RPC_RESYNC_MAX_SIGNATURES: usize = 10_000;
const RPC_RESYNC_OVERALL_TIMEOUT: Duration = Duration::from_secs(60);
const RESYNC_WALLET_CONCURRENCY: usize = 4;
const AUTO_RESYNC_COOLDOWN_MS: u64 = 5 * 60 * 1000;
const FORCE_CLOSE_COOLDOWN_MS: u64 = 60 * 60 * 1000;
const WRAPPED_SOL_MINT: &str = "So11111111111111111111111111111111111111112";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostic {
    pub key: String,
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
    pub first_seen_at_ms: u64,
    pub last_seen_at_ms: u64,
}

#[derive(Debug, Clone)]
#[cfg(test)]
struct RuntimeDiagnosticInput {
    severity: &'static str,
    source: &'static str,
    code: &'static str,
    message: String,
    detail: Option<String>,
    env_var: Option<&'static str>,
    endpoint_kind: Option<&'static str>,
    endpoint_url: Option<String>,
    restart_required: bool,
}

fn runtime_diagnostics_store() -> &'static Mutex<HashMap<String, RuntimeDiagnostic>> {
    static STORE: OnceLock<Mutex<HashMap<String, RuntimeDiagnostic>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn sanitized_endpoint_host(endpoint_url: &str) -> Option<String> {
    let trimmed = endpoint_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    reqwest::Url::parse(trimmed)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .or_else(|| Some("<invalid-url>".to_string()))
}

#[cfg(test)]
fn diagnostic_fingerprint(
    source: &str,
    endpoint_kind: Option<&str>,
    env_var: Option<&str>,
    host: Option<&str>,
    code: &str,
) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        source,
        endpoint_kind.unwrap_or("runtime"),
        env_var.unwrap_or(""),
        host.unwrap_or(""),
        code
    )
}

#[cfg(test)]
fn record_runtime_diagnostic(input: RuntimeDiagnosticInput) {
    let host = input
        .endpoint_url
        .as_deref()
        .and_then(sanitized_endpoint_host);
    let fingerprint = diagnostic_fingerprint(
        input.source,
        input.endpoint_kind,
        input.env_var,
        host.as_deref(),
        input.code,
    );
    let now = now_unix_ms();
    let mut store = runtime_diagnostics_store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match store.get_mut(&fingerprint) {
        Some(existing) => {
            existing.severity = input.severity.to_string();
            existing.message = input.message;
            existing.detail = input.detail;
            existing.active = true;
            existing.restart_required = input.restart_required;
            existing.last_seen_at_ms = now;
        }
        None => {
            store.insert(
                fingerprint.clone(),
                RuntimeDiagnostic {
                    key: fingerprint.clone(),
                    fingerprint,
                    severity: input.severity.to_string(),
                    source: input.source.to_string(),
                    code: input.code.to_string(),
                    message: input.message,
                    detail: input.detail,
                    env_var: input.env_var.map(str::to_string),
                    endpoint_kind: input.endpoint_kind.map(str::to_string),
                    host,
                    active: true,
                    restart_required: input.restart_required,
                    first_seen_at_ms: now,
                    last_seen_at_ms: now,
                },
            );
        }
    }
}

#[cfg(test)]
fn clear_runtime_diagnostic(
    source: &str,
    endpoint_kind: Option<&str>,
    env_var: Option<&str>,
    endpoint_url: Option<&str>,
    code: &str,
) {
    let host = endpoint_url.and_then(sanitized_endpoint_host);
    let fingerprint = diagnostic_fingerprint(source, endpoint_kind, env_var, host.as_deref(), code);
    let mut store = runtime_diagnostics_store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store.remove(&fingerprint);
}

fn runtime_diagnostics_snapshot() -> Vec<RuntimeDiagnostic> {
    let mut diagnostics = runtime_diagnostics_store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

pub fn execution_engine_port() -> u16 {
    std::env::var("EXECUTION_ENGINE_PORT")
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(DEFAULT_HOST_PORT)
}

pub fn host_bind_address() -> String {
    format!("{HOST_BIND_HOST}:{}", execution_engine_port())
}

fn shared_fee_market_runtime() -> SharedFeeMarketRuntime {
    SharedFeeMarketRuntime::new(SharedFeeMarketConfig::new(
        crate::paths::shared_fee_market_cache_path(),
        configured_rpc_url(),
        resolved_helius_priority_fee_rpc_url(),
        format!("execution-engine-{}", std::process::id()),
        Vec::new(),
    ))
}

fn spawn_shared_fee_market_refresh_task() {
    tokio::spawn(async move {
        loop {
            let runtime = shared_fee_market_runtime();
            runtime.refresh_helius_if_leased().await;
            tokio::time::sleep(runtime.config().helius_refresh_interval).await;
        }
    });
    tokio::spawn(async move {
        loop {
            let runtime = shared_fee_market_runtime();
            runtime.refresh_jito_if_leased().await;
            tokio::time::sleep(runtime.config().jito_reconnect_delay).await;
        }
    });
}

/// Process-wide counters for persistence failures. Wrapped in `Arc` so a
/// cheap clone lives on `AppState` and getter handlers can snapshot
/// without coordinating with writers.
#[derive(Debug, Default)]
pub struct PersistFailureCounters {
    /// Number of times `persist_batch_history(..)` returned an `Err(..)`
    /// that the caller swallowed to keep the trade response path
    /// user-successful.
    pub batch_history: std::sync::atomic::AtomicU64,
    /// Number of times `persist_trade_ledger(..)` returned an error that
    /// we surfaced to logs but kept the caller's flow otherwise intact.
    pub trade_ledger: std::sync::atomic::AtomicU64,
    /// Most recent error messages — keep the last few so `/runtime-status`
    /// can show operators what the underlying failure was.
    pub last_errors: std::sync::Mutex<Vec<String>>,
}

impl PersistFailureCounters {
    pub fn record_batch_history_failure(&self, error: &str) {
        self.batch_history
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.push_last_error(format!("batch_history: {error}"));
    }

    pub fn record_trade_ledger_failure(&self, error: &str) {
        self.trade_ledger
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.push_last_error(format!("trade_ledger: {error}"));
    }

    fn push_last_error(&self, message: String) {
        // Keep a small bounded ring (last 5) so /runtime-status stays
        // light-weight. Mutex here is fine: the call site is already
        // after an RPC send, so lock contention is negligible.
        if let Ok(mut guard) = self.last_errors.lock() {
            if guard.len() >= 5 {
                guard.remove(0);
            }
            guard.push(message);
        }
    }

    pub fn snapshot(&self) -> Value {
        use std::sync::atomic::Ordering;
        let last_errors = self
            .last_errors
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        json!({
            "batchHistory": self.batch_history.load(Ordering::Relaxed),
            "tradeLedger": self.trade_ledger.load(Ordering::Relaxed),
            "lastErrors": last_errors,
        })
    }
}

#[derive(Default)]
struct AutoResyncTracker {
    in_flight: HashSet<String>,
    cooldown_until_ms: HashMap<String, u64>,
    force_close_cooldown_until_ms: HashMap<String, u64>,
}

#[derive(Clone)]
pub struct AppState {
    engine: Arc<RwLock<StoredEngineState>>,
    batches: Arc<RwLock<HashMap<String, BatchStatusResponse>>>,
    accepted_requests: Arc<RwLock<HashMap<String, AcceptedRequestRecord>>>,
    token_distribution_requests: Arc<RwLock<HashMap<String, TokenDistributionRequestRecord>>>,
    rewards_requests: Arc<RwLock<HashMap<String, RewardsRequestRecord>>>,
    launchdeck_warm: SharedLaunchdeckWarmRegistry,
    state_path: PathBuf,
    batch_history_path: PathBuf,
    trade_ledger: Arc<RwLock<HashMap<String, crate::trade_ledger::TradeLedgerEntry>>>,
    trade_ledger_paths: TradeLedgerPaths,
    trade_ledger_event_ids: Arc<RwLock<HashSet<String>>>,
    auto_resync_tracker: Arc<RwLock<AutoResyncTracker>>,
    executor: ExecutionExecutor,
    auth: Arc<AuthManager>,
    balance_stream: BalanceStreamHandle,
    batch_status_revisions: Arc<Mutex<HashMap<String, u64>>>,
    live_mark_targets: Arc<RwLock<HashMap<String, LiveMarkTarget>>>,
    persist_failures: Arc<PersistFailureCounters>,
}

impl AppState {
    /// Handle to the process-wide balance + trade subscription manager.
    pub fn balance_stream(&self) -> BalanceStreamHandle {
        self.balance_stream.clone()
    }

    /// Auth manager for handlers that need direct access to token state
    /// outside the normal request middleware.
    pub fn auth_manager(&self) -> Arc<AuthManager> {
        self.auth.clone()
    }

    async fn clear_active_mark_target(&self, surface_id: &str) {
        let accounts = {
            let mut targets = self.live_mark_targets.write().await;
            targets.remove(surface_id);
            live_mark_accounts_from_targets(targets.values())
        };
        self.balance_stream().set_mark_accounts(accounts);
    }

    pub async fn set_active_mark_target(
        &self,
        request: ExtensionActiveMarkRequest,
    ) -> Result<(), String> {
        let surface_id = active_mark_surface_id(&request);
        if !request.active {
            self.clear_active_mark_target(&surface_id).await;
            return Ok(());
        }

        let Some(mint) = request
            .mint
            .as_deref()
            .and_then(trimmed_option)
            .map(str::to_string)
        else {
            self.clear_active_mark_target(&surface_id).await;
            return Err("mint is required for active mark target.".to_string());
        };
        let wallet_request = active_mark_wallet_status_request(&request, &mint);
        let engine = self.engine.read().await.clone();
        let target = match resolve_wallet_status_target(
            &build_effective_wallets(&engine),
            &build_effective_wallet_groups(&engine),
            &wallet_request,
        ) {
            Ok(target) => target,
            Err((_, error)) => {
                self.clear_active_mark_target(&surface_id).await;
                return Err(error);
            }
        };
        if target.wallet_keys.is_empty() {
            self.clear_active_mark_target(&surface_id).await;
            return Ok(());
        }

        let selector =
            match resolve_wallet_status_quote_selector(&engine, &wallet_request, &mint).await {
                Ok(selector) => selector,
                Err(error) => {
                    self.clear_active_mark_target(&surface_id).await;
                    return Err(error);
                }
            };
        let watch_accounts = live_mark_watch_accounts(&selector);
        if watch_accounts.is_empty() {
            self.clear_active_mark_target(&surface_id).await;
            return Ok(());
        }

        let selected_wallet_keys = target.wallet_keys.iter().cloned().collect::<HashSet<_>>();
        let token_balance_ui_by_wallet = request
            .wallet_token_balances
            .iter()
            .filter_map(|entry| {
                let env_key = entry.env_key.trim();
                let amount = entry.token_balance?;
                if env_key.is_empty()
                    || !selected_wallet_keys.contains(env_key)
                    || !amount.is_finite()
                    || amount < 0.0
                {
                    return None;
                }
                Some((env_key.to_string(), amount))
            })
            .collect::<HashMap<_, _>>();
        let token_balance_raw = request
            .token_balance_raw
            .or_else(|| {
                aggregate_live_mark_raw(&token_balance_ui_by_wallet, request.token_decimals)
            })
            .or_else(|| {
                request
                    .token_balance
                    .and_then(|amount| live_mark_ui_amount_to_raw(amount, request.token_decimals))
            });

        let live_target = LiveMarkTarget {
            surface_id: surface_id.clone(),
            mark_revision: 1,
            mint,
            wallet_keys: target.wallet_keys,
            wallet_group_id: target.wallet_group_id,
            selector,
            watch_accounts: watch_accounts.clone(),
            token_decimals: request.token_decimals,
            token_balance_raw,
            token_balance_ui_by_wallet,
        };
        let accounts = {
            let mut targets = self.live_mark_targets.write().await;
            targets.insert(surface_id, live_target.clone());
            live_mark_accounts_from_targets(targets.values())
        };
        self.balance_stream().set_mark_accounts(accounts);
        publish_live_mark(self, live_target, None).await;
        Ok(())
    }

    /// Register a trade-activity tick against the continuous transport warm
    /// loop. Called from `/prewarm`, `/buy`, `/sell`, and `/batch/preview`
    /// handlers so an active trading session keeps the warm loop awake
    /// instead of idle-suspending mid-trade. The `mark_operator_activity`
    /// call is cheap — it only reshuffles internal warm-target state.
    pub fn tick_trade_activity(&self) {
        let engine_snapshot = match self.engine.try_read() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        let routes =
            configured_active_warm_routes_from_config(&current_canonical_config(&engine_snapshot));
        if mark_operator_activity(&self.launchdeck_warm, routes) {
            let warm = self.launchdeck_warm.clone();
            tokio::spawn(async move {
                execute_immediate_warm_pass(warm).await;
            });
        }
    }
}

fn refresh_shared_alt_at_startup(rpc_url: &str) -> Result<(), String> {
    let rpc_url = rpc_url.to_string();
    let result = std::thread::Builder::new()
        .name("shared-alt-startup-refresh".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("failed to create shared ALT startup runtime: {error}"))?;
            runtime
                .block_on(crate::pump_native::initialize_shared_super_lookup_tables(
                    &rpc_url,
                ))
                .map(|_| ())
        })
        .map_err(|error| format!("failed to spawn shared ALT startup refresh: {error}"))
        .and_then(|handle| {
            handle
                .join()
                .map_err(|_| "shared ALT startup refresh panicked".to_string())?
        });
    match result {
        Ok(()) => {
            eprintln!("[execution-engine][startup] shared ALT cache is ready");
            Ok(())
        }
        Err(error) => {
            eprintln!("[execution-engine][startup] shared ALT refresh failed: {error}");
            Err(error)
        }
    }
}

fn refresh_shared_fee_market_at_startup() -> Result<(), String> {
    let result = std::thread::Builder::new()
        .name("shared-fee-market-startup-refresh".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| {
                    format!("failed to create shared fee-market startup runtime: {error}")
                })?;
            runtime.block_on(shared_fee_market_runtime().initialize_fee_market_snapshot())
        })
        .map_err(|error| format!("failed to spawn shared fee-market startup refresh: {error}"))
        .and_then(|handle| {
            handle
                .join()
                .map_err(|_| "shared fee-market startup refresh panicked".to_string())?
        });
    match result {
        Ok(status) => {
            eprintln!(
                "[execution-engine][startup] shared fee market is ready helius_age_ms={:?} jito_age_ms={:?}",
                status.helius_age_ms, status.jito_age_ms
            );
            Ok(())
        }
        Err(error) => {
            eprintln!("[execution-engine][startup] shared fee-market refresh failed: {error}");
            Err(error)
        }
    }
}

impl AppState {
    /// Fallible constructor. Returns a `Result` so `main` (or tests) can
    /// surface a clean error message instead of aborting on panic when
    /// the data root is unwritable or the on-disk auth file is
    /// corrupted. `new()` is kept as a thin wrapper that prints the
    /// error and still panics, for the rare callers (mostly tests) that
    /// don't want to plumb a `Result` through.
    pub fn try_new() -> Result<Self, String> {
        configure_shared_extension_runtime();
        let state_path = state_path();
        let engine = load_engine_state(&state_path).unwrap_or_else(fresh_engine_state);
        let engine = reconcile_wallet_metadata(engine);
        let default_warm_routes =
            configured_active_warm_routes_from_config(&current_canonical_config(&engine));
        let blockhash_commitment = engine.settings.execution_commitment.clone();
        let batch_history_path = batch_history_path(&engine.data_root);
        let trade_ledger_paths = trade_ledger_paths(&engine.data_root);
        let batches = recover_loaded_batches(load_batch_history(&batch_history_path));
        let trade_ledger = load_trade_ledger(&trade_ledger_paths);
        let trade_ledger_event_ids = load_trade_ledger_known_event_ids(&trade_ledger_paths);
        let rpc_url = configured_rpc_url();
        refresh_shared_alt_at_startup(&rpc_url)?;
        refresh_shared_fee_market_at_startup()?;
        let launchdeck_warm = new_launchdeck_warm_registry(default_warm_routes);
        let auth = Arc::new(AuthManager::new().map_err(|error| {
            format!(
                "failed to initialize auth manager: {error}. \
                 Fix: ensure the directory exists and is writable, or remove any \
                 corrupted tokens file inside it and restart.",
            )
        })?);
        let bootstrap = auth.bootstrap_info();
        println!(
            "execution-engine extension auth is enabled. Your extension auth token file is at {}. \
             Copy the token from this file into your LaunchDeck extension to authenticate it with the execution engine. \
             Keep this file and token private.",
            bootstrap.token_file_path
        );
        tokio::spawn(async move {
            let _ = shared_warming_service()
                .warm_execution_primitives(&rpc_url, &blockhash_commitment)
                .await;
        });
        spawn_shared_fee_market_refresh_task();
        spawn_continuous_warm_task(launchdeck_warm.clone());

        let initial_runtime_wallets = runtime_wallets_from_engine(&engine);
        let balance_stream = spawn_balance_stream(
            BalanceStreamConfig::new(resolve_solana_ws_url(), configured_rpc_url(), USD1_MINT)
                .with_initial_wallets(initial_runtime_wallets),
        );

        // Hook the wallet-token balance cache up to the stream so the
        // sell-sizing path can reuse recently observed ATA balances
        // instead of always round-tripping `getTokenAccountsByOwner`.
        crate::wallet_token_cache::ensure_subscriber(&balance_stream);

        // Seed the runtime-policy cell so the warm planner picks up the
        // persisted "Allow non-canonical pool trades" setting on cold
        // start, before any settings-save request lands.
        crate::rollout::set_allow_non_canonical_pool_trades(
            engine.settings.allow_non_canonical_pool_trades,
        );

        let state = Self {
            engine: Arc::new(RwLock::new(engine)),
            batches: Arc::new(RwLock::new(batches)),
            accepted_requests: Arc::new(RwLock::new(HashMap::new())),
            token_distribution_requests: Arc::new(RwLock::new(HashMap::new())),
            rewards_requests: Arc::new(RwLock::new(HashMap::new())),
            launchdeck_warm,
            state_path,
            batch_history_path,
            trade_ledger: Arc::new(RwLock::new(trade_ledger)),
            trade_ledger_paths,
            trade_ledger_event_ids: Arc::new(RwLock::new(trade_ledger_event_ids)),
            auto_resync_tracker: Arc::new(RwLock::new(AutoResyncTracker::default())),
            executor: ExecutionExecutor::default(),
            auth,
            balance_stream,
            batch_status_revisions: Arc::new(Mutex::new(HashMap::new())),
            live_mark_targets: Arc::new(RwLock::new(HashMap::new())),
            persist_failures: Arc::new(PersistFailureCounters::default()),
        };
        spawn_batch_trade_reconciliation_task(state.clone());
        spawn_live_mark_task(state.clone());
        Ok(state)
    }

    /// Legacy constructor kept for tests. Logs a cleaner message than
    /// the previous bare `panic!` before aborting. Production code
    /// (`main.rs` / `try_router`) should use `try_new` instead.
    pub fn new() -> Self {
        match Self::try_new() {
            Ok(state) => state,
            Err(error) => {
                eprintln!("[execution-engine][startup] {error}");
                panic!("execution-engine failed to initialize: {error}");
            }
        }
    }
}

fn resolve_solana_ws_url() -> String {
    if let Ok(value) = std::env::var("SOLANA_WS_URL")
        && !value.trim().is_empty()
    {
        return value.trim().to_string();
    }
    let rpc = configured_rpc_url();
    if let Some(rest) = rpc.strip_prefix("https://") {
        return format!("wss://{rest}");
    }
    if let Some(rest) = rpc.strip_prefix("http://") {
        return format!("ws://{rest}");
    }
    rpc
}

fn configure_shared_extension_runtime() {
    configure_wallet_runtime(
        WalletRuntimeConfig::new().with_ata_cache_path(execution_wallet_ata_cache_path()),
    );
}

fn execution_wallet_ata_cache_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(DEFAULT_DATA_ROOT)
        .join("wallet-ata-cache.json")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
        })
}

fn launchdeck_local_root_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("LAUNCHDECK_LOCAL_DATA_DIR") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    workspace_root().join(".local").join("launchdeck")
}

fn launchdeck_bags_credentials_path() -> PathBuf {
    launchdeck_local_root_dir().join("bags-credentials.json")
}

fn launchdeck_bags_session_path() -> PathBuf {
    launchdeck_local_root_dir().join("bags-session.json")
}

fn bags_launchpad_configured() -> bool {
    std::env::var("BAGS_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || fs::read_to_string(launchdeck_bags_credentials_path())
            .ok()
            .map(|value| value.contains("\"apiKey\""))
            .unwrap_or(false)
        || fs::read_to_string(launchdeck_bags_session_path())
            .ok()
            .map(|value| value.contains("\"apiKey\""))
            .unwrap_or(false)
}

fn launchpad_registry() -> BTreeMap<String, LaunchpadAvailability> {
    build_launchpad_registry(LaunchpadAvailabilityInputs {
        bags_configured: bags_launchpad_configured(),
    })
}

/// Build the router. Propagates the underlying `AppState::try_new`
/// error instead of panicking, so the binary can exit with a clean
/// message and a non-zero status code on corrupt data roots / auth
/// files. Prefer this over `router()` in new callers.
pub fn try_router() -> Result<Router, String> {
    install_rustls_crypto_provider();
    let state = AppState::try_new()?;
    Ok(build_router_with_state(state))
}

pub fn router() -> Router {
    install_rustls_crypto_provider();
    let state = AppState::new();
    build_router_with_state(state)
}

fn build_router_with_state(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/extension/health", get(get_health))
        .route("/api/extension/runtime-status", get(get_runtime_status))
        .route("/api/extension/bootstrap", get(get_bootstrap))
        .route("/api/extension/wallet-status", post(get_wallet_status))
        .route("/api/extension/pnl/resync", post(resync_pnl_history))
        .route("/api/extension/pnl/reset", post(reset_pnl_history))
        .route("/api/extension/pnl/export", post(export_pnl_history))
        .route("/api/extension/pnl/wipe", post(wipe_pnl_history))
        .route(
            "/api/extension/config",
            // Execution-owned compatibility surface used by the extension
            // options page for the engine's canonical config payload.
            get(get_canonical_config).put(update_canonical_config),
        )
        .route(
            "/api/extension/settings",
            get(get_settings).put(update_settings),
        )
        .route(
            "/api/extension/presets",
            get(list_presets).post(create_preset),
        )
        .route(
            "/api/extension/presets/{preset_id}",
            get(get_preset).put(update_preset).delete(delete_preset),
        )
        .route(
            "/api/extension/wallets",
            get(list_wallets).post(create_wallet),
        )
        .route("/api/extension/wallets/reorder", post(reorder_wallets))
        .route(
            "/api/extension/wallets/{wallet_key}",
            get(get_wallet).put(update_wallet).delete(delete_wallet),
        )
        .route(
            "/api/extension/wallet-groups",
            get(list_wallet_groups).post(create_wallet_group),
        )
        .route(
            "/api/extension/wallet-groups/{group_id}",
            get(get_wallet_group)
                .put(update_wallet_group)
                .delete(delete_wallet_group),
        )
        .route(
            "/api/extension/events/active-mint",
            post(crate::events_stream::set_active_mints),
        )
        .route(
            "/api/extension/events/presence",
            post(crate::events_stream::set_balance_presence),
        )
        .route(
            "/api/extension/events/active-mark",
            post(crate::events_stream::set_active_mark),
        )
        .route(
            "/api/extension/events/stream",
            get(crate::events_stream::events_stream),
        )
        .route("/api/extension/resolve-token", post(resolve_token))
        .route("/api/extension/prewarm", post(prewarm_mint))
        .route("/api/extension/trade-readiness", post(set_trade_readiness))
        .route("/api/extension/batches", get(list_batches))
        .route("/api/extension/batch/preview", post(preview_batch))
        .route("/api/extension/buy", post(buy))
        .route("/api/extension/sell", post(sell))
        .route(
            "/api/extension/token-distribution/split",
            post(split_tokens),
        )
        .route(
            "/api/extension/token-distribution/consolidate",
            post(consolidate_tokens),
        )
        .route("/api/extension/rewards/summary", post(rewards_summary))
        .route("/api/extension/rewards/claim", post(rewards_claim))
        .route("/api/extension/batch/{batch_id}", get(get_batch_status))
        .route(
            "/api/extension/auth/tokens",
            get(list_auth_tokens).post(create_auth_token),
        )
        .route(
            "/api/launchdeck/trade-ledger/record",
            // Execution-owned bridge endpoint. LaunchDeck posts confirmed trade
            // summaries here so the local execution ledger stays authoritative.
            post(record_launchdeck_confirmed_trades),
        )
        .route(
            "/api/extension/auth/tokens/{token_id}",
            axum::routing::delete(revoke_auth_token),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_authenticated_request,
        ));

    Router::new()
        .route("/api/extension/auth/bootstrap", get(get_auth_bootstrap))
        .merge(protected)
        .with_state(state)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapResponse {
    pub version: String,
    pub data_root: String,
    #[serde(default)]
    pub config_version: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub providers: BTreeMap<String, LaunchdeckProviderAvailability>,
    #[serde(default)]
    pub provider_registry: Vec<LaunchdeckProviderMeta>,
    #[serde(default)]
    pub launchpads: BTreeMap<String, LaunchpadAvailability>,
    #[serde(default)]
    pub strategies: Value,
    pub capabilities: ExtensionCapabilities,
    pub settings: EngineSettings,
    pub presets: Vec<PresetSummary>,
    pub wallets: Vec<WalletSummary>,
    pub wallet_groups: Vec<WalletGroupSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCapabilities {
    pub platforms: Vec<Platform>,
    pub supports_batch_preview: bool,
    pub supports_batch_status: bool,
    pub supports_resource_editing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSettings {
    pub default_buy_slippage_percent: String,
    pub default_sell_slippage_percent: String,
    #[serde(default = "default_mev_mode_off")]
    pub default_buy_mev_mode: MevMode,
    #[serde(default = "default_mev_mode_off")]
    pub default_sell_mev_mode: MevMode,
    #[serde(default = "default_execution_provider")]
    pub execution_provider: String,
    #[serde(default = "default_execution_endpoint_profile")]
    pub execution_endpoint_profile: String,
    #[serde(default = "default_execution_commitment")]
    pub execution_commitment: String,
    #[serde(default = "default_execution_skip_preflight")]
    pub execution_skip_preflight: bool,
    #[serde(default)]
    pub track_send_block_height: bool,
    pub max_active_batches: usize,
    #[serde(default)]
    pub rpc_url: String,
    #[serde(default)]
    pub ws_url: String,
    #[serde(default)]
    pub warm_rpc_url: String,
    #[serde(default)]
    pub warm_ws_url: String,
    #[serde(default)]
    pub shared_region: String,
    #[serde(default)]
    pub helius_rpc_url: String,
    #[serde(default)]
    pub helius_ws_url: String,
    #[serde(default)]
    pub standard_rpc_send_urls: Vec<String>,
    #[serde(default)]
    pub helius_sender_region: String,
    #[serde(default)]
    pub default_distribution_mode: BuyDistributionMode,
    /// When true, the warm planner will still compile trades against a
    /// non-canonical Pump AMM pool the user pinned (e.g. a low-liquidity
    /// pool selected by pasting a pair address into Axiom). When false
    /// (the default, safe mode), any non-canonical pool selection is
    /// refused with a clear error differentiating it from the canonical
    /// pool for the same mint.
    #[serde(default)]
    pub allow_non_canonical_pool_trades: bool,
    #[serde(default = "default_buy_funding_policy_sol_only")]
    pub default_buy_funding_policy: BuyFundingPolicy,
    #[serde(default = "default_sell_settlement_policy_always_to_sol")]
    pub default_sell_settlement_policy: SellSettlementPolicy,
    #[serde(default = "default_pnl_tracking_mode_local")]
    pub pnl_tracking_mode: PnlTrackingMode,
    #[serde(default = "default_true")]
    pub pnl_include_fees: bool,
    /// Default wrapper voluntary fee tier, in basis points. Valid
    /// values: `0`, `10` (0.1%), `20` (0.2%). The on-chain program
    /// hardcodes 20 bps as the absolute cap; anything above that is
    /// clamped back down before it reaches the program. This setting
    /// is persisted as the user's per-account default — individual
    /// trades reuse it without asking.
    #[serde(default = "default_wrapper_fee_bps")]
    pub wrapper_default_fee_bps: u16,
}

fn default_wrapper_fee_bps() -> u16 {
    crate::rollout::DEFAULT_WRAPPER_FEE_BPS
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionHealthResponse {
    pub contract_version: String,
    pub version: String,
    pub engine_version: String,
    pub runtime_mode: String,
    pub executor_route: String,
    pub execution_authority: String,
    pub status: String,
    pub bind_address: String,
    pub loopback_only: bool,
    pub bootstrap_revision: String,
    pub data_root: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRuntimeStatusResponse {
    pub contract_version: String,
    pub version: String,
    pub engine_version: String,
    pub runtime_mode: String,
    pub executor_route: String,
    pub execution_authority: String,
    pub status: String,
    pub bind_address: String,
    pub loopback_only: bool,
    pub bootstrap_revision: String,
    pub data_root: String,
    pub supported_origin_surfaces: Vec<Surface>,
    pub supported_canonical_surfaces: Vec<Surface>,
    pub capabilities: ExtensionCapabilities,
    pub active_batches: usize,
    pub max_active_batches: usize,
    pub idempotency_window_ms: u64,
    #[serde(default)]
    pub warm: Value,
    #[serde(default)]
    pub auto_fee: Value,
    #[serde(default)]
    pub providers: BTreeMap<String, LaunchdeckProviderAvailability>,
    #[serde(default)]
    pub diagnostics: Vec<RuntimeDiagnostic>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionWalletStatusRequest {
    #[serde(default)]
    wallet_key: Option<String>,
    #[serde(default)]
    wallet_keys: Option<Vec<String>>,
    #[serde(default)]
    wallet_group_id: Option<String>,
    #[serde(default)]
    mint: Option<String>,
    #[serde(default)]
    preset_id: Option<String>,
    #[serde(default)]
    buy_funding_policy: Option<BuyFundingPolicy>,
    #[serde(default)]
    sell_settlement_policy: Option<SellSettlementPolicy>,
    #[serde(default)]
    quoted_price: Option<f64>,
    #[serde(default)]
    route_address: Option<String>,
    #[serde(default)]
    pair: Option<String>,
    #[serde(default)]
    warm_key: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    lifecycle: Option<String>,
    #[serde(default)]
    quote_asset: Option<String>,
    #[serde(default)]
    canonical_market_key: Option<String>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    page_url: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    include_disabled: bool,
    #[serde(default)]
    read_only: bool,
    /// When true, bypass the per-wallet balance cache and re-fetch from RPC.
    #[serde(default)]
    force: bool,
    /// When true, skip the SOL balance RPC entirely — useful for callers that only need mint data.
    #[serde(default)]
    skip_sol_balance: bool,
    /// Explicit balance scope for newer callers. Defaults preserve legacy behavior.
    #[serde(default)]
    include_sol_balance: Option<bool>,
    #[serde(default)]
    include_usd1_balance: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionActiveMarkRequest {
    #[serde(default)]
    active: bool,
    #[serde(default)]
    surface_id: Option<String>,
    #[serde(default)]
    wallet_key: Option<String>,
    #[serde(default)]
    wallet_keys: Option<Vec<String>>,
    #[serde(default)]
    wallet_group_id: Option<String>,
    #[serde(default)]
    mint: Option<String>,
    #[serde(default)]
    preset_id: Option<String>,
    #[serde(default)]
    buy_funding_policy: Option<BuyFundingPolicy>,
    #[serde(default)]
    sell_settlement_policy: Option<SellSettlementPolicy>,
    #[serde(default)]
    route_address: Option<String>,
    #[serde(default)]
    pair: Option<String>,
    #[serde(default)]
    warm_key: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    lifecycle: Option<String>,
    #[serde(default)]
    quote_asset: Option<String>,
    #[serde(default)]
    canonical_market_key: Option<String>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    page_url: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    token_balance: Option<f64>,
    #[serde(default)]
    token_balance_raw: Option<u64>,
    #[serde(default)]
    token_decimals: Option<u8>,
    #[serde(default)]
    wallet_token_balances: Vec<ActiveMarkWalletTokenBalance>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveMarkWalletTokenBalance {
    #[serde(default)]
    env_key: String,
    #[serde(default)]
    token_balance: Option<f64>,
}

#[derive(Debug, Clone)]
struct LiveMarkTarget {
    surface_id: String,
    mark_revision: u64,
    mint: String,
    wallet_keys: Vec<String>,
    wallet_group_id: Option<String>,
    selector: LifecycleAndCanonicalMarket,
    watch_accounts: Vec<String>,
    token_decimals: Option<u8>,
    token_balance_raw: Option<u64>,
    token_balance_ui_by_wallet: HashMap<String, f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnlHistoryScopeRequest {
    #[serde(default)]
    wallet_key: Option<String>,
    #[serde(default)]
    wallet_keys: Option<Vec<String>>,
    #[serde(default)]
    wallet_group_id: Option<String>,
    mint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchdeckSettingsSaveRequest {
    config: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchdeckConfirmedTradeRecordRequest {
    wallet_key: String,
    mint: String,
    signature: String,
    #[serde(default)]
    client_request_id: Option<String>,
    #[serde(default)]
    batch_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchdeckConfirmedTradesRequest {
    #[serde(default)]
    trades: Vec<LaunchdeckConfirmedTradeRecordRequest>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchdeckConfirmedTradesResponse {
    ok: bool,
    recorded_count: usize,
    duplicate_count: usize,
    ignored_count: usize,
    /// Human-readable failure messages for rows that will not succeed on retry
    /// (missing identity fields, unknown wallet, signature belonged to a
    /// different wallet, etc.). Callers should log these and drop the rows.
    errors: Vec<String>,
    /// Rows whose failure was caused by a transient condition (RPC timeout,
    /// signature not yet confirmed, disk I/O error). The caller (e.g. the
    /// launchdeck-engine outbox flush) should keep these rows queued and retry
    /// them later.
    #[serde(default)]
    transient_failures: Vec<LaunchdeckConfirmedTradeFailure>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchdeckConfirmedTradeFailure {
    signature: String,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetSummary {
    pub id: String,
    pub label: String,
    pub buy_amount_sol: String,
    pub sell_percent: String,
    #[serde(default)]
    pub buy_amounts_sol: Vec<String>,
    #[serde(default)]
    pub sell_amounts_percent: Vec<String>,
    #[serde(default = "default_buy_amount_rows")]
    pub buy_amount_rows: u8,
    #[serde(default = "default_sell_percent_rows")]
    pub sell_percent_rows: u8,
    #[serde(default)]
    pub buy_fee_sol: String,
    #[serde(default)]
    pub buy_tip_sol: String,
    #[serde(default)]
    pub buy_slippage_percent: String,
    #[serde(default = "default_mev_mode_off")]
    pub buy_mev_mode: MevMode,
    #[serde(default)]
    pub buy_auto_tip_enabled: bool,
    #[serde(default)]
    pub buy_max_fee_sol: String,
    #[serde(default)]
    pub buy_provider: String,
    #[serde(default)]
    pub buy_endpoint_profile: String,
    #[serde(default)]
    pub sell_fee_sol: String,
    #[serde(default)]
    pub sell_tip_sol: String,
    #[serde(default)]
    pub sell_slippage_percent: String,
    #[serde(default = "default_mev_mode_off")]
    pub sell_mev_mode: MevMode,
    #[serde(default)]
    pub sell_auto_tip_enabled: bool,
    #[serde(default)]
    pub sell_max_fee_sol: String,
    #[serde(default)]
    pub sell_provider: String,
    #[serde(default)]
    pub sell_endpoint_profile: String,
    #[serde(default)]
    pub slippage_percent: String,
    #[serde(default = "default_mev_mode_off")]
    pub mev_mode: MevMode,
    #[serde(default = "default_buy_funding_policy_sol_only")]
    pub buy_funding_policy: BuyFundingPolicy,
    #[serde(default = "default_sell_settlement_policy_always_to_sol")]
    pub sell_settlement_policy: SellSettlementPolicy,
    #[serde(default)]
    pub buy_funding_policy_explicit: bool,
    #[serde(default)]
    pub sell_settlement_policy_explicit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletSummary {
    pub key: String,
    pub label: String,
    pub public_key: String,
    pub enabled: bool,
    #[serde(default)]
    pub emoji: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWalletRequest {
    pub label: String,
    pub private_key: String,
    #[serde(default = "default_wallet_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub emoji: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWalletRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub private_key: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub emoji: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderWalletsRequest {
    pub wallet_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAuthTokenRequest {
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletGroupSummary {
    pub id: String,
    pub label: String,
    pub wallet_keys: Vec<String>,
    #[serde(default)]
    pub batch_policy: WalletGroupBatchPolicy,
    #[serde(default)]
    pub emoji: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BuyDistributionMode {
    Split,
    #[default]
    Each,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransactionDelayMode {
    #[default]
    Off,
    On,
    FirstBuyOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransactionDelayStrategy {
    #[default]
    Fixed,
    Random,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WalletGroupBatchPolicy {
    #[serde(default)]
    pub distribution_mode: BuyDistributionMode,
    #[serde(default)]
    pub buy_variance_percent: u8,
    #[serde(default)]
    pub transaction_delay_mode: TransactionDelayMode,
    #[serde(default)]
    pub transaction_delay_strategy: TransactionDelayStrategy,
    #[serde(default)]
    pub transaction_delay_ms: u64,
    #[serde(default)]
    pub transaction_delay_min_ms: u64,
    #[serde(default)]
    pub transaction_delay_max_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchExecutionPolicySummary {
    pub distribution_mode: BuyDistributionMode,
    pub buy_variance_percent: u8,
    pub transaction_delay_mode: TransactionDelayMode,
    pub transaction_delay_strategy: TransactionDelayStrategy,
    pub transaction_delay_ms: u64,
    pub transaction_delay_min_ms: u64,
    pub transaction_delay_max_ms: u64,
    #[serde(default)]
    pub base_wallet_amount_sol: Option<String>,
    #[serde(default)]
    pub total_batch_spend_sol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletExecutionPlanSummary {
    pub wallet_key: String,
    #[serde(default)]
    pub buy_amount_sol: Option<String>,
    #[serde(default)]
    pub scheduled_delay_ms: u64,
    #[serde(default)]
    pub delay_applied: bool,
    #[serde(default)]
    pub first_buy: Option<bool>,
    #[serde(default)]
    pub applied_variance_percent: Option<f64>,
    /// Wrapper voluntary fee tier that will be applied to this wallet's
    /// SOL leg, in basis points. Always `<=` the on-chain hardcoded cap
    /// of 20 bps. `0` means the wrapper still runs (for consistency) but
    /// no lamports are transferred to the fee vault.
    #[serde(default)]
    pub wrapper_fee_bps: u16,
    /// Estimated wrapper fee on this wallet's SOL leg, denominated in
    /// SOL and floor-rounded to match the on-chain arithmetic. Absent
    /// when the route does not touch SOL or the SOL-leg size is not
    /// known until after the swap (SOL-out sells). The UI should render
    /// an "after swap" hint in those cases.
    #[serde(default)]
    pub wrapper_fee_sol: Option<String>,
    /// Route classification the wrapper will use, one of
    /// `sol_in`, `sol_out`, or `no_sol`. Populated so the UI can
    /// label SOL-out fees as "post-swap" and SOL-in fees as "pre-swap".
    #[serde(default)]
    pub wrapper_route: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Axiom,
    #[serde(rename = "j7")]
    J7,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Pulse,
    TokenDetail,
    ContractAddress,
    Watchlist,
    WalletTracker,
}

impl Surface {
    pub fn canonical(&self) -> Self {
        match self {
            Self::Watchlist | Self::WalletTracker => Self::TokenDetail,
            other => other.clone(),
        }
    }

    pub fn supported_origin_surfaces() -> Vec<Self> {
        vec![
            Self::Pulse,
            Self::TokenDetail,
            Self::ContractAddress,
            Self::Watchlist,
            Self::WalletTracker,
        ]
    }

    pub fn supported_canonical_surfaces() -> Vec<Self> {
        vec![Self::Pulse, Self::TokenDetail, Self::ContractAddress]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuyFundingPolicy {
    SolOnly,
    PreferUsd1ElseTopUp,
    Usd1Only,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SellSettlementPolicy {
    AlwaysToSol,
    AlwaysToUsd1,
    MatchStoredEntryPreference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeSettlementAsset {
    Sol,
    Usd1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PnlTrackingMode {
    Local,
    Rpc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MevMode {
    Off,
    Reduced,
    #[serde(rename = "secure")]
    Secure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchSelectionMode {
    SingleWallet,
    WalletList,
    WalletGroup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchLifecycleStatus {
    Queued,
    Submitted,
    PartiallyConfirmed,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveTokenRequest {
    pub source: String,
    pub platform: Platform,
    pub surface: Surface,
    #[serde(default)]
    pub url: Option<String>,
    /// Authoritative raw route input supplied by the extension. Must be a
    /// token mint or supported pool/pair address.
    #[serde(default)]
    pub address: Option<String>,
    /// Optional verified companion pair/pool address. When present, it is
    /// validated against `address` by the backend before it can lock a route.
    #[serde(default)]
    pub pair: Option<String>,
    /// Optional token-mint metadata supplied by the extension. The route
    /// planner still uses `address` as the authoritative route input.
    #[serde(default)]
    pub mint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenContextResponse {
    pub platform: Platform,
    pub surface: Surface,
    pub origin_surface: Surface,
    pub canonical_surface: Surface,
    pub source: String,
    pub source_url: String,
    pub mint: String,
    #[serde(default)]
    pub raw_address: Option<String>,
    #[serde(default)]
    pub pair_address: Option<String>,
    #[serde(default)]
    pub input_kind: Option<String>,
    #[serde(default)]
    pub non_canonical: bool,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub lifecycle: Option<String>,
    #[serde(default)]
    pub quote_asset: Option<String>,
    #[serde(default)]
    pub canonical_market_key: Option<String>,
    pub symbol: String,
    pub name: String,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrewarmRequest {
    /// Authoritative raw route input supplied by the extension. Must be a
    /// token mint or supported pool/pair address.
    #[serde(default)]
    pub address: Option<String>,
    /// Optional token-mint metadata supplied by the extension. This is not a
    /// substitute for the authoritative route `address`.
    #[serde(default)]
    pub mint: Option<String>,
    /// Optional verified companion pair/pool address. When present, it is
    /// validated against `address` by the backend before it can lock a route.
    #[serde(default)]
    pub pair: Option<String>,
    /// Page URL the warm came from — used only for diagnostics.
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    /// Optional side that the caller is waiting on. Omitted fire-and-forget
    /// warms cache both buy and sell routes.
    #[serde(default)]
    pub side: Option<TradeSide>,
    /// Optional active buy route policy from the extension. When absent,
    /// prewarm falls back to the engine default.
    #[serde(default)]
    pub buy_funding_policy: Option<BuyFundingPolicy>,
    /// Optional active sell settlement policy from the extension. When absent,
    /// prewarm falls back to the engine default.
    #[serde(default)]
    pub sell_settlement_policy: Option<SellSettlementPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeReadinessRequest {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub surface: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrewarmResponse {
    pub ok: bool,
    /// Opaque key clients can round-trip on buy/sell requests to match
    /// back to this warm entry.
    pub warm_key: String,
    /// Side-specific warm keys. `warm_key` remains the buy key for backwards
    /// compatibility with older extension builds.
    #[serde(default)]
    pub buy_warm_key: Option<String>,
    #[serde(default)]
    pub sell_warm_key: Option<String>,
    #[serde(default)]
    pub buy_warmed: bool,
    #[serde(default)]
    pub sell_warmed: bool,
    /// Mint that the warm entry is keyed on. If the request carried a
    /// pair address, this is the resolved mint.
    pub resolved_mint: String,
    /// Pool pubkey resolved from the request, if any.
    #[serde(default)]
    pub resolved_pair: Option<String>,
    #[serde(default)]
    pub raw_address: Option<String>,
    #[serde(default)]
    pub input_kind: Option<String>,
    #[serde(default)]
    pub non_canonical: bool,
    /// Resolved route family label (`pump-amm`, `bonk-raydium`,
    /// `meteora-damm-v2`, etc.) when planning succeeded. Absent when only
    /// classification succeeded.
    #[serde(default)]
    pub family: Option<String>,
    /// Lifecycle label of the cached selector (pre_migration |
    /// post_migration). Absent when only classification succeeded.
    #[serde(default)]
    pub lifecycle: Option<String>,
    /// Quote asset label (`SOL`, `WSOL`, `USD1`) when route identity is known.
    #[serde(default)]
    pub quote_asset: Option<String>,
    /// Canonical market key resolved for the warmed route when planning succeeded.
    #[serde(default)]
    pub canonical_market_key: Option<String>,
    /// How long the warm entry can be reused before its TTL expires
    /// (from this response's perspective).
    pub stale_after_ms: u64,
    /// Compact snapshot of the continuous transport warm loop so the
    /// panel can distinguish "first trade was slow because state was
    /// cold" from "transport warm was in idle suspend".
    #[serde(default)]
    pub transport_warm: Value,
    /// Per-family kill-switch state. Useful for the panel to gray out
    /// UI elements when a specific family is temporarily disabled.
    #[serde(default)]
    pub family_enabled: Value,
    /// Warning text surfaced back to the client (e.g. "non-canonical
    /// pool detected but policy is off"). Empty when nothing to say.
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPreviewRequest {
    pub side: TradeSide,
    /// Authoritative raw route input supplied by the extension. Must be a
    /// token mint or supported pool/pair address.
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    /// Optional verified companion pair/pool address. When present, it is
    /// validated against `address` by the backend before it can lock a route.
    #[serde(default)]
    pub mint: String,
    pub preset_id: String,
    pub wallet_key: Option<String>,
    pub wallet_keys: Option<Vec<String>>,
    pub wallet_group_id: Option<String>,
    #[serde(default)]
    pub buy_amount_sol: Option<String>,
    #[serde(default)]
    pub sell_percent: Option<String>,
    #[serde(default)]
    pub sell_output_sol: Option<String>,
    /// Legacy compatibility field. Ignored for route selection.
    #[serde(default)]
    pub pair: Option<String>,
    /// Legacy compatibility field. Ignored for route selection.
    #[serde(default)]
    pub pinned_pool: Option<String>,
    /// Opaque warm key returned by `/prewarm` identifying a cache entry.
    #[serde(default)]
    pub warm_key: Option<String>,
    #[serde(default)]
    pub buy_funding_policy: Option<BuyFundingPolicy>,
    #[serde(default)]
    pub sell_settlement_policy: Option<SellSettlementPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewTradePolicy {
    pub slippage_percent: String,
    pub mev_mode: MevMode,
    pub auto_tip_enabled: bool,
    pub fee_sol: String,
    pub tip_sol: String,
    pub buy_amount_sol: Option<String>,
    pub sell_percent: Option<String>,
    pub buy_funding_policy: BuyFundingPolicy,
    pub sell_settlement_policy: SellSettlementPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPreviewResponse {
    pub allowed: bool,
    pub side: TradeSide,
    pub target: ResolvedBatchTarget,
    pub policy: PreviewTradePolicy,
    #[serde(default)]
    pub batch_policy: Option<BatchExecutionPolicySummary>,
    #[serde(default)]
    pub execution_plan: Vec<WalletExecutionPlanSummary>,
    #[serde(default)]
    pub execution_adapter: Option<String>,
    #[serde(default)]
    pub execution_backend: Option<String>,
    #[serde(default)]
    pub planned_execution: Option<LifecycleAndCanonicalMarket>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuyRequest {
    pub client_request_id: String,
    #[serde(default)]
    pub client_started_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub client_request_started_at_unix_ms: Option<u64>,
    /// Authoritative raw route input supplied by the extension. Must be a
    /// token mint or supported pool/pair address.
    #[serde(default)]
    pub address: Option<String>,
    /// Legacy compatibility field. Ignored for route selection.
    #[serde(default)]
    pub mint: String,
    #[serde(default)]
    pub platform: Option<String>,
    pub preset_id: String,
    pub buy_amount_sol: Option<String>,
    pub wallet_key: Option<String>,
    pub wallet_keys: Option<Vec<String>>,
    pub wallet_group_id: Option<String>,
    /// Legacy compatibility field. Ignored for route selection.
    #[serde(default)]
    pub pair: Option<String>,
    /// Legacy compatibility field. Ignored for route selection.
    #[serde(default)]
    pub pinned_pool: Option<String>,
    #[serde(default)]
    pub warm_key: Option<String>,
    #[serde(default)]
    pub buy_funding_policy: Option<BuyFundingPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SellRequest {
    pub client_request_id: String,
    #[serde(default)]
    pub client_started_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub client_request_started_at_unix_ms: Option<u64>,
    /// Authoritative raw route input supplied by the extension. Must be a
    /// token mint or supported pool/pair address.
    #[serde(default)]
    pub address: Option<String>,
    /// Legacy compatibility field. Ignored for route selection.
    #[serde(default)]
    pub mint: String,
    #[serde(default)]
    pub platform: Option<String>,
    pub preset_id: String,
    #[serde(default)]
    pub sell_percent: Option<String>,
    #[serde(default)]
    pub sell_output_sol: Option<String>,
    pub wallet_key: Option<String>,
    pub wallet_keys: Option<Vec<String>>,
    pub wallet_group_id: Option<String>,
    /// Legacy compatibility field. Ignored for route selection.
    #[serde(default)]
    pub pair: Option<String>,
    /// Legacy compatibility field. Ignored for route selection.
    #[serde(default)]
    pub pinned_pool: Option<String>,
    #[serde(default)]
    pub warm_key: Option<String>,
    #[serde(default)]
    pub sell_settlement_policy: Option<SellSettlementPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedBatchTarget {
    pub selection_mode: BatchSelectionMode,
    pub wallet_group_id: Option<String>,
    #[serde(default)]
    pub wallet_group_label: Option<String>,
    #[serde(default)]
    pub batch_policy: Option<WalletGroupBatchPolicy>,
    pub wallet_keys: Vec<String>,
    pub wallet_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionAcceptedResponse {
    pub batch_id: String,
    pub client_request_id: String,
    pub accepted_at_unix_ms: u64,
    pub side: TradeSide,
    pub status: BatchLifecycleStatus,
    pub wallet_count: usize,
    pub deduped: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchStatusResponse {
    pub batch_id: String,
    pub client_request_id: String,
    pub side: TradeSide,
    pub status: BatchLifecycleStatus,
    #[serde(default)]
    pub created_at_unix_ms: u64,
    #[serde(default)]
    pub updated_at_unix_ms: u64,
    #[serde(default)]
    pub execution_adapter: Option<String>,
    #[serde(default)]
    pub execution_backend: Option<String>,
    #[serde(default)]
    pub planned_execution: Option<LifecycleAndCanonicalMarket>,
    #[serde(default)]
    pub batch_policy: Option<BatchExecutionPolicySummary>,
    pub summary: BatchSummary,
    pub wallets: Vec<WalletExecutionState>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResourceResponse {
    pub deleted: bool,
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchHistoryResponse {
    pub batches: Vec<BatchStatusResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    pub total_wallets: usize,
    pub queued_wallets: usize,
    pub submitted_wallets: usize,
    pub confirmed_wallets: usize,
    pub failed_wallets: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletExecutionState {
    pub wallet_key: String,
    pub status: BatchLifecycleStatus,
    pub tx_signature: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub buy_amount_sol: Option<String>,
    #[serde(default)]
    pub scheduled_delay_ms: u64,
    #[serde(default)]
    pub delay_applied: bool,
    #[serde(default)]
    pub first_buy: Option<bool>,
    #[serde(default)]
    pub applied_variance_percent: Option<f64>,
    #[serde(default)]
    pub entry_preference_asset: Option<TradeSettlementAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEngineState {
    version: String,
    data_root: String,
    settings: EngineSettings,
    #[serde(default)]
    config: Option<Value>,
    presets: Vec<PresetSummary>,
    wallets: Vec<WalletSummary>,
    wallet_groups: Vec<WalletGroupSummary>,
}

#[derive(Debug, Clone)]
struct AcceptedRequestRecord {
    fingerprint: String,
    accepted: ExecutionAcceptedResponse,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
struct TokenDistributionRequestRecord {
    fingerprint: String,
    response: Option<TokenDistributionResponse>,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
struct RewardsRequestRecord {
    fingerprint: String,
    response: Option<RewardsClaimResponse>,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
struct ExecutionSubmission {
    client_request_id: String,
    fingerprint: String,
    side: TradeSide,
    target: ResolvedBatchTarget,
    execution_adapter: Option<String>,
    execution_backend: String,
    planned_execution: Option<LifecycleAndCanonicalMarket>,
    client_started_at_unix_ms: Option<u64>,
    batch_policy: Option<BatchExecutionPolicySummary>,
    execution_plan: Vec<PlannedWalletExecution>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct ResolvedTokenRequest {
    platform: Platform,
    origin_surface: Surface,
    canonical_surface: Surface,
    source_url: String,
    raw_address: String,
}

#[derive(Debug, Clone)]
struct ResolvedTradePolicy {
    slippage_percent: String,
    mev_mode: MevMode,
    auto_tip_enabled: bool,
    fee_sol: String,
    tip_sol: String,
    provider: String,
    endpoint_profile: String,
    commitment: String,
    skip_preflight: bool,
    track_send_block_height: bool,
    buy_amount_sol: Option<String>,
    sell_percent: Option<String>,
    buy_funding_policy: BuyFundingPolicy,
    sell_settlement_policy: SellSettlementPolicy,
    sell_settlement_asset: TradeSettlementAsset,
    auto_fee_warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct PlannedWalletExecution {
    wallet_key: String,
    wallet_request: WalletTradeRequest,
    planned_summary: WalletExecutionPlanSummary,
}

#[derive(Debug, Clone)]
struct BatchExecutionPlan {
    batch_policy: Option<BatchExecutionPolicySummary>,
    wallets: Vec<PlannedWalletExecution>,
}

fn default_mev_mode_off() -> MevMode {
    MevMode::Off
}

fn default_buy_funding_policy_sol_only() -> BuyFundingPolicy {
    BuyFundingPolicy::SolOnly
}

fn default_sell_settlement_policy_always_to_sol() -> SellSettlementPolicy {
    SellSettlementPolicy::AlwaysToSol
}

fn default_pnl_tracking_mode_local() -> PnlTrackingMode {
    PnlTrackingMode::Local
}

fn default_buy_amount_rows() -> u8 {
    1
}

fn default_sell_percent_rows() -> u8 {
    1
}

const MAX_BUY_AMOUNT_ROWS: u8 = 2;
const BUY_AMOUNTS_PER_ROW: usize = 4;
// Sells use the same shape (max 2 rows of 4 entries each); separate constants
// would just be aliases of the buy ones.
const MAX_SELL_PERCENT_ROWS: u8 = MAX_BUY_AMOUNT_ROWS;
const SELL_PERCENTS_PER_ROW: usize = BUY_AMOUNTS_PER_ROW;

fn clamp_buy_amount_rows(rows: u8) -> u8 {
    if rows == 0 || rows > MAX_BUY_AMOUNT_ROWS {
        1
    } else {
        rows
    }
}

fn clamp_sell_percent_rows(rows: u8) -> u8 {
    if rows == 0 || rows > MAX_SELL_PERCENT_ROWS {
        1
    } else {
        rows
    }
}

fn infer_rows_from_shortcuts(rows: u8, values: &[String], values_per_row: usize) -> u8 {
    if rows == 2 {
        return 2;
    }
    let row2_has_value = values
        .iter()
        .skip(values_per_row)
        .take(values_per_row)
        .any(|value| !value.trim().is_empty());
    if row2_has_value { 2 } else { rows }
}

fn default_true() -> bool {
    true
}

fn route_buy_funding_policy(route: &Value) -> Option<BuyFundingPolicy> {
    parse_buy_funding_policy(route_string_field(route, "buyFundingPolicy").as_str())
}

fn route_sell_settlement_policy(route: &Value) -> Option<SellSettlementPolicy> {
    parse_sell_settlement_policy(route_string_field(route, "sellSettlementPolicy").as_str())
}

fn parse_buy_funding_policy(value: &str) -> Option<BuyFundingPolicy> {
    match value.trim().to_ascii_lowercase().as_str() {
        "sol_only" | "sol-only" | "sol only" => Some(BuyFundingPolicy::SolOnly),
        "prefer_usd1_else_topup"
        | "prefer_usd1_else_top_up"
        | "prefer-usd1-else-topup"
        | "prefer-usd1-else-top-up"
        | "prefer usd1 else topup"
        | "prefer usd1 else top up" => Some(BuyFundingPolicy::PreferUsd1ElseTopUp),
        "usd1_only" | "usd1-only" | "usd1 only" => Some(BuyFundingPolicy::Usd1Only),
        _ => None,
    }
}

fn parse_sell_settlement_policy(value: &str) -> Option<SellSettlementPolicy> {
    match value.trim().to_ascii_lowercase().as_str() {
        "always_to_sol" | "always-to-sol" | "always to sol" => {
            Some(SellSettlementPolicy::AlwaysToSol)
        }
        "always_to_usd1" | "always-to-usd1" | "always to usd1" => {
            Some(SellSettlementPolicy::AlwaysToUsd1)
        }
        "match_stored_entry_preference"
        | "match-stored-entry-preference"
        | "match stored entry preference" => Some(SellSettlementPolicy::MatchStoredEntryPreference),
        _ => None,
    }
}

fn resolve_stored_entry_preference_asset(
    entry_preference: Option<StoredEntryPreference>,
) -> Option<TradeSettlementAsset> {
    match entry_preference {
        Some(StoredEntryPreference::Sol) => Some(TradeSettlementAsset::Sol),
        Some(StoredEntryPreference::Usd1) => Some(TradeSettlementAsset::Usd1),
        _ => None,
    }
}

fn resolve_sell_settlement_asset(
    policy: SellSettlementPolicy,
    stored_entry_preference: Option<StoredEntryPreference>,
) -> TradeSettlementAsset {
    match policy {
        SellSettlementPolicy::AlwaysToSol => TradeSettlementAsset::Sol,
        SellSettlementPolicy::AlwaysToUsd1 => TradeSettlementAsset::Usd1,
        SellSettlementPolicy::MatchStoredEntryPreference => {
            resolve_stored_entry_preference_asset(stored_entry_preference)
                .unwrap_or(TradeSettlementAsset::Sol)
        }
    }
}

fn default_execution_provider() -> String {
    "standard-rpc".to_string()
}

fn default_execution_endpoint_profile() -> String {
    "global".to_string()
}

fn default_execution_commitment() -> String {
    "confirmed".to_string()
}

fn default_execution_skip_preflight() -> bool {
    true
}

fn default_wallet_enabled() -> bool {
    true
}

async fn get_auth_bootstrap(State(state): State<AppState>) -> Json<AuthBootstrapInfo> {
    Json(state.auth.bootstrap_info())
}

async fn require_authenticated_request(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix(AUTH_SCHEME_BEARER))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing bearer token for execution-engine API.".to_string(),
        ))?;
    let summary = state
        .auth
        .verify_token(token)
        .map_err(|error| (StatusCode::UNAUTHORIZED, error))?;
    request.extensions_mut().insert(summary);
    Ok(next.run(request).await)
}

async fn get_health(State(state): State<AppState>) -> Json<ExtensionHealthResponse> {
    let engine = state.engine.read().await.clone();
    let bootstrap = build_bootstrap_response(&engine);
    Json(ExtensionHealthResponse {
        contract_version: EXTENSION_CONTRACT_VERSION.to_string(),
        version: engine.version.clone(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        runtime_mode: EXECUTION_RUNTIME_MODE.to_string(),
        executor_route: state.executor.route_name().to_string(),
        execution_authority: EXECUTION_AUTHORITY.to_string(),
        status: "ready".to_string(),
        bind_address: host_bind_address(),
        loopback_only: true,
        bootstrap_revision: bootstrap_revision(&bootstrap, &state.state_path),
        data_root: engine.data_root,
    })
}

async fn get_runtime_status(State(state): State<AppState>) -> Json<ExtensionRuntimeStatusResponse> {
    let engine = state.engine.read().await.clone();
    let bootstrap = build_bootstrap_response(&engine);
    let active_batches = {
        let batches = state.batches.read().await;
        active_batch_count(&batches)
    };
    let mut diagnostics = runtime_diagnostics_snapshot();
    diagnostics.extend(
        state
            .balance_stream()
            .diagnostics_snapshot()
            .await
            .into_iter()
            .map(runtime_diagnostic_from_balance_stream),
    );
    Json(ExtensionRuntimeStatusResponse {
        contract_version: EXTENSION_CONTRACT_VERSION.to_string(),
        version: engine.version.clone(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        runtime_mode: EXECUTION_RUNTIME_MODE.to_string(),
        executor_route: state.executor.route_name().to_string(),
        execution_authority: EXECUTION_AUTHORITY.to_string(),
        status: "ready".to_string(),
        bind_address: host_bind_address(),
        loopback_only: true,
        bootstrap_revision: bootstrap_revision(&bootstrap, &state.state_path),
        data_root: engine.data_root,
        supported_origin_surfaces: Surface::supported_origin_surfaces(),
        supported_canonical_surfaces: Surface::supported_canonical_surfaces(),
        capabilities: bootstrap.capabilities,
        active_batches,
        max_active_batches: engine.settings.max_active_batches,
        idempotency_window_ms: IDEMPOTENCY_WINDOW_MS,
        warm: build_combined_warm_payload(&state.launchdeck_warm, &state.persist_failures),
        auto_fee: shared_fee_market_status_payload(shared_fee_market_runtime().config()),
        providers: provider_availability_registry(),
        diagnostics,
    })
}

fn runtime_diagnostic_from_balance_stream(payload: DiagnosticEventPayload) -> RuntimeDiagnostic {
    let at_ms = payload.at_ms.min(u128::from(u64::MAX)) as u64;
    RuntimeDiagnostic {
        key: payload.fingerprint.clone(),
        fingerprint: payload.fingerprint,
        severity: payload.severity,
        source: payload.source,
        code: payload.code,
        message: payload.message,
        detail: payload.detail,
        env_var: payload.env_var,
        endpoint_kind: payload.endpoint_kind,
        host: payload.host,
        active: payload.active,
        restart_required: payload.restart_required,
        first_seen_at_ms: at_ms,
        last_seen_at_ms: at_ms,
    }
}

fn build_combined_warm_payload(
    registry: &SharedLaunchdeckWarmRegistry,
    persist_failures: &PersistFailureCounters,
) -> Value {
    let mut payload = warm_runtime_payload(registry);
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "tradeMetrics".to_string(),
            crate::warm_metrics::shared_warm_metrics().snapshot(),
        );
        object.insert("persistFailures".to_string(), persist_failures.snapshot());
    }
    payload
}

async fn get_bootstrap(State(state): State<AppState>) -> Json<BootstrapResponse> {
    let engine = state.engine.read().await.clone();
    Json(build_bootstrap_response(&engine))
}

async fn get_wallet_status(
    State(state): State<AppState>,
    Json(request): Json<ExtensionWalletStatusRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let engine = state.engine.read().await.clone();
    let trade_ledger = state.trade_ledger.read().await.clone();
    let (payload, drifted_wallet_keys) =
        build_extension_wallet_status_payload(&engine, &trade_ledger, &request).await?;
    if !request.read_only {
        maybe_spawn_auto_resync(
            &state,
            &engine,
            &trade_ledger,
            &request,
            &drifted_wallet_keys,
        )
        .await;
    }
    Ok(Json(payload))
}

async fn maybe_spawn_auto_resync(
    state: &AppState,
    engine: &StoredEngineState,
    trade_ledger: &HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
    request: &ExtensionWalletStatusRequest,
    drifted_wallet_keys: &[String],
) {
    let Some(mint) = request
        .mint
        .as_deref()
        .and_then(trimmed_option)
        .map(str::to_string)
    else {
        return;
    };
    let Ok(target) = resolve_wallet_status_target(
        &build_effective_wallets(engine),
        &build_effective_wallet_groups(engine),
        request,
    ) else {
        return;
    };
    if target.wallet_keys.is_empty() {
        return;
    }
    let tracking_is_rpc = matches!(engine.settings.pnl_tracking_mode, PnlTrackingMode::Rpc);
    let drifted_set: HashSet<&str> = drifted_wallet_keys.iter().map(String::as_str).collect();
    let candidates: Vec<String> = target
        .wallet_keys
        .iter()
        .filter(|wallet_key| {
            if drifted_set.contains(wallet_key.as_str()) {
                return true;
            }
            if !tracking_is_rpc {
                return false;
            }
            let key = trade_ledger_lookup_key(wallet_key, &mint);
            match trade_ledger.get(&key) {
                Some(entry) => {
                    entry.buy_count == 0
                        && entry.sell_count == 0
                        && entry.last_trade_at_unix_ms == 0
                }
                None => true,
            }
        })
        .cloned()
        .collect();
    if candidates.is_empty() {
        return;
    }

    let now_ms = now_unix_ms();
    let to_schedule: Vec<String> = {
        let mut tracker = state.auto_resync_tracker.write().await;
        candidates
            .into_iter()
            .filter(|wallet_key| {
                let key = format!("{wallet_key}::{mint}");
                if tracker.in_flight.contains(&key) {
                    return false;
                }
                if let Some(next_at) = tracker.cooldown_until_ms.get(&key) {
                    if now_ms < *next_at {
                        return false;
                    }
                }
                tracker.in_flight.insert(key);
                true
            })
            .collect()
    };
    if to_schedule.is_empty() {
        return;
    }

    let state_clone = state.clone();
    let mint_clone = mint.clone();
    tokio::spawn(async move {
        run_auto_resync(state_clone, mint_clone, to_schedule).await;
    });
}

async fn run_auto_resync(state: AppState, mint: String, wallet_keys: Vec<String>) {
    let result = resync_pnl_history(
        State(state.clone()),
        Json(PnlHistoryScopeRequest {
            wallet_key: None,
            wallet_keys: Some(wallet_keys.clone()),
            wallet_group_id: None,
            mint: mint.clone(),
        }),
    )
    .await;
    let next_at = now_unix_ms().saturating_add(AUTO_RESYNC_COOLDOWN_MS);
    {
        let mut tracker = state.auto_resync_tracker.write().await;
        for wallet_key in &wallet_keys {
            let key = format!("{wallet_key}::{mint}");
            tracker.in_flight.remove(&key);
            tracker.cooldown_until_ms.insert(key, next_at);
        }
    }
    match result {
        Ok(_) => {
            maybe_force_close_drifted_positions(&state, &mint, &wallet_keys).await;
        }
        Err((status, err)) => {
            eprintln!(
                "[execution-engine][wallet-status] auto-resync failed mint={mint} status={status} err={err}"
            );
        }
    }
}

/// Invoked after a successful auto-resync when the chain reported an empty ATA
/// but the ledger still carries open lots. If the resync did not recover any
/// missing sells, we converge with on-chain truth by writing a
/// [`ForceCloseMarkerEvent`] and realising the remaining cost basis as a
/// synthetic realised loss. Bounded by [`FORCE_CLOSE_COOLDOWN_MS`] per
/// wallet::mint to prevent repeat writes if later polls observe the same
/// drift before snapshots propagate.
async fn maybe_force_close_drifted_positions(state: &AppState, mint: &str, wallet_keys: &[String]) {
    let engine = state.engine.read().await.clone();
    let wallet_views: Vec<WalletStatusView> = build_effective_wallets(&engine)
        .into_iter()
        .filter(|wallet| wallet_keys.iter().any(|key| key == &wallet.key))
        .map(|wallet| WalletStatusView {
            key: wallet.key,
            label: wallet.label,
            enabled: wallet.enabled,
            public_key: Some(wallet.public_key),
            error: None,
            balance_lamports: None,
            balance_sol: None,
            usd1_balance: None,
            balance_error: None,
            mint_balance: MintBalanceSnapshot::default(),
        })
        .collect();
    if wallet_views.is_empty() {
        return;
    }
    let mint_balances = match fetch_wallet_mint_balances(&configured_rpc_url(), &wallet_views, mint)
        .await
    {
        Ok(balances) => balances,
        Err(error) => {
            eprintln!(
                "[execution-engine][wallet-status] force-close balance refresh failed mint={mint} err={error}"
            );
            return;
        }
    };

    let drifted: Vec<String> = {
        let ledger = state.trade_ledger.read().await;
        wallet_keys
            .iter()
            .filter(|wallet_key| {
                let Some(entry) = ledger.get(&trade_ledger_lookup_key(wallet_key, mint)) else {
                    return false;
                };
                if entry.open_lots.is_empty() && !entry.position_open {
                    return false;
                }
                let Some(snapshot) = mint_balances.get(wallet_key.as_str()) else {
                    return false;
                };
                if snapshot.error.is_some() {
                    return false;
                }
                snapshot.raw == Some(0)
            })
            .cloned()
            .collect()
    };
    if drifted.is_empty() {
        return;
    }

    let now_ms = now_unix_ms();
    let to_apply: Vec<String> = {
        let mut tracker = state.auto_resync_tracker.write().await;
        drifted
            .into_iter()
            .filter(|wallet_key| {
                let key = format!("{wallet_key}::{mint}");
                if let Some(next_at) = tracker.force_close_cooldown_until_ms.get(&key) {
                    if now_ms < *next_at {
                        return false;
                    }
                }
                tracker
                    .force_close_cooldown_until_ms
                    .insert(key, now_ms.saturating_add(FORCE_CLOSE_COOLDOWN_MS));
                true
            })
            .collect()
    };
    if to_apply.is_empty() {
        return;
    }

    for wallet_key in &to_apply {
        let marker =
            ForceCloseMarkerEvent::new(wallet_key, mint, now_ms, "on-chain-zero-after-resync");
        if let Err((status, err)) = append_force_close_marker(&state.trade_ledger_paths, &marker) {
            eprintln!(
                "[execution-engine][wallet-status] force-close journal append failed wallet={wallet_key} mint={mint} status={status} err={err}"
            );
            continue;
        }
        let mut ledger = state.trade_ledger.write().await;
        force_close_trade_ledger_position(&mut ledger, wallet_key, mint, now_ms);
        if let Err((status, err)) = persist_trade_ledger(&state.trade_ledger_paths, &ledger) {
            eprintln!(
                "[execution-engine][wallet-status] force-close snapshot persist failed wallet={wallet_key} mint={mint} status={status} err={err}"
            );
        }
    }
}

async fn reset_pnl_history(
    State(state): State<AppState>,
    Json(request): Json<PnlHistoryScopeRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mint = request.mint.trim().to_string();
    if mint.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "mint is required".to_string()));
    }
    let engine = state.engine.read().await.clone();
    let target = resolve_wallet_status_target(
        &build_effective_wallets(&engine),
        &build_effective_wallet_groups(&engine),
        &ExtensionWalletStatusRequest {
            wallet_key: request.wallet_key.clone(),
            wallet_keys: request.wallet_keys.clone(),
            wallet_group_id: request.wallet_group_id.clone(),
            mint: Some(mint.clone()),
            ..ExtensionWalletStatusRequest::default()
        },
    )?;
    if target.wallet_keys.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No wallets selected.".to_string()));
    }

    let selected_wallets = build_effective_wallets(&engine)
        .into_iter()
        .filter(|wallet| target.wallet_keys.contains(&wallet.key))
        .map(|wallet| WalletStatusView {
            key: wallet.key,
            label: wallet.label,
            enabled: wallet.enabled,
            public_key: Some(wallet.public_key),
            error: None,
            balance_lamports: None,
            balance_sol: None,
            usd1_balance: None,
            balance_error: None,
            mint_balance: MintBalanceSnapshot::default(),
        })
        .collect::<Vec<_>>();
    let mint_balances = fetch_wallet_mint_balances(&configured_rpc_url(), &selected_wallets, &mint)
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
    let has_unknown_onchain_balance = target.wallet_keys.iter().any(|wallet_key| {
        let Some(snapshot) = mint_balances.get(wallet_key) else {
            return true;
        };
        snapshot.error.is_some() || snapshot.raw.is_none()
    });
    if has_unknown_onchain_balance {
        return Err((
            StatusCode::BAD_GATEWAY,
            "Could not verify on-chain token balances for every selected wallet. Try again."
                .to_string(),
        ));
    }
    let has_open_onchain_balance = target.wallet_keys.iter().any(|wallet_key| {
        mint_balances
            .get(wallet_key)
            .and_then(|snapshot| snapshot.raw)
            .unwrap_or(0)
            > 0
    });
    if has_open_onchain_balance {
        return Err((
            StatusCode::BAD_REQUEST,
            "Reset is only allowed when the position is fully closed.".to_string(),
        ));
    }

    let reset_at_unix_ms = now_unix_ms();
    let reset_at_slot = Some(
        fetch_current_confirmed_slot()
            .await
            .map_err(|error| (StatusCode::BAD_GATEWAY, error))?,
    );
    // Persist a reset marker to the append-only journal *before* mutating the
    // snapshot. If the snapshot write fails later, a subsequent journal rebuild
    // will still honour the reset. Markers are small (wallet/mint/timestamp),
    // so writing one per selected wallet is cheap.
    for wallet_key in &target.wallet_keys {
        let marker = crate::trade_ledger::ResetMarkerEvent::new(
            wallet_key,
            &mint,
            reset_at_unix_ms,
            reset_at_slot,
        );
        append_reset_marker(&state.trade_ledger_paths, &marker)?;
    }
    let mut ledger = state.trade_ledger.write().await;
    for wallet_key in &target.wallet_keys {
        reset_trade_ledger_position(
            &mut ledger,
            wallet_key,
            &mint,
            reset_at_unix_ms,
            reset_at_slot,
        );
    }
    persist_trade_ledger(&state.trade_ledger_paths, &ledger)?;
    Ok(Json(json!({
        "ok": true,
        "mint": mint,
        "walletKeys": target.wallet_keys,
        "resetAtUnixMs": reset_at_unix_ms,
        "resetAtSlot": reset_at_slot,
    })))
}

async fn wipe_pnl_history(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    match fs::remove_dir_all(&state.trade_ledger_paths.root_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
    }
    {
        let mut ledger = state.trade_ledger.write().await;
        ledger.clear();
    }
    {
        let mut event_ids = state.trade_ledger_event_ids.write().await;
        event_ids.clear();
    }
    {
        let mut tracker = state.auto_resync_tracker.write().await;
        tracker.in_flight.clear();
        tracker.cooldown_until_ms.clear();
        tracker.force_close_cooldown_until_ms.clear();
    }
    Ok(Json(json!({
        "ok": true,
        "wipedAtUnixMs": now_unix_ms(),
    })))
}

async fn export_pnl_history(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use std::io::{Cursor, Write};
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    let cursor = Cursor::new(Vec::<u8>::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();
    let exported_at_unix_ms = now_unix_ms();
    let root_dir = &state.trade_ledger_paths.root_dir;
    let files = [
        (
            "snapshots/open-positions.json",
            &state.trade_ledger_paths.open_positions_path,
        ),
        (
            "snapshots/closed-positions.json",
            &state.trade_ledger_paths.closed_positions_path,
        ),
        (
            "snapshots/pnl-snapshots.json",
            &state.trade_ledger_paths.snapshots_path,
        ),
    ];
    for (name, path) in files {
        if let Ok(bytes) = fs::read(path) {
            writer
                .start_file(name, options)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            writer
                .write_all(&bytes)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        }
    }
    if let Ok(entries) = fs::read_dir(&state.trade_ledger_paths.journal_dir) {
        let mut journal_paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        journal_paths.sort();
        for path in journal_paths {
            if !path.is_file() {
                continue;
            }
            let relative_name = path
                .strip_prefix(root_dir)
                .ok()
                .and_then(|value| value.to_str())
                .map(|value| value.replace('\\', "/"))
                .unwrap_or_else(|| "journal/unknown.jsonl".to_string());
            let bytes = fs::read(&path)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            writer
                .start_file(relative_name, options)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            writer
                .write_all(&bytes)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        }
    }
    let cursor = writer
        .finish()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let bytes = cursor.into_inner();
    Ok(Json(json!({
        "ok": true,
        "filename": format!("trench-tools-pnl-history-{}.zip", exported_at_unix_ms),
        "zipBase64": BASE64.encode(bytes),
        "exportedAtUnixMs": exported_at_unix_ms,
    })))
}

async fn resync_pnl_history(
    State(state): State<AppState>,
    Json(request): Json<PnlHistoryScopeRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mint = request.mint.trim().to_string();
    if mint.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "mint is required".to_string()));
    }
    let engine = state.engine.read().await.clone();
    let target = resolve_wallet_status_target(
        &build_effective_wallets(&engine),
        &build_effective_wallet_groups(&engine),
        &ExtensionWalletStatusRequest {
            wallet_key: request.wallet_key.clone(),
            wallet_keys: request.wallet_keys.clone(),
            wallet_group_id: request.wallet_group_id.clone(),
            mint: Some(mint.clone()),
            ..ExtensionWalletStatusRequest::default()
        },
    )?;
    if target.wallet_keys.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No wallets selected.".to_string()));
    }

    let shared_wallets = shared_config_manager().current_snapshot().wallets;
    let public_keys_by_wallet_key = shared_wallets
        .into_iter()
        .map(|wallet| (wallet.key, wallet.public_key))
        .collect::<HashMap<_, _>>();
    let known_wallets = public_keys_by_wallet_key
        .iter()
        .map(|(wallet_key, public_key)| KnownWalletIdentity {
            wallet_key: wallet_key.clone(),
            public_key: public_key.clone(),
        })
        .collect::<Vec<_>>();
    let helius_config = configured_helius_resync_config();
    let selected_wallet_keys = target.wallet_keys.iter().cloned().collect::<HashSet<_>>();
    let current_ledger = state.trade_ledger.read().await.clone();
    let reset_baselines_by_wallet = target
        .wallet_keys
        .iter()
        .map(|wallet_key| {
            let baseline = current_ledger
                .get(&trade_ledger_lookup_key(wallet_key, &mint))
                .map(|entry| (entry.reset_baseline_unix_ms, entry.reset_baseline_slot))
                .unwrap_or((0, None));
            (wallet_key.clone(), baseline)
        })
        .collect::<HashMap<_, _>>();
    let journal_entries = read_journal_entries(&state.trade_ledger_paths);
    let journal_events = journal_entries
        .iter()
        .filter_map(|entry| match entry {
            JournalEntry::Trade(event) => Some(event.clone()),
            _ => None,
        })
        .filter(|event| event.mint == mint && selected_wallet_keys.contains(&event.wallet_key))
        .filter(|event| {
            let (reset_baseline_unix_ms, reset_baseline_slot) = reset_baselines_by_wallet
                .get(&event.wallet_key)
                .copied()
                .unwrap_or((0, None));
            crate::trade_ledger::trade_event_is_after_reset_baseline(
                event.confirmed_at_unix_ms,
                event.slot,
                reset_baseline_unix_ms,
                reset_baseline_slot,
            )
        })
        .collect::<Vec<_>>();
    let local_events_by_id = journal_events
        .iter()
        .map(|event| (event.event_id(), event.clone()))
        .collect::<HashMap<_, _>>();
    let known_journal_event_ids = journal_entries
        .iter()
        .filter_map(|entry| match entry {
            JournalEntry::Trade(event) => Some(event),
            _ => None,
        })
        .map(|event| event.event_id())
        .collect::<HashSet<_>>();
    let mut rpc_candidates = fetch_resync_candidates_for_wallets(
        &target.wallet_keys,
        &public_keys_by_wallet_key,
        &mint,
        &reset_baselines_by_wallet,
        &known_wallets,
        helius_config.as_ref(),
    )
    .await?;
    let mut seen_candidates = HashSet::new();
    rpc_candidates.retain(|candidate| {
        seen_candidates.insert(format!(
            "{}::{}::{}",
            candidate.signature, candidate.wallet_key, mint
        ))
    });

    let mut resync_actions = build_rpc_resync_ledger_actions(
        rpc_candidates,
        &mint,
        &selected_wallet_keys,
        &local_events_by_id,
    )
    .await?;
    merge_missing_local_resync_events(&mut resync_actions, &journal_events);
    merge_scoped_local_resync_markers(
        &mut resync_actions,
        &journal_entries,
        &selected_wallet_keys,
        &mint,
        &reset_baselines_by_wallet,
    );
    sort_resync_actions(&mut resync_actions);

    let mut preview_ledger = current_ledger.clone();
    reset_resync_scope_in_ledger(
        &mut preview_ledger,
        &target.wallet_keys,
        &mint,
        &reset_baselines_by_wallet,
    );
    let extra_actions = replay_resync_actions(&mut preview_ledger, &resync_actions, true);
    let replayed_balances =
        replayed_token_balances_from_actions(&target.wallet_keys, &mint, &resync_actions);
    resync_actions.extend(extra_actions);
    let reconciliation_slot = fetch_current_confirmed_slot()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
    let onchain_snapshot = fetch_current_resync_balances(
        &target.wallet_keys,
        &public_keys_by_wallet_key,
        &mint,
        Some(reconciliation_slot),
    )
    .await?;
    let reconciliation_actions = build_balance_reconciliation_actions(
        &target.wallet_keys,
        &mint,
        &replayed_balances,
        &onchain_snapshot.balances,
        &onchain_snapshot.slots_by_wallet,
        now_unix_ms(),
    );
    resync_actions.extend(reconciliation_actions);
    prune_incomplete_zero_clears_shadowed_by_nonzero_actions(&mut resync_actions);
    sort_resync_actions(&mut resync_actions);

    let rpc_events = resync_actions
        .iter()
        .filter_map(|action| match action {
            RpcResyncLedgerAction::Trade(event) => Some(event.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let transfer_markers = resync_actions
        .iter()
        .filter_map(|action| match action {
            RpcResyncLedgerAction::KnownTransfer { marker, .. } => Some(marker.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let force_close_markers = resync_actions
        .iter()
        .filter_map(|action| match action {
            RpcResyncLedgerAction::ForceClose { marker, persist } if *persist => {
                Some(marker.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let incomplete_markers = resync_actions
        .iter()
        .filter_map(|action| match action {
            RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                wallet_key,
                mint,
                amount_raw,
                signature,
                applied_at_unix_ms,
                slot,
                persist,
            } => Some(persist.then(|| {
                IncompleteBalanceAdjustmentMarkerEvent::received_without_cost_basis(
                    wallet_key,
                    mint,
                    *amount_raw,
                    signature,
                    *applied_at_unix_ms,
                    *slot,
                )
            }))
            .flatten(),
            RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key,
                mint,
                amount_raw,
                signature,
                applied_at_unix_ms,
                slot,
                persist,
            } => Some(persist.then(|| {
                IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
                    wallet_key,
                    mint,
                    *amount_raw,
                    signature,
                    *applied_at_unix_ms,
                    *slot,
                )
            }))
            .flatten(),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut appended_rpc_events = 0usize;
    if !rpc_events.is_empty() {
        let mut event_ids = state.trade_ledger_event_ids.write().await;
        for event in &rpc_events {
            if !known_journal_event_ids.contains(&event.event_id()) {
                append_confirmed_trade_event(&state.trade_ledger_paths, event)?;
                appended_rpc_events = appended_rpc_events.saturating_add(1);
            }
            event_ids.insert(event.event_id());
        }
    }
    for marker in &transfer_markers {
        let _ =
            crate::trade_ledger::append_token_transfer_marker(&state.trade_ledger_paths, marker)?;
    }
    for marker in &force_close_markers {
        append_force_close_marker(&state.trade_ledger_paths, marker)?;
    }
    for marker in &incomplete_markers {
        let _ = append_incomplete_balance_adjustment_marker(&state.trade_ledger_paths, marker)?;
    }

    {
        let mut ledger = state.trade_ledger.write().await;
        reset_resync_scope_in_ledger(
            &mut ledger,
            &target.wallet_keys,
            &mint,
            &reset_baselines_by_wallet,
        );
        let _ = replay_resync_actions(&mut ledger, &resync_actions, false);
        persist_trade_ledger(&state.trade_ledger_paths, &ledger)?;
    }
    Ok(Json(json!({
        "ok": true,
        "mint": mint,
        "walletKeys": target.wallet_keys,
        "replayedEvents": resync_actions.len(),
        "appendedRpcEvents": appended_rpc_events,
        "resyncedAtUnixMs": now_unix_ms(),
    })))
}

async fn get_settings(State(state): State<AppState>) -> Json<EngineSettings> {
    let engine = state.engine.read().await.clone();
    Json(build_settings_response(&engine.settings))
}

async fn get_canonical_config(State(state): State<AppState>) -> Json<Value> {
    let engine = state.engine.read().await.clone();
    Json(build_launchdeck_settings_payload(&engine))
}

async fn update_settings(
    State(state): State<AppState>,
    Json(settings): Json<EngineSettings>,
) -> Result<Json<EngineSettings>, (StatusCode, String)> {
    let settings = normalize_settings(settings);
    let shared_rpc = shared_rpc_config_from_settings(&settings);
    shared_config_manager()
        .update_rpc_config(&shared_rpc)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let blockhash_commitment = settings.execution_commitment.clone();
    let mut engine = state.engine.write().await;
    engine.settings = settings.clone();
    if let Some(config) = engine.config.clone() {
        let next_config =
            set_track_send_block_height(&config, engine.settings.track_send_block_height);
        let next_config = set_allow_non_canonical_pool_trades(
            &next_config,
            engine.settings.allow_non_canonical_pool_trades,
        );
        let next_config = set_wrapper_default_fee_bps_in_config(
            &next_config,
            engine.settings.wrapper_default_fee_bps,
        );
        engine.config = Some(next_config);
    }
    crate::rollout::set_allow_non_canonical_pool_trades(
        engine.settings.allow_non_canonical_pool_trades,
    );
    crate::rollout::set_wrapper_default_fee_bps(engine.settings.wrapper_default_fee_bps);
    persist_engine_state(&state.state_path, &engine)?;
    let rpc_url = configured_rpc_url();
    tokio::spawn(async move {
        let _ = shared_warming_service()
            .warm_execution_primitives(&rpc_url, &blockhash_commitment)
            .await;
    });
    Ok(Json(build_settings_response(&engine.settings)))
}

async fn update_canonical_config(
    State(state): State<AppState>,
    Json(request): Json<LaunchdeckSettingsSaveRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let next_config =
        normalize_canonical_config(request.config.unwrap_or_else(default_canonical_config));
    let blockhash_commitment = {
        let engine = state.engine.read().await;
        engine.settings.execution_commitment.clone()
    };
    let rpc_url = configured_rpc_url();
    let mut engine = state.engine.write().await;
    engine.config = Some(next_config.clone());
    engine.settings.track_send_block_height = config_track_send_block_height(&next_config);
    engine.settings.default_buy_funding_policy = config_default_buy_funding_policy(&next_config);
    engine.settings.default_sell_settlement_policy =
        config_default_sell_settlement_policy(&next_config);
    engine.settings.allow_non_canonical_pool_trades =
        config_allow_non_canonical_pool_trades(&next_config);
    engine.settings.wrapper_default_fee_bps = config_wrapper_default_fee_bps(&next_config);
    crate::rollout::set_allow_non_canonical_pool_trades(
        engine.settings.allow_non_canonical_pool_trades,
    );
    crate::rollout::set_wrapper_default_fee_bps(engine.settings.wrapper_default_fee_bps);
    update_default_routes(
        &state.launchdeck_warm,
        configured_active_warm_routes_from_config(&next_config),
    );
    persist_engine_state(&state.state_path, &engine)?;
    tokio::spawn(async move {
        let _ = shared_warming_service()
            .warm_execution_primitives(&rpc_url, &blockhash_commitment)
            .await;
    });
    Ok(Json(build_launchdeck_settings_payload(&engine)))
}

async fn record_launchdeck_confirmed_trades(
    State(state): State<AppState>,
    Json(request): Json<LaunchdeckConfirmedTradesRequest>,
) -> Json<LaunchdeckConfirmedTradesResponse> {
    let mut response = LaunchdeckConfirmedTradesResponse::default();
    for trade in request.trades {
        let wallet_key = trade.wallet_key.trim();
        let mint = trade.mint.trim();
        let signature = trade.signature.trim();
        if wallet_key.is_empty() || mint.is_empty() || signature.is_empty() {
            response.errors.push(
                "LaunchDeck trade-ledger record is missing walletKey, mint, or signature."
                    .to_string(),
            );
            continue;
        }
        match record_inferred_confirmed_trade_ledger_entry(
            &state,
            wallet_key,
            signature,
            mint,
            PlatformTag::Unknown,
            EventProvenance::LocalExecution,
            trade.client_request_id.as_deref(),
            trade.batch_id.as_deref(),
        )
        .await
        {
            Ok(outcome) => match outcome.state {
                ConfirmedTradeLedgerRecordState::Recorded => {
                    response.recorded_count = response.recorded_count.saturating_add(1);
                    publish_confirmed_trade_balance_stream_event(
                        &state,
                        trade.client_request_id.as_deref(),
                        trade.batch_id.as_deref(),
                        signature,
                        outcome.slot,
                    );
                }
                ConfirmedTradeLedgerRecordState::Duplicate => {
                    response.duplicate_count = response.duplicate_count.saturating_add(1);
                }
                ConfirmedTradeLedgerRecordState::Ignored => {
                    response.ignored_count = response.ignored_count.saturating_add(1);
                }
            },
            Err(error) => {
                let message =
                    format!("wallet={wallet_key} mint={mint} signature={signature}: {error}");
                if is_transient_trade_ledger_error(&error) {
                    response
                        .transient_failures
                        .push(LaunchdeckConfirmedTradeFailure {
                            signature: signature.to_string(),
                            message,
                        });
                } else {
                    response.errors.push(message);
                }
            }
        }
    }
    response.ok = response.errors.is_empty() && response.transient_failures.is_empty();
    Json(response)
}

/// Heuristic: classify a trade-ledger record error as transient (keep in the
/// caller's outbox, worth retrying) vs permanent (retrying won't help).
///
/// The errors bubble up from a handful of sites in
/// `record_inferred_confirmed_trade_ledger_entry` and below - RPC lookups,
/// SOL price fallback, and ledger persistence. We use message-prefix matches
/// rather than typed errors to avoid plumbing a new error enum through every
/// helper.
fn is_transient_trade_ledger_error(message: &str) -> bool {
    const PERMANENT_MARKERS: &[&str] = &[
        "Unknown wallet key for trade ledger",
        "did not include wallet",
        "did not include a pre-balance",
        "did not include a post-balance",
        "did not include account keys",
    ];
    const TRANSIENT_MARKERS: &[&str] = &[
        "was not yet available",
        "Failed to fetch",
        "Failed to persist",
        "Failed to read",
        "Failed to write",
        "Failed to append",
        "Failed to resolve",
        "rpc",
        "RPC",
        "timeout",
        "Timeout",
        "timed out",
        "connection",
        "Connection",
        "temporarily",
    ];
    if PERMANENT_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
    {
        return false;
    }
    if TRANSIENT_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
    {
        return true;
    }
    // Unknown error shape: default to transient so we don't drop a row whose
    // failure cause we can't identify. The outbox has a retry cap that will
    // eventually archive these rows if they keep failing.
    true
}

async fn list_auth_tokens(State(state): State<AppState>) -> Json<Vec<AuthTokenSummary>> {
    Json(state.auth.list_tokens())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmedTradeLedgerRecordState {
    Recorded,
    Duplicate,
    Ignored,
}

#[derive(Debug, Clone, Copy)]
struct ConfirmedTradeLedgerRecordOutcome {
    state: ConfirmedTradeLedgerRecordState,
    slot: Option<u64>,
}

#[derive(Debug, Clone)]
enum ConfirmedTradeLedgerRecordError {
    Validation(String),
    Persist(String),
}

impl std::fmt::Display for ConfirmedTradeLedgerRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(error) | Self::Persist(error) => write!(f, "{error}"),
        }
    }
}

fn normalized_trade_event_identity(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn publish_confirmed_trade_balance_stream_event(
    state: &AppState,
    client_request_id: Option<&str>,
    batch_id: Option<&str>,
    signature: &str,
    slot: Option<u64>,
) {
    let normalized_signature = signature.trim();
    if normalized_signature.is_empty() {
        return;
    }
    state.balance_stream.publish_trade_event(TradeEventPayload {
        client_request_id: normalized_trade_event_identity(client_request_id, normalized_signature),
        batch_id: normalized_trade_event_identity(batch_id, normalized_signature),
        signature: normalized_signature.to_string(),
        status: "confirmed".to_string(),
        slot,
        err: None,
        ledger_applied: Some(true),
        at_ms: u128::from(now_unix_ms()),
    });
}

async fn create_auth_token(
    State(state): State<AppState>,
    Json(request): Json<CreateAuthTokenRequest>,
) -> Result<Json<CreatedAuthToken>, (StatusCode, String)> {
    let created = state
        .auth
        .create_token(&request.label)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(created))
}

async fn revoke_auth_token(
    State(state): State<AppState>,
    Path(token_id): Path<String>,
) -> Result<Json<AuthTokenSummary>, (StatusCode, String)> {
    let revoked = state
        .auth
        .revoke_token(&token_id)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(revoked))
}

async fn list_presets(State(state): State<AppState>) -> Json<Vec<PresetSummary>> {
    Json(state.engine.read().await.presets.clone())
}

async fn get_preset(
    State(state): State<AppState>,
    Path(preset_id): Path<String>,
) -> Result<Json<PresetSummary>, (StatusCode, String)> {
    let engine = state.engine.read().await;
    let preset = engine
        .presets
        .iter()
        .find(|preset| preset.id == preset_id)
        .cloned()
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("unknown preset id: {preset_id}"),
        ))?;
    Ok(Json(preset))
}

async fn create_preset(
    State(state): State<AppState>,
    Json(preset): Json<PresetSummary>,
) -> Result<Json<PresetSummary>, (StatusCode, String)> {
    let preset = normalize_preset(preset).map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let mut engine = state.engine.write().await;
    if engine.presets.iter().any(|item| item.id == preset.id) {
        return Err((
            StatusCode::CONFLICT,
            format!("preset {} already exists", preset.id),
        ));
    }
    engine.presets.push(preset.clone());
    sync_canonical_preset(&mut engine, &preset);
    persist_engine_state(&state.state_path, &engine)?;
    Ok(Json(preset))
}

async fn update_preset(
    State(state): State<AppState>,
    Path(preset_id): Path<String>,
    Json(preset): Json<PresetSummary>,
) -> Result<Json<PresetSummary>, (StatusCode, String)> {
    let preset = normalize_preset(preset).map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let mut engine = state.engine.write().await;
    let Some(_index) = engine.presets.iter().position(|item| item.id == preset_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("unknown preset id: {preset_id}"),
        ));
    };
    if preset.id != preset_id && engine.presets.iter().any(|item| item.id == preset.id) {
        return Err((
            StatusCode::CONFLICT,
            format!("preset {} already exists", preset.id),
        ));
    }
    let index = engine
        .presets
        .iter()
        .position(|item| item.id == preset_id)
        .expect("validated preset exists");
    engine.presets[index] = preset.clone();
    // If the preset id was renamed, drop the old entry from the canonical
    // presets collection first so we don't end up with duplicates.
    if preset.id != preset_id {
        remove_canonical_preset(&mut engine, &preset_id);
    }
    sync_canonical_preset(&mut engine, &preset);
    persist_engine_state(&state.state_path, &engine)?;
    Ok(Json(preset))
}

async fn delete_preset(
    State(state): State<AppState>,
    Path(preset_id): Path<String>,
) -> Result<Json<DeleteResourceResponse>, (StatusCode, String)> {
    let mut engine = state.engine.write().await;
    let original_len = engine.presets.len();
    engine.presets.retain(|preset| preset.id != preset_id);
    if engine.presets.len() == original_len {
        return Err((
            StatusCode::NOT_FOUND,
            format!("unknown preset id: {preset_id}"),
        ));
    }
    remove_canonical_preset(&mut engine, &preset_id);
    persist_engine_state(&state.state_path, &engine)?;
    Ok(Json(DeleteResourceResponse {
        deleted: true,
        resource_id: preset_id,
    }))
}

async fn list_wallets(State(state): State<AppState>) -> Json<Vec<WalletSummary>> {
    let engine = state.engine.read().await.clone();
    Json(build_effective_wallets(&engine))
}

async fn get_wallet(
    State(state): State<AppState>,
    Path(wallet_key): Path<String>,
) -> Result<Json<WalletSummary>, (StatusCode, String)> {
    let engine = state.engine.read().await.clone();
    let wallet = build_effective_wallets(&engine)
        .iter()
        .find(|wallet| wallet.key == wallet_key)
        .cloned()
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("unknown wallet key: {wallet_key}"),
        ))?;
    Ok(Json(wallet))
}

async fn create_wallet(
    State(state): State<AppState>,
    Json(request): Json<CreateWalletRequest>,
) -> Result<Json<WalletSummary>, (StatusCode, String)> {
    let mut engine = state.engine.write().await;
    let created = shared_config_manager()
        .create_wallet(&request.private_key, &request.label)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let wallet = WalletSummary {
        key: created.key,
        label: created.label,
        public_key: created.public_key,
        enabled: request.enabled,
        emoji: request.emoji.trim().to_string(),
    };
    engine.wallets.retain(|item| item.key != wallet.key);
    engine.wallets.push(wallet.clone());
    persist_engine_state(&state.state_path, &engine)?;
    invalidate_wallet_balance_cache(&[wallet.key.clone()]);
    state
        .balance_stream
        .resync_wallets(runtime_wallets_from_engine(&engine));
    Ok(Json(wallet))
}

async fn update_wallet(
    State(state): State<AppState>,
    Path(wallet_key): Path<String>,
    Json(request): Json<UpdateWalletRequest>,
) -> Result<Json<WalletSummary>, (StatusCode, String)> {
    let mut engine = state.engine.write().await;
    let updated_shared = shared_config_manager()
        .update_wallet(
            &wallet_key,
            request.private_key.as_deref(),
            request.label.as_deref(),
        )
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let index = engine
        .wallets
        .iter()
        .position(|item| item.key == wallet_key)
        .unwrap_or_else(|| {
            engine.wallets.push(WalletSummary {
                key: wallet_key.clone(),
                label: updated_shared.label.clone(),
                public_key: updated_shared.public_key.clone(),
                enabled: true,
                emoji: String::new(),
            });
            engine.wallets.len() - 1
        });
    let mut wallet = engine.wallets[index].clone();
    wallet.label = updated_shared.label;
    wallet.public_key = updated_shared.public_key;
    if let Some(enabled) = request.enabled {
        wallet.enabled = enabled;
    }
    if let Some(emoji) = request.emoji {
        wallet.emoji = emoji.trim().to_string();
    }
    engine.wallets[index] = wallet.clone();
    normalize_wallet_groups(&mut engine.wallet_groups);
    persist_engine_state(&state.state_path, &engine)?;
    invalidate_wallet_balance_cache(&[wallet.key.clone()]);
    state
        .balance_stream
        .resync_wallets(runtime_wallets_from_engine(&engine));
    Ok(Json(wallet))
}

async fn delete_wallet(
    State(state): State<AppState>,
    Path(wallet_key): Path<String>,
) -> Result<Json<DeleteResourceResponse>, (StatusCode, String)> {
    let mut engine = state.engine.write().await;
    shared_config_manager()
        .delete_wallet(&wallet_key)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    engine.wallets.retain(|wallet| wallet.key != wallet_key);
    for group in &mut engine.wallet_groups {
        group.wallet_keys.retain(|key| key != &wallet_key);
    }
    normalize_wallet_groups(&mut engine.wallet_groups);
    persist_engine_state(&state.state_path, &engine)?;
    invalidate_wallet_balance_cache(&[wallet_key.clone()]);
    state
        .balance_stream
        .resync_wallets(runtime_wallets_from_engine(&engine));
    Ok(Json(DeleteResourceResponse {
        deleted: true,
        resource_id: wallet_key,
    }))
}

async fn reorder_wallets(
    State(state): State<AppState>,
    Json(request): Json<ReorderWalletsRequest>,
) -> Result<Json<Vec<WalletSummary>>, (StatusCode, String)> {
    let mut engine = state.engine.write().await;
    let existing_keys: HashSet<String> = engine
        .wallets
        .iter()
        .map(|wallet| wallet.key.clone())
        .collect();
    let mut desired_order: Vec<String> = Vec::with_capacity(request.wallet_keys.len());
    let mut seen = HashSet::new();
    for key in request.wallet_keys.into_iter() {
        let trimmed = key.trim().to_string();
        if trimmed.is_empty() || !existing_keys.contains(&trimmed) {
            continue;
        }
        if seen.insert(trimmed.clone()) {
            desired_order.push(trimmed);
        }
    }
    let mut reordered: Vec<WalletSummary> = Vec::with_capacity(engine.wallets.len());
    let mut remaining: Vec<WalletSummary> = engine.wallets.clone();
    for key in &desired_order {
        if let Some(position) = remaining.iter().position(|wallet| &wallet.key == key) {
            reordered.push(remaining.remove(position));
        }
    }
    reordered.extend(remaining.into_iter());
    engine.wallets = reordered;
    persist_engine_state(&state.state_path, &engine)?;
    state
        .balance_stream
        .resync_wallets(runtime_wallets_from_engine(&engine));
    Ok(Json(build_effective_wallets(&engine)))
}

async fn list_wallet_groups(State(state): State<AppState>) -> Json<Vec<WalletGroupSummary>> {
    let engine = state.engine.read().await.clone();
    Json(build_effective_wallet_groups(&engine))
}

async fn get_wallet_group(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> Result<Json<WalletGroupSummary>, (StatusCode, String)> {
    let engine = state.engine.read().await.clone();
    let group = build_effective_wallet_groups(&engine)
        .iter()
        .find(|group| group.id == group_id)
        .cloned()
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("unknown wallet group id: {group_id}"),
        ))?;
    Ok(Json(group))
}

async fn create_wallet_group(
    State(state): State<AppState>,
    Json(group): Json<WalletGroupSummary>,
) -> Result<Json<WalletGroupSummary>, (StatusCode, String)> {
    let mut engine = state.engine.write().await;
    let effective_wallets = build_effective_wallets(&engine);
    let group = normalize_wallet_group(group, &effective_wallets)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    if engine.wallet_groups.iter().any(|item| item.id == group.id) {
        return Err((
            StatusCode::CONFLICT,
            format!("wallet group {} already exists", group.id),
        ));
    }
    engine.wallet_groups.push(group.clone());
    normalize_wallet_groups(&mut engine.wallet_groups);
    persist_engine_state(&state.state_path, &engine)?;
    Ok(Json(group))
}

async fn update_wallet_group(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(group): Json<WalletGroupSummary>,
) -> Result<Json<WalletGroupSummary>, (StatusCode, String)> {
    let mut engine = state.engine.write().await;
    let effective_wallets = build_effective_wallets(&engine);
    let group = normalize_wallet_group(group, &effective_wallets)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let Some(index) = engine
        .wallet_groups
        .iter()
        .position(|item| item.id == group_id)
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("unknown wallet group id: {group_id}"),
        ));
    };
    if group.id != group_id && engine.wallet_groups.iter().any(|item| item.id == group.id) {
        return Err((
            StatusCode::CONFLICT,
            format!("wallet group {} already exists", group.id),
        ));
    }
    engine.wallet_groups[index] = group.clone();
    normalize_wallet_groups(&mut engine.wallet_groups);
    persist_engine_state(&state.state_path, &engine)?;
    Ok(Json(group))
}

async fn delete_wallet_group(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> Result<Json<DeleteResourceResponse>, (StatusCode, String)> {
    let mut engine = state.engine.write().await;
    let original_len = engine.wallet_groups.len();
    engine.wallet_groups.retain(|group| group.id != group_id);
    if engine.wallet_groups.len() == original_len {
        return Err((
            StatusCode::NOT_FOUND,
            format!("unknown wallet group id: {group_id}"),
        ));
    }
    persist_engine_state(&state.state_path, &engine)?;
    Ok(Json(DeleteResourceResponse {
        deleted: true,
        resource_id: group_id,
    }))
}

async fn resolve_token(
    Json(request): Json<ResolveTokenRequest>,
) -> Result<Json<TokenContextResponse>, (StatusCode, String)> {
    let resolved = resolve_token_request(&request)?;
    let raw_address = resolved.raw_address.clone();
    let companion_pair = route_companion_pair(request.pair.as_deref(), None)?;
    let probe_request = build_route_probe_request(
        raw_address.clone(),
        Some(
            match request.platform {
                Platform::Axiom => "axiom",
                Platform::J7 => "j7",
            }
            .to_string(),
        ),
        companion_pair,
    );
    let route_descriptor = match crate::trade_dispatch::resolve_trade_plan(&probe_request).await {
        Ok(plan) => crate::trade_dispatch::RouteDescriptor::from_dispatch_plan(&plan),
        Err(error) => {
            if is_resolve_token_route_error(&error) {
                return Err((StatusCode::BAD_REQUEST, error));
            }
            if let Ok(Some(descriptor)) = crate::trade_dispatch::classify_route_input(
                &configured_rpc_url(),
                &raw_address,
                "confirmed",
            )
            .await
            {
                descriptor
            } else {
                crate::trade_dispatch::RouteDescriptor {
                    raw_address: raw_address.clone(),
                    resolved_input_kind: crate::trade_dispatch::TradeInputKind::Mint,
                    resolved_mint: raw_address.clone(),
                    resolved_pair: None,
                    route_locked_pair: None,
                    family: None,
                    lifecycle: None,
                    quote_asset: None,
                    canonical_market_key: None,
                    non_canonical: false,
                }
            }
        }
    };
    let (family, lifecycle, quote_asset, canonical_market_key) =
        route_descriptor_labels(&route_descriptor);
    let resolved_mint = route_descriptor.resolved_mint.clone();
    Ok(Json(TokenContextResponse {
        platform: resolved.platform,
        surface: resolved.origin_surface.clone(),
        origin_surface: resolved.origin_surface,
        canonical_surface: resolved.canonical_surface,
        source: request.source,
        source_url: resolved.source_url,
        mint: resolved_mint.clone(),
        raw_address: Some(raw_address.clone()),
        pair_address: route_descriptor_pair_address(&route_descriptor),
        input_kind: Some(route_descriptor.resolved_input_kind.label().to_string()),
        non_canonical: route_descriptor.non_canonical,
        family,
        lifecycle,
        quote_asset,
        canonical_market_key,
        symbol: short_symbol(&resolved_mint),
        name: format!("Token {}", short_symbol(&resolved_mint)),
        image_url: None,
    }))
}

fn runtime_buy_funding_policy_label(policy: BuyFundingPolicy) -> &'static str {
    match policy {
        BuyFundingPolicy::SolOnly => "sol_only",
        BuyFundingPolicy::PreferUsd1ElseTopUp => "prefer_usd1_else_top_up",
        BuyFundingPolicy::Usd1Only => "usd1_only",
    }
}

fn runtime_sell_settlement_asset_label(asset: TradeSettlementAsset) -> &'static str {
    match asset {
        TradeSettlementAsset::Sol => "sol",
        TradeSettlementAsset::Usd1 => "usd1",
    }
}

fn runtime_sell_settlement_policy_label(policy: SellSettlementPolicy) -> &'static str {
    match policy {
        SellSettlementPolicy::AlwaysToSol => "always_to_sol",
        SellSettlementPolicy::AlwaysToUsd1 => "always_to_usd1",
        SellSettlementPolicy::MatchStoredEntryPreference => "match_stored_entry_preference",
    }
}

fn runtime_route_policy_label(side: &TradeSide, policy: &RuntimeExecutionPolicy) -> String {
    let fee_bps = crate::rollout::wrapper_default_fee_bps();
    match side {
        TradeSide::Buy => format!(
            "buy:{}:wrapper_fee_bps={}:conversion={}",
            runtime_buy_funding_policy_label(policy.buy_funding_policy),
            fee_bps,
            runtime_route_policy_conversion_label(side, policy)
        ),
        TradeSide::Sell => format!(
            "sell:{}:wrapper_fee_bps={}:conversion={}",
            runtime_sell_settlement_asset_label(policy.sell_settlement_asset),
            fee_bps,
            runtime_route_policy_conversion_label(side, policy)
        ),
    }
}

fn runtime_route_policy_conversion_label(
    side: &TradeSide,
    policy: &RuntimeExecutionPolicy,
) -> &'static str {
    match side {
        TradeSide::Buy if policy.buy_funding_policy != BuyFundingPolicy::SolOnly => "to_sol_in",
        TradeSide::Sell if policy.sell_settlement_asset != TradeSettlementAsset::Sol => {
            "to_sol_out"
        }
        _ => "none",
    }
}

/// `POST /api/extension/prewarm`
///
/// Intent-driven per-mint prewarm. Called by the extension on token-page
/// mount, panel open, and debounced hover on actionable controls. The
/// handler:
///
/// 1. Counts as operator activity so the continuous transport warm loop
///    stays awake during the session.
/// 2. Normalizes the input — if the caller sent a pair address, we
///    classify it and resolve to the real mint.
/// 3. Runs the planner once and caches the resolved selector in the
///    per-mint warm cache so subsequent buy/sell clicks skip venue
///    discovery.
/// 4. Returns the warm key plus a compact transport-warm status.
///
/// Soft-fails: a prewarm that can't resolve a venue still returns
/// `ok: false` with a short warning rather than an HTTP error, because
/// losing a prewarm should never break the click path.
async fn prewarm_mint(
    State(state): State<AppState>,
    Json(request): Json<PrewarmRequest>,
) -> Result<Json<PrewarmResponse>, (StatusCode, String)> {
    use crate::mint_warm_cache::{
        build_fingerprint, prewarmed_from_plan, shared_mint_warm_cache, venue_family_label,
        warm_ttl_ms_for_lifecycle, warm_ttl_ms_for_lifecycle_label,
    };
    use crate::trade_runtime::{RuntimeExecutionPolicy, TradeRuntimeRequest};

    state.tick_trade_activity();

    let engine = state.engine.read().await.clone();
    let rpc_url = configured_rpc_url();
    let commitment = engine.settings.execution_commitment.clone();
    let allow_non_canonical = engine.settings.allow_non_canonical_pool_trades;

    let preferred_input = normalize_route_address(request.address.as_deref())?;
    let companion_pair = route_companion_pair(request.pair.as_deref(), None)?;

    let mut warnings = Vec::new();
    let warm_enabled = true;
    let buy_funding_policy = request
        .buy_funding_policy
        .unwrap_or(engine.settings.default_buy_funding_policy);
    let sell_settlement_policy = request
        .sell_settlement_policy
        .unwrap_or(engine.settings.default_sell_settlement_policy);

    // Construct a throwaway TradeRuntimeRequest for the planner. We
    // only need enough policy shape to reach `resolve_trade_plan`.
    let preset_defaults = RuntimeExecutionPolicy {
        slippage_percent: engine.settings.default_buy_slippage_percent.clone(),
        mev_mode: engine.settings.default_buy_mev_mode.clone(),
        auto_tip_enabled: false,
        fee_sol: String::new(),
        tip_sol: String::new(),
        provider: engine.settings.execution_provider.clone(),
        endpoint_profile: engine.settings.execution_endpoint_profile.clone(),
        commitment: commitment.clone(),
        skip_preflight: engine.settings.execution_skip_preflight,
        track_send_block_height: engine.settings.track_send_block_height,
        buy_funding_policy,
        sell_settlement_policy,
        sell_settlement_asset: resolve_sell_settlement_asset(sell_settlement_policy, None),
    };
    let runtime_request = TradeRuntimeRequest {
        side: TradeSide::Buy,
        mint: preferred_input.clone(),
        buy_amount_sol: None,
        sell_intent: None,
        policy: preset_defaults.clone(),
        platform_label: request.platform.clone(),
        planned_route: None,
        planned_trade: None,
        pinned_pool: companion_pair.clone(),
        warm_key: None,
    };
    let (runtime_request, _) = normalize_runtime_request_for_wrapper_trade(&runtime_request);
    let prewarm_route_policy = runtime_route_policy_label(&TradeSide::Buy, &runtime_request.policy);
    // Compute the flight fingerprint up front so the planner's
    // TradeRuntimeRequest can round-trip the same `warm_key` that the
    // eventual click will carry, keeping log lines / metrics labels
    // consistent between the prewarm planner pass and the later trade.
    let flight_fingerprint = build_fingerprint(
        &preferred_input,
        companion_pair.as_deref(),
        &rpc_url,
        &commitment,
        &prewarm_route_policy,
        allow_non_canonical,
    );

    // Single-flight dedupe. Three near-simultaneous prewarm calls for
    // the same mint (hover, panel-open, token-detail mount all firing
    // within tens of ms of each other) should produce one planner run,
    // not three. We acquire a per-fingerprint mutex keyed by the
    // *input* shape (not the resolved mint) so concurrent calls for
    // the same pair-address all wait on one planner.
    let flight_mutex = shared_mint_warm_cache()
        .flight_lock(&flight_fingerprint)
        .await;
    let _flight_guard = flight_mutex.lock().await;

    // Under the lock, re-check the cache: a concurrent prewarm that
    // was ahead of us may have populated the entry while we were
    // waiting. When it has, skip the planner entirely.
    let plan_result = if warm_enabled {
        if let Some(existing) = shared_mint_warm_cache().current(&flight_fingerprint).await {
            if let Some(cached_plan) = existing.plan.clone() {
                Ok(crate::trade_dispatch::TradeDispatchPlan {
                    adapter: crate::trade_dispatch::adapter_for_selector(&cached_plan.selector)
                        .unwrap_or(crate::trade_dispatch::TradeAdapter::PumpNative),
                    selector: cached_plan.selector,
                    execution_backend: crate::rollout::preferred_execution_backend(),
                    raw_address: preferred_input.clone(),
                    resolved_input_kind: if existing
                        .resolved_pair
                        .as_deref()
                        .is_some_and(|pair| pair == preferred_input)
                    {
                        crate::trade_dispatch::TradeInputKind::Pair
                    } else {
                        crate::trade_dispatch::TradeInputKind::Mint
                    },
                    resolved_mint: existing.mint.clone(),
                    resolved_pinned_pool: cached_plan
                        .resolved_pinned_pool
                        .clone()
                        .or_else(|| existing.resolved_pair.clone()),
                    non_canonical: cached_plan.non_canonical,
                })
            } else {
                plan_trade_request_to_dispatch(&runtime_request).await
            }
        } else {
            plan_trade_request_to_dispatch(&runtime_request).await
        }
    } else {
        Err("warm path disabled for this family".to_string())
    };

    let (
        resolved_mint,
        resolved_pair,
        family_label,
        lifecycle_label,
        quote_asset_label,
        canonical_market_key,
        input_kind,
        non_canonical,
        plan_for_cache,
    ) = match plan_result {
        Ok(plan) => {
            let route_descriptor =
                crate::trade_dispatch::RouteDescriptor::from_dispatch_plan(&plan);
            let (family, lifecycle, quote_asset, canonical_market_key) =
                route_descriptor_labels(&route_descriptor);
            let mint_for_cache = plan.resolved_mint.clone();
            let pair_for_cache = plan.resolved_pinned_pool.clone();
            (
                mint_for_cache,
                pair_for_cache,
                family,
                lifecycle,
                quote_asset,
                canonical_market_key,
                Some(plan.resolved_input_kind.label().to_string()),
                plan.non_canonical,
                Some(plan),
            )
        }
        Err(error) => {
            warnings.push(format!("prewarm planning skipped: {error}"));
            let classified = crate::trade_dispatch::classify_route_input(
                &rpc_url,
                &preferred_input,
                &commitment,
            )
            .await
            .ok()
            .flatten();
            let classified_pair = classified.as_ref().and_then(route_descriptor_pair_address);
            let (family, lifecycle, quote_asset, canonical_market_key) = classified
                .as_ref()
                .map(route_descriptor_labels)
                .unwrap_or((None, None, None, None));
            // Even if planning failed we still return an
            // acknowledgement so the extension's wiring stays
            // exercised and a later trade will fall through to
            // the cold path.
            (
                classified
                    .as_ref()
                    .map(|descriptor| descriptor.resolved_mint.clone())
                    .unwrap_or_else(|| preferred_input.clone()),
                classified_pair,
                family,
                lifecycle,
                quote_asset,
                canonical_market_key,
                classified
                    .as_ref()
                    .map(|descriptor| descriptor.resolved_input_kind.label().to_string()),
                classified
                    .as_ref()
                    .is_some_and(|descriptor| descriptor.non_canonical),
                None,
            )
        }
    };
    let response_resolved_pair = plan_for_cache
        .as_ref()
        .map(crate::trade_dispatch::RouteDescriptor::from_dispatch_plan)
        .as_ref()
        .and_then(route_descriptor_pair_address)
        .or_else(|| resolved_pair.clone());

    let fingerprint = build_fingerprint(
        &resolved_mint,
        resolved_pair.as_deref(),
        &rpc_url,
        &commitment,
        &prewarm_route_policy,
        allow_non_canonical,
    );
    let buy_warm_key = fingerprint.as_warm_key();
    let stale_after_ms = plan_for_cache
        .as_ref()
        .map(|plan| warm_ttl_ms_for_lifecycle(Some(&plan.selector.lifecycle)))
        .unwrap_or_else(|| warm_ttl_ms_for_lifecycle_label(lifecycle_label.as_deref()));

    if let Some(plan) = plan_for_cache.as_ref() {
        let entry = prewarmed_from_plan(&fingerprint, resolved_pair.clone(), plan);
        // Label is threaded through for future metrics wiring.
        let _ = venue_family_label(&entry.venue);
        shared_mint_warm_cache().insert(fingerprint, entry).await;
        if matches!(
            plan.selector.family,
            crate::trade_planner::TradeVenueFamily::MeteoraDbc
                | crate::trade_planner::TradeVenueFamily::MeteoraDammV2
        ) {
            let mut snapshot_request = runtime_request.clone();
            snapshot_request.mint = plan.resolved_mint.clone();
            snapshot_request.pinned_pool = plan
                .resolved_pinned_pool
                .clone()
                .or_else(|| resolved_pair.clone());
            snapshot_request.planned_trade = Some(plan.selector.clone());
            snapshot_request.warm_key = Some(buy_warm_key.clone());
            if let Err(error) = crate::meteora_native::warm_meteora_compile_snapshot(
                &plan.selector,
                &snapshot_request,
                &buy_warm_key,
            )
            .await
            {
                warnings.push(format!("meteora compile snapshot skipped: {error}"));
            }
        }
    }

    let sell_assets_to_warm = match sell_settlement_policy {
        SellSettlementPolicy::AlwaysToSol => vec![TradeSettlementAsset::Sol],
        SellSettlementPolicy::AlwaysToUsd1 => vec![TradeSettlementAsset::Usd1],
        SellSettlementPolicy::MatchStoredEntryPreference => {
            vec![TradeSettlementAsset::Sol, TradeSettlementAsset::Usd1]
        }
    };
    let mut sell_warm_key = None;
    let sell_assets_to_warm_count = sell_assets_to_warm.len();
    let mut sell_assets_warmed = 0usize;
    if warm_enabled && !matches!(request.side, Some(TradeSide::Buy)) {
        for sell_asset in sell_assets_to_warm {
            let should_return_sell_warm_key = !matches!(
                sell_settlement_policy,
                SellSettlementPolicy::MatchStoredEntryPreference
            );
            let mut sell_policy = preset_defaults.clone();
            sell_policy.sell_settlement_asset = sell_asset;
            let sell_runtime_request = TradeRuntimeRequest {
                side: TradeSide::Sell,
                mint: preferred_input.clone(),
                buy_amount_sol: None,
                sell_intent: None,
                policy: sell_policy,
                platform_label: request.platform.clone(),
                planned_route: None,
                planned_trade: None,
                pinned_pool: companion_pair.clone(),
                warm_key: None,
            };
            let (sell_runtime_request, _) =
                normalize_runtime_request_for_wrapper_trade(&sell_runtime_request);
            let sell_route_policy =
                runtime_route_policy_label(&TradeSide::Sell, &sell_runtime_request.policy);
            match plan_trade_request_to_dispatch(&sell_runtime_request).await {
                Ok(plan) => {
                    sell_assets_warmed += 1;
                    let sell_resolved_pair = plan
                        .resolved_pinned_pool
                        .clone()
                        .or_else(|| resolved_pair.clone());
                    let sell_fingerprint = build_fingerprint(
                        &plan.resolved_mint,
                        sell_resolved_pair.as_deref(),
                        &rpc_url,
                        &commitment,
                        &sell_route_policy,
                        allow_non_canonical,
                    );
                    if should_return_sell_warm_key {
                        sell_warm_key.get_or_insert_with(|| sell_fingerprint.as_warm_key());
                    }
                    let entry = prewarmed_from_plan(&sell_fingerprint, sell_resolved_pair, &plan);
                    let _ = venue_family_label(&entry.venue);
                    shared_mint_warm_cache()
                        .insert(sell_fingerprint, entry)
                        .await;
                }
                Err(error) => warnings.push(format!(
                    "sell prewarm planning skipped for {} settlement: {error}",
                    runtime_sell_settlement_asset_label(sell_asset)
                )),
            }
        }
    }
    let sell_warmed =
        sell_assets_to_warm_count > 0 && sell_assets_warmed == sell_assets_to_warm_count;
    crate::warm_metrics::shared_warm_metrics().record_prewarm_request(plan_for_cache.is_some());

    let transport_warm = warm_runtime_payload(&state.launchdeck_warm);
    let family_enabled = serde_json::json!({
        "pump": crate::rollout::family_warm_enabled_by_label("pump"),
        "raydiumAmmV4": crate::rollout::family_warm_enabled_by_label("raydium-amm-v4"),
        "raydiumCpmm": crate::rollout::family_warm_enabled_by_label("raydium-cpmm"),
        "raydiumLaunchLab": crate::rollout::family_warm_enabled_by_label("raydium-launchlab"),
        "bonk": crate::rollout::family_warm_enabled_by_label("bonk"),
        "bags": crate::rollout::family_warm_enabled_by_label("bags"),
    });

    Ok(Json(PrewarmResponse {
        ok: plan_for_cache.is_some(),
        warm_key: buy_warm_key.clone(),
        buy_warm_key: Some(buy_warm_key),
        sell_warm_key,
        buy_warmed: plan_for_cache.is_some(),
        sell_warmed,
        resolved_mint,
        resolved_pair: response_resolved_pair,
        raw_address: Some(preferred_input),
        input_kind,
        non_canonical,
        family: family_label,
        lifecycle: lifecycle_label,
        quote_asset: quote_asset_label,
        canonical_market_key,
        stale_after_ms,
        transport_warm,
        family_enabled,
        warnings,
    }))
}

async fn set_trade_readiness(
    State(state): State<AppState>,
    Json(request): Json<TradeReadinessRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let _surface = request.surface.as_deref().unwrap_or_default().trim();
    if request.active {
        state.tick_trade_activity();
    }
    Ok(Json(json!({ "ok": true, "active": request.active })))
}

/// Thin adapter: run the planner and return a full `TradeDispatchPlan`.
/// `plan_trade_request` in `trade_runtime` only exposes the selector, but
/// for prewarm we want the whole dispatch plan so we can cache adapter
/// + execution backend alongside it.
async fn plan_trade_request_to_dispatch(
    request: &TradeRuntimeRequest,
) -> Result<crate::trade_dispatch::TradeDispatchPlan, String> {
    crate::trade_dispatch::resolve_trade_plan(request).await
}

async fn preview_batch(
    State(state): State<AppState>,
    Json(request): Json<BatchPreviewRequest>,
) -> Result<Json<BatchPreviewResponse>, (StatusCode, String)> {
    state.tick_trade_activity();
    let engine = state.engine.read().await.clone();
    let trade_ledger = state.trade_ledger.read().await.clone();
    let preview_side = request.side.clone();
    let preview_address = normalize_route_address(request.address.as_deref())?;
    let preview_pinned_pool =
        route_companion_pair(request.pair.as_deref(), request.pinned_pool.as_deref())?;
    let wallets = build_effective_wallets(&engine);
    let wallet_groups = build_effective_wallet_groups(&engine);
    let target = resolve_batch_target(
        &wallets,
        &wallet_groups,
        request.wallet_key,
        request.wallet_keys,
        request.wallet_group_id,
    )?;
    let preset = resolve_preset(&engine.presets, &request.preset_id)?;
    let policy = match request.side {
        TradeSide::Buy => resolve_buy_policy(
            &engine.settings,
            engine.config.as_ref().unwrap_or(&Value::Null),
            preset,
            request.buy_amount_sol.as_deref(),
            request.buy_funding_policy,
        ),
        TradeSide::Sell => resolve_sell_policy(
            &engine.settings,
            engine.config.as_ref().unwrap_or(&Value::Null),
            preset,
            request.sell_settlement_policy,
        ),
    };

    let mut warnings = Vec::new();
    warnings.extend(policy.auto_fee_warnings.iter().cloned());
    if target.wallet_count > 1 {
        warnings
            .push("batch execution fans out to one independent transaction per wallet".to_string());
    }
    let preview_sell_intent = match &preview_side {
        TradeSide::Buy => None,
        TradeSide::Sell => Some(resolve_sell_intent(
            request.sell_percent.as_deref(),
            request.sell_output_sol.as_deref(),
            policy.sell_percent.as_deref(),
        )?),
    };
    let preview_buy_amount_sol = request
        .buy_amount_sol
        .clone()
        .or(policy.buy_amount_sol.clone());
    let preview_request = TradeRuntimeRequest {
        side: preview_side.clone(),
        mint: preview_address.clone(),
        buy_amount_sol: preview_buy_amount_sol.clone(),
        sell_intent: preview_sell_intent.clone().map(|intent| match intent {
            SellIntent::Percent(value) => RuntimeSellIntent::Percent(value),
            SellIntent::SolOutput(value) => RuntimeSellIntent::SolOutput(value),
        }),
        policy: RuntimeExecutionPolicy {
            slippage_percent: policy.slippage_percent.clone(),
            mev_mode: policy.mev_mode.clone(),
            auto_tip_enabled: policy.auto_tip_enabled,
            fee_sol: policy.fee_sol.clone(),
            tip_sol: policy.tip_sol.clone(),
            provider: policy.provider.clone(),
            endpoint_profile: policy.endpoint_profile.clone(),
            commitment: policy.commitment.clone(),
            skip_preflight: policy.skip_preflight,
            track_send_block_height: policy.track_send_block_height,
            buy_funding_policy: policy.buy_funding_policy,
            sell_settlement_policy: policy.sell_settlement_policy,
            sell_settlement_asset: policy.sell_settlement_asset,
        },
        platform_label: request.platform.clone(),
        planned_route: None,
        planned_trade: None,
        pinned_pool: preview_pinned_pool.clone(),
        warm_key: request.warm_key.clone(),
    };
    let (preview_wrapper_request, preview_route_conversion) =
        normalize_preview_request_for_wrapper_trade(&preview_request);
    let planned_dispatch = match resolve_trade_plan(&preview_wrapper_request).await {
        Ok(plan) => Some(plan),
        Err(error) => {
            warnings.push(error);
            None
        }
    };
    let planned_execution = planned_dispatch.as_ref().map(|plan| plan.selector.clone());
    let execution_adapter = planned_dispatch
        .as_ref()
        .map(|plan| plan.adapter.label().to_string());
    let mut execution_backend = planned_dispatch.as_ref().map(|_| "wrapper".to_string());
    // When the venue family is disabled by the rollout guard we surface
    // the warning **and** mark the preview as disallowed, so the panel
    // disables the trade button rather than rendering a preview that a
    // subsequent buy/sell would reject with HTTP 400 anyway.
    let mut preview_allowed = true;
    if let Some(selector) = planned_execution.as_ref() {
        if let Some(intent) = preview_sell_intent.as_ref() {
            validate_sell_intent_for_family(intent, selector)?;
        }
        if let Some(warning) = family_guard_warning(&selector.family) {
            warnings.push(warning);
            if !family_execution_enabled(&selector.family) {
                preview_allowed = false;
            }
        }
    } else {
        preview_allowed = false;
    }
    let wallet_request = WalletTradeRequest {
        side: preview_side.clone(),
        mint: planned_dispatch
            .as_ref()
            .map(|plan| plan.resolved_mint.clone())
            .unwrap_or_else(|| preview_address.clone()),
        platform_label: request.platform.clone(),
        buy_amount_sol: preview_buy_amount_sol.clone(),
        sell_intent: preview_sell_intent.clone(),
        policy: ExecutionPolicy {
            slippage_percent: policy.slippage_percent.clone(),
            mev_mode: policy.mev_mode.clone(),
            auto_tip_enabled: policy.auto_tip_enabled,
            fee_sol: policy.fee_sol.clone(),
            tip_sol: policy.tip_sol.clone(),
            provider: policy.provider.clone(),
            endpoint_profile: policy.endpoint_profile.clone(),
            commitment: policy.commitment.clone(),
            skip_preflight: policy.skip_preflight,
            track_send_block_height: policy.track_send_block_height,
            buy_funding_policy: preview_wrapper_request.policy.buy_funding_policy,
            sell_settlement_policy: preview_wrapper_request.policy.sell_settlement_policy,
            sell_settlement_asset: preview_wrapper_request.policy.sell_settlement_asset,
        },
        planned_route: planned_dispatch.clone(),
        planned_trade: planned_execution.clone(),
        pinned_pool: planned_dispatch
            .as_ref()
            .and_then(|plan| plan.resolved_pinned_pool.clone())
            .or_else(|| preview_pinned_pool.clone()),
        warm_key: request.warm_key.clone(),
    };
    let execution_plan = if matches!(preview_side, TradeSide::Buy) {
        build_buy_batch_execution_plan(
            &engine,
            &target,
            &wallet_request.mint,
            &wallet_request,
            &trade_ledger,
            &build_buy_planning_seed(
                &request.preset_id,
                &wallet_request.mint,
                &target,
                preview_buy_amount_sol.as_deref(),
            ),
            true,
        )
        .await?
    } else {
        build_default_batch_execution_plan(&target, &wallet_request, &trade_ledger)
    };
    let preview_probe_errors = run_preview_compile_probes(&execution_plan.wallets).await;
    if !preview_probe_errors.is_empty() {
        warnings.extend(preview_probe_errors);
        preview_allowed = false;
    }

    // Fee preview: for every SOL-touching wallet in the plan,
    // decorate the summary so the UI can render the wrapper fee amount
    // separately from the provider fee and tip. SOL-in buys have a
    // known gross input (the planned buy amount), so we surface a
    // lamport-floor estimate in SOL. SOL-out sells do not, so we
    // surface the bps tier and route kind but leave the amount absent.
    let wrapper_route_classification = planned_execution.as_ref().map(|selector| {
        crate::wrapper_payload::classify_trade_route(
            selector,
            &wallet_request_to_runtime_request(
                &wallet_request,
                wallet_request.planned_route.clone(),
                wallet_request.planned_trade.clone(),
            ),
        )
    });
    let planned_native_no_sol = planned_execution.as_ref().is_some_and(|selector| {
        matches!(
            selector.family,
            crate::trade_planner::TradeVenueFamily::TrustedStableSwap
        )
    });
    if matches!(
        wrapper_route_classification,
        Some(crate::wrapper_payload::WrapperRouteClassification::NoSol)
    ) {
        if planned_native_no_sol {
            execution_backend = planned_dispatch.as_ref().map(|_| "native".to_string());
        } else {
            warnings.push(
                "Selected route cannot be routed through the Trench wrapper; choose a SOL/WSOL-settled route."
                    .to_string(),
            );
            preview_allowed = false;
        }
    }
    let wrapper_fee_bps = crate::rollout::wrapper_default_fee_bps();
    let execution_plan_with_fees = execution_plan
        .wallets
        .into_iter()
        .map(|entry| {
            let mut summary = entry.planned_summary;
            if let Some(route) = wrapper_route_classification {
                if route.touches_sol() {
                    summary.wrapper_fee_bps = wrapper_fee_bps;
                    summary.wrapper_route = Some(if preview_route_conversion {
                        format!("{}:conversion", route.label())
                    } else {
                        route.label().to_string()
                    });
                }
                if matches!(
                    route,
                    crate::wrapper_payload::WrapperRouteClassification::SolIn
                ) {
                    let gross_sol_str = summary
                        .buy_amount_sol
                        .as_deref()
                        .or(preview_buy_amount_sol.as_deref());
                    if let Some(gross) = gross_sol_str {
                        let gross_lamports = parse_sol_to_lamports(gross).unwrap_or(0);
                        let fee_lamports = crate::wrapper_compile::estimate_sol_in_fee_lamports(
                            gross_lamports,
                            wrapper_fee_bps,
                        );
                        if fee_lamports > 0 {
                            summary.wrapper_fee_sol =
                                Some(format_lamports_to_sol_string(fee_lamports));
                        } else {
                            // Still emit the "0 SOL" line so the UI
                            // can show the tier explicitly even when
                            // the user selected 0%.
                            summary.wrapper_fee_sol = Some("0".to_string());
                        }
                    }
                }
            }
            summary
        })
        .collect::<Vec<_>>();

    Ok(Json(BatchPreviewResponse {
        allowed: preview_allowed,
        side: preview_side,
        target,
        policy: PreviewTradePolicy {
            slippage_percent: policy.slippage_percent,
            mev_mode: policy.mev_mode,
            auto_tip_enabled: policy.auto_tip_enabled,
            fee_sol: policy.fee_sol,
            tip_sol: policy.tip_sol,
            buy_amount_sol: preview_buy_amount_sol,
            sell_percent: request.sell_percent.or(policy.sell_percent),
            buy_funding_policy: policy.buy_funding_policy,
            sell_settlement_policy: policy.sell_settlement_policy,
        },
        batch_policy: execution_plan.batch_policy,
        execution_plan: execution_plan_with_fees,
        execution_adapter,
        execution_backend,
        planned_execution,
        warnings,
    }))
}

async fn buy(
    State(state): State<AppState>,
    Json(request): Json<BuyRequest>,
) -> Result<Json<ExecutionAcceptedResponse>, (StatusCode, String)> {
    state.tick_trade_activity();
    let planning_started = Instant::now();
    let client_click_started_at = request
        .client_request_started_at_unix_ms
        .or(request.client_started_at_unix_ms);
    let client_request_id = normalize_client_request_id(request.client_request_id)?;
    let engine = state.engine.read().await.clone();
    let trade_ledger = state.trade_ledger.read().await.clone();
    let wallets = build_effective_wallets(&engine);
    let wallet_groups = build_effective_wallet_groups(&engine);
    let target = resolve_batch_target(
        &wallets,
        &wallet_groups,
        request.wallet_key,
        request.wallet_keys,
        request.wallet_group_id,
    )?;
    let preset = resolve_preset(&engine.presets, &request.preset_id)?;
    let buy_amount_override = request.buy_amount_sol.clone();
    let policy = resolve_buy_policy(
        &engine.settings,
        engine.config.as_ref().unwrap_or(&Value::Null),
        preset,
        buy_amount_override.as_deref(),
        request.buy_funding_policy,
    );
    let address_input = normalize_route_address(request.address.as_deref())?;
    let companion_pair =
        route_companion_pair(request.pair.as_deref(), request.pinned_pool.as_deref())?;
    let batch_planning_mint = trimmed_option(&request.mint).unwrap_or(&address_input);
    let execution_backend = "runtime".to_string();
    let wallet_request = WalletTradeRequest {
        side: TradeSide::Buy,
        mint: address_input.clone(),
        platform_label: request.platform.clone(),
        buy_amount_sol: buy_amount_override
            .clone()
            .or(policy.buy_amount_sol.clone()),
        sell_intent: None,
        policy: ExecutionPolicy {
            slippage_percent: policy.slippage_percent.clone(),
            mev_mode: policy.mev_mode.clone(),
            auto_tip_enabled: policy.auto_tip_enabled,
            fee_sol: policy.fee_sol.clone(),
            tip_sol: policy.tip_sol.clone(),
            provider: policy.provider.clone(),
            endpoint_profile: policy.endpoint_profile.clone(),
            commitment: policy.commitment.clone(),
            skip_preflight: policy.skip_preflight,
            track_send_block_height: policy.track_send_block_height,
            buy_funding_policy: policy.buy_funding_policy,
            sell_settlement_policy: policy.sell_settlement_policy,
            sell_settlement_asset: policy.sell_settlement_asset,
        },
        planned_route: None,
        planned_trade: None,
        pinned_pool: companion_pair,
        warm_key: request.warm_key.clone(),
    };
    let (route_wallet_request, _) = normalize_wallet_request_for_wrapper_trade(&wallet_request);
    let execution_plan = build_buy_batch_execution_plan(
        &engine,
        &target,
        batch_planning_mint,
        &route_wallet_request,
        &trade_ledger,
        &build_buy_planning_seed(
            &request.preset_id,
            batch_planning_mint,
            &target,
            wallet_request.buy_amount_sol.as_deref(),
        ),
        false,
    )
    .await?;
    let fingerprint = build_trade_fingerprint(
        &TradeSide::Buy,
        &wallet_request.mint,
        &request.preset_id,
        &target,
        None,
        buy_amount_override.as_deref(),
        None,
        None,
        &policy,
        execution_plan.batch_policy.as_ref(),
        wallet_request.pinned_pool.as_deref(),
        request.warm_key.as_deref(),
        &execution_plan
            .wallets
            .iter()
            .map(|entry| entry.planned_summary.clone())
            .collect::<Vec<_>>(),
    );

    let accepted = enqueue_batch(
        &state,
        engine.settings.max_active_batches,
        ExecutionSubmission {
            client_request_id,
            fingerprint,
            side: TradeSide::Buy,
            target,
            execution_adapter: None,
            execution_backend,
            planned_execution: None,
            client_started_at_unix_ms: client_click_started_at,
            batch_policy: execution_plan.batch_policy,
            execution_plan: execution_plan.wallets,
            warnings: policy.auto_fee_warnings.clone(),
        },
    )
    .await?;
    if let Some(client_started_at) = client_click_started_at {
        eprintln!(
            "[execution-engine][latency] batch={} phase=enqueue-accepted side=buy planning_ms={} click_to_enqueue_ms={}",
            accepted.batch_id,
            planning_started.elapsed().as_millis(),
            accepted
                .accepted_at_unix_ms
                .saturating_sub(client_started_at)
        );
    }
    Ok(Json(accepted))
}

async fn sell(
    State(state): State<AppState>,
    Json(request): Json<SellRequest>,
) -> Result<Json<ExecutionAcceptedResponse>, (StatusCode, String)> {
    state.tick_trade_activity();
    let planning_started = Instant::now();
    let client_click_started_at = request
        .client_request_started_at_unix_ms
        .or(request.client_started_at_unix_ms);
    let client_request_id = normalize_client_request_id(request.client_request_id)?;
    let engine = state.engine.read().await.clone();
    let trade_ledger = state.trade_ledger.read().await.clone();
    let wallets = build_effective_wallets(&engine);
    let wallet_groups = build_effective_wallet_groups(&engine);
    let target = resolve_batch_target(
        &wallets,
        &wallet_groups,
        request.wallet_key,
        request.wallet_keys,
        request.wallet_group_id,
    )?;
    let preset = resolve_preset(&engine.presets, &request.preset_id)?;
    let policy = resolve_sell_policy(
        &engine.settings,
        engine.config.as_ref().unwrap_or(&Value::Null),
        preset,
        request.sell_settlement_policy,
    );
    let address_input = normalize_route_address(request.address.as_deref())?;
    let companion_pair =
        route_companion_pair(request.pair.as_deref(), request.pinned_pool.as_deref())?;
    let sell_intent = resolve_sell_intent(
        request.sell_percent.as_deref(),
        request.sell_output_sol.as_deref(),
        policy.sell_percent.as_deref(),
    )?;
    let execution_backend = "runtime".to_string();
    let wallet_request = WalletTradeRequest {
        side: TradeSide::Sell,
        mint: address_input.clone(),
        platform_label: request.platform.clone(),
        buy_amount_sol: None,
        sell_intent: Some(sell_intent.clone()),
        policy: ExecutionPolicy {
            slippage_percent: policy.slippage_percent.clone(),
            mev_mode: policy.mev_mode.clone(),
            auto_tip_enabled: policy.auto_tip_enabled,
            fee_sol: policy.fee_sol.clone(),
            tip_sol: policy.tip_sol.clone(),
            provider: policy.provider.clone(),
            endpoint_profile: policy.endpoint_profile.clone(),
            commitment: policy.commitment.clone(),
            skip_preflight: policy.skip_preflight,
            track_send_block_height: policy.track_send_block_height,
            buy_funding_policy: policy.buy_funding_policy,
            sell_settlement_policy: policy.sell_settlement_policy,
            sell_settlement_asset: policy.sell_settlement_asset,
        },
        planned_route: None,
        planned_trade: None,
        pinned_pool: companion_pair,
        warm_key: request.warm_key.clone(),
    };
    let (route_wallet_request, _) = normalize_wallet_request_for_wrapper_trade(&wallet_request);
    let execution_plan =
        build_default_batch_execution_plan(&target, &route_wallet_request, &trade_ledger);
    let fingerprint = build_trade_fingerprint(
        &TradeSide::Sell,
        &wallet_request.mint,
        &request.preset_id,
        &target,
        None,
        None,
        request.sell_percent.as_deref(),
        request.sell_output_sol.as_deref(),
        &policy,
        execution_plan.batch_policy.as_ref(),
        wallet_request.pinned_pool.as_deref(),
        request.warm_key.as_deref(),
        &execution_plan
            .wallets
            .iter()
            .map(|entry| entry.planned_summary.clone())
            .collect::<Vec<_>>(),
    );

    let accepted = enqueue_batch(
        &state,
        engine.settings.max_active_batches,
        ExecutionSubmission {
            client_request_id,
            fingerprint,
            side: TradeSide::Sell,
            target,
            execution_adapter: None,
            execution_backend,
            planned_execution: None,
            client_started_at_unix_ms: client_click_started_at,
            batch_policy: execution_plan.batch_policy,
            execution_plan: execution_plan.wallets,
            warnings: policy.auto_fee_warnings.clone(),
        },
    )
    .await?;
    if let Some(client_started_at) = client_click_started_at {
        eprintln!(
            "[execution-engine][latency] batch={} phase=enqueue-accepted side=sell planning_ms={} click_to_enqueue_ms={}",
            accepted.batch_id,
            planning_started.elapsed().as_millis(),
            accepted
                .accepted_at_unix_ms
                .saturating_sub(client_started_at)
        );
    }
    Ok(Json(accepted))
}

async fn split_tokens(
    State(state): State<AppState>,
    Json(mut request): Json<TokenSplitRequest>,
) -> Result<Json<TokenDistributionResponse>, (StatusCode, String)> {
    let client_request_id =
        normalize_token_distribution_client_request_id(request.client_request_id.clone())?;
    request.client_request_id = Some(client_request_id.clone());
    let engine = state.engine.read().await.clone();
    let available_wallet_keys = build_effective_wallets(&engine)
        .into_iter()
        .filter(|wallet| wallet.enabled)
        .map(|wallet| wallet.key)
        .collect::<HashSet<_>>();
    request.wallet_keys =
        validate_distribution_wallet_keys(&request.wallet_keys, &available_wallet_keys)?;
    request.source_wallet_keys =
        validate_distribution_wallet_keys(&request.source_wallet_keys, &available_wallet_keys)?;
    let preset_id = token_distribution_preset_id(&engine, &request.preset_id)?;
    request.preset_id = preset_id.clone();
    let preset = resolve_preset(&engine.presets, &preset_id)?;
    let policy = resolve_buy_policy(
        &engine.settings,
        engine.config.as_ref().unwrap_or(&Value::Null),
        preset,
        None,
        None,
    );
    let config = TokenDistributionExecutionConfig {
        commitment: policy.commitment.clone(),
        skip_preflight: policy.skip_preflight,
        track_send_block_height: policy.track_send_block_height,
        provider: policy.provider.clone(),
        endpoint_profile: policy.endpoint_profile.clone(),
        mev_mode: mev_mode_label(&policy.mev_mode).to_string(),
        fee_sol: policy.fee_sol.clone(),
        tip_sol: policy.tip_sol.clone(),
        warnings: policy.auto_fee_warnings.clone(),
    };
    let fingerprint = build_token_distribution_fingerprint("split", &request)?;
    let (response, fresh) =
        execute_idempotent_token_distribution(&state, client_request_id, fingerprint, || async {
            execute_token_split(request, config).await
        })
        .await?;
    if fresh {
        apply_token_distribution_ledger(&state, &response).await;
    }
    Ok(Json(response))
}

async fn consolidate_tokens(
    State(state): State<AppState>,
    Json(mut request): Json<TokenConsolidateRequest>,
) -> Result<Json<TokenDistributionResponse>, (StatusCode, String)> {
    let client_request_id =
        normalize_token_distribution_client_request_id(request.client_request_id.clone())?;
    request.client_request_id = Some(client_request_id.clone());
    request.destination_wallet_key = request.destination_wallet_key.trim().to_string();
    let engine = state.engine.read().await.clone();
    let available_wallet_keys = build_effective_wallets(&engine)
        .into_iter()
        .filter(|wallet| wallet.enabled)
        .map(|wallet| wallet.key)
        .collect::<Vec<_>>();
    let available_wallet_set = available_wallet_keys
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    if !available_wallet_set.contains(&request.destination_wallet_key) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Selected destination wallet is not available.".to_string(),
        ));
    }
    let preset_id = token_distribution_preset_id(&engine, &request.preset_id)?;
    request.preset_id = preset_id.clone();
    let preset = resolve_preset(&engine.presets, &preset_id)?;
    let policy = resolve_buy_policy(
        &engine.settings,
        engine.config.as_ref().unwrap_or(&Value::Null),
        preset,
        None,
        None,
    );
    let config = TokenDistributionExecutionConfig {
        commitment: policy.commitment.clone(),
        skip_preflight: policy.skip_preflight,
        track_send_block_height: policy.track_send_block_height,
        provider: policy.provider.clone(),
        endpoint_profile: policy.endpoint_profile.clone(),
        mev_mode: mev_mode_label(&policy.mev_mode).to_string(),
        fee_sol: policy.fee_sol.clone(),
        tip_sol: policy.tip_sol.clone(),
        warnings: policy.auto_fee_warnings.clone(),
    };
    let fingerprint = build_token_distribution_fingerprint("consolidate", &request)?;
    let (response, fresh) =
        execute_idempotent_token_distribution(&state, client_request_id, fingerprint, || async {
            execute_token_consolidate(request, available_wallet_keys, config).await
        })
        .await?;
    if fresh {
        apply_token_distribution_ledger(&state, &response).await;
    }
    Ok(Json(response))
}

fn token_distribution_preset_id(
    engine: &StoredEngineState,
    requested_preset_id: &str,
) -> Result<String, (StatusCode, String)> {
    let requested = requested_preset_id.trim();
    if !requested.is_empty() {
        return Ok(requested.to_string());
    }
    engine
        .presets
        .first()
        .map(|preset| preset.id.clone())
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Token distribution requires an active preset.".to_string(),
        ))
}

fn mev_mode_label(mode: &MevMode) -> &'static str {
    match mode {
        MevMode::Off => "off",
        MevMode::Reduced => "reduced",
        MevMode::Secure => "secure",
    }
}

async fn rewards_summary(
    State(state): State<AppState>,
    Json(request): Json<RewardsSummaryRequest>,
) -> Result<Json<RewardsSummaryResponse>, (StatusCode, String)> {
    let engine = state.engine.read().await.clone();
    let requested_keys = request
        .wallet_keys
        .iter()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .collect::<HashSet<_>>();
    let wallets = build_effective_wallets(&engine)
        .into_iter()
        .filter(|wallet| wallet.enabled)
        .filter(|wallet| requested_keys.is_empty() || requested_keys.contains(&wallet.key))
        .map(|wallet| RewardWallet {
            key: wallet.key,
            label: wallet.label,
            public_key: wallet.public_key,
        })
        .collect::<Vec<_>>();
    let config = RewardsExecutionConfig {
        commitment: engine.settings.execution_commitment.clone(),
        skip_preflight: engine.settings.execution_skip_preflight,
        track_send_block_height: config_track_send_block_height(
            engine.config.as_ref().unwrap_or(&Value::Null),
        ),
    };
    Ok(Json(summarize_rewards(wallets, config).await))
}

async fn rewards_claim(
    State(state): State<AppState>,
    Json(mut request): Json<RewardsClaimRequest>,
) -> Result<Json<RewardsClaimResponse>, (StatusCode, String)> {
    let client_request_id =
        normalize_token_distribution_client_request_id(request.client_request_id.clone())?;
    request.client_request_id = Some(client_request_id.clone());
    let engine = state.engine.read().await.clone();
    let available_wallets = build_effective_wallets(&engine)
        .into_iter()
        .filter(|wallet| wallet.enabled)
        .map(|wallet| (wallet.key.clone(), wallet.public_key.clone()))
        .collect::<HashMap<_, _>>();
    request.items = request
        .items
        .into_iter()
        .filter_map(|mut item| {
            item.wallet_key = item.wallet_key.trim().to_string();
            item.provider_id = item.provider_id.trim().to_string();
            item.wallet_public_key = item.wallet_public_key.trim().to_string();
            item.mint = item.mint.trim().to_string();
            if item.amount_lamports == 0 {
                return None;
            }
            let expected_public_key = available_wallets.get(&item.wallet_key)?;
            if expected_public_key != &item.wallet_public_key {
                return None;
            }
            Some(item)
        })
        .collect();
    if request.items.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "No claimable rewards were selected.".to_string(),
        ));
    }
    let config = RewardsExecutionConfig {
        commitment: engine.settings.execution_commitment.clone(),
        skip_preflight: engine.settings.execution_skip_preflight,
        track_send_block_height: config_track_send_block_height(
            engine.config.as_ref().unwrap_or(&Value::Null),
        ),
    };
    let mut fingerprint_request = request.clone();
    fingerprint_request.client_request_id = None;
    let fingerprint = build_token_distribution_fingerprint("rewards-claim", &fingerprint_request)?;
    let response =
        execute_idempotent_rewards_claim(&state, client_request_id, fingerprint, || async {
            claim_rewards(request, config).await
        })
        .await?;
    Ok(Json(response))
}

async fn execute_idempotent_rewards_claim<F, Fut>(
    state: &AppState,
    client_request_id: String,
    fingerprint: String,
    execute: F,
) -> Result<RewardsClaimResponse, (StatusCode, String)>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<RewardsClaimResponse, String>>,
{
    let now = now_unix_ms();
    {
        let mut requests = state.rewards_requests.write().await;
        prune_rewards_requests(&mut requests, now);
        if let Some(existing) = requests.get(&client_request_id) {
            if existing.fingerprint != fingerprint {
                return Err((
                    StatusCode::CONFLICT,
                    format!(
                        "clientRequestId {client_request_id} was already used for a different rewards claim"
                    ),
                ));
            }
            if let Some(response) = &existing.response {
                return Ok(response.clone());
            }
            return Err((
                StatusCode::CONFLICT,
                format!("clientRequestId {client_request_id} is already in progress"),
            ));
        }

        if let Some((_, existing)) = requests.iter().find(|(request_id, entry)| {
            **request_id != client_request_id && entry.fingerprint == fingerprint
        }) {
            if let Some(response) = &existing.response {
                return Ok(response.clone());
            }
            return Err((
                StatusCode::CONFLICT,
                "Matching rewards claim is already in progress.".to_string(),
            ));
        }

        requests.insert(
            client_request_id.clone(),
            RewardsRequestRecord {
                fingerprint: fingerprint.clone(),
                response: None,
                expires_at_unix_ms: now.saturating_add(TOKEN_DISTRIBUTION_STALE_MS),
            },
        );
    }

    let response = match execute().await {
        Ok(response) => response,
        Err(error) => {
            let mut requests = state.rewards_requests.write().await;
            if requests.get(&client_request_id).is_some_and(|record| {
                record.fingerprint == fingerprint && record.response.is_none()
            }) {
                requests.remove(&client_request_id);
            }
            return Err((StatusCode::BAD_REQUEST, error));
        }
    };
    let mut requests = state.rewards_requests.write().await;
    requests.insert(
        client_request_id,
        RewardsRequestRecord {
            fingerprint,
            response: Some(response.clone()),
            expires_at_unix_ms: now_unix_ms().saturating_add(TOKEN_DISTRIBUTION_STALE_MS),
        },
    );
    Ok(response)
}

fn prune_rewards_requests(requests: &mut HashMap<String, RewardsRequestRecord>, now_unix_ms: u64) {
    requests.retain(|_, entry| entry.expires_at_unix_ms > now_unix_ms);
}

async fn execute_idempotent_token_distribution<F, Fut>(
    state: &AppState,
    client_request_id: String,
    fingerprint: String,
    execute: F,
) -> Result<(TokenDistributionResponse, bool), (StatusCode, String)>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<TokenDistributionResponse, String>>,
{
    let now = now_unix_ms();
    {
        let mut requests = state.token_distribution_requests.write().await;
        prune_token_distribution_requests(&mut requests, now);
        if let Some(existing) = requests.get(&client_request_id) {
            if existing.fingerprint != fingerprint {
                return Err((
                    StatusCode::CONFLICT,
                    format!(
                        "clientRequestId {client_request_id} was already used for a different token distribution request"
                    ),
                ));
            }
            if let Some(response) = &existing.response {
                return Ok((response.clone(), false));
            }
            return Err((
                StatusCode::CONFLICT,
                format!("clientRequestId {client_request_id} is already in progress"),
            ));
        }

        if let Some((_, existing)) = requests.iter().find(|(request_id, entry)| {
            **request_id != client_request_id && entry.fingerprint == fingerprint
        }) {
            if let Some(response) = &existing.response {
                return Ok((response.clone(), false));
            }
            return Err((
                StatusCode::CONFLICT,
                "Matching token distribution request is already in progress.".to_string(),
            ));
        }

        requests.insert(
            client_request_id.clone(),
            TokenDistributionRequestRecord {
                fingerprint: fingerprint.clone(),
                response: None,
                expires_at_unix_ms: now.saturating_add(TOKEN_DISTRIBUTION_STALE_MS),
            },
        );
    }

    let response = match execute().await {
        Ok(response) => response,
        Err(error) => {
            let mut requests = state.token_distribution_requests.write().await;
            if requests.get(&client_request_id).is_some_and(|record| {
                record.fingerprint == fingerprint && record.response.is_none()
            }) {
                requests.remove(&client_request_id);
            }
            return Err((StatusCode::BAD_REQUEST, error));
        }
    };
    let mut requests = state.token_distribution_requests.write().await;
    requests.insert(
        client_request_id,
        TokenDistributionRequestRecord {
            fingerprint,
            response: Some(response.clone()),
            expires_at_unix_ms: now_unix_ms().saturating_add(TOKEN_DISTRIBUTION_STALE_MS),
        },
    );
    Ok((response, true))
}

fn prune_token_distribution_requests(
    requests: &mut HashMap<String, TokenDistributionRequestRecord>,
    now_unix_ms: u64,
) {
    requests.retain(|_, entry| entry.expires_at_unix_ms > now_unix_ms);
}

fn normalize_token_distribution_client_request_id(
    client_request_id: Option<String>,
) -> Result<String, (StatusCode, String)> {
    let normalized = client_request_id.unwrap_or_default().trim().to_string();
    if normalized.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "clientRequestId is required".to_string(),
        ));
    }
    Ok(normalized)
}

fn build_token_distribution_fingerprint<T: Serialize>(
    action: &str,
    request: &T,
) -> Result<String, (StatusCode, String)> {
    let mut hasher = Sha256::new();
    hasher.update(action.as_bytes());
    hasher.update(b"\0");
    let bytes = serde_json::to_vec(request).map_err(internal_error)?;
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_distribution_wallet_keys(
    wallet_keys: &[String],
    available_wallet_keys: &HashSet<String>,
) -> Result<Vec<String>, (StatusCode, String)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for wallet_key in wallet_keys {
        let normalized = wallet_key.trim();
        if normalized.is_empty() || !seen.insert(normalized.to_string()) {
            continue;
        }
        if !available_wallet_keys.contains(normalized) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Wallet {normalized} is not available."),
            ));
        }
        out.push(normalized.to_string());
    }
    Ok(out)
}

async fn apply_token_distribution_ledger(state: &AppState, response: &TokenDistributionResponse) {
    let confirmed_transfers = response
        .transfers
        .iter()
        .filter(|transfer| {
            transfer.status == "confirmed"
                && transfer.amount_raw > 0
                && transfer
                    .signature
                    .as_ref()
                    .is_some_and(|signature| !signature.trim().is_empty())
        })
        .collect::<Vec<_>>();
    if confirmed_transfers.is_empty() {
        return;
    }
    let now_ms = now_unix_ms();
    let mut ledger = state.trade_ledger.write().await;
    for transfer in confirmed_transfers {
        let Some(signature) = transfer.signature.as_deref() else {
            continue;
        };
        let marker = crate::trade_ledger::TokenTransferMarkerEvent::new(
            &transfer.source_wallet_key,
            &transfer.destination_wallet_key,
            &response.mint,
            transfer.amount_raw,
            signature,
            now_ms,
        );
        match crate::trade_ledger::append_token_transfer_marker(&state.trade_ledger_paths, &marker)
        {
            Ok(crate::trade_ledger::JournalAppendStatus::Appended) => {}
            Ok(crate::trade_ledger::JournalAppendStatus::Duplicate) => continue,
            Err((_, error)) => {
                eprintln!(
                    "[execution-engine][token-distribution] ledger marker append failed source={} destination={} signature={} err={}",
                    transfer.source_wallet_key, transfer.destination_wallet_key, signature, error
                );
                state.persist_failures.record_trade_ledger_failure(&error);
                continue;
            }
        }
        crate::trade_ledger::transfer_trade_ledger_position(
            &mut ledger,
            &transfer.source_wallet_key,
            &transfer.destination_wallet_key,
            &response.mint,
            transfer.amount_raw,
            signature,
            now_ms,
        );
    }
    if let Err((_, error)) = persist_trade_ledger(&state.trade_ledger_paths, &ledger) {
        eprintln!("[execution-engine][token-distribution] ledger persist failed: {error}");
        state.persist_failures.record_trade_ledger_failure(&error);
    }
}

async fn get_batch_status(
    State(state): State<AppState>,
    Path(batch_id): Path<String>,
) -> Result<Json<BatchStatusResponse>, (StatusCode, String)> {
    let batches = state.batches.read().await;
    let batch = batches.get(&batch_id).cloned().ok_or((
        StatusCode::NOT_FOUND,
        format!("unknown batch id: {batch_id}"),
    ))?;
    Ok(Json(batch))
}

async fn list_batches(State(state): State<AppState>) -> Json<BatchHistoryResponse> {
    let batches = state.batches.read().await;
    Json(BatchHistoryResponse {
        batches: history_entries(&batches),
    })
}

async fn enqueue_batch(
    state: &AppState,
    max_active_batches: usize,
    submission: ExecutionSubmission,
) -> Result<ExecutionAcceptedResponse, (StatusCode, String)> {
    if submission.target.wallet_count == 0 || submission.execution_plan.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "select at least one enabled wallet".to_string(),
        ));
    }

    let now = now_unix_ms();
    let mut accepted_requests = state.accepted_requests.write().await;
    prune_accepted_requests(&mut accepted_requests, now);

    if let Some(existing) = accepted_requests.get(&submission.client_request_id) {
        if existing.fingerprint != submission.fingerprint {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "clientRequestId {} was already used for a different trade request",
                    submission.client_request_id
                ),
            ));
        }

        let mut accepted = existing.accepted.clone();
        accepted.deduped = true;
        return Ok(accepted);
    }

    let mut batches = state.batches.write().await;
    if active_batch_count(&batches) >= max_active_batches {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "too many active batches; wait for confirmations before submitting more than {max_active_batches}"
            ),
        ));
    }

    let batch_id = Uuid::new_v4().to_string();
    let status = BatchStatusResponse {
        batch_id: batch_id.clone(),
        client_request_id: submission.client_request_id.clone(),
        side: submission.side.clone(),
        status: BatchLifecycleStatus::Queued,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
        execution_adapter: submission.execution_adapter.clone(),
        execution_backend: Some(submission.execution_backend.clone()),
        planned_execution: submission.planned_execution.clone(),
        batch_policy: submission.batch_policy.clone(),
        summary: BatchSummary {
            total_wallets: submission.target.wallet_count,
            queued_wallets: submission.target.wallet_count,
            submitted_wallets: 0,
            confirmed_wallets: 0,
            failed_wallets: 0,
        },
        wallets: submission
            .execution_plan
            .iter()
            .map(|entry| WalletExecutionState {
                wallet_key: entry.wallet_key.clone(),
                status: BatchLifecycleStatus::Queued,
                tx_signature: None,
                error: None,
                buy_amount_sol: entry.planned_summary.buy_amount_sol.clone(),
                scheduled_delay_ms: entry.planned_summary.scheduled_delay_ms,
                delay_applied: entry.planned_summary.delay_applied,
                first_buy: entry.planned_summary.first_buy,
                applied_variance_percent: entry.planned_summary.applied_variance_percent,
                entry_preference_asset: None,
            })
            .collect(),
    };

    batches.insert(batch_id.clone(), status.clone());
    persist_batch_history(&state.batch_history_path, &batches)?;
    publish_batch_status_snapshot(&state, &status, "queued");
    let accepted = ExecutionAcceptedResponse {
        batch_id: batch_id.clone(),
        client_request_id: submission.client_request_id.clone(),
        accepted_at_unix_ms: now,
        side: submission.side,
        status: BatchLifecycleStatus::Queued,
        wallet_count: submission.target.wallet_count,
        deduped: false,
        warnings: submission.warnings.clone(),
    };
    accepted_requests.insert(
        submission.client_request_id,
        AcceptedRequestRecord {
            fingerprint: submission.fingerprint,
            accepted: accepted.clone(),
            expires_at_unix_ms: now + IDEMPOTENCY_WINDOW_MS,
        },
    );

    let execution_plan = submission.execution_plan.clone();
    drop(batches);
    drop(accepted_requests);

    tokio::spawn(execute_batch_runtime(
        state.clone(),
        batch_id.clone(),
        submission.execution_adapter.clone(),
        submission.execution_backend.clone(),
        submission.planned_execution.clone(),
        submission.client_started_at_unix_ms,
        submission.batch_policy.clone(),
        execution_plan,
    ));

    Ok(accepted)
}

async fn update_batch_execution_metadata(
    state: &AppState,
    batch_id: &str,
    execution_adapter: Option<String>,
    execution_backend: Option<String>,
    planned_execution: Option<LifecycleAndCanonicalMarket>,
    batch_policy: Option<BatchExecutionPolicySummary>,
) {
    let now = now_unix_ms();
    let mut guard = state.batches.write().await;
    let Some(batch) = guard.get_mut(batch_id) else {
        return;
    };
    batch.updated_at_unix_ms = now;
    if execution_adapter.is_some() {
        batch.execution_adapter = execution_adapter;
    }
    if execution_backend.is_some() {
        batch.execution_backend = execution_backend;
    }
    if planned_execution.is_some() {
        batch.planned_execution = planned_execution;
    }
    if batch_policy.is_some() {
        batch.batch_policy = batch_policy;
    }
    let batch_snapshot = batch.clone();
    if let Err((_, error)) = persist_batch_history(&state.batch_history_path, &guard) {
        eprintln!("[execution-engine][persist] batch history write failed: {error}");
        state.persist_failures.record_batch_history_failure(&error);
    }
    drop(guard);
    publish_batch_status_snapshot(state, &batch_snapshot, "metadata");
}

async fn sync_batch_wallet_plan_summaries(
    state: &AppState,
    batch_id: &str,
    execution_plan: &[PlannedWalletExecution],
    failed_wallets: &[(WalletTradeRequest, WalletExecutionState)],
) {
    let mut summaries: HashMap<String, WalletExecutionPlanSummary> = execution_plan
        .iter()
        .map(|entry| (entry.wallet_key.clone(), entry.planned_summary.clone()))
        .collect();
    for (_, outcome) in failed_wallets {
        summaries
            .entry(outcome.wallet_key.clone())
            .or_insert(WalletExecutionPlanSummary {
                wallet_key: outcome.wallet_key.clone(),
                buy_amount_sol: outcome.buy_amount_sol.clone(),
                scheduled_delay_ms: outcome.scheduled_delay_ms,
                delay_applied: outcome.delay_applied,
                first_buy: outcome.first_buy,
                applied_variance_percent: outcome.applied_variance_percent,
                wrapper_fee_bps: 0,
                wrapper_fee_sol: None,
                wrapper_route: None,
            });
    }

    let now = now_unix_ms();
    let mut guard = state.batches.write().await;
    let Some(batch) = guard.get_mut(batch_id) else {
        return;
    };
    for wallet in &mut batch.wallets {
        if let Some(summary) = summaries.get(&wallet.wallet_key) {
            wallet.buy_amount_sol = summary.buy_amount_sol.clone();
            wallet.scheduled_delay_ms = summary.scheduled_delay_ms;
            wallet.delay_applied = summary.delay_applied;
            wallet.first_buy = summary.first_buy;
            wallet.applied_variance_percent = summary.applied_variance_percent;
        }
    }
    batch.updated_at_unix_ms = now;
    let batch_snapshot = batch.clone();
    if let Err((_, error)) = persist_batch_history(&state.batch_history_path, &guard) {
        eprintln!("[execution-engine][persist] batch history write failed: {error}");
        state.persist_failures.record_batch_history_failure(&error);
    }
    drop(guard);
    publish_batch_status_snapshot(state, &batch_snapshot, "plan_summary");
}

async fn execute_batch_runtime(
    state: AppState,
    batch_id: String,
    accepted_execution_adapter: Option<String>,
    execution_backend: String,
    accepted_planned_execution: Option<LifecycleAndCanonicalMarket>,
    client_started_at_unix_ms: Option<u64>,
    mut batch_policy: Option<BatchExecutionPolicySummary>,
    execution_plan: Vec<PlannedWalletExecution>,
) {
    let worker_started_at = now_unix_ms();
    if let Some(client_started_at) = client_started_at_unix_ms {
        eprintln!(
            "[execution-engine][latency] batch={} phase=worker-start click_to_worker_ms={}",
            batch_id,
            worker_started_at.saturating_sub(client_started_at)
        );
    }

    let batch_uses_hellomoon = execution_plan
        .iter()
        .any(|entry| is_hellomoon_provider(&entry.wallet_request.policy.provider));
    let balance_gate = TradeBalanceGate::new(&execution_plan);
    balance_gate.start_buy_refresh(state.clone(), execution_plan.clone());
    let hellomoon_deadline_unix_ms = batch_uses_hellomoon
        .then(|| worker_started_at.saturating_add(HELLOMOON_BATCH_WALLET_TIMEOUT_MS));
    let planning_started_at = now_unix_ms();
    let planned_batch = if batch_uses_hellomoon {
        let timeout =
            match hellomoon_remaining_timeout(hellomoon_deadline_unix_ms, "route planning") {
                Ok(timeout) => timeout,
                Err(error) => {
                    fail_unresolved_batch_wallets(&state, &batch_id, &error).await;
                    return;
                }
            };
        match tokio::time::timeout(timeout, plan_batch_wallet_trades(execution_plan)).await {
            Ok(planned_batch) => planned_batch,
            Err(_) => {
                fail_unresolved_batch_wallets(
                    &state,
                    &batch_id,
                    "Hello Moon route planning timed out after 10s.",
                )
                .await;
                return;
            }
        }
    } else {
        plan_batch_wallet_trades(execution_plan).await
    };
    let resolved_execution_adapter = planned_batch
        .execution_adapter
        .clone()
        .or(accepted_execution_adapter);
    let resolved_planned_execution = planned_batch
        .planned_execution
        .clone()
        .or(accepted_planned_execution);
    let resolved_execution_backend = planned_batch
        .execution_backend
        .clone()
        .unwrap_or(execution_backend);
    balance_gate.start_sell_refresh(state.clone(), planned_batch.execution_plan.clone());
    normalize_direct_stable_batch_policy_amounts(
        resolved_planned_execution.as_ref(),
        &planned_batch.execution_plan,
        &mut batch_policy,
    );
    update_batch_execution_metadata(
        &state,
        &batch_id,
        resolved_execution_adapter,
        Some(resolved_execution_backend),
        resolved_planned_execution.clone(),
        batch_policy.clone(),
    )
    .await;
    if let Some(selector) = resolved_planned_execution.as_ref() {
        eprintln!(
            "[execution-engine][latency] batch={} phase=route-plan family={} lifecycle={} planning_ms={}",
            batch_id,
            selector.family.label(),
            selector.lifecycle.label(),
            now_unix_ms().saturating_sub(planning_started_at)
        );
    }

    let execution_plan = if batch_uses_hellomoon {
        let timeout =
            match hellomoon_remaining_timeout(hellomoon_deadline_unix_ms, "first-buy refresh") {
                Ok(timeout) => timeout,
                Err(error) => {
                    fail_unresolved_batch_wallets(&state, &batch_id, &error).await;
                    return;
                }
            };
        match tokio::time::timeout(
            timeout,
            refresh_first_buy_only_batch_plan(
                &state,
                planned_batch.execution_plan,
                batch_policy.as_ref(),
            ),
        )
        .await
        {
            Ok(execution_plan) => execution_plan,
            Err(_) => {
                fail_unresolved_batch_wallets(
                    &state,
                    &batch_id,
                    "Hello Moon first-buy refresh timed out after 10s.",
                )
                .await;
                return;
            }
        }
    } else {
        refresh_first_buy_only_batch_plan(
            &state,
            planned_batch.execution_plan,
            batch_policy.as_ref(),
        )
        .await
    };
    balance_gate.refresh_plan_summaries(&execution_plan);
    sync_batch_wallet_plan_summaries(
        &state,
        &batch_id,
        &execution_plan,
        &planned_batch.failed_wallets,
    )
    .await;
    for (wallet_request, outcome) in planned_batch.failed_wallets {
        apply_wallet_execution_outcome(&state, &batch_id, &wallet_request, outcome).await;
    }
    if execution_plan.is_empty() {
        fail_unresolved_batch_wallets(
            &state,
            &batch_id,
            "No executable wallet routes remained after planning.",
        )
        .await;
        return;
    }
    let mut executions = JoinSet::new();
    for entry in execution_plan {
        let executor = state.executor.clone();
        let request = entry.wallet_request.clone();
        let wallet_key = entry.wallet_key.clone();
        let planned_summary = entry.planned_summary.clone();
        let hellomoon_deadline_for_execution = hellomoon_deadline_unix_ms;
        let balance_gate = Arc::clone(&balance_gate);
        let state_for_submit = state.clone();
        let batch_id_for_submit = batch_id.clone();
        executions.spawn(async move {
            let request_for_result = request.clone();
            let provider = request.policy.provider.clone();
            let uses_hellomoon = is_hellomoon_provider(&provider);
            let mut delayed_timeout_error = None;
            if planned_summary.scheduled_delay_ms > 0 {
                let delay = Duration::from_millis(planned_summary.scheduled_delay_ms);
                if uses_hellomoon {
                    match hellomoon_remaining_timeout(
                        hellomoon_deadline_for_execution,
                        "transaction",
                    ) {
                        Ok(remaining) if delay > remaining => {
                            tokio::time::sleep(remaining).await;
                            delayed_timeout_error = Some(hellomoon_transaction_timeout_error());
                        }
                        Ok(_) => tokio::time::sleep(delay).await,
                        Err(error) => delayed_timeout_error = Some(error),
                    }
                } else {
                    tokio::time::sleep(delay).await;
                }
            }
            let wallet_id = wallet_key.clone();
            let submit_state = state_for_submit.clone();
            let submit_batch_id = batch_id_for_submit.clone();
            let submit_wallet_key = wallet_key.clone();
            let on_submitted = move |signature: &str| {
                let submit_state = submit_state.clone();
                let submit_batch_id = submit_batch_id.clone();
                let submit_wallet_key = submit_wallet_key.clone();
                let signature = signature.to_string();
                tokio::spawn(async move {
                    apply_wallet_submitted(
                        &submit_state,
                        &submit_batch_id,
                        &submit_wallet_key,
                        &signature,
                    )
                    .await;
                });
            };
            let execution_result = if let Some(error) = delayed_timeout_error {
                Err(error)
            } else if uses_hellomoon {
                match hellomoon_remaining_timeout(hellomoon_deadline_for_execution, "transaction") {
                    Ok(timeout) => match tokio::time::timeout(timeout, {
                        let gate = Arc::clone(&balance_gate);
                        executor.execute_wallet_trade_checked_with_submit_callback(
                            request,
                            wallet_key,
                            move |wallet_key, compiled| gate.check_compiled(wallet_key, compiled),
                            on_submitted,
                        )
                    })
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => Err(hellomoon_transaction_timeout_error()),
                    },
                    Err(error) => Err(error),
                }
            } else {
                let gate = Arc::clone(&balance_gate);
                executor
                    .execute_wallet_trade_checked_with_submit_callback(
                        request,
                        wallet_key,
                        move |wallet_key, compiled| gate.check_compiled(wallet_key, compiled),
                        on_submitted,
                    )
                    .await
            };
            match execution_result {
                Ok(ExecutedTrade {
                    tx_signature,
                    entry_preference_asset,
                }) => (
                    WalletExecutionState {
                        wallet_key: wallet_id,
                        status: BatchLifecycleStatus::Confirmed,
                        tx_signature: Some(tx_signature),
                        error: None,
                        buy_amount_sol: planned_summary.buy_amount_sol.clone(),
                        scheduled_delay_ms: planned_summary.scheduled_delay_ms,
                        delay_applied: planned_summary.delay_applied,
                        first_buy: planned_summary.first_buy,
                        applied_variance_percent: planned_summary.applied_variance_percent,
                        entry_preference_asset,
                    },
                    request_for_result,
                ),
                Err(error) => {
                    let submitted_signature =
                        submitted_signature_from_confirmation_gap_error(&error);
                    (
                        WalletExecutionState {
                            wallet_key: wallet_id,
                            status: BatchLifecycleStatus::Failed,
                            tx_signature: submitted_signature,
                            error: Some(error),
                            buy_amount_sol: planned_summary.buy_amount_sol.clone(),
                            scheduled_delay_ms: planned_summary.scheduled_delay_ms,
                            delay_applied: planned_summary.delay_applied,
                            first_buy: planned_summary.first_buy,
                            applied_variance_percent: planned_summary.applied_variance_percent,
                            entry_preference_asset: None,
                        },
                        request_for_result,
                    )
                }
            }
        });
    }

    while let Some(result) = executions.join_next().await {
        let (outcome, wallet_request) = match result {
            Ok(outcome) => outcome,
            Err(error) => (
                WalletExecutionState {
                    wallet_key: "unknown".to_string(),
                    status: BatchLifecycleStatus::Failed,
                    tx_signature: None,
                    error: Some(format!("Wallet task join failed: {error}")),
                    buy_amount_sol: None,
                    scheduled_delay_ms: 0,
                    delay_applied: false,
                    first_buy: None,
                    applied_variance_percent: None,
                    entry_preference_asset: None,
                },
                WalletTradeRequest {
                    side: TradeSide::Buy,
                    mint: String::new(),
                    platform_label: None,
                    buy_amount_sol: None,
                    sell_intent: None,
                    policy: ExecutionPolicy {
                        slippage_percent: String::new(),
                        mev_mode: MevMode::Off,
                        auto_tip_enabled: false,
                        fee_sol: String::new(),
                        tip_sol: String::new(),
                        provider: String::new(),
                        endpoint_profile: String::new(),
                        commitment: String::new(),
                        skip_preflight: false,
                        track_send_block_height: false,
                        buy_funding_policy: default_buy_funding_policy_sol_only(),
                        sell_settlement_policy: default_sell_settlement_policy_always_to_sol(),
                        sell_settlement_asset: TradeSettlementAsset::Sol,
                    },
                    planned_route: None,
                    planned_trade: None,
                    pinned_pool: None,
                    warm_key: None,
                },
            ),
        };
        apply_wallet_execution_outcome(&state, &batch_id, &wallet_request, outcome).await;
    }
}

async fn fail_unresolved_batch_wallets(state: &AppState, batch_id: &str, error: &str) {
    let now = now_unix_ms();
    let mut guard = state.batches.write().await;
    let Some(batch) = guard.get_mut(batch_id) else {
        return;
    };
    let mut changed = false;
    for wallet in &mut batch.wallets {
        if matches!(
            wallet.status,
            BatchLifecycleStatus::Queued
                | BatchLifecycleStatus::Submitted
                | BatchLifecycleStatus::PartiallyConfirmed
        ) {
            wallet.status = BatchLifecycleStatus::Failed;
            wallet.error.get_or_insert_with(|| error.to_string());
            changed = true;
        }
    }
    if !changed {
        return;
    }
    recompute_batch_summary(batch);
    batch.updated_at_unix_ms = now;
    let batch_snapshot = batch.clone();
    if let Err((_, error)) = persist_batch_history(&state.batch_history_path, &guard) {
        eprintln!("[execution-engine][persist] batch history write failed: {error}");
        state.persist_failures.record_batch_history_failure(&error);
    }
    drop(guard);
    publish_batch_status_snapshot(state, &batch_snapshot, "wallet_failure_reconcile");
}

async fn apply_wallet_submitted(
    state: &AppState,
    batch_id: &str,
    wallet_key: &str,
    signature: &str,
) {
    let normalized_signature = signature.trim();
    if normalized_signature.is_empty() {
        return;
    }
    let now = now_unix_ms();
    let mut guard = state.batches.write().await;
    let Some(batch) = guard.get_mut(batch_id) else {
        return;
    };
    let client_request_id = batch.client_request_id.clone();
    let Some(wallet) = batch
        .wallets
        .iter_mut()
        .find(|wallet| wallet.wallet_key == wallet_key)
    else {
        return;
    };
    if matches!(
        wallet.status,
        BatchLifecycleStatus::Confirmed | BatchLifecycleStatus::Failed
    ) {
        return;
    }
    if matches!(wallet.status, BatchLifecycleStatus::Submitted)
        && wallet.tx_signature.as_deref() == Some(normalized_signature)
    {
        return;
    }
    wallet.status = BatchLifecycleStatus::Submitted;
    wallet.tx_signature = Some(normalized_signature.to_string());
    wallet.error = None;
    recompute_batch_summary(batch);
    batch.updated_at_unix_ms = now;
    let batch_snapshot = batch.clone();
    if let Err((_, error)) = persist_batch_history(&state.batch_history_path, &guard) {
        eprintln!("[execution-engine][persist] batch history write failed: {error}");
        state.persist_failures.record_batch_history_failure(&error);
    }
    drop(guard);
    state.balance_stream.register_trade(
        client_request_id.clone(),
        batch_id.to_string(),
        normalized_signature.to_string(),
    );
    publish_batch_status_snapshot(state, &batch_snapshot, "wallet_submitted");
}

fn is_hellomoon_provider(provider: &str) -> bool {
    provider.trim().eq_ignore_ascii_case("hellomoon")
}

fn hellomoon_remaining_timeout(
    deadline_unix_ms: Option<u64>,
    phase_label: &str,
) -> Result<Duration, String> {
    let Some(deadline_unix_ms) = deadline_unix_ms else {
        return Ok(Duration::from_millis(HELLOMOON_BATCH_WALLET_TIMEOUT_MS));
    };
    let now = now_unix_ms();
    if now >= deadline_unix_ms {
        return Err(format!("Hello Moon {phase_label} timed out after 10s."));
    }
    Ok(Duration::from_millis(deadline_unix_ms.saturating_sub(now)))
}

fn hellomoon_transaction_timeout_error() -> String {
    "Hello Moon transaction timed out after 10s and may not have landed.".to_string()
}

#[derive(Debug, Clone)]
enum TradeBalanceGateSnapshot {
    BuySol(HashMap<String, u64>),
    SellToken {
        mint: String,
        balances: HashMap<String, u64>,
        token_decimals: Option<u8>,
    },
}

#[derive(Debug, Clone)]
enum TradeBalanceGateState {
    Pending,
    Ready(TradeBalanceGateSnapshot),
    Unavailable,
}

#[derive(Debug)]
struct TradeBalanceGate {
    side: Option<TradeSide>,
    state: Mutex<TradeBalanceGateState>,
    plan_summaries: Mutex<HashMap<String, WalletExecutionPlanSummary>>,
}

impl TradeBalanceGate {
    fn new(execution_plan: &[PlannedWalletExecution]) -> Arc<Self> {
        let side = execution_plan
            .first()
            .map(|entry| entry.wallet_request.side.clone());
        let plan_summaries = execution_plan
            .iter()
            .map(|entry| (entry.wallet_key.clone(), entry.planned_summary.clone()))
            .collect::<HashMap<_, _>>();
        Arc::new(Self {
            side,
            state: Mutex::new(TradeBalanceGateState::Pending),
            plan_summaries: Mutex::new(plan_summaries),
        })
    }

    fn refresh_plan_summaries(&self, execution_plan: &[PlannedWalletExecution]) {
        if let Ok(mut summaries) = self.plan_summaries.lock() {
            *summaries = execution_plan
                .iter()
                .map(|entry| (entry.wallet_key.clone(), entry.planned_summary.clone()))
                .collect();
        }
    }

    fn start_buy_refresh(
        self: &Arc<Self>,
        state: AppState,
        execution_plan: Vec<PlannedWalletExecution>,
    ) {
        if !matches!(self.side.as_ref(), Some(TradeSide::Buy)) {
            return;
        }
        let gate = Arc::clone(self);
        tokio::spawn(async move {
            let stream = state.balance_stream();
            let result = tokio::time::timeout(
                Duration::from_millis(TRADE_BALANCE_GATE_TIMEOUT_MS),
                fetch_buy_balance_gate_snapshot(state, execution_plan),
            )
            .await
            .map_err(|_| "buy balance refresh timed out".to_string())
            .and_then(|result| result);
            let publish_snapshot = result.as_ref().ok().cloned();
            gate.finish_refresh(result);
            if let Some(snapshot) = publish_snapshot.as_ref() {
                publish_balance_gate_snapshot(&stream, snapshot);
            }
        });
    }

    fn start_sell_refresh(
        self: &Arc<Self>,
        state: AppState,
        execution_plan: Vec<PlannedWalletExecution>,
    ) {
        if !matches!(self.side.as_ref(), Some(TradeSide::Sell)) {
            return;
        }
        let gate = Arc::clone(self);
        tokio::spawn(async move {
            let stream = state.balance_stream();
            let result = tokio::time::timeout(
                Duration::from_millis(TRADE_BALANCE_GATE_TIMEOUT_MS),
                fetch_sell_balance_gate_snapshot(state, execution_plan),
            )
            .await
            .map_err(|_| "sell token balance refresh timed out".to_string())
            .and_then(|result| result);
            let publish_snapshot = result.as_ref().ok().cloned();
            gate.finish_refresh(result);
            if let Some(snapshot) = publish_snapshot.as_ref() {
                publish_balance_gate_snapshot(&stream, snapshot);
            }
        });
    }

    fn finish_refresh(&self, result: Result<TradeBalanceGateSnapshot, String>) {
        let next = match result {
            Ok(snapshot) => TradeBalanceGateState::Ready(snapshot),
            Err(error) => {
                eprintln!("[execution-engine][balance-gate] skipped: {error}");
                TradeBalanceGateState::Unavailable
            }
        };
        if let Ok(mut state) = self.state.lock() {
            *state = next;
        }
    }

    fn check_compiled(
        &self,
        wallet_key: &str,
        compiled: &crate::trade_runtime::CompiledTradePlan,
    ) -> Result<(), String> {
        let state = self
            .state
            .lock()
            .map(|state| state.clone())
            .unwrap_or(TradeBalanceGateState::Unavailable);
        match state {
            TradeBalanceGateState::Pending | TradeBalanceGateState::Unavailable => Ok(()),
            TradeBalanceGateState::Ready(TradeBalanceGateSnapshot::BuySol(balances)) => {
                let Some(available_lamports) = balances.get(wallet_key).copied() else {
                    return Ok(());
                };
                let Some(required_lamports) =
                    required_buy_balance_lamports(wallet_key, compiled, self)
                else {
                    return Ok(());
                };
                if available_lamports < required_lamports {
                    return Err(insufficient_sol_balance_message(
                        wallet_key,
                        required_lamports,
                        available_lamports,
                    ));
                }
                Ok(())
            }
            TradeBalanceGateState::Ready(TradeBalanceGateSnapshot::SellToken {
                mint,
                balances,
                token_decimals: _,
            }) => {
                if compiled.normalized_request.mint.trim() != mint {
                    return Ok(());
                }
                let Some(available_raw) = balances.get(wallet_key).copied() else {
                    return Ok(());
                };
                let Some(required_raw) =
                    required_sell_token_balance_raw(&compiled.normalized_request, available_raw)
                else {
                    return Ok(());
                };
                if available_raw < required_raw {
                    return Err(insufficient_token_balance_message(
                        wallet_key,
                        &mint,
                        required_raw,
                        available_raw,
                    ));
                }
                Ok(())
            }
        }
    }
}

async fn fetch_buy_balance_gate_snapshot(
    state: AppState,
    execution_plan: Vec<PlannedWalletExecution>,
) -> Result<TradeBalanceGateSnapshot, String> {
    let wallet_keys = execution_plan
        .iter()
        .map(|entry| entry.wallet_key.clone())
        .collect::<Vec<_>>();
    let wallet_public_keys = wallet_public_keys_for_balance_gate(&state, &wallet_keys).await;
    let balances =
        fetch_wallet_sol_balances_once(&configured_rpc_url(), &wallet_public_keys).await?;
    Ok(TradeBalanceGateSnapshot::BuySol(balances))
}

async fn fetch_sell_balance_gate_snapshot(
    state: AppState,
    execution_plan: Vec<PlannedWalletExecution>,
) -> Result<TradeBalanceGateSnapshot, String> {
    let started = Instant::now();
    let mint = execution_plan
        .iter()
        .find_map(|entry| {
            let mint = entry.wallet_request.mint.trim();
            (!mint.is_empty()).then(|| mint.to_string())
        })
        .ok_or_else(|| "sell token balance refresh had no mint".to_string())?;
    let wallet_keys = execution_plan
        .iter()
        .map(|entry| entry.wallet_key.clone())
        .collect::<Vec<_>>();
    let token_decimals = token_decimals_for_balance_gate(&state, &mint).await;
    let wallet_public_keys = wallet_public_keys_for_balance_gate(&state, &wallet_keys).await;
    let (mut balances, missing_wallets) = if let Some(decimals) = token_decimals {
        cached_sell_token_balances_for_gate(&wallet_public_keys, &mint, decimals).await
    } else {
        (HashMap::new(), wallet_public_keys.clone())
    };
    if missing_wallets.is_empty() {
        eprintln!(
            "[execution-engine][latency] phase=balance-gate side=sell balance_source=stream_cache balance_gate_ready_ms={}",
            started.elapsed().as_millis()
        );
        return Ok(TradeBalanceGateSnapshot::SellToken {
            mint,
            balances,
            token_decimals,
        });
    }
    let fetched =
        fetch_wallet_token_balances_once(&configured_rpc_url(), &missing_wallets, &mint).await?;
    if let Some(decimals) = token_decimals {
        for (wallet_key, raw) in &fetched {
            crate::wallet_token_cache::record_raw_token_balance(wallet_key, &mint, *raw, decimals)
                .await;
        }
    }
    let had_cached = !balances.is_empty();
    balances.extend(fetched);
    let balance_source = if had_cached {
        "stream_cache+getMultipleAccounts"
    } else {
        "getMultipleAccounts"
    };
    eprintln!(
        "[execution-engine][latency] phase=balance-gate side=sell balance_source={} balance_gate_ready_ms={}",
        balance_source,
        started.elapsed().as_millis()
    );
    Ok(TradeBalanceGateSnapshot::SellToken {
        mint,
        balances,
        token_decimals,
    })
}

async fn cached_sell_token_balances_for_gate(
    wallet_public_keys: &[(String, String)],
    mint: &str,
    token_decimals: u8,
) -> (HashMap<String, u64>, Vec<(String, String)>) {
    if wallet_public_keys.is_empty() {
        return (HashMap::new(), Vec::new());
    }
    let cache = crate::wallet_token_cache::shared_wallet_token_cache();
    let scale = 10u128.pow(u32::from(token_decimals));
    let mut balances = HashMap::with_capacity(wallet_public_keys.len());
    let mut missing = Vec::new();
    for (wallet_key, public_key) in wallet_public_keys {
        if let Some(cached) = cache.lookup(wallet_key, mint).await {
            let raw = (cached.ui_amount.max(0.0) * scale as f64) as u128;
            balances.insert(wallet_key.clone(), u64::try_from(raw).unwrap_or(u64::MAX));
        } else {
            missing.push((wallet_key.clone(), public_key.clone()));
        }
    }
    (balances, missing)
}

async fn wallet_public_keys_for_balance_gate(
    state: &AppState,
    wallet_keys: &[String],
) -> Vec<(String, String)> {
    let requested = wallet_keys.iter().cloned().collect::<HashSet<_>>();
    let engine = state.engine.read().await.clone();
    let enabled_wallets = build_effective_wallets(&engine)
        .into_iter()
        .filter(|wallet| wallet.enabled)
        .collect::<Vec<_>>();
    let use_all_enabled = enabled_wallets.len() <= TRADE_BALANCE_GATE_FRONTEND_REFRESH_WALLET_LIMIT;
    enabled_wallets
        .into_iter()
        .filter(|wallet| use_all_enabled || requested.contains(&wallet.key))
        .map(|wallet| (wallet.key, wallet.public_key))
        .collect()
}

async fn token_decimals_for_balance_gate(state: &AppState, mint: &str) -> Option<u8> {
    let targets = state.live_mark_targets.read().await;
    targets
        .values()
        .find(|target| target.mint.trim() == mint)
        .and_then(|target| target.token_decimals)
}

fn publish_balance_gate_snapshot(
    stream: &BalanceStreamHandle,
    snapshot: &TradeBalanceGateSnapshot,
) {
    let at_ms = u128::from(now_unix_ms());
    match snapshot {
        TradeBalanceGateSnapshot::BuySol(balances) => {
            for (env_key, lamports) in balances {
                stream.publish_balance_event(BalanceEventPayload {
                    env_key: env_key.clone(),
                    balance_sol: Some(*lamports as f64 / 1_000_000_000.0),
                    balance_lamports: Some(*lamports),
                    usd1_balance: None,
                    token_mint: None,
                    token_balance: None,
                    token_balance_raw: None,
                    token_decimals: None,
                    commitment: Some("confirmed".to_string()),
                    source: Some("balanceGate".to_string()),
                    slot: None,
                    at_ms,
                });
            }
        }
        TradeBalanceGateSnapshot::SellToken {
            mint,
            balances,
            token_decimals,
        } => {
            for (env_key, raw) in balances {
                let token_balance =
                    token_decimals.map(|decimals| *raw as f64 / 10_f64.powi(i32::from(decimals)));
                stream.publish_balance_event(BalanceEventPayload {
                    env_key: env_key.clone(),
                    balance_sol: None,
                    balance_lamports: None,
                    usd1_balance: None,
                    token_mint: Some(mint.clone()),
                    token_balance,
                    token_balance_raw: Some(*raw),
                    token_decimals: *token_decimals,
                    commitment: Some("confirmed".to_string()),
                    source: Some("balanceGate".to_string()),
                    slot: None,
                    at_ms,
                });
            }
        }
    }
}

async fn fetch_wallet_sol_balances_once(
    rpc_url: &str,
    wallet_public_keys: &[(String, String)],
) -> Result<HashMap<String, u64>, String> {
    if wallet_public_keys.is_empty() {
        return Ok(HashMap::new());
    }
    if wallet_public_keys.len() > MAX_MULTIPLE_ACCOUNTS_BATCH_SIZE {
        return Err(format!(
            "buy balance refresh requires {} accounts, above one-call limit {}",
            wallet_public_keys.len(),
            MAX_MULTIPLE_ACCOUNTS_BATCH_SIZE
        ));
    }
    let accounts = wallet_public_keys
        .iter()
        .map(|(_, public_key)| public_key.clone())
        .collect::<Vec<_>>();
    let client = extension_wallet_rpc_client()?;
    let result = crate::rpc_client::rpc_request_with_client(
        &client,
        rpc_url,
        "getMultipleAccounts",
        json!([
            accounts,
            {
                "encoding": "base64",
                "commitment": "confirmed",
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
        .ok_or_else(|| "RPC getMultipleAccounts did not return a value array.".to_string())?;
    if values.len() != wallet_public_keys.len() {
        return Err("SOL balance results did not match the wallet count.".to_string());
    }
    let mut balances = HashMap::new();
    for (index, (wallet_key, _)) in wallet_public_keys.iter().enumerate() {
        if values[index].is_null() {
            balances.insert(wallet_key.clone(), 0);
        } else if let Some(lamports) = values[index].get("lamports").and_then(Value::as_u64) {
            balances.insert(wallet_key.clone(), lamports);
        }
    }
    Ok(balances)
}

async fn fetch_wallet_token_balances_once(
    rpc_url: &str,
    wallet_public_keys: &[(String, String)],
    mint: &str,
) -> Result<HashMap<String, u64>, String> {
    if wallet_public_keys.is_empty() {
        return Ok(HashMap::new());
    }
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid mint {mint}: {error}"))?;
    let token_2022 = token_2022_program_id()?;
    let mut account_pairs = Vec::with_capacity(wallet_public_keys.len());
    let mut accounts = Vec::with_capacity(wallet_public_keys.len().saturating_mul(2));
    for (wallet_key, public_key) in wallet_public_keys {
        let owner = Pubkey::from_str(public_key)
            .map_err(|error| format!("Invalid wallet public key {public_key}: {error}"))?;
        let spl_ata =
            get_associated_token_address_with_program_id(&owner, &mint_pubkey, &spl_token::id())
                .to_string();
        let token_2022_ata =
            get_associated_token_address_with_program_id(&owner, &mint_pubkey, &token_2022)
                .to_string();
        let start = accounts.len();
        accounts.push(spl_ata);
        accounts.push(token_2022_ata);
        account_pairs.push((wallet_key.clone(), start));
    }
    if accounts.len() > MAX_MULTIPLE_ACCOUNTS_BATCH_SIZE {
        return Err(format!(
            "sell token balance refresh requires {} accounts, above one-call limit {}",
            accounts.len(),
            MAX_MULTIPLE_ACCOUNTS_BATCH_SIZE
        ));
    }
    let client = extension_wallet_rpc_client()?;
    let account_data =
        fetch_multiple_account_data_with_client(&client, rpc_url, &accounts, "confirmed").await?;
    if account_data.len() != accounts.len() {
        return Err("Token balance results did not match the account count.".to_string());
    }
    let mut balances = HashMap::new();
    for (wallet_key, start) in account_pairs {
        let mut total = 0u64;
        for offset in 0..2 {
            if let Some(data) = account_data
                .get(start + offset)
                .and_then(|data| data.as_ref())
            {
                total = total.saturating_add(parse_token_account_raw_balance(data)?);
            }
        }
        balances.insert(wallet_key, total);
    }
    Ok(balances)
}

fn token_2022_program_id() -> Result<Pubkey, String> {
    Pubkey::from_str(TOKEN_2022_PROGRAM_ID)
        .map_err(|error| format!("Invalid Token-2022 program id: {error}"))
}

fn required_buy_balance_lamports(
    wallet_key: &str,
    compiled: &crate::trade_runtime::CompiledTradePlan,
    gate: &TradeBalanceGate,
) -> Option<u64> {
    if !matches!(&compiled.normalized_request.side, TradeSide::Buy) {
        return None;
    }
    let buy_input = if compiled.wrapper_route.touches_sol() {
        compiled
            .wrapper_payload
            .as_ref()
            .map(|payload| payload.route_metadata.gross_sol_in_lamports)
            .filter(|amount| *amount > 0)
            .or_else(|| {
                compiled
                    .normalized_request
                    .buy_amount_sol
                    .as_deref()
                    .and_then(parse_sol_to_lamports)
            })
            .unwrap_or(0)
    } else {
        0
    };
    let compiled_cost = compiled_transaction_sol_cost_lamports(compiled);
    let rent_buffer = gate
        .plan_summaries
        .lock()
        .ok()
        .and_then(|summaries| summaries.get(wallet_key).cloned())
        .and_then(|summary| summary.first_buy)
        .filter(|first_buy| *first_buy)
        .map(|_| TRADE_BALANCE_GATE_FIRST_BUY_RENT_BUFFER_LAMPORTS)
        .unwrap_or(0);
    Some(required_buy_balance_lamports_from_parts(
        buy_input,
        compiled_cost,
        rent_buffer,
    ))
}

fn required_buy_balance_lamports_from_parts(
    buy_input_lamports: u64,
    compiled_cost_lamports: u64,
    rent_buffer_lamports: u64,
) -> u64 {
    buy_input_lamports
        .saturating_add(compiled_cost_lamports)
        .saturating_add(rent_buffer_lamports)
        .saturating_add(TRADE_BALANCE_GATE_FIXED_BUY_BUFFER_LAMPORTS)
}

fn compiled_transaction_sol_cost_lamports(
    compiled: &crate::trade_runtime::CompiledTradePlan,
) -> u64 {
    compiled.transactions.iter().fold(0u64, |sum, transaction| {
        sum.saturating_add(TRADE_BALANCE_GATE_SIGNATURE_FEE_LAMPORTS)
            .saturating_add(transaction.inline_tip_lamports.unwrap_or(0))
            .saturating_add(
                priority_fee_lamports(
                    transaction.compute_unit_limit,
                    transaction.compute_unit_price_micro_lamports,
                )
                .unwrap_or(0),
            )
    })
}

fn priority_fee_lamports(
    compute_unit_limit: Option<u64>,
    micro_lamports: Option<u64>,
) -> Option<u64> {
    let limit = compute_unit_limit?;
    let price = micro_lamports?;
    let product = u128::from(limit).checked_mul(u128::from(price))?;
    u64::try_from(product.saturating_add(999_999) / 1_000_000).ok()
}

fn required_sell_token_balance_raw(
    request: &TradeRuntimeRequest,
    available_raw: u64,
) -> Option<u64> {
    if !matches!(&request.side, TradeSide::Sell) {
        return None;
    }
    match request.sell_intent.as_ref()? {
        RuntimeSellIntent::Percent(percent) => {
            let percent_units = parse_decimal_units(percent, 4)?;
            if percent_units == 0 {
                return None;
            }
            if available_raw == 0 {
                return Some(1);
            }
            let required = (u128::from(available_raw) * u128::from(percent_units))
                .saturating_add(999_999)
                / 1_000_000;
            u64::try_from(required.max(1)).ok()
        }
        RuntimeSellIntent::SolOutput(output_sol) => {
            parse_sol_to_lamports(output_sol)?;
            None
        }
    }
}

fn insufficient_sol_balance_message(
    _wallet_key: &str,
    _required_lamports: u64,
    _available_lamports: u64,
) -> String {
    "Insufficient SOL balance for buy amount".to_string()
}

fn insufficient_token_balance_message(
    _wallet_key: &str,
    _mint: &str,
    _required_raw: u64,
    _available_raw: u64,
) -> String {
    "Insufficient token balance for sell amount".to_string()
}

fn wallet_request_to_runtime_request(
    request: &WalletTradeRequest,
    planned_route: Option<crate::trade_dispatch::TradeDispatchPlan>,
    planned_trade: Option<LifecycleAndCanonicalMarket>,
) -> TradeRuntimeRequest {
    let resolved_mint = planned_route
        .as_ref()
        .map(|plan| plan.resolved_mint.clone())
        .unwrap_or_else(|| request.mint.clone());
    let resolved_pinned_pool = planned_route
        .as_ref()
        .and_then(|plan| plan.resolved_pinned_pool.clone())
        .or_else(|| request.pinned_pool.clone());
    TradeRuntimeRequest {
        side: request.side.clone(),
        mint: resolved_mint,
        buy_amount_sol: request.buy_amount_sol.clone(),
        sell_intent: request.sell_intent.clone().map(|intent| match intent {
            SellIntent::Percent(value) => RuntimeSellIntent::Percent(value),
            SellIntent::SolOutput(value) => RuntimeSellIntent::SolOutput(value),
        }),
        policy: RuntimeExecutionPolicy {
            slippage_percent: request.policy.slippage_percent.clone(),
            mev_mode: request.policy.mev_mode.clone(),
            auto_tip_enabled: request.policy.auto_tip_enabled,
            fee_sol: request.policy.fee_sol.clone(),
            tip_sol: request.policy.tip_sol.clone(),
            provider: request.policy.provider.clone(),
            endpoint_profile: request.policy.endpoint_profile.clone(),
            commitment: request.policy.commitment.clone(),
            skip_preflight: request.policy.skip_preflight,
            track_send_block_height: request.policy.track_send_block_height,
            buy_funding_policy: request.policy.buy_funding_policy,
            sell_settlement_policy: request.policy.sell_settlement_policy,
            sell_settlement_asset: request.policy.sell_settlement_asset,
        },
        platform_label: request.platform_label.clone(),
        planned_route,
        planned_trade,
        pinned_pool: resolved_pinned_pool,
        warm_key: request.warm_key.clone(),
    }
}

fn normalize_preview_request_for_wrapper_trade(
    request: &TradeRuntimeRequest,
) -> (TradeRuntimeRequest, bool) {
    let mut normalized = request.clone();
    let original_policy = normalized.policy.clone();
    match normalized.side {
        TradeSide::Buy => {
            normalized.policy.buy_funding_policy = BuyFundingPolicy::SolOnly;
        }
        TradeSide::Sell => {
            normalized.policy.sell_settlement_policy = SellSettlementPolicy::AlwaysToSol;
            normalized.policy.sell_settlement_asset = TradeSettlementAsset::Sol;
        }
    }
    let converted = normalized.policy.buy_funding_policy != original_policy.buy_funding_policy
        || normalized.policy.sell_settlement_policy != original_policy.sell_settlement_policy
        || normalized.policy.sell_settlement_asset != original_policy.sell_settlement_asset;
    if converted {
        normalized.planned_route = None;
        normalized.planned_trade = None;
        normalized.warm_key = None;
    }
    (normalized, converted)
}

fn normalize_runtime_request_for_wrapper_trade(
    request: &TradeRuntimeRequest,
) -> (TradeRuntimeRequest, bool) {
    normalize_preview_request_for_wrapper_trade(request)
}

fn normalize_wallet_request_for_wrapper_trade(
    request: &WalletTradeRequest,
) -> (WalletTradeRequest, bool) {
    let mut normalized = request.clone();
    let original_policy = normalized.policy.clone();
    match normalized.side {
        TradeSide::Buy => {
            normalized.policy.buy_funding_policy = BuyFundingPolicy::SolOnly;
        }
        TradeSide::Sell => {
            normalized.policy.sell_settlement_policy = SellSettlementPolicy::AlwaysToSol;
            normalized.policy.sell_settlement_asset = TradeSettlementAsset::Sol;
        }
    }
    let converted = normalized.policy.buy_funding_policy != original_policy.buy_funding_policy
        || normalized.policy.sell_settlement_policy != original_policy.sell_settlement_policy
        || normalized.policy.sell_settlement_asset != original_policy.sell_settlement_asset;
    if converted {
        normalized.planned_route = None;
        normalized.planned_trade = None;
        normalized.warm_key = None;
    }
    (normalized, converted)
}

fn preview_compile_probe_required(
    side: &TradeSide,
    selector: &LifecycleAndCanonicalMarket,
) -> bool {
    matches!(
        selector.family,
        crate::trade_planner::TradeVenueFamily::MeteoraDbc
            | crate::trade_planner::TradeVenueFamily::MeteoraDammV2
    ) || (matches!(side, TradeSide::Buy | TradeSide::Sell)
        && matches!(
            selector.family,
            crate::trade_planner::TradeVenueFamily::BonkLaunchpad
                | crate::trade_planner::TradeVenueFamily::BonkRaydium
        )
        && matches!(
            selector.quote_asset,
            crate::trade_planner::PlannerQuoteAsset::Usd1
        ))
}

async fn run_preview_compile_probe(entry: &PlannedWalletExecution) -> Result<(), String> {
    let selector = entry
        .wallet_request
        .planned_trade
        .clone()
        .ok_or_else(|| "Preview compile probe requires a planned selector.".to_string())?;
    let runtime_request = wallet_request_to_runtime_request(
        &entry.wallet_request,
        entry.wallet_request.planned_route.clone(),
        Some(selector),
    );
    compile_wallet_trade(&runtime_request, &entry.wallet_key)
        .await
        .map(|_| ())
}

async fn run_preview_compile_probes(entries: &[PlannedWalletExecution]) -> Vec<String> {
    let mut join_set = JoinSet::new();
    for entry in entries.iter().cloned() {
        let requires_probe = entry
            .wallet_request
            .planned_trade
            .as_ref()
            .is_some_and(|selector| {
                preview_compile_probe_required(&entry.wallet_request.side, selector)
            });
        if !requires_probe {
            continue;
        }
        join_set.spawn(async move {
            let family = entry
                .wallet_request
                .planned_trade
                .as_ref()
                .map(|selector| selector.family.label().to_string())
                .unwrap_or_else(|| "unknown-family".to_string());
            let wallet_key = entry.wallet_key.clone();
            let result = run_preview_compile_probe(&entry).await;
            (wallet_key, family, result)
        });
    }
    let mut warnings = Vec::new();
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok((wallet_key, family, Ok(()))) => {
                let _ = (wallet_key, family);
            }
            Ok((wallet_key, family, Err(error))) => warnings.push(format!(
                "compile probe failed for wallet {wallet_key} on canonical {family} preview route: {error}"
            )),
            Err(error) => warnings.push(format!(
                "compile probe task failed unexpectedly: {error}"
            )),
        }
    }
    warnings
}

#[derive(Debug)]
struct PlannedBatchRoutes {
    execution_plan: Vec<PlannedWalletExecution>,
    execution_adapter: Option<String>,
    execution_backend: Option<String>,
    planned_execution: Option<LifecycleAndCanonicalMarket>,
    failed_wallets: Vec<(WalletTradeRequest, WalletExecutionState)>,
}

fn compress_first_buy_only_delays(
    execution_plan: &mut [PlannedWalletExecution],
    first_buy_flags: &HashMap<String, bool>,
    policy: &BatchExecutionPolicySummary,
) {
    if !matches!(
        policy.transaction_delay_mode,
        TransactionDelayMode::FirstBuyOnly
    ) {
        return;
    }

    let assumed_first_buy_indexes = execution_plan
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            matches!(entry.planned_summary.first_buy, Some(true)).then_some(index)
        })
        .collect::<Vec<_>>();
    let delayed_increments = match policy.transaction_delay_strategy {
        TransactionDelayStrategy::Fixed => {
            vec![policy.transaction_delay_ms; assumed_first_buy_indexes.len()]
        }
        TransactionDelayStrategy::Random => assumed_first_buy_indexes
            .iter()
            .enumerate()
            .map(|(position, index)| {
                assumed_first_buy_indexes
                    .get(position + 1)
                    .map(|next_index| {
                        execution_plan[*next_index]
                            .planned_summary
                            .scheduled_delay_ms
                            .saturating_sub(
                                execution_plan[*index].planned_summary.scheduled_delay_ms,
                            )
                    })
                    .unwrap_or_default()
            })
            .collect(),
    };

    let mut next_assumed_delay_index = 0usize;
    let mut cumulative_delay_ms = 0u64;
    for entry in execution_plan.iter_mut() {
        let was_assumed_first_buy = matches!(entry.planned_summary.first_buy, Some(true));
        let actual_first_buy = first_buy_flags
            .get(&entry.wallet_key)
            .copied()
            .unwrap_or(entry.planned_summary.first_buy.unwrap_or(false));
        entry.planned_summary.first_buy = Some(actual_first_buy);
        if actual_first_buy {
            entry.planned_summary.scheduled_delay_ms = cumulative_delay_ms;
            entry.planned_summary.delay_applied = cumulative_delay_ms > 0;
        } else {
            entry.planned_summary.scheduled_delay_ms = 0;
            entry.planned_summary.delay_applied = false;
        }
        if was_assumed_first_buy {
            let increment = delayed_increments
                .get(next_assumed_delay_index)
                .copied()
                .unwrap_or_default();
            next_assumed_delay_index += 1;
            if actual_first_buy {
                cumulative_delay_ms = cumulative_delay_ms.saturating_add(increment);
            }
        }
    }
}

async fn refresh_first_buy_only_batch_plan(
    state: &AppState,
    execution_plan: Vec<PlannedWalletExecution>,
    batch_policy: Option<&BatchExecutionPolicySummary>,
) -> Vec<PlannedWalletExecution> {
    let Some(policy) = batch_policy else {
        return execution_plan;
    };
    if !matches!(
        policy.transaction_delay_mode,
        TransactionDelayMode::FirstBuyOnly
    ) || execution_plan.is_empty()
        || !execution_plan
            .iter()
            .all(|entry| matches!(entry.wallet_request.side, TradeSide::Buy))
    {
        return execution_plan;
    }

    let Some(resolved_mint) = execution_plan
        .iter()
        .map(|entry| entry.wallet_request.mint.trim())
        .find(|mint| !mint.is_empty())
        .map(str::to_string)
    else {
        return execution_plan;
    };
    if execution_plan
        .iter()
        .map(|entry| entry.wallet_request.mint.trim())
        .filter(|mint| !mint.is_empty())
        .any(|mint| mint != resolved_mint)
    {
        return execution_plan;
    }

    let engine = state.engine.read().await.clone();
    let trade_ledger = state.trade_ledger.read().await.clone();
    let wallet_keys = execution_plan
        .iter()
        .map(|entry| entry.wallet_key.clone())
        .collect::<Vec<_>>();
    let first_buy_flags = determine_first_buy_flags(
        &build_effective_wallets(&engine),
        &wallet_keys,
        &resolved_mint,
        &trade_ledger,
        true,
    )
    .await;
    let mut updated = execution_plan;
    compress_first_buy_only_delays(&mut updated, &first_buy_flags, policy);
    updated
}

fn batch_route_plan_key_for_request(request: &WalletTradeRequest) -> String {
    json!({
        "side": request.side,
        "mint": request.mint,
        "pinnedPool": request.pinned_pool,
        "commitment": request.policy.commitment,
    })
    .to_string()
}

fn route_planning_error_for_request(
    request: &WalletTradeRequest,
    plan: &crate::trade_dispatch::TradeDispatchPlan,
) -> Result<(), String> {
    if !family_execution_enabled(&plan.selector.family) {
        return Err(family_guard_warning(&plan.selector.family)
            .unwrap_or_else(|| "Selected venue family is disabled.".to_string()));
    }
    if let Some(sell_intent) = request.sell_intent.as_ref() {
        validate_sell_intent_for_family(sell_intent, &plan.selector).map_err(|(_, error)| error)?;
    }
    Ok(())
}

fn execution_backend_for_planned_route(selector: &LifecycleAndCanonicalMarket) -> &'static str {
    if matches!(selector.family, TradeVenueFamily::TrustedStableSwap)
        && !matches!(
            selector.quote_asset,
            PlannerQuoteAsset::Sol | PlannerQuoteAsset::Wsol
        )
    {
        "native"
    } else {
        "wrapper"
    }
}

fn direct_stable_buy_input_decimals(selector: &LifecycleAndCanonicalMarket) -> Option<u8> {
    if matches!(selector.family, TradeVenueFamily::TrustedStableSwap)
        && !matches!(
            selector.quote_asset,
            PlannerQuoteAsset::Sol | PlannerQuoteAsset::Wsol
        )
    {
        Some(6)
    } else {
        None
    }
}

fn normalize_direct_stable_buy_amount_precision(
    selector: &LifecycleAndCanonicalMarket,
    request: &mut WalletTradeRequest,
    summary: &mut WalletExecutionPlanSummary,
) {
    if !matches!(request.side, TradeSide::Buy) {
        return;
    }
    let Some(decimals) = direct_stable_buy_input_decimals(selector) else {
        return;
    };
    let Some(amount) = request.buy_amount_sol.as_deref() else {
        return;
    };
    let Some(raw_amount) = parse_decimal_units(amount, decimals) else {
        return;
    };
    let normalized = format_decimal_units(raw_amount, decimals);
    request.buy_amount_sol = Some(normalized.clone());
    summary.buy_amount_sol = Some(normalized);
}

fn normalize_direct_stable_batch_policy_amounts(
    selector: Option<&LifecycleAndCanonicalMarket>,
    execution_plan: &[PlannedWalletExecution],
    batch_policy: &mut Option<BatchExecutionPolicySummary>,
) {
    let (Some(selector), Some(policy)) = (selector, batch_policy.as_mut()) else {
        return;
    };
    let Some(decimals) = direct_stable_buy_input_decimals(selector) else {
        return;
    };
    let mut total_raw = 0u64;
    let mut first_raw = None;
    for entry in execution_plan {
        if !matches!(entry.wallet_request.side, TradeSide::Buy) {
            return;
        }
        let Some(amount) = entry.planned_summary.buy_amount_sol.as_deref() else {
            return;
        };
        let Some(raw) = parse_decimal_units(amount, decimals) else {
            return;
        };
        first_raw.get_or_insert(raw);
        total_raw = total_raw.saturating_add(raw);
    }
    if let Some(raw) = first_raw {
        policy.base_wallet_amount_sol = Some(format_decimal_units(raw, decimals));
        policy.total_batch_spend_sol = Some(format_decimal_units(total_raw, decimals));
    }
}

async fn plan_batch_wallet_trades(
    execution_plan: Vec<PlannedWalletExecution>,
) -> PlannedBatchRoutes {
    let mut grouped: HashMap<String, (WalletTradeRequest, Vec<usize>)> = HashMap::new();
    for (index, entry) in execution_plan.iter().enumerate() {
        let (normalized_request, _) =
            normalize_wallet_request_for_wrapper_trade(&entry.wallet_request);
        let key = batch_route_plan_key_for_request(&normalized_request);
        grouped
            .entry(key)
            .and_modify(|(_, indexes)| indexes.push(index))
            .or_insert_with(|| (normalized_request, vec![index]));
    }

    let mut resolved: HashMap<String, Result<crate::trade_dispatch::TradeDispatchPlan, String>> =
        HashMap::new();
    let mut join_set = JoinSet::new();
    for (key, (request, _)) in &grouped {
        let key = key.clone();
        let (request, _) = normalize_wallet_request_for_wrapper_trade(request);
        join_set.spawn(async move {
            let runtime_request = wallet_request_to_runtime_request(&request, None, None);
            let planned = plan_trade_request_to_dispatch(&runtime_request)
                .await
                .and_then(|plan| {
                    route_planning_error_for_request(&request, &plan)?;
                    Ok(plan)
                });
            (key, planned)
        });
    }
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((key, planned)) => {
                resolved.insert(key, planned);
            }
            Err(error) => {
                eprintln!("[execution-engine][batch-plan] route planning task failed: {error}");
            }
        }
    }

    let mut updated = execution_plan;
    let mut execution_adapter = None;
    let mut execution_backend = None;
    let mut planned_execution = None;
    let mut failed_wallets = Vec::new();
    for (key, (request, indexes)) in grouped {
        match resolved.remove(&key) {
            Some(Ok(plan)) => {
                if execution_adapter.is_none() {
                    execution_adapter = Some(plan.adapter.label().to_string());
                }
                if planned_execution.is_none() {
                    planned_execution = Some(plan.selector.clone());
                }
                if execution_backend.is_none() {
                    execution_backend =
                        Some(execution_backend_for_planned_route(&plan.selector).to_string());
                }
                for index in indexes {
                    if let Some(entry) = updated.get_mut(index) {
                        let (normalized_request, _) =
                            normalize_wallet_request_for_wrapper_trade(&entry.wallet_request);
                        let mut planned_summary = entry.planned_summary.clone();
                        let mut routed_request = WalletTradeRequest {
                            mint: plan.resolved_mint.clone(),
                            pinned_pool: plan.resolved_pinned_pool.clone(),
                            planned_route: Some(plan.clone()),
                            planned_trade: Some(plan.selector.clone()),
                            ..normalized_request
                        };
                        normalize_direct_stable_buy_amount_precision(
                            &plan.selector,
                            &mut routed_request,
                            &mut planned_summary,
                        );
                        entry.wallet_request = routed_request;
                        entry.planned_summary = planned_summary;
                    }
                }
            }
            Some(Err(error)) => {
                for index in indexes {
                    if let Some(entry) = updated.get(index) {
                        failed_wallets.push((
                            entry.wallet_request.clone(),
                            WalletExecutionState {
                                wallet_key: entry.wallet_key.clone(),
                                status: BatchLifecycleStatus::Failed,
                                tx_signature: None,
                                error: Some(error.clone()),
                                buy_amount_sol: entry.planned_summary.buy_amount_sol.clone(),
                                scheduled_delay_ms: entry.planned_summary.scheduled_delay_ms,
                                delay_applied: entry.planned_summary.delay_applied,
                                first_buy: entry.planned_summary.first_buy,
                                applied_variance_percent: entry
                                    .planned_summary
                                    .applied_variance_percent,
                                entry_preference_asset: None,
                            },
                        ));
                    } else {
                        failed_wallets.push((
                            request.clone(),
                            WalletExecutionState {
                                wallet_key: "unknown".to_string(),
                                status: BatchLifecycleStatus::Failed,
                                tx_signature: None,
                                error: Some(error.clone()),
                                buy_amount_sol: request.buy_amount_sol.clone(),
                                scheduled_delay_ms: 0,
                                delay_applied: false,
                                first_buy: None,
                                applied_variance_percent: None,
                                entry_preference_asset: None,
                            },
                        ));
                    }
                }
            }
            None => {
                let error = "Route planning failed before first submit.".to_string();
                for index in indexes {
                    if let Some(entry) = updated.get(index) {
                        failed_wallets.push((
                            entry.wallet_request.clone(),
                            WalletExecutionState {
                                wallet_key: entry.wallet_key.clone(),
                                status: BatchLifecycleStatus::Failed,
                                tx_signature: None,
                                error: Some(error.clone()),
                                buy_amount_sol: entry.planned_summary.buy_amount_sol.clone(),
                                scheduled_delay_ms: entry.planned_summary.scheduled_delay_ms,
                                delay_applied: entry.planned_summary.delay_applied,
                                first_buy: entry.planned_summary.first_buy,
                                applied_variance_percent: entry
                                    .planned_summary
                                    .applied_variance_percent,
                                entry_preference_asset: None,
                            },
                        ));
                    } else {
                        failed_wallets.push((
                            request.clone(),
                            WalletExecutionState {
                                wallet_key: "unknown".to_string(),
                                status: BatchLifecycleStatus::Failed,
                                tx_signature: None,
                                error: Some(error.clone()),
                                buy_amount_sol: request.buy_amount_sol.clone(),
                                scheduled_delay_ms: 0,
                                delay_applied: false,
                                first_buy: None,
                                applied_variance_percent: None,
                                entry_preference_asset: None,
                            },
                        ));
                    }
                }
            }
        }
    }

    let failed_wallet_keys = failed_wallets
        .iter()
        .map(|(_, outcome)| outcome.wallet_key.as_str())
        .collect::<HashSet<_>>();
    let successful_entries = updated
        .into_iter()
        .filter(|entry| !failed_wallet_keys.contains(entry.wallet_key.as_str()))
        .collect();

    PlannedBatchRoutes {
        execution_plan: successful_entries,
        execution_adapter,
        execution_backend,
        planned_execution,
        failed_wallets,
    }
}

async fn apply_wallet_execution_outcome(
    state: &AppState,
    batch_id: &str,
    wallet_request: &WalletTradeRequest,
    mut outcome: WalletExecutionState,
) {
    let now = now_unix_ms();
    let (batch_client_request_id, duplicate_owner, already_registered_signature) = {
        let guard = state.batches.read().await;
        let Some(batch) = guard.get(batch_id) else {
            return;
        };
        let duplicate_owner = if matches!(outcome.status, BatchLifecycleStatus::Confirmed) {
            outcome.tx_signature.as_deref().and_then(|signature| {
                duplicate_signature_owner(&guard, batch_id, &outcome.wallet_key, signature)
            })
        } else {
            None
        };
        let already_registered_signature = outcome
            .tx_signature
            .as_deref()
            .and_then(|signature| {
                batch
                    .wallets
                    .iter()
                    .find(|wallet| wallet.wallet_key == outcome.wallet_key)
                    .and_then(|wallet| wallet.tx_signature.as_deref())
                    .map(|existing| existing.trim() == signature.trim())
            })
            .unwrap_or(false);
        (
            batch.client_request_id.clone(),
            duplicate_owner,
            already_registered_signature,
        )
    };

    let mut register_trade_signature = if already_registered_signature {
        None
    } else {
        outcome.tx_signature.clone()
    };
    let mut confirmed_trade_event: Option<(String, Option<u64>)> = None;

    if let Some((owner_batch_id, owner_wallet_key)) = duplicate_owner {
        let signature = outcome.tx_signature.clone().unwrap_or_default();
        outcome.status = BatchLifecycleStatus::Failed;
        outcome.error = Some(duplicate_signature_error(
            &signature,
            &owner_batch_id,
            &owner_wallet_key,
        ));
        register_trade_signature = None;
    }

    if matches!(outcome.status, BatchLifecycleStatus::Confirmed)
        && let Some(tx_signature) = outcome.tx_signature.clone()
    {
        match record_confirmed_trade_ledger_entry(
            state,
            wallet_request,
            &outcome.wallet_key,
            &tx_signature,
            outcome.entry_preference_asset,
            &batch_client_request_id,
            batch_id,
        )
        .await
        {
            Ok(record_outcome) => match record_outcome.state {
                ConfirmedTradeLedgerRecordState::Recorded => {
                    confirmed_trade_event = Some((tx_signature.clone(), record_outcome.slot));
                }
                ConfirmedTradeLedgerRecordState::Duplicate => {
                    outcome.status = BatchLifecycleStatus::Failed;
                    outcome.error = Some(format!(
                        "Duplicate confirmed trade signature {tx_signature}; this request did not submit a distinct transaction."
                    ));
                    register_trade_signature = None;
                }
                ConfirmedTradeLedgerRecordState::Ignored => {
                    outcome.status = BatchLifecycleStatus::Failed;
                    outcome.error = Some(format!(
                        "Confirmed transaction {tx_signature} did not produce a ledger-applicable trade."
                    ));
                    register_trade_signature = None;
                }
            },
            Err(ConfirmedTradeLedgerRecordError::Validation(error)) => {
                outcome.status = BatchLifecycleStatus::Failed;
                outcome.error = Some(error);
                register_trade_signature = None;
            }
            Err(ConfirmedTradeLedgerRecordError::Persist(error)) => {
                eprintln!(
                    "failed to record confirmed trade ledger entry for {} {}: {}",
                    outcome.wallet_key, tx_signature, error
                );
                state.persist_failures.record_trade_ledger_failure(&error);
            }
        }
    }

    let mut guard = state.batches.write().await;
    let Some(batch) = guard.get_mut(batch_id) else {
        return;
    };

    if let Some(wallet) = batch
        .wallets
        .iter_mut()
        .find(|wallet| wallet.wallet_key == outcome.wallet_key)
    {
        wallet.status = outcome.status;
        wallet.tx_signature = outcome.tx_signature;
        wallet.error = outcome.error;
        wallet.entry_preference_asset = outcome.entry_preference_asset;
    } else {
        return;
    }

    recompute_batch_summary(batch);
    batch.updated_at_unix_ms = now;
    let batch_snapshot = batch.clone();
    if let Err((_, error)) = persist_batch_history(&state.batch_history_path, &guard) {
        eprintln!("[execution-engine][persist] batch history write failed: {error}");
        state.persist_failures.record_batch_history_failure(&error);
    }
    drop(guard);
    publish_batch_status_snapshot(state, &batch_snapshot, "wallet_outcome");

    // Register the signature with the balance stream so every surface receives
    // a trade event (and the stream subscribes to any follow-up status). The
    // executor returned a signature, which means the tx was at least submitted.
    if let Some(signature) = register_trade_signature {
        state.balance_stream.register_trade(
            batch_client_request_id.clone(),
            batch_id.to_string(),
            signature,
        );
    }

    if let Some((tx_signature, slot)) = confirmed_trade_event {
        publish_confirmed_trade_balance_stream_event(
            state,
            Some(&batch_client_request_id),
            Some(batch_id),
            &tx_signature,
            slot,
        );
    }
}

async fn record_confirmed_trade_ledger_entry(
    state: &AppState,
    wallet_request: &WalletTradeRequest,
    wallet_key: &str,
    tx_signature: &str,
    entry_preference_asset: Option<TradeSettlementAsset>,
    client_request_id: &str,
    batch_id: &str,
) -> Result<ConfirmedTradeLedgerRecordOutcome, ConfirmedTradeLedgerRecordError> {
    let wallet_public_key = wallet_public_key_for_trade_ledger(wallet_key)
        .map_err(ConfirmedTradeLedgerRecordError::Persist)?;
    let ledger_snapshot = fetch_wallet_trade_ledger_snapshot_for_signature(
        tx_signature,
        &wallet_public_key,
        &wallet_request.mint,
    )
    .await
    .map_err(ConfirmedTradeLedgerRecordError::Persist)?
    .ok_or_else(|| {
        ConfirmedTradeLedgerRecordError::Persist(format!(
            "Confirmed transaction {tx_signature} did not include wallet {wallet_key} mint {} token-balance metadata.",
            wallet_request.mint
        ))
    })?;
    validate_confirmed_trade_direction(
        &ledger_snapshot,
        &wallet_request.side,
        wallet_key,
        &wallet_request.mint,
        tx_signature,
    )
    .map_err(ConfirmedTradeLedgerRecordError::Validation)?;
    let bought_lamports = match wallet_request.side {
        TradeSide::Buy => resolve_confirmed_trade_notional_lamports(confirmed_buy_notional_source(
            &ledger_snapshot,
        ))
        .await
        .map_err(ConfirmedTradeLedgerRecordError::Persist)?,
        TradeSide::Sell => 0,
    };
    let sold_lamports = match wallet_request.side {
        TradeSide::Sell => resolve_confirmed_trade_notional_lamports(
            confirmed_sell_notional_source(&ledger_snapshot),
        )
        .await
        .map_err(ConfirmedTradeLedgerRecordError::Persist)?,
        TradeSide::Buy => 0,
    };
    let trade_value_lamports = match wallet_request.side {
        TradeSide::Buy => bought_lamports,
        TradeSide::Sell => sold_lamports,
    };
    let slot = ledger_snapshot.slot;
    let inferred_entry_preference =
        entry_preference_asset.or_else(|| inferred_entry_preference_asset(wallet_request));
    let params = RecordConfirmedTradeParams {
        wallet_key,
        wallet_public_key: &wallet_public_key,
        mint: &wallet_request.mint,
        side: wallet_request.side.clone(),
        trade_value_lamports,
        token_delta_raw: ledger_snapshot.token_delta_raw,
        token_decimals: ledger_snapshot.token_decimals,
        confirmed_at_unix_ms: ledger_snapshot
            .block_time_unix_ms
            .unwrap_or_else(now_unix_ms),
        slot: ledger_snapshot.slot,
        entry_preference_asset: inferred_entry_preference,
        settlement_asset: inferred_entry_preference
            .or(wallet_request_side_settlement_asset(wallet_request)),
        explicit_fees: ledger_snapshot.explicit_fees,
        platform_tag: platform_tag_from_label(wallet_request.platform_label.as_deref()),
        provenance: EventProvenance::LocalExecution,
        signature: tx_signature,
        client_request_id: Some(client_request_id),
        batch_id: Some(batch_id),
    };
    Ok(ConfirmedTradeLedgerRecordOutcome {
        state: persist_confirmed_trade_ledger_params(state, params)
            .await
            .map_err(ConfirmedTradeLedgerRecordError::Persist)?,
        slot,
    })
}

fn wallet_public_key_for_trade_ledger(wallet_key: &str) -> Result<String, String> {
    shared_config_manager()
        .current_snapshot()
        .wallets
        .iter()
        .find(|wallet| wallet.key == wallet_key)
        .map(|wallet| wallet.public_key.clone())
        .ok_or_else(|| format!("Unknown wallet key for trade ledger: {wallet_key}"))
}

async fn record_inferred_confirmed_trade_ledger_entry(
    state: &AppState,
    wallet_key: &str,
    tx_signature: &str,
    mint: &str,
    platform_tag: PlatformTag,
    provenance: EventProvenance,
    client_request_id: Option<&str>,
    batch_id: Option<&str>,
) -> Result<ConfirmedTradeLedgerRecordOutcome, String> {
    let wallet_public_key = wallet_public_key_for_trade_ledger(wallet_key)?;
    let ledger_snapshot =
        fetch_wallet_trade_ledger_snapshot_for_signature(tx_signature, &wallet_public_key, mint)
            .await?
            .ok_or_else(|| {
                format!(
                    "Confirmed transaction {tx_signature} did not include wallet {wallet_key} mint {mint} token-balance metadata."
                )
            })?;
    let side = if ledger_snapshot.token_delta_raw > 0 {
        TradeSide::Buy
    } else if ledger_snapshot.token_delta_raw < 0 {
        TradeSide::Sell
    } else {
        return Ok(ConfirmedTradeLedgerRecordOutcome {
            state: ConfirmedTradeLedgerRecordState::Ignored,
            slot: ledger_snapshot.slot,
        });
    };
    let trade_value_lamports = match side {
        TradeSide::Buy => {
            resolve_confirmed_trade_notional_lamports(confirmed_buy_notional_source(
                &ledger_snapshot,
            ))
            .await?
        }
        TradeSide::Sell => {
            resolve_confirmed_trade_notional_lamports(confirmed_sell_notional_source(
                &ledger_snapshot,
            ))
            .await?
        }
    };
    let slot = ledger_snapshot.slot;
    let settlement_asset = settlement_asset_from_snapshot(&ledger_snapshot, &side);
    let entry_preference_asset = if matches!(side, TradeSide::Buy) {
        settlement_asset
    } else {
        None
    };
    let params = RecordConfirmedTradeParams {
        wallet_key,
        wallet_public_key: &wallet_public_key,
        mint,
        side,
        trade_value_lamports,
        token_delta_raw: ledger_snapshot.token_delta_raw,
        token_decimals: ledger_snapshot.token_decimals,
        confirmed_at_unix_ms: ledger_snapshot
            .block_time_unix_ms
            .unwrap_or_else(now_unix_ms),
        slot: ledger_snapshot.slot,
        entry_preference_asset,
        settlement_asset,
        explicit_fees: ledger_snapshot.explicit_fees,
        platform_tag,
        provenance,
        signature: tx_signature,
        client_request_id,
        batch_id,
    };
    Ok(ConfirmedTradeLedgerRecordOutcome {
        state: persist_confirmed_trade_ledger_params(state, params).await?,
        slot,
    })
}

async fn persist_confirmed_trade_ledger_params(
    state: &AppState,
    params: RecordConfirmedTradeParams<'_>,
) -> Result<ConfirmedTradeLedgerRecordState, String> {
    let event = crate::trade_ledger::ConfirmedTradeEvent {
        schema_version: crate::trade_ledger::trade_ledger_schema_version(),
        signature: params.signature.to_string(),
        slot: params.slot,
        confirmed_at_unix_ms: params.confirmed_at_unix_ms,
        wallet_key: params.wallet_key.to_string(),
        wallet_public_key: params.wallet_public_key.to_string(),
        mint: params.mint.to_string(),
        side: params.side.clone(),
        platform_tag: params.platform_tag,
        provenance: params.provenance,
        settlement_asset: params.settlement_asset,
        token_delta_raw: params.token_delta_raw,
        token_decimals: params.token_decimals,
        trade_value_lamports: params.trade_value_lamports,
        explicit_fees: params.explicit_fees.clone(),
        client_request_id: params.client_request_id.map(str::to_string),
        batch_id: params.batch_id.map(str::to_string),
    };
    let event_id = event.event_id();
    {
        let mut known_event_ids = state.trade_ledger_event_ids.write().await;
        if !known_event_ids.insert(event_id.clone()) {
            return Ok(ConfirmedTradeLedgerRecordState::Duplicate);
        }
    }
    if let Err((_, error)) = append_confirmed_trade_event(&state.trade_ledger_paths, &event) {
        state.trade_ledger_event_ids.write().await.remove(&event_id);
        return Err(error);
    }
    let mut ledger = state.trade_ledger.write().await;
    record_confirmed_trade(&mut ledger, params);
    persist_trade_ledger(&state.trade_ledger_paths, &ledger).map_err(|(_, error)| error)?;
    Ok(ConfirmedTradeLedgerRecordState::Recorded)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmedTradeNotionalSource {
    Lamports(u64),
    Usd1Raw(u64),
}

fn inferred_entry_preference_asset(
    wallet_request: &WalletTradeRequest,
) -> Option<TradeSettlementAsset> {
    if !matches!(wallet_request.side, TradeSide::Buy) {
        return None;
    }
    match wallet_request.policy.buy_funding_policy {
        BuyFundingPolicy::SolOnly => Some(TradeSettlementAsset::Sol),
        BuyFundingPolicy::Usd1Only => Some(TradeSettlementAsset::Usd1),
        BuyFundingPolicy::PreferUsd1ElseTopUp => None,
    }
}

fn wallet_request_side_settlement_asset(
    wallet_request: &WalletTradeRequest,
) -> Option<TradeSettlementAsset> {
    match wallet_request.side {
        TradeSide::Buy => inferred_entry_preference_asset(wallet_request),
        TradeSide::Sell => Some(wallet_request.policy.sell_settlement_asset),
    }
}

#[derive(Debug, Clone, Default)]
struct ConfirmedTradeLedgerSnapshot {
    lamport_delta: i64,
    usd1_delta_raw: i128,
    token_delta_raw: i128,
    token_decimals: Option<u8>,
    slot: Option<u64>,
    block_time_unix_ms: Option<u64>,
    explicit_fees: ExplicitFeeBreakdown,
}

#[derive(Debug, Clone)]
struct RpcResyncCandidate {
    signature: String,
    wallet_key: String,
    wallet_public_key: String,
    snapshot: ConfirmedTradeLedgerSnapshot,
}

#[derive(Debug, Clone)]
struct HeliusResyncConfig {
    base_url: String,
    api_key: String,
}

#[derive(Debug, Clone)]
struct KnownWalletIdentity {
    wallet_key: String,
    public_key: String,
}

fn configured_helius_resync_config() -> Option<HeliusResyncConfig> {
    if let Ok(value) = std::env::var("HELIUS_API_KEY") {
        let api_key = value.trim();
        if !api_key.is_empty() {
            return Some(HeliusResyncConfig {
                base_url: "https://api-mainnet.helius-rpc.com".to_string(),
                api_key: api_key.to_string(),
            });
        }
    }

    let rpc_url = configured_rpc_url();
    if !rpc_url.to_ascii_lowercase().contains("helius") {
        return None;
    }
    let api_key = extract_query_param(&rpc_url, "api-key")
        .or_else(|| extract_query_param(&rpc_url, "api_key"))?;
    Some(HeliusResyncConfig {
        base_url: "https://api-mainnet.helius-rpc.com".to_string(),
        api_key,
    })
}

fn extract_query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key == name && !value.trim().is_empty() {
            return Some(value.trim().to_string());
        }
    }
    None
}

#[derive(Debug, Clone)]
enum RpcResyncLedgerAction {
    Trade(crate::trade_ledger::ConfirmedTradeEvent),
    ForceClose {
        marker: crate::trade_ledger::ForceCloseMarkerEvent,
        persist: bool,
    },
    KnownTransfer {
        marker: crate::trade_ledger::TokenTransferMarkerEvent,
        slot: Option<u64>,
    },
    ReceivedWithoutCostBasis {
        wallet_key: String,
        mint: String,
        amount_raw: u64,
        signature: String,
        applied_at_unix_ms: u64,
        slot: Option<u64>,
        persist: bool,
    },
    SentWithoutProceeds {
        wallet_key: String,
        mint: String,
        amount_raw: u64,
        signature: String,
        applied_at_unix_ms: u64,
        slot: Option<u64>,
        persist: bool,
    },
}

impl RpcResyncLedgerAction {
    fn slot_sort_key(&self) -> u64 {
        match self {
            RpcResyncLedgerAction::Trade(event) => event.slot.unwrap_or(u64::MAX),
            RpcResyncLedgerAction::ForceClose { .. } => u64::MAX,
            RpcResyncLedgerAction::KnownTransfer { slot, .. }
            | RpcResyncLedgerAction::ReceivedWithoutCostBasis { slot, .. }
            | RpcResyncLedgerAction::SentWithoutProceeds { slot, .. } => slot.unwrap_or(u64::MAX),
        }
    }

    fn applied_at_unix_ms(&self) -> u64 {
        match self {
            RpcResyncLedgerAction::Trade(event) => event.confirmed_at_unix_ms,
            RpcResyncLedgerAction::ForceClose { marker, .. } => marker.applied_at_unix_ms,
            RpcResyncLedgerAction::KnownTransfer { marker, .. } => marker.applied_at_unix_ms,
            RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                applied_at_unix_ms, ..
            }
            | RpcResyncLedgerAction::SentWithoutProceeds {
                applied_at_unix_ms, ..
            } => *applied_at_unix_ms,
        }
    }

    fn signature(&self) -> &str {
        match self {
            RpcResyncLedgerAction::Trade(event) => &event.signature,
            RpcResyncLedgerAction::ForceClose { marker, .. } => &marker.reason,
            RpcResyncLedgerAction::KnownTransfer { marker, .. } => &marker.signature,
            RpcResyncLedgerAction::ReceivedWithoutCostBasis { signature, .. }
            | RpcResyncLedgerAction::SentWithoutProceeds { signature, .. } => signature,
        }
    }

    fn wallet_mint_key(&self) -> String {
        match self {
            RpcResyncLedgerAction::Trade(event) => {
                trade_ledger_lookup_key(&event.wallet_key, &event.mint)
            }
            RpcResyncLedgerAction::ForceClose { marker, .. } => {
                trade_ledger_lookup_key(&marker.wallet_key, &marker.mint)
            }
            RpcResyncLedgerAction::KnownTransfer { marker, .. } => {
                trade_ledger_lookup_key(&marker.source_wallet_key, &marker.mint)
            }
            RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                wallet_key, mint, ..
            }
            | RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key, mint, ..
            } => trade_ledger_lookup_key(wallet_key, mint),
        }
    }

    fn replay_rank(&self) -> u8 {
        match self {
            RpcResyncLedgerAction::Trade(_) => 0,
            RpcResyncLedgerAction::KnownTransfer { .. } => 2,
            RpcResyncLedgerAction::ReceivedWithoutCostBasis { .. }
            | RpcResyncLedgerAction::SentWithoutProceeds { .. } => 3,
            RpcResyncLedgerAction::ForceClose { .. } => 4,
        }
    }
}

fn sort_resync_actions(actions: &mut [RpcResyncLedgerAction]) {
    actions.sort_by(|left, right| {
        left.applied_at_unix_ms()
            .cmp(&right.applied_at_unix_ms())
            .then_with(|| left.slot_sort_key().cmp(&right.slot_sort_key()))
            .then_with(|| left.wallet_mint_key().cmp(&right.wallet_mint_key()))
            .then_with(|| left.replay_rank().cmp(&right.replay_rank()))
            .then_with(|| left.signature().cmp(right.signature()))
            .then_with(|| resync_action_dedupe_key(left).cmp(&resync_action_dedupe_key(right)))
    });
}

fn validate_confirmed_trade_direction(
    snapshot: &ConfirmedTradeLedgerSnapshot,
    side: &TradeSide,
    wallet_key: &str,
    mint: &str,
    tx_signature: &str,
) -> Result<(), String> {
    match side {
        TradeSide::Buy if snapshot.token_delta_raw <= 0 => Err(format!(
            "Confirmed buy {tx_signature} did not increase token balance for wallet {wallet_key} mint {mint} (token delta {}).",
            snapshot.token_delta_raw
        )),
        TradeSide::Sell if snapshot.token_delta_raw >= 0 => Err(format!(
            "Confirmed sell {tx_signature} did not decrease token balance for wallet {wallet_key} mint {mint} (token delta {}).",
            snapshot.token_delta_raw
        )),
        _ => Ok(()),
    }
}

fn confirmed_buy_notional_source(
    snapshot: &ConfirmedTradeLedgerSnapshot,
) -> Option<ConfirmedTradeNotionalSource> {
    let effective_lamport_delta = snapshot
        .lamport_delta
        .saturating_add(snapshot.explicit_fees.total_lamports());
    if snapshot.usd1_delta_raw < 0 {
        u64::try_from(-snapshot.usd1_delta_raw)
            .ok()
            .map(ConfirmedTradeNotionalSource::Usd1Raw)
    } else if effective_lamport_delta < 0 {
        Some(ConfirmedTradeNotionalSource::Lamports(
            (-effective_lamport_delta) as u64,
        ))
    } else {
        None
    }
}

fn confirmed_sell_notional_source(
    snapshot: &ConfirmedTradeLedgerSnapshot,
) -> Option<ConfirmedTradeNotionalSource> {
    let effective_lamport_delta = snapshot
        .lamport_delta
        .saturating_add(snapshot.explicit_fees.total_lamports());
    if snapshot.usd1_delta_raw > 0 {
        u64::try_from(snapshot.usd1_delta_raw)
            .ok()
            .map(ConfirmedTradeNotionalSource::Usd1Raw)
    } else if effective_lamport_delta > 0 {
        Some(ConfirmedTradeNotionalSource::Lamports(
            effective_lamport_delta as u64,
        ))
    } else {
        None
    }
}

async fn resolve_confirmed_trade_notional_lamports(
    source: Option<ConfirmedTradeNotionalSource>,
) -> Result<u64, String> {
    match source {
        None => Ok(0),
        Some(ConfirmedTradeNotionalSource::Lamports(value)) => Ok(value),
        Some(ConfirmedTradeNotionalSource::Usd1Raw(value)) => {
            crate::bonk_native::quote_sol_lamports_for_exact_usd1_input(
                &configured_rpc_url(),
                value,
            )
            .await
        }
    }
}

fn token_balance_meta_matches_owner(balance: &Value, owner: &str) -> bool {
    balance
        .get("owner")
        .and_then(Value::as_str)
        .is_some_and(|value| value == owner)
}

fn token_balance_meta_matches_owner_and_mint(balance: &Value, owner: &str, mint: &str) -> bool {
    token_balance_meta_matches_owner(balance, owner)
        && balance
            .get("mint")
            .and_then(Value::as_str)
            .is_some_and(|value| value == mint)
}

fn token_balance_account_index_from_meta(balance: &Value) -> Option<usize> {
    balance
        .get("accountIndex")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn token_balance_amount_from_meta(balance: &Value) -> Option<u64> {
    balance
        .get("uiTokenAmount")
        .and_then(|value| value.get("amount"))
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<u64>().ok())
}

fn token_balance_decimals_from_meta(balance: &Value) -> Option<u8> {
    balance
        .get("uiTokenAmount")
        .and_then(|value| value.get("decimals"))
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
}

fn total_token_balance_amount_from_meta(
    balances: &[Value],
    owner: &str,
    mint: &str,
) -> Result<Option<u64>, String> {
    let mut total = 0u128;
    let mut found = false;
    for balance in balances {
        if token_balance_meta_matches_owner_and_mint(balance, owner, mint) {
            found = true;
            let amount = token_balance_amount_from_meta(balance).ok_or_else(|| {
                format!(
                    "Transaction token-balance metadata for wallet {owner} mint {mint} had an invalid amount."
                )
            })?;
            total = total.saturating_add(u128::from(amount));
        }
    }
    Ok(found.then_some(total.min(u128::from(u64::MAX)) as u64))
}

#[cfg(test)]
fn trade_token_delta_from_meta(
    pre_token_balances: &[Value],
    post_token_balances: &[Value],
    owner: &str,
    mint: &str,
) -> Result<(i128, Option<u8>), String> {
    maybe_trade_token_delta_from_meta(pre_token_balances, post_token_balances, owner, mint)?
        .ok_or_else(|| {
            format!(
                "Transaction token-balance metadata did not include wallet {owner} mint {mint}."
            )
        })
}

fn maybe_trade_token_delta_from_meta(
    pre_token_balances: &[Value],
    post_token_balances: &[Value],
    owner: &str,
    mint: &str,
) -> Result<Option<(i128, Option<u8>)>, String> {
    let pre_raw = total_token_balance_amount_from_meta(pre_token_balances, owner, mint)?;
    let post_raw = total_token_balance_amount_from_meta(post_token_balances, owner, mint)?;
    if pre_raw.is_none() && post_raw.is_none() {
        return Ok(None);
    }
    let token_decimals = post_token_balances
        .iter()
        .chain(pre_token_balances.iter())
        .find(|balance| token_balance_meta_matches_owner_and_mint(balance, owner, mint))
        .and_then(token_balance_decimals_from_meta);
    let effective_pre_raw = pre_raw.unwrap_or(0);
    let effective_post_raw = post_raw.or_else(|| pre_raw.map(|_| 0)).unwrap_or(0);
    Ok(Some((
        i128::from(effective_post_raw) - i128::from(effective_pre_raw),
        token_decimals,
    )))
}

fn helius_raw_token_amount(change: &Value, owner: &str, mint: &str) -> Result<i128, String> {
    let raw = change
        .get("rawTokenAmount")
        .and_then(|value| value.get("tokenAmount"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!("Helius token balance change for wallet {owner} mint {mint} had no raw amount.")
        })?;
    raw.trim().parse::<i128>().map_err(|error| {
        format!("Helius token balance change for wallet {owner} mint {mint} had invalid raw amount: {error}")
    })
}

fn helius_token_decimals(change: &Value) -> Option<u8> {
    change
        .get("rawTokenAmount")
        .and_then(|value| value.get("decimals"))
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
}

fn helius_token_delta_from_account_data(
    account_data: &[Value],
    owner: &str,
    mint: &str,
) -> Result<Option<(i128, Option<u8>)>, String> {
    let mut total = 0i128;
    let mut decimals = None;
    let mut found = false;
    for account in account_data {
        let Some(changes) = account.get("tokenBalanceChanges").and_then(Value::as_array) else {
            continue;
        };
        for change in changes {
            let matches_owner = change
                .get("userAccount")
                .and_then(Value::as_str)
                .is_some_and(|value| value == owner);
            let matches_mint = change
                .get("mint")
                .and_then(Value::as_str)
                .is_some_and(|value| value == mint);
            if !matches_owner || !matches_mint {
                continue;
            }
            found = true;
            total = total.saturating_add(helius_raw_token_amount(change, owner, mint)?);
            if decimals.is_none() {
                decimals = helius_token_decimals(change);
            }
        }
    }
    Ok(found.then_some((total, decimals)))
}

fn helius_native_delta_from_account_data(account_data: &[Value], owner: &str) -> i64 {
    account_data
        .iter()
        .find(|account| {
            account
                .get("account")
                .and_then(Value::as_str)
                .is_some_and(|value| value == owner)
        })
        .and_then(|account| account.get("nativeBalanceChange"))
        .and_then(|value| match value {
            Value::Number(number) => number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|raw| i64::try_from(raw).ok())),
            Value::String(raw) => raw.trim().parse::<i64>().ok(),
            _ => None,
        })
        .unwrap_or(0)
}

fn helius_explicit_fees(
    transaction: &Value,
    account_data: &[Value],
    wallet_public_key: &str,
    tracked_mint: &str,
) -> ExplicitFeeBreakdown {
    let fee_payer = transaction
        .get("feePayer")
        .and_then(Value::as_str)
        .unwrap_or_default();
    ExplicitFeeBreakdown {
        network_fee_lamports: if fee_payer == wallet_public_key {
            transaction
                .get("fee")
                .and_then(Value::as_u64)
                .unwrap_or_default()
        } else {
            0
        },
        tip_lamports: helius_tip_lamports(transaction, wallet_public_key),
        rent_delta_lamports: helius_wallet_owned_token_account_rent_delta_lamports(
            account_data,
            wallet_public_key,
            tracked_mint,
        ),
        ..ExplicitFeeBreakdown::default()
    }
}

fn helius_tip_lamports(transaction: &Value, wallet_public_key: &str) -> u64 {
    let Some(transfers) = transaction.get("nativeTransfers").and_then(Value::as_array) else {
        return 0;
    };
    transfers.iter().fold(0u64, |sum, transfer| {
        let from_wallet = transfer
            .get("fromUserAccount")
            .and_then(Value::as_str)
            .is_some_and(|value| value == wallet_public_key);
        let to_tip_account = transfer
            .get("toUserAccount")
            .and_then(Value::as_str)
            .is_some_and(|value| recognized_tip_accounts().contains(value));
        if from_wallet && to_tip_account {
            sum.saturating_add(value_as_u64_loose(transfer.get("amount")).unwrap_or_default())
        } else {
            sum
        }
    })
}

fn helius_wallet_owned_token_account_rent_delta_lamports(
    account_data: &[Value],
    owner: &str,
    mint: &str,
) -> i64 {
    let mut total = 0i128;
    for account in account_data {
        let Some(account_key) = account.get("account").and_then(Value::as_str) else {
            continue;
        };
        if account_key == owner {
            continue;
        }
        let Some(native_delta) = account
            .get("nativeBalanceChange")
            .and_then(|value| match value {
                Value::Number(number) => number
                    .as_i64()
                    .or_else(|| number.as_u64().and_then(|raw| i64::try_from(raw).ok())),
                Value::String(raw) => raw.trim().parse::<i64>().ok(),
                _ => None,
            })
        else {
            continue;
        };
        let owns_relevant_mint = account
            .get("tokenBalanceChanges")
            .and_then(Value::as_array)
            .is_some_and(|changes| {
                changes.iter().any(|change| {
                    change
                        .get("userAccount")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value == owner)
                        && change
                            .get("mint")
                            .and_then(Value::as_str)
                            .is_some_and(|value| value == mint || value == USD1_MINT)
                        && change
                            .get("tokenAccount")
                            .and_then(Value::as_str)
                            .map_or(true, |value| value == account_key)
                })
            });
        if owns_relevant_mint {
            total = total.saturating_add(i128::from(native_delta));
        }
    }
    total.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn push_selected_token_only_movement_action(
    actions: &mut Vec<RpcResyncLedgerAction>,
    movement: TokenOnlyMovement,
    selected_wallet_keys: &HashSet<String>,
    is_incoming: bool,
) {
    if !selected_wallet_keys.contains(&movement.wallet_key) || movement.amount_raw == 0 {
        return;
    }
    if is_incoming {
        actions.push(RpcResyncLedgerAction::ReceivedWithoutCostBasis {
            wallet_key: movement.wallet_key,
            mint: movement.mint,
            amount_raw: movement.amount_raw,
            signature: movement.signature,
            applied_at_unix_ms: movement.applied_at_unix_ms,
            slot: movement.slot,
            persist: true,
        });
    } else {
        actions.push(RpcResyncLedgerAction::SentWithoutProceeds {
            wallet_key: movement.wallet_key,
            mint: movement.mint,
            amount_raw: movement.amount_raw,
            signature: movement.signature,
            applied_at_unix_ms: movement.applied_at_unix_ms,
            slot: movement.slot,
            persist: true,
        });
    }
}

fn push_clear_incomplete_movement_action(
    actions: &mut Vec<RpcResyncLedgerAction>,
    wallet_key: &str,
    mint: &str,
    signature: &str,
    applied_at_unix_ms: u64,
    slot: Option<u64>,
    is_incoming: bool,
) {
    if is_incoming {
        actions.push(RpcResyncLedgerAction::ReceivedWithoutCostBasis {
            wallet_key: wallet_key.to_string(),
            mint: mint.to_string(),
            amount_raw: 0,
            signature: signature.to_string(),
            applied_at_unix_ms,
            slot,
            persist: true,
        });
    } else {
        actions.push(RpcResyncLedgerAction::SentWithoutProceeds {
            wallet_key: wallet_key.to_string(),
            mint: mint.to_string(),
            amount_raw: 0,
            signature: signature.to_string(),
            applied_at_unix_ms,
            slot,
            persist: true,
        });
    }
}

fn transfer_incomplete_signature(
    signature: &str,
    wallet_key: &str,
    counterparty_wallet_key: &str,
    is_incoming: bool,
) -> String {
    format!(
        "{}:incomplete:{}:{}:{}",
        signature.trim(),
        if is_incoming {
            "received_from"
        } else {
            "sent_to"
        },
        wallet_key.trim(),
        counterparty_wallet_key.trim()
    )
}

fn helius_candidate_from_transaction(
    transaction: &Value,
    wallet_key: &str,
    wallet_public_key: &str,
    mint: &str,
) -> Result<Option<RpcResyncCandidate>, String> {
    if transaction
        .get("transactionError")
        .is_some_and(|value| !value.is_null())
    {
        return Ok(None);
    }
    let signature = transaction
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| "Helius transaction did not include a signature.".to_string())?;
    let account_data = transaction
        .get("accountData")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some((token_delta_raw, token_decimals)) =
        helius_token_delta_from_account_data(&account_data, wallet_public_key, mint)?
    else {
        return Ok(None);
    };
    if token_delta_raw == 0 {
        return Ok(None);
    }
    let usd1_delta_raw =
        helius_token_delta_from_account_data(&account_data, wallet_public_key, USD1_MINT)?
            .map(|(delta, _)| delta)
            .unwrap_or(0);
    Ok(Some(RpcResyncCandidate {
        signature: signature.to_string(),
        wallet_key: wallet_key.to_string(),
        wallet_public_key: wallet_public_key.to_string(),
        snapshot: ConfirmedTradeLedgerSnapshot {
            lamport_delta: helius_native_delta_from_account_data(&account_data, wallet_public_key),
            usd1_delta_raw,
            token_delta_raw,
            token_decimals,
            slot: transaction.get("slot").and_then(Value::as_u64),
            block_time_unix_ms: transaction
                .get("timestamp")
                .and_then(Value::as_i64)
                .and_then(|value| u64::try_from(value).ok())
                .map(|value| value.saturating_mul(1_000)),
            explicit_fees: helius_explicit_fees(
                transaction,
                &account_data,
                wallet_public_key,
                mint,
            ),
        },
    }))
}

fn value_as_u64_loose(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(raw)) => raw
            .as_u64()
            .or_else(|| raw.as_i64().and_then(|signed| u64::try_from(signed).ok())),
        Some(Value::String(raw)) => raw.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn parsed_instruction_type(instruction: &Value) -> Option<&str> {
    instruction
        .get("parsed")
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
}

fn parsed_instruction_info(instruction: &Value) -> Option<&Value> {
    instruction
        .get("parsed")
        .and_then(|value| value.get("info"))
}

fn recognized_tip_accounts() -> &'static HashSet<String> {
    static RECOGNIZED_TIP_ACCOUNTS: std::sync::OnceLock<HashSet<String>> =
        std::sync::OnceLock::new();
    RECOGNIZED_TIP_ACCOUNTS.get_or_init(|| {
        let mut accounts: HashSet<String> = crate::provider_tip::all_known_tip_accounts()
            .map(str::to_string)
            .collect();
        for key in [
            "EXECUTION_ENGINE_JITO_TIP_ACCOUNT",
            "JITO_TIP_ACCOUNT",
            "EXECUTION_ENGINE_HELIUS_SENDER_TIP_ACCOUNT",
            "EXECUTION_ENGINE_HELLOMOON_TIP_ACCOUNT",
        ] {
            let Ok(value) = std::env::var(key) else {
                continue;
            };
            for entry in
                value.split(|character: char| character == ',' || character.is_whitespace())
            {
                let trimmed = entry.trim();
                if !trimmed.is_empty() {
                    accounts.insert(trimmed.to_string());
                }
            }
        }
        accounts
    })
}

fn priority_fee_lamports_from_compute_budget(
    compute_unit_limit: Option<u64>,
    compute_unit_price_micro_lamports: Option<u64>,
) -> u64 {
    let Some(limit) = compute_unit_limit else {
        return 0;
    };
    let Some(price) = compute_unit_price_micro_lamports else {
        return 0;
    };
    let product = u128::from(limit).saturating_mul(u128::from(price));
    let rounded_up = product.saturating_add(999_999) / 1_000_000;
    rounded_up.min(u128::from(u64::MAX)) as u64
}

fn wallet_owned_token_account_rent_delta_lamports(
    result: &Value,
    wallet_public_key: &str,
    tracked_mint: &str,
) -> i64 {
    let pre_token_balances = result
        .get("meta")
        .and_then(|value| value.get("preTokenBalances"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let post_token_balances = result
        .get("meta")
        .and_then(|value| value.get("postTokenBalances"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pre_balances = result
        .get("meta")
        .and_then(|value| value.get("preBalances"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let post_balances = result
        .get("meta")
        .and_then(|value| value.get("postBalances"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut account_indices = HashSet::new();
    for balance in pre_token_balances.iter().chain(post_token_balances.iter()) {
        let Some(mint) = balance.get("mint").and_then(Value::as_str) else {
            continue;
        };
        if mint == WRAPPED_SOL_MINT {
            continue;
        }
        if mint != tracked_mint && mint != USD1_MINT {
            continue;
        }
        if !token_balance_meta_matches_owner(balance, wallet_public_key) {
            continue;
        }
        let Some(account_index) = token_balance_account_index_from_meta(balance) else {
            continue;
        };
        account_indices.insert(account_index);
    }
    let rent_delta = account_indices
        .into_iter()
        .fold(0i128, |sum, account_index| {
            let pre_balance = pre_balances
                .get(account_index)
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let post_balance = post_balances
                .get(account_index)
                .and_then(Value::as_u64)
                .unwrap_or(0);
            sum.saturating_add(i128::from(post_balance) - i128::from(pre_balance))
        });
    rent_delta.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn explicit_fee_breakdown_from_transaction(
    result: &Value,
    wallet_public_key: &str,
    tracked_mint: &str,
) -> ExplicitFeeBreakdown {
    let total_network_plus_priority = result
        .get("meta")
        .and_then(|value| value.get("fee"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let tip_accounts = recognized_tip_accounts();
    let mut compute_unit_limit = None;
    let mut compute_unit_price_micro_lamports = None;
    let mut tip_lamports = 0u64;

    let mut process = |instruction: &Value| {
        let program_id = instruction
            .get("programId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match program_id {
            COMPUTE_BUDGET_PROGRAM_ID => match parsed_instruction_type(instruction) {
                Some("setComputeUnitLimit") => {
                    compute_unit_limit = value_as_u64_loose(
                        parsed_instruction_info(instruction).and_then(|info| info.get("units")),
                    );
                }
                Some("setComputeUnitPrice") => {
                    compute_unit_price_micro_lamports = value_as_u64_loose(
                        parsed_instruction_info(instruction)
                            .and_then(|info| info.get("microLamports"))
                            .or_else(|| {
                                parsed_instruction_info(instruction)
                                    .and_then(|info| info.get("micro_lamports"))
                            }),
                    );
                }
                _ => {}
            },
            SYSTEM_PROGRAM_ID => {
                if !matches!(parsed_instruction_type(instruction), Some("transfer")) {
                    return;
                }
                let Some(info) = parsed_instruction_info(instruction) else {
                    return;
                };
                let source = info
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let destination = info
                    .get("destination")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if source != wallet_public_key || !tip_accounts.contains(destination) {
                    return;
                }
                tip_lamports = tip_lamports
                    .saturating_add(value_as_u64_loose(info.get("lamports")).unwrap_or_default());
            }
            _ => {}
        }
    };

    if let Some(instructions) = result
        .get("transaction")
        .and_then(|value| value.get("message"))
        .and_then(|value| value.get("instructions"))
        .and_then(Value::as_array)
    {
        for instruction in instructions {
            process(instruction);
        }
    }

    if let Some(inner_groups) = result
        .get("meta")
        .and_then(|value| value.get("innerInstructions"))
        .and_then(Value::as_array)
    {
        for group in inner_groups {
            let Some(inner_instructions) = group.get("instructions").and_then(Value::as_array)
            else {
                continue;
            };
            for instruction in inner_instructions {
                process(instruction);
            }
        }
    }

    let priority_fee_lamports = priority_fee_lamports_from_compute_budget(
        compute_unit_limit,
        compute_unit_price_micro_lamports,
    )
    .min(total_network_plus_priority);
    ExplicitFeeBreakdown {
        network_fee_lamports: total_network_plus_priority.saturating_sub(priority_fee_lamports),
        priority_fee_lamports,
        tip_lamports,
        rent_delta_lamports: wallet_owned_token_account_rent_delta_lamports(
            result,
            wallet_public_key,
            tracked_mint,
        ),
        ..ExplicitFeeBreakdown::default()
    }
}

async fn fetch_wallet_trade_ledger_snapshot_for_signature(
    tx_signature: &str,
    wallet_public_key: &str,
    mint: &str,
) -> Result<Option<ConfirmedTradeLedgerSnapshot>, String> {
    let client = extension_wallet_rpc_client()?;
    for _attempt in 0..3 {
        let result = crate::rpc_client::rpc_request_with_client(
            &client,
            &configured_rpc_url(),
            "getTransaction",
            json!([
                tx_signature,
                {
                    "encoding": "jsonParsed",
                    "commitment": "confirmed",
                    "maxSupportedTransactionVersion": 0,
                }
            ]),
        )
        .await?;
        if result.is_null() {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            continue;
        }
        let account_keys = result
            .get("transaction")
            .and_then(|value| value.get("message"))
            .and_then(|value| value.get("accountKeys"))
            .and_then(Value::as_array)
            .ok_or_else(|| "Transaction message did not include account keys.".to_string())?;
        let account_index = account_keys
            .iter()
            .position(|entry| {
                entry.as_str() == Some(wallet_public_key)
                    || entry
                        .get("pubkey")
                        .and_then(Value::as_str)
                        .is_some_and(|pubkey| pubkey == wallet_public_key)
            })
            .ok_or_else(|| {
                format!(
                    "Transaction {} did not include wallet {} in its account keys.",
                    tx_signature, wallet_public_key
                )
            })?;
        let pre_balance = result
            .get("meta")
            .and_then(|value| value.get("preBalances"))
            .and_then(Value::as_array)
            .and_then(|items| items.get(account_index))
            .and_then(Value::as_u64)
            .ok_or_else(|| "Transaction meta did not include a pre-balance.".to_string())?;
        let post_balance = result
            .get("meta")
            .and_then(|value| value.get("postBalances"))
            .and_then(Value::as_array)
            .and_then(|items| items.get(account_index))
            .and_then(Value::as_u64)
            .ok_or_else(|| "Transaction meta did not include a post-balance.".to_string())?;
        let pre_token_balances = result
            .get("meta")
            .and_then(|value| value.get("preTokenBalances"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let post_token_balances = result
            .get("meta")
            .and_then(|value| value.get("postTokenBalances"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let Some((token_delta_raw, token_decimals)) = maybe_trade_token_delta_from_meta(
            &pre_token_balances,
            &post_token_balances,
            wallet_public_key,
            mint,
        )?
        else {
            return Ok(None);
        };
        return Ok(Some(ConfirmedTradeLedgerSnapshot {
            lamport_delta: post_balance as i64 - pre_balance as i64,
            usd1_delta_raw: i128::from(
                total_token_balance_amount_from_meta(
                    &post_token_balances,
                    wallet_public_key,
                    USD1_MINT,
                )?
                .unwrap_or(0),
            ) - i128::from(
                total_token_balance_amount_from_meta(
                    &pre_token_balances,
                    wallet_public_key,
                    USD1_MINT,
                )?
                .unwrap_or(0),
            ),
            token_delta_raw,
            token_decimals,
            slot: result.get("slot").and_then(Value::as_u64),
            block_time_unix_ms: result
                .get("blockTime")
                .and_then(Value::as_i64)
                .and_then(|value| u64::try_from(value).ok())
                .map(|value| value.saturating_mul(1000)),
            explicit_fees: explicit_fee_breakdown_from_transaction(
                &result,
                wallet_public_key,
                mint,
            ),
        }));
    }
    Err(format!(
        "Confirmed transaction {} was not yet available for ledger inspection.",
        tx_signature
    ))
}

fn settlement_asset_from_snapshot(
    snapshot: &ConfirmedTradeLedgerSnapshot,
    side: &TradeSide,
) -> Option<TradeSettlementAsset> {
    match side {
        TradeSide::Buy => {
            if snapshot.usd1_delta_raw < 0 {
                Some(TradeSettlementAsset::Usd1)
            } else if snapshot.lamport_delta < 0 {
                Some(TradeSettlementAsset::Sol)
            } else {
                None
            }
        }
        TradeSide::Sell => {
            if snapshot.usd1_delta_raw > 0 {
                Some(TradeSettlementAsset::Usd1)
            } else if snapshot.lamport_delta > 0 {
                Some(TradeSettlementAsset::Sol)
            } else {
                None
            }
        }
    }
}

fn rpc_resync_event_id(signature: &str, wallet_key: &str, mint: &str, side: &TradeSide) -> String {
    format!(
        "{}::{}::{}::{}",
        signature.trim(),
        wallet_key.trim(),
        mint.trim(),
        match side {
            TradeSide::Buy => "buy",
            TradeSide::Sell => "sell",
        }
    )
}

fn enrich_rpc_resync_event_from_local(
    mut event: crate::trade_ledger::ConfirmedTradeEvent,
    local_events_by_id: &HashMap<String, crate::trade_ledger::ConfirmedTradeEvent>,
) -> crate::trade_ledger::ConfirmedTradeEvent {
    if let Some(local) = local_events_by_id.get(&event.event_id()) {
        event.platform_tag = local.platform_tag;
        event.provenance = local.provenance;
        event.client_request_id = local.client_request_id.clone();
        event.batch_id = local.batch_id.clone();
    }
    event
}

fn merge_missing_local_resync_events(
    actions: &mut Vec<RpcResyncLedgerAction>,
    local_events: &[crate::trade_ledger::ConfirmedTradeEvent],
) {
    let mut known_event_ids = actions
        .iter()
        .filter_map(|action| match action {
            RpcResyncLedgerAction::Trade(event) => Some(event.event_id()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    actions.extend(
        local_events
            .iter()
            .filter(|event| known_event_ids.insert(event.event_id()))
            .cloned()
            .map(RpcResyncLedgerAction::Trade),
    );
}

fn resync_action_dedupe_key(action: &RpcResyncLedgerAction) -> String {
    match action {
        RpcResyncLedgerAction::Trade(event) => format!("trade::{}", event.event_id()),
        RpcResyncLedgerAction::ForceClose { marker, .. } => format!(
            "force-close::{}::{}::{}::{}",
            marker.wallet_key.trim(),
            marker.mint.trim(),
            marker.applied_at_unix_ms,
            marker.reason.trim()
        ),
        RpcResyncLedgerAction::KnownTransfer { marker, .. } => {
            format!("token-transfer::{}", marker.event_id())
        }
        RpcResyncLedgerAction::ReceivedWithoutCostBasis {
            wallet_key,
            mint,
            amount_raw,
            signature,
            slot,
            ..
        } => format!(
            "incomplete::received_without_cost_basis::{wallet_key}::{mint}::{signature}::{amount_raw}::{}",
            slot.unwrap_or(u64::MAX)
        ),
        RpcResyncLedgerAction::SentWithoutProceeds {
            wallet_key,
            mint,
            amount_raw,
            signature,
            slot,
            ..
        } => format!(
            "incomplete::sent_without_proceeds::{wallet_key}::{mint}::{signature}::{amount_raw}::{}",
            slot.unwrap_or(u64::MAX)
        ),
    }
}

fn marker_is_after_resync_baseline(
    wallet_key: &str,
    applied_at_unix_ms: u64,
    slot: Option<u64>,
    reset_baselines_by_wallet: &HashMap<String, (u64, Option<u64>)>,
) -> bool {
    let (reset_baseline_unix_ms, reset_baseline_slot) = reset_baselines_by_wallet
        .get(wallet_key)
        .copied()
        .unwrap_or((0, None));
    crate::trade_ledger::trade_event_is_after_reset_baseline(
        applied_at_unix_ms,
        slot,
        reset_baseline_unix_ms,
        reset_baseline_slot,
    )
}

fn incomplete_marker_resync_key(
    marker: &crate::trade_ledger::IncompleteBalanceAdjustmentMarkerEvent,
) -> String {
    match marker.adjustment_kind {
        crate::trade_ledger::IncompleteBalanceAdjustmentKind::ReceivedWithoutCostBasis => format!(
            "received_without_cost_basis::{}::{}::{}",
            marker.wallet_key, marker.mint, marker.signature
        ),
        crate::trade_ledger::IncompleteBalanceAdjustmentKind::SentWithoutProceeds => format!(
            "sent_without_proceeds::{}::{}::{}",
            marker.wallet_key, marker.mint, marker.signature
        ),
    }
}

fn incomplete_marker_order_key(
    marker: &crate::trade_ledger::IncompleteBalanceAdjustmentMarkerEvent,
) -> (u64, u64) {
    (marker.slot.unwrap_or(0), marker.applied_at_unix_ms)
}

fn incomplete_marker_to_resync_action(
    marker: crate::trade_ledger::IncompleteBalanceAdjustmentMarkerEvent,
) -> RpcResyncLedgerAction {
    match marker.adjustment_kind {
        crate::trade_ledger::IncompleteBalanceAdjustmentKind::ReceivedWithoutCostBasis => {
            RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                wallet_key: marker.wallet_key,
                mint: marker.mint,
                amount_raw: marker.amount_raw,
                signature: marker.signature,
                applied_at_unix_ms: marker.applied_at_unix_ms,
                slot: marker.slot,
                persist: false,
            }
        }
        crate::trade_ledger::IncompleteBalanceAdjustmentKind::SentWithoutProceeds => {
            RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key: marker.wallet_key,
                mint: marker.mint,
                amount_raw: marker.amount_raw,
                signature: marker.signature,
                applied_at_unix_ms: marker.applied_at_unix_ms,
                slot: marker.slot,
                persist: false,
            }
        }
    }
}

fn incomplete_marker_to_persisted_clear_action(
    marker: crate::trade_ledger::IncompleteBalanceAdjustmentMarkerEvent,
) -> RpcResyncLedgerAction {
    match marker.adjustment_kind {
        crate::trade_ledger::IncompleteBalanceAdjustmentKind::ReceivedWithoutCostBasis => {
            RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                wallet_key: marker.wallet_key,
                mint: marker.mint,
                amount_raw: 0,
                signature: marker.signature,
                applied_at_unix_ms: marker.applied_at_unix_ms,
                slot: marker.slot,
                persist: true,
            }
        }
        crate::trade_ledger::IncompleteBalanceAdjustmentKind::SentWithoutProceeds => {
            RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key: marker.wallet_key,
                mint: marker.mint,
                amount_raw: 0,
                signature: marker.signature,
                applied_at_unix_ms: marker.applied_at_unix_ms,
                slot: marker.slot,
                persist: true,
            }
        }
    }
}

fn merge_scoped_local_resync_markers(
    actions: &mut Vec<RpcResyncLedgerAction>,
    journal_entries: &[JournalEntry],
    selected_wallet_keys: &HashSet<String>,
    mint: &str,
    reset_baselines_by_wallet: &HashMap<String, (u64, Option<u64>)>,
) {
    let mut known_actions = actions
        .iter()
        .map(resync_action_dedupe_key)
        .collect::<HashSet<_>>();
    let mut transfer_shadowed_incomplete_keys = HashSet::new();
    let mut marker_actions = Vec::new();
    let mut latest_incomplete_markers = HashMap::new();

    for entry in journal_entries {
        match entry {
            JournalEntry::ForceCloseMarker(marker)
                if marker.mint == mint
                    && selected_wallet_keys.contains(&marker.wallet_key)
                    && marker_is_after_resync_baseline(
                        &marker.wallet_key,
                        marker.applied_at_unix_ms,
                        None,
                        reset_baselines_by_wallet,
                    ) =>
            {
                marker_actions.push(RpcResyncLedgerAction::ForceClose {
                    marker: marker.clone(),
                    persist: false,
                });
            }
            JournalEntry::TokenTransferMarker(marker) if marker.mint == mint => {
                let source_selected = selected_wallet_keys.contains(&marker.source_wallet_key);
                let destination_selected =
                    selected_wallet_keys.contains(&marker.destination_wallet_key);
                if !source_selected && !destination_selected {
                    continue;
                }
                if source_selected
                    && !marker_is_after_resync_baseline(
                        &marker.source_wallet_key,
                        marker.applied_at_unix_ms,
                        marker.slot,
                        reset_baselines_by_wallet,
                    )
                {
                    continue;
                }
                if destination_selected
                    && !marker_is_after_resync_baseline(
                        &marker.destination_wallet_key,
                        marker.applied_at_unix_ms,
                        marker.slot,
                        reset_baselines_by_wallet,
                    )
                {
                    continue;
                }

                let sent_signature = transfer_incomplete_signature(
                    &marker.signature,
                    &marker.source_wallet_key,
                    &marker.destination_wallet_key,
                    false,
                );
                transfer_shadowed_incomplete_keys.insert(format!(
                    "sent_without_proceeds::{}::{}::{}",
                    marker.source_wallet_key, marker.mint, sent_signature
                ));
                let received_signature = transfer_incomplete_signature(
                    &marker.signature,
                    &marker.destination_wallet_key,
                    &marker.source_wallet_key,
                    true,
                );
                transfer_shadowed_incomplete_keys.insert(format!(
                    "received_without_cost_basis::{}::{}::{}",
                    marker.destination_wallet_key, marker.mint, received_signature
                ));

                match (source_selected, destination_selected) {
                    (true, true) => marker_actions.push(RpcResyncLedgerAction::KnownTransfer {
                        marker: marker.clone(),
                        slot: marker.slot,
                    }),
                    (true, false) => {
                        marker_actions.push(RpcResyncLedgerAction::SentWithoutProceeds {
                            wallet_key: marker.source_wallet_key.clone(),
                            mint: marker.mint.clone(),
                            amount_raw: marker.amount_raw,
                            signature: sent_signature,
                            applied_at_unix_ms: marker.applied_at_unix_ms,
                            slot: marker.slot,
                            persist: false,
                        });
                    }
                    (false, true) => {
                        marker_actions.push(RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                            wallet_key: marker.destination_wallet_key.clone(),
                            mint: marker.mint.clone(),
                            amount_raw: marker.amount_raw,
                            signature: received_signature,
                            applied_at_unix_ms: marker.applied_at_unix_ms,
                            slot: marker.slot,
                            persist: false,
                        });
                    }
                    (false, false) => {}
                }
            }
            JournalEntry::IncompleteBalanceAdjustmentMarker(marker)
                if marker.mint == mint
                    && selected_wallet_keys.contains(&marker.wallet_key)
                    && !marker
                        .signature
                        .trim()
                        .starts_with("resync-balance-reconcile:")
                    && marker_is_after_resync_baseline(
                        &marker.wallet_key,
                        marker.applied_at_unix_ms,
                        marker.slot,
                        reset_baselines_by_wallet,
                    ) =>
            {
                let clear_scope = incomplete_marker_resync_key(marker);
                if marker.amount_raw == 0 {
                    latest_incomplete_markers.retain(|_, existing| {
                        incomplete_marker_resync_key(existing) != clear_scope
                    });
                    latest_incomplete_markers.insert(marker.event_id(), marker.clone());
                    continue;
                }
                let event_id = marker.event_id();
                let should_replace =
                    latest_incomplete_markers
                        .get(&event_id)
                        .map_or(true, |existing| {
                            incomplete_marker_order_key(marker)
                                >= incomplete_marker_order_key(existing)
                        });
                if should_replace {
                    latest_incomplete_markers.insert(event_id, marker.clone());
                }
            }
            _ => {}
        }
    }

    for marker in latest_incomplete_markers.into_values() {
        if transfer_shadowed_incomplete_keys.contains(&incomplete_marker_resync_key(&marker)) {
            marker_actions.push(incomplete_marker_to_persisted_clear_action(marker));
        } else {
            marker_actions.push(incomplete_marker_to_resync_action(marker));
        }
    }

    actions.extend(
        marker_actions
            .into_iter()
            .filter(|action| known_actions.insert(resync_action_dedupe_key(action))),
    );
}

#[derive(Debug, Clone)]
struct TokenOnlyMovement {
    wallet_key: String,
    mint: String,
    amount_raw: u64,
    signature: String,
    applied_at_unix_ms: u64,
    slot: Option<u64>,
}

async fn build_rpc_resync_ledger_actions(
    candidates: Vec<RpcResyncCandidate>,
    mint: &str,
    selected_wallet_keys: &HashSet<String>,
    local_events_by_id: &HashMap<String, crate::trade_ledger::ConfirmedTradeEvent>,
) -> Result<Vec<RpcResyncLedgerAction>, (StatusCode, String)> {
    let mut actions = Vec::new();
    let mut token_only_by_signature: BTreeMap<String, Vec<RpcResyncCandidate>> = BTreeMap::new();

    for candidate in candidates {
        let snapshot = &candidate.snapshot;
        let side = if snapshot.token_delta_raw > 0 {
            TradeSide::Buy
        } else if snapshot.token_delta_raw < 0 {
            TradeSide::Sell
        } else {
            continue;
        };
        let notional_source = match side {
            TradeSide::Buy => confirmed_buy_notional_source(snapshot),
            TradeSide::Sell => confirmed_sell_notional_source(snapshot),
        };
        let Some(notional_source) = notional_source else {
            token_only_by_signature
                .entry(candidate.signature.clone())
                .or_default()
                .push(candidate);
            continue;
        };
        if !selected_wallet_keys.contains(&candidate.wallet_key) {
            continue;
        }
        let trade_value_lamports = resolve_confirmed_trade_notional_lamports(Some(notional_source))
            .await
            .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
        let mut event = crate::trade_ledger::ConfirmedTradeEvent {
            schema_version: crate::trade_ledger::trade_ledger_schema_version(),
            signature: candidate.signature.clone(),
            slot: snapshot.slot,
            confirmed_at_unix_ms: snapshot.block_time_unix_ms.unwrap_or_else(now_unix_ms),
            wallet_key: candidate.wallet_key.clone(),
            wallet_public_key: candidate.wallet_public_key.clone(),
            mint: mint.to_string(),
            side: side.clone(),
            platform_tag: PlatformTag::Unknown,
            provenance: EventProvenance::RpcResync,
            settlement_asset: settlement_asset_from_snapshot(snapshot, &side),
            token_delta_raw: snapshot.token_delta_raw,
            token_decimals: snapshot.token_decimals,
            trade_value_lamports,
            explicit_fees: snapshot.explicit_fees.clone(),
            client_request_id: None,
            batch_id: None,
        };
        if let Some(local) = local_events_by_id.get(&rpc_resync_event_id(
            &event.signature,
            &event.wallet_key,
            &event.mint,
            &event.side,
        )) {
            event.platform_tag = local.platform_tag;
            event.provenance = local.provenance;
            event.client_request_id = local.client_request_id.clone();
            event.batch_id = local.batch_id.clone();
        }
        actions.push(RpcResyncLedgerAction::Trade(
            enrich_rpc_resync_event_from_local(event, local_events_by_id),
        ));
    }

    for (signature, movements) in token_only_by_signature {
        let mut outgoing = Vec::new();
        let mut incoming = Vec::new();
        for candidate in movements {
            let applied_at_unix_ms = candidate
                .snapshot
                .block_time_unix_ms
                .unwrap_or_else(now_unix_ms);
            if candidate.snapshot.token_delta_raw > 0 {
                incoming.push(TokenOnlyMovement {
                    wallet_key: candidate.wallet_key,
                    mint: mint.to_string(),
                    amount_raw: u64::try_from(candidate.snapshot.token_delta_raw)
                        .unwrap_or(u64::MAX),
                    signature: signature.clone(),
                    applied_at_unix_ms,
                    slot: candidate.snapshot.slot,
                });
            } else if candidate.snapshot.token_delta_raw < 0 {
                outgoing.push(TokenOnlyMovement {
                    wallet_key: candidate.wallet_key,
                    mint: mint.to_string(),
                    amount_raw: u64::try_from(-candidate.snapshot.token_delta_raw)
                        .unwrap_or(u64::MAX),
                    signature: signature.clone(),
                    applied_at_unix_ms,
                    slot: candidate.snapshot.slot,
                });
            }
        }

        if outgoing.len() > 1 && incoming.len() > 1 {
            for movement in incoming {
                push_selected_token_only_movement_action(
                    &mut actions,
                    movement,
                    selected_wallet_keys,
                    true,
                );
            }
            for movement in outgoing {
                push_selected_token_only_movement_action(
                    &mut actions,
                    movement,
                    selected_wallet_keys,
                    false,
                );
            }
            continue;
        }

        for source in &mut outgoing {
            for destination in &mut incoming {
                if source.amount_raw == 0 || destination.amount_raw == 0 {
                    continue;
                }
                let moved_amount = source.amount_raw.min(destination.amount_raw);
                source.amount_raw = source.amount_raw.saturating_sub(moved_amount);
                destination.amount_raw = destination.amount_raw.saturating_sub(moved_amount);
                let source_selected = selected_wallet_keys.contains(&source.wallet_key);
                let destination_selected = selected_wallet_keys.contains(&destination.wallet_key);
                let applied_at_unix_ms = source
                    .applied_at_unix_ms
                    .max(destination.applied_at_unix_ms);
                let slot = source.slot.or(destination.slot);
                match (source_selected, destination_selected) {
                    (true, true) => {
                        actions.push(RpcResyncLedgerAction::KnownTransfer {
                            marker: crate::trade_ledger::TokenTransferMarkerEvent::new(
                                &source.wallet_key,
                                &destination.wallet_key,
                                &source.mint,
                                moved_amount,
                                &signature,
                                applied_at_unix_ms,
                            )
                            .with_slot(slot),
                            slot,
                        });
                        push_clear_incomplete_movement_action(
                            &mut actions,
                            &source.wallet_key,
                            &source.mint,
                            &transfer_incomplete_signature(
                                &signature,
                                &source.wallet_key,
                                &destination.wallet_key,
                                false,
                            ),
                            applied_at_unix_ms,
                            slot,
                            false,
                        );
                        push_clear_incomplete_movement_action(
                            &mut actions,
                            &destination.wallet_key,
                            &destination.mint,
                            &transfer_incomplete_signature(
                                &signature,
                                &destination.wallet_key,
                                &source.wallet_key,
                                true,
                            ),
                            applied_at_unix_ms,
                            slot,
                            true,
                        );
                    }
                    (true, false) => {
                        let incomplete_signature = transfer_incomplete_signature(
                            &signature,
                            &source.wallet_key,
                            &destination.wallet_key,
                            false,
                        );
                        actions.push(RpcResyncLedgerAction::SentWithoutProceeds {
                            wallet_key: source.wallet_key.clone(),
                            mint: source.mint.clone(),
                            amount_raw: moved_amount,
                            signature: incomplete_signature,
                            applied_at_unix_ms,
                            slot,
                            persist: true,
                        });
                    }
                    (false, true) => {
                        let incomplete_signature = transfer_incomplete_signature(
                            &signature,
                            &destination.wallet_key,
                            &source.wallet_key,
                            true,
                        );
                        actions.push(RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                            wallet_key: destination.wallet_key.clone(),
                            mint: destination.mint.clone(),
                            amount_raw: moved_amount,
                            signature: incomplete_signature,
                            applied_at_unix_ms,
                            slot,
                            persist: true,
                        });
                    }
                    (false, false) => {}
                }
            }
        }

        for movement in incoming {
            push_selected_token_only_movement_action(
                &mut actions,
                movement,
                selected_wallet_keys,
                true,
            );
        }
        for movement in outgoing {
            push_selected_token_only_movement_action(
                &mut actions,
                movement,
                selected_wallet_keys,
                false,
            );
        }
    }

    Ok(actions)
}

fn apply_resync_action_to_ledger(
    ledger: &mut HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
    action: &RpcResyncLedgerAction,
    emit_known_transfer_extras: bool,
) -> Vec<RpcResyncLedgerAction> {
    match action {
        RpcResyncLedgerAction::Trade(event) => {
            let params = RecordConfirmedTradeParams {
                wallet_key: &event.wallet_key,
                wallet_public_key: &event.wallet_public_key,
                mint: &event.mint,
                side: event.side.clone(),
                trade_value_lamports: event.trade_value_lamports,
                token_delta_raw: event.token_delta_raw,
                token_decimals: event.token_decimals,
                confirmed_at_unix_ms: event.confirmed_at_unix_ms,
                slot: event.slot,
                entry_preference_asset: match event.side {
                    TradeSide::Buy => event.settlement_asset,
                    TradeSide::Sell => None,
                },
                settlement_asset: event.settlement_asset,
                explicit_fees: event.explicit_fees.clone(),
                platform_tag: event.platform_tag,
                provenance: event.provenance,
                signature: &event.signature,
                client_request_id: event.client_request_id.as_deref(),
                batch_id: event.batch_id.as_deref(),
            };
            record_confirmed_trade(ledger, params);
            Vec::new()
        }
        RpcResyncLedgerAction::ForceClose { marker, .. } => {
            crate::trade_ledger::force_close_trade_ledger_position(
                ledger,
                &marker.wallet_key,
                &marker.mint,
                marker.applied_at_unix_ms,
            );
            Vec::new()
        }
        RpcResyncLedgerAction::KnownTransfer { marker, slot } => {
            let moved_amount = crate::trade_ledger::transfer_trade_ledger_position(
                ledger,
                &marker.source_wallet_key,
                &marker.destination_wallet_key,
                &marker.mint,
                marker.amount_raw,
                &marker.signature,
                marker.applied_at_unix_ms,
            );
            let unmoved_amount = marker.amount_raw.saturating_sub(moved_amount);
            if !emit_known_transfer_extras || unmoved_amount == 0 {
                return Vec::new();
            }
            crate::trade_ledger::mark_trade_ledger_sent_without_proceeds(
                ledger,
                &marker.source_wallet_key,
                &marker.mint,
                unmoved_amount,
                &marker.signature,
                marker.applied_at_unix_ms,
            );
            crate::trade_ledger::mark_trade_ledger_received_without_cost_basis(
                ledger,
                &marker.destination_wallet_key,
                &marker.mint,
                unmoved_amount,
                &marker.signature,
                marker.applied_at_unix_ms,
            );
            vec![
                RpcResyncLedgerAction::SentWithoutProceeds {
                    wallet_key: marker.source_wallet_key.clone(),
                    mint: marker.mint.clone(),
                    amount_raw: unmoved_amount,
                    signature: transfer_incomplete_signature(
                        &marker.signature,
                        &marker.source_wallet_key,
                        &marker.destination_wallet_key,
                        false,
                    ),
                    applied_at_unix_ms: marker.applied_at_unix_ms,
                    slot: *slot,
                    persist: true,
                },
                RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                    wallet_key: marker.destination_wallet_key.clone(),
                    mint: marker.mint.clone(),
                    amount_raw: unmoved_amount,
                    signature: transfer_incomplete_signature(
                        &marker.signature,
                        &marker.destination_wallet_key,
                        &marker.source_wallet_key,
                        true,
                    ),
                    applied_at_unix_ms: marker.applied_at_unix_ms,
                    slot: *slot,
                    persist: true,
                },
            ]
        }
        RpcResyncLedgerAction::ReceivedWithoutCostBasis {
            wallet_key,
            mint,
            amount_raw,
            signature,
            applied_at_unix_ms,
            ..
        } => {
            if *amount_raw == 0 {
                return Vec::new();
            }
            crate::trade_ledger::mark_trade_ledger_received_without_cost_basis(
                ledger,
                wallet_key,
                mint,
                *amount_raw,
                signature,
                *applied_at_unix_ms,
            );
            Vec::new()
        }
        RpcResyncLedgerAction::SentWithoutProceeds {
            wallet_key,
            mint,
            amount_raw,
            signature,
            applied_at_unix_ms,
            ..
        } => {
            if *amount_raw == 0 {
                return Vec::new();
            }
            crate::trade_ledger::mark_trade_ledger_sent_without_proceeds(
                ledger,
                wallet_key,
                mint,
                *amount_raw,
                signature,
                *applied_at_unix_ms,
            );
            Vec::new()
        }
    }
}

fn reset_resync_scope_in_ledger(
    ledger: &mut HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
    wallet_keys: &[String],
    mint: &str,
    reset_baselines_by_wallet: &HashMap<String, (u64, Option<u64>)>,
) {
    for wallet_key in wallet_keys {
        let (reset_baseline_unix_ms, reset_baseline_slot) = reset_baselines_by_wallet
            .get(wallet_key)
            .copied()
            .unwrap_or((0, None));
        if reset_baseline_unix_ms > 0 {
            reset_trade_ledger_position(
                ledger,
                wallet_key,
                mint,
                reset_baseline_unix_ms,
                reset_baseline_slot,
            );
        } else {
            ledger.remove(&trade_ledger_lookup_key(wallet_key, mint));
        }
    }
}

fn replay_resync_actions(
    ledger: &mut HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
    actions: &[RpcResyncLedgerAction],
    emit_known_transfer_extras: bool,
) -> Vec<RpcResyncLedgerAction> {
    let mut extra_actions = Vec::new();
    for action in actions {
        extra_actions.extend(apply_resync_action_to_ledger(
            ledger,
            action,
            emit_known_transfer_extras,
        ));
    }
    extra_actions
}

#[derive(Debug, Clone, Default)]
struct ResyncBalanceSnapshot {
    balances: HashMap<String, u64>,
    slots_by_wallet: HashMap<String, u64>,
}

async fn fetch_current_resync_balance_with_context(
    client: Client,
    rpc_url: String,
    owner: String,
    mint: String,
    min_context_slot: Option<u64>,
) -> Result<(u64, u64), String> {
    let mut config = serde_json::Map::new();
    config.insert("encoding".to_string(), json!("jsonParsed"));
    config.insert("commitment".to_string(), json!("confirmed"));
    if let Some(slot) = min_context_slot {
        config.insert("minContextSlot".to_string(), json!(slot));
    }
    let result = crate::rpc_client::rpc_request_with_client(
        &client,
        &rpc_url,
        "getTokenAccountsByOwner",
        json!([owner, { "mint": mint }, Value::Object(config)]),
    )
    .await?;
    let context_slot = result
        .get("context")
        .and_then(|value| value.get("slot"))
        .and_then(Value::as_u64)
        .or(min_context_slot)
        .unwrap_or(0);
    let accounts = result
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "RPC getTokenAccountsByOwner returned invalid account data.".to_string())?;

    let mut total_raw = 0u128;
    for entry in accounts {
        let token_amount = entry
            .get("account")
            .and_then(|value| value.get("data"))
            .and_then(|value| value.get("parsed"))
            .and_then(|value| value.get("info"))
            .and_then(|value| value.get("tokenAmount"))
            .ok_or_else(|| {
                "RPC getTokenAccountsByOwner returned invalid token amount data.".to_string()
            })?;
        let amount_raw = token_amount
            .get("amount")
            .and_then(Value::as_str)
            .ok_or_else(|| "RPC token balance response did not include raw amount.".to_string())?
            .parse::<u128>()
            .map_err(|error| format!("RPC token balance amount parse failed: {error}"))?;
        total_raw = total_raw.saturating_add(amount_raw);
    }
    let total_raw = u64::try_from(total_raw)
        .map_err(|_| format!("Token balance for {owner} exceeded u64 limits."))?;
    Ok((total_raw, context_slot))
}

async fn fetch_current_resync_balances(
    wallet_keys: &[String],
    public_keys_by_wallet_key: &HashMap<String, String>,
    mint: &str,
    min_context_slot: Option<u64>,
) -> Result<ResyncBalanceSnapshot, (StatusCode, String)> {
    let client = extension_wallet_rpc_client().map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
    let rpc_url = configured_rpc_url();
    let mut snapshot = ResyncBalanceSnapshot::default();
    for wallet_key in wallet_keys {
        if !public_keys_by_wallet_key.contains_key(wallet_key) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Selected wallet {wallet_key} is missing a configured public key."),
            ));
        }
    }
    let mut pending = wallet_keys
        .iter()
        .map(|wallet_key| {
            (
                wallet_key.clone(),
                public_keys_by_wallet_key
                    .get(wallet_key)
                    .expect("wallet public key was prevalidated")
                    .clone(),
            )
        })
        .collect::<Vec<_>>()
        .into_iter();
    let mut tasks = JoinSet::new();

    loop {
        while tasks.len() < RESYNC_WALLET_CONCURRENCY {
            let Some((wallet_key, public_key)) = pending.next() else {
                break;
            };
            let mint = mint.to_string();
            let client = client.clone();
            let rpc_url = rpc_url.clone();
            tasks.spawn(async move {
                let (amount_raw, context_slot) = fetch_current_resync_balance_with_context(
                    client,
                    rpc_url,
                    public_key,
                    mint,
                    min_context_slot,
                )
                .await
                .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
                Ok::<_, (StatusCode, String)>((wallet_key, amount_raw, context_slot))
            });
        }

        let Some(joined) = tasks.join_next().await else {
            break;
        };
        let (wallet_key, amount_raw, context_slot) = joined.map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Current balance resync task failed: {error}"),
            )
        })??;
        snapshot.balances.insert(wallet_key.clone(), amount_raw);
        snapshot.slots_by_wallet.insert(wallet_key, context_slot);
    }
    for wallet_key in wallet_keys {
        if !snapshot.balances.contains_key(wallet_key) {
            return Err((
                StatusCode::BAD_GATEWAY,
                format!("Current balance resync did not return data for wallet {wallet_key}."),
            ));
        }
    }
    Ok(snapshot)
}

fn replayed_token_balances_from_actions(
    wallet_keys: &[String],
    mint: &str,
    actions: &[RpcResyncLedgerAction],
) -> HashMap<String, u64> {
    let selected = wallet_keys.iter().cloned().collect::<HashSet<_>>();
    let mut balances = wallet_keys
        .iter()
        .map(|wallet_key| (wallet_key.clone(), 0i128))
        .collect::<HashMap<_, _>>();
    for action in actions {
        match action {
            RpcResyncLedgerAction::Trade(event)
                if event.mint == mint && selected.contains(&event.wallet_key) =>
            {
                let entry = balances.entry(event.wallet_key.clone()).or_default();
                *entry = entry.saturating_add(event.token_delta_raw);
            }
            RpcResyncLedgerAction::KnownTransfer { marker, .. } if marker.mint == mint => {
                if selected.contains(&marker.source_wallet_key) {
                    let entry = balances
                        .entry(marker.source_wallet_key.clone())
                        .or_default();
                    *entry = entry.saturating_sub(i128::from(marker.amount_raw));
                }
                if selected.contains(&marker.destination_wallet_key) {
                    let entry = balances
                        .entry(marker.destination_wallet_key.clone())
                        .or_default();
                    *entry = entry.saturating_add(i128::from(marker.amount_raw));
                }
            }
            RpcResyncLedgerAction::ForceClose { marker, .. }
                if marker.mint == mint && selected.contains(&marker.wallet_key) =>
            {
                balances.insert(marker.wallet_key.clone(), 0);
            }
            RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                wallet_key,
                mint: action_mint,
                amount_raw,
                ..
            } if action_mint == mint && selected.contains(wallet_key) => {
                let entry = balances.entry(wallet_key.clone()).or_default();
                *entry = entry.saturating_add(i128::from(*amount_raw));
            }
            RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key,
                mint: action_mint,
                amount_raw,
                ..
            } if action_mint == mint && selected.contains(wallet_key) => {
                let entry = balances.entry(wallet_key.clone()).or_default();
                *entry = entry.saturating_sub(i128::from(*amount_raw));
            }
            _ => {}
        }
    }
    balances
        .into_iter()
        .map(|(wallet_key, amount)| {
            let clamped = amount.clamp(0, i128::from(u64::MAX)) as u64;
            (wallet_key, clamped)
        })
        .collect()
}

fn incomplete_resync_action_key(action: &RpcResyncLedgerAction) -> Option<String> {
    match action {
        RpcResyncLedgerAction::ReceivedWithoutCostBasis {
            wallet_key,
            mint,
            signature,
            ..
        } => Some(format!(
            "received_without_cost_basis::{wallet_key}::{mint}::{signature}"
        )),
        RpcResyncLedgerAction::SentWithoutProceeds {
            wallet_key,
            mint,
            signature,
            ..
        } => Some(format!(
            "sent_without_proceeds::{wallet_key}::{mint}::{signature}"
        )),
        _ => None,
    }
}

fn incomplete_resync_action_amount(action: &RpcResyncLedgerAction) -> Option<u64> {
    match action {
        RpcResyncLedgerAction::ReceivedWithoutCostBasis { amount_raw, .. }
        | RpcResyncLedgerAction::SentWithoutProceeds { amount_raw, .. } => Some(*amount_raw),
        _ => None,
    }
}

fn prune_incomplete_zero_clears_shadowed_by_nonzero_actions(
    actions: &mut Vec<RpcResyncLedgerAction>,
) {
    let nonzero_keys = actions
        .iter()
        .filter(|action| incomplete_resync_action_amount(action).is_some_and(|amount| amount > 0))
        .filter_map(incomplete_resync_action_key)
        .collect::<HashSet<_>>();
    actions.retain(|action| {
        let is_zero_clear = incomplete_resync_action_amount(action) == Some(0);
        !is_zero_clear
            || incomplete_resync_action_key(action).map_or(true, |key| !nonzero_keys.contains(&key))
    });
}

fn build_balance_reconciliation_actions(
    wallet_keys: &[String],
    mint: &str,
    replayed_balances: &HashMap<String, u64>,
    onchain_balances: &HashMap<String, u64>,
    balance_slots_by_wallet: &HashMap<String, u64>,
    applied_at_unix_ms: u64,
) -> Vec<RpcResyncLedgerAction> {
    let mut actions = Vec::new();
    for wallet_key in wallet_keys {
        let Some(onchain) = onchain_balances.get(wallet_key).copied() else {
            continue;
        };
        let local = replayed_balances.get(wallet_key).copied().unwrap_or(0);
        let Some(slot) = balance_slots_by_wallet.get(wallet_key).copied() else {
            continue;
        };
        let slot = Some(slot);
        let received_signature =
            format!("resync-balance-reconcile:received_without_cost_basis:{wallet_key}:{mint}");
        let sent_signature =
            format!("resync-balance-reconcile:sent_without_proceeds:{wallet_key}:{mint}");
        if onchain > local {
            actions.push(RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                wallet_key: wallet_key.clone(),
                mint: mint.to_string(),
                amount_raw: onchain.saturating_sub(local),
                signature: received_signature.clone(),
                applied_at_unix_ms,
                slot,
                persist: true,
            });
            push_clear_incomplete_movement_action(
                &mut actions,
                wallet_key,
                mint,
                &sent_signature,
                applied_at_unix_ms,
                slot,
                false,
            );
        } else if local > onchain {
            push_clear_incomplete_movement_action(
                &mut actions,
                wallet_key,
                mint,
                &received_signature,
                applied_at_unix_ms,
                slot,
                true,
            );
            if onchain == 0 {
                push_clear_incomplete_movement_action(
                    &mut actions,
                    wallet_key,
                    mint,
                    &sent_signature,
                    applied_at_unix_ms,
                    slot,
                    false,
                );
                actions.push(RpcResyncLedgerAction::ForceClose {
                    marker: ForceCloseMarkerEvent::new(
                        wallet_key,
                        mint,
                        applied_at_unix_ms,
                        "on-chain-zero-after-resync",
                    ),
                    persist: true,
                });
            } else {
                actions.push(RpcResyncLedgerAction::SentWithoutProceeds {
                    wallet_key: wallet_key.clone(),
                    mint: mint.to_string(),
                    amount_raw: local.saturating_sub(onchain),
                    signature: sent_signature,
                    applied_at_unix_ms,
                    slot,
                    persist: true,
                });
            }
        } else {
            push_clear_incomplete_movement_action(
                &mut actions,
                wallet_key,
                mint,
                &received_signature,
                applied_at_unix_ms,
                slot,
                true,
            );
            push_clear_incomplete_movement_action(
                &mut actions,
                wallet_key,
                mint,
                &sent_signature,
                applied_at_unix_ms,
                slot,
                false,
            );
        }
    }
    actions
}

async fn fetch_current_confirmed_slot() -> Result<u64, String> {
    let client = extension_wallet_rpc_client()?;
    let response = crate::rpc_client::rpc_request_with_client(
        &client,
        &configured_rpc_url(),
        "getSlot",
        json!([{ "commitment": "confirmed" }]),
    )
    .await?;
    response
        .as_u64()
        .ok_or_else(|| "RPC getSlot returned an invalid payload.".to_string())
}

async fn fetch_resync_candidates_for_wallet_mint(
    wallet_key: &str,
    wallet_public_key: &str,
    mint: &str,
    reset_baseline_unix_ms: u64,
    reset_baseline_slot: Option<u64>,
    known_wallets: &[KnownWalletIdentity],
    helius_config: Option<&HeliusResyncConfig>,
) -> Result<Vec<RpcResyncCandidate>, (StatusCode, String)> {
    if let Some(config) = helius_config {
        match fetch_helius_resync_candidates_for_wallet_mint(
            config,
            wallet_public_key,
            mint,
            reset_baseline_unix_ms,
            reset_baseline_slot,
            known_wallets,
        )
        .await
        {
            Ok(candidates) => return Ok(candidates),
            Err((status, error)) => {
                eprintln!(
                    "[execution-engine][pnl-resync] Helius history failed for wallet={} mint={} status={} err={}; falling back to token-account RPC.",
                    wallet_public_key, mint, status, error
                );
            }
        }
    }
    fetch_rpc_resync_candidates_for_wallet_mint(
        wallet_key,
        wallet_public_key,
        mint,
        reset_baseline_unix_ms,
        reset_baseline_slot,
        known_wallets,
    )
    .await
}

async fn fetch_resync_candidates_for_wallets(
    wallet_keys: &[String],
    public_keys_by_wallet_key: &HashMap<String, String>,
    mint: &str,
    reset_baselines_by_wallet: &HashMap<String, (u64, Option<u64>)>,
    known_wallets: &[KnownWalletIdentity],
    helius_config: Option<&HeliusResyncConfig>,
) -> Result<Vec<RpcResyncCandidate>, (StatusCode, String)> {
    let mut pending = wallet_keys
        .iter()
        .filter_map(|wallet_key| {
            public_keys_by_wallet_key.get(wallet_key).map(|public_key| {
                let (reset_baseline_unix_ms, reset_baseline_slot) = reset_baselines_by_wallet
                    .get(wallet_key)
                    .copied()
                    .unwrap_or((0, None));
                (
                    wallet_key.clone(),
                    public_key.clone(),
                    reset_baseline_unix_ms,
                    reset_baseline_slot,
                )
            })
        })
        .collect::<Vec<_>>()
        .into_iter();
    let mut tasks = JoinSet::new();
    let mut candidates = Vec::new();

    loop {
        while tasks.len() < RESYNC_WALLET_CONCURRENCY {
            let Some((wallet_key, wallet_public_key, reset_baseline_unix_ms, reset_baseline_slot)) =
                pending.next()
            else {
                break;
            };
            let mint = mint.to_string();
            let known_wallets = known_wallets.to_vec();
            let helius_config = helius_config.cloned();
            tasks.spawn(async move {
                fetch_resync_candidates_for_wallet_mint(
                    &wallet_key,
                    &wallet_public_key,
                    &mint,
                    reset_baseline_unix_ms,
                    reset_baseline_slot,
                    &known_wallets,
                    helius_config.as_ref(),
                )
                .await
            });
        }

        let Some(joined) = tasks.join_next().await else {
            break;
        };
        candidates.extend(joined.map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Wallet resync task failed: {error}"),
            )
        })??);
    }

    Ok(candidates)
}

async fn fetch_helius_resync_candidates_for_wallet_mint(
    config: &HeliusResyncConfig,
    wallet_public_key: &str,
    mint: &str,
    reset_baseline_unix_ms: u64,
    reset_baseline_slot: Option<u64>,
    known_wallets: &[KnownWalletIdentity],
) -> Result<Vec<RpcResyncCandidate>, (StatusCode, String)> {
    let client = extension_wallet_rpc_client().map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
    let endpoint = format!(
        "{}/v0/addresses/{}/transactions",
        config.base_url.trim_end_matches('/'),
        wallet_public_key
    );
    let deadline = Instant::now() + RPC_RESYNC_OVERALL_TIMEOUT;
    let mut before: Option<String> = None;
    let mut candidates = Vec::new();
    let mut pages_processed = 0usize;
    let mut transactions_examined = 0usize;

    loop {
        if Instant::now() >= deadline {
            return Err((
                StatusCode::BAD_GATEWAY,
                "Helius enhanced transaction history timed out before reaching the resync boundary."
                    .to_string(),
            ));
        }
        if pages_processed >= RPC_RESYNC_MAX_PAGES
            || transactions_examined >= RPC_RESYNC_MAX_SIGNATURES
        {
            return Err((
                StatusCode::BAD_GATEWAY,
                "Helius enhanced transaction history hit the resync scan limit before reaching the resync boundary."
                    .to_string(),
            ));
        }
        pages_processed += 1;

        let mut query = vec![
            ("api-key".to_string(), config.api_key.clone()),
            ("commitment".to_string(), "confirmed".to_string()),
            ("token-accounts".to_string(), "balanceChanged".to_string()),
            ("limit".to_string(), HELIUS_RESYNC_PAGE_SIZE.to_string()),
        ];
        if let Some(slot) = reset_baseline_slot {
            query.push(("gte-slot".to_string(), slot.to_string()));
        } else if reset_baseline_unix_ms > 0 {
            query.push((
                "gte-time".to_string(),
                reset_baseline_unix_ms.saturating_div(1_000).to_string(),
            ));
        }
        if let Some(before_signature) = before.clone() {
            query.push(("before-signature".to_string(), before_signature));
        }

        let response = client
            .get(&endpoint)
            .query(&query)
            .send()
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_GATEWAY,
                    format!("Helius enhanced transaction request failed: {error}"),
                )
            })?;
        if !response.status().is_success() {
            return Err((
                StatusCode::BAD_GATEWAY,
                format!(
                    "Helius enhanced transaction request returned status {}.",
                    response.status()
                ),
            ));
        }
        let transactions = response.json::<Vec<Value>>().await.map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Helius enhanced transaction response was invalid: {error}"),
            )
        })?;
        if transactions.is_empty() {
            break;
        }

        let mut page_has_candidate = false;
        for transaction in &transactions {
            if Instant::now() >= deadline || transactions_examined >= RPC_RESYNC_MAX_SIGNATURES {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "Helius enhanced transaction history stopped mid-page before reaching the resync boundary."
                        .to_string(),
                ));
            }
            transactions_examined += 1;
            let list_slot = transaction.get("slot").and_then(Value::as_u64);
            let list_block_time_unix_ms = transaction
                .get("timestamp")
                .and_then(Value::as_i64)
                .filter(|value| *value > 0)
                .map(|value| (value as u64).saturating_mul(1_000));
            if !crate::trade_ledger::trade_event_is_after_reset_baseline(
                list_block_time_unix_ms.unwrap_or(0),
                list_slot,
                reset_baseline_unix_ms,
                reset_baseline_slot,
            ) {
                continue;
            }
            page_has_candidate = true;
            for identity in known_wallets {
                let Some(candidate) = helius_candidate_from_transaction(
                    transaction,
                    &identity.wallet_key,
                    &identity.public_key,
                    mint,
                )
                .map_err(|error| (StatusCode::BAD_GATEWAY, error))?
                else {
                    continue;
                };
                candidates.push(candidate);
            }
        }

        if reset_baseline_unix_ms > 0 && !page_has_candidate {
            break;
        }
        if transactions.len() < HELIUS_RESYNC_PAGE_SIZE {
            break;
        }
        before = transactions
            .last()
            .and_then(|item| item.get("signature"))
            .and_then(Value::as_str)
            .map(str::to_string);
        if before.is_none() {
            return Err((
                StatusCode::BAD_GATEWAY,
                "Helius enhanced transaction history page did not include a pagination signature."
                    .to_string(),
            ));
        }
    }

    Ok(candidates)
}

async fn fetch_rpc_token_accounts_for_owner_mint(
    client: &Client,
    rpc_url: &str,
    owner: &str,
    mint: &str,
) -> Result<Vec<String>, String> {
    let response = crate::rpc_client::rpc_request_with_client(
        client,
        rpc_url,
        "getTokenAccountsByOwner",
        json!([
            owner,
            { "mint": mint },
            { "encoding": "jsonParsed", "commitment": "confirmed" }
        ]),
    )
    .await?;
    let accounts = response
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "RPC getTokenAccountsByOwner returned invalid account data.".to_string())?;
    Ok(accounts
        .iter()
        .filter_map(|entry| {
            entry
                .get("pubkey")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect())
}

async fn fetch_rpc_resync_candidates_for_signature(
    client: &Client,
    tx_signature: &str,
    mint: &str,
    known_wallets: &[KnownWalletIdentity],
) -> Result<Vec<RpcResyncCandidate>, String> {
    let result = crate::rpc_client::rpc_request_with_client(
        client,
        &configured_rpc_url(),
        "getTransaction",
        json!([
            tx_signature,
            {
                "encoding": "jsonParsed",
                "commitment": "confirmed",
                "maxSupportedTransactionVersion": 0,
            }
        ]),
    )
    .await?;
    if result.is_null() {
        return Ok(Vec::new());
    }
    let account_keys = result
        .get("transaction")
        .and_then(|value| value.get("message"))
        .and_then(|value| value.get("accountKeys"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pre_balances = result
        .get("meta")
        .and_then(|value| value.get("preBalances"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let post_balances = result
        .get("meta")
        .and_then(|value| value.get("postBalances"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pre_token_balances = result
        .get("meta")
        .and_then(|value| value.get("preTokenBalances"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let post_token_balances = result
        .get("meta")
        .and_then(|value| value.get("postTokenBalances"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut candidates = Vec::new();
    for identity in known_wallets {
        let Some((token_delta_raw, token_decimals)) = maybe_trade_token_delta_from_meta(
            &pre_token_balances,
            &post_token_balances,
            &identity.public_key,
            mint,
        )?
        else {
            continue;
        };
        if token_delta_raw == 0 {
            continue;
        }
        let account_index = account_keys.iter().position(|entry| {
            entry.as_str() == Some(identity.public_key.as_str())
                || entry
                    .get("pubkey")
                    .and_then(Value::as_str)
                    .is_some_and(|pubkey| pubkey == identity.public_key)
        });
        let lamport_delta = account_index
            .and_then(|index| {
                let pre = pre_balances.get(index).and_then(Value::as_u64)?;
                let post = post_balances.get(index).and_then(Value::as_u64)?;
                Some(post as i64 - pre as i64)
            })
            .unwrap_or(0);
        let usd1_delta_raw = i128::from(
            total_token_balance_amount_from_meta(
                &post_token_balances,
                &identity.public_key,
                USD1_MINT,
            )?
            .unwrap_or(0),
        ) - i128::from(
            total_token_balance_amount_from_meta(
                &pre_token_balances,
                &identity.public_key,
                USD1_MINT,
            )?
            .unwrap_or(0),
        );
        candidates.push(RpcResyncCandidate {
            signature: tx_signature.to_string(),
            wallet_key: identity.wallet_key.clone(),
            wallet_public_key: identity.public_key.clone(),
            snapshot: ConfirmedTradeLedgerSnapshot {
                lamport_delta,
                usd1_delta_raw,
                token_delta_raw,
                token_decimals,
                slot: result.get("slot").and_then(Value::as_u64),
                block_time_unix_ms: result
                    .get("blockTime")
                    .and_then(Value::as_i64)
                    .and_then(|value| u64::try_from(value).ok())
                    .map(|value| value.saturating_mul(1_000)),
                explicit_fees: explicit_fee_breakdown_from_transaction(
                    &result,
                    &identity.public_key,
                    mint,
                ),
            },
        });
    }
    Ok(candidates)
}

async fn fetch_rpc_resync_candidates_for_wallet_mint(
    _wallet_key: &str,
    wallet_public_key: &str,
    mint: &str,
    reset_baseline_unix_ms: u64,
    reset_baseline_slot: Option<u64>,
    known_wallets: &[KnownWalletIdentity],
) -> Result<Vec<RpcResyncCandidate>, (StatusCode, String)> {
    let client = extension_wallet_rpc_client().map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
    let rpc_url = configured_rpc_url();
    let (_decimals, token_program) =
        fetch_mint_metadata_with_client(&client, &rpc_url, mint, "confirmed")
            .await
            .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
    let owner_pubkey = Pubkey::from_str(wallet_public_key).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid wallet public key {wallet_public_key}: {error}"),
        )
    })?;
    let mint_pubkey = Pubkey::from_str(mint).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid mint {mint}: {error}"),
        )
    })?;
    let ata =
        get_associated_token_address_with_program_id(&owner_pubkey, &mint_pubkey, &token_program)
            .to_string();
    let mut token_accounts =
        fetch_rpc_token_accounts_for_owner_mint(&client, &rpc_url, wallet_public_key, mint)
            .await
            .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
    token_accounts.push(ata);
    token_accounts.sort();
    token_accounts.dedup();

    let deadline = Instant::now() + RPC_RESYNC_OVERALL_TIMEOUT;
    let mut candidates = Vec::new();
    let mut seen_signatures = HashSet::new();
    let mut pages_processed = 0usize;
    let mut signatures_examined = 0usize;

    for token_account in token_accounts {
        let mut before: Option<String> = None;
        loop {
            if Instant::now() >= deadline {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "RPC token-account transaction history timed out before reaching the resync boundary."
                        .to_string(),
                ));
            }
            if pages_processed >= RPC_RESYNC_MAX_PAGES
                || signatures_examined >= RPC_RESYNC_MAX_SIGNATURES
            {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "RPC token-account transaction history hit the resync scan limit before reaching the resync boundary."
                        .to_string(),
                ));
            }
            pages_processed += 1;

            let mut options = serde_json::Map::new();
            options.insert("limit".to_string(), json!(RPC_RESYNC_PAGE_SIZE));
            options.insert("commitment".to_string(), json!("confirmed"));
            if let Some(before_signature) = before.clone() {
                options.insert("before".to_string(), json!(before_signature));
            }
            let response = crate::rpc_client::rpc_request_with_client(
                &client,
                &rpc_url,
                "getSignaturesForAddress",
                json!([token_account, Value::Object(options)]),
            )
            .await
            .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
            let signatures = response.as_array().ok_or_else(|| {
                (
                    StatusCode::BAD_GATEWAY,
                    "RPC getSignaturesForAddress returned an invalid payload.".to_string(),
                )
            })?;
            if signatures.is_empty() {
                break;
            }

            let mut page_has_candidate = false;
            for item in signatures {
                if Instant::now() >= deadline || signatures_examined >= RPC_RESYNC_MAX_SIGNATURES {
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        "RPC token-account transaction history stopped mid-page before reaching the resync boundary."
                            .to_string(),
                    ));
                }
                signatures_examined += 1;
                let Some(signature) = item.get("signature").and_then(Value::as_str) else {
                    continue;
                };
                let list_slot = item.get("slot").and_then(Value::as_u64);
                let list_block_time_unix_ms = item
                    .get("blockTime")
                    .and_then(Value::as_i64)
                    .filter(|value| *value > 0)
                    .map(|value| (value as u64).saturating_mul(1_000));
                if !crate::trade_ledger::trade_event_is_after_reset_baseline(
                    list_block_time_unix_ms.unwrap_or(0),
                    list_slot,
                    reset_baseline_unix_ms,
                    reset_baseline_slot,
                ) {
                    continue;
                }
                page_has_candidate = true;
                if !seen_signatures.insert(signature.to_string()) {
                    continue;
                }
                let signature_candidates = fetch_rpc_resync_candidates_for_signature(
                    &client,
                    signature,
                    mint,
                    known_wallets,
                )
                .await
                .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
                for candidate in signature_candidates {
                    if !crate::trade_ledger::trade_event_is_after_reset_baseline(
                        candidate.snapshot.block_time_unix_ms.unwrap_or(0),
                        candidate.snapshot.slot,
                        reset_baseline_unix_ms,
                        reset_baseline_slot,
                    ) {
                        continue;
                    }
                    candidates.push(candidate);
                }
            }

            if reset_baseline_unix_ms > 0 && !page_has_candidate {
                break;
            }
            if signatures.len() < RPC_RESYNC_PAGE_SIZE {
                break;
            }
            before = signatures
                .last()
                .and_then(|item| item.get("signature"))
                .and_then(Value::as_str)
                .map(str::to_string);
            if before.is_none() {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "RPC token-account transaction history page did not include a pagination signature."
                        .to_string(),
                ));
            }
        }
    }

    Ok(candidates)
}

fn parse_sol_to_lamports(value: &str) -> Option<u64> {
    parse_decimal_units(value, 9)
}

fn parse_decimal_units(value: &str, decimals: u8) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }
    let mut parts = trimmed.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !fraction.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let scale = 10u128.checked_pow(u32::from(decimals))?;
    let whole_value = if whole.is_empty() {
        0u128
    } else {
        whole.parse::<u128>().ok()?
    };
    let mut fraction_string = fraction.to_string();
    if fraction_string.len() > usize::from(decimals) {
        fraction_string.truncate(usize::from(decimals));
    }
    while fraction_string.len() < usize::from(decimals) {
        fraction_string.push('0');
    }
    let fraction_value = if fraction_string.is_empty() {
        0u128
    } else {
        fraction_string.parse::<u128>().ok()?
    };
    let value = whole_value
        .checked_mul(scale)?
        .checked_add(fraction_value)?;
    if value == 0 {
        return None;
    }
    u64::try_from(value).ok()
}

fn build_buy_planning_seed(
    preset_id: &str,
    planning_mint: &str,
    target: &ResolvedBatchTarget,
    buy_amount_sol: Option<&str>,
) -> String {
    format!(
        "buy:{}:{}:{}:{}",
        preset_id,
        planning_mint,
        target.wallet_group_id.clone().unwrap_or_default(),
        buy_amount_sol.unwrap_or_default()
    )
}

fn format_lamports_to_sol_string(lamports: u64) -> String {
    format_decimal_units(lamports, 9)
}

fn format_decimal_units(amount: u64, decimals: u8) -> String {
    let scale = 10u64.pow(u32::from(decimals));
    let whole = amount / scale;
    let fractional = amount % scale;
    if fractional == 0 {
        return whole.to_string();
    }
    let mut fractional_text = format!("{fractional:0width$}", width = usize::from(decimals));
    while fractional_text.ends_with('0') {
        fractional_text.pop();
    }
    format!("{whole}.{fractional_text}")
}

fn trade_ledger_lookup_key(wallet_key: &str, mint: &str) -> String {
    format!("{}::{}", wallet_key.trim(), mint.trim())
}

fn wallet_position_drifts_from_onchain(
    entry: Option<&crate::trade_ledger::TradeLedgerEntry>,
    on_chain_raw: Option<u64>,
) -> bool {
    let Some(entry) = entry else {
        return false;
    };
    let Some(on_chain_raw) = on_chain_raw else {
        return false;
    };
    let local_position_open = entry.position_open || !entry.open_lots.is_empty();
    if on_chain_raw == 0 {
        return local_position_open;
    }
    !local_position_open
        && entry.last_trade_at_unix_ms > 0
        && (entry.buy_count > 0 || entry.sell_count > 0)
}

fn stable_random_unit(seed: &str) -> f64 {
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    let value = u64::from_le_bytes(bytes);
    (value as f64) / (u64::MAX as f64)
}

fn stable_random_delay(seed: &str, min_delay_ms: u64, max_delay_ms: u64) -> u64 {
    if max_delay_ms <= min_delay_ms {
        return min_delay_ms;
    }
    let spread = max_delay_ms.saturating_sub(min_delay_ms);
    min_delay_ms + ((spread as f64) * stable_random_unit(seed)).round() as u64
}

fn apply_buy_variance(base_lamports: u64, variance_percent: u8, seed: &str) -> (u64, Option<f64>) {
    if base_lamports == 0 || variance_percent == 0 {
        return (base_lamports, None);
    }
    let variance_range = f64::from(variance_percent);
    let applied_variance_percent = (stable_random_unit(seed) * 2.0 - 1.0) * variance_range;
    let adjusted = ((base_lamports as f64) * (1.0 + (applied_variance_percent / 100.0)))
        .round()
        .max(1.0) as u64;
    (adjusted, Some(applied_variance_percent))
}

async fn determine_first_buy_flags(
    wallets: &[WalletSummary],
    wallet_keys: &[String],
    mint: &str,
    trade_ledger: &HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
    allow_onchain_balance_probe: bool,
) -> HashMap<String, bool> {
    let selected_wallets = wallets
        .iter()
        .filter(|wallet| wallet_keys.contains(&wallet.key))
        .map(|wallet| WalletStatusView {
            key: wallet.key.clone(),
            label: wallet.label.clone(),
            enabled: wallet.enabled,
            public_key: Some(wallet.public_key.clone()),
            error: None,
            balance_lamports: None,
            balance_sol: None,
            usd1_balance: None,
            balance_error: None,
            mint_balance: MintBalanceSnapshot::default(),
        })
        .collect::<Vec<_>>();
    let mint_balances = if allow_onchain_balance_probe {
        fetch_wallet_mint_balances(&configured_rpc_url(), &selected_wallets, mint)
            .await
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    wallet_keys
        .iter()
        .map(|wallet_key| {
            let ledger_entry = trade_ledger.get(&trade_ledger_lookup_key(wallet_key, mint));
            let onchain_balance = mint_balances
                .get(wallet_key)
                .and_then(|snapshot| snapshot.raw)
                .unwrap_or(0);
            let has_existing_position = onchain_balance > 0
                || ledger_entry
                    .map(|entry| entry.position_open)
                    .unwrap_or(false);
            (wallet_key.clone(), !has_existing_position)
        })
        .collect()
}

fn build_default_batch_execution_plan(
    target: &ResolvedBatchTarget,
    wallet_request: &WalletTradeRequest,
    trade_ledger: &HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
) -> BatchExecutionPlan {
    BatchExecutionPlan {
        batch_policy: None,
        wallets: target
            .wallet_keys
            .iter()
            .map(|wallet_key| PlannedWalletExecution {
                wallet_key: wallet_key.clone(),
                wallet_request: resolve_wallet_request_for_execution(
                    wallet_request,
                    wallet_key,
                    trade_ledger,
                ),
                planned_summary: WalletExecutionPlanSummary {
                    wallet_key: wallet_key.clone(),
                    buy_amount_sol: wallet_request.buy_amount_sol.clone(),
                    scheduled_delay_ms: 0,
                    delay_applied: false,
                    first_buy: None,
                    applied_variance_percent: None,
                    wrapper_fee_bps: 0,
                    wrapper_fee_sol: None,
                    wrapper_route: None,
                },
            })
            .collect(),
    }
}

async fn build_buy_batch_execution_plan(
    engine: &StoredEngineState,
    target: &ResolvedBatchTarget,
    mint: &str,
    wallet_request: &WalletTradeRequest,
    trade_ledger: &HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
    planning_seed: &str,
    allow_onchain_first_buy_probe: bool,
) -> Result<BatchExecutionPlan, (StatusCode, String)> {
    let policy = match target.batch_policy.clone() {
        Some(policy) => policy,
        None => {
            if target.wallet_keys.len() <= 1
                && matches!(
                    engine.settings.default_distribution_mode,
                    BuyDistributionMode::Each
                )
            {
                return Ok(build_default_batch_execution_plan(
                    target,
                    wallet_request,
                    trade_ledger,
                ));
            }
            WalletGroupBatchPolicy {
                distribution_mode: engine.settings.default_distribution_mode.clone(),
                ..WalletGroupBatchPolicy::default()
            }
        }
    };
    let Some(base_buy_amount_sol) = wallet_request.buy_amount_sol.clone() else {
        return Ok(build_default_batch_execution_plan(
            target,
            wallet_request,
            trade_ledger,
        ));
    };
    let base_buy_lamports = parse_sol_to_lamports(&base_buy_amount_sol).ok_or((
        StatusCode::BAD_REQUEST,
        "buy amount must be greater than zero for batch planning".to_string(),
    ))?;
    let wallet_count = target.wallet_keys.len().max(1);
    let base_wallet_lamports = match policy.distribution_mode {
        BuyDistributionMode::Each => vec![base_buy_lamports; wallet_count],
        BuyDistributionMode::Split => {
            let share = base_buy_lamports / wallet_count as u64;
            let remainder = base_buy_lamports % wallet_count as u64;
            (0..wallet_count)
                .map(|index| share + u64::from(index < remainder as usize))
                .collect()
        }
    };
    let first_buy_flags = if matches!(
        policy.transaction_delay_mode,
        TransactionDelayMode::FirstBuyOnly
    ) {
        determine_first_buy_flags(
            &build_effective_wallets(engine),
            &target.wallet_keys,
            mint,
            trade_ledger,
            allow_onchain_first_buy_probe,
        )
        .await
    } else {
        HashMap::new()
    };

    let mut total_spend_lamports = 0u64;
    let mut cumulative_delay_ms = 0u64;
    let mut delayed_wallet_index = 0usize;
    let mut planned_wallets = Vec::with_capacity(wallet_count);

    for (index, wallet_key) in target.wallet_keys.iter().enumerate() {
        let (adjusted_lamports, applied_variance_percent) = apply_buy_variance(
            base_wallet_lamports[index],
            policy.buy_variance_percent,
            &format!("{planning_seed}:variance:{wallet_key}"),
        );
        total_spend_lamports = total_spend_lamports.saturating_add(adjusted_lamports);

        let first_buy = match policy.transaction_delay_mode {
            TransactionDelayMode::FirstBuyOnly => {
                Some(*first_buy_flags.get(wallet_key).unwrap_or(&true))
            }
            _ => None,
        };
        let should_delay = match policy.transaction_delay_mode {
            TransactionDelayMode::Off => false,
            TransactionDelayMode::On => true,
            TransactionDelayMode::FirstBuyOnly => first_buy.unwrap_or(false),
        };
        let scheduled_delay_ms = if should_delay {
            let delay_value = cumulative_delay_ms;
            let increment = match policy.transaction_delay_strategy {
                TransactionDelayStrategy::Fixed => policy.transaction_delay_ms,
                TransactionDelayStrategy::Random => stable_random_delay(
                    &format!("{planning_seed}:delay:{wallet_key}:{delayed_wallet_index}"),
                    policy.transaction_delay_min_ms,
                    policy.transaction_delay_max_ms,
                ),
            };
            delayed_wallet_index += 1;
            cumulative_delay_ms = cumulative_delay_ms.saturating_add(increment);
            delay_value
        } else {
            0
        };

        let planned_buy_amount_sol = format_lamports_to_sol_string(adjusted_lamports);
        let mut request =
            resolve_wallet_request_for_execution(wallet_request, wallet_key, trade_ledger);
        request.buy_amount_sol = Some(planned_buy_amount_sol.clone());
        planned_wallets.push(PlannedWalletExecution {
            wallet_key: wallet_key.clone(),
            wallet_request: request,
            planned_summary: WalletExecutionPlanSummary {
                wallet_key: wallet_key.clone(),
                buy_amount_sol: Some(planned_buy_amount_sol),
                scheduled_delay_ms,
                delay_applied: scheduled_delay_ms > 0,
                first_buy,
                applied_variance_percent,
                wrapper_fee_bps: 0,
                wrapper_fee_sol: None,
                wrapper_route: None,
            },
        });
    }

    Ok(BatchExecutionPlan {
        batch_policy: Some(BatchExecutionPolicySummary {
            distribution_mode: policy.distribution_mode.clone(),
            buy_variance_percent: policy.buy_variance_percent,
            transaction_delay_mode: policy.transaction_delay_mode.clone(),
            transaction_delay_strategy: policy.transaction_delay_strategy.clone(),
            transaction_delay_ms: policy.transaction_delay_ms,
            transaction_delay_min_ms: policy.transaction_delay_min_ms,
            transaction_delay_max_ms: policy.transaction_delay_max_ms,
            base_wallet_amount_sol: Some(format_lamports_to_sol_string(base_wallet_lamports[0])),
            total_batch_spend_sol: Some(format_lamports_to_sol_string(total_spend_lamports)),
        }),
        wallets: planned_wallets,
    })
}

fn resolve_wallet_request_for_execution(
    wallet_request: &WalletTradeRequest,
    wallet_key: &str,
    trade_ledger: &HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
) -> WalletTradeRequest {
    let mut request = wallet_request.clone();
    if matches!(request.side, TradeSide::Sell) {
        let ledger_key = format!("{}::{}", wallet_key.trim(), request.mint.trim());
        let stored_preference = trade_ledger
            .get(&ledger_key)
            .and_then(|entry| entry.entry_preference);
        request.policy.sell_settlement_asset =
            resolve_sell_settlement_asset(request.policy.sell_settlement_policy, stored_preference);
    }
    request
}

fn recompute_batch_summary(batch: &mut BatchStatusResponse) {
    let mut queued_wallets = 0;
    let mut submitted_wallets = 0;
    let mut confirmed_wallets = 0;
    let mut failed_wallets = 0;

    for wallet in &batch.wallets {
        match wallet.status {
            BatchLifecycleStatus::Queued => queued_wallets += 1,
            BatchLifecycleStatus::Submitted => submitted_wallets += 1,
            BatchLifecycleStatus::Confirmed => confirmed_wallets += 1,
            BatchLifecycleStatus::Failed => failed_wallets += 1,
            BatchLifecycleStatus::PartiallyConfirmed => submitted_wallets += 1,
        }
    }

    batch.summary.queued_wallets = queued_wallets;
    batch.summary.submitted_wallets = submitted_wallets;
    batch.summary.confirmed_wallets = confirmed_wallets;
    batch.summary.failed_wallets = failed_wallets;
    batch.status = if failed_wallets == batch.summary.total_wallets {
        BatchLifecycleStatus::Failed
    } else if confirmed_wallets == batch.summary.total_wallets {
        BatchLifecycleStatus::Confirmed
    } else if confirmed_wallets > 0 || failed_wallets > 0 {
        BatchLifecycleStatus::PartiallyConfirmed
    } else if submitted_wallets > 0 {
        BatchLifecycleStatus::Submitted
    } else {
        BatchLifecycleStatus::Queued
    };
}

fn submitted_signature_from_confirmation_gap_error(error: &str) -> Option<String> {
    let prefix = "Transport submitted transaction ";
    let separator = ", but ";
    let suffix = " confirmation was not observed.";
    let trimmed = error.trim();
    let remainder = trimmed.strip_prefix(prefix)?;
    let (signature, commitment_clause) = remainder.split_once(separator)?;
    if !commitment_clause.ends_with(suffix) {
        return None;
    }
    let normalized = signature.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn next_batch_status_revision(state: &AppState, batch_id: &str) -> u64 {
    let Ok(mut guard) = state.batch_status_revisions.lock() else {
        return now_unix_ms();
    };
    let next = guard
        .get(batch_id)
        .copied()
        .unwrap_or_default()
        .saturating_add(1);
    guard.insert(batch_id.to_string(), next);
    next
}

fn publish_batch_status_snapshot(state: &AppState, batch: &BatchStatusResponse, reason: &str) {
    let revision = next_batch_status_revision(state, &batch.batch_id);
    let at_ms = u128::from(now_unix_ms());
    let mut snapshot = serde_json::to_value(batch).unwrap_or_else(|_| json!({}));
    if let Value::Object(ref mut object) = snapshot {
        object.insert("revision".to_string(), json!(revision));
        object.insert("streamReason".to_string(), json!(reason));
        object.insert("streamEmittedAtUnixMs".to_string(), json!(at_ms));
    }
    state
        .balance_stream
        .publish_batch_status_event(BatchStatusEventPayload {
            batch_id: batch.batch_id.clone(),
            client_request_id: batch.client_request_id.clone(),
            revision,
            snapshot,
            reason: reason.to_string(),
            at_ms,
        });
    eprintln!(
        "[execution-engine][latency] phase=sse-emit event=batchStatus batch={} clientRequestId={} revision={} reason={} emitted_at_ms={}",
        batch.batch_id, batch.client_request_id, revision, reason, at_ms
    );
}

fn duplicate_signature_owner(
    batches: &HashMap<String, BatchStatusResponse>,
    current_batch_id: &str,
    current_wallet_key: &str,
    signature: &str,
) -> Option<(String, String)> {
    let normalized_signature = signature.trim();
    if normalized_signature.is_empty() {
        return None;
    }
    for (batch_id, batch) in batches {
        for wallet in &batch.wallets {
            if wallet.tx_signature.as_deref().map(str::trim) != Some(normalized_signature) {
                continue;
            }
            if batch_id == current_batch_id && wallet.wallet_key == current_wallet_key {
                continue;
            }
            return Some((batch_id.clone(), wallet.wallet_key.clone()));
        }
    }
    None
}

fn duplicate_signature_error(
    signature: &str,
    owner_batch_id: &str,
    owner_wallet_key: &str,
) -> String {
    format!(
        "Duplicate transaction signature {signature} already belongs to batch {owner_batch_id} wallet {owner_wallet_key}; this request did not submit a distinct transaction."
    )
}

fn trade_event_error_message(err: &Value) -> String {
    if let Some(message) = err.as_str() {
        let trimmed = message.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    serde_json::to_string(err).unwrap_or_else(|_| "Transaction failed.".to_string())
}

fn resolve_trade_event_wallet_index(batch: &BatchStatusResponse, signature: &str) -> Option<usize> {
    let normalized_signature = signature.trim();
    if normalized_signature.is_empty() {
        return None;
    }
    if let Some(index) = batch.wallets.iter().position(|wallet| {
        wallet.tx_signature.as_deref().map(str::trim) == Some(normalized_signature)
    }) {
        return Some(index);
    }
    let unresolved = batch.wallets.iter().position(|wallet| {
        !matches!(
            wallet.status,
            BatchLifecycleStatus::Confirmed | BatchLifecycleStatus::Failed
        )
    });
    unresolved.or_else(|| (batch.wallets.len() == 1).then_some(0))
}

fn spawn_batch_trade_reconciliation_task(state: AppState) {
    let mut events = state.balance_stream().subscribe_events();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(StreamEvent::Trade(payload)) => {
                    if let Err(error) = reconcile_batch_with_trade_event(&state, &payload).await {
                        eprintln!("[execution-engine][batch-reconcile] {error}");
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    eprintln!(
                        "[execution-engine][batch-reconcile] trade event listener lagged; skipped {} event(s)",
                        skipped
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn reconcile_batch_with_trade_event(
    state: &AppState,
    payload: &TradeEventPayload,
) -> Result<(), String> {
    let normalized_status = payload.status.trim().to_ascii_lowercase();
    if !matches!(normalized_status.as_str(), "confirmed" | "failed") {
        return Ok(());
    }
    let mut guard = state.batches.write().await;
    let Some(wallet_index) = guard
        .get(&payload.batch_id)
        .and_then(|batch| resolve_trade_event_wallet_index(batch, &payload.signature))
    else {
        return Ok(());
    };
    let duplicate_owner = if normalized_status == "confirmed" {
        guard
            .get(&payload.batch_id)
            .and_then(|batch| batch.wallets.get(wallet_index))
            .and_then(|wallet| {
                duplicate_signature_owner(
                    &guard,
                    &payload.batch_id,
                    &wallet.wallet_key,
                    &payload.signature,
                )
            })
    } else {
        None
    };
    let Some(batch) = guard.get_mut(&payload.batch_id) else {
        return Ok(());
    };
    let wallet = batch.wallets.get_mut(wallet_index).ok_or_else(|| {
        format!(
            "batch {} wallet index {} missing",
            payload.batch_id, wallet_index
        )
    })?;
    wallet.tx_signature = Some(payload.signature.clone());
    if let Some((owner_batch_id, owner_wallet_key)) = duplicate_owner {
        wallet.status = BatchLifecycleStatus::Failed;
        wallet.error = Some(duplicate_signature_error(
            &payload.signature,
            &owner_batch_id,
            &owner_wallet_key,
        ));
    } else if normalized_status == "confirmed" {
        wallet.status = BatchLifecycleStatus::Confirmed;
        wallet.error = None;
    } else {
        wallet.status = BatchLifecycleStatus::Failed;
        wallet.error = payload
            .err
            .as_ref()
            .map(trade_event_error_message)
            .or_else(|| Some("Transaction failed.".to_string()));
    }
    batch.updated_at_unix_ms = now_unix_ms();
    recompute_batch_summary(batch);
    let batch_snapshot = batch.clone();
    if let Err((_, error)) = persist_batch_history(&state.batch_history_path, &guard) {
        eprintln!("[execution-engine][persist] batch history write failed: {error}");
        state.persist_failures.record_batch_history_failure(&error);
    }
    drop(guard);
    publish_batch_status_snapshot(state, &batch_snapshot, "trade_reconcile");
    Ok(())
}

fn active_batch_count(batches: &HashMap<String, BatchStatusResponse>) -> usize {
    batches
        .values()
        .filter(|batch| {
            matches!(
                batch.status,
                BatchLifecycleStatus::Queued
                    | BatchLifecycleStatus::Submitted
                    | BatchLifecycleStatus::PartiallyConfirmed
            )
        })
        .count()
}

fn recover_loaded_batches(
    mut batches: HashMap<String, BatchStatusResponse>,
) -> HashMap<String, BatchStatusResponse> {
    let now = now_unix_ms();
    for batch in batches.values_mut() {
        if matches!(
            batch.status,
            BatchLifecycleStatus::Queued
                | BatchLifecycleStatus::Submitted
                | BatchLifecycleStatus::PartiallyConfirmed
        ) {
            for wallet in &mut batch.wallets {
                if !matches!(wallet.status, BatchLifecycleStatus::Confirmed) {
                    wallet.status = BatchLifecycleStatus::Failed;
                    if wallet.error.is_none() {
                        wallet.error =
                            Some("Execution host restarted before batch completion".to_string());
                    }
                }
            }
            recompute_batch_summary(batch);
            batch.status = BatchLifecycleStatus::Failed;
            batch.updated_at_unix_ms = now;
        }
    }
    batches
}

fn prune_accepted_requests(
    accepted_requests: &mut HashMap<String, AcceptedRequestRecord>,
    now_unix_ms: u64,
) {
    accepted_requests.retain(|_, entry| entry.expires_at_unix_ms > now_unix_ms);
}

fn normalize_client_request_id(client_request_id: String) -> Result<String, (StatusCode, String)> {
    let normalized = client_request_id.trim().to_string();
    if normalized.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "clientRequestId is required for trade submission".to_string(),
        ));
    }
    Ok(normalized)
}

fn build_trade_fingerprint(
    side: &TradeSide,
    mint: &str,
    preset_id: &str,
    target: &ResolvedBatchTarget,
    planned_execution: Option<&LifecycleAndCanonicalMarket>,
    buy_amount_sol: Option<&str>,
    sell_percent: Option<&str>,
    sell_output_sol: Option<&str>,
    policy: &ResolvedTradePolicy,
    batch_policy: Option<&BatchExecutionPolicySummary>,
    pinned_pool: Option<&str>,
    warm_key: Option<&str>,
    execution_plan: &[WalletExecutionPlanSummary],
) -> String {
    let mut wallet_keys = target.wallet_keys.clone();
    wallet_keys.sort();
    json!({
        "side": side,
        "mint": mint,
        "presetId": preset_id,
        "selectionMode": target.selection_mode,
        "walletGroupId": target.wallet_group_id,
        "walletKeys": wallet_keys,
        "plannedExecution": planned_execution,
        "buyAmountSol": buy_amount_sol,
        "sellPercent": sell_percent,
        "sellOutputSol": sell_output_sol,
        "pinnedPool": pinned_pool,
        "warmKey": warm_key,
        "batchPolicy": batch_policy,
        "executionPlan": execution_plan,
        "slippagePercent": policy.slippage_percent,
        "mevMode": policy.mev_mode,
        "autoTipEnabled": policy.auto_tip_enabled,
        "feeSol": policy.fee_sol,
        "tipSol": policy.tip_sol,
        "provider": policy.provider,
        "endpointProfile": policy.endpoint_profile,
        "commitment": policy.commitment,
        "skipPreflight": policy.skip_preflight,
        "trackSendBlockHeight": policy.track_send_block_height,
        "buyFundingPolicy": policy.buy_funding_policy,
        "sellSettlementPolicy": policy.sell_settlement_policy,
        "sellSettlementAsset": policy.sell_settlement_asset,
        "wrapperFeeBps": crate::rollout::wrapper_default_fee_bps(),
        "wrapperRouteConversion": trade_fingerprint_route_conversion(side, policy)
    })
    .to_string()
}

fn trade_fingerprint_route_conversion(
    side: &TradeSide,
    policy: &ResolvedTradePolicy,
) -> &'static str {
    match side {
        TradeSide::Buy if policy.buy_funding_policy != BuyFundingPolicy::SolOnly => "to_sol_in",
        TradeSide::Sell if policy.sell_settlement_asset != TradeSettlementAsset::Sol => {
            "to_sol_out"
        }
        _ => "none",
    }
}

fn resolve_batch_target(
    wallets: &[WalletSummary],
    wallet_groups: &[WalletGroupSummary],
    wallet_key: Option<String>,
    wallet_keys: Option<Vec<String>>,
    wallet_group_id: Option<String>,
) -> Result<ResolvedBatchTarget, (StatusCode, String)> {
    let selector_count = usize::from(wallet_key.is_some())
        + usize::from(wallet_keys.as_ref().is_some_and(|keys| !keys.is_empty()))
        + usize::from(wallet_group_id.is_some());

    if selector_count != 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "provide exactly one of walletKey, walletKeys, or walletGroupId".to_string(),
        ));
    }

    let enabled_wallets: HashSet<String> = wallets
        .iter()
        .filter(|wallet| wallet.enabled)
        .map(|wallet| wallet.key.clone())
        .collect();

    if let Some(wallet_key) = wallet_key {
        if !enabled_wallets.contains(&wallet_key) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unknown or disabled wallet: {wallet_key}"),
            ));
        }
        return Ok(ResolvedBatchTarget {
            selection_mode: BatchSelectionMode::SingleWallet,
            wallet_group_id: None,
            wallet_group_label: None,
            batch_policy: None,
            wallet_keys: vec![wallet_key],
            wallet_count: 1,
        });
    }

    if let Some(wallet_keys) = wallet_keys.filter(|keys| !keys.is_empty()) {
        let mut deduped = Vec::new();
        let mut seen = HashSet::new();
        for wallet_key in wallet_keys {
            if !enabled_wallets.contains(&wallet_key) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("unknown or disabled wallet: {wallet_key}"),
                ));
            }
            if seen.insert(wallet_key.clone()) {
                deduped.push(wallet_key);
            }
        }
        let wallet_count = deduped.len();
        return Ok(ResolvedBatchTarget {
            selection_mode: if wallet_count == 1 {
                BatchSelectionMode::SingleWallet
            } else {
                BatchSelectionMode::WalletList
            },
            wallet_group_id: None,
            wallet_group_label: None,
            batch_policy: None,
            wallet_keys: deduped,
            wallet_count,
        });
    }

    let wallet_group_id = wallet_group_id.expect("validated group selector");
    let group = wallet_groups
        .iter()
        .find(|group| group.id == wallet_group_id)
        .ok_or((
            StatusCode::BAD_REQUEST,
            format!("unknown wallet group: {wallet_group_id}"),
        ))?;

    let mut wallet_keys = Vec::new();
    let mut seen = HashSet::new();
    for wallet_key in &group.wallet_keys {
        if !enabled_wallets.contains(wallet_key) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("wallet group contains unknown or disabled wallet: {wallet_key}"),
            ));
        }
        if seen.insert(wallet_key.clone()) {
            wallet_keys.push(wallet_key.clone());
        }
    }
    if wallet_keys.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("wallet group {} has no enabled wallets", group.id),
        ));
    }

    Ok(ResolvedBatchTarget {
        selection_mode: if wallet_keys.len() == 1 {
            BatchSelectionMode::SingleWallet
        } else {
            BatchSelectionMode::WalletGroup
        },
        wallet_group_id: Some(group.id.clone()),
        wallet_group_label: Some(group.label.clone()),
        batch_policy: Some(group.batch_policy.clone()),
        wallet_keys: wallet_keys.clone(),
        wallet_count: wallet_keys.len(),
    })
}

fn resolve_token_request(
    request: &ResolveTokenRequest,
) -> Result<ResolvedTokenRequest, (StatusCode, String)> {
    let raw_address = normalize_route_address(request.address.as_deref())?;
    let origin_surface = request.surface.clone();
    let canonical_surface = origin_surface.canonical();
    let source_url = request
        .url
        .as_deref()
        .and_then(trimmed_option)
        .unwrap_or_default()
        .to_string();
    Ok(ResolvedTokenRequest {
        platform: request.platform.clone(),
        origin_surface,
        canonical_surface,
        source_url,
        raw_address,
    })
}

fn route_descriptor_labels(
    descriptor: &crate::trade_dispatch::RouteDescriptor,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    (
        descriptor
            .family
            .as_ref()
            .map(|family| family.label().to_string()),
        descriptor
            .lifecycle
            .as_ref()
            .map(|lifecycle| lifecycle.label().to_string()),
        descriptor
            .quote_asset
            .as_ref()
            .map(|quote_asset| quote_asset.label().to_string()),
        descriptor
            .canonical_market_key
            .clone()
            .filter(|value| !value.trim().is_empty()),
    )
}

fn route_descriptor_pair_address(
    descriptor: &crate::trade_dispatch::RouteDescriptor,
) -> Option<String> {
    descriptor.resolved_pair.clone().or_else(|| {
        descriptor
            .family
            .as_ref()
            .filter(|family| {
                !matches!(
                    family,
                    crate::trade_planner::TradeVenueFamily::PumpBondingCurve
                )
            })
            .and_then(|_| {
                descriptor
                    .canonical_market_key
                    .clone()
                    .filter(|value| !value.trim().is_empty())
            })
    })
}

fn route_error_code(message: &str) -> Option<&str> {
    let trimmed = message.trim();
    let suffix = trimmed.strip_prefix('[')?;
    let closing = suffix.find(']')?;
    Some(&suffix[..closing])
}

fn is_resolve_token_route_error(message: &str) -> bool {
    matches!(
        route_error_code(message),
        Some("pair_mismatch" | "non_canonical_blocked")
    )
}

fn build_route_probe_request(
    raw_address: String,
    platform_label: Option<String>,
    pinned_pool: Option<String>,
) -> crate::trade_runtime::TradeRuntimeRequest {
    crate::trade_runtime::TradeRuntimeRequest {
        side: TradeSide::Buy,
        mint: raw_address,
        buy_amount_sol: None,
        sell_intent: None,
        policy: crate::trade_runtime::RuntimeExecutionPolicy {
            slippage_percent: "5".to_string(),
            mev_mode: MevMode::Off,
            auto_tip_enabled: false,
            fee_sol: "0".to_string(),
            tip_sol: "0".to_string(),
            provider: default_execution_provider(),
            endpoint_profile: default_execution_endpoint_profile(),
            commitment: "confirmed".to_string(),
            skip_preflight: false,
            track_send_block_height: false,
            buy_funding_policy: default_buy_funding_policy_sol_only(),
            sell_settlement_policy: default_sell_settlement_policy_always_to_sol(),
            sell_settlement_asset: TradeSettlementAsset::Sol,
        },
        platform_label,
        planned_route: None,
        planned_trade: None,
        pinned_pool,
        warm_key: None,
    }
}

fn normalize_route_address(address: Option<&str>) -> Result<String, (StatusCode, String)> {
    address.and_then(trimmed_option).map(str::to_string).ok_or((
        StatusCode::BAD_REQUEST,
        "address is required for token resolution and trade submission".to_string(),
    ))
}

fn normalize_optional_route_address(
    address: Option<&str>,
) -> Result<Option<String>, (StatusCode, String)> {
    match address.and_then(trimmed_option) {
        Some(value) => {
            solana_sdk::pubkey::Pubkey::from_str(value).map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid route companion address {value}: {error}"),
                )
            })?;
            Ok(Some(value.to_string()))
        }
        None => Ok(None),
    }
}

fn route_companion_pair(
    pair: Option<&str>,
    pinned_pool: Option<&str>,
) -> Result<Option<String>, (StatusCode, String)> {
    normalize_optional_route_address(pair)?.map_or_else(
        || normalize_optional_route_address(pinned_pool),
        |pair| Ok(Some(pair)),
    )
}

fn resolve_preset<'a>(
    presets: &'a [PresetSummary],
    preset_id: &str,
) -> Result<&'a PresetSummary, (StatusCode, String)> {
    presets.iter().find(|preset| preset.id == preset_id).ok_or((
        StatusCode::BAD_REQUEST,
        format!("unknown preset id: {preset_id}"),
    ))
}

fn find_matching_canonical_preset(config: &Value, preset_id: &str) -> Option<Value> {
    config
        .get("presets")
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("id")
                    .and_then(Value::as_str)
                    .map(|value| value == preset_id)
                    .unwrap_or(false)
            })
        })
        .cloned()
}

fn resolve_capped_auto_fee_fields(
    provider: &str,
    action: &str,
    action_label: &str,
    max_fee_sol: &str,
    fallback_priority_fee_sol: &str,
    fallback_tip_sol: &str,
) -> (String, String, Vec<String>) {
    let runtime = shared_fee_market_runtime();
    let snapshot_status = read_shared_fee_market_snapshot(runtime.config());
    let output = resolve_buffered_auto_fee_components(AutoFeeResolutionInput {
        provider,
        execution_class: "manual",
        action,
        action_label,
        max_fee_sol,
        fallback_priority_fee_sol,
        fallback_tip_sol,
        snapshot_status,
        allow_unavailable_fallback: true,
    });
    match output {
        Ok(output) => {
            eprintln!(
                "[execution-engine][auto-fee] provider={} action={} priority_source={} tip_source={} priority_lamports={:?} tip_lamports={:?} priority_estimate_lamports={:?} tip_estimate_lamports={:?} cap_lamports={:?} degradations={}",
                provider,
                action,
                output.priority_source,
                output.tip_source,
                output.priority_lamports,
                output.tip_lamports,
                output.priority_estimate_lamports,
                output.tip_estimate_lamports,
                output.cap_lamports,
                output.degradations.len()
            );
            let fee_sol = output
                .priority_lamports
                .map(format_lamports_to_sol_decimal)
                .unwrap_or_default();
            let tip_sol = if crate::provider_tip::provider_required_tip_lamports(provider).is_some()
            {
                output
                    .tip_lamports
                    .map(format_lamports_to_sol_decimal)
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let warnings = output
                .degradations
                .into_iter()
                .map(|degradation| degradation.message)
                .collect();
            (fee_sol, tip_sol, warnings)
        }
        Err(error) => {
            let fee_sol = auto_fee_fallback_sol(fallback_priority_fee_sol);
            let tip_sol = if crate::provider_tip::provider_required_tip_lamports(provider).is_some()
            {
                auto_fee_fallback_sol(fallback_tip_sol)
            } else {
                String::new()
            };
            eprintln!(
                "[execution-engine][auto-fee] provider={} action={} source=error-fallback error={} fee_sol={} tip_sol={}",
                provider, action, error, fee_sol, tip_sol
            );
            (
                fee_sol.clone(),
                tip_sol.clone(),
                vec![auto_fee_unavailable_error_warning(
                    &error,
                    &fee_sol,
                    if tip_sol.is_empty() {
                        None
                    } else {
                        Some(tip_sol.as_str())
                    },
                )],
            )
        }
    }
}

fn auto_fee_fallback_sol(value: &str) -> String {
    let lamports = parse_sol_decimal_to_lamports(value)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_AUTO_FEE_FALLBACK_LAMPORTS);
    format_lamports_to_sol_decimal(lamports)
}

fn auto_fee_unavailable_error_warning(error: &str, fee_sol: &str, tip_sol: Option<&str>) -> String {
    match tip_sol {
        Some(tip_sol) => format!(
            "Auto Fee unavailable: {error}. Defaulted priority fee to {fee_sol} SOL and tip to {tip_sol} SOL."
        ),
        None => format!("Auto Fee unavailable: {error}. Defaulted priority fee to {fee_sol} SOL."),
    }
}

fn resolve_buy_policy(
    settings: &EngineSettings,
    config: &Value,
    preset: &PresetSummary,
    buy_amount_override: Option<&str>,
    buy_funding_policy_override: Option<BuyFundingPolicy>,
) -> ResolvedTradePolicy {
    let canonical_preset = find_matching_canonical_preset(config, &preset.id);
    let buy_route = canonical_preset
        .as_ref()
        .and_then(|value| value.get("buySettings"))
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    let route_slippage = route_string_field(&buy_route, "slippagePercent");
    let route_priority_fee = route_string_field(&buy_route, "priorityFeeSol");
    let route_tip = route_string_field(&buy_route, "tipSol");
    let route_max_fee = route_string_field(&buy_route, "maxFeeSol");
    let route_provider = route_string_field(&buy_route, "provider");
    let route_endpoint_profile = route_string_field(&buy_route, "endpointProfile");
    let resolved_provider = first_non_empty(&[
        Some(route_provider.as_str()),
        Some(preset.buy_provider.as_str()),
        Some(settings.execution_provider.as_str()),
    ])
    .unwrap_or("")
    .to_string();
    let provider_supports_tip =
        crate::provider_tip::provider_required_tip_lamports(&resolved_provider).is_some();
    let auto_tip_enabled = route_bool_field(&buy_route, "autoFee") || preset.buy_auto_tip_enabled;
    let max_fee_sol = first_non_empty(&[
        Some(route_max_fee.as_str()),
        Some(preset.buy_max_fee_sol.as_str()),
    ])
    .unwrap_or("");
    let manual_priority_fee_sol = if route_priority_fee.trim().is_empty() {
        preset.buy_fee_sol.clone()
    } else {
        route_priority_fee
    };
    let manual_tip_sol = if !provider_supports_tip {
        String::new()
    } else if route_tip.trim().is_empty() {
        preset.buy_tip_sol.clone()
    } else {
        route_tip
    };
    let (fee_sol, tip_sol, auto_fee_warnings) = if auto_tip_enabled {
        resolve_capped_auto_fee_fields(
            &resolved_provider,
            "buy",
            "Buy",
            max_fee_sol,
            &manual_priority_fee_sol,
            &manual_tip_sol,
        )
    } else {
        (manual_priority_fee_sol, manual_tip_sol, Vec::new())
    };
    let buy_funding_policy = buy_funding_policy_override
        .or_else(|| route_buy_funding_policy(&buy_route))
        .or_else(|| {
            if !preset.buy_funding_policy_explicit
                && preset.buy_funding_policy == default_buy_funding_policy_sol_only()
            {
                Some(settings.default_buy_funding_policy)
            } else {
                Some(preset.buy_funding_policy)
            }
        })
        .unwrap_or(settings.default_buy_funding_policy);
    ResolvedTradePolicy {
        slippage_percent: first_non_empty(&[
            Some(route_slippage.as_str()),
            Some(preset.buy_slippage_percent.as_str()),
            Some(preset.slippage_percent.as_str()),
            Some(settings.default_buy_slippage_percent.as_str()),
        ])
        .unwrap_or("20")
        .to_string(),
        mev_mode: {
            let route_mode = route_mev_mode(&buy_route);
            if matches!(route_mode, MevMode::Off) {
                if matches!(preset.buy_mev_mode, MevMode::Off) {
                    settings.default_buy_mev_mode.clone()
                } else {
                    preset.buy_mev_mode.clone()
                }
            } else {
                route_mode
            }
        },
        auto_tip_enabled,
        fee_sol,
        tip_sol,
        provider: resolved_provider,
        endpoint_profile: first_non_empty(&[
            Some(route_endpoint_profile.as_str()),
            Some(preset.buy_endpoint_profile.as_str()),
            Some(settings.execution_endpoint_profile.as_str()),
        ])
        .unwrap_or("")
        .to_string(),
        commitment: settings.execution_commitment.clone(),
        skip_preflight: settings.execution_skip_preflight,
        track_send_block_height: config_track_send_block_height(config),
        buy_amount_sol: first_non_empty(&[
            buy_amount_override,
            Some(preset.buy_amount_sol.as_str()),
            preset
                .buy_amounts_sol
                .iter()
                .find(|value| !value.trim().is_empty())
                .map(String::as_str),
        ])
        .map(str::to_string),
        sell_percent: None,
        buy_funding_policy,
        sell_settlement_policy: settings.default_sell_settlement_policy,
        sell_settlement_asset: resolve_sell_settlement_asset(
            settings.default_sell_settlement_policy,
            None,
        ),
        auto_fee_warnings,
    }
}

fn resolve_sell_policy(
    settings: &EngineSettings,
    config: &Value,
    preset: &PresetSummary,
    sell_settlement_policy_override: Option<SellSettlementPolicy>,
) -> ResolvedTradePolicy {
    let canonical_preset = find_matching_canonical_preset(config, &preset.id);
    let sell_route = canonical_preset
        .as_ref()
        .and_then(|value| value.get("sellSettings"))
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    let route_slippage = route_string_field(&sell_route, "slippagePercent");
    let route_priority_fee = route_string_field(&sell_route, "priorityFeeSol");
    let route_tip = route_string_field(&sell_route, "tipSol");
    let route_max_fee = route_string_field(&sell_route, "maxFeeSol");
    let route_provider = route_string_field(&sell_route, "provider");
    let route_endpoint_profile = route_string_field(&sell_route, "endpointProfile");
    let resolved_provider = first_non_empty(&[
        Some(route_provider.as_str()),
        Some(preset.sell_provider.as_str()),
        Some(settings.execution_provider.as_str()),
    ])
    .unwrap_or("")
    .to_string();
    let provider_supports_tip =
        crate::provider_tip::provider_required_tip_lamports(&resolved_provider).is_some();
    let auto_tip_enabled = route_bool_field(&sell_route, "autoFee") || preset.sell_auto_tip_enabled;
    let max_fee_sol = first_non_empty(&[
        Some(route_max_fee.as_str()),
        Some(preset.sell_max_fee_sol.as_str()),
    ])
    .unwrap_or("");
    let manual_priority_fee_sol = if route_priority_fee.trim().is_empty() {
        preset.sell_fee_sol.clone()
    } else {
        route_priority_fee
    };
    let manual_tip_sol = if !provider_supports_tip {
        String::new()
    } else if route_tip.trim().is_empty() {
        preset.sell_tip_sol.clone()
    } else {
        route_tip
    };
    let (fee_sol, tip_sol, auto_fee_warnings) = if auto_tip_enabled {
        resolve_capped_auto_fee_fields(
            &resolved_provider,
            "sell",
            "Sell",
            max_fee_sol,
            &manual_priority_fee_sol,
            &manual_tip_sol,
        )
    } else {
        (manual_priority_fee_sol, manual_tip_sol, Vec::new())
    };
    let sell_settlement_policy = sell_settlement_policy_override
        .or_else(|| route_sell_settlement_policy(&sell_route))
        .or_else(|| {
            if !preset.sell_settlement_policy_explicit
                && preset.sell_settlement_policy == default_sell_settlement_policy_always_to_sol()
            {
                Some(settings.default_sell_settlement_policy)
            } else {
                Some(preset.sell_settlement_policy)
            }
        })
        .unwrap_or(settings.default_sell_settlement_policy);
    ResolvedTradePolicy {
        slippage_percent: first_non_empty(&[
            Some(route_slippage.as_str()),
            Some(preset.sell_slippage_percent.as_str()),
            Some(preset.slippage_percent.as_str()),
            Some(settings.default_sell_slippage_percent.as_str()),
        ])
        .unwrap_or("20")
        .to_string(),
        mev_mode: {
            let route_mode = route_mev_mode(&sell_route);
            if matches!(route_mode, MevMode::Off) {
                if matches!(preset.sell_mev_mode, MevMode::Off) {
                    settings.default_sell_mev_mode.clone()
                } else {
                    preset.sell_mev_mode.clone()
                }
            } else {
                route_mode
            }
        },
        auto_tip_enabled,
        fee_sol,
        tip_sol,
        provider: resolved_provider,
        endpoint_profile: first_non_empty(&[
            Some(route_endpoint_profile.as_str()),
            Some(preset.sell_endpoint_profile.as_str()),
            Some(settings.execution_endpoint_profile.as_str()),
        ])
        .unwrap_or("")
        .to_string(),
        commitment: settings.execution_commitment.clone(),
        skip_preflight: settings.execution_skip_preflight,
        track_send_block_height: config_track_send_block_height(config),
        buy_amount_sol: None,
        sell_percent: first_non_empty(&[
            Some(preset.sell_percent.as_str()),
            preset
                .sell_amounts_percent
                .iter()
                .find(|value| !value.trim().is_empty())
                .map(String::as_str),
        ])
        .map(str::to_string),
        buy_funding_policy: settings.default_buy_funding_policy,
        sell_settlement_policy,
        sell_settlement_asset: resolve_sell_settlement_asset(sell_settlement_policy, None),
        auto_fee_warnings,
    }
}

fn resolve_sell_intent(
    sell_percent: Option<&str>,
    sell_output_sol: Option<&str>,
    preset_sell_percent: Option<&str>,
) -> Result<SellIntent, (StatusCode, String)> {
    let percent = sell_percent.and_then(trimmed_option);
    let sol_output = sell_output_sol.and_then(trimmed_option);
    match (percent, sol_output) {
        (Some(_), Some(_)) => Err((
            StatusCode::BAD_REQUEST,
            "provide either sellPercent or sellOutputSol, not both".to_string(),
        )),
        (Some(percent), None) => Ok(SellIntent::Percent(percent.to_string())),
        (None, Some(sol_output)) => Ok(SellIntent::SolOutput(sol_output.to_string())),
        (None, None) => Ok(SellIntent::Percent(
            preset_sell_percent
                .and_then(trimmed_option)
                .unwrap_or("100")
                .to_string(),
        )),
    }
}

fn validate_sell_intent_for_family(
    sell_intent: &SellIntent,
    selector: &LifecycleAndCanonicalMarket,
) -> Result<(), (StatusCode, String)> {
    if !matches!(sell_intent, SellIntent::SolOutput(_)) {
        return Ok(());
    }
    if matches!(selector.family, TradeVenueFamily::TrustedStableSwap) {
        let output_mint = match selector.runtime_bundle.as_ref() {
            Some(PlannerRuntimeBundle::TrustedStable(bundle)) => bundle.sell_output_mint.as_str(),
            _ => {
                crate::stable_native::trusted_stable_route_for_pool(&selector.canonical_market_key)
                    .map(|route| route.sell_output_mint)
                    .unwrap_or_default()
            }
        };
        if output_mint != crate::wrapper_abi::WSOL_MINT.to_string() {
            return Err((
                StatusCode::BAD_REQUEST,
                "Trusted stable sellOutputSol requires a SOL-output route.".to_string(),
            ));
        }
    }
    if matches!(
        selector.family,
        TradeVenueFamily::MeteoraDbc | TradeVenueFamily::MeteoraDammV2
    ) && matches!(
        selector.quote_asset,
        PlannerQuoteAsset::Usd1 | PlannerQuoteAsset::Usdt
    ) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Meteora sellOutputSol stable routing is currently USDC-only.".to_string(),
        ));
    }
    Ok(())
}

fn build_settings_response(base: &EngineSettings) -> EngineSettings {
    let shared_rpc = shared_config_manager().current_snapshot().rpc;
    let mut settings = base.clone();
    settings.rpc_url = shared_rpc.rpc_url;
    settings.ws_url = shared_rpc.ws_url;
    settings.warm_rpc_url = shared_rpc.warm_rpc_url;
    settings.warm_ws_url = shared_rpc.warm_ws_url;
    settings.shared_region = shared_rpc.shared_region;
    settings.helius_rpc_url = shared_rpc.helius_rpc_url;
    settings.helius_ws_url = shared_rpc.helius_ws_url;
    settings.standard_rpc_send_urls = shared_rpc.standard_rpc_send_urls;
    settings.helius_sender_region = shared_rpc.helius_sender_region;
    // The options UI renders the region selector from
    // `executionEndpointProfile`. For non-`helius-sender` providers the
    // stored value is intentionally blank, which would make the selector
    // display `global` even though `USER_REGION`/`USER_REGION_HELIUS_SENDER`
    // are set. Surface the effective shared region here so GET responses
    // accurately reflect the persisted routing for display purposes.
    if settings.execution_endpoint_profile.trim().is_empty() {
        let fallback = if !settings.shared_region.trim().is_empty() {
            settings.shared_region.clone()
        } else {
            settings.helius_sender_region.clone()
        };
        let fallback = fallback.trim();
        if !fallback.is_empty() {
            settings.execution_endpoint_profile = fallback.to_string();
        }
    }
    settings
}

fn shared_rpc_config_from_settings(settings: &EngineSettings) -> SharedRpcConfig {
    SharedRpcConfig {
        rpc_url: settings.rpc_url.clone(),
        ws_url: settings.ws_url.clone(),
        warm_rpc_url: settings.warm_rpc_url.clone(),
        warm_ws_url: settings.warm_ws_url.clone(),
        shared_region: settings.shared_region.clone(),
        helius_rpc_url: settings.helius_rpc_url.clone(),
        helius_ws_url: settings.helius_ws_url.clone(),
        standard_rpc_send_urls: settings.standard_rpc_send_urls.clone(),
        helius_sender_region: settings.helius_sender_region.clone(),
    }
}

fn current_canonical_config(engine: &StoredEngineState) -> Value {
    engine
        .config
        .clone()
        .map(normalize_canonical_config)
        .unwrap_or_else(default_canonical_config)
}

fn build_launchdeck_region_routing_payload(settings: &EngineSettings) -> Value {
    json!({
        "shared": {
            "configured": settings.shared_region,
            "effective": if settings.shared_region.trim().is_empty() { "global" } else { settings.shared_region.as_str() },
        },
        "providers": {
            "helius-sender": {
                "configured": settings.helius_sender_region,
                "effective": if settings.helius_sender_region.trim().is_empty() { "global" } else { settings.helius_sender_region.as_str() },
                "endpointOverrideActive": false,
            },
            "hellomoon": {
                "configured": execution_configured_provider_region("hellomoon"),
                "effective": execution_default_endpoint_profile_for_provider("hellomoon"),
                "endpointOverrideActive": false,
            }
        },
        "restartRequired": false,
    })
}

fn build_launchdeck_settings_payload(engine: &StoredEngineState) -> Value {
    json!({
        "ok": true,
        "configVersion": CANONICAL_CONFIG_VERSION,
        "schemaVersion": CANONICAL_CONFIG_SCHEMA_VERSION,
        "config": current_canonical_config(engine),
        "defaults": default_canonical_config(),
        "providers": provider_availability_registry(),
        "providerRegistry": provider_registry(),
        "launchpads": launchpad_registry(),
        "strategies": strategy_registry(),
        "regionRouting": build_launchdeck_region_routing_payload(&build_settings_response(&engine.settings)),
        "engine": {
            "backend": "execution-engine",
            "url": host_bind_address(),
        },
    })
}

const TOKEN_ACCOUNT_AMOUNT_OFFSET: usize = 64;
const TOKEN_ACCOUNT_AMOUNT_LEN: usize = 8;
const MINT_DECIMALS_OFFSET: usize = 44;
const MAX_MULTIPLE_ACCOUNTS_BATCH_SIZE: usize = 100;

#[derive(Debug, Clone, Default)]
struct MintBalanceSnapshot {
    raw: Option<u64>,
    ui_amount: Option<f64>,
    decimals: Option<u8>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct WalletStatusView {
    key: String,
    label: String,
    enabled: bool,
    public_key: Option<String>,
    error: Option<String>,
    balance_lamports: Option<u64>,
    balance_sol: Option<f64>,
    usd1_balance: Option<f64>,
    balance_error: Option<String>,
    mint_balance: MintBalanceSnapshot,
}

fn extension_wallet_rpc_client() -> Result<Client, String> {
    Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| format!("Failed to build wallet RPC client: {error}"))
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
        let result = crate::rpc_client::rpc_request_with_client(
            client,
            rpc_url,
            "getMultipleAccounts",
            json!([
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
                use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
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

fn parse_mint_decimals(data: &[u8]) -> Result<u8, String> {
    if data.len() <= MINT_DECIMALS_OFFSET {
        return Err("Mint account data was too short to contain decimals.".to_string());
    }
    Ok(data[MINT_DECIMALS_OFFSET])
}

async fn fetch_mint_metadata_with_client(
    _client: &Client,
    rpc_url: &str,
    mint: &str,
    commitment: &str,
) -> Result<(u8, Pubkey), String> {
    let owner_and_data =
        crate::rpc_client::fetch_account_owner_and_data(rpc_url, mint, commitment).await?;
    let (token_program, mint_data) =
        owner_and_data.ok_or_else(|| format!("Mint account {mint} was not found."))?;
    let decimals = parse_mint_decimals(&mint_data)
        .map_err(|error| format!("Mint account {mint} had invalid decimals data: {error}"))?;
    Ok((decimals, token_program))
}

async fn fetch_wallet_mint_balances(
    rpc_url: &str,
    wallets: &[WalletStatusView],
    mint: &str,
) -> Result<HashMap<String, MintBalanceSnapshot>, String> {
    if mint.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let client = extension_wallet_rpc_client()?;
    let (decimals, token_program) =
        fetch_mint_metadata_with_client(&client, rpc_url, mint, "confirmed").await?;
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid mint {mint}: {error}"))?;
    let mut wallet_keys = Vec::new();
    let mut public_keys = Vec::new();
    let mut ata_accounts = Vec::new();
    for wallet in wallets {
        let Some(public_key) = wallet.public_key.as_ref() else {
            continue;
        };
        let owner_pubkey = Pubkey::from_str(public_key)
            .map_err(|error| format!("Invalid wallet public key {public_key}: {error}"))?;
        wallet_keys.push(wallet.key.clone());
        public_keys.push(public_key.clone());
        ata_accounts.push(
            get_associated_token_address_with_program_id(
                &owner_pubkey,
                &mint_pubkey,
                &token_program,
            )
            .to_string(),
        );
    }
    if wallet_keys.is_empty() {
        return Ok(HashMap::new());
    }
    let account_data =
        fetch_multiple_account_data_with_client(&client, rpc_url, &ata_accounts, "confirmed")
            .await?;
    if account_data.len() != wallet_keys.len() {
        return Err("Mint balance results did not match the wallet count.".to_string());
    }
    let scale = 10_f64.powi(i32::from(decimals));
    let mut snapshots = HashMap::new();
    for (index, wallet_key) in wallet_keys.iter().enumerate() {
        let snapshot = match account_data[index].as_ref() {
            Some(data) => match parse_token_account_raw_balance(data) {
                Ok(raw_amount) => MintBalanceSnapshot {
                    raw: Some(raw_amount),
                    ui_amount: Some(raw_amount as f64 / scale),
                    decimals: Some(decimals),
                    error: None,
                },
                Err(primary_error) => {
                    match crate::rpc_client::fetch_token_balance_via_ata(
                        &public_keys[index],
                        mint,
                        decimals,
                        "confirmed",
                    )
                    .await
                    {
                        Ok(fallback) => MintBalanceSnapshot {
                            raw: Some(fallback.amount_raw),
                            ui_amount: Some(
                                fallback.amount_raw as f64
                                    / 10_f64.powi(i32::from(fallback.decimals)),
                            ),
                            decimals: Some(fallback.decimals),
                            error: None,
                        },
                        Err(_) => MintBalanceSnapshot {
                            raw: None,
                            ui_amount: None,
                            decimals: Some(decimals),
                            error: Some(primary_error),
                        },
                    }
                }
            },
            None => MintBalanceSnapshot {
                raw: Some(0),
                ui_amount: Some(0.0),
                decimals: Some(decimals),
                error: None,
            },
        };
        snapshots.insert(wallet_key.clone(), snapshot);
    }
    Ok(snapshots)
}

fn resolve_wallet_status_target(
    wallets: &[WalletSummary],
    wallet_groups: &[WalletGroupSummary],
    request: &ExtensionWalletStatusRequest,
) -> Result<ResolvedBatchTarget, (StatusCode, String)> {
    let has_selector = request.wallet_key.is_some()
        || request
            .wallet_keys
            .as_ref()
            .is_some_and(|keys| !keys.is_empty())
        || request.wallet_group_id.is_some();
    if has_selector {
        return resolve_batch_target(
            wallets,
            wallet_groups,
            request.wallet_key.clone(),
            request.wallet_keys.clone(),
            request.wallet_group_id.clone(),
        );
    }
    let mut wallet_keys = wallets
        .iter()
        .filter(|wallet| wallet.enabled)
        .map(|wallet| wallet.key.clone())
        .collect::<Vec<_>>();
    if wallet_keys.is_empty() {
        if request.include_disabled {
            wallet_keys = wallets
                .iter()
                .map(|wallet| wallet.key.clone())
                .collect::<Vec<_>>();
        }
        if wallet_keys.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "no enabled wallets are available".to_string(),
            ));
        }
    }
    Ok(ResolvedBatchTarget {
        selection_mode: if wallet_keys.len() == 1 {
            BatchSelectionMode::SingleWallet
        } else {
            BatchSelectionMode::WalletList
        },
        wallet_group_id: None,
        wallet_group_label: None,
        batch_policy: None,
        wallet_keys: wallet_keys.clone(),
        wallet_count: wallet_keys.len(),
    })
}

#[derive(Debug, Clone)]
struct ActiveTokenSolQuote {
    value_lamports: u64,
    source: String,
    quote_asset: String,
    quote_age_ms: u64,
}

#[derive(Debug, Clone)]
struct CachedActiveTokenSolQuote {
    fetched_at: Instant,
    quote: ActiveTokenSolQuote,
}

#[derive(Debug, Clone)]
struct CachedActiveQuoteSelector {
    fetched_at: Instant,
    selector: LifecycleAndCanonicalMarket,
}

#[derive(Debug, Clone)]
struct CachedStableSolQuote {
    fetched_at: Instant,
    lamports: u64,
}

fn active_token_quote_cache()
-> &'static tokio::sync::Mutex<HashMap<String, CachedActiveTokenSolQuote>> {
    static CACHE: OnceLock<tokio::sync::Mutex<HashMap<String, CachedActiveTokenSolQuote>>> =
        OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn active_token_selector_cache()
-> &'static tokio::sync::Mutex<HashMap<String, CachedActiveQuoteSelector>> {
    static CACHE: OnceLock<tokio::sync::Mutex<HashMap<String, CachedActiveQuoteSelector>>> =
        OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn active_token_quote_flights()
-> &'static tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>> {
    static FLIGHTS: OnceLock<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    FLIGHTS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn stable_quote_cache() -> &'static tokio::sync::Mutex<HashMap<String, CachedStableSolQuote>> {
    static CACHE: OnceLock<tokio::sync::Mutex<HashMap<String, CachedStableSolQuote>>> =
        OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn active_token_quote_ttl(selector: &LifecycleAndCanonicalMarket) -> Duration {
    match selector.lifecycle {
        crate::trade_planner::TradeLifecycle::PreMigration => Duration::from_millis(1_500),
        crate::trade_planner::TradeLifecycle::PostMigration => Duration::from_millis(3_000),
    }
}

fn active_quote_value_cache_key(
    rpc_url: &str,
    commitment: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
) -> String {
    format!(
        "rpc={}|cmt={}|family={}|market={}|quote={}|mint={}|amount={}",
        rpc_url,
        commitment,
        selector.family.label(),
        selector.canonical_market_key,
        selector.quote_asset.label(),
        mint,
        token_amount_raw
    )
}

fn active_quote_route_cache_key(
    rpc_url: &str,
    commitment: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
) -> String {
    format!(
        "rpc={}|cmt={}|family={}|market={}|quote={}|mint={}",
        rpc_url,
        commitment,
        selector.family.label(),
        selector.canonical_market_key,
        selector.quote_asset.label(),
        mint,
    )
}

fn wallet_status_hint(value: &Option<String>) -> String {
    value
        .as_deref()
        .and_then(trimmed_option)
        .unwrap_or_default()
        .to_string()
}

fn active_quote_request_cache_key(
    rpc_url: &str,
    commitment: &str,
    request: &ExtensionWalletStatusRequest,
    mint: &str,
) -> String {
    format!(
        "rpc={}|cmt={}|mint={}|preset={}|buyPolicy={}|sellPolicy={}|warm={}|route={}|pair={}|family={}|lifecycle={}|quote={}|market={}|source={}",
        rpc_url,
        commitment,
        mint,
        wallet_status_hint(&request.preset_id),
        request
            .buy_funding_policy
            .map(|policy| runtime_buy_funding_policy_label(policy))
            .unwrap_or_default(),
        request
            .sell_settlement_policy
            .map(|policy| runtime_sell_settlement_policy_label(policy))
            .unwrap_or_default(),
        wallet_status_hint(&request.warm_key),
        wallet_status_hint(&request.route_address),
        wallet_status_hint(&request.pair),
        wallet_status_hint(&request.family),
        wallet_status_hint(&request.lifecycle),
        wallet_status_hint(&request.quote_asset),
        wallet_status_hint(&request.canonical_market_key),
        wallet_status_hint(&request.source),
    )
}

fn active_mark_surface_id(request: &ExtensionActiveMarkRequest) -> String {
    request
        .surface_id
        .as_deref()
        .and_then(trimmed_option)
        .unwrap_or("default")
        .to_string()
}

fn live_mark_accounts_from_targets<'a>(
    targets: impl Iterator<Item = &'a LiveMarkTarget>,
) -> Vec<String> {
    let mut accounts = Vec::new();
    let mut seen = HashSet::new();
    for target in targets {
        for account in &target.watch_accounts {
            if seen.insert(account.clone()) {
                accounts.push(account.clone());
            }
        }
    }
    accounts
}

fn live_mark_target_matches(left: &LiveMarkTarget, right: &LiveMarkTarget) -> bool {
    left.surface_id == right.surface_id
        && left.mark_revision == right.mark_revision
        && left.mint == right.mint
        && left.wallet_keys == right.wallet_keys
        && left.wallet_group_id == right.wallet_group_id
        && left.selector.lifecycle == right.selector.lifecycle
        && left.selector.family == right.selector.family
        && left.selector.canonical_market_key == right.selector.canonical_market_key
        && left.selector.quote_asset == right.selector.quote_asset
        && left.selector.verification_source == right.selector.verification_source
        && left.selector.wrapper_action == right.selector.wrapper_action
        && left.selector.wrapper_accounts == right.selector.wrapper_accounts
        && left.selector.market_subtype == right.selector.market_subtype
        && left.selector.direct_protocol_target == right.selector.direct_protocol_target
        && left.selector.input_amount_hint == right.selector.input_amount_hint
        && left.selector.minimum_output_hint == right.selector.minimum_output_hint
        && left.selector.runtime_bundle == right.selector.runtime_bundle
        && left.watch_accounts == right.watch_accounts
        && left.token_decimals == right.token_decimals
        && left.token_balance_raw == right.token_balance_raw
}

async fn is_current_live_mark_target(state: &AppState, target: &LiveMarkTarget) -> bool {
    let targets = state.live_mark_targets.read().await;
    targets
        .get(&target.surface_id)
        .is_some_and(|current| live_mark_target_matches(current, target))
}

fn active_mark_wallet_status_request(
    request: &ExtensionActiveMarkRequest,
    mint: &str,
) -> ExtensionWalletStatusRequest {
    ExtensionWalletStatusRequest {
        wallet_key: request.wallet_key.clone(),
        wallet_keys: request.wallet_keys.clone(),
        wallet_group_id: request.wallet_group_id.clone(),
        mint: Some(mint.to_string()),
        preset_id: request.preset_id.clone(),
        buy_funding_policy: request.buy_funding_policy,
        sell_settlement_policy: request.sell_settlement_policy,
        quoted_price: None,
        route_address: request.route_address.clone(),
        pair: request.pair.clone(),
        warm_key: request.warm_key.clone(),
        family: request.family.clone(),
        lifecycle: request.lifecycle.clone(),
        quote_asset: request.quote_asset.clone(),
        canonical_market_key: request.canonical_market_key.clone(),
        surface: request.surface.clone(),
        page_url: request.page_url.clone(),
        source: request.source.clone(),
        include_disabled: false,
        read_only: true,
        force: false,
        skip_sol_balance: true,
        include_sol_balance: Some(false),
        include_usd1_balance: Some(false),
    }
}

fn live_mark_ui_amount_to_raw(amount: f64, decimals: Option<u8>) -> Option<u64> {
    let decimals = decimals?;
    if !amount.is_finite() || amount < 0.0 {
        return None;
    }
    let scale = 10u128.checked_pow(u32::from(decimals))?;
    let raw = (amount * scale as f64).round();
    if !raw.is_finite() || raw < 0.0 || raw > u64::MAX as f64 {
        return None;
    }
    Some(raw as u64)
}

fn aggregate_live_mark_raw(balances: &HashMap<String, f64>, decimals: Option<u8>) -> Option<u64> {
    let decimals = decimals?;
    let scale = 10u128.checked_pow(u32::from(decimals))?;
    let mut total = 0u64;
    let mut any = false;
    for amount in balances.values() {
        if !amount.is_finite() || *amount < 0.0 {
            continue;
        }
        let raw = (*amount * scale as f64).round();
        if !raw.is_finite() || raw < 0.0 || raw > u64::MAX as f64 {
            return None;
        }
        total = total.saturating_add(raw as u64);
        any = true;
    }
    any.then_some(total)
}

fn push_live_mark_account(accounts: &mut Vec<String>, account: impl AsRef<str>) {
    let account = account.as_ref().trim();
    if account.is_empty() || Pubkey::from_str(account).is_err() {
        return;
    }
    if !accounts.iter().any(|existing| existing == account) {
        accounts.push(account.to_string());
    }
}

fn live_mark_watch_accounts(selector: &LifecycleAndCanonicalMarket) -> Vec<String> {
    let mut accounts = Vec::new();
    match selector.runtime_bundle.as_ref() {
        Some(PlannerRuntimeBundle::PumpBondingCurve(bundle)) => {
            push_live_mark_account(&mut accounts, &bundle.bonding_curve);
            push_live_mark_account(&mut accounts, &bundle.associated_bonding_curve);
        }
        Some(PlannerRuntimeBundle::PumpAmm(bundle)) => {
            push_live_mark_account(&mut accounts, &bundle.pool);
            push_live_mark_account(&mut accounts, &bundle.pool_base_token_account);
            push_live_mark_account(&mut accounts, &bundle.pool_quote_token_account);
        }
        Some(PlannerRuntimeBundle::RaydiumAmmV4(bundle)) => {
            push_live_mark_account(&mut accounts, &bundle.pool);
            push_live_mark_account(&mut accounts, &bundle.base_vault);
            push_live_mark_account(&mut accounts, &bundle.quote_vault);
            push_live_mark_account(&mut accounts, &bundle.open_orders);
        }
        Some(PlannerRuntimeBundle::RaydiumCpmm(bundle)) => {
            push_live_mark_account(&mut accounts, &bundle.pool);
            push_live_mark_account(&mut accounts, &bundle.vault_a);
            push_live_mark_account(&mut accounts, &bundle.vault_b);
            push_live_mark_account(&mut accounts, &bundle.observation_id);
        }
        Some(PlannerRuntimeBundle::Bags(bundle)) => {
            push_live_mark_account(
                &mut accounts,
                &bundle.bags_launch.preMigrationDbcPoolAddress,
            );
            push_live_mark_account(
                &mut accounts,
                &bundle.bags_launch.postMigrationDammPoolAddress,
            );
        }
        Some(PlannerRuntimeBundle::TrustedStable(_)) | None => {}
    }
    if accounts.is_empty() {
        push_live_mark_account(
            &mut accounts,
            &selector.direct_protocol_target.clone().unwrap_or_default(),
        );
        push_live_mark_account(&mut accounts, &selector.canonical_market_key);
    }
    accounts
}

fn normalized_route_hint(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn quote_asset_hint_matches(hint: &str, actual: &str) -> bool {
    let hint = normalized_route_hint(hint);
    let actual = normalized_route_hint(actual);
    hint == actual || ((hint == "sol" || hint == "wsol") && (actual == "sol" || actual == "wsol"))
}

fn wallet_status_has_quote_policy_context(request: &ExtensionWalletStatusRequest) -> bool {
    request
        .preset_id
        .as_deref()
        .and_then(trimmed_option)
        .is_some()
        || request.buy_funding_policy.is_some()
        || request.sell_settlement_policy.is_some()
}

fn validate_wallet_status_quote_selector(
    selector: &LifecycleAndCanonicalMarket,
    request: &ExtensionWalletStatusRequest,
) -> Result<(), String> {
    let policy_context = wallet_status_has_quote_policy_context(request);
    if let Some(expected) = request.family.as_deref().and_then(trimmed_option)
        && normalized_route_hint(expected) != normalized_route_hint(selector.family.label())
    {
        return Err(format!(
            "Resolved quote route family {} did not match wallet-status hint {}.",
            selector.family.label(),
            expected
        ));
    }
    if let Some(expected) = request.lifecycle.as_deref().and_then(trimmed_option)
        && normalized_route_hint(expected) != normalized_route_hint(selector.lifecycle.label())
    {
        return Err(format!(
            "Resolved quote route lifecycle {} did not match wallet-status hint {}.",
            selector.lifecycle.label(),
            expected
        ));
    }
    if let Some(expected) = request.quote_asset.as_deref().and_then(trimmed_option)
        && !policy_context
        && !quote_asset_hint_matches(expected, selector.quote_asset.label())
    {
        return Err(format!(
            "Resolved quote route asset {} did not match wallet-status hint {}.",
            selector.quote_asset.label(),
            expected
        ));
    }
    if let Some(expected) = request
        .canonical_market_key
        .as_deref()
        .and_then(trimmed_option)
        && !policy_context
        && expected != selector.canonical_market_key
    {
        return Err(format!(
            "Resolved quote route market {} did not match wallet-status hint {}.",
            selector.canonical_market_key, expected
        ));
    }
    Ok(())
}

fn wallet_status_default_quote_policy(engine: &StoredEngineState) -> RuntimeExecutionPolicy {
    RuntimeExecutionPolicy {
        slippage_percent: engine.settings.default_buy_slippage_percent.clone(),
        mev_mode: engine.settings.default_buy_mev_mode.clone(),
        auto_tip_enabled: false,
        fee_sol: String::new(),
        tip_sol: String::new(),
        provider: engine.settings.execution_provider.clone(),
        endpoint_profile: engine.settings.execution_endpoint_profile.clone(),
        commitment: engine.settings.execution_commitment.clone(),
        skip_preflight: engine.settings.execution_skip_preflight,
        track_send_block_height: engine.settings.track_send_block_height,
        buy_funding_policy: engine.settings.default_buy_funding_policy,
        sell_settlement_policy: engine.settings.default_sell_settlement_policy,
        sell_settlement_asset: resolve_sell_settlement_asset(
            engine.settings.default_sell_settlement_policy,
            None,
        ),
    }
}

fn wallet_status_quote_policy(
    engine: &StoredEngineState,
    request: &ExtensionWalletStatusRequest,
) -> Result<RuntimeExecutionPolicy, String> {
    let Some(preset_id) = request.preset_id.as_deref().and_then(trimmed_option) else {
        return Ok(wallet_status_default_quote_policy(engine));
    };
    let preset = resolve_preset(&engine.presets, preset_id).map_err(|(_, error)| error)?;
    let config = engine.config.as_ref().unwrap_or(&Value::Null);
    let buy_policy = resolve_buy_policy(
        &engine.settings,
        config,
        preset,
        None,
        request.buy_funding_policy,
    );
    let sell_policy = resolve_sell_policy(
        &engine.settings,
        config,
        preset,
        request.sell_settlement_policy,
    );
    Ok(RuntimeExecutionPolicy {
        slippage_percent: buy_policy.slippage_percent,
        mev_mode: buy_policy.mev_mode,
        auto_tip_enabled: false,
        fee_sol: String::new(),
        tip_sol: String::new(),
        provider: buy_policy.provider,
        endpoint_profile: buy_policy.endpoint_profile,
        commitment: buy_policy.commitment,
        skip_preflight: buy_policy.skip_preflight,
        track_send_block_height: buy_policy.track_send_block_height,
        buy_funding_policy: buy_policy.buy_funding_policy,
        sell_settlement_policy: sell_policy.sell_settlement_policy,
        sell_settlement_asset: sell_policy.sell_settlement_asset,
    })
}

async fn wallet_status_selector_from_warm_key(
    request: &ExtensionWalletStatusRequest,
    mint: &str,
) -> Option<LifecycleAndCanonicalMarket> {
    if wallet_status_has_quote_policy_context(request) {
        return None;
    }
    let warm_key = request.warm_key.as_deref()?.trim();
    if warm_key.is_empty() {
        return None;
    }
    let warm = crate::mint_warm_cache::shared_mint_warm_cache()
        .current_by_warm_key(warm_key)
        .await?;
    if warm.mint.trim() != mint {
        return None;
    }
    warm.plan.map(|plan| plan.selector)
}

async fn resolve_wallet_status_quote_selector(
    engine: &StoredEngineState,
    request: &ExtensionWalletStatusRequest,
    mint: &str,
) -> Result<LifecycleAndCanonicalMarket, String> {
    if let Some(selector) = wallet_status_selector_from_warm_key(request, mint).await {
        validate_wallet_status_quote_selector(&selector, request)?;
        return Ok(selector);
    }
    let route_input = request
        .route_address
        .as_deref()
        .and_then(trimmed_option)
        .or_else(|| {
            request
                .canonical_market_key
                .as_deref()
                .and_then(trimmed_option)
        })
        .unwrap_or(mint);
    let companion_pair =
        route_companion_pair(request.pair.as_deref(), None).map_err(|(_, error)| error)?;
    let policy = wallet_status_quote_policy(engine, request)?;
    let runtime_request = TradeRuntimeRequest {
        side: TradeSide::Buy,
        mint: route_input.to_string(),
        buy_amount_sol: None,
        sell_intent: None,
        policy,
        platform_label: request.source.clone(),
        planned_route: None,
        planned_trade: None,
        pinned_pool: companion_pair,
        warm_key: request.warm_key.clone(),
    };
    let plan = resolve_trade_plan(&runtime_request).await?;
    if plan.resolved_mint.trim() != mint {
        return Err(format!(
            "Resolved quote route mint {} did not match wallet-status mint {}.",
            plan.resolved_mint, mint
        ));
    }
    validate_wallet_status_quote_selector(&plan.selector, request)?;
    Ok(plan.selector)
}

async fn quote_usd1_to_sol_cached(
    rpc_url: &str,
    commitment: &str,
    usd1_raw: u64,
) -> Result<u64, String> {
    quote_usd1_to_sol_with_cache(rpc_url, commitment, usd1_raw, true).await
}

async fn quote_usd1_to_sol_fresh(
    rpc_url: &str,
    commitment: &str,
    usd1_raw: u64,
) -> Result<u64, String> {
    quote_usd1_to_sol_with_cache(rpc_url, commitment, usd1_raw, false).await
}

async fn quote_usd1_to_sol_with_cache(
    rpc_url: &str,
    commitment: &str,
    usd1_raw: u64,
    use_cache: bool,
) -> Result<u64, String> {
    if usd1_raw == 0 {
        return Ok(0);
    }
    let key = format!("rpc={rpc_url}|cmt={commitment}|usd1={usd1_raw}");
    if use_cache {
        let cache = stable_quote_cache().lock().await;
        if let Some(entry) = cache.get(&key)
            && entry.fetched_at.elapsed() <= Duration::from_millis(3_000)
        {
            return Ok(entry.lamports);
        }
    }
    let max_setup_age = if use_cache {
        Duration::from_millis(3_000)
    } else {
        Duration::ZERO
    };
    let lamports =
        crate::bonk_execution_support::quote_sol_lamports_for_exact_usd1_input_with_max_setup_age(
            rpc_url,
            usd1_raw,
            max_setup_age,
        )
        .await?;
    if use_cache {
        let mut cache = stable_quote_cache().lock().await;
        cache.insert(
            key,
            CachedStableSolQuote {
                fetched_at: Instant::now(),
                lamports,
            },
        );
        if cache.len() > 256 {
            cache.retain(|_, entry| entry.fetched_at.elapsed() <= Duration::from_secs(30));
        }
    }
    Ok(lamports)
}

async fn quote_active_token_value_lamports_for_rpc(
    rpc_url: &str,
    commitment: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
    use_cache: bool,
) -> Result<u64, String> {
    match selector.family {
        TradeVenueFamily::PumpBondingCurve | TradeVenueFamily::PumpAmm => {
            if use_cache {
                crate::pump_native::quote_pump_holding_value_sol(
                    rpc_url,
                    selector,
                    mint,
                    token_amount_raw,
                    commitment,
                )
                .await
            } else {
                crate::pump_native::quote_pump_holding_value_sol_fresh(
                    rpc_url,
                    selector,
                    mint,
                    token_amount_raw,
                    commitment,
                )
                .await
            }
        }
        TradeVenueFamily::RaydiumAmmV4 => {
            if use_cache {
                crate::raydium_amm_v4_native::quote_raydium_amm_v4_holding_value_sol(
                    rpc_url,
                    selector,
                    mint,
                    token_amount_raw,
                    commitment,
                )
                .await
            } else {
                crate::raydium_amm_v4_native::quote_raydium_amm_v4_holding_value_sol_fresh(
                    rpc_url,
                    selector,
                    mint,
                    token_amount_raw,
                    commitment,
                )
                .await
            }
        }
        TradeVenueFamily::RaydiumCpmm => {
            crate::raydium_cpmm_native::quote_raydium_cpmm_holding_value_sol(
                rpc_url,
                selector,
                mint,
                token_amount_raw,
                commitment,
            )
            .await
        }
        TradeVenueFamily::RaydiumLaunchLab => {
            crate::raydium_launchlab_native::quote_raydium_launchlab_token_value_lamports(
                rpc_url,
                commitment,
                selector,
                mint,
                token_amount_raw,
            )
            .await
        }
        TradeVenueFamily::BonkLaunchpad | TradeVenueFamily::BonkRaydium => {
            let quote_asset = match selector.quote_asset {
                PlannerQuoteAsset::Sol | PlannerQuoteAsset::Wsol => "sol",
                PlannerQuoteAsset::Usd1 => "usd1",
                PlannerQuoteAsset::Usdc | PlannerQuoteAsset::Usdt => {
                    return Err(format!(
                        "Unsupported Bonk holding quote asset {}.",
                        selector.quote_asset.label()
                    ));
                }
            };
            let (quote_raw, quote_asset) = if use_cache {
                crate::bonk_execution_support::quote_bonk_holding_value_quote_raw(
                    rpc_url,
                    mint,
                    Some(&selector.canonical_market_key),
                    quote_asset,
                    token_amount_raw,
                    commitment,
                )
                .await?
            } else {
                crate::bonk_execution_support::quote_bonk_holding_value_quote_raw_fresh(
                    rpc_url,
                    mint,
                    Some(&selector.canonical_market_key),
                    quote_asset,
                    token_amount_raw,
                    commitment,
                )
                .await?
            };
            if quote_asset.eq_ignore_ascii_case("usd1") {
                if use_cache {
                    quote_usd1_to_sol_cached(rpc_url, commitment, quote_raw).await
                } else {
                    quote_usd1_to_sol_fresh(rpc_url, commitment, quote_raw).await
                }
            } else {
                Ok(quote_raw)
            }
        }
        TradeVenueFamily::MeteoraDbc | TradeVenueFamily::MeteoraDammV2 => {
            let bags_launch = crate::meteora_native::selector_to_bags_launch(selector);
            if use_cache {
                crate::bags_execution_support::quote_bags_holding_value_sol(
                    rpc_url,
                    mint,
                    token_amount_raw,
                    commitment,
                    Some(&bags_launch),
                )
                .await
            } else {
                crate::bags_execution_support::quote_bags_holding_value_sol_fresh(
                    rpc_url,
                    mint,
                    token_amount_raw,
                    commitment,
                    Some(&bags_launch),
                )
                .await
            }
        }
        TradeVenueFamily::TrustedStableSwap => {
            Err("Trusted stable routes are not token holding quote markets.".to_string())
        }
    }
}

async fn quote_active_token_value_lamports_for_rpc_cached(
    rpc_url: &str,
    commitment: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
) -> Result<u64, String> {
    quote_active_token_value_lamports_for_rpc(
        rpc_url,
        commitment,
        selector,
        mint,
        token_amount_raw,
        true,
    )
    .await
}

async fn quote_active_token_value_lamports_for_rpc_fresh(
    rpc_url: &str,
    commitment: &str,
    selector: &LifecycleAndCanonicalMarket,
    mint: &str,
    token_amount_raw: u64,
) -> Result<u64, String> {
    quote_active_token_value_lamports_for_rpc(
        rpc_url,
        commitment,
        selector,
        mint,
        token_amount_raw,
        false,
    )
    .await
}

async fn quote_active_token_value_sol(
    engine: &StoredEngineState,
    request: &ExtensionWalletStatusRequest,
    mint: &str,
    token_amount_raw: u64,
) -> Result<ActiveTokenSolQuote, String> {
    if token_amount_raw == 0 {
        return Ok(ActiveTokenSolQuote {
            value_lamports: 0,
            source: "zero-balance".to_string(),
            quote_asset: "SOL".to_string(),
            quote_age_ms: 0,
        });
    }
    let rpc_url = configured_rpc_url();
    let commitment = engine.settings.execution_commitment.as_str();
    let request_cache_key = active_quote_request_cache_key(&rpc_url, commitment, request, mint);
    let request_flight = {
        let mut flights = active_token_quote_flights().lock().await;
        if flights.len() > 256 {
            flights.retain(|_, flight| Arc::strong_count(flight) > 1);
        }
        flights
            .entry(format!("request|{request_cache_key}"))
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _request_flight_guard = request_flight.lock().await;
    let selector = {
        let cache = active_token_selector_cache().lock().await;
        cache
            .get(&request_cache_key)
            .filter(|entry| entry.fetched_at.elapsed() <= Duration::from_millis(1_500))
            .map(|entry| entry.selector.clone())
    };
    let selector = if let Some(selector) = selector {
        selector
    } else {
        let selector = resolve_wallet_status_quote_selector(engine, request, mint).await?;
        {
            let mut cache = active_token_selector_cache().lock().await;
            cache.insert(
                request_cache_key,
                CachedActiveQuoteSelector {
                    fetched_at: Instant::now(),
                    selector: selector.clone(),
                },
            );
            if cache.len() > 256 {
                cache.retain(|_, entry| entry.fetched_at.elapsed() <= Duration::from_secs(30));
            }
        }
        selector
    };
    let cache_key =
        active_quote_value_cache_key(&rpc_url, commitment, &selector, mint, token_amount_raw);
    let route_cache_key = active_quote_route_cache_key(&rpc_url, commitment, &selector, mint);
    let ttl = active_token_quote_ttl(&selector);
    {
        let cache = active_token_quote_cache().lock().await;
        if let Some(entry) = cache.get(&cache_key)
            && entry.fetched_at.elapsed() <= ttl
        {
            let mut quote = entry.quote.clone();
            quote.quote_age_ms = entry
                .fetched_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            return Ok(quote);
        }
    }
    let flight = {
        let mut flights = active_token_quote_flights().lock().await;
        if flights.len() > 256 {
            flights.retain(|_, flight| Arc::strong_count(flight) > 1);
        }
        flights
            .entry(route_cache_key)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _flight_guard = flight.lock().await;
    {
        let cache = active_token_quote_cache().lock().await;
        if let Some(entry) = cache.get(&cache_key)
            && entry.fetched_at.elapsed() <= ttl
        {
            let mut quote = entry.quote.clone();
            quote.quote_age_ms = entry
                .fetched_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            return Ok(quote);
        }
    }
    let value_lamports = quote_active_token_value_lamports_for_rpc_cached(
        &rpc_url,
        commitment,
        &selector,
        mint,
        token_amount_raw,
    )
    .await?;
    let quote = ActiveTokenSolQuote {
        value_lamports,
        source: selector.family.label().to_string(),
        quote_asset: selector.quote_asset.label().to_string(),
        quote_age_ms: 0,
    };
    {
        let mut cache = active_token_quote_cache().lock().await;
        cache.insert(
            cache_key,
            CachedActiveTokenSolQuote {
                fetched_at: Instant::now(),
                quote: quote.clone(),
            },
        );
        if cache.len() > 256 {
            cache.retain(|_, entry| entry.fetched_at.elapsed() <= Duration::from_secs(30));
        }
    }
    Ok(quote)
}

fn live_mark_pnl_payload(
    trade_ledger: &HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
    target: &LiveMarkTarget,
    holding_value_lamports: u64,
    slot: Option<u64>,
) -> MarkEventPayload {
    let trade_summary = aggregate_trade_ledger(trade_ledger, &target.wallet_keys, &target.mint);
    let holding_value_sol = holding_value_lamports as f64 / 1_000_000_000.0;
    let tracked_bought_sol = trade_summary.tracked_bought_lamports as f64 / 1_000_000_000.0;
    let remaining_cost_basis_sol =
        trade_summary.remaining_cost_basis_lamports as f64 / 1_000_000_000.0;
    let realized_pnl_gross_sol = trade_summary.realized_pnl_gross_lamports as f64 / 1_000_000_000.0;
    let explicit_fee_total_sol = trade_summary.explicit_fee_total_lamports as f64 / 1_000_000_000.0;
    let unrealized_pnl_gross_sol = holding_value_sol - remaining_cost_basis_sol;
    let pnl_gross = realized_pnl_gross_sol + unrealized_pnl_gross_sol;
    let pnl_net = pnl_gross - explicit_fee_total_sol;
    let pnl_percent_gross = if tracked_bought_sol > 0.0 {
        Some((pnl_gross / tracked_bought_sol) * 100.0)
    } else {
        None
    };
    let pnl_percent_net = if tracked_bought_sol > 0.0 {
        Some((pnl_net / tracked_bought_sol) * 100.0)
    } else {
        None
    };
    let token_balance = target.token_balance_raw.and_then(|raw| {
        target
            .token_decimals
            .map(|decimals| raw as f64 / 10_f64.powi(i32::from(decimals)))
    });
    MarkEventPayload {
        surface_id: Some(target.surface_id.clone()),
        mark_revision: target.mark_revision,
        mint: target.mint.clone(),
        wallet_keys: target.wallet_keys.clone(),
        wallet_group_id: target.wallet_group_id.clone(),
        token_balance,
        token_balance_raw: target.token_balance_raw,
        holding_value_sol: Some(holding_value_sol),
        holding: Some(holding_value_sol),
        pnl_gross: Some(pnl_gross),
        pnl_net: Some(pnl_net),
        pnl_percent_gross,
        pnl_percent_net,
        quote_source: Some(format!("live-mark:{}", target.selector.family.label())),
        commitment: Some("processed".to_string()),
        slot,
        at_ms: u128::from(now_unix_ms()),
    }
}

async fn publish_live_mark(state: &AppState, target: LiveMarkTarget, slot: Option<u64>) {
    let Some(token_amount_raw) = target.token_balance_raw else {
        return;
    };
    if !is_current_live_mark_target(state, &target).await {
        return;
    }
    let trade_ledger = state.trade_ledger.read().await.clone();
    let value_lamports = match quote_active_token_value_lamports_for_rpc_fresh(
        &configured_rpc_url(),
        "processed",
        &target.selector,
        &target.mint,
        token_amount_raw,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            eprintln!(
                "[execution-engine][live-mark] quote failed mint={} raw={} err={}",
                target.mint, token_amount_raw, error
            );
            return;
        }
    };
    if !is_current_live_mark_target(state, &target).await {
        return;
    }
    let payload = live_mark_pnl_payload(&trade_ledger, &target, value_lamports, slot);
    state.balance_stream().publish_mark_event(payload);
}

async fn update_live_mark_token_balance(
    state: &AppState,
    env_key: &str,
    mint: &str,
    amount: f64,
    slot: Option<u64>,
) {
    let targets = {
        let mut guard = state.live_mark_targets.write().await;
        let mut targets = Vec::new();
        for target in guard.values_mut() {
            if target.mint != mint || !target.wallet_keys.iter().any(|key| key == env_key) {
                continue;
            }
            target.mark_revision = target.mark_revision.saturating_add(1);
            target
                .token_balance_ui_by_wallet
                .insert(env_key.to_string(), amount);
            if let Some(raw) =
                aggregate_live_mark_raw(&target.token_balance_ui_by_wallet, target.token_decimals)
            {
                target.token_balance_raw = Some(raw);
            }
            targets.push(target.clone());
        }
        targets
    };
    for target in targets {
        publish_live_mark(state, target, slot).await;
    }
}

async fn update_live_mark_market_account(state: &AppState, account: &str, slot: Option<u64>) {
    let targets = {
        let mut guard = state.live_mark_targets.write().await;
        let mut targets = Vec::new();
        for target in guard.values_mut() {
            if !target.watch_accounts.iter().any(|watch| watch == account) {
                continue;
            }
            target.mark_revision = target.mark_revision.saturating_add(1);
            targets.push(target.clone());
        }
        targets
    };
    for target in targets {
        publish_live_mark(state, target, slot).await;
    }
}

async fn update_live_mark_trade_ledger(state: &AppState, slot: Option<u64>) {
    let targets = {
        let mut guard = state.live_mark_targets.write().await;
        let mut targets = Vec::new();
        for target in guard.values_mut() {
            target.mark_revision = target.mark_revision.saturating_add(1);
            targets.push(target.clone());
        }
        targets
    };
    for target in targets {
        publish_live_mark(state, target, slot).await;
    }
}

fn spawn_live_mark_task(state: AppState) {
    let mut events = state.balance_stream().subscribe_events();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(StreamEvent::Balance(payload)) => {
                    if let (Some(mint), Some(balance)) =
                        (payload.token_mint.as_deref(), payload.token_balance)
                    {
                        update_live_mark_token_balance(
                            &state,
                            &payload.env_key,
                            mint,
                            balance,
                            payload.slot,
                        )
                        .await;
                    }
                }
                Ok(StreamEvent::MarketAccount(payload)) => {
                    update_live_mark_market_account(&state, &payload.account, payload.slot).await;
                }
                Ok(StreamEvent::Trade(payload)) => {
                    if payload.ledger_applied == Some(true) {
                        update_live_mark_trade_ledger(&state, payload.slot).await;
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    eprintln!(
                        "[execution-engine][live-mark] broadcast lagged by {missed} events; skipping to latest."
                    );
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
fn active_quote_error_should_try_primary(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    let deterministic_quote_error = [
        "unsupported bonk holding quote asset",
        "trusted stable routes are not token holding quote markets",
        "was not found",
        "not found",
        "missing",
        "invalid",
        "unsupported",
        "unexpected",
        "too short",
        "shorter than expected",
        "reserves are empty",
        "pool reserves are empty",
        "liquidity",
        "division by zero",
        "fee configuration is invalid",
        "mints do not match",
        "does not match",
        "slippagepercent is out of range",
        "sell percent",
        "zero tokens",
        "you have 0 tokens",
    ]
    .iter()
    .any(|needle| normalized.contains(needle));
    if deterministic_quote_error {
        return false;
    }
    [
        "failed to fetch",
        "rpc",
        "http",
        "request",
        "response",
        "status",
        "timeout",
        "timed out",
        "connect",
        "connection",
        "network",
        "transport",
        "rate limit",
        "too many requests",
        "unauthorized",
        "forbidden",
        "429",
        "401",
        "403",
        "500",
        "502",
        "503",
        "504",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[cfg(test)]
fn wallet_status_has_balance_error(status: &WalletStatusSummary) -> bool {
    status.error.is_none()
        && status
            .balanceError
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
fn wallet_status_has_usable_balance(status: &WalletStatusSummary) -> bool {
    status.balanceError.is_none()
        && (status.balanceLamports.is_some()
            || status.balanceSol.is_some()
            || status.usd1Balance.is_some())
}

#[cfg(test)]
fn merge_wallet_status_primary_fallback(
    warm_statuses: Vec<WalletStatusSummary>,
    primary_statuses: Vec<WalletStatusSummary>,
) -> (Vec<WalletStatusSummary>, bool) {
    let primary_by_key = primary_statuses
        .into_iter()
        .map(|status| (status.envKey.clone(), status))
        .collect::<HashMap<_, _>>();
    let mut recovered_any = false;
    let merged = warm_statuses
        .into_iter()
        .map(|warm| {
            if !wallet_status_has_balance_error(&warm) {
                return warm;
            }
            let Some(primary) = primary_by_key.get(&warm.envKey) else {
                return warm;
            };
            if wallet_status_has_usable_balance(primary) {
                recovered_any = true;
                return primary.clone();
            }
            warm
        })
        .collect();
    (merged, recovered_any)
}

async fn build_extension_wallet_status_payload(
    engine: &StoredEngineState,
    trade_ledger: &HashMap<String, crate::trade_ledger::TradeLedgerEntry>,
    request: &ExtensionWalletStatusRequest,
) -> Result<(Value, Vec<String>), (StatusCode, String)> {
    let effective_wallets = build_effective_wallets(engine);
    let effective_wallet_groups = build_effective_wallet_groups(engine);
    let target =
        resolve_wallet_status_target(&effective_wallets, &effective_wallet_groups, request)?;
    let visible_wallets = effective_wallets
        .iter()
        .filter(|wallet| request.include_disabled || wallet.enabled)
        .cloned()
        .collect::<Vec<_>>();
    let visible_wallet_keys = visible_wallets
        .iter()
        .map(|wallet| wallet.key.clone())
        .collect::<HashSet<_>>();
    let wallet_order = visible_wallets
        .iter()
        .enumerate()
        .map(|(index, wallet)| (wallet.key.clone(), index))
        .collect::<HashMap<_, _>>();
    let primary_rpc_url = configured_rpc_url();
    let include_sol_balance = request
        .include_sol_balance
        .unwrap_or(!request.skip_sol_balance)
        && !request.skip_sol_balance;
    let include_usd1_balance = request
        .include_usd1_balance
        .unwrap_or(!request.skip_sol_balance);
    let runtime_wallets = runtime_wallets_from_wallet_summaries(&effective_wallets);
    let mut raw_wallet_statuses = if !include_sol_balance && !include_usd1_balance {
        runtime_wallets
            .clone()
            .into_iter()
            .map(|wallet| WalletStatusSummary {
                envKey: wallet.envKey,
                customName: wallet.customName,
                publicKey: wallet.publicKey,
                error: wallet.error,
                balanceLamports: None,
                balanceSol: None,
                usd1Balance: None,
                balanceError: None,
            })
            .collect::<Vec<_>>()
    } else {
        enrich_wallet_statuses_with_balance_options(
            &primary_rpc_url,
            USD1_MINT,
            &runtime_wallets,
            request.force,
            include_sol_balance,
            include_usd1_balance,
        )
        .await
    };
    raw_wallet_statuses.retain(|wallet| visible_wallet_keys.contains(&wallet.envKey));
    raw_wallet_statuses.sort_by_key(|wallet| {
        wallet_order
            .get(&wallet.envKey)
            .copied()
            .unwrap_or(usize::MAX)
    });
    let metadata_by_key = visible_wallets
        .iter()
        .cloned()
        .map(|wallet| (wallet.key.clone(), wallet))
        .collect::<HashMap<_, _>>();
    let mut wallets = raw_wallet_statuses
        .into_iter()
        .map(|wallet| {
            let metadata = metadata_by_key.get(&wallet.envKey);
            WalletStatusView {
                key: wallet.envKey.clone(),
                label: metadata
                    .map(|entry| entry.label.clone())
                    .unwrap_or_else(|| {
                        wallet
                            .customName
                            .clone()
                            .unwrap_or_else(|| wallet.envKey.clone())
                    }),
                enabled: metadata.map(|entry| entry.enabled).unwrap_or(true),
                public_key: wallet.publicKey.clone(),
                error: wallet.error.clone(),
                balance_lamports: wallet.balanceLamports,
                balance_sol: wallet.balanceSol,
                usd1_balance: wallet.usd1Balance,
                balance_error: wallet.balanceError.clone(),
                mint_balance: MintBalanceSnapshot::default(),
            }
        })
        .collect::<Vec<_>>();
    let requested_mint = request
        .mint
        .as_deref()
        .and_then(trimmed_option)
        .map(str::to_string);
    if let Some(mint) = requested_mint.as_deref() {
        let mint_balances = match fetch_wallet_mint_balances(&primary_rpc_url, &wallets, mint).await
        {
            Ok(balances) => balances,
            Err(error) => {
                eprintln!(
                    "[execution-engine][wallet-status] mint balance refresh failed mint={} wallets={} err={}",
                    mint,
                    wallets.len(),
                    error
                );
                wallets
                    .iter()
                    .map(|wallet| {
                        (
                            wallet.key.clone(),
                            MintBalanceSnapshot {
                                raw: None,
                                ui_amount: None,
                                decimals: None,
                                error: Some(error.clone()),
                            },
                        )
                    })
                    .collect::<HashMap<_, _>>()
            }
        };
        for wallet in &mut wallets {
            if let Some(snapshot) = mint_balances.get(&wallet.key) {
                wallet.mint_balance = snapshot.clone();
            }
        }
    }
    let mut pnl_drift_by_wallet: HashMap<String, bool> = HashMap::new();
    if let Some(mint) = requested_mint.as_deref() {
        for wallet in &wallets {
            let entry = trade_ledger.get(&trade_ledger_lookup_key(&wallet.key, mint));
            let on_chain_raw = if wallet.mint_balance.error.is_none() {
                wallet.mint_balance.raw
            } else {
                None
            };
            pnl_drift_by_wallet.insert(
                wallet.key.clone(),
                wallet_position_drifts_from_onchain(entry, on_chain_raw),
            );
        }
    }
    let selected_wallet_keys = target.wallet_keys.iter().cloned().collect::<HashSet<_>>();
    let selected_wallet = if target.wallet_count == 1 {
        wallets
            .iter()
            .find(|wallet| selected_wallet_keys.contains(&wallet.key))
    } else {
        None
    };
    let aggregate_balance_lamports = wallets
        .iter()
        .filter(|wallet| selected_wallet_keys.contains(&wallet.key))
        .fold(0u64, |sum, wallet| {
            sum.saturating_add(wallet.balance_lamports.unwrap_or(0))
        });
    let aggregate_balance_sol = wallets
        .iter()
        .filter(|wallet| selected_wallet_keys.contains(&wallet.key))
        .fold(0.0f64, |sum, wallet| {
            sum + wallet.balance_sol.unwrap_or(0.0)
        });
    let aggregate_usd1_balance = wallets
        .iter()
        .filter(|wallet| selected_wallet_keys.contains(&wallet.key))
        .fold(0.0f64, |sum, wallet| {
            sum + wallet.usd1_balance.unwrap_or(0.0)
        });
    let aggregate_mint_raw = wallets
        .iter()
        .filter(|wallet| selected_wallet_keys.contains(&wallet.key))
        .fold(0u64, |sum, wallet| {
            sum.saturating_add(wallet.mint_balance.raw.unwrap_or(0))
        });
    let aggregate_mint_ui = wallets
        .iter()
        .filter(|wallet| selected_wallet_keys.contains(&wallet.key))
        .fold(0.0f64, |sum, wallet| {
            sum + wallet.mint_balance.ui_amount.unwrap_or(0.0)
        });
    let aggregate_mint_decimals = wallets
        .iter()
        .filter(|wallet| selected_wallet_keys.contains(&wallet.key))
        .find_map(|wallet| wallet.mint_balance.decimals);
    let selected_mint_balances_known = requested_mint.is_none()
        || wallets
            .iter()
            .filter(|wallet| selected_wallet_keys.contains(&wallet.key))
            .all(|wallet| {
                wallet.mint_balance.error.is_none()
                    && wallet.mint_balance.raw.is_some()
                    && wallet.mint_balance.decimals.is_some()
            });
    let trade_summary = requested_mint
        .as_deref()
        .map(|mint| aggregate_trade_ledger(trade_ledger, &target.wallet_keys, mint))
        .unwrap_or_default();
    let tracked_bought_sol = trade_summary.tracked_bought_lamports as f64 / 1_000_000_000.0;
    let tracked_sold_sol = trade_summary.tracked_sold_lamports as f64 / 1_000_000_000.0;
    let explicit_fee_total_sol = trade_summary.explicit_fee_total_lamports as f64 / 1_000_000_000.0;
    let realized_pnl_gross_sol = trade_summary.realized_pnl_gross_lamports as f64 / 1_000_000_000.0;
    let realized_pnl_net_sol = trade_summary.realized_pnl_net_lamports as f64 / 1_000_000_000.0;
    let remaining_cost_basis_sol =
        trade_summary.remaining_cost_basis_lamports as f64 / 1_000_000_000.0;
    let mut holding_quote_error: Option<String> = None;
    let holding_quote = if let Some(mint) = requested_mint.as_deref() {
        if selected_mint_balances_known {
            match quote_active_token_value_sol(engine, request, mint, aggregate_mint_raw).await {
                Ok(quote) => Some(quote),
                Err(error) => {
                    if aggregate_mint_raw > 0 {
                        eprintln!(
                            "[execution-engine][wallet-status] active token quote failed mint={} raw={} err={}",
                            mint, aggregate_mint_raw, error
                        );
                        holding_quote_error = Some(error);
                    }
                    None
                }
            }
        } else {
            holding_quote_error =
                Some("Mint balance is unavailable for one or more selected wallets.".to_string());
            None
        }
    } else {
        None
    };
    let holding_value_sol = holding_quote
        .as_ref()
        .map(|quote| quote.value_lamports as f64 / 1_000_000_000.0);
    let token_balance_known = selected_mint_balances_known && aggregate_mint_decimals.is_some();
    let pnl_requires_quote = requested_mint.is_some()
        && (!token_balance_known || (aggregate_mint_raw > 0 && holding_value_sol.is_none()));
    let holding_metric = if requested_mint.is_some() {
        holding_value_sol
    } else {
        Some(aggregate_balance_sol)
    };
    let unrealized_pnl_gross_sol = if pnl_requires_quote {
        None
    } else {
        holding_value_sol.map(|value| value - remaining_cost_basis_sol)
    };
    let unrealized_pnl_net_sol = unrealized_pnl_gross_sol;
    let pnl_gross_value = if pnl_requires_quote {
        None
    } else {
        Some(
            unrealized_pnl_gross_sol
                .map(|value| realized_pnl_gross_sol + value)
                .unwrap_or(tracked_sold_sol - tracked_bought_sol),
        )
    };
    let pnl_net_value = pnl_gross_value.map(|value| value - explicit_fee_total_sol);
    let pnl_value = if engine.settings.pnl_include_fees {
        pnl_net_value
    } else {
        pnl_gross_value
    };
    let pnl_percent_gross = match pnl_gross_value {
        Some(value) if tracked_bought_sol > 0.0 => Some((value / tracked_bought_sol) * 100.0),
        _ => None,
    };
    let pnl_percent_net = match pnl_net_value {
        Some(value) if tracked_bought_sol > 0.0 => Some((value / tracked_bought_sol) * 100.0),
        _ => None,
    };
    let bootstrap = build_bootstrap_response(engine);
    let wallet_payloads = wallets
        .iter()
        .map(|wallet| {
            let mut payload = serde_json::Map::new();
            payload.insert("key".to_string(), Value::String(wallet.key.clone()));
            payload.insert("envKey".to_string(), Value::String(wallet.key.clone()));
            payload.insert("label".to_string(), Value::String(wallet.label.clone()));
            payload.insert(
                "customName".to_string(),
                Value::String(wallet.label.clone()),
            );
            payload.insert("enabled".to_string(), Value::Bool(wallet.enabled));
            payload.insert("publicKey".to_string(), json!(wallet.public_key));
            payload.insert("error".to_string(), json!(wallet.error));
            payload.insert(
                "balanceLamports".to_string(),
                json!(wallet.balance_lamports),
            );
            payload.insert("balanceSol".to_string(), json!(wallet.balance_sol));
            payload.insert("usd1Balance".to_string(), json!(wallet.usd1_balance));
            payload.insert("balanceError".to_string(), json!(wallet.balance_error));
            payload.insert("mint".to_string(), json!(requested_mint.clone()));
            payload.insert("mintBalanceRaw".to_string(), json!(wallet.mint_balance.raw));
            payload.insert(
                "mintBalance".to_string(),
                json!(wallet.mint_balance.ui_amount),
            );
            payload.insert(
                "mintBalanceUi".to_string(),
                json!(wallet.mint_balance.ui_amount),
            );
            payload.insert(
                "mintDecimals".to_string(),
                json!(wallet.mint_balance.decimals),
            );
            payload.insert(
                "mintBalanceError".to_string(),
                json!(wallet.mint_balance.error),
            );
            payload.insert(
                "tokenBalanceRaw".to_string(),
                json!(wallet.mint_balance.raw),
            );
            payload.insert(
                "tokenBalance".to_string(),
                json!(wallet.mint_balance.ui_amount),
            );
            payload.insert(
                "tokenDecimals".to_string(),
                json!(wallet.mint_balance.decimals),
            );
            payload.insert(
                "pnlDrift".to_string(),
                Value::Bool(
                    pnl_drift_by_wallet
                        .get(&wallet.key)
                        .copied()
                        .unwrap_or(false),
                ),
            );
            Value::Object(payload)
        })
        .collect::<Vec<_>>();
    let drifted_selected_wallet_keys: Vec<String> = target
        .wallet_keys
        .iter()
        .filter(|wallet_key| {
            pnl_drift_by_wallet
                .get(*wallet_key)
                .copied()
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    let pnl_drift_detected = !drifted_selected_wallet_keys.is_empty();
    let mut payload = serde_json::Map::new();
    payload.insert("ok".to_string(), Value::Bool(true));
    payload.insert("rpcUrl".to_string(), Value::String(configured_rpc_url()));
    payload.insert(
        "connected".to_string(),
        Value::Bool(!target.wallet_keys.is_empty()),
    );
    payload.insert("selectionMode".to_string(), json!(target.selection_mode));
    payload.insert("walletGroupId".to_string(), json!(target.wallet_group_id));
    payload.insert("walletKeys".to_string(), json!(target.wallet_keys));
    payload.insert(
        "selectedWalletKey".to_string(),
        json!(selected_wallet.as_ref().map(|wallet| wallet.key.clone())),
    );
    payload.insert(
        "wallet".to_string(),
        json!(
            selected_wallet
                .as_ref()
                .and_then(|wallet| wallet.public_key.clone())
        ),
    );
    payload.insert(
        "balanceLamports".to_string(),
        Value::Number(aggregate_balance_lamports.into()),
    );
    payload.insert("balanceSol".to_string(), json!(aggregate_balance_sol));
    payload.insert("usd1Balance".to_string(), json!(aggregate_usd1_balance));
    payload.insert("mint".to_string(), json!(requested_mint.clone()));
    payload.insert(
        "mintBalanceRaw".to_string(),
        json!(aggregate_mint_decimals.map(|_| aggregate_mint_raw)),
    );
    payload.insert(
        "mintBalance".to_string(),
        json!(aggregate_mint_decimals.map(|_| aggregate_mint_ui)),
    );
    payload.insert(
        "mintBalanceUi".to_string(),
        json!(aggregate_mint_decimals.map(|_| aggregate_mint_ui)),
    );
    payload.insert("mintDecimals".to_string(), json!(aggregate_mint_decimals));
    payload.insert(
        "tokenBalanceRaw".to_string(),
        json!(aggregate_mint_decimals.map(|_| aggregate_mint_raw)),
    );
    payload.insert(
        "tokenBalance".to_string(),
        json!(aggregate_mint_decimals.map(|_| aggregate_mint_ui)),
    );
    payload.insert("tokenDecimals".to_string(), json!(aggregate_mint_decimals));
    payload.insert("quotedPrice".to_string(), json!(None::<f64>));
    payload.insert(
        "holdingAmount".to_string(),
        json!(if aggregate_mint_decimals.is_some() {
            Some(aggregate_mint_ui)
        } else {
            None::<f64>
        }),
    );
    payload.insert("holdingValueSol".to_string(), json!(holding_value_sol));
    payload.insert("holding".to_string(), json!(holding_metric));
    payload.insert(
        "holdingQuoteSource".to_string(),
        json!(holding_quote.as_ref().map(|quote| quote.source.clone())),
    );
    payload.insert(
        "holdingQuoteAsset".to_string(),
        json!(
            holding_quote
                .as_ref()
                .map(|quote| quote.quote_asset.clone())
        ),
    );
    payload.insert(
        "holdingQuoteAgeMs".to_string(),
        json!(holding_quote.as_ref().map(|quote| quote.quote_age_ms)),
    );
    payload.insert("holdingQuoteError".to_string(), json!(holding_quote_error));
    payload.insert(
        "trackedBoughtLamports".to_string(),
        Value::Number(trade_summary.tracked_bought_lamports.into()),
    );
    payload.insert("trackedBoughtSol".to_string(), json!(tracked_bought_sol));
    payload.insert(
        "trackedSoldLamports".to_string(),
        Value::Number(trade_summary.tracked_sold_lamports.into()),
    );
    payload.insert("trackedSoldSol".to_string(), json!(tracked_sold_sol));
    payload.insert("totalBought".to_string(), json!(tracked_bought_sol));
    payload.insert("totalSold".to_string(), json!(tracked_sold_sol));
    payload.insert(
        "remainingCostBasisLamports".to_string(),
        Value::Number(trade_summary.remaining_cost_basis_lamports.into()),
    );
    payload.insert(
        "remainingCostBasisSol".to_string(),
        json!(remaining_cost_basis_sol),
    );
    payload.insert(
        "explicitFeeTotalLamports".to_string(),
        json!(trade_summary.explicit_fee_total_lamports),
    );
    payload.insert(
        "explicitFeeTotalSol".to_string(),
        json!(explicit_fee_total_sol),
    );
    payload.insert(
        "realizedPnlGrossSol".to_string(),
        json!(realized_pnl_gross_sol),
    );
    payload.insert("realizedPnlNetSol".to_string(), json!(realized_pnl_net_sol));
    payload.insert(
        "unrealizedPnlGrossSol".to_string(),
        json!(unrealized_pnl_gross_sol),
    );
    payload.insert(
        "unrealizedPnlNetSol".to_string(),
        json!(unrealized_pnl_net_sol),
    );
    payload.insert("pnlGross".to_string(), json!(pnl_gross_value));
    payload.insert("pnlNet".to_string(), json!(pnl_net_value));
    payload.insert("pnlPercentGross".to_string(), json!(pnl_percent_gross));
    payload.insert("pnlPercentNet".to_string(), json!(pnl_percent_net));
    payload.insert(
        "pnlRequiresQuote".to_string(),
        Value::Bool(pnl_requires_quote),
    );
    payload.insert(
        "includeFees".to_string(),
        Value::Bool(engine.settings.pnl_include_fees),
    );
    payload.insert(
        "trackingMode".to_string(),
        json!(engine.settings.pnl_tracking_mode),
    );
    payload.insert(
        "needsResync".to_string(),
        Value::Bool(trade_summary.needs_resync),
    );
    payload.insert(
        "unmatchedSellAmountRaw".to_string(),
        json!(trade_summary.unmatched_sell_amount_raw),
    );
    payload.insert(
        "pnlDriftDetected".to_string(),
        Value::Bool(pnl_drift_detected),
    );
    payload.insert(
        "pnlDriftWalletKeys".to_string(),
        json!(drifted_selected_wallet_keys),
    );
    payload.insert("pnl".to_string(), json!(pnl_value));
    payload.insert(
        "buyCount".to_string(),
        Value::Number(trade_summary.buy_count.into()),
    );
    payload.insert(
        "sellCount".to_string(),
        Value::Number(trade_summary.sell_count.into()),
    );
    payload.insert(
        "lastTradeAtUnixMs".to_string(),
        json!(if trade_summary.last_trade_at_unix_ms > 0 {
            Some(trade_summary.last_trade_at_unix_ms)
        } else {
            None::<u64>
        }),
    );
    payload.insert("surface".to_string(), json!(request.surface.clone()));
    payload.insert("pageUrl".to_string(), json!(request.page_url.clone()));
    payload.insert("source".to_string(), json!(request.source.clone()));
    payload.insert("wallets".to_string(), Value::Array(wallet_payloads));
    payload.insert("config".to_string(), current_canonical_config(engine));
    payload.insert(
        "regionRouting".to_string(),
        build_launchdeck_region_routing_payload(&build_settings_response(&engine.settings)),
    );
    payload.insert(
        "providers".to_string(),
        json!(provider_availability_registry()),
    );
    payload.insert("launchpads".to_string(), json!(launchpad_registry()));
    payload.insert(
        "bootstrapRevision".to_string(),
        Value::String(bootstrap_revision(&bootstrap, &state_path())),
    );
    Ok((Value::Object(payload), drifted_selected_wallet_keys))
}

fn build_effective_wallets(engine: &StoredEngineState) -> Vec<WalletSummary> {
    let snapshot = shared_config_manager().current_snapshot();
    let snapshot_by_key: HashMap<String, crate::shared_config::SharedWalletEntry> = snapshot
        .wallets
        .iter()
        .cloned()
        .map(|wallet| (wallet.key.clone(), wallet))
        .collect();
    let metadata: HashMap<String, WalletSummary> = engine
        .wallets
        .iter()
        .cloned()
        .map(|wallet| (wallet.key.clone(), wallet))
        .collect();

    let mut ordered: Vec<WalletSummary> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for meta in &engine.wallets {
        let Some(snapshot_wallet) = snapshot_by_key.get(&meta.key) else {
            continue;
        };
        seen.insert(meta.key.clone());
        ordered.push(WalletSummary {
            key: snapshot_wallet.key.clone(),
            label: if !meta.label.trim().is_empty() {
                meta.label.clone()
            } else if !snapshot_wallet.label.trim().is_empty() {
                snapshot_wallet.label.clone()
            } else {
                snapshot_wallet.key.clone()
            },
            public_key: snapshot_wallet.public_key.clone(),
            enabled: meta.enabled,
            emoji: meta.emoji.clone(),
        });
    }

    for snapshot_wallet in &snapshot.wallets {
        if seen.contains(&snapshot_wallet.key) {
            continue;
        }
        let meta = metadata.get(&snapshot_wallet.key);
        ordered.push(WalletSummary {
            key: snapshot_wallet.key.clone(),
            label: if snapshot_wallet.label.trim().is_empty() {
                meta.map(|entry| entry.label.clone())
                    .unwrap_or_else(|| snapshot_wallet.key.clone())
            } else {
                snapshot_wallet.label.clone()
            },
            public_key: snapshot_wallet.public_key.clone(),
            enabled: meta.map(|entry| entry.enabled).unwrap_or(true),
            emoji: meta.map(|entry| entry.emoji.clone()).unwrap_or_default(),
        });
    }

    ordered
}

fn runtime_wallets_from_wallet_summaries(wallets: &[WalletSummary]) -> Vec<RuntimeWalletSummary> {
    wallets
        .iter()
        .map(|wallet| {
            let label = wallet.label.trim();
            RuntimeWalletSummary {
                envKey: wallet.key.clone(),
                customName: if label.is_empty() || label == wallet.key {
                    None
                } else {
                    Some(label.to_string())
                },
                publicKey: Some(wallet.public_key.clone()),
                error: None,
            }
        })
        .collect()
}

fn runtime_wallets_from_engine(engine: &StoredEngineState) -> Vec<RuntimeWalletSummary> {
    runtime_wallets_from_wallet_summaries(&build_effective_wallets(engine))
}

fn build_effective_wallet_groups(engine: &StoredEngineState) -> Vec<WalletGroupSummary> {
    let known_wallets: HashSet<String> = build_effective_wallets(engine)
        .into_iter()
        .map(|wallet| wallet.key)
        .collect();
    let mut groups = engine.wallet_groups.clone();
    for group in &mut groups {
        group
            .wallet_keys
            .retain(|wallet_key| known_wallets.contains(wallet_key));
    }
    normalize_wallet_groups(&mut groups);
    groups
}

fn reconcile_wallet_metadata(mut engine: StoredEngineState) -> StoredEngineState {
    let effective_wallets = build_effective_wallets(&engine);
    let enabled_by_key: HashMap<String, bool> = engine
        .wallets
        .iter()
        .map(|wallet| (wallet.key.clone(), wallet.enabled))
        .collect();
    engine.wallets = effective_wallets
        .into_iter()
        .map(|mut wallet| {
            if let Some(enabled) = enabled_by_key.get(&wallet.key) {
                wallet.enabled = *enabled;
            }
            wallet
        })
        .collect();
    let known_wallets: HashSet<String> = engine
        .wallets
        .iter()
        .map(|wallet| wallet.key.clone())
        .collect();
    for group in &mut engine.wallet_groups {
        group
            .wallet_keys
            .retain(|wallet_key| known_wallets.contains(wallet_key));
    }
    normalize_wallet_groups(&mut engine.wallet_groups);
    engine
}

fn build_bootstrap_response(engine: &StoredEngineState) -> BootstrapResponse {
    let config = engine
        .config
        .clone()
        .map(normalize_canonical_config)
        .unwrap_or_else(default_canonical_config);
    BootstrapResponse {
        version: engine.version.clone(),
        data_root: engine.data_root.clone(),
        config_version: CANONICAL_CONFIG_VERSION.to_string(),
        schema_version: CANONICAL_CONFIG_SCHEMA_VERSION,
        config: config.clone(),
        providers: provider_availability_registry(),
        provider_registry: provider_registry(),
        launchpads: launchpad_registry(),
        strategies: strategy_registry(),
        capabilities: ExtensionCapabilities {
            platforms: vec![Platform::Axiom, Platform::J7],
            supports_batch_preview: true,
            supports_batch_status: true,
            supports_resource_editing: true,
        },
        settings: build_settings_response(&engine.settings),
        presets: engine.presets.clone(),
        wallets: build_effective_wallets(engine),
        wallet_groups: build_effective_wallet_groups(engine),
    }
}

fn fresh_engine_state() -> StoredEngineState {
    let mut engine = sample_engine_state();
    engine.config = Some(default_canonical_config());
    engine.presets = Vec::new();
    engine.wallets = Vec::new();
    engine.wallet_groups = Vec::new();
    engine
}

fn sample_engine_state() -> StoredEngineState {
    let settings = normalize_settings(EngineSettings {
        default_buy_slippage_percent: "20".to_string(),
        default_sell_slippage_percent: "20".to_string(),
        default_buy_mev_mode: MevMode::Off,
        default_sell_mev_mode: MevMode::Off,
        execution_provider: "standard-rpc".to_string(),
        execution_endpoint_profile: "global".to_string(),
        execution_commitment: "confirmed".to_string(),
        execution_skip_preflight: true,
        track_send_block_height: true,
        max_active_batches: 32,
        rpc_url: String::new(),
        ws_url: String::new(),
        warm_rpc_url: String::new(),
        warm_ws_url: String::new(),
        shared_region: String::new(),
        helius_rpc_url: String::new(),
        helius_ws_url: String::new(),
        standard_rpc_send_urls: Vec::new(),
        helius_sender_region: String::new(),
        default_distribution_mode: BuyDistributionMode::Each,
        allow_non_canonical_pool_trades: false,
        default_buy_funding_policy: default_buy_funding_policy_sol_only(),
        default_sell_settlement_policy: default_sell_settlement_policy_always_to_sol(),
        pnl_tracking_mode: default_pnl_tracking_mode_local(),
        pnl_include_fees: true,
        wrapper_default_fee_bps: crate::rollout::DEFAULT_WRAPPER_FEE_BPS,
    });
    let presets = vec![
        PresetSummary {
            id: "preset1".to_string(),
            label: "P1".to_string(),
            buy_amount_sol: "0.5".to_string(),
            sell_percent: "25".to_string(),
            buy_amounts_sol: vec![
                "0.5".to_string(),
                "1.0".to_string(),
                "2.0".to_string(),
                "3.0".to_string(),
            ],
            sell_amounts_percent: vec![
                "25".to_string(),
                "50".to_string(),
                "75".to_string(),
                "100".to_string(),
            ],
            buy_amount_rows: 1,
            sell_percent_rows: 1,
            buy_fee_sol: "0.0005".to_string(),
            buy_tip_sol: "0.0010".to_string(),
            buy_slippage_percent: "20".to_string(),
            buy_mev_mode: MevMode::Off,
            buy_auto_tip_enabled: false,
            buy_max_fee_sol: String::new(),
            buy_provider: String::new(),
            buy_endpoint_profile: String::new(),
            sell_fee_sol: "0.0005".to_string(),
            sell_tip_sol: "0.0010".to_string(),
            sell_slippage_percent: "20".to_string(),
            sell_mev_mode: MevMode::Off,
            sell_auto_tip_enabled: false,
            sell_max_fee_sol: String::new(),
            sell_provider: String::new(),
            sell_endpoint_profile: String::new(),
            slippage_percent: "20".to_string(),
            mev_mode: MevMode::Off,
            buy_funding_policy: default_buy_funding_policy_sol_only(),
            sell_settlement_policy: default_sell_settlement_policy_always_to_sol(),
            buy_funding_policy_explicit: false,
            sell_settlement_policy_explicit: false,
        },
        PresetSummary {
            id: "preset2".to_string(),
            label: "P2".to_string(),
            buy_amount_sol: "1.0".to_string(),
            sell_percent: "10".to_string(),
            buy_amounts_sol: vec![
                "1.0".to_string(),
                "2.0".to_string(),
                "3.0".to_string(),
                "5.0".to_string(),
            ],
            sell_amounts_percent: vec![
                "10".to_string(),
                "25".to_string(),
                "50".to_string(),
                "100".to_string(),
            ],
            buy_amount_rows: 1,
            sell_percent_rows: 1,
            buy_fee_sol: "0.0010".to_string(),
            buy_tip_sol: "0.0020".to_string(),
            buy_slippage_percent: "15".to_string(),
            buy_mev_mode: MevMode::Reduced,
            buy_auto_tip_enabled: true,
            buy_max_fee_sol: String::new(),
            buy_provider: String::new(),
            buy_endpoint_profile: String::new(),
            sell_fee_sol: "0.0010".to_string(),
            sell_tip_sol: "0.0020".to_string(),
            sell_slippage_percent: "12".to_string(),
            sell_mev_mode: MevMode::Secure,
            sell_auto_tip_enabled: false,
            sell_max_fee_sol: String::new(),
            sell_provider: String::new(),
            sell_endpoint_profile: String::new(),
            slippage_percent: "15".to_string(),
            mev_mode: MevMode::Reduced,
            buy_funding_policy: default_buy_funding_policy_sol_only(),
            sell_settlement_policy: default_sell_settlement_policy_always_to_sol(),
            buy_funding_policy_explicit: false,
            sell_settlement_policy_explicit: false,
        },
    ];
    StoredEngineState {
        version: CURRENT_ENGINE_STATE_VERSION.to_string(),
        data_root: DEFAULT_DATA_ROOT.to_string(),
        settings: settings.clone(),
        config: Some(canonical_config_from_legacy(&settings, &presets)),
        presets,
        wallets: vec![
            WalletSummary {
                key: "wallet-alpha".to_string(),
                label: "Wallet Alpha".to_string(),
                public_key: "Alpha11111111111111111111111111111111111".to_string(),
                enabled: true,
                emoji: String::new(),
            },
            WalletSummary {
                key: "wallet-bravo".to_string(),
                label: "Wallet Bravo".to_string(),
                public_key: "Bravo11111111111111111111111111111111111".to_string(),
                enabled: true,
                emoji: String::new(),
            },
            WalletSummary {
                key: "wallet-charlie".to_string(),
                label: "Wallet Charlie".to_string(),
                public_key: "Charlie111111111111111111111111111111111".to_string(),
                enabled: true,
                emoji: String::new(),
            },
        ],
        wallet_groups: vec![
            WalletGroupSummary {
                id: "group-core".to_string(),
                label: "Core Wallets".to_string(),
                wallet_keys: vec!["wallet-alpha".to_string(), "wallet-bravo".to_string()],
                batch_policy: WalletGroupBatchPolicy::default(),
                emoji: String::new(),
            },
            WalletGroupSummary {
                id: "group-all".to_string(),
                label: "All Wallets".to_string(),
                wallet_keys: vec![
                    "wallet-alpha".to_string(),
                    "wallet-bravo".to_string(),
                    "wallet-charlie".to_string(),
                ],
                batch_policy: WalletGroupBatchPolicy::default(),
                emoji: String::new(),
            },
        ],
    }
}

fn state_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(DEFAULT_DATA_ROOT)
        .join(DEFAULT_STATE_FILE)
}

fn load_engine_state(path: &PathBuf) -> Option<StoredEngineState> {
    let contents = fs::read_to_string(path).ok()?;
    let mut state = serde_json::from_str::<StoredEngineState>(&contents).ok()?;
    let needs_reset_persist = should_clear_legacy_presets(&state.version);
    state.settings = normalize_settings(state.settings);
    normalize_resource_state(&mut state).ok()?;
    if needs_reset_persist {
        let _ = persist_engine_state(path, &state);
    }
    Some(reconcile_wallet_metadata(state))
}

fn sync_canonical_preset(engine: &mut StoredEngineState, preset: &PresetSummary) {
    // Keep the canonical `config.presets.items` in lock-step with the legacy
    // `engine.presets` vec. Anything that resolves a trade (resolve_buy_policy,
    // resolve_sell_policy) reads from the canonical config, so a preset edit
    // that only updated the legacy vec would silently not take effect.
    let current_config = engine
        .config
        .clone()
        .unwrap_or_else(|| canonical_config_from_legacy(&engine.settings, &engine.presets));
    engine.config = Some(upsert_legacy_preset(
        &current_config,
        &engine.settings,
        preset,
    ));
}

fn remove_canonical_preset(engine: &mut StoredEngineState, preset_id: &str) {
    let current_config = engine
        .config
        .clone()
        .unwrap_or_else(|| canonical_config_from_legacy(&engine.settings, &engine.presets));
    engine.config = Some(remove_legacy_preset(&current_config, preset_id));
}

fn persist_engine_state(
    path: &PathBuf,
    engine: &StoredEngineState,
) -> Result<(), (StatusCode, String)> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(internal_error)?;
    }
    let contents = serde_json::to_string_pretty(engine).map_err(internal_error)?;
    fs::write(path, contents).map_err(internal_error)?;
    Ok(())
}

fn should_clear_legacy_presets(version: &str) -> bool {
    matches!(version.trim(), "" | "0.2.0")
}

fn normalize_resource_state(engine: &mut StoredEngineState) -> Result<(), String> {
    let should_reset_presets = should_clear_legacy_presets(&engine.version);
    if should_reset_presets {
        engine.presets.clear();
    }
    engine.version = if engine.version.trim().is_empty() || should_reset_presets {
        CURRENT_ENGINE_STATE_VERSION.to_string()
    } else {
        engine.version.trim().to_string()
    };
    engine.settings = normalize_settings(engine.settings.clone());
    engine.data_root = default_if_empty(&engine.data_root, DEFAULT_DATA_ROOT);
    engine.presets = engine
        .presets
        .clone()
        .into_iter()
        .map(normalize_preset)
        .collect::<Result<Vec<_>, _>>()?;
    engine.wallets = engine
        .wallets
        .clone()
        .into_iter()
        .map(normalize_wallet)
        .collect::<Result<Vec<_>, _>>()?;
    let wallets = {
        let snapshot = shared_config_manager().current_snapshot();
        let metadata: HashMap<String, WalletSummary> = engine
            .wallets
            .iter()
            .cloned()
            .map(|wallet| (wallet.key.clone(), wallet))
            .collect();
        snapshot
            .wallets
            .into_iter()
            .map(|wallet| WalletSummary {
                key: wallet.key.clone(),
                label: if wallet.label.trim().is_empty() {
                    metadata
                        .get(&wallet.key)
                        .map(|entry| entry.label.clone())
                        .unwrap_or_else(|| wallet.key.clone())
                } else {
                    wallet.label
                },
                public_key: wallet.public_key,
                enabled: metadata
                    .get(&wallet.key)
                    .map(|entry| entry.enabled)
                    .unwrap_or(true),
                emoji: metadata
                    .get(&wallet.key)
                    .map(|entry| entry.emoji.clone())
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>()
    };
    engine.wallet_groups = engine
        .wallet_groups
        .clone()
        .into_iter()
        .map(|group| normalize_wallet_group(group, &wallets))
        .collect::<Result<Vec<_>, _>>()?;
    normalize_wallet_groups(&mut engine.wallet_groups);
    let canonical_config = match engine.config.clone() {
        Some(config) => normalize_canonical_config(config),
        None => canonical_config_from_legacy(&engine.settings, &engine.presets),
    };
    engine.settings.track_send_block_height = config_track_send_block_height(&canonical_config);
    engine.settings.default_buy_funding_policy =
        config_default_buy_funding_policy(&canonical_config);
    engine.settings.default_sell_settlement_policy =
        config_default_sell_settlement_policy(&canonical_config);
    engine.settings.allow_non_canonical_pool_trades =
        config_allow_non_canonical_pool_trades(&canonical_config);
    engine.settings.wrapper_default_fee_bps = config_wrapper_default_fee_bps(&canonical_config);
    crate::rollout::set_allow_non_canonical_pool_trades(
        engine.settings.allow_non_canonical_pool_trades,
    );
    crate::rollout::set_wrapper_default_fee_bps(engine.settings.wrapper_default_fee_bps);
    let mut canonical_config = canonical_config;
    if engine.presets.is_empty() {
        if let Some(defaults) = canonical_config
            .get_mut("defaults")
            .and_then(Value::as_object_mut)
        {
            defaults.insert("activePresetId".to_string(), Value::String(String::new()));
        }
        if let Some(items) = canonical_config
            .get_mut("presets")
            .and_then(Value::as_object_mut)
            .and_then(|presets| presets.get_mut("items"))
            .and_then(Value::as_array_mut)
        {
            items.clear();
        }
    } else {
        // Reconcile canonical presets with the legacy vec. The legacy vec is
        // the source of truth for CUD operations, so every known legacy
        // preset must be mirrored into the canonical presets collection
        // (which is what trade resolution consults via
        // `find_matching_canonical_preset`).
        for preset in &engine.presets {
            canonical_config = upsert_legacy_preset(&canonical_config, &engine.settings, preset);
        }
    }
    engine.config = Some(canonical_config);
    Ok(())
}

fn normalize_settings(mut settings: EngineSettings) -> EngineSettings {
    settings.default_buy_slippage_percent =
        default_if_empty(&settings.default_buy_slippage_percent, "20");
    settings.default_sell_slippage_percent =
        default_if_empty(&settings.default_sell_slippage_percent, "20");
    settings.execution_provider =
        match default_if_empty(&settings.execution_provider, "standard-rpc")
            .to_lowercase()
            .as_str()
        {
            "helius" | "helius-sender" => "helius-sender".to_string(),
            _ => "standard-rpc".to_string(),
        };
    settings.execution_endpoint_profile = if settings.execution_provider == "helius-sender" {
        crate::endpoint_profile::parse_config_endpoint_profile(&default_if_empty(
            &settings.execution_endpoint_profile,
            "global",
        ))
        .unwrap_or_else(|_| "global".to_string())
    } else {
        String::new()
    };
    settings.execution_commitment =
        match default_if_empty(&settings.execution_commitment, "confirmed")
            .to_lowercase()
            .as_str()
        {
            "processed" => "processed".to_string(),
            "finalized" => "finalized".to_string(),
            _ => "confirmed".to_string(),
        };
    if settings.max_active_batches == 0 {
        settings.max_active_batches = 32;
    }
    settings.rpc_url = settings.rpc_url.trim().to_string();
    settings.ws_url = settings.ws_url.trim().to_string();
    settings.warm_rpc_url = settings.warm_rpc_url.trim().to_string();
    settings.warm_ws_url = settings.warm_ws_url.trim().to_string();
    settings.shared_region = settings.shared_region.trim().to_string();
    settings.helius_rpc_url = settings.helius_rpc_url.trim().to_string();
    settings.helius_ws_url = settings.helius_ws_url.trim().to_string();
    settings.helius_sender_region = settings.helius_sender_region.trim().to_string();
    settings.standard_rpc_send_urls = settings
        .standard_rpc_send_urls
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    // Clamp the persisted wrapper fee tier through the same ladder the
    // on-chain program enforces (0, 10, 20 bps) so a hand-edited
    // settings file cannot smuggle in an out-of-range tier. Also push
    // the normalized value into the runtime override so follow-on
    // trades immediately see the latest default.
    settings.wrapper_default_fee_bps =
        crate::rollout::normalize_wrapper_fee_bps(settings.wrapper_default_fee_bps);
    crate::rollout::set_wrapper_default_fee_bps(settings.wrapper_default_fee_bps);
    settings
}

fn normalize_preset(mut preset: PresetSummary) -> Result<PresetSummary, String> {
    preset.id = preset.id.trim().to_string();
    preset.label = preset.label.trim().to_string();
    if preset.id.is_empty() || preset.label.is_empty() {
        return Err("preset id and label are required".to_string());
    }
    preset.buy_amount_sol = default_if_empty(&preset.buy_amount_sol, "");
    preset.sell_percent = default_if_empty(&preset.sell_percent, "");
    preset.buy_amount_rows = infer_rows_from_shortcuts(
        clamp_buy_amount_rows(preset.buy_amount_rows),
        &preset.buy_amounts_sol,
        BUY_AMOUNTS_PER_ROW,
    );
    let buy_amounts_length = preset.buy_amount_rows as usize * BUY_AMOUNTS_PER_ROW;
    preset.buy_amounts_sol = normalize_shortcut_values(
        preset.buy_amounts_sol,
        Some(&preset.buy_amount_sol),
        buy_amounts_length,
    );
    // Auto-collapse: if row 2 was opened but every row-2 entry is empty,
    // shrink back to a single 4-entry row.
    if preset.buy_amount_rows == 2 {
        let row2_all_empty = preset.buy_amounts_sol[BUY_AMOUNTS_PER_ROW..]
            .iter()
            .all(|value| value.is_empty());
        if row2_all_empty {
            preset.buy_amount_rows = 1;
            preset.buy_amounts_sol.truncate(BUY_AMOUNTS_PER_ROW);
        }
    }
    preset.sell_percent_rows = infer_rows_from_shortcuts(
        clamp_sell_percent_rows(preset.sell_percent_rows),
        &preset.sell_amounts_percent,
        SELL_PERCENTS_PER_ROW,
    );
    let sell_percents_length = preset.sell_percent_rows as usize * SELL_PERCENTS_PER_ROW;
    preset.sell_amounts_percent = normalize_shortcut_values(
        preset.sell_amounts_percent,
        Some(&preset.sell_percent),
        sell_percents_length,
    );
    // Auto-collapse: same rule as buys — if row 2 was opened but every
    // row-2 entry is empty, shrink back to a single 4-entry row.
    if preset.sell_percent_rows == 2 {
        let row2_all_empty = preset.sell_amounts_percent[SELL_PERCENTS_PER_ROW..]
            .iter()
            .all(|value| value.is_empty());
        if row2_all_empty {
            preset.sell_percent_rows = 1;
            preset.sell_amounts_percent.truncate(SELL_PERCENTS_PER_ROW);
        }
    }
    preset.buy_provider = preset.buy_provider.trim().to_lowercase();
    preset.buy_endpoint_profile = preset.buy_endpoint_profile.trim().to_string();
    preset.buy_max_fee_sol = preset.buy_max_fee_sol.trim().to_string();
    preset.sell_provider = preset.sell_provider.trim().to_lowercase();
    preset.sell_endpoint_profile = preset.sell_endpoint_profile.trim().to_string();
    preset.sell_max_fee_sol = preset.sell_max_fee_sol.trim().to_string();
    preset.buy_slippage_percent = default_if_empty(
        &first_non_empty(&[
            Some(preset.buy_slippage_percent.as_str()),
            Some(preset.slippage_percent.as_str()),
        ])
        .unwrap_or(""),
        "",
    );
    preset.sell_slippage_percent = default_if_empty(
        &first_non_empty(&[
            Some(preset.sell_slippage_percent.as_str()),
            Some(preset.slippage_percent.as_str()),
        ])
        .unwrap_or(""),
        "",
    );
    preset.slippage_percent = default_if_empty(
        &first_non_empty(&[
            Some(preset.buy_slippage_percent.as_str()),
            Some(preset.sell_slippage_percent.as_str()),
            Some(preset.slippage_percent.as_str()),
        ])
        .unwrap_or(""),
        "",
    );
    Ok(preset)
}

fn normalize_wallet(mut wallet: WalletSummary) -> Result<WalletSummary, String> {
    wallet.key = wallet.key.trim().to_string();
    wallet.label = wallet.label.trim().to_string();
    wallet.public_key = wallet.public_key.trim().to_string();
    if wallet.key.is_empty() {
        return Err("wallet key is required".to_string());
    }
    if wallet.label.is_empty() {
        wallet.label = wallet.key.clone();
    }
    Ok(wallet)
}

fn normalize_wallet_group(
    mut group: WalletGroupSummary,
    wallets: &[WalletSummary],
) -> Result<WalletGroupSummary, String> {
    group.id = group.id.trim().to_string();
    group.label = group.label.trim().to_string();
    if group.id.is_empty() {
        return Err("wallet group id is required".to_string());
    }
    if group.label.is_empty() {
        group.label = group.id.clone();
    }
    let known_wallets: HashSet<String> = wallets.iter().map(|wallet| wallet.key.clone()).collect();
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for wallet_key in group.wallet_keys {
        let normalized = wallet_key.trim().to_string();
        if normalized.is_empty() {
            continue;
        }
        if !known_wallets.contains(&normalized) {
            return Err(format!(
                "wallet group references unknown wallet {normalized}"
            ));
        }
        if seen.insert(normalized.clone()) {
            deduped.push(normalized);
        }
    }
    group.wallet_keys = deduped;
    group.batch_policy = normalize_wallet_group_batch_policy(group.batch_policy);
    group.emoji = group.emoji.trim().to_string();
    Ok(group)
}

fn normalize_wallet_groups(groups: &mut [WalletGroupSummary]) {
    for group in groups {
        let mut deduped = Vec::new();
        let mut seen = HashSet::new();
        for wallet_key in group.wallet_keys.drain(..) {
            if seen.insert(wallet_key.clone()) {
                deduped.push(wallet_key);
            }
        }
        group.wallet_keys = deduped;
        group.batch_policy = normalize_wallet_group_batch_policy(group.batch_policy.clone());
        group.emoji = group.emoji.trim().to_string();
    }
}

fn normalize_wallet_group_batch_policy(
    mut policy: WalletGroupBatchPolicy,
) -> WalletGroupBatchPolicy {
    if policy.buy_variance_percent > 100 {
        policy.buy_variance_percent = 100;
    }
    match policy.transaction_delay_mode {
        TransactionDelayMode::Off => {
            policy.transaction_delay_ms = 0;
            policy.transaction_delay_min_ms = 0;
            policy.transaction_delay_max_ms = 0;
        }
        TransactionDelayMode::On | TransactionDelayMode::FirstBuyOnly => {
            policy.transaction_delay_ms = policy.transaction_delay_ms.min(MAX_TRANSACTION_DELAY_MS);
            policy.transaction_delay_min_ms = policy
                .transaction_delay_min_ms
                .min(MAX_TRANSACTION_DELAY_MS);
            policy.transaction_delay_max_ms = policy
                .transaction_delay_max_ms
                .min(MAX_TRANSACTION_DELAY_MS);
            if matches!(
                policy.transaction_delay_strategy,
                TransactionDelayStrategy::Random
            ) {
                if policy.transaction_delay_min_ms > policy.transaction_delay_max_ms {
                    std::mem::swap(
                        &mut policy.transaction_delay_min_ms,
                        &mut policy.transaction_delay_max_ms,
                    );
                }
            } else {
                policy.transaction_delay_min_ms = 0;
                policy.transaction_delay_max_ms = 0;
            }
        }
    }
    policy
}

fn normalize_shortcut_values(
    values: Vec<String>,
    fallback: Option<&str>,
    length: usize,
) -> Vec<String> {
    let mut normalized: Vec<String> = values
        .into_iter()
        .take(length)
        .map(|value| value.trim().to_string())
        .collect();
    while normalized.len() < length {
        normalized.push(String::new());
    }
    if !normalized.iter().any(|value| !value.is_empty()) {
        if let Some(fallback) = fallback.and_then(trimmed_option) {
            normalized[0] = fallback.to_string();
        }
    }
    normalized
}

fn trimmed_option(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn default_if_empty(value: &str, fallback: &str) -> String {
    trimmed_option(value).unwrap_or(fallback).to_string()
}

fn first_non_empty<'a>(values: &[Option<&'a str>]) -> Option<&'a str> {
    values
        .iter()
        .filter_map(|value| value.and_then(trimmed_option))
        .next()
}

fn internal_error(error: impl ToString) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn bootstrap_revision(bootstrap: &BootstrapResponse, path: &PathBuf) -> String {
    let modified_ms = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    let shared_env_modified_ms = shared_config_manager()
        .current_snapshot()
        .env_modified_unix_ms as u64;
    format!(
        "v{}-p{}-w{}-g{}-m{}-e{}",
        bootstrap.version,
        bootstrap.presets.len(),
        bootstrap.wallets.len(),
        bootstrap.wallet_groups.len(),
        modified_ms,
        shared_env_modified_ms
    )
}

fn short_symbol(mint: &str) -> String {
    let shortened: String = mint.chars().take(4).collect();
    shortened.to_uppercase()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod warm_fallback_tests {
    use super::*;

    fn wallet_status(
        key: &str,
        balance_lamports: Option<u64>,
        balance_error: Option<&str>,
    ) -> WalletStatusSummary {
        WalletStatusSummary {
            envKey: key.to_string(),
            customName: None,
            publicKey: Some(format!("{key}Pubkey")),
            error: None,
            balanceLamports: balance_lamports,
            balanceSol: balance_lamports.map(|value| value as f64 / 1_000_000_000.0),
            usd1Balance: None,
            balanceError: balance_error.map(str::to_string),
        }
    }

    #[test]
    fn merges_primary_balance_success_over_warm_balance_error() {
        let warm = vec![
            wallet_status("SOLANA_PRIVATE_KEY", None, Some("warm invalid json")),
            wallet_status("SOLANA_PRIVATE_KEY2", Some(42), None),
        ];
        let primary = vec![
            wallet_status("SOLANA_PRIVATE_KEY", Some(100), None),
            wallet_status("SOLANA_PRIVATE_KEY2", Some(99), None),
        ];

        let (merged, recovered) = merge_wallet_status_primary_fallback(warm, primary);

        assert!(recovered);
        assert_eq!(merged[0].balanceLamports, Some(100));
        assert_eq!(merged[0].balanceError, None);
        assert_eq!(merged[1].balanceLamports, Some(42));
    }

    #[test]
    fn diagnostic_fingerprint_uses_host_without_query_string() {
        let host = sanitized_endpoint_host("https://rpc.example.test/path?api-key=secret");
        assert_eq!(host.as_deref(), Some("rpc.example.test"));
        let fingerprint = diagnostic_fingerprint(
            "execution-engine",
            Some("warm-rpc"),
            Some("WARM_RPC_URL"),
            host.as_deref(),
            "warm_rpc_failed_primary_used",
        );
        assert!(fingerprint.contains("rpc.example.test"));
        assert!(!fingerprint.contains("secret"));
    }

    #[test]
    fn runtime_diagnostics_record_update_and_clear_by_sanitized_host() {
        clear_runtime_diagnostic(
            "execution-engine",
            Some("warm-rpc"),
            Some("WARM_RPC_URL"),
            Some("https://rpc.example.test/path?api-key=secret"),
            "warm_rpc_failed_primary_used",
        );

        record_runtime_diagnostic(RuntimeDiagnosticInput {
            severity: "warning",
            source: "execution-engine",
            code: "warm_rpc_failed_primary_used",
            message: "warm RPC failed".to_string(),
            detail: Some("first failure".to_string()),
            env_var: Some("WARM_RPC_URL"),
            endpoint_kind: Some("warm-rpc"),
            endpoint_url: Some("https://rpc.example.test/path?api-key=secret".to_string()),
            restart_required: false,
        });
        record_runtime_diagnostic(RuntimeDiagnosticInput {
            severity: "error",
            source: "execution-engine",
            code: "warm_rpc_failed_primary_used",
            message: "warm RPC still failing".to_string(),
            detail: Some("second failure".to_string()),
            env_var: Some("WARM_RPC_URL"),
            endpoint_kind: Some("warm-rpc"),
            endpoint_url: Some("https://rpc.example.test/other?api-key=secret".to_string()),
            restart_required: true,
        });

        let diagnostics = runtime_diagnostics_snapshot();
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "warm_rpc_failed_primary_used")
            .expect("recorded runtime diagnostic");
        assert_eq!(diagnostic.severity, "error");
        assert_eq!(diagnostic.message, "warm RPC still failing");
        assert_eq!(diagnostic.detail.as_deref(), Some("second failure"));
        assert_eq!(diagnostic.host.as_deref(), Some("rpc.example.test"));
        assert!(diagnostic.restart_required);
        assert!(diagnostic.last_seen_at_ms >= diagnostic.first_seen_at_ms);
        assert!(!diagnostic.fingerprint.contains("secret"));

        clear_runtime_diagnostic(
            "execution-engine",
            Some("warm-rpc"),
            Some("WARM_RPC_URL"),
            Some("https://rpc.example.test/path?api-key=secret"),
            "warm_rpc_failed_primary_used",
        );

        assert!(
            !runtime_diagnostics_snapshot()
                .iter()
                .any(|diagnostic| diagnostic.code == "warm_rpc_failed_primary_used")
        );
    }

    #[test]
    fn active_quote_primary_fallback_only_for_likely_endpoint_errors() {
        assert!(active_quote_error_should_try_primary(
            "Failed to fetch Bonk multiple accounts: request timed out"
        ));
        assert!(active_quote_error_should_try_primary(
            "RPC status 429 Too Many Requests"
        ));
        assert!(!active_quote_error_should_try_primary(
            "Raydium AMM v4 pool reserves are empty."
        ));
        assert!(!active_quote_error_should_try_primary(
            "Raydium AMM v4 pool ExamplePool was not found."
        ));
        assert!(!active_quote_error_should_try_primary(
            "Unsupported Bonk holding quote asset."
        ));
    }

    #[test]
    fn runtime_wallet_summaries_preserve_shared_config_wallet_labels() {
        let wallets = vec![WalletSummary {
            key: "SOLANA_PRIVATE_KEY6".to_string(),
            label: "New wallet".to_string(),
            public_key: "ExamplePublicKey".to_string(),
            enabled: true,
            emoji: String::new(),
        }];

        let runtime = runtime_wallets_from_wallet_summaries(&wallets);

        assert_eq!(runtime.len(), 1);
        assert_eq!(runtime[0].envKey, "SOLANA_PRIVATE_KEY6");
        assert_eq!(runtime[0].customName.as_deref(), Some("New wallet"));
        assert_eq!(runtime[0].publicKey.as_deref(), Some("ExamplePublicKey"));
        assert_eq!(runtime[0].error, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical_config::{
        config_default_buy_funding_policy, config_default_sell_settlement_policy,
        get_canonical_preset,
    };

    fn sample_batch_with_wallet_signature(
        batch_id: &str,
        wallet_key: &str,
        signature: Option<&str>,
    ) -> BatchStatusResponse {
        BatchStatusResponse {
            batch_id: batch_id.to_string(),
            client_request_id: format!("client-{batch_id}"),
            side: TradeSide::Buy,
            status: if signature.is_some() {
                BatchLifecycleStatus::Confirmed
            } else {
                BatchLifecycleStatus::Queued
            },
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
            execution_adapter: None,
            execution_backend: None,
            planned_execution: None,
            batch_policy: None,
            summary: BatchSummary {
                total_wallets: 1,
                queued_wallets: if signature.is_none() { 1 } else { 0 },
                submitted_wallets: 0,
                confirmed_wallets: if signature.is_some() { 1 } else { 0 },
                failed_wallets: 0,
            },
            wallets: vec![WalletExecutionState {
                wallet_key: wallet_key.to_string(),
                status: if signature.is_some() {
                    BatchLifecycleStatus::Confirmed
                } else {
                    BatchLifecycleStatus::Queued
                },
                tx_signature: signature.map(str::to_string),
                error: None,
                buy_amount_sol: None,
                scheduled_delay_ms: 0,
                delay_applied: false,
                first_buy: None,
                applied_variance_percent: None,
                entry_preference_asset: None,
            }],
        }
    }

    #[test]
    fn hellomoon_provider_detection_is_case_insensitive() {
        assert!(is_hellomoon_provider("hellomoon"));
        assert!(is_hellomoon_provider(" HelloMoon "));
        assert!(!is_hellomoon_provider("helius-sender"));
    }

    fn sell_balance_gate_request(intent: Option<RuntimeSellIntent>) -> TradeRuntimeRequest {
        TradeRuntimeRequest {
            side: TradeSide::Sell,
            mint: "Mint111".to_string(),
            buy_amount_sol: None,
            sell_intent: intent,
            policy: RuntimeExecutionPolicy {
                slippage_percent: "10".to_string(),
                mev_mode: MevMode::Off,
                auto_tip_enabled: false,
                fee_sol: "0.0001".to_string(),
                tip_sol: "0.0002".to_string(),
                provider: "standard-rpc".to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: true,
                track_send_block_height: false,
                buy_funding_policy: BuyFundingPolicy::SolOnly,
                sell_settlement_policy: SellSettlementPolicy::AlwaysToSol,
                sell_settlement_asset: TradeSettlementAsset::Sol,
            },
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: None,
            warm_key: None,
        }
    }

    #[test]
    fn buy_balance_gate_requirement_includes_costs_and_buffers() {
        let required = required_buy_balance_lamports_from_parts(
            50_000_000,
            1_005_000,
            TRADE_BALANCE_GATE_FIRST_BUY_RENT_BUFFER_LAMPORTS,
        );
        assert_eq!(
            required,
            50_000_000
                + 1_005_000
                + TRADE_BALANCE_GATE_FIRST_BUY_RENT_BUFFER_LAMPORTS
                + TRADE_BALANCE_GATE_FIXED_BUY_BUFFER_LAMPORTS
        );
    }

    #[test]
    fn priority_fee_lamports_rounds_up_micro_lamports() {
        assert_eq!(priority_fee_lamports(Some(1_400_000), Some(2)), Some(3));
        assert_eq!(priority_fee_lamports(None, Some(2)), None);
        assert_eq!(priority_fee_lamports(Some(1_400_000), None), None);
    }

    #[test]
    fn sell_balance_gate_percent_requires_available_tokens() {
        let full = sell_balance_gate_request(Some(RuntimeSellIntent::Percent("100".to_string())));
        assert_eq!(required_sell_token_balance_raw(&full, 1_000), Some(1_000));
        assert_eq!(required_sell_token_balance_raw(&full, 0), Some(1));

        let partial = sell_balance_gate_request(Some(RuntimeSellIntent::Percent("25".to_string())));
        assert_eq!(required_sell_token_balance_raw(&partial, 1_000), Some(250));

        let over = sell_balance_gate_request(Some(RuntimeSellIntent::Percent("125".to_string())));
        assert_eq!(required_sell_token_balance_raw(&over, 1_000), Some(1_250));
    }

    #[test]
    fn sell_balance_gate_sol_output_is_resolved_by_compiler() {
        let request =
            sell_balance_gate_request(Some(RuntimeSellIntent::SolOutput("0.1".to_string())));
        assert_eq!(required_sell_token_balance_raw(&request, 0), None);
        assert_eq!(required_sell_token_balance_raw(&request, 500), None);
    }

    #[test]
    fn sell_balance_gate_missing_or_zero_intent_is_inconclusive() {
        let missing = sell_balance_gate_request(None);
        assert_eq!(required_sell_token_balance_raw(&missing, 1_000), None);

        let zero = sell_balance_gate_request(Some(RuntimeSellIntent::Percent("0".to_string())));
        assert_eq!(required_sell_token_balance_raw(&zero, 1_000), None);
    }

    #[test]
    fn hellomoon_remaining_timeout_uses_shared_deadline() {
        let future_deadline = now_unix_ms().saturating_add(HELLOMOON_BATCH_WALLET_TIMEOUT_MS);
        let remaining =
            hellomoon_remaining_timeout(Some(future_deadline), "transaction").expect("remaining");
        assert!(remaining <= Duration::from_millis(HELLOMOON_BATCH_WALLET_TIMEOUT_MS));

        let error = hellomoon_remaining_timeout(Some(0), "transaction")
            .expect_err("elapsed deadline should fail");
        assert!(error.contains("Hello Moon transaction timed out after 10s"));
    }

    #[tokio::test]
    async fn active_token_quote_zero_balance_short_circuits() {
        let quote = quote_active_token_value_sol(
            &sample_engine_state(),
            &ExtensionWalletStatusRequest::default(),
            "Mint111",
            0,
        )
        .await
        .expect("zero balance quote");

        assert_eq!(quote.value_lamports, 0);
        assert_eq!(quote.source, "zero-balance");
        assert_eq!(quote.quote_asset, "SOL");
    }

    #[test]
    fn resolves_single_wallet_target() {
        let state = sample_engine_state();
        let target = resolve_batch_target(
            &state.wallets,
            &state.wallet_groups,
            Some("wallet-alpha".to_string()),
            None,
            None,
        )
        .expect("single wallet target");

        assert_eq!(target.wallet_count, 1);
        assert!(matches!(
            target.selection_mode,
            BatchSelectionMode::SingleWallet
        ));
    }

    #[test]
    fn rejects_multiple_selection_modes() {
        let state = sample_engine_state();
        let err = resolve_batch_target(
            &state.wallets,
            &state.wallet_groups,
            Some("wallet-alpha".to_string()),
            Some(vec!["wallet-bravo".to_string()]),
            None,
        )
        .expect_err("multiple selectors should fail");

        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn resolves_wallet_group_target() {
        let state = sample_engine_state();
        let target = resolve_batch_target(
            &state.wallets,
            &state.wallet_groups,
            None,
            None,
            Some("group-core".to_string()),
        )
        .expect("wallet group target");

        assert_eq!(target.wallet_count, 2);
        assert_eq!(target.wallet_group_id.as_deref(), Some("group-core"));
    }

    #[test]
    fn rejects_empty_wallet_group_target() {
        let mut state = sample_engine_state();
        state.wallet_groups.push(WalletGroupSummary {
            id: "group-empty".to_string(),
            label: "Empty".to_string(),
            wallet_keys: Vec::new(),
            batch_policy: WalletGroupBatchPolicy::default(),
            emoji: String::new(),
        });

        let error = resolve_batch_target(
            &state.wallets,
            &state.wallet_groups,
            None,
            None,
            Some("group-empty".to_string()),
        )
        .expect_err("empty group should fail");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(error.1.contains("has no enabled wallets"));
    }

    #[test]
    fn buy_planning_seed_is_shared_by_preview_and_execution() {
        let state = sample_engine_state();
        let target = resolve_batch_target(
            &state.wallets,
            &state.wallet_groups,
            None,
            None,
            Some("group-core".to_string()),
        )
        .expect("wallet group target");

        let preview_seed = build_buy_planning_seed("preset1", "Mint111", &target, Some("0.5"));
        let execution_seed = build_buy_planning_seed("preset1", "Mint111", &target, Some("0.5"));

        assert_eq!(preview_seed, execution_seed);
        assert!(preview_seed.starts_with("buy:"));
    }

    #[test]
    fn resolve_token_request_canonicalizes_surface() {
        let resolved = resolve_token_request(&ResolveTokenRequest {
            source: "page".to_string(),
            platform: Platform::Axiom,
            surface: Surface::Watchlist,
            url: Some("https://axiom.trade".to_string()),
            address: Some("Mint111111111111111111111111111111111111".to_string()),
            pair: None,
            mint: "Mint111111111111111111111111111111111111".to_string(),
        })
        .expect("token request");

        assert!(matches!(resolved.origin_surface, Surface::Watchlist));
        assert!(matches!(resolved.canonical_surface, Surface::TokenDetail));
    }

    #[test]
    fn resolve_token_request_prefers_explicit_address() {
        let resolved = resolve_token_request(&ResolveTokenRequest {
            source: "page".to_string(),
            platform: Platform::Axiom,
            surface: Surface::TokenDetail,
            url: Some("https://axiom.trade".to_string()),
            address: Some("  Pair111  ".to_string()),
            pair: Some("  Pool222  ".to_string()),
            mint: "Mint333".to_string(),
        })
        .expect("token request");

        assert_eq!(resolved.raw_address, "Pair111");
    }

    #[test]
    fn resolve_token_request_requires_address() {
        let error = resolve_token_request(&ResolveTokenRequest {
            source: "page".to_string(),
            platform: Platform::Axiom,
            surface: Surface::TokenDetail,
            url: Some("https://axiom.trade".to_string()),
            address: None,
            pair: Some("  Pool222  ".to_string()),
            mint: "   ".to_string(),
        })
        .expect_err("missing address should fail");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(error.1.contains("address is required"));
    }

    #[test]
    fn route_probe_request_carries_companion_pair() {
        let request = build_route_probe_request(
            "Mint111".to_string(),
            Some("axiom".to_string()),
            Some("Pool222".to_string()),
        );

        assert_eq!(request.mint, "Mint111");
        assert_eq!(request.platform_label.as_deref(), Some("axiom"));
        assert_eq!(request.pinned_pool.as_deref(), Some("Pool222"));
    }

    #[test]
    fn duplicate_signature_owner_detects_other_batch() {
        let mut batches = HashMap::new();
        batches.insert(
            "batch-a".to_string(),
            sample_batch_with_wallet_signature("batch-a", "wallet-alpha", Some("sig-1")),
        );
        batches.insert(
            "batch-b".to_string(),
            sample_batch_with_wallet_signature("batch-b", "wallet-alpha", None),
        );

        assert_eq!(
            duplicate_signature_owner(&batches, "batch-b", "wallet-alpha", "sig-1"),
            Some(("batch-a".to_string(), "wallet-alpha".to_string()))
        );
        assert_eq!(
            duplicate_signature_owner(&batches, "batch-a", "wallet-alpha", "sig-1"),
            None
        );
    }

    #[test]
    fn confirmed_trade_direction_rejects_wrong_or_zero_delta() {
        let zero_buy = ConfirmedTradeLedgerSnapshot {
            token_delta_raw: 0,
            ..ConfirmedTradeLedgerSnapshot::default()
        };
        assert!(
            validate_confirmed_trade_direction(
                &zero_buy,
                &TradeSide::Buy,
                "wallet-alpha",
                "Mint111",
                "sig-1"
            )
            .is_err()
        );

        let positive_sell = ConfirmedTradeLedgerSnapshot {
            token_delta_raw: 10,
            ..ConfirmedTradeLedgerSnapshot::default()
        };
        assert!(
            validate_confirmed_trade_direction(
                &positive_sell,
                &TradeSide::Sell,
                "wallet-alpha",
                "Mint111",
                "sig-2"
            )
            .is_err()
        );

        let positive_buy = ConfirmedTradeLedgerSnapshot {
            token_delta_raw: 10,
            ..ConfirmedTradeLedgerSnapshot::default()
        };
        assert!(
            validate_confirmed_trade_direction(
                &positive_buy,
                &TradeSide::Buy,
                "wallet-alpha",
                "Mint111",
                "sig-3"
            )
            .is_ok()
        );
    }

    #[test]
    fn resolve_token_route_error_filter_only_surfaces_hard_mismatches() {
        assert!(is_resolve_token_route_error(
            "[pair_mismatch] Selected pair Pool222 did not match the resolved market."
        ));
        assert!(is_resolve_token_route_error(
            "[non_canonical_blocked] Selected pair Pool222 is not the canonical market."
        ));
        assert!(!is_resolve_token_route_error(
            "[unsupported_address] No supported execution venue for address Foo."
        ));
    }

    #[test]
    fn route_descriptor_labels_render_route_identity() {
        let descriptor = crate::trade_dispatch::RouteDescriptor {
            raw_address: "Pool222".to_string(),
            resolved_input_kind: crate::trade_dispatch::TradeInputKind::Pair,
            resolved_mint: "Mint111".to_string(),
            resolved_pair: Some("Pool222".to_string()),
            route_locked_pair: Some("Pool222".to_string()),
            family: Some(crate::trade_planner::TradeVenueFamily::BonkRaydium),
            lifecycle: Some(crate::trade_planner::TradeLifecycle::PostMigration),
            quote_asset: Some(crate::trade_planner::PlannerQuoteAsset::Usd1),
            canonical_market_key: Some("pool-1".to_string()),
            non_canonical: false,
        };

        let (family, lifecycle, quote_asset, canonical_market_key) =
            route_descriptor_labels(&descriptor);

        assert_eq!(family.as_deref(), Some("bonk-raydium"));
        assert_eq!(lifecycle.as_deref(), Some("post_migration"));
        assert_eq!(quote_asset.as_deref(), Some("USD1"));
        assert_eq!(canonical_market_key.as_deref(), Some("pool-1"));
    }

    #[test]
    fn route_descriptor_pair_address_uses_canonical_market_for_pool_routes() {
        let descriptor = crate::trade_dispatch::RouteDescriptor {
            raw_address: "Mint111".to_string(),
            resolved_input_kind: crate::trade_dispatch::TradeInputKind::Mint,
            resolved_mint: "Mint111".to_string(),
            resolved_pair: None,
            route_locked_pair: None,
            family: Some(crate::trade_planner::TradeVenueFamily::BonkRaydium),
            lifecycle: Some(crate::trade_planner::TradeLifecycle::PostMigration),
            quote_asset: Some(crate::trade_planner::PlannerQuoteAsset::Usd1),
            canonical_market_key: Some("pool-1".to_string()),
            non_canonical: false,
        };

        assert_eq!(
            route_descriptor_pair_address(&descriptor).as_deref(),
            Some("pool-1")
        );
    }

    #[test]
    fn sample_state_exposes_canonical_config_schema_version() {
        let state = sample_engine_state();
        let config = state.config.expect("sample state config");
        assert_eq!(
            config
                .get("schemaVersion")
                .and_then(Value::as_u64)
                .expect("schemaVersion"),
            u64::from(CANONICAL_CONFIG_SCHEMA_VERSION),
        );
    }

    #[test]
    fn canonical_config_preserves_quick_trade_shortcuts() {
        let state = sample_engine_state();
        let config = state.config.expect("sample state config");
        let preset = get_canonical_preset(&config, "preset1").expect("preset1");
        let buy_amounts = preset
            .get("buyAmountsSol")
            .and_then(Value::as_array)
            .expect("buyAmountsSol");
        let sell_amounts = preset
            .get("sellAmountsPercent")
            .and_then(Value::as_array)
            .expect("sellAmountsPercent");
        assert_eq!(buy_amounts.first().and_then(Value::as_str), Some("0.5"));
        assert_eq!(sell_amounts.first().and_then(Value::as_str), Some("25"));
    }

    #[test]
    fn normalize_preset_round_trips_two_buy_rows() {
        let mut preset = sample_engine_state().presets.remove(0);
        preset.buy_amount_rows = 2;
        preset.buy_amounts_sol = vec![
            "0.1".to_string(),
            "0.2".to_string(),
            "0.3".to_string(),
            "0.4".to_string(),
            "0.5".to_string(),
            "0.6".to_string(),
            "0.7".to_string(),
            "0.8".to_string(),
        ];
        let normalized = normalize_preset(preset).expect("normalize");
        assert_eq!(normalized.buy_amount_rows, 2);
        assert_eq!(
            normalized.buy_amounts_sol,
            vec![
                "0.1".to_string(),
                "0.2".to_string(),
                "0.3".to_string(),
                "0.4".to_string(),
                "0.5".to_string(),
                "0.6".to_string(),
                "0.7".to_string(),
                "0.8".to_string(),
            ]
        );
    }

    #[test]
    fn normalize_preset_auto_collapses_empty_second_buy_row() {
        let mut preset = sample_engine_state().presets.remove(0);
        preset.buy_amount_rows = 2;
        preset.buy_amounts_sol = vec![
            "0.1".to_string(),
            "0.2".to_string(),
            "0.3".to_string(),
            "0.4".to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ];
        let normalized = normalize_preset(preset).expect("normalize");
        assert_eq!(normalized.buy_amount_rows, 1);
        assert_eq!(normalized.buy_amounts_sol.len(), 4);
    }

    #[test]
    fn normalize_preset_infers_second_buy_row_from_values() {
        let mut preset = sample_engine_state().presets.remove(0);
        preset.buy_amount_rows = 5;
        preset.buy_amounts_sol = vec![
            "0.1".to_string(),
            "0.2".to_string(),
            "0.3".to_string(),
            "0.4".to_string(),
            "0.5".to_string(),
            "0.6".to_string(),
            "0.7".to_string(),
            "0.8".to_string(),
        ];
        let normalized = normalize_preset(preset).expect("normalize");
        assert_eq!(normalized.buy_amount_rows, 2);
        assert_eq!(normalized.buy_amounts_sol.len(), 8);
        assert_eq!(
            normalized.buy_amounts_sol,
            vec![
                "0.1".to_string(),
                "0.2".to_string(),
                "0.3".to_string(),
                "0.4".to_string(),
                "0.5".to_string(),
                "0.6".to_string(),
                "0.7".to_string(),
                "0.8".to_string(),
            ]
        );
    }

    #[test]
    fn normalize_preset_round_trips_two_sell_rows() {
        let mut preset = sample_engine_state().presets.remove(0);
        preset.sell_percent_rows = 2;
        preset.sell_amounts_percent = vec![
            "10".to_string(),
            "20".to_string(),
            "30".to_string(),
            "40".to_string(),
            "55".to_string(),
            "65".to_string(),
            "75".to_string(),
            "100".to_string(),
        ];
        let normalized = normalize_preset(preset).expect("normalize");
        assert_eq!(normalized.sell_percent_rows, 2);
        assert_eq!(
            normalized.sell_amounts_percent,
            vec![
                "10".to_string(),
                "20".to_string(),
                "30".to_string(),
                "40".to_string(),
                "55".to_string(),
                "65".to_string(),
                "75".to_string(),
                "100".to_string(),
            ]
        );
    }

    #[test]
    fn normalize_preset_auto_collapses_empty_second_sell_row() {
        let mut preset = sample_engine_state().presets.remove(0);
        preset.sell_percent_rows = 2;
        preset.sell_amounts_percent = vec![
            "25".to_string(),
            "50".to_string(),
            "75".to_string(),
            "100".to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ];
        let normalized = normalize_preset(preset).expect("normalize");
        assert_eq!(normalized.sell_percent_rows, 1);
        assert_eq!(normalized.sell_amounts_percent.len(), 4);
    }

    #[test]
    fn normalize_preset_infers_second_sell_row_from_values() {
        let mut preset = sample_engine_state().presets.remove(0);
        preset.sell_percent_rows = 7;
        preset.sell_amounts_percent = vec![
            "10".to_string(),
            "20".to_string(),
            "30".to_string(),
            "40".to_string(),
            "55".to_string(),
            "65".to_string(),
            "75".to_string(),
            "100".to_string(),
        ];
        let normalized = normalize_preset(preset).expect("normalize");
        assert_eq!(normalized.sell_percent_rows, 2);
        assert_eq!(normalized.sell_amounts_percent.len(), 8);
        assert_eq!(
            normalized.sell_amounts_percent,
            vec![
                "10".to_string(),
                "20".to_string(),
                "30".to_string(),
                "40".to_string(),
                "55".to_string(),
                "65".to_string(),
                "75".to_string(),
                "100".to_string(),
            ]
        );
    }

    #[test]
    fn buy_policy_prefers_canonical_route_values() {
        let state = sample_engine_state();
        let preset = resolve_preset(&state.presets, "preset1").expect("preset");
        let policy = resolve_buy_policy(
            &state.settings,
            state.config.as_ref().expect("config"),
            preset,
            None,
            None,
        );
        assert_eq!(policy.provider, "standard-rpc");
        assert_eq!(policy.fee_sol, "0.0005");
        assert_eq!(policy.tip_sol, "");
    }

    #[test]
    fn buy_auto_fee_uses_user_priority_fallback_for_standard_rpc_without_tip() {
        let mut state = sample_engine_state();
        let mut config = state.config.take().expect("config");
        let buy_settings = config
            .get_mut("presets")
            .and_then(|value| value.get_mut("items"))
            .and_then(Value::as_array_mut)
            .and_then(|items| items.first_mut())
            .and_then(|preset| preset.get_mut("buySettings"))
            .expect("buy settings");
        buy_settings["provider"] = json!("standard-rpc");
        buy_settings["autoFee"] = json!(true);
        buy_settings["maxFeeSol"] = json!("0.001");
        buy_settings["priorityFeeSol"] = json!("0.009");
        buy_settings["tipSol"] = json!("0.009");

        let preset = resolve_preset(&state.presets, "preset1").expect("preset");
        let policy = resolve_buy_policy(&state.settings, &config, preset, None, None);

        assert!(policy.auto_tip_enabled);
        assert_eq!(policy.provider, "standard-rpc");
        assert_eq!(policy.fee_sol, "0.009");
        assert_eq!(policy.tip_sol, "");
        assert_eq!(policy.auto_fee_warnings.len(), 1);
        assert!(policy.auto_fee_warnings[0].contains("helius"));
        assert!(policy.auto_fee_warnings[0].contains("0.009 SOL"));
    }

    #[test]
    fn buy_auto_fee_falls_back_per_unavailable_source_for_tip_provider() {
        let mut state = sample_engine_state();
        let mut config = state.config.take().expect("config");
        let buy_settings = config
            .get_mut("presets")
            .and_then(|value| value.get_mut("items"))
            .and_then(Value::as_array_mut)
            .and_then(|items| items.first_mut())
            .and_then(|preset| preset.get_mut("buySettings"))
            .expect("buy settings");
        buy_settings["provider"] = json!("helius-sender");
        buy_settings["autoFee"] = json!(true);
        buy_settings["maxFeeSol"] = json!("0.001");
        buy_settings["priorityFeeSol"] = json!("0.009");
        buy_settings["tipSol"] = json!("0.009");

        let preset = resolve_preset(&state.presets, "preset1").expect("preset");
        let policy = resolve_buy_policy(&state.settings, &config, preset, None, None);

        assert!(policy.auto_tip_enabled);
        assert_eq!(policy.provider, "helius-sender");
        assert_eq!(policy.fee_sol, "0.009");
        assert_eq!(policy.tip_sol, "0.009");
        assert_eq!(policy.auto_fee_warnings.len(), 2);
        assert!(
            policy
                .auto_fee_warnings
                .iter()
                .any(|warning| warning.contains("Defaulted priority fee to 0.009 SOL"))
        );
        assert!(
            policy
                .auto_fee_warnings
                .iter()
                .any(|warning| warning.contains("Defaulted tip to 0.009 SOL"))
        );
    }

    #[test]
    fn sell_auto_fee_uses_user_priority_fallback_for_standard_rpc_without_tip() {
        let mut state = sample_engine_state();
        let mut config = state.config.take().expect("config");
        let sell_settings = config
            .get_mut("presets")
            .and_then(|value| value.get_mut("items"))
            .and_then(Value::as_array_mut)
            .and_then(|items| items.first_mut())
            .and_then(|preset| preset.get_mut("sellSettings"))
            .expect("sell settings");
        sell_settings["provider"] = json!("standard-rpc");
        sell_settings["autoFee"] = json!(true);
        sell_settings["maxFeeSol"] = json!("0.001");
        sell_settings["priorityFeeSol"] = json!("0.009");
        sell_settings["tipSol"] = json!("0.009");

        let preset = resolve_preset(&state.presets, "preset1").expect("preset");
        let policy = resolve_sell_policy(&state.settings, &config, preset, None);

        assert!(policy.auto_tip_enabled);
        assert_eq!(policy.provider, "standard-rpc");
        assert_eq!(policy.fee_sol, "0.009");
        assert_eq!(policy.tip_sol, "");
        assert_eq!(policy.auto_fee_warnings.len(), 1);
        assert!(policy.auto_fee_warnings[0].contains("helius"));
        assert!(policy.auto_fee_warnings[0].contains("0.009 SOL"));
    }

    #[test]
    fn auto_fee_error_warning_names_default_fallback_values() {
        assert_eq!(auto_fee_fallback_sol(""), "0.001");
        assert_eq!(auto_fee_fallback_sol("not-a-number"), "0.001");
        assert_eq!(auto_fee_fallback_sol("0.0040"), "0.004");

        let warning = auto_fee_unavailable_error_warning(
            "cap unavailable",
            &auto_fee_fallback_sol(""),
            Some(auto_fee_fallback_sol("").as_str()),
        );
        assert!(warning.contains("Defaulted priority fee to 0.001 SOL"));
        assert!(warning.contains("tip to 0.001 SOL"));
    }

    #[test]
    fn buy_policy_uses_settings_default_funding_when_preset_keeps_placeholder_default() {
        let mut state = sample_engine_state();
        state.settings.default_buy_funding_policy = BuyFundingPolicy::PreferUsd1ElseTopUp;
        let preset = resolve_preset(&state.presets, "preset1").expect("preset");
        let policy = resolve_buy_policy(
            &state.settings,
            state.config.as_ref().expect("config"),
            preset,
            None,
            None,
        );
        assert_eq!(
            policy.buy_funding_policy,
            BuyFundingPolicy::PreferUsd1ElseTopUp
        );
    }

    #[test]
    fn buy_policy_respects_explicit_default_looking_funding_policy() {
        let mut state = sample_engine_state();
        state.settings.default_buy_funding_policy = BuyFundingPolicy::PreferUsd1ElseTopUp;
        let preset = state
            .presets
            .iter_mut()
            .find(|preset| preset.id == "preset1")
            .expect("preset");
        preset.buy_funding_policy = BuyFundingPolicy::SolOnly;
        preset.buy_funding_policy_explicit = true;
        let preset_snapshot = preset.clone();
        sync_canonical_preset(&mut state, &preset_snapshot);

        let preset = resolve_preset(&state.presets, "preset1").expect("preset");
        let policy = resolve_buy_policy(
            &state.settings,
            state.config.as_ref().expect("config"),
            preset,
            None,
            None,
        );

        assert_eq!(policy.buy_funding_policy, BuyFundingPolicy::SolOnly);
        assert_eq!(
            state.config.as_ref().expect("config")["presets"]["items"][0]["buySettings"]["buyFundingPolicy"],
            Value::String("sol_only".to_string())
        );
    }

    #[test]
    fn buy_policy_prefers_route_buy_funding_override() {
        let state = sample_engine_state();
        let mut config = state.config.clone().expect("config");
        config["presets"]["items"][0]["buySettings"]["buyFundingPolicy"] =
            Value::String("usd1_only".to_string());
        let preset = resolve_preset(&state.presets, "preset1").expect("preset");
        let policy = resolve_buy_policy(&state.settings, &config, preset, None, None);
        assert_eq!(policy.buy_funding_policy, BuyFundingPolicy::Usd1Only);
    }

    #[test]
    fn sell_policy_prefers_route_settlement_override() {
        let state = sample_engine_state();
        let mut config = state.config.clone().expect("config");
        config["presets"]["items"][0]["sellSettings"]["sellSettlementPolicy"] =
            Value::String("always_to_usd1".to_string());
        let preset = resolve_preset(&state.presets, "preset1").expect("preset");
        let policy = resolve_sell_policy(&state.settings, &config, preset, None);
        assert_eq!(
            policy.sell_settlement_policy,
            SellSettlementPolicy::AlwaysToUsd1
        );
        assert_eq!(policy.sell_settlement_asset, TradeSettlementAsset::Usd1);
    }

    #[test]
    fn sell_policy_respects_explicit_default_looking_settlement_policy() {
        let mut state = sample_engine_state();
        state.settings.default_sell_settlement_policy = SellSettlementPolicy::AlwaysToUsd1;
        let preset = state
            .presets
            .iter_mut()
            .find(|preset| preset.id == "preset1")
            .expect("preset");
        preset.sell_settlement_policy = SellSettlementPolicy::AlwaysToSol;
        preset.sell_settlement_policy_explicit = true;
        let preset_snapshot = preset.clone();
        sync_canonical_preset(&mut state, &preset_snapshot);

        let preset = resolve_preset(&state.presets, "preset1").expect("preset");
        let policy = resolve_sell_policy(
            &state.settings,
            state.config.as_ref().expect("config"),
            preset,
            None,
        );

        assert_eq!(
            policy.sell_settlement_policy,
            SellSettlementPolicy::AlwaysToSol
        );
        assert_eq!(policy.sell_settlement_asset, TradeSettlementAsset::Sol);
        assert_eq!(
            state.config.as_ref().expect("config")["presets"]["items"][0]["sellSettings"]["sellSettlementPolicy"],
            Value::String("always_to_sol".to_string())
        );
    }

    #[test]
    fn submitted_signature_parser_accepts_non_confirmed_commitments() {
        assert_eq!(
            submitted_signature_from_confirmation_gap_error(
                "Transport submitted transaction sig-123, but finalized confirmation was not observed."
            ),
            Some("sig-123".to_string())
        );
    }

    #[test]
    fn resolve_wallet_request_matches_stored_entry_preference_without_rpc() {
        let wallet_request = WalletTradeRequest {
            side: TradeSide::Sell,
            mint: "Mint111".to_string(),
            platform_label: None,
            buy_amount_sol: None,
            sell_intent: Some(SellIntent::Percent("100".to_string())),
            policy: ExecutionPolicy {
                slippage_percent: "10".to_string(),
                mev_mode: MevMode::Off,
                auto_tip_enabled: false,
                fee_sol: "0.001".to_string(),
                tip_sol: "0.001".to_string(),
                provider: "standard-rpc".to_string(),
                endpoint_profile: "global".to_string(),
                commitment: "confirmed".to_string(),
                skip_preflight: false,
                track_send_block_height: false,
                buy_funding_policy: BuyFundingPolicy::SolOnly,
                sell_settlement_policy: SellSettlementPolicy::MatchStoredEntryPreference,
                sell_settlement_asset: TradeSettlementAsset::Sol,
            },
            planned_route: None,
            planned_trade: None,
            pinned_pool: None,
            warm_key: None,
        };
        let mut ledger = HashMap::new();
        ledger.insert(
            "wallet-alpha::Mint111".to_string(),
            crate::trade_ledger::TradeLedgerEntry {
                wallet_key: "wallet-alpha".to_string(),
                mint: "Mint111".to_string(),
                entry_preference: Some(crate::trade_ledger::StoredEntryPreference::Usd1),
                ..crate::trade_ledger::TradeLedgerEntry::default()
            },
        );

        let resolved =
            resolve_wallet_request_for_execution(&wallet_request, "wallet-alpha", &ledger);
        assert_eq!(
            resolved.policy.sell_settlement_asset,
            TradeSettlementAsset::Usd1
        );
    }

    #[test]
    fn live_mark_pnl_uses_ledger_baseline_and_mark_value() {
        let target = LiveMarkTarget {
            surface_id: "content:test".to_string(),
            mark_revision: 1,
            mint: "Mint111".to_string(),
            wallet_keys: vec!["wallet-alpha".to_string()],
            wallet_group_id: None,
            selector: LifecycleAndCanonicalMarket {
                lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
                family: TradeVenueFamily::PumpAmm,
                canonical_market_key: "Market111".to_string(),
                quote_asset: PlannerQuoteAsset::Sol,
                verification_source:
                    crate::trade_planner::PlannerVerificationSource::OnchainDerived,
                wrapper_action: crate::trade_planner::WrapperAction::PumpAmmWsolSell,
                wrapper_accounts: Vec::new(),
                market_subtype: None,
                direct_protocol_target: None,
                input_amount_hint: None,
                minimum_output_hint: None,
                runtime_bundle: None,
            },
            watch_accounts: vec!["Market111".to_string()],
            token_decimals: Some(6),
            token_balance_raw: Some(1_000_000),
            token_balance_ui_by_wallet: HashMap::new(),
        };
        let mut ledger = HashMap::new();
        ledger.insert(
            "wallet-alpha::Mint111".to_string(),
            crate::trade_ledger::TradeLedgerEntry {
                wallet_key: "wallet-alpha".to_string(),
                mint: "Mint111".to_string(),
                tracked_bought_lamports: 2_000_000_000,
                remaining_cost_basis_lamports: 1_500_000_000,
                realized_pnl_gross_lamports: 250_000_000,
                explicit_fee_total_lamports: 50_000_000,
                ..crate::trade_ledger::TradeLedgerEntry::default()
            },
        );

        let payload = live_mark_pnl_payload(&ledger, &target, 1_800_000_000, Some(77));

        assert_eq!(payload.holding_value_sol, Some(1.8));
        assert_eq!(payload.pnl_gross, Some(0.55));
        assert_eq!(payload.pnl_net, Some(0.5));
        assert_eq!(payload.pnl_percent_gross, Some(27.500000000000004));
        assert_eq!(payload.slot, Some(77));
    }

    #[test]
    fn confirmed_trade_notional_prefers_usd1_balance_deltas() {
        let buy_snapshot = ConfirmedTradeLedgerSnapshot {
            lamport_delta: -5_000,
            usd1_delta_raw: -2_500_000,
            token_delta_raw: 42,
            token_decimals: Some(6),
            slot: Some(1),
            block_time_unix_ms: Some(1_000),
            explicit_fees: ExplicitFeeBreakdown::default(),
        };
        assert_eq!(
            confirmed_buy_notional_source(&buy_snapshot),
            Some(ConfirmedTradeNotionalSource::Usd1Raw(2_500_000))
        );

        let sell_snapshot = ConfirmedTradeLedgerSnapshot {
            lamport_delta: -5_000,
            usd1_delta_raw: 1_250_000,
            token_delta_raw: -42,
            token_decimals: Some(6),
            slot: Some(2),
            block_time_unix_ms: Some(2_000),
            explicit_fees: ExplicitFeeBreakdown::default(),
        };
        assert_eq!(
            confirmed_sell_notional_source(&sell_snapshot),
            Some(ConfirmedTradeNotionalSource::Usd1Raw(1_250_000))
        );
    }

    #[test]
    fn explicit_fee_breakdown_splits_network_priority_and_tip() {
        let transaction = json!({
            "meta": {
                "fee": 5_050
            },
            "transaction": {
                "message": {
                    "instructions": [
                        {
                            "programId": COMPUTE_BUDGET_PROGRAM_ID,
                            "parsed": {
                                "type": "setComputeUnitLimit",
                                "info": {
                                    "units": 50_000
                                }
                            }
                        },
                        {
                            "programId": COMPUTE_BUDGET_PROGRAM_ID,
                            "parsed": {
                                "type": "setComputeUnitPrice",
                                "info": {
                                    "microLamports": "1"
                                }
                            }
                        },
                        {
                            "programId": SYSTEM_PROGRAM_ID,
                            "parsed": {
                                "type": "transfer",
                                "info": {
                                    "source": "wallet-alpha",
                                    "destination": crate::provider_tip::pick_tip_account_for_provider("jito-bundle"),
                                    "lamports": "200000"
                                }
                            }
                        }
                    ]
                }
            }
        });

        let fees = explicit_fee_breakdown_from_transaction(&transaction, "wallet-alpha", "Mint111");
        assert_eq!(fees.network_fee_lamports, 5_049);
        assert_eq!(fees.priority_fee_lamports, 1);
        assert_eq!(fees.tip_lamports, 200_000);
        assert_eq!(fees.rent_delta_lamports, 0);
    }

    #[test]
    fn explicit_fee_breakdown_recognizes_tips_in_inner_instructions() {
        let transaction = json!({
            "meta": {
                "fee": 5_000,
                "innerInstructions": [
                    {
                        "index": 0,
                        "instructions": [
                            {
                                "programId": SYSTEM_PROGRAM_ID,
                                "parsed": {
                                    "type": "transfer",
                                    "info": {
                                        "source": "wallet-alpha",
                                        "destination": crate::provider_tip::pick_tip_account_for_provider("jito-bundle"),
                                        "lamports": "350000"
                                    }
                                }
                            }
                        ]
                    }
                ]
            },
            "transaction": {
                "message": {
                    "instructions": []
                }
            }
        });

        let fees = explicit_fee_breakdown_from_transaction(&transaction, "wallet-alpha", "Mint111");
        assert_eq!(fees.network_fee_lamports, 5_000);
        assert_eq!(fees.priority_fee_lamports, 0);
        assert_eq!(fees.tip_lamports, 350_000);
        assert_eq!(fees.rent_delta_lamports, 0);
    }

    #[test]
    fn explicit_fee_breakdown_tracks_wallet_token_account_rent_spend() {
        let transaction = json!({
            "meta": {
                "fee": 5_000,
                "preBalances": [1_000_000_000, 0],
                "postBalances": [997_960_720, 2_039_280],
                "preTokenBalances": [],
                "postTokenBalances": [
                    {
                        "accountIndex": 1,
                        "owner": "wallet-alpha",
                        "mint": "Mint111",
                        "uiTokenAmount": {
                            "amount": "100",
                            "decimals": 6
                        }
                    }
                ]
            },
            "transaction": {
                "message": {
                    "instructions": []
                }
            }
        });

        let fees = explicit_fee_breakdown_from_transaction(&transaction, "wallet-alpha", "Mint111");
        assert_eq!(fees.network_fee_lamports, 5_000);
        assert_eq!(fees.rent_delta_lamports, 2_039_280);
    }

    #[test]
    fn explicit_fee_breakdown_tracks_wallet_token_account_rent_refund() {
        let transaction = json!({
            "meta": {
                "fee": 5_000,
                "preBalances": [1_000_000_000, 2_039_280],
                "postBalances": [1_002_034_280, 0],
                "preTokenBalances": [
                    {
                        "accountIndex": 1,
                        "owner": "wallet-alpha",
                        "mint": "Mint111",
                        "uiTokenAmount": {
                            "amount": "1",
                            "decimals": 6
                        }
                    }
                ],
                "postTokenBalances": []
            },
            "transaction": {
                "message": {
                    "instructions": []
                }
            }
        });

        let fees = explicit_fee_breakdown_from_transaction(&transaction, "wallet-alpha", "Mint111");
        assert_eq!(fees.network_fee_lamports, 5_000);
        assert_eq!(fees.rent_delta_lamports, -2_039_280);
    }

    #[test]
    fn token_balance_helper_sums_all_owned_accounts_for_a_mint() {
        let balances = vec![
            json!({
                "owner": "wallet-alpha",
                "mint": "Mint111",
                "uiTokenAmount": { "amount": "7" }
            }),
            json!({
                "owner": "wallet-alpha",
                "mint": "Mint111",
                "uiTokenAmount": { "amount": "11" }
            }),
            json!({
                "owner": "wallet-bravo",
                "mint": "Mint111",
                "uiTokenAmount": { "amount": "99" }
            }),
        ];

        assert_eq!(
            total_token_balance_amount_from_meta(&balances, "wallet-alpha", "Mint111")
                .expect("valid token metadata"),
            Some(18)
        );
    }

    #[test]
    fn trade_token_delta_errors_when_wallet_mint_metadata_is_absent() {
        let error = trade_token_delta_from_meta(&[], &[], "wallet-alpha", "Mint111")
            .expect_err("missing token metadata should not become zero delta");

        assert!(error.contains("wallet-alpha"));
        assert!(error.contains("Mint111"));
    }

    #[test]
    fn rpc_resync_token_delta_treats_absent_wallet_mint_metadata_as_irrelevant() {
        let unrelated = vec![json!({
            "owner": "wallet-alpha",
            "mint": "OtherMint",
            "uiTokenAmount": { "amount": "99", "decimals": 6 }
        })];

        assert!(
            maybe_trade_token_delta_from_meta(&[], &unrelated, "wallet-alpha", "Mint111")
                .expect("metadata parse")
                .is_none()
        );
    }

    #[test]
    fn trade_token_delta_handles_account_creation_and_closure() {
        let created_post = vec![json!({
            "owner": "wallet-alpha",
            "mint": "Mint111",
            "uiTokenAmount": {
                "amount": "42",
                "decimals": 6
            }
        })];
        let (created_delta, created_decimals) =
            trade_token_delta_from_meta(&[], &created_post, "wallet-alpha", "Mint111")
                .expect("created account delta");
        assert_eq!(created_delta, 42);
        assert_eq!(created_decimals, Some(6));

        let closed_pre = vec![json!({
            "owner": "wallet-alpha",
            "mint": "Mint111",
            "uiTokenAmount": {
                "amount": "42",
                "decimals": 6
            }
        })];
        let (closed_delta, closed_decimals) =
            trade_token_delta_from_meta(&closed_pre, &[], "wallet-alpha", "Mint111")
                .expect("closed account delta");
        assert_eq!(closed_delta, -42);
        assert_eq!(closed_decimals, Some(6));
    }

    fn rpc_resync_candidate(
        signature: &str,
        wallet_key: &str,
        token_delta_raw: i128,
        lamport_delta: i64,
        usd1_delta_raw: i128,
        at_ms: u64,
    ) -> RpcResyncCandidate {
        RpcResyncCandidate {
            signature: signature.to_string(),
            wallet_key: wallet_key.to_string(),
            wallet_public_key: format!("{wallet_key}-public"),
            snapshot: ConfirmedTradeLedgerSnapshot {
                lamport_delta,
                usd1_delta_raw,
                token_delta_raw,
                token_decimals: Some(6),
                slot: Some(at_ms / 100),
                block_time_unix_ms: Some(at_ms),
                explicit_fees: ExplicitFeeBreakdown::default(),
            },
        }
    }

    #[tokio::test]
    async fn rpc_resync_actions_replay_external_and_local_buys_once() {
        let local = crate::trade_ledger::ConfirmedTradeEvent {
            schema_version: crate::trade_ledger::trade_ledger_schema_version(),
            signature: "sig-trench".to_string(),
            slot: Some(20),
            confirmed_at_unix_ms: 2_000,
            wallet_key: "wallet-alpha".to_string(),
            wallet_public_key: "wallet-alpha-public".to_string(),
            mint: "Mint111".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 50,
            token_decimals: Some(6),
            trade_value_lamports: 500,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: Some("client-1".to_string()),
            batch_id: Some("batch-1".to_string()),
        };
        let local_events_by_id = HashMap::from([(local.event_id(), local)]);
        let actions = build_rpc_resync_ledger_actions(
            vec![
                rpc_resync_candidate("sig-external", "wallet-alpha", 25, -250, 0, 1_000),
                rpc_resync_candidate("sig-trench", "wallet-alpha", 50, -500, 0, 2_000),
            ],
            "Mint111",
            &HashSet::from(["wallet-alpha".to_string()]),
            &local_events_by_id,
        )
        .await
        .expect("resync actions");
        let trades = actions
            .iter()
            .filter_map(|action| match action {
                RpcResyncLedgerAction::Trade(event) => Some(event),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].signature, "sig-external");
        assert_eq!(trades[0].trade_value_lamports, 250);
        assert_eq!(trades[1].signature, "sig-trench");
        assert_eq!(trades[1].platform_tag, PlatformTag::Axiom);
        assert_eq!(trades[1].provenance, EventProvenance::LocalExecution);
        assert_eq!(trades[1].client_request_id.as_deref(), Some("client-1"));
    }

    #[test]
    fn missing_local_resync_events_are_preserved_when_rpc_misses_them() {
        let local = crate::trade_ledger::ConfirmedTradeEvent {
            schema_version: crate::trade_ledger::trade_ledger_schema_version(),
            signature: "sig-local-only".to_string(),
            slot: Some(20),
            confirmed_at_unix_ms: 2_000,
            wallet_key: "wallet-alpha".to_string(),
            wallet_public_key: "wallet-alpha-public".to_string(),
            mint: "Mint111".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 50,
            token_decimals: Some(6),
            trade_value_lamports: 500,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: Some("client-1".to_string()),
            batch_id: Some("batch-1".to_string()),
        };
        let mut actions = Vec::new();

        merge_missing_local_resync_events(&mut actions, &[local.clone()]);

        assert!(matches!(
            actions.as_slice(),
            [RpcResyncLedgerAction::Trade(event)]
                if event.event_id() == local.event_id()
        ));
    }

    #[test]
    fn missing_local_resync_events_are_deduped_before_live_replay() {
        let local = crate::trade_ledger::ConfirmedTradeEvent {
            schema_version: crate::trade_ledger::trade_ledger_schema_version(),
            signature: "sig-local-only".to_string(),
            slot: Some(20),
            confirmed_at_unix_ms: 2_000,
            wallet_key: "wallet-alpha".to_string(),
            wallet_public_key: "wallet-alpha-public".to_string(),
            mint: "Mint111".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 50,
            token_decimals: Some(6),
            trade_value_lamports: 500,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: Some("client-1".to_string()),
            batch_id: Some("batch-1".to_string()),
        };
        let mut actions = Vec::new();

        merge_missing_local_resync_events(&mut actions, &[local.clone(), local]);

        assert_eq!(
            actions
                .iter()
                .filter(|action| matches!(action, RpcResyncLedgerAction::Trade(_)))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn rpc_resync_actions_do_not_turn_token_only_incoming_into_zero_cost_buy() {
        let actions = build_rpc_resync_ledger_actions(
            vec![rpc_resync_candidate(
                "sig-received",
                "wallet-alpha",
                25,
                0,
                0,
                1_000,
            )],
            "Mint111",
            &HashSet::from(["wallet-alpha".to_string()]),
            &HashMap::new(),
        )
        .await
        .expect("resync actions");

        assert!(matches!(
            actions.as_slice(),
            [RpcResyncLedgerAction::ReceivedWithoutCostBasis {
                wallet_key,
                amount_raw: 25,
                ..
            }] if wallet_key == "wallet-alpha"
        ));
    }

    #[tokio::test]
    async fn rpc_resync_actions_do_not_turn_token_only_outgoing_into_sell() {
        let actions = build_rpc_resync_ledger_actions(
            vec![rpc_resync_candidate(
                "sig-sent",
                "wallet-alpha",
                -25,
                0,
                0,
                1_000,
            )],
            "Mint111",
            &HashSet::from(["wallet-alpha".to_string()]),
            &HashMap::new(),
        )
        .await
        .expect("resync actions");

        assert!(matches!(
            actions.as_slice(),
            [RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key,
                amount_raw: 25,
                ..
            }] if wallet_key == "wallet-alpha"
        ));
    }

    #[tokio::test]
    async fn rpc_resync_actions_pair_known_wallet_token_only_movements_as_transfer() {
        let actions = build_rpc_resync_ledger_actions(
            vec![
                rpc_resync_candidate("sig-transfer", "wallet-alpha", -40, 0, 0, 1_000),
                rpc_resync_candidate("sig-transfer", "wallet-bravo", 40, 0, 0, 1_000),
            ],
            "Mint111",
            &HashSet::from(["wallet-alpha".to_string(), "wallet-bravo".to_string()]),
            &HashMap::new(),
        )
        .await
        .expect("resync actions");

        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::KnownTransfer { marker, .. }
                if marker.source_wallet_key == "wallet-alpha"
                    && marker.destination_wallet_key == "wallet-bravo"
                    && marker.amount_raw == 40
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::SentWithoutProceeds { wallet_key, amount_raw: 0, .. }
                if wallet_key == "wallet-alpha"
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::ReceivedWithoutCostBasis { wallet_key, amount_raw: 0, .. }
                if wallet_key == "wallet-bravo"
        )));
    }

    #[tokio::test]
    async fn rpc_resync_actions_do_not_mutate_unselected_known_transfer_counterparty() {
        let actions = build_rpc_resync_ledger_actions(
            vec![
                rpc_resync_candidate("sig-transfer", "wallet-alpha", -40, 0, 0, 1_000),
                rpc_resync_candidate("sig-transfer", "wallet-bravo", 40, 0, 0, 1_000),
            ],
            "Mint111",
            &HashSet::from(["wallet-alpha".to_string()]),
            &HashMap::new(),
        )
        .await
        .expect("resync actions");

        assert!(matches!(
            actions.as_slice(),
            [RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key,
                amount_raw: 40,
                persist: true,
                ..
            }] if wallet_key == "wallet-alpha"
        ));
    }

    #[tokio::test]
    async fn rpc_resync_actions_do_not_guess_ambiguous_multi_wallet_transfers() {
        let actions = build_rpc_resync_ledger_actions(
            vec![
                rpc_resync_candidate("sig-ambiguous", "wallet-alpha", -40, 0, 0, 1_000),
                rpc_resync_candidate("sig-ambiguous", "wallet-bravo", -30, 0, 0, 1_000),
                rpc_resync_candidate("sig-ambiguous", "wallet-charlie", 40, 0, 0, 1_000),
                rpc_resync_candidate("sig-ambiguous", "wallet-delta", 30, 0, 0, 1_000),
            ],
            "Mint111",
            &HashSet::from([
                "wallet-alpha".to_string(),
                "wallet-bravo".to_string(),
                "wallet-charlie".to_string(),
                "wallet-delta".to_string(),
            ]),
            &HashMap::new(),
        )
        .await
        .expect("resync actions");

        assert!(
            actions
                .iter()
                .all(|action| !matches!(action, RpcResyncLedgerAction::KnownTransfer { .. }))
        );
        assert_eq!(
            actions
                .iter()
                .filter(|action| matches!(
                    action,
                    RpcResyncLedgerAction::SentWithoutProceeds { .. }
                ))
                .count(),
            2
        );
        assert_eq!(
            actions
                .iter()
                .filter(|action| matches!(
                    action,
                    RpcResyncLedgerAction::ReceivedWithoutCostBasis { .. }
                ))
                .count(),
            2
        );
    }

    #[test]
    fn scoped_local_markers_preserve_transfer_without_stale_partial_duplicate() {
        let transfer = crate::trade_ledger::TokenTransferMarkerEvent::new(
            "wallet-alpha",
            "wallet-bravo",
            "Mint111",
            40,
            "sig-transfer",
            2_000,
        )
        .with_slot(Some(20));
        let stale_partial =
            crate::trade_ledger::IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
                "wallet-alpha",
                "Mint111",
                40,
                "sig-transfer:incomplete:sent_to:wallet-alpha:wallet-bravo",
                1_000,
                Some(10),
            );
        let mut actions = Vec::new();

        merge_scoped_local_resync_markers(
            &mut actions,
            &[
                JournalEntry::IncompleteBalanceAdjustmentMarker(stale_partial),
                JournalEntry::TokenTransferMarker(transfer),
            ],
            &HashSet::from(["wallet-alpha".to_string(), "wallet-bravo".to_string()]),
            "Mint111",
            &HashMap::new(),
        );

        assert_eq!(
            actions
                .iter()
                .filter(|action| matches!(action, RpcResyncLedgerAction::KnownTransfer { .. }))
                .count(),
            1
        );
        assert!(actions.iter().all(|action| !matches!(
            action,
            RpcResyncLedgerAction::SentWithoutProceeds { amount_raw: 40, .. }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::SentWithoutProceeds {
                amount_raw: 0,
                persist: true,
                ..
            }
        )));
    }

    #[test]
    fn scoped_local_markers_do_not_mutate_unselected_transfer_counterparty() {
        let transfer = crate::trade_ledger::TokenTransferMarkerEvent::new(
            "wallet-alpha",
            "wallet-bravo",
            "Mint111",
            40,
            "sig-transfer",
            2_000,
        )
        .with_slot(Some(20));
        let mut actions = Vec::new();

        merge_scoped_local_resync_markers(
            &mut actions,
            &[JournalEntry::TokenTransferMarker(transfer)],
            &HashSet::from(["wallet-alpha".to_string()]),
            "Mint111",
            &HashMap::new(),
        );

        assert!(matches!(
            actions.as_slice(),
            [RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key,
                amount_raw: 40,
                persist: false,
                ..
            }] if wallet_key == "wallet-alpha"
        ));
    }

    #[test]
    fn scoped_local_markers_preserve_force_close_after_reset_baseline() {
        let mut actions = Vec::new();
        let marker = ForceCloseMarkerEvent::new(
            "wallet-alpha",
            "Mint111",
            2_000,
            "on-chain-zero-after-resync",
        );

        merge_scoped_local_resync_markers(
            &mut actions,
            &[JournalEntry::ForceCloseMarker(marker)],
            &HashSet::from(["wallet-alpha".to_string()]),
            "Mint111",
            &HashMap::from([("wallet-alpha".to_string(), (1_000, None))]),
        );

        assert!(matches!(
            actions.as_slice(),
            [RpcResyncLedgerAction::ForceClose { marker, persist: false }]
                if marker.wallet_key == "wallet-alpha"
        ));
    }

    #[test]
    fn malformed_matching_token_metadata_is_a_payload_error() {
        let malformed = vec![json!({
            "owner": "wallet-alpha",
            "mint": "Mint111",
            "uiTokenAmount": { "decimals": 6 }
        })];

        let error = maybe_trade_token_delta_from_meta(&[], &malformed, "wallet-alpha", "Mint111")
            .expect_err("matching metadata without amount should fail");
        assert!(error.contains("invalid amount"));
    }

    #[test]
    fn rpc_resync_action_sorting_matches_journal_timestamp_order() {
        let mut actions = vec![
            RpcResyncLedgerAction::Trade(crate::trade_ledger::ConfirmedTradeEvent {
                schema_version: crate::trade_ledger::trade_ledger_schema_version(),
                signature: "sig-sell".to_string(),
                slot: Some(20),
                confirmed_at_unix_ms: 1_000,
                wallet_key: "wallet-alpha".to_string(),
                wallet_public_key: "wallet-alpha-public".to_string(),
                mint: "Mint111".to_string(),
                side: TradeSide::Sell,
                platform_tag: PlatformTag::Unknown,
                provenance: EventProvenance::RpcResync,
                settlement_asset: Some(TradeSettlementAsset::Sol),
                token_delta_raw: -10,
                token_decimals: Some(6),
                trade_value_lamports: 100,
                explicit_fees: ExplicitFeeBreakdown::default(),
                client_request_id: None,
                batch_id: None,
            }),
            RpcResyncLedgerAction::Trade(crate::trade_ledger::ConfirmedTradeEvent {
                schema_version: crate::trade_ledger::trade_ledger_schema_version(),
                signature: "sig-buy".to_string(),
                slot: Some(10),
                confirmed_at_unix_ms: 2_000,
                wallet_key: "wallet-alpha".to_string(),
                wallet_public_key: "wallet-alpha-public".to_string(),
                mint: "Mint111".to_string(),
                side: TradeSide::Buy,
                platform_tag: PlatformTag::Unknown,
                provenance: EventProvenance::RpcResync,
                settlement_asset: Some(TradeSettlementAsset::Sol),
                token_delta_raw: 10,
                token_decimals: Some(6),
                trade_value_lamports: 100,
                explicit_fees: ExplicitFeeBreakdown::default(),
                client_request_id: None,
                batch_id: None,
            }),
        ];

        sort_resync_actions(&mut actions);

        assert_eq!(actions[0].signature(), "sig-sell");
    }

    #[test]
    fn helius_parser_uses_user_account_token_deltas_and_wallet_native_delta() {
        let transaction = json!({
            "signature": "sig-helius",
            "slot": 77,
            "timestamp": 123,
            "fee": 5000,
            "feePayer": "other-wallet",
            "transactionError": null,
            "nativeTransfers": [
                {
                    "fromUserAccount": "wallet-public",
                    "toUserAccount": "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
                    "amount": 7
                },
                {
                    "fromUserAccount": "other-wallet",
                    "toUserAccount": "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
                    "amount": 999
                }
            ],
            "accountData": [
                {
                    "account": "wallet-public",
                    "nativeBalanceChange": -2000,
                    "tokenBalanceChanges": [
                        {
                            "userAccount": "wallet-public",
                            "tokenAccount": "token-account",
                            "mint": "Mint111",
                            "rawTokenAmount": { "tokenAmount": "100", "decimals": 6 }
                        },
                        {
                            "userAccount": "wallet-public",
                            "tokenAccount": "usd1-account",
                            "mint": USD1_MINT,
                            "rawTokenAmount": { "tokenAmount": "-50", "decimals": 6 }
                        },
                        {
                            "userAccount": "other-wallet",
                            "tokenAccount": "other-token-account",
                            "mint": "Mint111",
                            "rawTokenAmount": { "tokenAmount": "999", "decimals": 6 }
                        }
                    ]
                },
                {
                    "account": "other-wallet",
                    "nativeBalanceChange": -999999,
                    "tokenBalanceChanges": []
                }
            ]
        });

        let candidate =
            helius_candidate_from_transaction(&transaction, "wallet-6", "wallet-public", "Mint111")
                .expect("parse helius transaction")
                .expect("candidate");

        assert_eq!(candidate.signature, "sig-helius");
        assert_eq!(candidate.snapshot.token_delta_raw, 100);
        assert_eq!(candidate.snapshot.token_decimals, Some(6));
        assert_eq!(candidate.snapshot.usd1_delta_raw, -50);
        assert_eq!(candidate.snapshot.lamport_delta, -2000);
        assert_eq!(candidate.snapshot.slot, Some(77));
        assert_eq!(candidate.snapshot.block_time_unix_ms, Some(123_000));
        assert_eq!(candidate.snapshot.explicit_fees.network_fee_lamports, 0);
        assert_eq!(candidate.snapshot.explicit_fees.tip_lamports, 7);
    }

    #[test]
    fn helius_parser_rejects_malformed_matching_raw_token_amount() {
        let transaction = json!({
            "signature": "sig-helius-bad",
            "slot": 77,
            "timestamp": 123,
            "transactionError": null,
            "accountData": [
                {
                    "account": "wallet-public",
                    "nativeBalanceChange": 0,
                    "tokenBalanceChanges": [
                        {
                            "userAccount": "wallet-public",
                            "tokenAccount": "token-account",
                            "mint": "Mint111",
                            "rawTokenAmount": { "tokenAmount": "not-a-number", "decimals": 6 }
                        }
                    ]
                }
            ]
        });

        let error =
            helius_candidate_from_transaction(&transaction, "wallet-6", "wallet-public", "Mint111")
                .expect_err("matching malformed raw amount should fail");

        assert!(error.contains("invalid raw amount"));
    }

    #[test]
    fn helius_rent_delta_tracks_wallet_owned_token_account_lamports() {
        let account_data = vec![
            json!({
                "account": "wallet-public",
                "nativeBalanceChange": -999,
                "tokenBalanceChanges": []
            }),
            json!({
                "account": "token-account",
                "nativeBalanceChange": -2_039_280,
                "tokenBalanceChanges": [{
                    "userAccount": "wallet-public",
                    "tokenAccount": "token-account",
                    "mint": "Mint111",
                    "rawTokenAmount": { "tokenAmount": "100", "decimals": 6 }
                }]
            }),
            json!({
                "account": "other-token-account",
                "nativeBalanceChange": -2_039_280,
                "tokenBalanceChanges": [{
                    "userAccount": "other-wallet",
                    "tokenAccount": "other-token-account",
                    "mint": "Mint111",
                    "rawTokenAmount": { "tokenAmount": "100", "decimals": 6 }
                }]
            }),
        ];

        assert_eq!(
            helius_wallet_owned_token_account_rent_delta_lamports(
                &account_data,
                "wallet-public",
                "Mint111"
            ),
            -2_039_280
        );
    }

    #[test]
    fn helius_rent_delta_includes_wallet_owned_usd1_token_account_lamports() {
        let account_data = vec![json!({
            "account": "usd1-token-account",
            "nativeBalanceChange": -2_039_280,
            "tokenBalanceChanges": [{
                "userAccount": "wallet-public",
                "tokenAccount": "usd1-token-account",
                "mint": USD1_MINT,
                "rawTokenAmount": { "tokenAmount": "-100", "decimals": 6 }
            }]
        })];

        assert_eq!(
            helius_wallet_owned_token_account_rent_delta_lamports(
                &account_data,
                "wallet-public",
                "Mint111"
            ),
            -2_039_280
        );
    }

    #[test]
    fn balance_reconciliation_marks_higher_and_lower_onchain_differences() {
        let mut ledger = HashMap::new();
        record_confirmed_trade(
            &mut ledger,
            RecordConfirmedTradeParams {
                wallet_key: "wallet-high",
                wallet_public_key: "wallet-high-public",
                mint: "Mint111",
                side: TradeSide::Buy,
                trade_value_lamports: 100,
                token_delta_raw: 40,
                token_decimals: Some(6),
                confirmed_at_unix_ms: 100,
                slot: Some(10),
                entry_preference_asset: Some(TradeSettlementAsset::Sol),
                settlement_asset: Some(TradeSettlementAsset::Sol),
                explicit_fees: ExplicitFeeBreakdown::default(),
                platform_tag: PlatformTag::Unknown,
                provenance: EventProvenance::RpcResync,
                signature: "sig-high-buy",
                client_request_id: None,
                batch_id: None,
            },
        );
        record_confirmed_trade(
            &mut ledger,
            RecordConfirmedTradeParams {
                wallet_key: "wallet-low",
                wallet_public_key: "wallet-low-public",
                mint: "Mint111",
                side: TradeSide::Buy,
                trade_value_lamports: 100,
                token_delta_raw: 40,
                token_decimals: Some(6),
                confirmed_at_unix_ms: 100,
                slot: Some(10),
                entry_preference_asset: Some(TradeSettlementAsset::Sol),
                settlement_asset: Some(TradeSettlementAsset::Sol),
                explicit_fees: ExplicitFeeBreakdown::default(),
                platform_tag: PlatformTag::Unknown,
                provenance: EventProvenance::RpcResync,
                signature: "sig-low-buy",
                client_request_id: None,
                batch_id: None,
            },
        );
        let wallet_keys = vec![
            "wallet-high".to_string(),
            "wallet-low".to_string(),
            "wallet-empty".to_string(),
        ];
        let onchain = HashMap::from([
            ("wallet-high".to_string(), 55),
            ("wallet-low".to_string(), 10),
            ("wallet-empty".to_string(), 7),
        ]);

        let replayed = HashMap::from([
            ("wallet-high".to_string(), 40),
            ("wallet-low".to_string(), 40),
            ("wallet-empty".to_string(), 0),
        ]);
        let slots = HashMap::from([
            ("wallet-high".to_string(), 20),
            ("wallet-low".to_string(), 21),
            ("wallet-empty".to_string(), 22),
        ]);
        let actions = build_balance_reconciliation_actions(
            &wallet_keys,
            "Mint111",
            &replayed,
            &onchain,
            &slots,
            2_000,
        );

        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::ReceivedWithoutCostBasis { wallet_key, amount_raw, .. }
                if wallet_key == "wallet-high" && *amount_raw == 15
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::SentWithoutProceeds { wallet_key, amount_raw, .. }
                if wallet_key == "wallet-low" && *amount_raw == 30
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::ReceivedWithoutCostBasis { wallet_key, amount_raw, .. }
                if wallet_key == "wallet-empty" && *amount_raw == 7
        )));
        assert!(actions.iter().any(|action| {
            matches!(
                action,
                RpcResyncLedgerAction::ReceivedWithoutCostBasis { wallet_key, slot, .. }
                    if wallet_key == "wallet-high" && *slot == Some(20)
            )
        }));
        assert!(actions.iter().any(|action| {
            matches!(
                action,
                RpcResyncLedgerAction::SentWithoutProceeds { wallet_key, slot, .. }
                    if wallet_key == "wallet-low" && *slot == Some(21)
            )
        }));
    }

    #[test]
    fn balance_reconciliation_clears_stale_markers_when_balances_match() {
        let wallet_keys = vec!["wallet-alpha".to_string()];
        let balances = HashMap::from([("wallet-alpha".to_string(), 40)]);
        let slots = HashMap::from([("wallet-alpha".to_string(), 20)]);
        let actions = build_balance_reconciliation_actions(
            &wallet_keys,
            "Mint111",
            &balances,
            &balances,
            &slots,
            2_000,
        );

        assert_eq!(actions.len(), 2);
        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::ReceivedWithoutCostBasis { amount_raw: 0, .. }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::SentWithoutProceeds { amount_raw: 0, .. }
        )));
    }

    #[test]
    fn balance_reconciliation_ignores_wallet_without_observed_balance_slot() {
        let wallet_keys = vec!["wallet-alpha".to_string()];
        let replayed = HashMap::from([("wallet-alpha".to_string(), 40)]);
        let actions = build_balance_reconciliation_actions(
            &wallet_keys,
            "Mint111",
            &replayed,
            &HashMap::new(),
            &HashMap::new(),
            2_000,
        );

        assert!(actions.is_empty());
    }

    #[test]
    fn balance_reconciliation_force_closes_when_onchain_balance_is_zero() {
        let wallet_keys = vec!["wallet-alpha".to_string()];
        let replayed = HashMap::from([("wallet-alpha".to_string(), 40)]);
        let onchain = HashMap::from([("wallet-alpha".to_string(), 0)]);
        let slots = HashMap::from([("wallet-alpha".to_string(), 20)]);
        let actions = build_balance_reconciliation_actions(
            &wallet_keys,
            "Mint111",
            &replayed,
            &onchain,
            &slots,
            2_000,
        );

        assert!(actions.iter().any(|action| matches!(
            action,
            RpcResyncLedgerAction::ForceClose {
                marker,
                persist: true
            } if marker.wallet_key == "wallet-alpha"
                && marker.reason == "on-chain-zero-after-resync"
        )));
        assert!(actions.iter().all(|action| !matches!(
            action,
            RpcResyncLedgerAction::SentWithoutProceeds { amount_raw: 40, .. }
        )));
    }

    #[test]
    fn zero_clear_does_not_shadow_same_signature_nonzero_adjustment() {
        let mut actions = vec![
            RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key: "wallet-alpha".to_string(),
                mint: "Mint111".to_string(),
                amount_raw: 0,
                signature: "sig-transfer".to_string(),
                applied_at_unix_ms: 1_000,
                slot: Some(10),
                persist: true,
            },
            RpcResyncLedgerAction::SentWithoutProceeds {
                wallet_key: "wallet-alpha".to_string(),
                mint: "Mint111".to_string(),
                amount_raw: 40,
                signature: "sig-transfer".to_string(),
                applied_at_unix_ms: 1_000,
                slot: Some(10),
                persist: true,
            },
        ];

        prune_incomplete_zero_clears_shadowed_by_nonzero_actions(&mut actions);

        assert!(matches!(
            actions.as_slice(),
            [RpcResyncLedgerAction::SentWithoutProceeds { amount_raw: 40, .. }]
        ));
    }

    #[test]
    fn canonical_config_round_trips_default_policy_settings() {
        let mut state = sample_engine_state();
        state.settings.default_buy_funding_policy = BuyFundingPolicy::PreferUsd1ElseTopUp;
        state.settings.default_sell_settlement_policy = SellSettlementPolicy::AlwaysToUsd1;

        let config = canonical_config_from_legacy(&state.settings, &state.presets);
        assert_eq!(
            config["defaults"]["misc"]["defaultBuyFundingPolicy"],
            json!("prefer_usd1_else_topup")
        );
        assert_eq!(
            config["defaults"]["misc"]["defaultSellSettlementPolicy"],
            json!("always_to_usd1")
        );
        assert_eq!(
            config_default_buy_funding_policy(&config),
            BuyFundingPolicy::PreferUsd1ElseTopUp
        );
        assert_eq!(
            config_default_sell_settlement_policy(&config),
            SellSettlementPolicy::AlwaysToUsd1
        );
    }

    #[test]
    fn fingerprint_changes_when_provider_changes() {
        let state = sample_engine_state();
        let target = resolve_batch_target(
            &state.wallets,
            &state.wallet_groups,
            Some("wallet-alpha".to_string()),
            None,
            None,
        )
        .expect("single wallet target");
        let planned = LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: crate::trade_planner::TradeVenueFamily::PumpAmm,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Wsol,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::PumpAmmWsolBuy,
            wrapper_accounts: vec![],
            market_subtype: None,
            direct_protocol_target: None,
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        };
        let base_policy = ResolvedTradePolicy {
            slippage_percent: "10".to_string(),
            mev_mode: MevMode::Off,
            auto_tip_enabled: false,
            fee_sol: "0.001".to_string(),
            tip_sol: "0.001".to_string(),
            provider: "standard-rpc".to_string(),
            endpoint_profile: "global".to_string(),
            commitment: "confirmed".to_string(),
            skip_preflight: false,
            track_send_block_height: true,
            buy_amount_sol: Some("0.5".to_string()),
            sell_percent: None,
            buy_funding_policy: BuyFundingPolicy::SolOnly,
            sell_settlement_policy: SellSettlementPolicy::AlwaysToSol,
            sell_settlement_asset: TradeSettlementAsset::Sol,
            auto_fee_warnings: Vec::new(),
        };
        let alt_policy = ResolvedTradePolicy {
            provider: "hellomoon".to_string(),
            ..base_policy.clone()
        };
        let execution_plan = vec![WalletExecutionPlanSummary {
            wallet_key: "wallet-alpha".to_string(),
            buy_amount_sol: Some("0.5".to_string()),
            scheduled_delay_ms: 0,
            delay_applied: false,
            first_buy: None,
            applied_variance_percent: None,
            wrapper_fee_bps: 0,
            wrapper_fee_sol: None,
            wrapper_route: None,
        }];
        let left = build_trade_fingerprint(
            &TradeSide::Buy,
            "Mint111",
            "preset1",
            &target,
            Some(&planned),
            Some("0.5"),
            None,
            None,
            &base_policy,
            None,
            None,
            None,
            &execution_plan,
        );
        let right = build_trade_fingerprint(
            &TradeSide::Buy,
            "Mint111",
            "preset1",
            &target,
            Some(&planned),
            Some("0.5"),
            None,
            None,
            &alt_policy,
            None,
            None,
            None,
            &execution_plan,
        );
        assert_ne!(left, right);
    }

    #[test]
    fn compress_first_buy_only_delays_drops_waits_for_existing_positions() {
        let mut execution_plan = vec![
            PlannedWalletExecution {
                wallet_key: "wallet-alpha".to_string(),
                wallet_request: WalletTradeRequest {
                    side: TradeSide::Buy,
                    mint: "Mint111".to_string(),
                    platform_label: None,
                    buy_amount_sol: Some("0.5".to_string()),
                    sell_intent: None,
                    policy: ExecutionPolicy {
                        slippage_percent: "10".to_string(),
                        mev_mode: MevMode::Off,
                        auto_tip_enabled: false,
                        fee_sol: "0.001".to_string(),
                        tip_sol: "0.001".to_string(),
                        provider: "standard-rpc".to_string(),
                        endpoint_profile: "global".to_string(),
                        commitment: "confirmed".to_string(),
                        skip_preflight: false,
                        track_send_block_height: false,
                        buy_funding_policy: BuyFundingPolicy::SolOnly,
                        sell_settlement_policy: SellSettlementPolicy::AlwaysToSol,
                        sell_settlement_asset: TradeSettlementAsset::Sol,
                    },
                    planned_route: None,
                    planned_trade: None,
                    pinned_pool: None,
                    warm_key: None,
                },
                planned_summary: WalletExecutionPlanSummary {
                    wallet_key: "wallet-alpha".to_string(),
                    buy_amount_sol: Some("0.5".to_string()),
                    scheduled_delay_ms: 0,
                    delay_applied: false,
                    first_buy: Some(true),
                    applied_variance_percent: None,
                    wrapper_fee_bps: 0,
                    wrapper_fee_sol: None,
                    wrapper_route: None,
                },
            },
            PlannedWalletExecution {
                wallet_key: "wallet-bravo".to_string(),
                wallet_request: WalletTradeRequest {
                    side: TradeSide::Buy,
                    mint: "Mint111".to_string(),
                    platform_label: None,
                    buy_amount_sol: Some("0.5".to_string()),
                    sell_intent: None,
                    policy: ExecutionPolicy {
                        slippage_percent: "10".to_string(),
                        mev_mode: MevMode::Off,
                        auto_tip_enabled: false,
                        fee_sol: "0.001".to_string(),
                        tip_sol: "0.001".to_string(),
                        provider: "standard-rpc".to_string(),
                        endpoint_profile: "global".to_string(),
                        commitment: "confirmed".to_string(),
                        skip_preflight: false,
                        track_send_block_height: false,
                        buy_funding_policy: BuyFundingPolicy::SolOnly,
                        sell_settlement_policy: SellSettlementPolicy::AlwaysToSol,
                        sell_settlement_asset: TradeSettlementAsset::Sol,
                    },
                    planned_route: None,
                    planned_trade: None,
                    pinned_pool: None,
                    warm_key: None,
                },
                planned_summary: WalletExecutionPlanSummary {
                    wallet_key: "wallet-bravo".to_string(),
                    buy_amount_sol: Some("0.5".to_string()),
                    scheduled_delay_ms: 100,
                    delay_applied: true,
                    first_buy: Some(true),
                    applied_variance_percent: None,
                    wrapper_fee_bps: 0,
                    wrapper_fee_sol: None,
                    wrapper_route: None,
                },
            },
            PlannedWalletExecution {
                wallet_key: "wallet-charlie".to_string(),
                wallet_request: WalletTradeRequest {
                    side: TradeSide::Buy,
                    mint: "Mint111".to_string(),
                    platform_label: None,
                    buy_amount_sol: Some("0.5".to_string()),
                    sell_intent: None,
                    policy: ExecutionPolicy {
                        slippage_percent: "10".to_string(),
                        mev_mode: MevMode::Off,
                        auto_tip_enabled: false,
                        fee_sol: "0.001".to_string(),
                        tip_sol: "0.001".to_string(),
                        provider: "standard-rpc".to_string(),
                        endpoint_profile: "global".to_string(),
                        commitment: "confirmed".to_string(),
                        skip_preflight: false,
                        track_send_block_height: false,
                        buy_funding_policy: BuyFundingPolicy::SolOnly,
                        sell_settlement_policy: SellSettlementPolicy::AlwaysToSol,
                        sell_settlement_asset: TradeSettlementAsset::Sol,
                    },
                    planned_route: None,
                    planned_trade: None,
                    pinned_pool: None,
                    warm_key: None,
                },
                planned_summary: WalletExecutionPlanSummary {
                    wallet_key: "wallet-charlie".to_string(),
                    buy_amount_sol: Some("0.5".to_string()),
                    scheduled_delay_ms: 200,
                    delay_applied: true,
                    first_buy: Some(true),
                    applied_variance_percent: None,
                    wrapper_fee_bps: 0,
                    wrapper_fee_sol: None,
                    wrapper_route: None,
                },
            },
        ];
        let mut first_buy_flags = HashMap::new();
        first_buy_flags.insert("wallet-alpha".to_string(), true);
        first_buy_flags.insert("wallet-bravo".to_string(), false);
        first_buy_flags.insert("wallet-charlie".to_string(), true);
        let policy = BatchExecutionPolicySummary {
            distribution_mode: BuyDistributionMode::Each,
            buy_variance_percent: 0,
            transaction_delay_mode: TransactionDelayMode::FirstBuyOnly,
            transaction_delay_strategy: TransactionDelayStrategy::Fixed,
            transaction_delay_ms: 100,
            transaction_delay_min_ms: 0,
            transaction_delay_max_ms: 0,
            base_wallet_amount_sol: Some("0.5".to_string()),
            total_batch_spend_sol: Some("1.5".to_string()),
        };

        compress_first_buy_only_delays(&mut execution_plan, &first_buy_flags, &policy);

        assert_eq!(execution_plan[0].planned_summary.first_buy, Some(true));
        assert_eq!(execution_plan[0].planned_summary.scheduled_delay_ms, 0);
        assert_eq!(execution_plan[1].planned_summary.first_buy, Some(false));
        assert_eq!(execution_plan[1].planned_summary.scheduled_delay_ms, 0);
        assert_eq!(execution_plan[1].planned_summary.delay_applied, false);
        assert_eq!(execution_plan[2].planned_summary.first_buy, Some(true));
        assert_eq!(execution_plan[2].planned_summary.scheduled_delay_ms, 100);
    }

    #[test]
    fn wallet_group_policy_caps_transaction_delays() {
        let fixed = normalize_wallet_group_batch_policy(WalletGroupBatchPolicy {
            transaction_delay_mode: TransactionDelayMode::On,
            transaction_delay_strategy: TransactionDelayStrategy::Fixed,
            transaction_delay_ms: 30_000,
            ..WalletGroupBatchPolicy::default()
        });
        assert_eq!(fixed.transaction_delay_ms, MAX_TRANSACTION_DELAY_MS);

        let random = normalize_wallet_group_batch_policy(WalletGroupBatchPolicy {
            transaction_delay_mode: TransactionDelayMode::On,
            transaction_delay_strategy: TransactionDelayStrategy::Random,
            transaction_delay_min_ms: 30_000,
            transaction_delay_max_ms: 10_000,
            ..WalletGroupBatchPolicy::default()
        });
        assert_eq!(random.transaction_delay_min_ms, MAX_TRANSACTION_DELAY_MS);
        assert_eq!(random.transaction_delay_max_ms, MAX_TRANSACTION_DELAY_MS);
    }

    #[test]
    fn bonk_sell_output_is_allowed_for_target_sizing() {
        let selector = LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PreMigration,
            family: crate::trade_planner::TradeVenueFamily::BonkLaunchpad,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Sol,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::BonkLaunchpadSolSell,
            wrapper_accounts: vec![],
            market_subtype: None,
            direct_protocol_target: None,
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        };
        validate_sell_intent_for_family(&SellIntent::SolOutput("0.1".to_string()), &selector)
            .expect("bonk sellOutputSol should pass API validation");
    }

    #[test]
    fn trusted_stable_sell_output_requires_sol_output_route() {
        let selector = LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: crate::trade_planner::TradeVenueFamily::TrustedStableSwap,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Usdc,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::TrustedStableSwapSell,
            wrapper_accounts: vec![],
            market_subtype: Some("orca-whirlpool".to_string()),
            direct_protocol_target: None,
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: Some(crate::trade_planner::PlannerRuntimeBundle::TrustedStable(
                crate::trade_planner::TrustedStableRuntimeBundle {
                    pool: "pool-1".to_string(),
                    venue: "orca-whirlpool".to_string(),
                    buy_input_mint: crate::wrapper_abi::WSOL_MINT.to_string(),
                    buy_output_mint: "token-mint".to_string(),
                    sell_input_mint: "token-mint".to_string(),
                    sell_output_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                },
            )),
        };
        let err =
            validate_sell_intent_for_family(&SellIntent::SolOutput("0.1".to_string()), &selector)
                .expect_err("non-SOL trusted stable route should be rejected");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn trusted_stable_sell_output_allows_sol_output_route() {
        let selector = LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: crate::trade_planner::TradeVenueFamily::TrustedStableSwap,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Sol,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::TrustedStableSwapSell,
            wrapper_accounts: vec![],
            market_subtype: Some("raydium-clmm".to_string()),
            direct_protocol_target: None,
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: Some(crate::trade_planner::PlannerRuntimeBundle::TrustedStable(
                crate::trade_planner::TrustedStableRuntimeBundle {
                    pool: "pool-1".to_string(),
                    venue: "raydium-clmm".to_string(),
                    buy_input_mint: crate::wrapper_abi::WSOL_MINT.to_string(),
                    buy_output_mint: "token-mint".to_string(),
                    sell_input_mint: "token-mint".to_string(),
                    sell_output_mint: crate::wrapper_abi::WSOL_MINT.to_string(),
                },
            )),
        };
        validate_sell_intent_for_family(&SellIntent::SolOutput("0.1".to_string()), &selector)
            .expect("SOL-output trusted stable route should pass API validation");
    }

    #[test]
    fn meteora_sell_output_rejects_non_usdc_stable_routes() {
        let selector = LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: crate::trade_planner::TradeVenueFamily::MeteoraDammV2,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Usd1,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::MeteoraDammV2Sell,
            wrapper_accounts: vec![],
            market_subtype: Some("damm-v2".to_string()),
            direct_protocol_target: Some("damm-v2".to_string()),
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        };
        let err =
            validate_sell_intent_for_family(&SellIntent::SolOutput("0.1".to_string()), &selector)
                .expect_err("USD1 Meteora sellOutputSol should be rejected");
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn preview_compile_probe_targets_bonk_usd1_sells() {
        let selector = LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: crate::trade_planner::TradeVenueFamily::BonkRaydium,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Usd1,
            verification_source: crate::trade_planner::PlannerVerificationSource::HybridDerived,
            wrapper_action: crate::trade_planner::WrapperAction::BonkRaydiumUsd1Sell,
            wrapper_accounts: vec![],
            market_subtype: Some("canonical-raydium".to_string()),
            direct_protocol_target: Some("raydium".to_string()),
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        };
        assert!(preview_compile_probe_required(&TradeSide::Sell, &selector));
        assert!(preview_compile_probe_required(&TradeSide::Buy, &selector));
    }

    #[test]
    fn preview_compile_probe_targets_meteora_routes() {
        let mut selector = LifecycleAndCanonicalMarket {
            lifecycle: crate::trade_planner::TradeLifecycle::PostMigration,
            family: crate::trade_planner::TradeVenueFamily::MeteoraDammV2,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: crate::trade_planner::PlannerQuoteAsset::Sol,
            verification_source: crate::trade_planner::PlannerVerificationSource::OnchainDerived,
            wrapper_action: crate::trade_planner::WrapperAction::MeteoraDammV2Buy,
            wrapper_accounts: vec![],
            market_subtype: Some("damm-v2".to_string()),
            direct_protocol_target: Some("meteora-damm-v2".to_string()),
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        };
        assert!(preview_compile_probe_required(&TradeSide::Buy, &selector));
        selector.family = crate::trade_planner::TradeVenueFamily::MeteoraDbc;
        selector.wrapper_action = crate::trade_planner::WrapperAction::MeteoraDbcBuy;
        assert!(preview_compile_probe_required(&TradeSide::Buy, &selector));
    }
}
