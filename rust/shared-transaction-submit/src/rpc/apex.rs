//! OrbitFlare Apex QUIC submission.

use bytes::Bytes;
use orbitflare_apex::{ApexSenderClient, ClientOptions};
use shared_execution_routing::provider_tip::{APEX_TIP_ACCOUNTS, provider_required_tip_lamports};

use super::*;
use crate::transport::configured_apex_api_key;

fn apex_client_cache() -> &'static AsyncMutex<HashMap<String, ApexSenderClient>> {
    static CACHE: OnceLock<AsyncMutex<HashMap<String, ApexSenderClient>>> = OnceLock::new();
    CACHE.get_or_init(|| AsyncMutex::new(HashMap::new()))
}

fn apex_client_cache_key(endpoint: &str, api_key: &str, mev_protect: bool) -> String {
    format!("{endpoint}|{mev_protect}|{api_key}")
}

async fn cached_apex_client(
    endpoint: &str,
    api_key: &str,
    mev_protect: bool,
) -> Result<ApexSenderClient, String> {
    let key = apex_client_cache_key(endpoint, api_key, mev_protect);
    if let Some(client) = apex_client_cache().lock().await.get(&key) {
        return Ok(client.clone());
    }
    let options = ClientOptions {
        endpoint: Some(endpoint.to_string()),
        mev_protect,
        ..ClientOptions::default()
    };
    let client = ApexSenderClient::connect_with_options(options, api_key)
        .await
        .map_err(|error| format!("OrbitFlare Apex connect failed for {endpoint}: {error}"))?;
    let mut cache = apex_client_cache().lock().await;
    Ok(cache.entry(key).or_insert(client).clone())
}

async fn invalidate_apex_client(endpoint: &str, api_key: &str, mev_protect: bool) {
    let key = apex_client_cache_key(endpoint, api_key, mev_protect);
    apex_client_cache().lock().await.remove(&key);
}

pub async fn prewarm_apex_quic_endpoint(endpoint: &str, mev_protect: bool) -> Result<(), String> {
    let api_key = configured_apex_api_key();
    if api_key.is_empty() {
        return Err("OrbitFlare Apex prewarm requires APEX_API_KEY.".to_string());
    }
    cached_apex_client(endpoint, &api_key, mev_protect)
        .await
        .map(|_| ())
}

fn validate_apex_transaction(transaction: &CompiledTransaction) -> Result<(), String> {
    let minimum = provider_required_tip_lamports("apex").unwrap_or(1_000_000);
    if transaction.inlineTipLamports.unwrap_or(0) < minimum {
        return Err(format!(
            "Transaction {} is missing the required inline OrbitFlare Apex tip (minimum 0.001 SOL).",
            transaction.label
        ));
    }
    if let Some(account) = transaction.inlineTipAccount.as_deref() {
        if !APEX_TIP_ACCOUNTS.contains(&account) {
            return Err(format!(
                "Transaction {} tips {account}, which is not an OrbitFlare Apex tip account.",
                transaction.label
            ));
        }
    }
    Ok(())
}

async fn send_transaction_apex_endpoint(
    endpoint: &str,
    api_key: &str,
    mev_protect: bool,
    payload: &[u8],
) -> Result<(), String> {
    let client = cached_apex_client(endpoint, api_key, mev_protect).await?;
    if let Err(error) = client
        .send_transaction_bytes(Bytes::copy_from_slice(payload))
        .await
    {
        invalidate_apex_client(endpoint, api_key, mev_protect).await;
        return Err(format!(
            "OrbitFlare Apex send failed on {endpoint}: {error}"
        ));
    }
    Ok(())
}

