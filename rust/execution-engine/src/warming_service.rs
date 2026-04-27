use solana_sdk::hash::Hash;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{Mutex, RwLock},
    time::{Duration, sleep},
};

use crate::{
    mint_warm_cache::warm_ttl_ms_for_lifecycle,
    rpc_client::{fetch_block_height, fetch_latest_blockhash_fresh_or_recent},
    trade_planner::LifecycleAndCanonicalMarket,
};

const DEFAULT_COMMITMENT: &str = "confirmed";
const BLOCKHASH_REFRESH_INTERVAL_MS: u64 = 5_000;
/// How long a cached blockhash can be reused for planning before we refresh.
/// Matches LaunchDeck's `BLOCKHASH_MAX_AGE` (20s) after which we always
/// force-refresh. Kept below the ~2-minute Solana validity window by a wide
/// margin.
const BLOCKHASH_STALE_AFTER_MS: u64 = 20_000;
const SELECTOR_STALE_AFTER_MS: u64 = 4_000;
const WALLET_STATE_STALE_AFTER_MS: u64 = 8_000;
/// LaunchDeck-style runway check: if fewer than this many blocks remain
/// before `lastValidBlockHeight`, force-refresh the blockhash before compile.
/// 20 blocks ≈ 8 seconds of runway. Protects against signing a tx on a hash
/// that will expire mid-flight.
const COMPILE_BLOCKHASH_MIN_REMAINING_BLOCKS: u64 = 20;
/// TTL for the sampled current block height used by runway checks. 200ms
/// matches LaunchDeck's sampling cadence.
const BLOCK_HEIGHT_SAMPLE_TTL_MS: u64 = 200;

#[derive(Debug, Clone)]
pub struct CachedBlockhash {
    pub blockhash: Hash,
    pub last_valid_block_height: u64,
    pub fetched_at_unix_ms: u64,
    pub rpc_url: String,
    pub commitment: String,
}

impl CachedBlockhash {
    pub fn is_stale(&self, now_unix_ms: u64) -> bool {
        now_unix_ms.saturating_sub(self.fetched_at_unix_ms) > BLOCKHASH_STALE_AFTER_MS
    }
}

#[derive(Debug, Clone)]
pub struct CachedTradeSelector {
    pub selector: LifecycleAndCanonicalMarket,
    pub fetched_at_unix_ms: u64,
    pub rpc_url: String,
    pub commitment: String,
    pub side: String,
    pub route_policy: String,
    pub mint: String,
    pub pinned_pool: Option<String>,
    pub allow_non_canonical: bool,
}

impl CachedTradeSelector {
    pub fn is_stale(&self, now_unix_ms: u64) -> bool {
        let ttl_ms =
            SELECTOR_STALE_AFTER_MS.max(warm_ttl_ms_for_lifecycle(Some(&self.selector.lifecycle)));
        now_unix_ms.saturating_sub(self.fetched_at_unix_ms) > ttl_ms
    }
}

#[derive(Debug, Clone)]
pub struct CachedWalletTokenState {
    pub wallet_key: String,
    pub mint: String,
    pub token_balance_raw: Option<u64>,
    pub associated_token_exists: bool,
    pub fetched_at_unix_ms: u64,
}

impl CachedWalletTokenState {
    pub fn is_stale(&self, now_unix_ms: u64) -> bool {
        now_unix_ms.saturating_sub(self.fetched_at_unix_ms) > WALLET_STATE_STALE_AFTER_MS
    }
}

#[derive(Debug, Clone, Copy)]
struct CachedBlockHeight {
    height: u64,
    fetched_at_unix_ms: u64,
}

/// Cached bytes of a Solana account, keyed by `(rpc_url, commitment, address)`.
/// Used for static caches like Pump AMM global config, Pump AMM fee config,
/// and Pump bonding-curve global state — accounts that rarely change and
/// pay heavy RPC cost to re-fetch on every trade.
#[derive(Debug, Clone)]
pub struct CachedAccountBytes {
    pub data: Vec<u8>,
    pub fetched_at_unix_ms: u64,
}

