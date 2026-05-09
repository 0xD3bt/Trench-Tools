//! Per-mint warm cache.
//!
//! The warming_service module already handles cross-cutting static state
//! (blockhash runway, Pump global configs, rent exemptions). This module
//! owns the *per-mint* layer: a short-lived, single-flight cache of the
//! resolved trade plan plus any family-specific state that a subsequent
//! buy/sell click can consume to skip venue discovery.
//!
//! Design notes:
//! - **Intent-driven, not surface-driven.** Entries are created when the
//!   UI signals intent (token page mount, panel open, hover on an
//!   actionable control) and expire quickly so we don't keep hundreds of
//!   Pulse rows warm.
//! - **Family-specific variants.** Pump, Bonk, and Meteora have genuinely
//!   different hot paths — Pump needs pool + creator, Bonk needs pool +
//!   config + quote-asset route, Meteora needs DBC/DAMM selection. The
//!   `VenueWarmData` enum carries the shape each family actually wants.
//! - **Canonical warm key.** We fingerprint each entry by
//!   `(mint, pinned_pool, rpc, commitment, route_policy, allow_non_canonical)`
//!   so settings changes invalidate automatically without trusting scraped route hints.
//! - **Single-flight.** A `tokio::Mutex` per mint dedupes concurrent
//!   hover + panel-open + click warms into one RPC burst.
//! - **TTL + LRU.** Entries get a rolling 30s TTL (refreshed on hit) and
//!   a small LRU cap so the cache size stays bounded.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use shared_extension_runtime::follow_contract::BagsLaunchMetadata;
use tokio::sync::{Mutex, RwLock};

use crate::{
    bonk_native::{BonkImportContext, selector_to_bonk_import_context},
    meteora_native::{BagsImportContext, selector_to_bags_import_context, selector_to_bags_launch},
    trade_dispatch::TradeDispatchPlan,
    trade_planner::{LifecycleAndCanonicalMarket, PlannerRuntimeBundle, TradeLifecycle},
};

/// Pre-migration routes are the riskiest to cache because the mint can
/// still move to its final venue underneath us, so keep this path tight.
const PRE_MIGRATION_WARM_TTL_MS: u64 = 10_000;
/// Post-migration routes are materially more stable, so let repeated
/// clicks reuse the resolved route for longer.
const POST_MIGRATION_WARM_TTL_MS: u64 = 60 * 60 * 1000;
/// Fallback when we do not have enough routing context to classify the
/// lifecycle confidently. Bias short rather than serving stale routes.
const UNKNOWN_WARM_TTL_MS: u64 = PRE_MIGRATION_WARM_TTL_MS;

/// Hard cap on the number of warm mints we keep simultaneously. Once the
/// cap is exceeded, the least-recently-used entry is dropped. Tuned for
/// the "intent-driven" use case: a handful of token-page / panel-open
/// mints at a time, plus the odd hover.
const WARM_LRU_CAP: usize = 256;

