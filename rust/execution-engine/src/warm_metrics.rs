//! Lightweight warm-path metrics.
//!
//! This module collects per-family counters for the four outcomes the
//! unified warm plan cares about so we can later decide whether a
//! bigger architectural merge is warranted:
//!
//! 1. `mint_warm_hit`  — prewarm entry served the trade's routing plan.
//! 2. `static_cache_hit` — hot-path RPC was skipped by a tier-1 cache.
//! 3. `classifier_fallback` — pair-to-mint classifier had to run on the
//!    click path.
//! 4. `cold_path` — nothing was warm and full discovery ran.
//!
//! Everything is `AtomicU64` so reads + writes are lock-free on the hot
//! path. The `/api/extension/prewarm` response and the `/runtime-status`
//! payload can both render this snapshot as JSON.

use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

use serde_json::{Value, json};

#[derive(Default)]
pub struct FamilyCounters {
    pub mint_warm_hit: AtomicU64,
    pub static_cache_hit: AtomicU64,
    pub classifier_fallback: AtomicU64,
    pub cold_path: AtomicU64,
    pub non_canonical_refused: AtomicU64,
    pub cold_latency_ms_sum: AtomicU64,
    pub cold_latency_count: AtomicU64,
    pub warm_latency_ms_sum: AtomicU64,
    pub warm_latency_count: AtomicU64,
}

#[derive(Default)]
pub struct WarmMetricsRegistry {
    pub pump: FamilyCounters,
    pub bonk: FamilyCounters,
    pub bags: FamilyCounters,
    pub other: FamilyCounters,
    pub unresolved: FamilyCounters,
    pub prewarm_requests: AtomicU64,
    pub prewarm_plan_ok: AtomicU64,
    pub prewarm_plan_err: AtomicU64,
}

static WARM_METRICS: OnceLock<WarmMetricsRegistry> = OnceLock::new();

pub fn shared_warm_metrics() -> &'static WarmMetricsRegistry {
    WARM_METRICS.get_or_init(WarmMetricsRegistry::default)
}

/// Canonical family label used both for metrics bucketing and for the
/// warm-key fingerprint.
#[derive(Debug, Clone, Copy)]
pub enum FamilyBucket {
    Pump,
    Bonk,
    Bags,
    /// Used for resolved families outside the primary launch venues.
    Other,
    /// Used for terminal dispatcher errors where no planner succeeded
    /// and we therefore don't know the family. Kept as a distinct bucket
    /// so "nothing resolved" is countable without polluting `Other`.
    Unresolved,
}

impl FamilyBucket {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pump => "pump",
            Self::Bonk => "bonk",
            Self::Bags => "bags",
            Self::Other => "other",
            Self::Unresolved => "unresolved",
        }
    }

    /// Derive the metrics bucket from the resolved `TradeVenueFamily`.
    pub fn from_venue_family(family: &crate::trade_planner::TradeVenueFamily) -> Self {
        use crate::trade_planner::TradeVenueFamily;
        match family {
            TradeVenueFamily::PumpBondingCurve | TradeVenueFamily::PumpAmm => Self::Pump,
            TradeVenueFamily::RaydiumAmmV4 => Self::Other,
            TradeVenueFamily::BonkLaunchpad | TradeVenueFamily::BonkRaydium => Self::Bonk,
            TradeVenueFamily::TrustedStableSwap => Self::Other,
            TradeVenueFamily::MeteoraDbc | TradeVenueFamily::MeteoraDammV2 => Self::Bags,
        }
    }
}

impl WarmMetricsRegistry {
    pub fn counters(&self, bucket: FamilyBucket) -> &FamilyCounters {
        match bucket {
            FamilyBucket::Pump => &self.pump,
            FamilyBucket::Bonk => &self.bonk,
            FamilyBucket::Bags => &self.bags,
            FamilyBucket::Other => &self.other,
            FamilyBucket::Unresolved => &self.unresolved,
        }
    }