/// TTL for "slowly-changing" global state caches (Pump AMM global config,
/// Pump AMM fee config). Conservative 10-minute TTL — long enough to collapse
/// repeated trades, short enough to pick up admin config updates quickly.
///
/// Trade success **does not** trigger an invalidation of this cache. These
/// accounts are changed only by Pump admin actions (fee tier updates,
/// protocol-fee-recipient rotations, etc.) and don't move as part of a
/// normal trade. If Pump publishes a config change and the operator wants
/// it picked up immediately, restart the host or call
/// `WarmingService::invalidate_account_bytes` directly.
const GLOBAL_STATE_TTL_MS: u64 = 10 * 60 * 1000;

/// TTL for Pump bonding-curve global state. Very long TTL — this account
/// changes approximately never within a session. Same caveat as
/// `GLOBAL_STATE_TTL_MS` about not being invalidated by trade completion.
const PUMP_BONDING_GLOBAL_TTL_MS: u64 = 60 * 60 * 1000;

/// Cached output of `getMinimumBalanceForRentExemption`, keyed by the data
/// length. Rent floors only change on cluster-level upgrades, so we cache
/// these forever within a process lifetime.
#[derive(Debug, Clone, Copy)]
struct CachedRentExempt {
    lamports: u64,
}

#[derive(Clone, Default)]
pub struct WarmingService {
    blockhash_entries: Arc<RwLock<HashMap<String, CachedBlockhash>>>,
    blockhash_refresh_tasks: Arc<Mutex<HashSet<String>>>,
    selector_entries: Arc<RwLock<HashMap<String, CachedTradeSelector>>>,
    wallet_token_entries: Arc<RwLock<HashMap<String, CachedWalletTokenState>>>,
    block_height_entries: Arc<RwLock<HashMap<String, CachedBlockHeight>>>,
    account_bytes_entries: Arc<RwLock<HashMap<String, CachedAccountBytes>>>,
    rent_exempt_entries: Arc<RwLock<HashMap<u64, CachedRentExempt>>>,
}

static SHARED_WARMING_SERVICE: OnceLock<WarmingService> = OnceLock::new();

pub fn shared_warming_service() -> &'static WarmingService {
    SHARED_WARMING_SERVICE.get_or_init(WarmingService::default)
}

impl WarmingService {
    pub async fn warm_execution_primitives(
        &self,
        rpc_url: &str,
        commitment: &str,
    ) -> Result<(), String> {
        self.ensure_blockhash_refresh_task(rpc_url, commitment)
            .await;
        if let Some(entry) = self.current_blockhash(rpc_url, commitment).await {
            if !entry.is_stale(now_unix_ms()) {
                return Ok(());
            }
        }
        self.refresh_blockhash_now(rpc_url, commitment).await?;
        Ok(())
    }

    pub async fn latest_blockhash(
        &self,
        rpc_url: &str,
        commitment: &str,
    ) -> Result<CachedBlockhash, String> {
        // LaunchDeck-style runway check: before we hand back a cached
        // blockhash to sign against, verify we have enough remaining block
        // height. This avoids signing a transaction whose hash is about to
        // expire mid-flight. If the runway is too short (or the cached
        // entry is stale, or we've never fetched one), force-refresh.
        self.ensure_blockhash_refresh_task(rpc_url, commitment)
            .await;
        let now = now_unix_ms();
        if let Some(entry) = self.current_blockhash(rpc_url, commitment).await {
            if !entry.is_stale(now) {
                match self.sampled_block_height(rpc_url, commitment).await {
                    Ok(current_height) => {
                        let remaining =
                            entry.last_valid_block_height.saturating_sub(current_height);
                        if remaining >= COMPILE_BLOCKHASH_MIN_REMAINING_BLOCKS {
                            return Ok(entry);
                        }
                    }
                    Err(_) => {
                        // If the runway probe fails (network hiccup), fall
                        // back to the cached entry rather than stalling the
                        // trade — the staleness TTL still bounds the risk.
                        return Ok(entry);
                    }
                }
            }
        }
        self.refresh_blockhash_now(rpc_url, commitment).await
    }

