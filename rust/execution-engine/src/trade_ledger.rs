use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::extension_api::{TradeSettlementAsset, TradeSide};

const PNL_ROOT_DIR: &str = "pnl";
const PNL_JOURNAL_DIR: &str = "journal";
const PNL_SNAPSHOTS_DIR: &str = "snapshots";
const LEGACY_TRADE_LEDGER_FILE: &str = "trade-ledger.json";
const PNL_JOURNAL_PREFIX: &str = "confirmed-trades";
const PNL_JOURNAL_SUFFIX: &str = ".jsonl";
const PNL_JOURNAL_SEGMENT_MAX_BYTES: u64 = 4 * 1024 * 1024;
const OPEN_POSITIONS_FILE: &str = "open-positions.json";
const CLOSED_POSITIONS_FILE: &str = "closed-positions.json";
const SNAPSHOTS_FILE: &str = "pnl-snapshots.json";
const TRADE_LEDGER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct TradeLedgerPaths {
    pub root_dir: PathBuf,
    pub journal_dir: PathBuf,
    pub open_positions_path: PathBuf,
    pub closed_positions_path: PathBuf,
    pub snapshots_path: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StoredEntryPreference {
    Sol,
    Usd1,
    Mixed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PlatformTag {
    Axiom,
    J7,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventProvenance {
    LocalExecution,
    RpcResync,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExplicitFeeBreakdown {
    #[serde(default)]
    pub network_fee_lamports: u64,
    #[serde(default)]
    pub priority_fee_lamports: u64,
    #[serde(default)]
    pub tip_lamports: u64,
    #[serde(default)]
    pub rent_delta_lamports: i64,
}

impl ExplicitFeeBreakdown {
    pub fn total_lamports(&self) -> i64 {
        i64::try_from(
            self.network_fee_lamports
                .saturating_add(self.priority_fee_lamports)
                .saturating_add(self.tip_lamports),
        )
        .unwrap_or(i64::MAX)
        .saturating_add(self.rent_delta_lamports)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenLot {
    pub acquired_at_unix_ms: u64,
    pub signature: String,
    #[serde(default)]
    pub settlement_asset: Option<TradeSettlementAsset>,
    pub acquired_amount_raw: u64,
    pub remaining_amount_raw: u64,
    pub remaining_cost_basis_lamports: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedTradeEvent {
    #[serde(default = "trade_ledger_schema_version")]
    pub schema_version: u32,
    pub signature: String,
    #[serde(default)]
    pub slot: Option<u64>,
    pub confirmed_at_unix_ms: u64,
    pub wallet_key: String,
    pub wallet_public_key: String,
    pub mint: String,
    pub side: TradeSide,
    pub platform_tag: PlatformTag,
    pub provenance: EventProvenance,
    #[serde(default)]
    pub settlement_asset: Option<TradeSettlementAsset>,
    pub token_delta_raw: i128,
    #[serde(default)]
    pub token_decimals: Option<u8>,
    pub trade_value_lamports: u64,
    #[serde(default)]
    pub explicit_fees: ExplicitFeeBreakdown,
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub batch_id: Option<String>,
}

impl ConfirmedTradeEvent {
    pub fn event_id(&self) -> String {
        format!(
            "{}::{}::{}::{}",
            self.signature.trim(),
            self.wallet_key.trim(),
            self.mint.trim(),
            match self.side {
                TradeSide::Buy => "buy",
                TradeSide::Sell => "sell",
            }
        )
    }
}

/// Journal marker emitted when a user triggers "Reset PnL for this coin".
///
/// Written to the same append-only `confirmed-trades-*.jsonl` segments as trade
/// events. Rebuilding the ledger from the journal alone must honour these
/// markers, otherwise a snapshot-file loss would silently re-import old trades
/// the user explicitly wiped.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetMarkerEvent {
    #[serde(default = "trade_ledger_schema_version")]
    pub schema_version: u32,
    /// Discriminator that distinguishes this line from a [`ConfirmedTradeEvent`]
    /// when parsing a journal segment. Always serialised as `"reset_marker"`.
    pub event_kind: String,
    pub wallet_key: String,
    pub mint: String,
    pub reset_at_unix_ms: u64,
    #[serde(default)]
    pub reset_at_slot: Option<u64>,
}

pub const JOURNAL_RESET_MARKER_KIND: &str = "reset_marker";

impl ResetMarkerEvent {
    pub fn new(
        wallet_key: &str,
        mint: &str,
        reset_at_unix_ms: u64,
        reset_at_slot: Option<u64>,
    ) -> Self {
        Self {
            schema_version: trade_ledger_schema_version(),
            event_kind: JOURNAL_RESET_MARKER_KIND.to_string(),
            wallet_key: wallet_key.to_string(),
            mint: mint.to_string(),
            reset_at_unix_ms,
            reset_at_slot,
        }
    }
}

/// Journal marker emitted when auto-reconciliation confirms the on-chain
/// balance is zero while the local ledger still carries open lots. Writing
/// this marker synthesises a full close at zero proceeds, realising the
/// remaining cost basis as a loss so subsequent rebuilds converge with the
/// chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForceCloseMarkerEvent {
    #[serde(default = "trade_ledger_schema_version")]
    pub schema_version: u32,
    pub event_kind: String,
    pub wallet_key: String,
    pub mint: String,
    pub applied_at_unix_ms: u64,
    #[serde(default)]
    pub reason: String,
}

pub const JOURNAL_FORCE_CLOSE_MARKER_KIND: &str = "force_close_marker";

impl ForceCloseMarkerEvent {
    pub fn new(wallet_key: &str, mint: &str, applied_at_unix_ms: u64, reason: &str) -> Self {
        Self {
            schema_version: trade_ledger_schema_version(),
            event_kind: JOURNAL_FORCE_CLOSE_MARKER_KIND.to_string(),
            wallet_key: wallet_key.to_string(),
            mint: mint.to_string(),
            applied_at_unix_ms,
            reason: reason.to_string(),
        }
    }
}

/// Journal marker emitted by token distribution transfers. It moves open-lot
/// cost basis between wallets without creating buy/sell volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTransferMarkerEvent {
    #[serde(default = "trade_ledger_schema_version")]
    pub schema_version: u32,
    pub event_kind: String,
    pub source_wallet_key: String,
    pub destination_wallet_key: String,
    pub mint: String,
    pub amount_raw: u64,
    pub signature: String,
    pub applied_at_unix_ms: u64,
    #[serde(default)]
    pub slot: Option<u64>,
}

pub const JOURNAL_TOKEN_TRANSFER_MARKER_KIND: &str = "token_transfer_marker";

impl TokenTransferMarkerEvent {
    pub fn new(
        source_wallet_key: &str,
        destination_wallet_key: &str,
        mint: &str,
        amount_raw: u64,
        signature: &str,
        applied_at_unix_ms: u64,
    ) -> Self {
        Self {
            schema_version: trade_ledger_schema_version(),
            event_kind: JOURNAL_TOKEN_TRANSFER_MARKER_KIND.to_string(),
            source_wallet_key: source_wallet_key.to_string(),
            destination_wallet_key: destination_wallet_key.to_string(),
            mint: mint.to_string(),
            amount_raw,
            signature: signature.to_string(),
            applied_at_unix_ms,
            slot: None,
        }
    }

    pub fn with_slot(mut self, slot: Option<u64>) -> Self {
        self.slot = slot;
        self
    }

    pub fn event_id(&self) -> String {
        format!(
            "{}::{}::{}::{}::{}",
            self.signature.trim(),
            self.source_wallet_key.trim(),
            self.destination_wallet_key.trim(),
            self.mint.trim(),
            self.amount_raw
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncompleteBalanceAdjustmentKind {
    ReceivedWithoutCostBasis,
    SentWithoutProceeds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncompleteBalanceAdjustmentMarkerEvent {
    #[serde(default = "trade_ledger_schema_version")]
    pub schema_version: u32,
    pub event_kind: String,
    pub adjustment_kind: IncompleteBalanceAdjustmentKind,
    pub wallet_key: String,
    pub mint: String,
    pub amount_raw: u64,
    pub signature: String,
    pub applied_at_unix_ms: u64,
    #[serde(default)]
    pub slot: Option<u64>,
}

pub const JOURNAL_INCOMPLETE_BALANCE_ADJUSTMENT_MARKER_KIND: &str =
    "incomplete_balance_adjustment_marker";

impl IncompleteBalanceAdjustmentMarkerEvent {
    pub fn received_without_cost_basis(
        wallet_key: &str,
        mint: &str,
        amount_raw: u64,
        signature: &str,
        applied_at_unix_ms: u64,
        slot: Option<u64>,
    ) -> Self {
        Self::new(
            IncompleteBalanceAdjustmentKind::ReceivedWithoutCostBasis,
            wallet_key,
            mint,
            amount_raw,
            signature,
            applied_at_unix_ms,
            slot,
        )
    }

    pub fn sent_without_proceeds(
        wallet_key: &str,
        mint: &str,
        amount_raw: u64,
        signature: &str,
        applied_at_unix_ms: u64,
        slot: Option<u64>,
    ) -> Self {
        Self::new(
            IncompleteBalanceAdjustmentKind::SentWithoutProceeds,
            wallet_key,
            mint,
            amount_raw,
            signature,
            applied_at_unix_ms,
            slot,
        )
    }

    fn new(
        adjustment_kind: IncompleteBalanceAdjustmentKind,
        wallet_key: &str,
        mint: &str,
        amount_raw: u64,
        signature: &str,
        applied_at_unix_ms: u64,
        slot: Option<u64>,
    ) -> Self {
        Self {
            schema_version: trade_ledger_schema_version(),
            event_kind: JOURNAL_INCOMPLETE_BALANCE_ADJUSTMENT_MARKER_KIND.to_string(),
            adjustment_kind,
            wallet_key: wallet_key.to_string(),
            mint: mint.to_string(),
            amount_raw,
            signature: signature.to_string(),
            applied_at_unix_ms,
            slot,
        }
    }

    pub fn event_id(&self) -> String {
        if self
            .signature
            .trim()
            .starts_with("resync-balance-reconcile:")
        {
            return format!(
                "resync-balance-reconcile::{:?}::{}::{}",
                self.adjustment_kind,
                self.wallet_key.trim(),
                self.mint.trim()
            );
        }
        format!(
            "{}::{:?}::{}::{}",
            self.signature.trim(),
            self.adjustment_kind,
            self.wallet_key.trim(),
            self.mint.trim()
        )
    }
}

#[derive(Debug, Clone)]
pub enum JournalEntry {
    Trade(ConfirmedTradeEvent),
    ResetMarker(ResetMarkerEvent),
    ForceCloseMarker(ForceCloseMarkerEvent),
    TokenTransferMarker(TokenTransferMarkerEvent),
    IncompleteBalanceAdjustmentMarker(IncompleteBalanceAdjustmentMarkerEvent),
}

impl JournalEntry {
    fn timestamp(&self) -> u64 {
        match self {
            JournalEntry::Trade(event) => event.confirmed_at_unix_ms,
            JournalEntry::ResetMarker(marker) => marker.reset_at_unix_ms,
            JournalEntry::ForceCloseMarker(marker) => marker.applied_at_unix_ms,
            JournalEntry::TokenTransferMarker(marker) => marker.applied_at_unix_ms,
            JournalEntry::IncompleteBalanceAdjustmentMarker(marker) => marker.applied_at_unix_ms,
        }
    }

    fn slot_sort_key(&self) -> u64 {
        match self {
            JournalEntry::Trade(event) => event.slot.unwrap_or(u64::MAX),
            JournalEntry::ResetMarker(marker) => marker.reset_at_slot.unwrap_or(u64::MAX),
            JournalEntry::TokenTransferMarker(marker) => marker.slot.unwrap_or(u64::MAX),
            JournalEntry::IncompleteBalanceAdjustmentMarker(marker) => {
                marker.slot.unwrap_or(u64::MAX)
            }
            JournalEntry::ForceCloseMarker(_) => u64::MAX,
        }
    }

    fn wallet_mint_key(&self) -> String {
        match self {
            JournalEntry::Trade(event) => trade_ledger_key(&event.wallet_key, &event.mint),
            JournalEntry::ResetMarker(marker) => trade_ledger_key(&marker.wallet_key, &marker.mint),
            JournalEntry::ForceCloseMarker(marker) => {
                trade_ledger_key(&marker.wallet_key, &marker.mint)
            }
            JournalEntry::TokenTransferMarker(marker) => {
                trade_ledger_key(&marker.source_wallet_key, &marker.mint)
            }
            JournalEntry::IncompleteBalanceAdjustmentMarker(marker) => {
                trade_ledger_key(&marker.wallet_key, &marker.mint)
            }
        }
    }

    fn replay_rank(&self) -> u8 {
        match self {
            JournalEntry::Trade(_) => 0,
            JournalEntry::ResetMarker(_) => 1,
            JournalEntry::TokenTransferMarker(_) => 2,
            JournalEntry::IncompleteBalanceAdjustmentMarker(_) => 3,
            JournalEntry::ForceCloseMarker(_) => 4,
        }
    }

    fn identity_key(&self) -> String {
        match self {
            JournalEntry::Trade(event) => event.event_id(),
            JournalEntry::ResetMarker(marker) => format!(
                "reset::{}::{}::{}::{}",
                marker.wallet_key.trim(),
                marker.mint.trim(),
                marker.reset_at_unix_ms,
                marker.reset_at_slot.unwrap_or(u64::MAX)
            ),
            JournalEntry::ForceCloseMarker(marker) => format!(
                "force-close::{}::{}::{}::{}",
                marker.wallet_key.trim(),
                marker.mint.trim(),
                marker.applied_at_unix_ms,
                marker.reason.trim()
            ),
            JournalEntry::TokenTransferMarker(marker) => marker.event_id(),
            JournalEntry::IncompleteBalanceAdjustmentMarker(marker) => marker.event_id(),
        }
    }
}

fn incomplete_marker_order_key(marker: &IncompleteBalanceAdjustmentMarkerEvent) -> (u64, u64) {
    (marker.slot.unwrap_or(0), marker.applied_at_unix_ms)
}

fn incomplete_marker_clear_scope(marker: &IncompleteBalanceAdjustmentMarkerEvent) -> String {
    format!(
        "{:?}::{}::{}::{}",
        marker.adjustment_kind,
        marker.wallet_key.trim(),
        marker.mint.trim(),
        marker.signature.trim()
    )
}

fn compare_journal_entries(left: &JournalEntry, right: &JournalEntry) -> std::cmp::Ordering {
    left.timestamp()
        .cmp(&right.timestamp())
        .then_with(|| left.slot_sort_key().cmp(&right.slot_sort_key()))
        .then_with(|| left.wallet_mint_key().cmp(&right.wallet_mint_key()))
        .then_with(|| left.replay_rank().cmp(&right.replay_rank()))
        .then_with(|| left.identity_key().cmp(&right.identity_key()))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TradeLedgerEntry {
    pub wallet_key: String,
    pub mint: String,
    #[serde(default)]
    pub tracked_bought_lamports: u64,
    #[serde(default)]
    pub tracked_sold_lamports: u64,
    #[serde(default)]
    pub buy_count: u64,
    #[serde(default)]
    pub sell_count: u64,
    #[serde(default)]
    pub last_trade_at_unix_ms: u64,
    #[serde(default)]
    pub entry_preference: Option<StoredEntryPreference>,
    #[serde(default)]
    pub position_open: bool,
    #[serde(default)]
    pub realized_pnl_gross_lamports: i64,
    #[serde(default)]
    pub realized_pnl_net_lamports: i64,
    #[serde(default)]
    pub explicit_fee_total_lamports: i64,
    #[serde(default)]
    pub remaining_cost_basis_lamports: u64,
    #[serde(default)]
    pub unmatched_sell_amount_raw: u64,
    #[serde(default)]
    pub needs_resync: bool,
    #[serde(default)]
    pub reset_baseline_unix_ms: u64,
    #[serde(default)]
    pub reset_baseline_slot: Option<u64>,
    #[serde(default)]
    pub open_lots: Vec<OpenLot>,
    #[serde(default)]
    pub platform_tags: Vec<PlatformTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TradeLedgerAggregate {
    #[serde(default)]
    pub tracked_bought_lamports: u64,
    #[serde(default)]
    pub tracked_sold_lamports: u64,
    #[serde(default)]
    pub buy_count: u64,
    #[serde(default)]
    pub sell_count: u64,
    #[serde(default)]
    pub last_trade_at_unix_ms: u64,
    #[serde(default)]
    pub realized_pnl_gross_lamports: i64,
    #[serde(default)]
    pub realized_pnl_net_lamports: i64,
    #[serde(default)]
    pub explicit_fee_total_lamports: i64,
    #[serde(default)]
    pub remaining_cost_basis_lamports: u64,
    #[serde(default)]
    pub unmatched_sell_amount_raw: u64,
    #[serde(default)]
    pub needs_resync: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnlSnapshotRecord {
    wallet_key: String,
    mint: String,
    tracked_bought_lamports: u64,
    tracked_sold_lamports: u64,
    buy_count: u64,
    sell_count: u64,
    last_trade_at_unix_ms: u64,
    entry_preference: Option<StoredEntryPreference>,
    position_open: bool,
    realized_pnl_gross_lamports: i64,
    realized_pnl_net_lamports: i64,
    explicit_fee_total_lamports: i64,
    remaining_cost_basis_lamports: u64,
    unmatched_sell_amount_raw: u64,
    needs_resync: bool,
    reset_baseline_unix_ms: u64,
    #[serde(default)]
    reset_baseline_slot: Option<u64>,
    #[serde(default)]
    open_lots: Vec<OpenLot>,
    #[serde(default)]
    platform_tags: Vec<PlatformTag>,
}

#[derive(Debug, Clone)]
pub struct RecordConfirmedTradeParams<'a> {
    pub wallet_key: &'a str,
    pub wallet_public_key: &'a str,
    pub mint: &'a str,
    pub side: TradeSide,
    pub trade_value_lamports: u64,
    pub token_delta_raw: i128,
    pub token_decimals: Option<u8>,
    pub confirmed_at_unix_ms: u64,
    pub slot: Option<u64>,
    pub entry_preference_asset: Option<TradeSettlementAsset>,
    pub settlement_asset: Option<TradeSettlementAsset>,
    pub explicit_fees: ExplicitFeeBreakdown,
    pub platform_tag: PlatformTag,
    pub provenance: EventProvenance,
    pub signature: &'a str,
    pub client_request_id: Option<&'a str>,
    pub batch_id: Option<&'a str>,
}

pub fn trade_ledger_schema_version() -> u32 {
    TRADE_LEDGER_SCHEMA_VERSION
}

pub fn trade_ledger_paths(data_root: &str) -> TradeLedgerPaths {
    let root_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(data_root)
        .join(PNL_ROOT_DIR);
    let journal_dir = root_dir.join(PNL_JOURNAL_DIR);
    let snapshots_dir = root_dir.join(PNL_SNAPSHOTS_DIR);
    TradeLedgerPaths {
        root_dir,
        journal_dir,
        open_positions_path: snapshots_dir.join(OPEN_POSITIONS_FILE),
        closed_positions_path: snapshots_dir.join(CLOSED_POSITIONS_FILE),
        snapshots_path: snapshots_dir.join(SNAPSHOTS_FILE),
    }
}

pub fn trade_ledger_path(data_root: &str) -> PathBuf {
    trade_ledger_paths(data_root).snapshots_path
}

pub fn load_trade_ledger(paths: &TradeLedgerPaths) -> HashMap<String, TradeLedgerEntry> {
    let mut entries = HashMap::new();
    let full_ok = merge_entries_from_snapshot_file(&paths.snapshots_path, &mut entries);
    let open_ok = merge_entries_from_file(&paths.open_positions_path, &mut entries);
    let closed_ok = merge_entries_from_file(&paths.closed_positions_path, &mut entries);
    if full_ok || (open_ok && closed_ok) {
        return entries;
    }
    if open_ok || closed_ok {
        let rebuilt = rebuild_trade_ledger_from_journal(paths);
        if !rebuilt.is_empty() {
            return rebuilt;
        }
        return entries;
    }
    if let Some(legacy_entries) = load_legacy_trade_ledger(paths) {
        return legacy_entries;
    }
    rebuild_trade_ledger_from_journal(paths)
}

pub fn load_trade_ledger_known_event_ids(paths: &TradeLedgerPaths) -> HashSet<String> {
    let mut known_event_ids = HashSet::new();
    for event in read_confirmed_trade_events(paths) {
        known_event_ids.insert(event.event_id());
    }
    known_event_ids
}

pub fn read_confirmed_trade_events(paths: &TradeLedgerPaths) -> Vec<ConfirmedTradeEvent> {
    read_journal_entries(paths)
        .into_iter()
        .filter_map(|entry| match entry {
            JournalEntry::Trade(event) => Some(event),
            JournalEntry::ResetMarker(_)
            | JournalEntry::ForceCloseMarker(_)
            | JournalEntry::TokenTransferMarker(_)
            | JournalEntry::IncompleteBalanceAdjustmentMarker(_) => None,
        })
        .collect()
}

pub fn read_journal_entries(paths: &TradeLedgerPaths) -> Vec<JournalEntry> {
    let mut entries = Vec::new();
    for path in journal_segment_paths(&paths.journal_dir) {
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                continue;
            };
            let event_kind = value.get("eventKind").and_then(serde_json::Value::as_str);
            match event_kind {
                Some(JOURNAL_RESET_MARKER_KIND) => {
                    if let Ok(marker) = serde_json::from_value::<ResetMarkerEvent>(value) {
                        entries.push(JournalEntry::ResetMarker(marker));
                    }
                }
                Some(JOURNAL_FORCE_CLOSE_MARKER_KIND) => {
                    if let Ok(marker) = serde_json::from_value::<ForceCloseMarkerEvent>(value) {
                        entries.push(JournalEntry::ForceCloseMarker(marker));
                    }
                }
                Some(JOURNAL_TOKEN_TRANSFER_MARKER_KIND) => {
                    if let Ok(marker) = serde_json::from_value::<TokenTransferMarkerEvent>(value) {
                        entries.push(JournalEntry::TokenTransferMarker(marker));
                    }
                }
                Some(JOURNAL_INCOMPLETE_BALANCE_ADJUSTMENT_MARKER_KIND) => {
                    if let Ok(marker) =
                        serde_json::from_value::<IncompleteBalanceAdjustmentMarkerEvent>(value)
                    {
                        entries.push(JournalEntry::IncompleteBalanceAdjustmentMarker(marker));
                    }
                }
                _ => {
                    if let Ok(event) = serde_json::from_value::<ConfirmedTradeEvent>(value) {
                        entries.push(JournalEntry::Trade(event));
                    }
                }
            }
        }
    }
    entries
}

pub fn persist_trade_ledger(
    paths: &TradeLedgerPaths,
    ledger: &HashMap<String, TradeLedgerEntry>,
) -> Result<(), (StatusCode, String)> {
    fs::create_dir_all(&paths.journal_dir).map_err(internal_error)?;
    if let Some(parent) = paths.open_positions_path.parent() {
        fs::create_dir_all(parent).map_err(internal_error)?;
    }

    let mut open_positions = Vec::new();
    let mut closed_positions = Vec::new();
    let mut snapshot_records = Vec::new();

    let mut entries = ledger.values().cloned().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .last_trade_at_unix_ms
            .cmp(&left.last_trade_at_unix_ms)
            .then(left.wallet_key.cmp(&right.wallet_key))
            .then(left.mint.cmp(&right.mint))
    });

    for entry in entries {
        snapshot_records.push(snapshot_record_from_entry(&entry));
        if entry.position_open {
            open_positions.push(entry);
        } else {
            closed_positions.push(entry);
        }
    }

    atomic_write_json(&paths.snapshots_path, &snapshot_records)?;
    atomic_write_json(&paths.open_positions_path, &open_positions)?;
    atomic_write_json(&paths.closed_positions_path, &closed_positions)?;
    Ok(())
}

pub fn record_confirmed_trade(
    ledger: &mut HashMap<String, TradeLedgerEntry>,
    params: RecordConfirmedTradeParams<'_>,
) -> ConfirmedTradeEvent {
    let event = ConfirmedTradeEvent {
        schema_version: trade_ledger_schema_version(),
        signature: params.signature.to_string(),
        slot: params.slot,
        confirmed_at_unix_ms: params.confirmed_at_unix_ms,
        wallet_key: params.wallet_key.to_string(),
        wallet_public_key: params.wallet_public_key.to_string(),
        mint: params.mint.to_string(),
        side: params.side,
        platform_tag: params.platform_tag,
        provenance: params.provenance,
        settlement_asset: params.settlement_asset,
        token_delta_raw: params.token_delta_raw,
        token_decimals: params.token_decimals,
        trade_value_lamports: params.trade_value_lamports,
        explicit_fees: params.explicit_fees,
        client_request_id: params.client_request_id.map(str::to_string),
        batch_id: params.batch_id.map(str::to_string),
    };
    apply_confirmed_trade_event(ledger, &event, params.entry_preference_asset);
    event
}

pub fn append_confirmed_trade_event(
    paths: &TradeLedgerPaths,
    event: &ConfirmedTradeEvent,
) -> Result<(), (StatusCode, String)> {
    append_journal_line(
        paths,
        &serde_json::to_string(event).map_err(internal_error)?,
    )
}

/// Append a reset-marker line to the confirmed-trade journal so that a journal
/// rebuild after snapshot-file loss still honours the user's "Reset PnL for
/// this coin" action.
pub fn append_reset_marker(
    paths: &TradeLedgerPaths,
    marker: &ResetMarkerEvent,
) -> Result<(), (StatusCode, String)> {
    append_journal_line(
        paths,
        &serde_json::to_string(marker).map_err(internal_error)?,
    )
}

/// Append a force-close-marker line to the journal. Durability mirrors
/// [`append_reset_marker`]: rebuilding from the journal alone replays the
/// synthetic close-at-zero so the ledger converges with on-chain truth.
pub fn append_force_close_marker(
    paths: &TradeLedgerPaths,
    marker: &ForceCloseMarkerEvent,
) -> Result<(), (StatusCode, String)> {
    append_journal_line(
        paths,
        &serde_json::to_string(marker).map_err(internal_error)?,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalAppendStatus {
    Appended,
    Duplicate,
}

pub fn append_token_transfer_marker(
    paths: &TradeLedgerPaths,
    marker: &TokenTransferMarkerEvent,
) -> Result<JournalAppendStatus, (StatusCode, String)> {
    let event_id = marker.event_id();
    if read_journal_entries(paths).into_iter().any(|entry| {
        matches!(entry, JournalEntry::TokenTransferMarker(existing) if existing.event_id() == event_id)
    }) {
        return Ok(JournalAppendStatus::Duplicate);
    }
    append_journal_line(
        paths,
        &serde_json::to_string(marker).map_err(internal_error)?,
    )?;
    Ok(JournalAppendStatus::Appended)
}

pub fn append_incomplete_balance_adjustment_marker(
    paths: &TradeLedgerPaths,
    marker: &IncompleteBalanceAdjustmentMarkerEvent,
) -> Result<JournalAppendStatus, (StatusCode, String)> {
    let event_id = marker.event_id();
    if read_journal_entries(paths).into_iter().any(|entry| {
        matches!(
            entry,
            JournalEntry::IncompleteBalanceAdjustmentMarker(existing)
                if existing.event_id() == event_id
                    && existing.amount_raw == marker.amount_raw
                    && existing.slot == marker.slot
        )
    }) {
        return Ok(JournalAppendStatus::Duplicate);
    }
    append_journal_line(
        paths,
        &serde_json::to_string(marker).map_err(internal_error)?,
    )?;
    Ok(JournalAppendStatus::Appended)
}

fn append_journal_line(paths: &TradeLedgerPaths, line: &str) -> Result<(), (StatusCode, String)> {
    fs::create_dir_all(&paths.journal_dir).map_err(internal_error)?;
    let path = active_journal_segment_path(&paths.journal_dir).map_err(internal_error)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(internal_error)?;
    writeln!(file, "{line}").map_err(internal_error)?;
    file.flush().map_err(internal_error)?;
    Ok(())
}

pub fn reset_trade_ledger_position(
    ledger: &mut HashMap<String, TradeLedgerEntry>,
    wallet_key: &str,
    mint: &str,
    reset_at_unix_ms: u64,
    reset_at_slot: Option<u64>,
) {
    let entry = ledger
        .entry(trade_ledger_key(wallet_key, mint))
        .or_insert_with(|| TradeLedgerEntry {
            wallet_key: wallet_key.to_string(),
            mint: mint.to_string(),
            ..TradeLedgerEntry::default()
        });
    entry.tracked_bought_lamports = 0;
    entry.tracked_sold_lamports = 0;
    entry.buy_count = 0;
    entry.sell_count = 0;
    entry.last_trade_at_unix_ms = reset_at_unix_ms;
    entry.entry_preference = None;
    entry.position_open = false;
    entry.realized_pnl_gross_lamports = 0;
    entry.realized_pnl_net_lamports = 0;
    entry.explicit_fee_total_lamports = 0;
    entry.remaining_cost_basis_lamports = 0;
    entry.unmatched_sell_amount_raw = 0;
    entry.needs_resync = false;
    entry.reset_baseline_unix_ms = reset_at_unix_ms;
    entry.reset_baseline_slot = reset_at_slot;
    entry.open_lots.clear();
    entry.platform_tags.clear();
}

/// Close any remaining open lots at zero proceeds, realising the outstanding
/// cost basis as a loss. Invoked when auto-reconciliation confirms the chain
/// holds none of this mint while the ledger still has unclosed lots — the
/// missing sells could not be recovered from RPC (burn, transfer-then-swap,
/// out-of-window signatures) so we converge with on-chain truth by treating
/// the residual basis as unrecoverable.
///
/// Invariants after the call:
/// - `open_lots` is empty, `position_open` is `false`, `remaining_cost_basis = 0`.
/// - `realized_pnl_gross_lamports` decreases by the previous remaining basis.
/// - `sell_count` bumps by one (the synthetic close counts as a single event).
/// - `tracked_sold_lamports` is unchanged (no proceeds were received).
/// - `unmatched_sell_amount_raw` and `needs_resync` are cleared; drift is
///   resolved from this ledger's perspective.
pub fn force_close_trade_ledger_position(
    ledger: &mut HashMap<String, TradeLedgerEntry>,
    wallet_key: &str,
    mint: &str,
    applied_at_unix_ms: u64,
) {
    let Some(entry) = ledger.get_mut(&trade_ledger_key(wallet_key, mint)) else {
        return;
    };
    let realized_loss: u64 = entry.open_lots.iter().fold(0u64, |sum, lot| {
        sum.saturating_add(lot.remaining_cost_basis_lamports)
    });
    let had_open_lots = !entry.open_lots.is_empty();
    entry.open_lots.clear();
    entry.remaining_cost_basis_lamports = 0;
    entry.position_open = false;
    entry.entry_preference = None;
    entry.unmatched_sell_amount_raw = 0;
    entry.needs_resync = false;
    if !had_open_lots {
        return;
    }
    entry.sell_count = entry.sell_count.saturating_add(1);
    entry.last_trade_at_unix_ms = entry.last_trade_at_unix_ms.max(applied_at_unix_ms);
    entry.realized_pnl_gross_lamports = entry
        .realized_pnl_gross_lamports
        .saturating_sub(i64::try_from(realized_loss).unwrap_or(i64::MAX));
    entry.realized_pnl_net_lamports = entry
        .realized_pnl_gross_lamports
        .saturating_sub(entry.explicit_fee_total_lamports);
}

pub fn transfer_trade_ledger_position(
    ledger: &mut HashMap<String, TradeLedgerEntry>,
    source_wallet_key: &str,
    destination_wallet_key: &str,
    mint: &str,
    amount_raw: u64,
    signature: &str,
    applied_at_unix_ms: u64,
) -> u64 {
    if amount_raw == 0 || source_wallet_key == destination_wallet_key {
        return 0;
    }
    let source_key = trade_ledger_key(source_wallet_key, mint);
    let destination_key = trade_ledger_key(destination_wallet_key, mint);
    let Some(source) = ledger.get_mut(&source_key) else {
        return 0;
    };
    let mut remaining_to_move = amount_raw;
    let mut moved_lots = Vec::new();
    let mut moved_total = 0u64;
    for lot in &mut source.open_lots {
        if remaining_to_move == 0 || lot.remaining_amount_raw == 0 {
            continue;
        }
        let moved_amount = remaining_to_move.min(lot.remaining_amount_raw);
        let moved_cost = proportional_amount(
            lot.remaining_cost_basis_lamports,
            moved_amount,
            lot.remaining_amount_raw,
        );
        lot.remaining_amount_raw = lot.remaining_amount_raw.saturating_sub(moved_amount);
        lot.remaining_cost_basis_lamports =
            lot.remaining_cost_basis_lamports.saturating_sub(moved_cost);
        moved_lots.push(OpenLot {
            acquired_at_unix_ms: lot.acquired_at_unix_ms,
            signature: signature.to_string(),
            settlement_asset: lot.settlement_asset,
            acquired_amount_raw: moved_amount,
            remaining_amount_raw: moved_amount,
            remaining_cost_basis_lamports: moved_cost,
        });
        moved_total = moved_total.saturating_add(moved_amount);
        remaining_to_move = remaining_to_move.saturating_sub(moved_amount);
    }
    if moved_lots.is_empty() {
        return 0;
    }

    source.open_lots.retain(|lot| lot.remaining_amount_raw > 0);
    source.position_open = source
        .open_lots
        .iter()
        .any(|lot| lot.remaining_amount_raw > 0);
    source.remaining_cost_basis_lamports = source.open_lots.iter().fold(0u64, |sum, lot| {
        sum.saturating_add(lot.remaining_cost_basis_lamports)
    });
    source.last_trade_at_unix_ms = source.last_trade_at_unix_ms.max(applied_at_unix_ms);
    if !source.position_open {
        source.entry_preference = None;
    }
    let source_tags = source.platform_tags.clone();
    let source_preference = source.entry_preference;

    let destination = ledger
        .entry(destination_key)
        .or_insert_with(|| TradeLedgerEntry {
            wallet_key: destination_wallet_key.to_string(),
            mint: mint.to_string(),
            ..TradeLedgerEntry::default()
        });
    destination.open_lots.extend(moved_lots);
    destination.position_open = destination
        .open_lots
        .iter()
        .any(|lot| lot.remaining_amount_raw > 0);
    destination.remaining_cost_basis_lamports =
        destination.open_lots.iter().fold(0u64, |sum, lot| {
            sum.saturating_add(lot.remaining_cost_basis_lamports)
        });
    destination.last_trade_at_unix_ms = destination.last_trade_at_unix_ms.max(applied_at_unix_ms);
    if destination.entry_preference.is_none() {
        destination.entry_preference = source_preference;
    } else if source_preference.is_some() && destination.entry_preference != source_preference {
        destination.entry_preference = Some(StoredEntryPreference::Mixed);
    }
    for tag in source_tags {
        if !destination.platform_tags.contains(&tag) {
            destination.platform_tags.push(tag);
        }
    }
    moved_total
}

pub fn mark_trade_ledger_received_without_cost_basis(
    ledger: &mut HashMap<String, TradeLedgerEntry>,
    wallet_key: &str,
    mint: &str,
    _amount_raw: u64,
    _signature: &str,
    applied_at_unix_ms: u64,
) {
    let entry = ledger
        .entry(trade_ledger_key(wallet_key, mint))
        .or_insert_with(|| TradeLedgerEntry {
            wallet_key: wallet_key.to_string(),
            mint: mint.to_string(),
            ..TradeLedgerEntry::default()
        });
    entry.last_trade_at_unix_ms = entry.last_trade_at_unix_ms.max(applied_at_unix_ms);
    entry.needs_resync = true;
}

pub fn mark_trade_ledger_sent_without_proceeds(
    ledger: &mut HashMap<String, TradeLedgerEntry>,
    wallet_key: &str,
    mint: &str,
    amount_raw: u64,
    _signature: &str,
    applied_at_unix_ms: u64,
) {
    let key = trade_ledger_key(wallet_key, mint);
    let entry = ledger.entry(key).or_insert_with(|| TradeLedgerEntry {
        wallet_key: wallet_key.to_string(),
        mint: mint.to_string(),
        ..TradeLedgerEntry::default()
    });
    entry.last_trade_at_unix_ms = entry.last_trade_at_unix_ms.max(applied_at_unix_ms);
    let mut remaining_to_send = amount_raw;
    for lot in &mut entry.open_lots {
        if remaining_to_send == 0 || lot.remaining_amount_raw == 0 {
            continue;
        }
        let sent_amount = remaining_to_send.min(lot.remaining_amount_raw);
        let sent_cost = proportional_amount(
            lot.remaining_cost_basis_lamports,
            sent_amount,
            lot.remaining_amount_raw,
        );
        lot.remaining_amount_raw = lot.remaining_amount_raw.saturating_sub(sent_amount);
        lot.remaining_cost_basis_lamports =
            lot.remaining_cost_basis_lamports.saturating_sub(sent_cost);
        remaining_to_send = remaining_to_send.saturating_sub(sent_amount);
    }
    entry.open_lots.retain(|lot| lot.remaining_amount_raw > 0);
    entry.position_open = entry
        .open_lots
        .iter()
        .any(|lot| lot.remaining_amount_raw > 0);
    entry.remaining_cost_basis_lamports = entry.open_lots.iter().fold(0u64, |sum, lot| {
        sum.saturating_add(lot.remaining_cost_basis_lamports)
    });
    if !entry.position_open {
        entry.entry_preference = None;
    }
    if remaining_to_send > 0 {
        entry.needs_resync = true;
    }
}

pub fn stored_entry_preference_from_asset(asset: TradeSettlementAsset) -> StoredEntryPreference {
    match asset {
        TradeSettlementAsset::Sol => StoredEntryPreference::Sol,
        TradeSettlementAsset::Usd1 => StoredEntryPreference::Usd1,
    }
}

pub fn aggregate_trade_ledger(
    ledger: &HashMap<String, TradeLedgerEntry>,
    wallet_keys: &[String],
    mint: &str,
) -> TradeLedgerAggregate {
    wallet_keys.iter().fold(
        TradeLedgerAggregate::default(),
        |mut aggregate, wallet_key| {
            if let Some(entry) = ledger.get(&trade_ledger_key(wallet_key, mint)) {
                aggregate.tracked_bought_lamports = aggregate
                    .tracked_bought_lamports
                    .saturating_add(entry.tracked_bought_lamports);
                aggregate.tracked_sold_lamports = aggregate
                    .tracked_sold_lamports
                    .saturating_add(entry.tracked_sold_lamports);
                aggregate.buy_count = aggregate.buy_count.saturating_add(entry.buy_count);
                aggregate.sell_count = aggregate.sell_count.saturating_add(entry.sell_count);
                aggregate.last_trade_at_unix_ms = aggregate
                    .last_trade_at_unix_ms
                    .max(entry.last_trade_at_unix_ms);
                aggregate.realized_pnl_gross_lamports = aggregate
                    .realized_pnl_gross_lamports
                    .saturating_add(entry.realized_pnl_gross_lamports);
                aggregate.realized_pnl_net_lamports = aggregate
                    .realized_pnl_net_lamports
                    .saturating_add(entry.realized_pnl_net_lamports);
                aggregate.explicit_fee_total_lamports = aggregate
                    .explicit_fee_total_lamports
                    .saturating_add(entry.explicit_fee_total_lamports);
                aggregate.remaining_cost_basis_lamports = aggregate
                    .remaining_cost_basis_lamports
                    .saturating_add(entry.remaining_cost_basis_lamports);
                aggregate.unmatched_sell_amount_raw = aggregate
                    .unmatched_sell_amount_raw
                    .saturating_add(entry.unmatched_sell_amount_raw);
                aggregate.needs_resync |= entry.needs_resync;
            }
            aggregate
        },
    )
}

pub fn platform_tag_from_label(label: Option<&str>) -> PlatformTag {
    match label
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "axiom" => PlatformTag::Axiom,
        "j7" | "j7tracker" => PlatformTag::J7,
        _ => PlatformTag::Unknown,
    }
}

fn merge_entries_from_file(
    path: &PathBuf,
    entries: &mut HashMap<String, TradeLedgerEntry>,
) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(parsed_entries) = serde_json::from_str::<Vec<TradeLedgerEntry>>(&contents) else {
        return false;
    };
    for entry in parsed_entries {
        entries.insert(trade_ledger_key(&entry.wallet_key, &entry.mint), entry);
    }
    true
}

fn merge_entries_from_snapshot_file(
    path: &PathBuf,
    entries: &mut HashMap<String, TradeLedgerEntry>,
) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(parsed_records) = serde_json::from_str::<Vec<PnlSnapshotRecord>>(&contents) else {
        return false;
    };
    for record in parsed_records {
        let entry = trade_ledger_entry_from_snapshot_record(record);
        entries.insert(trade_ledger_key(&entry.wallet_key, &entry.mint), entry);
    }
    true
}

fn load_legacy_trade_ledger(paths: &TradeLedgerPaths) -> Option<HashMap<String, TradeLedgerEntry>> {
    let legacy_path = paths
        .root_dir
        .parent()
        .unwrap_or(paths.root_dir.as_path())
        .join(LEGACY_TRADE_LEDGER_FILE);
    let contents = fs::read_to_string(legacy_path).ok()?;
    let parsed_entries = serde_json::from_str::<Vec<TradeLedgerEntry>>(&contents).ok()?;
    Some(
        parsed_entries
            .into_iter()
            .map(|entry| (trade_ledger_key(&entry.wallet_key, &entry.mint), entry))
            .collect(),
    )
}

fn rebuild_trade_ledger_from_journal(
    paths: &TradeLedgerPaths,
) -> HashMap<String, TradeLedgerEntry> {
    let mut ledger = HashMap::new();
    let mut seen_event_ids = HashSet::new();
    let mut seen_token_transfer_ids = HashSet::new();
    let mut latest_incomplete_markers: HashMap<String, IncompleteBalanceAdjustmentMarkerEvent> =
        HashMap::new();
    let mut entries = Vec::new();
    for entry in read_journal_entries(paths) {
        match entry {
            JournalEntry::IncompleteBalanceAdjustmentMarker(marker) => {
                if marker.amount_raw == 0 {
                    let clear_scope = incomplete_marker_clear_scope(&marker);
                    latest_incomplete_markers.retain(|_, existing| {
                        incomplete_marker_clear_scope(existing) != clear_scope
                    });
                    latest_incomplete_markers.insert(marker.event_id(), marker);
                    continue;
                }
                let event_id = marker.event_id();
                let should_replace =
                    latest_incomplete_markers
                        .get(&event_id)
                        .map_or(true, |existing| {
                            incomplete_marker_order_key(&marker)
                                >= incomplete_marker_order_key(existing)
                        });
                if should_replace {
                    latest_incomplete_markers.insert(event_id, marker);
                }
            }
            other => entries.push(other),
        }
    }
    entries.extend(
        latest_incomplete_markers
            .into_values()
            .map(JournalEntry::IncompleteBalanceAdjustmentMarker),
    );
    // Keep rebuild ordering aligned with live resync replay so snapshots and
    // journal-only recovery converge to the same FIFO lots.
    entries.sort_by(compare_journal_entries);
    for entry in entries {
        match entry {
            JournalEntry::Trade(event) => {
                if !seen_event_ids.insert(event.event_id()) {
                    continue;
                }
                let entry_preference = match event.side {
                    TradeSide::Buy => event.settlement_asset,
                    TradeSide::Sell => None,
                };
                apply_confirmed_trade_event(&mut ledger, &event, entry_preference);
            }
            JournalEntry::ResetMarker(marker) => {
                reset_trade_ledger_position(
                    &mut ledger,
                    &marker.wallet_key,
                    &marker.mint,
                    marker.reset_at_unix_ms,
                    marker.reset_at_slot,
                );
            }
            JournalEntry::ForceCloseMarker(marker) => {
                force_close_trade_ledger_position(
                    &mut ledger,
                    &marker.wallet_key,
                    &marker.mint,
                    marker.applied_at_unix_ms,
                );
            }
            JournalEntry::TokenTransferMarker(marker) => {
                if !seen_token_transfer_ids.insert(marker.event_id()) {
                    continue;
                }
                transfer_trade_ledger_position(
                    &mut ledger,
                    &marker.source_wallet_key,
                    &marker.destination_wallet_key,
                    &marker.mint,
                    marker.amount_raw,
                    &marker.signature,
                    marker.applied_at_unix_ms,
                );
            }
            JournalEntry::IncompleteBalanceAdjustmentMarker(marker) => {
                if marker.amount_raw == 0 {
                    continue;
                }
                match marker.adjustment_kind {
                    IncompleteBalanceAdjustmentKind::ReceivedWithoutCostBasis => {
                        mark_trade_ledger_received_without_cost_basis(
                            &mut ledger,
                            &marker.wallet_key,
                            &marker.mint,
                            marker.amount_raw,
                            &marker.signature,
                            marker.applied_at_unix_ms,
                        );
                    }
                    IncompleteBalanceAdjustmentKind::SentWithoutProceeds => {
                        mark_trade_ledger_sent_without_proceeds(
                            &mut ledger,
                            &marker.wallet_key,
                            &marker.mint,
                            marker.amount_raw,
                            &marker.signature,
                            marker.applied_at_unix_ms,
                        );
                    }
                }
            }
        }
    }
    ledger
}

fn apply_confirmed_trade_event(
    ledger: &mut HashMap<String, TradeLedgerEntry>,
    event: &ConfirmedTradeEvent,
    entry_preference_asset: Option<TradeSettlementAsset>,
) {
    let entry = ledger
        .entry(trade_ledger_key(&event.wallet_key, &event.mint))
        .or_insert_with(|| TradeLedgerEntry {
            wallet_key: event.wallet_key.clone(),
            mint: event.mint.clone(),
            ..TradeLedgerEntry::default()
        });

    if !trade_event_is_after_reset_baseline(
        event.confirmed_at_unix_ms,
        event.slot,
        entry.reset_baseline_unix_ms,
        entry.reset_baseline_slot,
    ) {
        return;
    }

    entry.last_trade_at_unix_ms = entry.last_trade_at_unix_ms.max(event.confirmed_at_unix_ms);
    entry.explicit_fee_total_lamports = entry
        .explicit_fee_total_lamports
        .saturating_add(event.explicit_fees.total_lamports());
    if !entry.platform_tags.contains(&event.platform_tag) {
        entry.platform_tags.push(event.platform_tag);
    }

    match event.side {
        TradeSide::Buy => apply_buy_event(entry, event, entry_preference_asset),
        TradeSide::Sell => apply_sell_event(entry, event),
    }

    entry.position_open = entry
        .open_lots
        .iter()
        .any(|lot| lot.remaining_amount_raw > 0);
    entry.remaining_cost_basis_lamports = entry.open_lots.iter().fold(0u64, |sum, lot| {
        sum.saturating_add(lot.remaining_cost_basis_lamports)
    });
    entry.realized_pnl_net_lamports = entry
        .realized_pnl_gross_lamports
        .saturating_sub(entry.explicit_fee_total_lamports);
    if !entry.position_open {
        entry.entry_preference = None;
        entry.open_lots.clear();
        entry.remaining_cost_basis_lamports = 0;
    }
}

fn apply_buy_event(
    entry: &mut TradeLedgerEntry,
    event: &ConfirmedTradeEvent,
    entry_preference_asset: Option<TradeSettlementAsset>,
) {
    entry.tracked_bought_lamports = entry
        .tracked_bought_lamports
        .saturating_add(event.trade_value_lamports);
    entry.buy_count = entry.buy_count.saturating_add(1);

    if let Some(preference) = entry_preference_asset.map(stored_entry_preference_from_asset) {
        entry.entry_preference = Some(match entry.entry_preference {
            Some(StoredEntryPreference::Mixed) => StoredEntryPreference::Mixed,
            Some(existing) if existing != preference => StoredEntryPreference::Mixed,
            _ => preference,
        });
    }

    let amount_raw = if event.token_delta_raw > 0 {
        u64::try_from(event.token_delta_raw).unwrap_or(u64::MAX)
    } else {
        0
    };
    if amount_raw > 0 {
        entry.open_lots.push(OpenLot {
            acquired_at_unix_ms: event.confirmed_at_unix_ms,
            signature: event.signature.clone(),
            settlement_asset: event.settlement_asset,
            acquired_amount_raw: amount_raw,
            remaining_amount_raw: amount_raw,
            remaining_cost_basis_lamports: event.trade_value_lamports,
        });
    }
}

fn apply_sell_event(entry: &mut TradeLedgerEntry, event: &ConfirmedTradeEvent) {
    entry.tracked_sold_lamports = entry
        .tracked_sold_lamports
        .saturating_add(event.trade_value_lamports);
    entry.sell_count = entry.sell_count.saturating_add(1);

    let mut remaining_to_match = if event.token_delta_raw < 0 {
        u64::try_from(-event.token_delta_raw).unwrap_or(u64::MAX)
    } else {
        0
    };
    let mut matched_cost_basis = 0u64;

    for lot in &mut entry.open_lots {
        if remaining_to_match == 0 || lot.remaining_amount_raw == 0 {
            continue;
        }
        let matched_amount = remaining_to_match.min(lot.remaining_amount_raw);
        let matched_cost = proportional_amount(
            lot.remaining_cost_basis_lamports,
            matched_amount,
            lot.remaining_amount_raw,
        );
        lot.remaining_amount_raw = lot.remaining_amount_raw.saturating_sub(matched_amount);
        lot.remaining_cost_basis_lamports = lot
            .remaining_cost_basis_lamports
            .saturating_sub(matched_cost);
        matched_cost_basis = matched_cost_basis.saturating_add(matched_cost);
        remaining_to_match = remaining_to_match.saturating_sub(matched_amount);
    }

    entry.open_lots.retain(|lot| lot.remaining_amount_raw > 0);
    if remaining_to_match > 0 {
        entry.unmatched_sell_amount_raw = entry
            .unmatched_sell_amount_raw
            .saturating_add(remaining_to_match);
        entry.needs_resync = true;
    }
    entry.realized_pnl_gross_lamports = entry.realized_pnl_gross_lamports.saturating_add(
        i64::try_from(event.trade_value_lamports)
            .unwrap_or(i64::MAX)
            .saturating_sub(i64::try_from(matched_cost_basis).unwrap_or(i64::MAX)),
    );
}

fn snapshot_record_from_entry(entry: &TradeLedgerEntry) -> PnlSnapshotRecord {
    PnlSnapshotRecord {
        wallet_key: entry.wallet_key.clone(),
        mint: entry.mint.clone(),
        tracked_bought_lamports: entry.tracked_bought_lamports,
        tracked_sold_lamports: entry.tracked_sold_lamports,
        buy_count: entry.buy_count,
        sell_count: entry.sell_count,
        last_trade_at_unix_ms: entry.last_trade_at_unix_ms,
        entry_preference: entry.entry_preference,
        position_open: entry.position_open,
        realized_pnl_gross_lamports: entry.realized_pnl_gross_lamports,
        realized_pnl_net_lamports: entry.realized_pnl_net_lamports,
        explicit_fee_total_lamports: entry.explicit_fee_total_lamports,
        remaining_cost_basis_lamports: entry.remaining_cost_basis_lamports,
        unmatched_sell_amount_raw: entry.unmatched_sell_amount_raw,
        needs_resync: entry.needs_resync,
        reset_baseline_unix_ms: entry.reset_baseline_unix_ms,
        reset_baseline_slot: entry.reset_baseline_slot,
        open_lots: entry.open_lots.clone(),
        platform_tags: entry.platform_tags.clone(),
    }
}

fn trade_ledger_entry_from_snapshot_record(record: PnlSnapshotRecord) -> TradeLedgerEntry {
    TradeLedgerEntry {
        wallet_key: record.wallet_key,
        mint: record.mint,
        tracked_bought_lamports: record.tracked_bought_lamports,
        tracked_sold_lamports: record.tracked_sold_lamports,
        buy_count: record.buy_count,
        sell_count: record.sell_count,
        last_trade_at_unix_ms: record.last_trade_at_unix_ms,
        entry_preference: record.entry_preference,
        position_open: record.position_open,
        realized_pnl_gross_lamports: record.realized_pnl_gross_lamports,
        realized_pnl_net_lamports: record.realized_pnl_net_lamports,
        explicit_fee_total_lamports: record.explicit_fee_total_lamports,
        remaining_cost_basis_lamports: record.remaining_cost_basis_lamports,
        unmatched_sell_amount_raw: record.unmatched_sell_amount_raw,
        needs_resync: record.needs_resync,
        reset_baseline_unix_ms: record.reset_baseline_unix_ms,
        reset_baseline_slot: record.reset_baseline_slot,
        open_lots: record.open_lots,
        platform_tags: record.platform_tags,
    }
}

pub fn trade_event_is_after_reset_baseline(
    confirmed_at_unix_ms: u64,
    slot: Option<u64>,
    reset_baseline_unix_ms: u64,
    reset_baseline_slot: Option<u64>,
) -> bool {
    if let (Some(event_slot), Some(reset_slot)) = (slot, reset_baseline_slot) {
        return event_slot > reset_slot;
    }
    confirmed_at_unix_ms > reset_baseline_unix_ms
}

fn journal_segment_paths(journal_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(journal_dir) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| {
                    name.starts_with(PNL_JOURNAL_PREFIX) && name.ends_with(PNL_JOURNAL_SUFFIX)
                })
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn active_journal_segment_path(journal_dir: &Path) -> Result<PathBuf, std::io::Error> {
    let segments = journal_segment_paths(journal_dir);
    if let Some(last) = segments.last() {
        let metadata = fs::metadata(last)?;
        if metadata.len() < PNL_JOURNAL_SEGMENT_MAX_BYTES {
            return Ok(last.clone());
        }
    }
    let next_index = next_journal_segment_index(&segments);
    Ok(journal_dir.join(format!(
        "{PNL_JOURNAL_PREFIX}-{next_index:05}{PNL_JOURNAL_SUFFIX}"
    )))
}

fn next_journal_segment_index(segments: &[PathBuf]) -> usize {
    segments
        .iter()
        .filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| {
                    name.strip_prefix(PNL_JOURNAL_PREFIX)?
                        .strip_prefix('-')?
                        .strip_suffix(PNL_JOURNAL_SUFFIX)?
                        .parse::<usize>()
                        .ok()
                })
        })
        .max()
        .map(|max| max.saturating_add(1))
        .unwrap_or(1)
}

fn atomic_write_json<T: Serialize>(path: &PathBuf, value: &T) -> Result<(), (StatusCode, String)> {
    let serialized = serde_json::to_string_pretty(value).map_err(internal_error)?;
    let parent = path
        .parent()
        .ok_or_else(|| internal_error("trade ledger parent directory missing"))?;
    fs::create_dir_all(parent).map_err(internal_error)?;
    let tmp_path = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("trade-ledger"),
        now_unix_ms()
    ));
    fs::write(&tmp_path, serialized).map_err(internal_error)?;
    fs::rename(&tmp_path, path).map_err(internal_error)?;
    Ok(())
}

