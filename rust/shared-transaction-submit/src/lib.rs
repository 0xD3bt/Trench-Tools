pub mod compiled_transaction_signers;
pub mod observability;
mod rpc;
pub mod transport;

pub use rpc::{
    COMPILE_BLOCKHASH_MIN_REMAINING_BLOCKS, CompiledTransaction, JitoWarmResult,
    SendTimingBreakdown, SentResult, TransactionTokenBalance, fetch_account_data,
    fetch_account_data_with_owner, fetch_latest_blockhash_cached,
    fetch_latest_blockhash_cached_with_prime, fetch_multiple_account_data,
};
pub use transport::{JitoBundleEndpoint, TransportEnvironment, TransportPlan};

pub async fn prewarm_rpc_endpoint(rpc_url: &str) -> Result<(), String> {
    rpc::prewarm_rpc_endpoint(rpc_url).await
}

pub async fn prewarm_watch_websocket_endpoint(endpoint: &str) -> Result<(), String> {
    rpc::prewarm_watch_websocket_endpoint(endpoint).await
}

pub async fn prewarm_helius_transaction_subscribe_endpoint(endpoint: &str) -> Result<(), String> {
    rpc::prewarm_helius_transaction_subscribe_endpoint(endpoint).await
}

pub async fn prewarm_hellomoon_quic_endpoint(
    endpoint: &str,
    mev_protect: bool,
    environment: &TransportEnvironment,
) -> Result<(), String> {
    transport::with_transport_environment(environment.clone(), async move {
        rpc::prewarm_hellomoon_quic_endpoint(endpoint, mev_protect).await
    })
    .await
}

pub async fn prewarm_hellomoon_bundle_endpoint(
    endpoint: &str,
    environment: &TransportEnvironment,
) -> Result<(), String> {
    transport::with_transport_environment(environment.clone(), async move {
        rpc::prewarm_hellomoon_bundle_endpoint(endpoint).await
    })
    .await
}

pub async fn prewarm_jito_bundle_endpoint(
    endpoint: &JitoBundleEndpoint,
) -> Result<JitoWarmResult, String> {
    rpc::prewarm_jito_bundle_endpoint(endpoint).await
}

pub fn precompute_transaction_signature(serialized_base64: &str) -> Option<String> {
    rpc::precompute_transaction_signature(serialized_base64)
}

pub async fn submit_independent_transactions_for_transport(
    rpc_url: &str,
    transport_plan: &TransportPlan,
    transactions: &[CompiledTransaction],
    commitment: &str,
    skip_preflight: bool,
    track_send_block_height: bool,
    environment: &TransportEnvironment,
) -> Result<(Vec<SentResult>, Vec<String>, u128), String> {
    transport::with_transport_environment(environment.clone(), async move {
        rpc::submit_independent_transactions_for_transport(
            rpc_url,
            transport_plan,
            transactions,
            commitment,
            skip_preflight,
            track_send_block_height,
        )
        .await
    })
    .await
}

pub async fn confirm_submitted_transactions_for_transport(
    rpc_url: &str,
    transport_plan: &TransportPlan,
    submitted: &mut [SentResult],
    commitment: &str,
    track_send_block_height: bool,
    environment: &TransportEnvironment,
) -> Result<(Vec<String>, u128), String> {
    transport::with_transport_environment(environment.clone(), async move {
        rpc::confirm_submitted_transactions_for_transport(
            rpc_url,
            transport_plan,
            submitted,
            commitment,
            track_send_block_height,
        )
        .await
    })
    .await
}
