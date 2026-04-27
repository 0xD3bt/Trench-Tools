use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::sync::{Mutex, RwLock};

use crate::{
    trade_dispatch::{TradeDispatchPlan, TradeInputKind},
    trade_planner::{LifecycleAndCanonicalMarket, TradeLifecycle},
};

const PRE_MIGRATION_ROLLING_TTL_MS: u64 = 10_000;
const PRE_MIGRATION_ABSOLUTE_MAX_AGE_MS: u64 = 30_000;
const POST_MIGRATION_ROLLING_TTL_MS: u64 = 60 * 60 * 1000;
const POST_MIGRATION_ABSOLUTE_MAX_AGE_MS: u64 = 4 * 60 * 60 * 1000;
const NEGATIVE_CACHE_TTL_MS: u64 = 10_000;
const ROUTE_INDEX_LRU_CAP: usize = 256;
const ROUTE_INDEX_POLICY_VERSION: &str = "verified-mint-or-pool-v2";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteIndexKey {
    pub submitted_address: String,
    pub rpc_url: String,
    pub commitment: String,
    pub side: String,
    pub route_policy: String,
    pub pinned_pool: Option<String>,
    pub allow_non_canonical: bool,
    pub policy_version: String,
}

impl RouteIndexKey {
    pub fn new(
        submitted_address: &str,
        rpc_url: &str,
        commitment: &str,
        side: &str,
        route_policy: &str,
        pinned_pool: Option<&str>,
        allow_non_canonical: bool,
    ) -> Self {
        Self {
            submitted_address: submitted_address.trim().to_string(),
            rpc_url: rpc_url.trim().to_string(),
            commitment: commitment.trim().to_ascii_lowercase(),
            side: side.trim().to_ascii_lowercase(),
            route_policy: route_policy.trim().to_ascii_lowercase(),
            pinned_pool: pinned_pool
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            allow_non_canonical,
            policy_version: ROUTE_INDEX_POLICY_VERSION.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RouteIndexEntry {
    pub submitted_address: String,
    pub input_kind: TradeInputKind,
    pub resolved_mint: String,
    pub resolved_pool: Option<String>,
    pub selector: LifecycleAndCanonicalMarket,
    pub non_canonical: bool,
    pub source: String,
    pub fetched_at_unix_ms: u64,
    pub last_used_at_unix_ms: u64,
}

impl RouteIndexEntry {
    fn from_plan(plan: &TradeDispatchPlan, source: &str) -> Self {
        let now = now_unix_ms();
        Self {
            submitted_address: plan.raw_address.clone(),
            input_kind: plan.resolved_input_kind,
            resolved_mint: plan.resolved_mint.clone(),
            resolved_pool: plan.resolved_pinned_pool.clone(),
            selector: plan.selector.clone(),
            non_canonical: plan.non_canonical,
            source: source.to_string(),
            fetched_at_unix_ms: now,
            last_used_at_unix_ms: now,
        }
    }

    fn is_stale(&self, now: u64) -> bool {
        let (rolling_ttl, absolute_max_age) = match self.selector.lifecycle {
            TradeLifecycle::PreMigration => (
                PRE_MIGRATION_ROLLING_TTL_MS,
                PRE_MIGRATION_ABSOLUTE_MAX_AGE_MS,
            ),
            TradeLifecycle::PostMigration => (
                POST_MIGRATION_ROLLING_TTL_MS,
                POST_MIGRATION_ABSOLUTE_MAX_AGE_MS,
            ),
        };
        now.saturating_sub(self.last_used_at_unix_ms) > rolling_ttl
            || now.saturating_sub(self.fetched_at_unix_ms) > absolute_max_age
    }
}

#[derive(Debug, Clone)]
pub struct NegativeRouteEntry {
    pub error_code: String,
    pub message: String,
    pub fetched_at_unix_ms: u64,
}

impl NegativeRouteEntry {
    fn is_stale(&self, now: u64) -> bool {
        now.saturating_sub(self.fetched_at_unix_ms) > NEGATIVE_CACHE_TTL_MS
    }
}

#[derive(Debug, Clone)]
pub enum RouteIndexLookup {
    Hit(RouteIndexEntry),
    Negative(NegativeRouteEntry),
}

#[derive(Default)]
pub struct RouteIndex {
    entries: RwLock<HashMap<RouteIndexKey, RouteIndexEntry>>,
    negative_entries: RwLock<HashMap<RouteIndexKey, NegativeRouteEntry>>,
    flight_locks: Mutex<HashMap<RouteIndexKey, Arc<Mutex<()>>>>,
}

static SHARED_ROUTE_INDEX: OnceLock<RouteIndex> = OnceLock::new();

pub fn shared_route_index() -> &'static RouteIndex {
    SHARED_ROUTE_INDEX.get_or_init(RouteIndex::default)
}

impl RouteIndex {
    pub async fn current(&self, key: &RouteIndexKey) -> Option<RouteIndexLookup> {
        let now = now_unix_ms();
        {
            let mut entries = self.entries.write().await;
            if let Some(entry) = entries.get_mut(key) {
                if entry.is_stale(now) {
                    entries.remove(key);
                } else {
                    entry.last_used_at_unix_ms = now;
                    return Some(RouteIndexLookup::Hit(entry.clone()));
                }
            }
        }
        let mut negative_entries = self.negative_entries.write().await;
        if let Some(entry) = negative_entries.get(key) {
            if entry.is_stale(now) {
                negative_entries.remove(key);
            } else {
                return Some(RouteIndexLookup::Negative(entry.clone()));
            }
        }
        None
    }

    pub async fn insert_plan(&self, key: RouteIndexKey, plan: &TradeDispatchPlan, source: &str) {
        self.negative_entries.write().await.remove(&key);
        let mut entries = self.entries.write().await;
        entries.insert(key, RouteIndexEntry::from_plan(plan, source));
        if entries.len() > ROUTE_INDEX_LRU_CAP {
            if let Some(victim_key) = entries
                .iter()
                .min_by_key(|(_, value)| value.last_used_at_unix_ms)
                .map(|(key, _)| key.clone())
            {
                entries.remove(&victim_key);
            }
        }
    }

    pub async fn insert_negative(&self, key: RouteIndexKey, error_code: &str, message: &str) {
        self.entries.write().await.remove(&key);
        let mut negative_entries = self.negative_entries.write().await;
        negative_entries.insert(
            key,
            NegativeRouteEntry {
                error_code: error_code.to_string(),
                message: message.to_string(),
                fetched_at_unix_ms: now_unix_ms(),
            },
        );
    }

    pub async fn invalidate(&self, key: &RouteIndexKey) {
        self.entries.write().await.remove(key);
        self.negative_entries.write().await.remove(key);
    }

    pub async fn flight_lock(&self, key: &RouteIndexKey) -> Arc<Mutex<()>> {
        let mut locks = self.flight_locks.lock().await;
        locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub async fn finish_flight(&self, key: &RouteIndexKey, lock: &Arc<Mutex<()>>) {
        let mut locks = self.flight_locks.lock().await;
        if locks
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, lock) && Arc::strong_count(lock) <= 2)
        {
            locks.remove(key);
        }
    }
}

pub fn route_index_ttl_ms_for_plan(plan: &TradeDispatchPlan) -> u64 {
    match plan.selector.lifecycle {
        TradeLifecycle::PreMigration => PRE_MIGRATION_ROLLING_TTL_MS,
        TradeLifecycle::PostMigration => POST_MIGRATION_ROLLING_TTL_MS,
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rollout::TradeExecutionBackend,
        trade_dispatch::TradeAdapter,
        trade_planner::{
            PlannerQuoteAsset, PlannerVerificationSource, TradeVenueFamily, WrapperAction,
        },
    };

    fn plan(lifecycle: TradeLifecycle) -> TradeDispatchPlan {
        TradeDispatchPlan {
            adapter: TradeAdapter::RaydiumAmmV4Native,
            selector: LifecycleAndCanonicalMarket {
                lifecycle,
                family: TradeVenueFamily::RaydiumAmmV4,
                canonical_market_key: "Pool111".to_string(),
                quote_asset: PlannerQuoteAsset::Wsol,
                verification_source: PlannerVerificationSource::OnchainDerived,
                wrapper_action: WrapperAction::RaydiumAmmV4WsolBuy,
                wrapper_accounts: vec!["Pool111".to_string()],
                market_subtype: Some("amm-v4".to_string()),
                direct_protocol_target: Some("raydium-amm-v4".to_string()),
                input_amount_hint: None,
                minimum_output_hint: None,
                runtime_bundle: None,
            },
            execution_backend: TradeExecutionBackend::Native,
            raw_address: "Pool111".to_string(),
            resolved_input_kind: TradeInputKind::Pair,
            resolved_mint: "Mint111".to_string(),
            resolved_pinned_pool: Some("Pool111".to_string()),
            non_canonical: false,
        }
    }

    #[test]
    fn route_index_key_includes_transport_and_side_policy() {
        let buy = RouteIndexKey::new(
            "Mint111",
            "https://rpc-a",
            "CONFIRMED",
            "buy",
            "buy:sol_only",
            None,
            false,
        );
        let usd1_buy = RouteIndexKey::new(
            "Mint111",
            "https://rpc-a",
            "confirmed",
            "buy",
            "buy:usd1_only",
            None,
            false,
        );
        let sell = RouteIndexKey::new(
            "Mint111",
            "https://rpc-a",
            "confirmed",
            "sell",
            "sell:sol",
            None,
            false,
        );
        let other_rpc = RouteIndexKey::new(
            "Mint111",
            "https://rpc-b",
            "confirmed",
            "buy",
            "buy:sol_only",
            None,
            false,
        );

        assert_eq!(buy.commitment, "confirmed");
        assert_eq!(buy.policy_version, ROUTE_INDEX_POLICY_VERSION);
        assert_ne!(buy, usd1_buy);
        assert_ne!(buy, sell);
        assert_ne!(buy, other_rpc);
    }

    #[test]
    fn route_index_uses_lifecycle_ttls() {
        assert_eq!(
            route_index_ttl_ms_for_plan(&plan(TradeLifecycle::PreMigration)),
            PRE_MIGRATION_ROLLING_TTL_MS
        );
        assert_eq!(
            route_index_ttl_ms_for_plan(&plan(TradeLifecycle::PostMigration)),
            POST_MIGRATION_ROLLING_TTL_MS
        );
    }

    #[tokio::test]
    async fn finish_flight_removes_completed_lock() {
        let index = RouteIndex::default();
        let key = RouteIndexKey::new(
            "Mint111",
            "https://rpc-a",
            "confirmed",
            "buy",
            "buy:sol_only",
            None,
            false,
        );
        let lock = index.flight_lock(&key).await;
        {
            let _guard = lock.lock().await;
        }

        index.finish_flight(&key, &lock).await;
        let next = index.flight_lock(&key).await;

        assert!(!std::sync::Arc::ptr_eq(&lock, &next));
    }

    #[tokio::test]
    async fn finish_flight_keeps_lock_while_waiters_hold_clones() {
        let index = RouteIndex::default();
        let key = RouteIndexKey::new(
            "Mint111",
            "https://rpc-a",
            "confirmed",
            "buy",
            "buy:sol_only",
            None,
            false,
        );
        let lock = index.flight_lock(&key).await;
        let waiter = lock.clone();

        index.finish_flight(&key, &lock).await;
        let current = index.flight_lock(&key).await;

        assert!(std::sync::Arc::ptr_eq(&lock, &current));
        drop(waiter);
        drop(current);
        index.finish_flight(&key, &lock).await;
        let next = index.flight_lock(&key).await;
        assert!(!std::sync::Arc::ptr_eq(&lock, &next));
    }
}