fn proportional_amount(total: u64, part: u64, whole: u64) -> u64 {
    if total == 0 || part == 0 || whole == 0 {
        return 0;
    }
    if part >= whole {
        return total;
    }
    (((total as u128) * (part as u128)) / (whole as u128)).min(u128::from(u64::MAX)) as u64
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn trade_ledger_key(wallet_key: &str, mint: &str) -> String {
    format!("{}::{}", wallet_key.trim(), mint.trim())
}

fn internal_error(error: impl ToString) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buy_event<'a>(
        wallet_key: &'a str,
        mint: &'a str,
        amount_raw: i128,
        value_lamports: u64,
        preference: Option<TradeSettlementAsset>,
    ) -> RecordConfirmedTradeParams<'a> {
        RecordConfirmedTradeParams {
            wallet_key,
            wallet_public_key: wallet_key,
            mint,
            side: TradeSide::Buy,
            trade_value_lamports: value_lamports,
            token_delta_raw: amount_raw,
            token_decimals: Some(6),
            confirmed_at_unix_ms: 1,
            slot: Some(1),
            entry_preference_asset: preference,
            settlement_asset: preference,
            explicit_fees: ExplicitFeeBreakdown::default(),
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            signature: "sig-buy",
            client_request_id: None,
            batch_id: None,
        }
    }

    #[test]
    fn buy_entry_preference_becomes_mixed_when_funding_assets_change() {
        let mut ledger = HashMap::new();
        record_confirmed_trade(
            &mut ledger,
            buy_event(
                "wallet-a",
                "mint-a",
                100,
                100,
                Some(TradeSettlementAsset::Sol),
            ),
        );
        let mut second = buy_event(
            "wallet-a",
            "mint-a",
            50,
            50,
            Some(TradeSettlementAsset::Usd1),
        );
        second.signature = "sig-buy-2";
        second.confirmed_at_unix_ms = 2;
        record_confirmed_trade(&mut ledger, second);

        let entry = ledger.get("wallet-a::mint-a").expect("trade ledger entry");
        assert_eq!(entry.entry_preference, Some(StoredEntryPreference::Mixed));
    }

    #[test]
    fn sell_entry_preference_clears_after_full_exit() {
        let mut ledger = HashMap::new();
        record_confirmed_trade(
            &mut ledger,
            buy_event(
                "wallet-a",
                "mint-a",
                100,
                100,
                Some(TradeSettlementAsset::Usd1),
            ),
        );
        record_confirmed_trade(
            &mut ledger,
            RecordConfirmedTradeParams {
                wallet_key: "wallet-a",
                wallet_public_key: "wallet-a",
                mint: "mint-a",
                side: TradeSide::Sell,
                trade_value_lamports: 100,
                token_delta_raw: -100,
                token_decimals: Some(6),
                confirmed_at_unix_ms: 2,
                slot: Some(2),
                entry_preference_asset: None,
                settlement_asset: Some(TradeSettlementAsset::Usd1),
                explicit_fees: ExplicitFeeBreakdown::default(),
                platform_tag: PlatformTag::Axiom,
                provenance: EventProvenance::LocalExecution,
                signature: "sig-sell",
                client_request_id: None,
                batch_id: None,
            },
        );

        let entry = ledger.get("wallet-a::mint-a").expect("trade ledger entry");
        assert_eq!(entry.entry_preference, None);
        assert!(!entry.position_open);
    }

    #[test]
    fn sell_keeps_entry_preference_when_position_stays_open() {
        let mut ledger = HashMap::new();
        record_confirmed_trade(
            &mut ledger,
            buy_event(
                "wallet-a",
                "mint-a",
                100,
                100,
                Some(TradeSettlementAsset::Usd1),
            ),
        );
        record_confirmed_trade(
            &mut ledger,
            RecordConfirmedTradeParams {
                wallet_key: "wallet-a",
                wallet_public_key: "wallet-a",
                mint: "mint-a",
                side: TradeSide::Sell,
                trade_value_lamports: 60,
                token_delta_raw: -40,
                token_decimals: Some(6),
                confirmed_at_unix_ms: 2,
                slot: Some(2),
                entry_preference_asset: None,
                settlement_asset: Some(TradeSettlementAsset::Usd1),
                explicit_fees: ExplicitFeeBreakdown::default(),
                platform_tag: PlatformTag::Axiom,
                provenance: EventProvenance::LocalExecution,
                signature: "sig-sell",
                client_request_id: None,
                batch_id: None,
            },
        );

        let entry = ledger.get("wallet-a::mint-a").expect("trade ledger entry");
        assert_eq!(entry.entry_preference, Some(StoredEntryPreference::Usd1));
        assert!(entry.position_open);
    }

    #[test]
    fn fifo_realized_pnl_uses_remaining_lot_cost_basis() {
        let mut ledger = HashMap::new();
        record_confirmed_trade(
            &mut ledger,
            buy_event(
                "wallet-a",
                "mint-a",
                100,
                100,
                Some(TradeSettlementAsset::Sol),
            ),
        );
        let mut second_buy = buy_event(
            "wallet-a",
            "mint-a",
            100,
            200,
            Some(TradeSettlementAsset::Sol),
        );
        second_buy.signature = "sig-buy-2";
        second_buy.confirmed_at_unix_ms = 2;
        record_confirmed_trade(&mut ledger, second_buy);
        record_confirmed_trade(
            &mut ledger,
            RecordConfirmedTradeParams {
                wallet_key: "wallet-a",
                wallet_public_key: "wallet-a",
                mint: "mint-a",
                side: TradeSide::Sell,
                trade_value_lamports: 225,
                token_delta_raw: -150,
                token_decimals: Some(6),
                confirmed_at_unix_ms: 3,
                slot: Some(3),
                entry_preference_asset: None,
                settlement_asset: Some(TradeSettlementAsset::Sol),
                explicit_fees: ExplicitFeeBreakdown::default(),
                platform_tag: PlatformTag::Axiom,
                provenance: EventProvenance::LocalExecution,
                signature: "sig-sell",
                client_request_id: None,
                batch_id: None,
            },
        );

        let entry = ledger.get("wallet-a::mint-a").expect("trade ledger entry");
        assert_eq!(entry.realized_pnl_gross_lamports, 25);
        assert_eq!(entry.remaining_cost_basis_lamports, 100);
        assert_eq!(entry.open_lots.len(), 1);
        assert_eq!(entry.open_lots[0].remaining_amount_raw, 50);
    }

    #[test]
    fn token_transfer_moves_open_lot_cost_basis_without_buy_sell_counts() {
        let mut ledger = HashMap::new();
        record_confirmed_trade(
            &mut ledger,
            buy_event(
                "wallet-a",
                "mint-a",
                100,
                1_000,
                Some(TradeSettlementAsset::Sol),
            ),
        );
        let moved = transfer_trade_ledger_position(
            &mut ledger,
            "wallet-a",
            "wallet-b",
            "mint-a",
            40,
            "sig-transfer",
            10,
        );
        assert_eq!(moved, 40);

        let source = ledger.get("wallet-a::mint-a").expect("source entry");
        let destination = ledger.get("wallet-b::mint-a").expect("destination entry");
        assert_eq!(source.buy_count, 1);
        assert_eq!(source.sell_count, 0);
        assert_eq!(destination.buy_count, 0);
        assert_eq!(destination.sell_count, 0);
        assert_eq!(source.remaining_cost_basis_lamports, 600);
        assert_eq!(source.open_lots[0].remaining_amount_raw, 60);
        assert_eq!(destination.remaining_cost_basis_lamports, 400);
        assert_eq!(destination.open_lots[0].remaining_amount_raw, 40);
        assert_eq!(destination.open_lots[0].signature, "sig-transfer");
    }

    #[test]
    fn token_transfer_reports_zero_when_no_cost_basis_can_move() {
        let mut ledger = HashMap::new();
        let moved = transfer_trade_ledger_position(
            &mut ledger,
            "wallet-a",
            "wallet-b",
            "mint-a",
            40,
            "sig-transfer",
            10,
        );

        assert_eq!(moved, 0);
        assert!(ledger.is_empty());
    }

    #[test]
    fn token_transfer_marker_round_trips_and_rebuilds() {
        let temp = TempDirGuard::new("transfer-marker");
        let paths = test_paths(&temp.path);
        let buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-buy".to_string(),
            slot: Some(1),
            confirmed_at_unix_ms: 100,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 1_000,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        append_confirmed_trade_event(&paths, &buy).expect("append buy");
        let marker = TokenTransferMarkerEvent::new(
            "wallet-a",
            "wallet-b",
            "mint-a",
            25,
            "sig-transfer",
            200,
        );
        append_token_transfer_marker(&paths, &marker).expect("append transfer marker");
        append_token_transfer_marker(&paths, &marker).expect("dedupe duplicate transfer marker");
        let entries = read_journal_entries(&paths);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[1], JournalEntry::TokenTransferMarker(_)));

        let rebuilt = rebuild_trade_ledger_from_journal(&paths);
        let source = rebuilt.get("wallet-a::mint-a").expect("source entry");
        let destination = rebuilt.get("wallet-b::mint-a").expect("destination entry");
        assert_eq!(source.remaining_cost_basis_lamports, 750);
        assert_eq!(destination.remaining_cost_basis_lamports, 250);
    }

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "trade-ledger-{}-{}-{}",
                tag,
                std::process::id(),
                now_unix_ms()
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn test_paths(root: &Path) -> TradeLedgerPaths {
        let journal_dir = root.join("pnl").join("journal");
        let snapshots_dir = root.join("pnl").join("snapshots");
        TradeLedgerPaths {
            root_dir: root.join("pnl"),
            journal_dir,
            open_positions_path: snapshots_dir.join(OPEN_POSITIONS_FILE),
            closed_positions_path: snapshots_dir.join(CLOSED_POSITIONS_FILE),
            snapshots_path: snapshots_dir.join(SNAPSHOTS_FILE),
        }
    }

    #[test]
    fn journal_rebuild_applies_reset_marker_and_drops_prior_trades() {
        let temp = TempDirGuard::new("rebuild-reset");
        let paths = test_paths(&temp.path);

        let pre_reset_buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-pre".to_string(),
            slot: Some(1),
            confirmed_at_unix_ms: 100,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        append_confirmed_trade_event(&paths, &pre_reset_buy).expect("append pre-reset buy");

        let marker = ResetMarkerEvent::new("wallet-a", "mint-a", 500, Some(5));
        append_reset_marker(&paths, &marker).expect("append reset marker");

        let post_reset_buy = ConfirmedTradeEvent {
            signature: "sig-post".to_string(),
            slot: Some(6),
            confirmed_at_unix_ms: 1_000,
            token_delta_raw: 50,
            trade_value_lamports: 200,
            ..pre_reset_buy.clone()
        };
        append_confirmed_trade_event(&paths, &post_reset_buy).expect("append post-reset buy");

        let rebuilt = rebuild_trade_ledger_from_journal(&paths);
        let entry = rebuilt
            .get("wallet-a::mint-a")
            .expect("rebuilt entry for wallet/mint");
        assert_eq!(entry.reset_baseline_unix_ms, 500);
        assert_eq!(entry.reset_baseline_slot, Some(5));
        assert_eq!(entry.buy_count, 1, "only the post-reset buy should count");
        assert_eq!(entry.tracked_bought_lamports, 200);
        assert_eq!(entry.open_lots.len(), 1);
        assert_eq!(entry.open_lots[0].signature, "sig-post");
    }

    #[test]
    fn load_trade_ledger_tolerates_missing_closed_positions_file() {
        let temp = TempDirGuard::new("load-partial");
        let paths = test_paths(&temp.path);
        fs::create_dir_all(&paths.journal_dir).expect("create journal dir");
        if let Some(parent) = paths.open_positions_path.parent() {
            fs::create_dir_all(parent).expect("create snapshots dir");
        }

        let entry = TradeLedgerEntry {
            wallet_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            tracked_bought_lamports: 1_000,
            buy_count: 1,
            position_open: true,
            reset_baseline_unix_ms: 42,
            ..TradeLedgerEntry::default()
        };
        atomic_write_json(&paths.open_positions_path, &vec![entry.clone()])
            .expect("write open snapshot");
        assert!(!paths.closed_positions_path.exists());

        let loaded = load_trade_ledger(&paths);
        let recovered = loaded
            .get("wallet-a::mint-a")
            .expect("recovered entry from partial snapshot");
        assert_eq!(recovered.reset_baseline_unix_ms, 42);
        assert_eq!(recovered.tracked_bought_lamports, 1_000);
    }

    #[test]
    fn load_trade_ledger_recovers_from_full_snapshot_when_split_files_are_missing() {
        let temp = TempDirGuard::new("load-full-snapshot");
        let paths = test_paths(&temp.path);
        if let Some(parent) = paths.snapshots_path.parent() {
            fs::create_dir_all(parent).expect("create snapshots dir");
        }
        let entry = TradeLedgerEntry {
            wallet_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            tracked_bought_lamports: 1_500,
            buy_count: 2,
            position_open: true,
            reset_baseline_unix_ms: 99,
            reset_baseline_slot: Some(44),
            open_lots: vec![OpenLot {
                acquired_at_unix_ms: 1_000,
                signature: "sig-buy".to_string(),
                settlement_asset: Some(TradeSettlementAsset::Sol),
                acquired_amount_raw: 150,
                remaining_amount_raw: 150,
                remaining_cost_basis_lamports: 1_500,
            }],
            ..TradeLedgerEntry::default()
        };
        atomic_write_json(
            &paths.snapshots_path,
            &vec![snapshot_record_from_entry(&entry)],
        )
        .expect("write full snapshot");

        let loaded = load_trade_ledger(&paths);
        let recovered = loaded
            .get("wallet-a::mint-a")
            .expect("recovered entry from full snapshot");
        assert_eq!(recovered.tracked_bought_lamports, 1_500);
        assert_eq!(recovered.reset_baseline_slot, Some(44));
        assert_eq!(recovered.open_lots.len(), 1);
        assert_eq!(recovered.open_lots[0].signature, "sig-buy");
    }

    #[test]
    fn load_trade_ledger_falls_back_to_legacy_trade_ledger_file() {
        let temp = TempDirGuard::new("load-legacy");
        let paths = test_paths(&temp.path);
        let entry = TradeLedgerEntry {
            wallet_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            tracked_bought_lamports: 777,
            buy_count: 1,
            position_open: true,
            ..TradeLedgerEntry::default()
        };
        let legacy_path = temp.path.join(LEGACY_TRADE_LEDGER_FILE);
        fs::write(
            legacy_path,
            serde_json::to_string_pretty(&vec![entry.clone()]).expect("serialize legacy ledger"),
        )
        .expect("write legacy ledger");

        let loaded = load_trade_ledger(&paths);
        let recovered = loaded
            .get("wallet-a::mint-a")
            .expect("recovered entry from legacy ledger");
        assert_eq!(recovered.tracked_bought_lamports, 777);
        assert!(recovered.position_open);
    }

    #[test]
    fn reset_marker_round_trips_through_journal_parser() {
        let temp = TempDirGuard::new("reset-roundtrip");
        let paths = test_paths(&temp.path);
        let marker = ResetMarkerEvent::new("wallet-a", "mint-a", 777, Some(88));
        append_reset_marker(&paths, &marker).expect("append reset marker");
        let entries = read_journal_entries(&paths);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            JournalEntry::ResetMarker(parsed) => {
                assert_eq!(parsed.wallet_key, "wallet-a");
                assert_eq!(parsed.mint, "mint-a");
                assert_eq!(parsed.reset_at_unix_ms, 777);
                assert_eq!(parsed.reset_at_slot, Some(88));
                assert_eq!(parsed.event_kind, JOURNAL_RESET_MARKER_KIND);
            }
            other => panic!("expected reset marker entry, got {other:?}"),
        }
        assert!(read_confirmed_trade_events(&paths).is_empty());
    }

    #[test]
    fn slot_based_reset_baseline_keeps_same_second_post_reset_trade() {
        let mut ledger = HashMap::new();
        reset_trade_ledger_position(&mut ledger, "wallet-a", "mint-a", 900, Some(10));

        let mut same_second_trade = buy_event(
            "wallet-a",
            "mint-a",
            100,
            100,
            Some(TradeSettlementAsset::Sol),
        );
        same_second_trade.confirmed_at_unix_ms = 0;
        same_second_trade.slot = Some(11);
        same_second_trade.signature = "sig-after-reset";
        record_confirmed_trade(&mut ledger, same_second_trade);

        let entry = ledger.get("wallet-a::mint-a").expect("entry after reset");
        assert_eq!(entry.buy_count, 1);
        assert_eq!(entry.tracked_bought_lamports, 100);
        assert_eq!(entry.reset_baseline_slot, Some(10));
    }

    #[test]
    fn active_journal_segment_path_picks_max_index_plus_one_after_gap() {
        let temp = TempDirGuard::new("journal-gap");
        let journal_dir = temp.path.join("journal");
        fs::create_dir_all(&journal_dir).expect("create journal dir");

        for index in [1usize, 2, 4] {
            let path = journal_dir.join(format!(
                "{PNL_JOURNAL_PREFIX}-{index:05}{PNL_JOURNAL_SUFFIX}"
            ));
            let body = "x".repeat(PNL_JOURNAL_SEGMENT_MAX_BYTES as usize);
            fs::write(&path, body).expect("write segment");
        }

        let next = active_journal_segment_path(&journal_dir).expect("next segment path");
        let name = next
            .file_name()
            .and_then(|value| value.to_str())
            .expect("file name");
        assert_eq!(name, "confirmed-trades-00005.jsonl");
    }

    #[test]
    fn force_close_realises_remaining_cost_basis_as_loss() {
        let mut ledger = HashMap::new();
        record_confirmed_trade(
            &mut ledger,
            buy_event(
                "wallet-a",
                "mint-a",
                100,
                120,
                Some(TradeSettlementAsset::Sol),
            ),
        );
        let mut partial_sell = RecordConfirmedTradeParams {
            wallet_key: "wallet-a",
            wallet_public_key: "wallet-a",
            mint: "mint-a",
            side: TradeSide::Sell,
            trade_value_lamports: 50,
            token_delta_raw: -40,
            token_decimals: Some(6),
            confirmed_at_unix_ms: 2,
            slot: Some(2),
            entry_preference_asset: None,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            explicit_fees: ExplicitFeeBreakdown::default(),
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            signature: "sig-sell-partial",
            client_request_id: None,
            batch_id: None,
        };
        partial_sell.signature = "sig-sell-partial";
        record_confirmed_trade(&mut ledger, partial_sell);

        let before = ledger
            .get("wallet-a::mint-a")
            .expect("entry before force-close")
            .clone();
        assert!(before.position_open);
        assert_eq!(before.sell_count, 1);
        let remaining_basis_before = before.remaining_cost_basis_lamports;
        assert!(remaining_basis_before > 0);
        let realized_before = before.realized_pnl_gross_lamports;

        force_close_trade_ledger_position(&mut ledger, "wallet-a", "mint-a", 9_999);

        let entry = ledger
            .get("wallet-a::mint-a")
            .expect("entry after force-close");
        assert!(!entry.position_open);
        assert!(entry.open_lots.is_empty());
        assert_eq!(entry.remaining_cost_basis_lamports, 0);
        assert_eq!(entry.unmatched_sell_amount_raw, 0);
        assert!(!entry.needs_resync);
        assert_eq!(entry.sell_count, 2);
        assert_eq!(entry.last_trade_at_unix_ms, 9_999);
        assert_eq!(
            entry.realized_pnl_gross_lamports,
            realized_before.saturating_sub(i64::try_from(remaining_basis_before).unwrap())
        );
        assert_eq!(
            entry.realized_pnl_net_lamports,
            entry
                .realized_pnl_gross_lamports
                .saturating_sub(entry.explicit_fee_total_lamports)
        );
        assert_eq!(entry.entry_preference, None);
    }

    #[test]
    fn force_close_is_a_no_op_when_no_open_lots() {
        let mut ledger = HashMap::new();
        record_confirmed_trade(
            &mut ledger,
            buy_event(
                "wallet-a",
                "mint-a",
                100,
                100,
                Some(TradeSettlementAsset::Sol),
            ),
        );
        record_confirmed_trade(
            &mut ledger,
            RecordConfirmedTradeParams {
                wallet_key: "wallet-a",
                wallet_public_key: "wallet-a",
                mint: "mint-a",
                side: TradeSide::Sell,
                trade_value_lamports: 150,
                token_delta_raw: -100,
                token_decimals: Some(6),
                confirmed_at_unix_ms: 2,
                slot: Some(2),
                entry_preference_asset: None,
                settlement_asset: Some(TradeSettlementAsset::Sol),
                explicit_fees: ExplicitFeeBreakdown::default(),
                platform_tag: PlatformTag::Axiom,
                provenance: EventProvenance::LocalExecution,
                signature: "sig-sell-all",
                client_request_id: None,
                batch_id: None,
            },
        );
        let snapshot = ledger.get("wallet-a::mint-a").expect("entry").clone();

        force_close_trade_ledger_position(&mut ledger, "wallet-a", "mint-a", 9_999);

        let entry = ledger.get("wallet-a::mint-a").expect("entry after noop");
        assert_eq!(entry.sell_count, snapshot.sell_count);
        assert_eq!(
            entry.realized_pnl_gross_lamports,
            snapshot.realized_pnl_gross_lamports
        );
        assert_eq!(entry.last_trade_at_unix_ms, snapshot.last_trade_at_unix_ms);
    }

    #[test]
    fn force_close_marker_round_trips_through_journal_parser() {
        let temp = TempDirGuard::new("force-close-roundtrip");
        let paths = test_paths(&temp.path);
        let marker =
            ForceCloseMarkerEvent::new("wallet-a", "mint-a", 555, "on-chain-zero-after-resync");
        append_force_close_marker(&paths, &marker).expect("append force-close marker");
        let entries = read_journal_entries(&paths);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            JournalEntry::ForceCloseMarker(parsed) => {
                assert_eq!(parsed.wallet_key, "wallet-a");
                assert_eq!(parsed.mint, "mint-a");
                assert_eq!(parsed.applied_at_unix_ms, 555);
                assert_eq!(parsed.reason, "on-chain-zero-after-resync");
                assert_eq!(parsed.event_kind, JOURNAL_FORCE_CLOSE_MARKER_KIND);
            }
            other => panic!("expected force-close marker entry, got {other:?}"),
        }
        assert!(read_confirmed_trade_events(&paths).is_empty());
    }

    #[test]
    fn incomplete_balance_adjustment_markers_round_trip_and_rebuild() {
        let temp = TempDirGuard::new("incomplete-adjustment-roundtrip");
        let paths = test_paths(&temp.path);
        let received = IncompleteBalanceAdjustmentMarkerEvent::received_without_cost_basis(
            "wallet-a",
            "mint-a",
            25,
            "sig-received",
            100,
            Some(10),
        );
        let sent = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-b",
            "mint-a",
            15,
            "sig-sent",
            200,
            Some(20),
        );
        append_incomplete_balance_adjustment_marker(&paths, &received)
            .expect("append received incomplete marker");
        append_incomplete_balance_adjustment_marker(&paths, &sent)
            .expect("append sent incomplete marker");

        let entries = read_journal_entries(&paths);
        assert_eq!(entries.len(), 2);
        assert!(matches!(
            entries[0],
            JournalEntry::IncompleteBalanceAdjustmentMarker(_)
        ));

        let rebuilt = rebuild_trade_ledger_from_journal(&paths);
        let received_entry = rebuilt.get("wallet-a::mint-a").expect("received entry");
        let sent_entry = rebuilt.get("wallet-b::mint-a").expect("sent entry");
        assert!(received_entry.needs_resync);
        assert!(sent_entry.needs_resync);
        assert_eq!(received_entry.last_trade_at_unix_ms, 100);
        assert_eq!(sent_entry.last_trade_at_unix_ms, 200);
    }

    #[test]
    fn reconciliation_markers_rebuild_with_latest_entry_only() {
        let temp = TempDirGuard::new("reconcile-marker-latest");
        let paths = test_paths(&temp.path);
        let buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-buy".to_string(),
            slot: Some(5),
            confirmed_at_unix_ms: 500,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        append_confirmed_trade_event(&paths, &buy).expect("append buy");
        let older = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            30,
            "resync-balance-reconcile:wallet-a:mint-a:1000",
            1_000,
            Some(10),
        );
        let newer = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            10,
            "resync-balance-reconcile:sent_without_proceeds:wallet-a:mint-a",
            2_000,
            Some(20),
        );
        append_incomplete_balance_adjustment_marker(&paths, &older).expect("append older marker");
        append_incomplete_balance_adjustment_marker(&paths, &newer).expect("append newer marker");

        let rebuilt = rebuild_trade_ledger_from_journal(&paths);
        let entry = rebuilt.get("wallet-a::mint-a").expect("rebuilt entry");
        assert_eq!(entry.open_lots[0].remaining_amount_raw, 90);
    }

    #[test]
    fn zero_reconciliation_marker_clears_prior_adjustment_on_rebuild() {
        let temp = TempDirGuard::new("reconcile-marker-clear");
        let paths = test_paths(&temp.path);
        let buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-buy".to_string(),
            slot: Some(5),
            confirmed_at_unix_ms: 500,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        append_confirmed_trade_event(&paths, &buy).expect("append buy");
        let stale = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            30,
            "resync-balance-reconcile:sent_without_proceeds:wallet-a:mint-a",
            1_000,
            Some(10),
        );
        let clear = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            0,
            "resync-balance-reconcile:sent_without_proceeds:wallet-a:mint-a",
            2_000,
            Some(20),
        );
        append_incomplete_balance_adjustment_marker(&paths, &stale).expect("append stale marker");
        append_incomplete_balance_adjustment_marker(&paths, &clear).expect("append clear marker");

        let rebuilt = rebuild_trade_ledger_from_journal(&paths);
        let entry = rebuilt.get("wallet-a::mint-a").expect("rebuilt entry");
        assert_eq!(entry.open_lots[0].remaining_amount_raw, 100);
    }

    #[test]
    fn known_transfer_clear_marker_replaces_prior_partial_fallback() {
        let temp = TempDirGuard::new("known-transfer-clears-partial");
        let paths = test_paths(&temp.path);
        let buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-buy".to_string(),
            slot: Some(5),
            confirmed_at_unix_ms: 500,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        append_confirmed_trade_event(&paths, &buy).expect("append buy");
        let partial = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            40,
            "sig-transfer:incomplete:sent_to:wallet-a:wallet-b",
            1_000,
            Some(10),
        );
        let transfer = TokenTransferMarkerEvent::new(
            "wallet-a",
            "wallet-b",
            "mint-a",
            40,
            "sig-transfer",
            2_000,
        )
        .with_slot(Some(20));
        let clear = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            0,
            "sig-transfer:incomplete:sent_to:wallet-a:wallet-b",
            2_000,
            Some(20),
        );
        append_incomplete_balance_adjustment_marker(&paths, &partial)
            .expect("append partial marker");
        append_token_transfer_marker(&paths, &transfer).expect("append transfer marker");
        append_incomplete_balance_adjustment_marker(&paths, &clear).expect("append clear marker");

        let rebuilt = rebuild_trade_ledger_from_journal(&paths);
        let source = rebuilt.get("wallet-a::mint-a").expect("source entry");
        let destination = rebuilt.get("wallet-b::mint-a").expect("destination entry");
        assert_eq!(source.open_lots[0].remaining_amount_raw, 60);
        assert_eq!(destination.open_lots[0].remaining_amount_raw, 40);
    }

    #[test]
    fn reconciliation_marker_slot_keeps_future_buys_out_of_old_adjustment() {
        let temp = TempDirGuard::new("reconcile-marker-slot-order");
        let paths = test_paths(&temp.path);
        let early_buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-early-buy".to_string(),
            slot: Some(10),
            confirmed_at_unix_ms: 1_000,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        let later_buy = ConfirmedTradeEvent {
            signature: "sig-later-buy".to_string(),
            slot: Some(30),
            confirmed_at_unix_ms: 3_000,
            token_delta_raw: 25,
            trade_value_lamports: 25,
            ..early_buy.clone()
        };
        append_confirmed_trade_event(&paths, &later_buy).expect("append later buy first");
        append_confirmed_trade_event(&paths, &early_buy).expect("append early buy second");
        let marker = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            110,
            "resync-balance-reconcile:sent_without_proceeds:wallet-a:mint-a",
            2_000,
            Some(20),
        );
        append_incomplete_balance_adjustment_marker(&paths, &marker).expect("append marker");

        let rebuilt = rebuild_trade_ledger_from_journal(&paths);
        let entry = rebuilt.get("wallet-a::mint-a").expect("rebuilt entry");
        assert_eq!(entry.open_lots.len(), 1);
        assert_eq!(entry.open_lots[0].signature, "sig-later-buy");
        assert_eq!(entry.open_lots[0].remaining_amount_raw, 25);
    }

    #[test]
    fn same_signature_incomplete_adjustments_do_not_collapse_on_rebuild() {
        let temp = TempDirGuard::new("same-signature-incomplete");
        let paths = test_paths(&temp.path);
        let buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-buy".to_string(),
            slot: Some(1),
            confirmed_at_unix_ms: 100,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        append_confirmed_trade_event(&paths, &buy).expect("append buy");
        let first = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            30,
            "sig-transfer:incomplete:sent_to:wallet-a:wallet-b",
            1_000,
            Some(10),
        );
        let second = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            20,
            "sig-transfer:incomplete:sent_to:wallet-a:wallet-c",
            1_000,
            Some(10),
        );
        append_incomplete_balance_adjustment_marker(&paths, &first).expect("append first marker");
        append_incomplete_balance_adjustment_marker(&paths, &second).expect("append second marker");

        let rebuilt = rebuild_trade_ledger_from_journal(&paths);
        let entry = rebuilt.get("wallet-a::mint-a").expect("rebuilt entry");
        assert_eq!(entry.open_lots[0].remaining_amount_raw, 50);
    }

    #[test]
    fn changed_incomplete_adjustment_amount_replaces_prior_amount_on_rebuild() {
        let temp = TempDirGuard::new("changed-incomplete-amount");
        let paths = test_paths(&temp.path);
        let buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-buy".to_string(),
            slot: Some(1),
            confirmed_at_unix_ms: 100,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        append_confirmed_trade_event(&paths, &buy).expect("append buy");
        let older = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            40,
            "sig-transfer:incomplete:sent_to:wallet-a:wallet-b",
            1_000,
            Some(10),
        );
        let newer = IncompleteBalanceAdjustmentMarkerEvent::sent_without_proceeds(
            "wallet-a",
            "mint-a",
            10,
            "sig-transfer:incomplete:sent_to:wallet-a:wallet-b",
            2_000,
            Some(20),
        );
        append_incomplete_balance_adjustment_marker(&paths, &older).expect("append older marker");
        append_incomplete_balance_adjustment_marker(&paths, &newer).expect("append newer marker");

        let rebuilt = rebuild_trade_ledger_from_journal(&paths);
        let entry = rebuilt.get("wallet-a::mint-a").expect("rebuilt entry");
        assert_eq!(entry.open_lots[0].remaining_amount_raw, 90);
    }

    #[test]
    fn journal_rebuild_applies_force_close_after_prior_buys() {
        let temp = TempDirGuard::new("rebuild-force-close");
        let paths = test_paths(&temp.path);

        let buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-buy".to_string(),
            slot: Some(1),
            confirmed_at_unix_ms: 100,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 250,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        append_confirmed_trade_event(&paths, &buy).expect("append buy");
        let marker = ForceCloseMarkerEvent::new("wallet-a", "mint-a", 200, "rebuild-test");
        append_force_close_marker(&paths, &marker).expect("append force-close marker");

        let ledger = rebuild_trade_ledger_from_journal(&paths);
        let entry = ledger.get("wallet-a::mint-a").expect("rebuilt entry");
        assert!(!entry.position_open);
        assert!(entry.open_lots.is_empty());
        assert_eq!(entry.buy_count, 1);
        assert_eq!(entry.sell_count, 1);
        assert_eq!(entry.realized_pnl_gross_lamports, -250);
        assert_eq!(entry.last_trade_at_unix_ms, 200);
        assert_eq!(entry.remaining_cost_basis_lamports, 0);
    }

    #[test]
    fn no_slot_force_close_rebuilds_before_later_slotted_buy_by_timestamp() {
        let temp = TempDirGuard::new("force-close-before-later-buy");
        let paths = test_paths(&temp.path);
        let early_buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-early-buy".to_string(),
            slot: Some(10),
            confirmed_at_unix_ms: 1_000,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        let later_buy = ConfirmedTradeEvent {
            signature: "sig-later-buy".to_string(),
            slot: Some(30),
            confirmed_at_unix_ms: 3_000,
            token_delta_raw: 25,
            trade_value_lamports: 25,
            ..early_buy.clone()
        };
        append_confirmed_trade_event(&paths, &later_buy).expect("append later buy first");
        append_confirmed_trade_event(&paths, &early_buy).expect("append early buy second");
        append_force_close_marker(
            &paths,
            &ForceCloseMarkerEvent::new("wallet-a", "mint-a", 2_000, "rebuild-test"),
        )
        .expect("append force-close marker");

        let ledger = rebuild_trade_ledger_from_journal(&paths);
        let entry = ledger.get("wallet-a::mint-a").expect("rebuilt entry");
        assert_eq!(entry.open_lots.len(), 1);
        assert_eq!(entry.open_lots[0].signature, "sig-later-buy");
        assert_eq!(entry.open_lots[0].remaining_amount_raw, 25);
    }

    #[test]
    fn no_slot_token_transfer_rebuilds_before_later_slotted_sell_by_timestamp() {
        let temp = TempDirGuard::new("transfer-before-later-sell");
        let paths = test_paths(&temp.path);
        let buy = ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-buy".to_string(),
            slot: Some(10),
            confirmed_at_unix_ms: 1_000,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        };
        let sell_from_destination = ConfirmedTradeEvent {
            signature: "sig-sell".to_string(),
            slot: Some(30),
            confirmed_at_unix_ms: 3_000,
            wallet_key: "wallet-b".to_string(),
            wallet_public_key: "wallet-b".to_string(),
            side: TradeSide::Sell,
            token_delta_raw: -40,
            trade_value_lamports: 60,
            ..buy.clone()
        };
        append_confirmed_trade_event(&paths, &sell_from_destination).expect("append sell first");
        append_confirmed_trade_event(&paths, &buy).expect("append buy second");
        append_token_transfer_marker(
            &paths,
            &TokenTransferMarkerEvent::new(
                "wallet-a",
                "wallet-b",
                "mint-a",
                40,
                "sig-transfer",
                2_000,
            ),
        )
        .expect("append transfer marker");

        let ledger = rebuild_trade_ledger_from_journal(&paths);
        let source = ledger.get("wallet-a::mint-a").expect("source entry");
        let destination = ledger.get("wallet-b::mint-a").expect("destination entry");
        assert_eq!(source.open_lots[0].remaining_amount_raw, 60);
        assert!(destination.open_lots.is_empty());
        assert_eq!(destination.realized_pnl_gross_lamports, 20);
    }

    #[test]
    fn journal_entry_comparator_is_transitive_for_mixed_slot_and_timestamp_order() {
        let a = JournalEntry::Trade(ConfirmedTradeEvent {
            schema_version: trade_ledger_schema_version(),
            signature: "sig-a".to_string(),
            slot: Some(5),
            confirmed_at_unix_ms: 3_000,
            wallet_key: "wallet-a".to_string(),
            wallet_public_key: "wallet-a".to_string(),
            mint: "mint-a".to_string(),
            side: TradeSide::Buy,
            platform_tag: PlatformTag::Axiom,
            provenance: EventProvenance::LocalExecution,
            settlement_asset: Some(TradeSettlementAsset::Sol),
            token_delta_raw: 100,
            token_decimals: Some(6),
            trade_value_lamports: 100,
            explicit_fees: ExplicitFeeBreakdown::default(),
            client_request_id: None,
            batch_id: None,
        });
        let b = JournalEntry::Trade(ConfirmedTradeEvent {
            signature: "sig-b".to_string(),
            slot: Some(10),
            confirmed_at_unix_ms: 1_000,
            ..match &a {
                JournalEntry::Trade(event) => event.clone(),
                _ => unreachable!(),
            }
        });
        let c = JournalEntry::ForceCloseMarker(ForceCloseMarkerEvent::new(
            "wallet-a", "mint-a", 2_000, "test",
        ));

        assert!(compare_journal_entries(&b, &c).is_lt());
        assert!(compare_journal_entries(&c, &a).is_lt());
        assert!(compare_journal_entries(&b, &a).is_lt());
    }
}
