//! Tiny subscriber-backed per-wallet/per-mint token balance cache.
//!
//! The `balance_stream` crate already owns a persistent websocket that
//! emits `StreamEvent::Balance` events containing per-ATA token
//! amounts whenever the user's focused mints change. This module
//! consumes those events and keeps a small `(env_key, mint)` → `(ui
//! amount, decimals, at_ms)` map so the trade hot path can avoid the
//! `getTokenAccountsByOwner` RPC round-trip when the balance is still
//! reasonably fresh.
//!
//! Semantics:
//! - The cache is **advisory**. The hot path always re-checks before
//!   sending a transaction that would under- or over-spend.
//! - Entries expire after `CACHE_TTL_MS` so a stale balance doesn't
//!   silently sit past a trade that the user just made.
//! - Only mints that are currently "active" (i.e. a panel is open for
//!   them) actually populate this cache — the stream only subscribes
//!   to ATAs for active mints, by design.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use shared_extension_runtime::balance_stream::{BalanceStreamHandle, StreamEvent};
use tokio::sync::RwLock;
use tokio::time::{Duration, sleep};

const CACHE_TTL_MS: u64 = 2_500;
const CACHE_SETTLE_RETRY_DELAYS_MS: [u64; 3] = [60, 140, 280];

#[derive(Debug, Clone, Copy)]
pub struct CachedTokenBalance {
    pub ui_amount: f64,
    pub at_ms: u64,
}

impl CachedTokenBalance {
    pub fn is_fresh(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.at_ms) <= CACHE_TTL_MS
    }
}

#[derive(Default)]
pub struct WalletTokenCache {
    entries: RwLock<HashMap<String, CachedTokenBalance>>,
}

static SHARED_WALLET_TOKEN_CACHE: OnceLock<Arc<WalletTokenCache>> = OnceLock::new();

pub fn shared_wallet_token_cache() -> Arc<WalletTokenCache> {
    SHARED_WALLET_TOKEN_CACHE
        .get_or_init(|| Arc::new(WalletTokenCache::default()))
        .clone()
}

fn cache_key(env_key: &str, mint: &str) -> String {
    format!("{}::{}", env_key.trim(), mint.trim())
}

impl WalletTokenCache {
    /// Look up a cached balance for the given wallet env key and mint.
    /// Returns `None` if we've never seen a stream event for this
    /// pair, or if the cached event is older than `CACHE_TTL_MS`.
    pub async fn lookup(&self, env_key: &str, mint: &str) -> Option<CachedTokenBalance> {
        let now = now_unix_ms();
        let guard = self.entries.read().await;
        guard
            .get(&cache_key(env_key, mint))
            .copied()
            .filter(|entry| entry.is_fresh(now))
    }

    /// Explicit invalidation after a confirmed trade — the next sell
    /// click should pick up the post-trade balance rather than the
    /// cached pre-trade value.
    pub async fn invalidate(&self, env_key: &str, mint: &str) {
        self.entries.write().await.remove(&cache_key(env_key, mint));
    }

    pub async fn record(&self, env_key: &str, mint: &str, ui_amount: f64) {
        let entry = CachedTokenBalance {
            ui_amount,
            at_ms: now_unix_ms(),
        };
        self.entries
            .write()
            .await
            .insert(cache_key(env_key, mint), entry);
    }
}

use crate::rpc_client::{TokenBalance, fetch_token_balance_via_ata};

/// Fetch a token balance for `(wallet_key, owner, mint)` using the
/// warm balance-stream cache when possible. Falls back to an ATA-first
/// on-chain read when the cache has no fresh entry.
///
/// `decimals` is required because the balance-stream events only
/// carry UI amounts — we reconvert to raw lamports using the mint's
/// decimals as known by the caller.
pub async fn fetch_token_balance_with_cache(
    wallet_key: Option<&str>,
    owner: &str,
    mint: &str,
    decimals: u8,
) -> Result<TokenBalance, String> {
    if let Some(key) = wallet_key.filter(|value| !value.is_empty()) {
        let cache = shared_wallet_token_cache();
        for delay_ms in std::iter::once(0).chain(CACHE_SETTLE_RETRY_DELAYS_MS.into_iter()) {
            if delay_ms > 0 {
                sleep(Duration::from_millis(delay_ms)).await;
            }
            if let Some(cached) = cache.lookup(key, mint).await {
                let scale = 10u128.pow(u32::from(decimals));
                let raw = (cached.ui_amount.max(0.0) * scale as f64) as u128;
                let amount_raw = u64::try_from(raw).unwrap_or(u64::MAX);
                return Ok(TokenBalance {
                    amount_raw,
                    decimals,
                });
            }
        }
    }
    fetch_token_balance_via_ata(owner, mint, decimals, "confirmed").await
}

/// Spawn a background task that consumes `StreamEvent::Balance` events
/// from the shared balance stream and records any per-mint token
/// balances we see. Safe to call multiple times — only the first call
/// actually spawns.
pub fn ensure_subscriber(stream: &BalanceStreamHandle) {
    static SPAWNED: OnceLock<()> = OnceLock::new();
    if SPAWNED.set(()).is_err() {
        return;
    }
    let cache = shared_wallet_token_cache();
    let mut events = stream.subscribe_events();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(StreamEvent::Balance(payload)) => {
                    if let (Some(mint), Some(balance)) =
                        (payload.token_mint.as_ref(), payload.token_balance)
                    {
                        if !payload.env_key.is_empty() && !mint.is_empty() {
                            cache.record(&payload.env_key, mint, balance).await;
                        }
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    // Dropped events are fine — the RPC fallback keeps
                    // correctness. Log once per lag so the operator can
                    // spot the pattern if the stream is consistently
                    // behind (e.g. user opened 50 panels at once).
                    eprintln!(
                        "[execution-engine][wallet-token-cache] broadcast lagged by {missed} events; \
                         skipping to latest. Sell-sizing will fall back to RPC until the cache repopulates."
                    );
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
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

    #[tokio::test]
    async fn record_and_lookup_are_ttl_bounded() {
        let cache = WalletTokenCache::default();
        cache.record("wallet1", "mint1", 42.0).await;
        let hit = cache.lookup("wallet1", "mint1").await;
        assert!(hit.is_some());
        assert!((hit.unwrap().ui_amount - 42.0).abs() < f64::EPSILON);
        cache.invalidate("wallet1", "mint1").await;
        assert!(cache.lookup("wallet1", "mint1").await.is_none());
    }
}