async fn submit_single_transaction_apex_quic(
    endpoints: &[String],
    transaction: &CompiledTransaction,
    mev_protect: bool,
) -> Result<(SentResult, Vec<String>), String> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    validate_apex_transaction(transaction)?;
    let api_key = configured_apex_api_key();
    if api_key.is_empty() {
        return Err("OrbitFlare Apex requires APEX_API_KEY.".to_string());
    }
    let payload = BASE64
        .decode(&transaction.serializedBase64)
        .map_err(|error| {
            format!(
                "Failed to decode {} for OrbitFlare Apex: {error}",
                transaction.label
            )
        })?;
    let signature = transaction
        .signature
        .clone()
        .or_else(|| signature_from_serialized_base64(&transaction.serializedBase64))
        .ok_or_else(|| {
            format!(
                "OrbitFlare Apex could not derive a signature for {}.",
                transaction.label
            )
        })?;
    let mut endpoint_results = endpoints
        .iter()
        .cloned()
        .map(|endpoint| {
            let api_key = api_key.clone();
            let payload = payload.clone();
            async move {
                let result =
                    send_transaction_apex_endpoint(&endpoint, &api_key, mev_protect, &payload)
                        .await;
                (endpoint, result)
            }
        })
        .collect::<FuturesUnordered<_>>();
    let mut first_successful_endpoint = None;
    let mut errors = Vec::new();
    while let Some((endpoint, result)) = endpoint_results.next().await {
        match result {
            Ok(()) => {
                eprintln!(
                    "[shared-transaction-submit][apex-quic] endpoint accepted label={} endpoint={} signature={} compute_unit_price_micro_lamports={:?} inline_tip_lamports={:?} inline_tip_account={:?}",
                    transaction.label,
                    endpoint,
                    signature,
                    transaction.computeUnitPriceMicroLamports,
                    transaction.inlineTipLamports,
                    transaction.inlineTipAccount,
                );
                first_successful_endpoint = Some(endpoint);
                break;
            }
            Err(error) => {
                eprintln!(
                    "[shared-transaction-submit][apex-quic] endpoint failed label={} endpoint={} error={}",
                    transaction.label, endpoint, error
                );
                errors.push(format!("{endpoint}: {error}"));
            }
        }
    }
    if first_successful_endpoint.is_none() {
        return Err(format!(
            "OrbitFlare Apex failed for transaction {} on all attempted endpoints: {}",
            transaction.label,
            errors.join(" | ")
        ));
    }
    let mut warnings = Vec::new();
    if !errors.is_empty() {
        warnings.push(format!(
            "OrbitFlare Apex fanout had partial failures for {}: {}",
            transaction.label,
            errors.join(" | ")
        ));
    }
    Ok((
        SentResult {
            label: transaction.label.clone(),
            format: transaction.format.clone(),
            signature: Some(signature.clone()),
            explorerUrl: Some(format!("https://solscan.io/tx/{signature}")),
            transportType: "apex-quic".to_string(),
            endpoint: first_successful_endpoint,
            attemptedEndpoints: endpoints.to_vec(),
            skipPreflight: true,
            maxRetries: 0,
            confirmationStatus: None,
            confirmationSource: None,
            submittedAtMs: Some(current_time_ms()),
            firstObservedStatus: None,
            firstObservedSlot: None,
            firstObservedAtMs: None,
            confirmedAtMs: None,
            sendObservedSlot: None,
            confirmedObservedSlot: None,
            confirmedSlot: None,
            computeUnitLimit: transaction.computeUnitLimit,
            computeUnitPriceMicroLamports: transaction.computeUnitPriceMicroLamports,
            inlineTipLamports: transaction.inlineTipLamports,
            inlineTipAccount: transaction.inlineTipAccount.clone(),
            bundleId: None,
            attemptedBundleIds: vec![],
            transactionSubscribeAccountRequired:
                derive_helius_transaction_subscribe_account_required(&transaction.serializedBase64),
            postTokenBalances: vec![],
            confirmedTokenBalanceRaw: None,
            balanceWatchAccount: None,
            capturePostTokenBalances: false,
            requestFullTransactionDetails: false,
        },
        warnings,
    ))
}