    /// Record that dispatch produced no venue (final Err path). Use for
    /// the "we probed everything and nothing matched" branch.
    pub fn record_unresolved(&self) {
        self.unresolved.cold_path.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_mint_warm_hit(&self, bucket: FamilyBucket) {
        self.counters(bucket)
            .mint_warm_hit
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_static_cache_hit(&self, bucket: FamilyBucket) {
        self.counters(bucket)
            .static_cache_hit
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_classifier_fallback(&self, bucket: FamilyBucket) {
        self.counters(bucket)
            .classifier_fallback
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_cold_path(&self, bucket: FamilyBucket) {
        self.counters(bucket)
            .cold_path
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_non_canonical_refused(&self, bucket: FamilyBucket) {
        self.counters(bucket)
            .non_canonical_refused
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_trade_latency(&self, bucket: FamilyBucket, warm_hit: bool, latency_ms: u64) {
        let counters = self.counters(bucket);
        if warm_hit {
            counters
                .warm_latency_ms_sum
                .fetch_add(latency_ms, Ordering::Relaxed);
            counters.warm_latency_count.fetch_add(1, Ordering::Relaxed);
        } else {
            counters
                .cold_latency_ms_sum
                .fetch_add(latency_ms, Ordering::Relaxed);
            counters.cold_latency_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_prewarm_request(&self, planned_ok: bool) {
        self.prewarm_requests.fetch_add(1, Ordering::Relaxed);
        if planned_ok {
            self.prewarm_plan_ok.fetch_add(1, Ordering::Relaxed);
        } else {
            self.prewarm_plan_err.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Snapshot metrics as JSON for `/runtime-status` or diagnostic
    /// responses. Uses `Relaxed` reads because small skew across
    /// counters doesn't matter for humans reading the dashboard.
    pub fn snapshot(&self) -> Value {
        fn snap_family(label: &str, counters: &FamilyCounters) -> Value {
            let cold_count = counters.cold_latency_count.load(Ordering::Relaxed);
            let cold_sum = counters.cold_latency_ms_sum.load(Ordering::Relaxed);
            let warm_count = counters.warm_latency_count.load(Ordering::Relaxed);
            let warm_sum = counters.warm_latency_ms_sum.load(Ordering::Relaxed);
            json!({
                "family": label,
                "mintWarmHit": counters.mint_warm_hit.load(Ordering::Relaxed),
                "staticCacheHit": counters.static_cache_hit.load(Ordering::Relaxed),
                "classifierFallback": counters.classifier_fallback.load(Ordering::Relaxed),
                "coldPath": counters.cold_path.load(Ordering::Relaxed),
                "nonCanonicalRefused": counters.non_canonical_refused.load(Ordering::Relaxed),
                "coldLatencyAvgMs": if cold_count == 0 { 0 } else { cold_sum / cold_count },
                "warmLatencyAvgMs": if warm_count == 0 { 0 } else { warm_sum / warm_count },
                "coldLatencySamples": cold_count,
                "warmLatencySamples": warm_count,
            })
        }
        json!({
            "prewarmRequests": self.prewarm_requests.load(Ordering::Relaxed),
            "prewarmPlanOk": self.prewarm_plan_ok.load(Ordering::Relaxed),
            "prewarmPlanErr": self.prewarm_plan_err.load(Ordering::Relaxed),
            "families": [
                snap_family("pump", &self.pump),
                snap_family("bonk", &self.bonk),
                snap_family("bags", &self.bags),
                snap_family("other", &self.other),
                snap_family("unresolved", &self.unresolved),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_are_isolated_per_family() {
        let registry = WarmMetricsRegistry::default();
        registry.record_mint_warm_hit(FamilyBucket::Pump);
        registry.record_mint_warm_hit(FamilyBucket::Pump);
        registry.record_mint_warm_hit(FamilyBucket::Bonk);
        let snapshot = registry.snapshot();
        let families = snapshot.get("families").and_then(Value::as_array).unwrap();
        let pump = families
            .iter()
            .find(|entry| entry.get("family").and_then(Value::as_str) == Some("pump"))
            .unwrap();
        assert_eq!(pump.get("mintWarmHit").and_then(Value::as_u64), Some(2));
        let bonk = families
            .iter()
            .find(|entry| entry.get("family").and_then(Value::as_str) == Some("bonk"))
            .unwrap();
        assert_eq!(bonk.get("mintWarmHit").and_then(Value::as_u64), Some(1));
    }
}