/// Family-specific warm data attached to a `PrewarmedMint`. Right now
/// we only persist the resolved routing selector; per-family venue
/// details (pool state, creator, DBC market, etc.) are fetched on the
/// compile path, but the enum is shaped so those fields can be added
/// without disturbing callers.
#[derive(Debug, Clone)]
pub enum VenueWarmData {
    Pump {
        /// Optional pinned pool pubkey if the caller selected a
        /// non-canonical pool. `None` means "canonical pool, derive it
        /// on compile".
        pinned_pool: Option<String>,
        /// True if the pinned pool was verified to be non-canonical
        /// (i.e. input was a pair address, not the canonical pool for
        /// the mint).
        non_canonical: bool,
        /// Pre- vs post-migration Pump route. Helpful when we want the
        /// warm entry itself (not just the selector) to describe which
        /// account matrix will be used on compile.
        lifecycle: TradeLifecycle,
        /// Market the selector resolved to: bonding-curve PDA for
        /// pre-migration, pool pubkey for Pump AMM.
        market_key: String,
        /// Mint program / base-token program the compile path should use.
        mint_token_program: Option<String>,
        /// Launch creator / coin creator used to derive creator-vault and
        /// related Pump fee accounts.
        creator: Option<String>,
        /// Mode bits that influence fee recipient selection and optional
        /// remaining accounts on compile.
        is_mayhem_mode: bool,
        is_cashback_coin: bool,
    },
    Bonk {
        /// Quote asset resolved by the verified selector (`SOL` vs `USD1`).
        quote_asset: Option<String>,
        /// Cached Bonk venue-discovery result so later warm hits can
        /// explain or reseed the route without re-running import lookup.
        import_context: Option<BonkImportContext>,
    },
    Bags {
        /// Verified lifecycle of the cached selector.
        lifecycle: Option<TradeLifecycle>,
        /// Cached Bags venue-discovery result.
        import_context: Option<BagsImportContext>,
        /// Launch metadata paired with the resolved Bags selector.
        bags_launch: Option<BagsLaunchMetadata>,
    },
    Stable {
        pool: String,
        venue: Option<String>,
    },
    RaydiumAmmV4 {
        pool: String,
    },
    RaydiumCpmm {
        pool: String,
    },
    RaydiumLaunchLab {
        pool: String,
    },
}

/// A prewarmed mint entry. Carries the resolved trade plan plus the
/// normalized mint / pair, family variant, and warm fingerprint.
#[derive(Debug, Clone)]
pub struct PrewarmedMint {
    /// Normalized mint pubkey string. Always a real mint, never a pair.
    pub mint: String,
    /// If the input to `/prewarm` was a pair / pool address, this holds
    /// the pool pubkey we resolved it to. Otherwise `None`.
    pub resolved_pair: Option<String>,
    /// Opaque warm key clients can round-trip on buy/sell requests to
    /// match against the cache entry used on prewarm.
    pub warm_key: String,
    /// Non-canonical policy bit baked into the warm fingerprint. Warm-key
    /// reuse must reject entries created under a different policy.
    pub allow_non_canonical: bool,
    /// Resolved family + selector snapshot. `None` when the prewarm
    /// request only carried the normalized mint but didn't fully plan
    /// the trade (e.g. classification succeeded but the planner hasn't
    /// been run yet).
    pub plan: Option<CachedPlan>,
    /// Family-specific warm data.
    pub venue: VenueWarmData,
    /// When this entry was first warmed.
    pub warmed_at_unix_ms: u64,
    /// Last time a read (prewarm re-hit, trade click, etc.) touched it.
    pub last_used_at_unix_ms: u64,
}

/// Route snapshot of a resolved plan. We intentionally don't persist the
/// full `TradeDispatchPlan` here because `execution_backend` and the
/// user-supplied raw address are request-specific — both are reattached
/// when the cache is consumed.
#[derive(Debug, Clone)]
pub struct CachedPlan {
    pub selector: LifecycleAndCanonicalMarket,
    pub resolved_pinned_pool: Option<String>,
    pub non_canonical: bool,
}

impl PrewarmedMint {
    pub fn is_stale(&self, now_unix_ms: u64) -> bool {
        now_unix_ms.saturating_sub(self.last_used_at_unix_ms) > self.ttl_ms()
    }

    pub fn ttl_ms(&self) -> u64 {
        warm_ttl_ms_for_lifecycle(self.lifecycle().as_ref())
    }

