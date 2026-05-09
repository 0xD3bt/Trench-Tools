use serde_json::{Value, json};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex},
    time::Instant,
};

tokio::task_local! {
    static ACTIVE_ROUTE_METRICS: Arc<RouteMetricsScope>;
}

#[derive(Debug)]
struct RouteMetricsScope {
    started_at: Instant,
    rpc_methods: Mutex<BTreeMap<String, u64>>,
    phases_ms: Mutex<BTreeMap<String, u128>>,
    account_owner_data: Mutex<BTreeMap<String, Option<(Pubkey, Vec<u8>)>>>,
}

#[derive(Debug, Clone)]
pub struct RouteMetricsSnapshot {
    pub elapsed_ms: u128,
    pub rpc_methods: BTreeMap<String, u64>,
    pub phases_ms: BTreeMap<String, u128>,
}

impl RouteMetricsScope {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            rpc_methods: Mutex::new(BTreeMap::new()),
            phases_ms: Mutex::new(BTreeMap::new()),
            account_owner_data: Mutex::new(BTreeMap::new()),
        }
    }

    fn record_rpc_method(&self, method: &str) {
        if let Ok(mut methods) = self.rpc_methods.lock() {
            *methods.entry(method.to_string()).or_insert(0) += 1;
        }
    }

    fn record_phase_ms(&self, phase: &str, elapsed_ms: u128) {
        if let Ok(mut phases) = self.phases_ms.lock() {
            *phases.entry(phase.to_string()).or_insert(0) += elapsed_ms;
        }
    }

    fn snapshot(&self) -> RouteMetricsSnapshot {
        RouteMetricsSnapshot {
            elapsed_ms: self.started_at.elapsed().as_millis(),
            rpc_methods: self
                .rpc_methods
                .lock()
                .map(|methods| methods.clone())
                .unwrap_or_default(),
            phases_ms: self
                .phases_ms
                .lock()
                .map(|phases| phases.clone())
                .unwrap_or_default(),
        }
    }

    fn account_cache_key(address: &str, commitment: &str) -> String {
        format!(
            "{}:{}",
            commitment.trim().to_ascii_lowercase(),
            address.trim()
        )
    }

    fn get_account_owner_data(
        &self,
        address: &str,
        commitment: &str,
    ) -> Option<Option<(Pubkey, Vec<u8>)>> {
        self.account_owner_data.lock().ok().and_then(|accounts| {
            accounts
                .get(&Self::account_cache_key(address, commitment))
                .cloned()
        })
    }

    fn insert_account_owner_data(
        &self,
        address: &str,
        commitment: &str,
        value: Option<(Pubkey, Vec<u8>)>,
    ) {
        if let Ok(mut accounts) = self.account_owner_data.lock() {
            accounts.insert(Self::account_cache_key(address, commitment), value);
        }
    }
}

impl RouteMetricsSnapshot {
    pub fn rpc_total(&self) -> u64 {
        self.rpc_methods.values().sum()
    }

    pub fn rpc_methods_json(&self) -> Value {
        json!(self.rpc_methods)
    }

    pub fn phases_json(&self) -> Value {
        json!(self.phases_ms)
    }
}

pub async fn collect_route_metrics<F, T>(future: F) -> (T, RouteMetricsSnapshot)
where
    F: Future<Output = T>,
{
    let scope = Arc::new(RouteMetricsScope::new());
    let output = ACTIVE_ROUTE_METRICS.scope(scope.clone(), future).await;
    (output, scope.snapshot())
}

pub fn record_rpc_method(method: &str) {
    let _ = ACTIVE_ROUTE_METRICS.try_with(|scope| {
        scope.record_rpc_method(method);
    });
}

pub fn record_phase_ms(phase: &str, elapsed_ms: u128) {
    let normalized = phase.trim();
    if normalized.is_empty() {
        return;
    }
    let _ = ACTIVE_ROUTE_METRICS.try_with(|scope| {
        scope.record_phase_ms(normalized, elapsed_ms);
    });
}

pub fn cached_account_owner_data(
    address: &str,
    commitment: &str,
) -> Option<Option<(Pubkey, Vec<u8>)>> {
    ACTIVE_ROUTE_METRICS
        .try_with(|scope| scope.get_account_owner_data(address, commitment))
        .ok()
        .flatten()
}

pub fn cache_account_owner_data(address: &str, commitment: &str, value: Option<(Pubkey, Vec<u8>)>) {
    let _ = ACTIVE_ROUTE_METRICS.try_with(|scope| {
        scope.insert_account_owner_data(address, commitment, value);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scoped_metrics_collect_rpc_counts_and_account_cache() {
        let ((), snapshot) = collect_route_metrics(async {
            record_rpc_method("getAccountInfo");
            record_rpc_method("getAccountInfo");
            record_rpc_method("getMultipleAccounts");
            cache_account_owner_data("Account111", "confirmed", None);
            assert!(cached_account_owner_data("Account111", "confirmed").is_some());
        })
        .await;

        assert_eq!(snapshot.rpc_total(), 3);
        assert_eq!(snapshot.rpc_methods.get("getAccountInfo"), Some(&2));
        assert_eq!(snapshot.rpc_methods.get("getMultipleAccounts"), Some(&1));
    }
}