    async fn sampled_block_height(&self, rpc_url: &str, commitment: &str) -> Result<u64, String> {
        let normalized_commitment = normalize_commitment(commitment);
        let key = blockhash_cache_key(rpc_url, &normalized_commitment);
        let now = now_unix_ms();
        if let Some(sample) = self.block_height_entries.read().await.get(&key).copied() {
            if now.saturating_sub(sample.fetched_at_unix_ms) <= BLOCK_HEIGHT_SAMPLE_TTL_MS {
                return Ok(sample.height);
            }
        }
        let height = fetch_block_height(rpc_url, &normalized_commitment).await?;
        self.block_height_entries.write().await.insert(
            key,
            CachedBlockHeight {
                height,
                fetched_at_unix_ms: now_unix_ms(),
            },
        );
        Ok(height)
    }

    pub async fn current_blockhash(
        &self,
        rpc_url: &str,
        commitment: &str,
    ) -> Option<CachedBlockhash> {
        self.blockhash_entries
            .read()
            .await
            .get(&blockhash_cache_key(rpc_url, commitment))
            .cloned()
    }

    pub async fn cache_selector(
        &self,
        rpc_url: &str,
        commitment: &str,
        side: &str,
        route_policy: &str,
        mint: &str,
        pinned_pool: Option<&str>,
        allow_non_canonical: bool,
        selector: LifecycleAndCanonicalMarket,
    ) {
        let entry = CachedTradeSelector {
            selector,
            fetched_at_unix_ms: now_unix_ms(),
            rpc_url: rpc_url.trim().to_string(),
            commitment: normalize_commitment(commitment),
            side: normalize_side(side),
            route_policy: normalize_policy(route_policy),
            mint: mint.trim().to_string(),
            pinned_pool: normalize_optional(pinned_pool),
            allow_non_canonical,
        };
        self.selector_entries.write().await.insert(
            selector_cache_key(
                rpc_url,
                commitment,
                side,
                route_policy,
                mint,
                pinned_pool,
                allow_non_canonical,
            ),
            entry,
        );
    }

    pub async fn current_selector(
        &self,
        rpc_url: &str,
        commitment: &str,
        side: &str,
        route_policy: &str,
        mint: &str,
        pinned_pool: Option<&str>,
        allow_non_canonical: bool,
    ) -> Option<CachedTradeSelector> {
        self.selector_entries
            .read()
            .await
            .get(&selector_cache_key(
                rpc_url,
                commitment,
                side,
                route_policy,
                mint,
                pinned_pool,
                allow_non_canonical,
            ))
            .cloned()
    }

    pub async fn invalidate_selector(
        &self,
        rpc_url: &str,
        commitment: &str,
        side: &str,
        route_policy: &str,
        mint: &str,
        pinned_pool: Option<&str>,
        allow_non_canonical: bool,
    ) {
        self.selector_entries
            .write()
            .await
            .remove(&selector_cache_key(
                rpc_url,
                commitment,
                side,
                route_policy,
                mint,
                pinned_pool,
                allow_non_canonical,
            ));
    }

    pub async fn invalidate_selectors_for_mint(
        &self,
        rpc_url: &str,
        commitment: &str,
        side: &str,
        mint: &str,
    ) {
        let normalized_rpc = rpc_url.trim().to_string();
        let normalized_commitment = normalize_commitment(commitment);
        let normalized_side = normalize_side(side);
        let normalized_mint = mint.trim().to_string();
        self.selector_entries.write().await.retain(|_, entry| {
            !(entry.rpc_url == normalized_rpc
                && entry.commitment == normalized_commitment
                && entry.side == normalized_side
                && entry.mint == normalized_mint)
        });
    }

    pub async fn cache_wallet_token_state(
        &self,
        wallet_key: &str,
        mint: &str,
        token_balance_raw: Option<u64>,
        associated_token_exists: bool,
    ) {
        let entry = CachedWalletTokenState {
            wallet_key: wallet_key.trim().to_string(),
            mint: mint.trim().to_string(),
            token_balance_raw,
            associated_token_exists,
            fetched_at_unix_ms: now_unix_ms(),
        };
        self.wallet_token_entries
            .write()
            .await
            .insert(wallet_token_state_cache_key(wallet_key, mint), entry);
    }

    pub async fn current_wallet_token_state(
        &self,
        wallet_key: &str,
        mint: &str,
    ) -> Option<CachedWalletTokenState> {
        self.wallet_token_entries
            .read()
            .await
            .get(&wallet_token_state_cache_key(wallet_key, mint))
            .cloned()
    }