    fn lifecycle(&self) -> Option<TradeLifecycle> {
        self.plan
            .as_ref()
            .map(|plan| plan.selector.lifecycle.clone())
            .or_else(|| match &self.venue {
                VenueWarmData::Pump { lifecycle, .. } => Some(lifecycle.clone()),
                VenueWarmData::Bags { lifecycle, .. } => lifecycle.clone(),
                VenueWarmData::Bonk { import_context, .. } => {
                    import_context.as_ref().and_then(|context| {
                        let normalized = context.mode.trim().to_ascii_lowercase();
                        if normalized.contains("launchpad") || normalized.contains("pre") {
                            Some(TradeLifecycle::PreMigration)
                        } else if normalized.contains("raydium") || normalized.contains("post") {
                            Some(TradeLifecycle::PostMigration)
                        } else {
                            None
                        }
                    })
                }
                VenueWarmData::Stable { .. }
                | VenueWarmData::RaydiumAmmV4 { .. }
                | VenueWarmData::RaydiumCpmm { .. } => Some(TradeLifecycle::PostMigration),
                VenueWarmData::RaydiumLaunchLab { .. } => Some(TradeLifecycle::PreMigration),
            })
    }
}

pub fn warm_ttl_ms_for_lifecycle(lifecycle: Option<&TradeLifecycle>) -> u64 {
    match lifecycle {
        Some(TradeLifecycle::PreMigration) => PRE_MIGRATION_WARM_TTL_MS,
        Some(TradeLifecycle::PostMigration) => POST_MIGRATION_WARM_TTL_MS,
        None => UNKNOWN_WARM_TTL_MS,
    }
}

pub fn warm_ttl_ms_for_lifecycle_label(value: Option<&str>) -> u64 {
    let lifecycle = match value.map(str::trim).map(|value| value.to_ascii_lowercase()) {
        Some(label)
            if matches!(
                label.as_str(),
                "pre_bond" | "pre-migration" | "pre_migration" | "launchpad"
            ) =>
        {
            Some(TradeLifecycle::PreMigration)
        }
        Some(label)
            if matches!(
                label.as_str(),
                "on_amm" | "migrated" | "post-migration" | "post_migration"
            ) =>
        {
            Some(TradeLifecycle::PostMigration)
        }
        _ => None,
    };
    warm_ttl_ms_for_lifecycle(lifecycle.as_ref())
}

/// Fingerprint used both as the cache key and as the `warm_key` string
/// returned to clients. Captures every input that would change the
/// compile route so two incompatible settings never collide on one
/// warm entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WarmFingerprint {
    pub mint: String,
    pub pinned_pool: Option<String>,
    pub rpc_url: String,
    pub commitment: String,
    pub route_policy: String,
    pub allow_non_canonical: bool,
}

impl WarmFingerprint {
    /// Stable short string for use as an opaque `warm_key`. Deliberately
    /// not a hash — it's easier to diagnose in logs/metrics if we can
    /// see which settings produced the key.
    pub fn as_warm_key(&self) -> String {
        format!(
            "mint={}|pool={}|rpc={}|cmt={}|policy={}|nc={}",
            self.mint,
            self.pinned_pool.as_deref().unwrap_or("-"),
            self.rpc_url,
            self.commitment,
            self.route_policy,
            if self.allow_non_canonical { "1" } else { "0" },
        )
    }
}

/// Process-wide mint warm cache.
#[derive(Default)]
pub struct MintWarmCache {
    entries: RwLock<HashMap<WarmFingerprint, PrewarmedMint>>,
    /// Per-fingerprint mutexes used for single-flight warming. When two
    /// concurrent `current_or_warm` calls arrive for the same
    /// fingerprint, only one performs the underlying RPC work.
    flight_locks: Mutex<HashMap<WarmFingerprint, Arc<Mutex<()>>>>,
}

static SHARED_MINT_WARM_CACHE: OnceLock<MintWarmCache> = OnceLock::new();

pub fn shared_mint_warm_cache() -> &'static MintWarmCache {
    SHARED_MINT_WARM_CACHE.get_or_init(MintWarmCache::default)
}