pub async fn submit_transactions_apex_quic(
    rpc_url: &str,
    endpoints: &[String],
    transactions: &[CompiledTransaction],
    commitment: &str,
    track_send_block_height: bool,
    mev_protect: bool,
) -> Result<(Vec<SentResult>, Vec<String>, u128), String> {
    if endpoints.is_empty() {
        return Err("OrbitFlare Apex endpoint is not configured.".to_string());
    }
    let submit_started = std::time::Instant::now();
    let mut results = Vec::with_capacity(transactions.len());
    let mut warnings = Vec::new();
    for transaction in transactions {
        let (entry, entry_warnings) =
            submit_single_transaction_apex_quic(endpoints, transaction, mev_protect).await?;
        results.push(entry);
        warnings.extend(entry_warnings);
    }
    if track_send_block_height {
        let send_observed_slot = fetch_sampled_slot_snapshot(rpc_url, commitment).await.ok();
        for result in &mut results {
            result.sendObservedSlot = send_observed_slot;
        }
    }
    Ok((results, warnings, submit_started.elapsed().as_millis()))
}

pub async fn submit_transactions_apex_quic_parallel(
    rpc_url: &str,
    endpoints: &[String],
    transactions: &[CompiledTransaction],
    commitment: &str,
    track_send_block_height: bool,
    mev_protect: bool,
) -> Result<(Vec<SentResult>, Vec<String>, u128), String> {
    if endpoints.is_empty() {
        return Err("OrbitFlare Apex endpoint is not configured.".to_string());
    }
    let submit_started = std::time::Instant::now();
    let results = join_all(transactions.iter().map(|transaction| {
        submit_single_transaction_apex_quic(endpoints, transaction, mev_protect)
    }))
    .await;
    let mut sent = Vec::with_capacity(results.len());
    let mut warnings = Vec::new();
    for result in results {
        let (entry, entry_warnings) = result?;
        sent.push(entry);
        warnings.extend(entry_warnings);
    }
    if track_send_block_height {
        let send_observed_slot = fetch_sampled_slot_snapshot(rpc_url, commitment).await.ok();
        for result in &mut sent {
            result.sendObservedSlot = send_observed_slot;
        }
    }
    Ok((sent, warnings, submit_started.elapsed().as_millis()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transaction(tip_lamports: Option<u64>, tip_account: Option<&str>) -> CompiledTransaction {
        CompiledTransaction {
            label: "test".to_string(),
            format: "v0".to_string(),
            blockhash: String::new(),
            lastValidBlockHeight: 0,
            serializedBase64: String::new(),
            signature: None,
            lookupTablesUsed: vec![],
            computeUnitLimit: None,
            computeUnitPriceMicroLamports: None,
            inlineTipLamports: tip_lamports,
            inlineTipAccount: tip_account.map(str::to_string),
        }
    }

    #[test]
    fn apex_requires_minimum_tip() {
        assert!(validate_apex_transaction(&transaction(Some(999_999), None)).is_err());
        assert!(validate_apex_transaction(&transaction(None, None)).is_err());
        assert!(
            validate_apex_transaction(&transaction(Some(1_000_000), Some(APEX_TIP_ACCOUNTS[0])))
                .is_ok()
        );
    }

    #[test]
    fn apex_rejects_non_apex_tip_account() {
        let error = validate_apex_transaction(&transaction(
            Some(1_000_000),
            Some("moon17L6BgxXRX5uHKudAmqVF96xia9h8ygcmG2sL3F"),
        ))
        .unwrap_err();
        assert!(error.contains("not an OrbitFlare Apex tip account"));
    }

    #[test]
    fn apex_cache_key_separates_mev_and_key() {
        assert_ne!(
            apex_client_cache_key("a:7001", "k", true),
            apex_client_cache_key("a:7001", "k", false)
        );
        assert_ne!(
            apex_client_cache_key("a:7001", "k1", false),
            apex_client_cache_key("a:7001", "k2", false)
        );
    }
}