    /// Read a cached account-data blob if it is still within its TTL,
    /// otherwise fetch via `fetcher` and cache the result. `ttl_ms` caps
    /// how long the cached bytes are trusted without a re-fetch.
    ///
    /// The cache key is `(rpc_url, commitment, address)` so different
    /// commitments and RPC endpoints never cross-pollinate.
    pub async fn account_bytes_with_ttl<F, Fut>(
        &self,
        rpc_url: &str,
        commitment: &str,
        address: &str,
        ttl_ms: u64,
        fetcher: F,
    ) -> Result<Vec<u8>, String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
    {
        let key = account_bytes_cache_key(rpc_url, commitment, address);
        let now = now_unix_ms();
        {
            let read = self.account_bytes_entries.read().await;
            if let Some(entry) = read.get(&key) {
                if now.saturating_sub(entry.fetched_at_unix_ms) <= ttl_ms {
                    return Ok(entry.data.clone());
                }
            }
        }
        let fresh = fetcher().await?;
        self.account_bytes_entries.write().await.insert(
            key,
            CachedAccountBytes {
                data: fresh.clone(),
                fetched_at_unix_ms: now_unix_ms(),
            },
        );
        Ok(fresh)
    }

    /// Convenience wrapper for slowly-changing global state (e.g. Pump AMM
    /// global config, Pump AMM fee config). 10-minute TTL.
    pub async fn global_state_account_bytes<F, Fut>(
        &self,
        rpc_url: &str,
        commitment: &str,
        address: &str,
        fetcher: F,
    ) -> Result<Vec<u8>, String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
    {
        self.account_bytes_with_ttl(rpc_url, commitment, address, GLOBAL_STATE_TTL_MS, fetcher)
            .await
    }

    /// Convenience wrapper for Pump bonding-curve global state.
    /// 1-hour TTL — the value effectively never changes during a session.
    pub async fn pump_bonding_global_bytes<F, Fut>(
        &self,
        rpc_url: &str,
        commitment: &str,
        address: &str,
        fetcher: F,
    ) -> Result<Vec<u8>, String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
    {
        self.account_bytes_with_ttl(
            rpc_url,
            commitment,
            address,
            PUMP_BONDING_GLOBAL_TTL_MS,
            fetcher,
        )
        .await
    }

    /// Explicit invalidation for a cached account blob. Useful when the
    /// caller knows their read returned stale data — e.g. the compile
    /// path detected that a cached pool's reserves disagree with the
    /// on-chain state.
    pub async fn invalidate_account_bytes(&self, rpc_url: &str, commitment: &str, address: &str) {
        let key = account_bytes_cache_key(rpc_url, commitment, address);
        self.account_bytes_entries.write().await.remove(&key);
    }

    /// Returns the cached rent-exempt lamport floor for an account of the
    /// given data length, or fetches it via `fetcher` if we haven't seen
    /// this length yet. Values are cached for the lifetime of the process
    /// because rent floors only change on cluster-level upgrades.
    pub async fn minimum_balance_for_rent_exemption<F, Fut>(
        &self,
        data_len: u64,
        fetcher: F,
    ) -> Result<u64, String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<u64, String>>,
    {
        {
            let read = self.rent_exempt_entries.read().await;
            if let Some(entry) = read.get(&data_len) {
                return Ok(entry.lamports);
            }
        }
        let lamports = fetcher().await?;
        self.rent_exempt_entries
            .write()
            .await
            .insert(data_len, CachedRentExempt { lamports });
        Ok(lamports)
    }

    async fn ensure_blockhash_refresh_task(&self, rpc_url: &str, commitment: &str) {
        let normalized_commitment = normalize_commitment(commitment);
        let key = blockhash_cache_key(rpc_url, &normalized_commitment);
        let should_spawn = {
            let mut tasks = self.blockhash_refresh_tasks.lock().await;
            tasks.insert(key)
        };
        if !should_spawn {
            return;
        }

        let service = self.clone();
        let rpc_url = rpc_url.trim().to_string();
        tokio::spawn(async move {
            loop {
                let _ = service
                    .refresh_blockhash_now(&rpc_url, &normalized_commitment)
                    .await;
                sleep(Duration::from_millis(BLOCKHASH_REFRESH_INTERVAL_MS)).await;
            }
        });
    }