impl MintWarmCache {
    /// Read a warm entry for the given fingerprint if it is still fresh.
    /// Updates `last_used_at_unix_ms` as a side effect so rolling TTL works.
    pub async fn current(&self, fingerprint: &WarmFingerprint) -> Option<PrewarmedMint> {
        let now = now_unix_ms();
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(fingerprint) {
            if entry.is_stale(now) {
                entries.remove(fingerprint);
                return None;
            }
            entry.last_used_at_unix_ms = now;
            return Some(entry.clone());
        }
        None
    }

    /// Read a warm entry by its opaque `warm_key`. This is a slower scan than
    /// direct fingerprint lookup, so callers should prefer `current(..)` when
    /// they can build the exact fingerprint. The click path uses this to reuse
    /// a specific `/prewarm` result even after request normalization.
    pub async fn current_by_warm_key(&self, warm_key: &str) -> Option<PrewarmedMint> {
        let normalized = warm_key.trim();
        if normalized.is_empty() {
            return None;
        }
        let now = now_unix_ms();
        let mut entries = self.entries.write().await;
        let stale_keys = entries
            .iter()
            .filter_map(|(key, value)| value.is_stale(now).then_some(key.clone()))
            .collect::<Vec<_>>();
        for key in stale_keys {
            entries.remove(&key);
        }
        for entry in entries.values_mut() {
            if entry.warm_key == normalized {
                entry.last_used_at_unix_ms = now;
                return Some(entry.clone());
            }
        }
        None
    }

    /// Insert / replace a warm entry. Enforces the LRU cap by evicting
    /// the least-recently-used entry when the cap is exceeded.
    pub async fn insert(&self, fingerprint: WarmFingerprint, entry: PrewarmedMint) {
        let mut entries = self.entries.write().await;
        entries.insert(fingerprint, entry);
        if entries.len() > WARM_LRU_CAP {
            // Find + drop the oldest `last_used_at_unix_ms`.
            if let Some(victim_key) = entries
                .iter()
                .min_by_key(|(_, value)| value.last_used_at_unix_ms)
                .map(|(key, _)| key.clone())
            {
                entries.remove(&victim_key);
            }
        }
    }

    /// Explicit invalidation — used by the trade path after a
    /// confirmed trade or when the compile path detects a cached
    /// selector is no longer valid.
    pub async fn invalidate(&self, fingerprint: &WarmFingerprint) {
        self.entries.write().await.remove(fingerprint);
    }