    async fn refresh_blockhash_now(
        &self,
        rpc_url: &str,
        commitment: &str,
    ) -> Result<CachedBlockhash, String> {
        let normalized_commitment = normalize_commitment(commitment);
        let (blockhash, last_valid_block_height) =
            fetch_latest_blockhash_fresh_or_recent(rpc_url, &normalized_commitment, 0).await?;
        let entry = CachedBlockhash {
            blockhash,
            last_valid_block_height,
            fetched_at_unix_ms: now_unix_ms(),
            rpc_url: rpc_url.trim().to_string(),
            commitment: normalized_commitment.clone(),
        };
        self.blockhash_entries.write().await.insert(
            blockhash_cache_key(rpc_url, &normalized_commitment),
            entry.clone(),
        );
        Ok(entry)
    }
}

fn blockhash_cache_key(rpc_url: &str, commitment: &str) -> String {
    format!("{}::{}", rpc_url.trim(), normalize_commitment(commitment))
}

fn account_bytes_cache_key(rpc_url: &str, commitment: &str, address: &str) -> String {
    format!(
        "{}::{}::{}",
        rpc_url.trim(),
        normalize_commitment(commitment),
        address.trim()
    )
}

fn selector_cache_key(
    rpc_url: &str,
    commitment: &str,
    side: &str,
    route_policy: &str,
    mint: &str,
    pinned_pool: Option<&str>,
    allow_non_canonical: bool,
) -> String {
    format!(
        "{}::{}::{}::{}::{}::{}::{}",
        rpc_url.trim(),
        normalize_commitment(commitment),
        normalize_side(side),
        normalize_policy(route_policy),
        mint.trim(),
        normalize_optional(pinned_pool).unwrap_or_default(),
        if allow_non_canonical { "1" } else { "0" }
    )
}

fn wallet_token_state_cache_key(wallet_key: &str, mint: &str) -> String {
    format!("{}::{}", wallet_key.trim(), mint.trim())
}

fn normalize_commitment(commitment: &str) -> String {
    let normalized = commitment.trim().to_lowercase();
    if normalized.is_empty() {
        DEFAULT_COMMITMENT.to_string()
    } else {
        normalized
    }
}

fn normalize_side(side: &str) -> String {
    let normalized = side.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        "buy".to_string()
    } else {
        normalized
    }
}

fn normalize_policy(policy: &str) -> String {
    let normalized = policy.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        "default".to_string()
    } else {
        normalized
    }
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value
        .map(|candidate| candidate.trim().to_string())
        .filter(|candidate| !candidate.is_empty())
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
    use crate::trade_planner::{
        LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerVerificationSource, TradeLifecycle,
        TradeVenueFamily, WrapperAction,
    };

    fn sample_selector() -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PostMigration,
            family: TradeVenueFamily::PumpAmm,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: PlannerQuoteAsset::Wsol,
            verification_source: PlannerVerificationSource::OnchainDerived,
            wrapper_action: WrapperAction::PumpAmmWsolBuy,
            wrapper_accounts: vec![],
            market_subtype: None,
            direct_protocol_target: None,
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    #[tokio::test]
    async fn selector_cache_separates_pinned_pool_variants() {
        let service = WarmingService::default();
        service
            .cache_selector(
                "https://rpc",
                "confirmed",
                "buy",
                "buy:sol_only",
                "Mint111",
                Some("PoolAAA"),
                false,
                sample_selector(),
            )
            .await;
        assert!(
            service
                .current_selector(
                    "https://rpc",
                    "confirmed",
                    "buy",
                    "buy:sol_only",
                    "Mint111",
                    Some("PoolBBB"),
                    false,
                )
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn invalidate_selectors_for_mint_removes_matching_entries() {
        let service = WarmingService::default();
        service
            .cache_selector(
                "https://rpc",
                "confirmed",
                "buy",
                "buy:sol_only",
                "Mint111",
                None,
                false,
                sample_selector(),
            )
            .await;

        service
            .invalidate_selectors_for_mint("https://rpc", "confirmed", "buy", "Mint111")
            .await;

        assert!(
            service
                .current_selector(
                    "https://rpc",
                    "confirmed",
                    "buy",
                    "buy:sol_only",
                    "Mint111",
                    None,
                    false,
                )
                .await
                .is_none()
        );
    }
}