    /// Single-flight flight-lock retrieval. Callers use this to dedupe
    /// concurrent warms for the same fingerprint: the first caller
    /// holds the lock and does the RPC work, the second caller awaits
    /// and then typically finds the populated entry on its next read.
    pub async fn flight_lock(&self, fingerprint: &WarmFingerprint) -> Arc<Mutex<()>> {
        let mut locks = self.flight_locks.lock().await;
        locks
            .entry(fingerprint.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Returns the number of live entries. Test + metrics only.
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }
}

/// Build a fingerprint from a normalized request context. Kept as a
/// free function so both `/prewarm` and the trade hot path can produce
/// the same key without depending on each other's internal state.
pub fn build_fingerprint(
    mint: &str,
    pinned_pool: Option<&str>,
    rpc_url: &str,
    commitment: &str,
    route_policy: &str,
    allow_non_canonical: bool,
) -> WarmFingerprint {
    WarmFingerprint {
        mint: mint.trim().to_string(),
        pinned_pool: pinned_pool
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        rpc_url: rpc_url.trim().to_string(),
        commitment: commitment.trim().to_ascii_lowercase(),
        route_policy: route_policy.trim().to_ascii_lowercase(),
        allow_non_canonical,
    }
}

/// Helper: construct a `PrewarmedMint` from a freshly resolved plan.
pub fn prewarmed_from_plan(
    fingerprint: &WarmFingerprint,
    resolved_pair: Option<String>,
    plan: &TradeDispatchPlan,
) -> PrewarmedMint {
    let now = now_unix_ms();
    let venue = match plan.selector.family {
        crate::trade_planner::TradeVenueFamily::PumpBondingCurve
        | crate::trade_planner::TradeVenueFamily::PumpAmm => VenueWarmData::Pump {
            pinned_pool: plan
                .resolved_pinned_pool
                .clone()
                .or_else(|| fingerprint.pinned_pool.clone()),
            non_canonical: plan.non_canonical,
            lifecycle: plan.selector.lifecycle.clone(),
            market_key: plan.selector.canonical_market_key.clone(),
            mint_token_program: match plan.selector.runtime_bundle.as_ref() {
                Some(PlannerRuntimeBundle::PumpBondingCurve(bundle)) => {
                    Some(bundle.token_program.clone())
                }
                Some(PlannerRuntimeBundle::PumpAmm(bundle)) => {
                    Some(bundle.mint_token_program.clone())
                }
                _ => None,
            },
            creator: match plan.selector.runtime_bundle.as_ref() {
                Some(PlannerRuntimeBundle::PumpBondingCurve(bundle)) => {
                    Some(bundle.launch_creator.clone())
                }
                Some(PlannerRuntimeBundle::PumpAmm(bundle)) => Some(bundle.coin_creator.clone()),
                _ => None,
            },
            is_mayhem_mode: match plan.selector.runtime_bundle.as_ref() {
                Some(PlannerRuntimeBundle::PumpBondingCurve(bundle)) => bundle.is_mayhem_mode,
                Some(PlannerRuntimeBundle::PumpAmm(bundle)) => bundle.is_mayhem_mode,
                _ => false,
            },
            is_cashback_coin: match plan.selector.runtime_bundle.as_ref() {
                Some(PlannerRuntimeBundle::PumpBondingCurve(bundle)) => bundle.is_cashback_coin,
                Some(PlannerRuntimeBundle::PumpAmm(bundle)) => bundle.is_cashback_coin,
                _ => false,
            },
        },
        crate::trade_planner::TradeVenueFamily::BonkLaunchpad
        | crate::trade_planner::TradeVenueFamily::BonkRaydium => VenueWarmData::Bonk {
            quote_asset: Some(match plan.selector.quote_asset {
                crate::trade_planner::PlannerQuoteAsset::Sol => "SOL".to_string(),
                crate::trade_planner::PlannerQuoteAsset::Wsol => "WSOL".to_string(),
                crate::trade_planner::PlannerQuoteAsset::Usd1 => "USD1".to_string(),
                crate::trade_planner::PlannerQuoteAsset::Usdc => "USDC".to_string(),
                crate::trade_planner::PlannerQuoteAsset::Usdt => "USDT".to_string(),
            }),
            import_context: selector_to_bonk_import_context(&plan.selector),
        },
        crate::trade_planner::TradeVenueFamily::TrustedStableSwap => VenueWarmData::Stable {
            pool: plan
                .resolved_pinned_pool
                .clone()
                .unwrap_or_else(|| plan.selector.canonical_market_key.clone()),
            venue: plan.selector.market_subtype.clone(),
        },
        crate::trade_planner::TradeVenueFamily::RaydiumAmmV4 => VenueWarmData::RaydiumAmmV4 {
            pool: plan
                .resolved_pinned_pool
                .clone()
                .unwrap_or_else(|| plan.selector.canonical_market_key.clone()),
        },
        crate::trade_planner::TradeVenueFamily::RaydiumCpmm => VenueWarmData::RaydiumCpmm {
            pool: plan
                .resolved_pinned_pool
                .clone()
                .unwrap_or_else(|| plan.selector.canonical_market_key.clone()),
        },
        crate::trade_planner::TradeVenueFamily::RaydiumLaunchLab => {
            VenueWarmData::RaydiumLaunchLab {
                pool: plan
                    .resolved_pinned_pool
                    .clone()
                    .unwrap_or_else(|| plan.selector.canonical_market_key.clone()),
            }
        }
        crate::trade_planner::TradeVenueFamily::MeteoraDbc
        | crate::trade_planner::TradeVenueFamily::MeteoraDammV2 => VenueWarmData::Bags {
            lifecycle: Some(plan.selector.lifecycle.clone()),
            import_context: selector_to_bags_import_context(&plan.selector),
            bags_launch: Some(selector_to_bags_launch(&plan.selector)),
        },
    };
    PrewarmedMint {
        mint: fingerprint.mint.clone(),
        resolved_pair,
        warm_key: fingerprint.as_warm_key(),
        allow_non_canonical: fingerprint.allow_non_canonical,
        plan: Some(CachedPlan {
            selector: plan.selector.clone(),
            resolved_pinned_pool: plan.resolved_pinned_pool.clone(),
            non_canonical: plan.non_canonical,
        }),
        venue,
        warmed_at_unix_ms: now,
        last_used_at_unix_ms: now,
    }
}

/// Family label for a warm entry's venue, used in metrics / log lines.
pub fn venue_family_label(venue: &VenueWarmData) -> &'static str {
    match venue {
        VenueWarmData::Pump { .. } => "pump",
        VenueWarmData::Bonk { .. } => "bonk",
        VenueWarmData::Bags { .. } => "meteora",
        VenueWarmData::Stable { .. } => "stable",
        VenueWarmData::RaydiumAmmV4 { .. } => "raydium-amm-v4",
        VenueWarmData::RaydiumCpmm { .. } => "raydium-cpmm",
        VenueWarmData::RaydiumLaunchLab { .. } => "raydium-launchlab",
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

    #[test]
    fn fingerprint_warm_key_is_stable() {
        let f = build_fingerprint(
            "Mint111",
            Some("Pool222"),
            "https://rpc",
            "CONFIRMED",
            "buy:sol_only",
            false,
        );
        assert_eq!(f.commitment, "confirmed");
        let key = f.as_warm_key();
        assert!(key.contains("mint=Mint111"));
        assert!(key.contains("pool=Pool222"));
        assert!(key.contains("policy=buy:sol_only"));
        assert!(key.contains("nc=0"));
    }

    #[test]
    fn different_settings_produce_different_keys() {
        let base = build_fingerprint(
            "Mint111",
            None,
            "https://rpc",
            "confirmed",
            "buy:sol_only",
            false,
        );
        let allow_nc = build_fingerprint(
            "Mint111",
            None,
            "https://rpc",
            "confirmed",
            "buy:sol_only",
            true,
        );
        let usd1 = build_fingerprint(
            "Mint111",
            None,
            "https://rpc",
            "confirmed",
            "buy:usd1_only",
            false,
        );
        assert_ne!(base.as_warm_key(), allow_nc.as_warm_key());
        assert_ne!(base.as_warm_key(), usd1.as_warm_key());
    }

    #[test]
    fn lifecycle_ttl_is_short_pre_migration_and_long_post_migration() {
        assert_eq!(
            warm_ttl_ms_for_lifecycle(Some(&TradeLifecycle::PreMigration)),
            PRE_MIGRATION_WARM_TTL_MS
        );
        assert_eq!(
            warm_ttl_ms_for_lifecycle(Some(&TradeLifecycle::PostMigration)),
            POST_MIGRATION_WARM_TTL_MS
        );
        assert_eq!(warm_ttl_ms_for_lifecycle(None), UNKNOWN_WARM_TTL_MS);
    }

    #[test]
    fn lifecycle_label_ttl_uses_short_fallback_for_unknown_values() {
        assert_eq!(
            warm_ttl_ms_for_lifecycle_label(Some("migrated")),
            POST_MIGRATION_WARM_TTL_MS
        );
        assert_eq!(
            warm_ttl_ms_for_lifecycle_label(Some("pre_bond")),
            PRE_MIGRATION_WARM_TTL_MS
        );
        assert_eq!(
            warm_ttl_ms_for_lifecycle_label(Some("mystery")),
            UNKNOWN_WARM_TTL_MS
        );
    }

    #[tokio::test]
    async fn insert_and_current_round_trip() {
        let cache = MintWarmCache::default();
        let fingerprint = build_fingerprint(
            "Mint111",
            None,
            "https://rpc",
            "confirmed",
            "buy:sol_only",
            false,
        );
        let now = now_unix_ms();
        let entry = PrewarmedMint {
            mint: "Mint111".to_string(),
            resolved_pair: None,
            warm_key: fingerprint.as_warm_key(),
            allow_non_canonical: false,
            plan: None,
            venue: VenueWarmData::Pump {
                pinned_pool: None,
                non_canonical: false,
                lifecycle: TradeLifecycle::PreMigration,
                market_key: "Mint111".to_string(),
                mint_token_program: None,
                creator: None,
                is_mayhem_mode: false,
                is_cashback_coin: false,
            },
            warmed_at_unix_ms: now,
            last_used_at_unix_ms: now,
        };
        cache.insert(fingerprint.clone(), entry).await;
        let current = cache.current(&fingerprint).await;
        assert!(current.is_some());
        assert_eq!(current.unwrap().mint, "Mint111");
    }

    #[tokio::test]
    async fn current_by_warm_key_round_trips() {
        let cache = MintWarmCache::default();
        let fingerprint = build_fingerprint(
            "Mint111",
            Some("Pool222"),
            "https://rpc",
            "confirmed",
            "buy:usd1_only",
            false,
        );
        let now = now_unix_ms();
        cache
            .insert(
                fingerprint.clone(),
                PrewarmedMint {
                    mint: "Mint111".to_string(),
                    resolved_pair: Some("Pool222".to_string()),
                    warm_key: fingerprint.as_warm_key(),
                    allow_non_canonical: false,
                    plan: None,
                    venue: VenueWarmData::Bonk {
                        quote_asset: Some("USD1".to_string()),
                        import_context: None,
                    },
                    warmed_at_unix_ms: now,
                    last_used_at_unix_ms: now,
                },
            )
            .await;
        let current = cache.current_by_warm_key(&fingerprint.as_warm_key()).await;
        assert!(current.is_some());
        assert_eq!(current.unwrap().resolved_pair.as_deref(), Some("Pool222"));
    }

    #[test]
    fn prewarmed_entry_ttl_tracks_cached_lifecycle() {
        let now = now_unix_ms();
        let pre_entry = PrewarmedMint {
            mint: "Mint111".to_string(),
            resolved_pair: None,
            warm_key: "pre".to_string(),
            allow_non_canonical: false,
            plan: None,
            venue: VenueWarmData::Pump {
                pinned_pool: None,
                non_canonical: false,
                lifecycle: TradeLifecycle::PreMigration,
                market_key: "Mint111".to_string(),
                mint_token_program: None,
                creator: None,
                is_mayhem_mode: false,
                is_cashback_coin: false,
            },
            warmed_at_unix_ms: now,
            last_used_at_unix_ms: now,
        };
        let post_entry = PrewarmedMint {
            mint: "Mint111".to_string(),
            resolved_pair: None,
            warm_key: "post".to_string(),
            allow_non_canonical: false,
            plan: None,
            venue: VenueWarmData::Pump {
                pinned_pool: None,
                non_canonical: false,
                lifecycle: TradeLifecycle::PostMigration,
                market_key: "Mint111".to_string(),
                mint_token_program: None,
                creator: None,
                is_mayhem_mode: false,
                is_cashback_coin: false,
            },
            warmed_at_unix_ms: now,
            last_used_at_unix_ms: now,
        };

        assert_eq!(pre_entry.ttl_ms(), PRE_MIGRATION_WARM_TTL_MS);
        assert_eq!(post_entry.ttl_ms(), POST_MIGRATION_WARM_TTL_MS);
    }
}
