#![allow(non_snake_case, dead_code)]

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};

use crate::transport::{JitoBundleEndpoint, TransportPlan};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledTransaction {
    pub label: String,
    pub format: String,
    pub blockhash: String,
    pub lastValidBlockHeight: u64,
    pub serializedBase64: String,
    #[serde(default)]
    pub lookupTablesUsed: Vec<String>,
    #[serde(default)]
    pub computeUnitLimit: Option<u64>,
    #[serde(default)]
    pub computeUnitPriceMicroLamports: Option<u64>,
    #[serde(default)]
    pub inlineTipLamports: Option<u64>,
    #[serde(default)]
    pub inlineTipAccount: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationResult {
    pub label: String,
    pub format: String,
    pub err: Option<Value>,
    pub unitsConsumed: Option<u64>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentResult {
    pub label: String,
    pub format: String,
    pub signature: Option<String>,
    pub explorerUrl: Option<String>,
    pub transportType: String,
    pub endpoint: Option<String>,
    pub attemptedEndpoints: Vec<String>,
    pub skipPreflight: bool,
    pub maxRetries: u32,
    pub confirmationStatus: Option<String>,
    pub sendObservedBlockHeight: Option<u64>,
    pub confirmedObservedBlockHeight: Option<u64>,
    pub confirmedSlot: Option<u64>,
    pub computeUnitLimit: Option<u64>,
    pub computeUnitPriceMicroLamports: Option<u64>,
    pub inlineTipLamports: Option<u64>,
    pub inlineTipAccount: Option<String>,
    pub bundleId: Option<String>,
    pub attemptedBundleIds: Vec<String>,
}

struct ConfirmationDetails {
    status: Value,
    confirmed_observed_block_height: Option<u64>,
    confirmed_slot: Option<u64>,
}

async fn rpc_request(rpc_url: &str, method: &str, params: Value) -> Result<Value, String> {
    let client = Client::new();
    let response = client
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;
    let payload: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({ "raw": body }));
    if !status.is_success() {
        let detail = payload
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .or_else(|| payload.get("raw").and_then(Value::as_str))
            .unwrap_or("No response body.");
        return Err(format!(
            "RPC {} failed with status {}: {}",
            method, status, detail
        ));
    }
    if let Some(error) = payload.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("RPC request failed.")
            .to_string());
    }
    Ok(payload.get("result").cloned().unwrap_or(Value::Null))
}

pub async fn fetch_latest_blockhash(
    rpc_url: &str,
    commitment: &str,
) -> Result<(String, u64), String> {
    let result = rpc_request(
        rpc_url,
        "getLatestBlockhash",
        json!([
            {
                "commitment": commitment,
            }
        ]),
    )
    .await?;
    let value = result
        .get("value")
        .cloned()
        .unwrap_or_else(|| result.clone());
    let blockhash = value
        .get("blockhash")
        .and_then(Value::as_str)
        .ok_or_else(|| "RPC getLatestBlockhash did not return a blockhash.".to_string())?
        .to_string();
    let last_valid_block_height = value
        .get("lastValidBlockHeight")
        .and_then(Value::as_u64)
        .ok_or_else(|| "RPC getLatestBlockhash did not return lastValidBlockHeight.".to_string())?;
    Ok((blockhash, last_valid_block_height))
}

pub async fn fetch_account_data(
    rpc_url: &str,
    account: &str,
    commitment: &str,
) -> Result<Vec<u8>, String> {
    let result = rpc_request(
        rpc_url,
        "getAccountInfo",
        json!([
            account,
            {
                "encoding": "base64",
                "commitment": commitment,
            }
        ]),
    )
    .await?;
    let value = result
        .get("value")
        .ok_or_else(|| format!("RPC getAccountInfo did not return a value for {account}."))?;
    if value.is_null() {
        return Err(format!("Account {account} was not found."));
    }
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .ok_or_else(|| format!("RPC getAccountInfo returned invalid base64 data for {account}."))?;
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    BASE64.decode(data).map_err(|error| error.to_string())
}

pub async fn fetch_account_exists(
    rpc_url: &str,
    account: &str,
    commitment: &str,
) -> Result<bool, String> {
    let result = rpc_request(
        rpc_url,
        "getAccountInfo",
        json!([
            account,
            {
                "encoding": "base64",
                "commitment": commitment,
            }
        ]),
    )
    .await?;
    Ok(result.get("value").is_some_and(|value| !value.is_null()))
}

pub async fn fetch_current_block_height(
    rpc_url: &str,
    commitment: &str,
) -> Result<u64, String> {
    let result = rpc_request(
        rpc_url,
        "getBlockHeight",
        json!([
            {
                "commitment": commitment,
            }
        ]),
    )
    .await?;
    result
        .as_u64()
        .ok_or_else(|| "RPC getBlockHeight did not return a block height.".to_string())
}

pub async fn simulate_transactions(
    rpc_url: &str,
    transactions: &[CompiledTransaction],
    commitment: &str,
) -> Result<(Vec<SimulationResult>, Vec<String>), String> {
    let mut results = Vec::new();
    let mut warnings = Vec::new();
    if transactions.len() > 1 {
        warnings.push(
            "Simulation runs each transaction independently; follow-up transactions may fail if they depend on earlier state changes."
                .to_string(),
        );
    }
    for transaction in transactions {
        let result = rpc_request(
            rpc_url,
            "simulateTransaction",
            json!([
                transaction.serializedBase64,
                {
                    "encoding": "base64",
                    "commitment": commitment,
                    "replaceRecentBlockhash": false,
                    "sigVerify": true,
                }
            ]),
        )
        .await?;
        let logs = result
            .get("value")
            .and_then(|value| value.get("logs"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        results.push(SimulationResult {
            label: transaction.label.clone(),
            format: transaction.format.clone(),
            err: result
                .get("value")
                .and_then(|value| value.get("err"))
                .cloned(),
            unitsConsumed: result
                .get("value")
                .and_then(|value| value.get("unitsConsumed"))
                .and_then(Value::as_u64),
            logs,
        });
    }
    Ok((results, warnings))
}

fn commitment_rank(commitment: &str) -> u8 {
    match commitment {
        "processed" => 1,
        "confirmed" => 2,
        "finalized" => 3,
        _ => 0,
    }
}

fn commitment_satisfied(actual: &str, required: &str) -> bool {
    commitment_rank(actual) >= commitment_rank(required)
}

async fn wait_for_confirmation(
    rpc_url: &str,
    signature: &str,
    commitment: &str,
    max_attempts: u32,
    track_confirmed_block_height: bool,
) -> Result<ConfirmationDetails, String> {
    for _ in 0..max_attempts {
        let result = rpc_request(
            rpc_url,
            "getSignatureStatuses",
            json!([
                [signature],
                { "searchTransactionHistory": true }
            ]),
        )
        .await?;
        if let Some(status) = result
            .get("value")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .cloned()
        {
            if status.is_null() {
                sleep(Duration::from_millis(1500)).await;
                continue;
            }
            let actual_commitment = status
                .get("confirmationStatus")
                .and_then(Value::as_str)
                .unwrap_or("processed");
            if status.get("err").is_some() && !status.get("err").unwrap_or(&Value::Null).is_null() {
                return Err(format!(
                    "Transaction {} failed on-chain: {}",
                    signature,
                    status.get("err").cloned().unwrap_or(Value::Null)
                ));
            }
            if commitment_satisfied(actual_commitment, commitment) {
                let confirmed_observed_block_height = if track_confirmed_block_height {
                    fetch_current_block_height(rpc_url, commitment).await.ok()
                } else {
                    None
                };
                let confirmed_slot = status.get("slot").and_then(Value::as_u64);
                return Ok(ConfirmationDetails {
                    status,
                    confirmed_observed_block_height,
                    confirmed_slot,
                });
            }
        }
        sleep(Duration::from_millis(1500)).await;
    }
    Err(format!(
        "Timed out waiting for transaction {} to reach {}.",
        signature, commitment
    ))
}

pub async fn send_transactions_sequential(
    rpc_url: &str,
    transactions: &[CompiledTransaction],
    commitment: &str,
    skip_preflight: bool,
    track_send_block_height: bool,
) -> Result<(Vec<SentResult>, Vec<String>), String> {
    let mut results = Vec::new();
    let warnings = Vec::new();
    for transaction in transactions {
        let max_retries = 3;
        let signature = rpc_request(
            rpc_url,
            "sendTransaction",
            json!([
                transaction.serializedBase64,
                {
                    "encoding": "base64",
                    "skipPreflight": skip_preflight,
                    "preflightCommitment": commitment,
                    "maxRetries": max_retries,
                }
            ]),
        )
        .await?
        .as_str()
        .ok_or_else(|| "RPC sendTransaction did not return a signature.".to_string())?
        .to_string();
        let send_observed_block_height = if track_send_block_height {
            fetch_current_block_height(rpc_url, commitment).await.ok()
        } else {
            None
        };
        let confirmation =
            wait_for_confirmation(rpc_url, &signature, commitment, 20, track_send_block_height)
                .await?;
        results.push(SentResult {
            label: transaction.label.clone(),
            format: transaction.format.clone(),
            signature: Some(signature.clone()),
            explorerUrl: Some(format!("https://solscan.io/tx/{signature}")),
            transportType: "standard-rpc-sequential".to_string(),
            endpoint: Some(rpc_url.to_string()),
            attemptedEndpoints: vec![rpc_url.to_string()],
            skipPreflight: skip_preflight,
            maxRetries: max_retries,
            confirmationStatus: confirmation
                .status
                .get("confirmationStatus")
                .and_then(Value::as_str)
                .map(str::to_string),
            sendObservedBlockHeight: send_observed_block_height,
            confirmedObservedBlockHeight: confirmation.confirmed_observed_block_height,
            confirmedSlot: confirmation.confirmed_slot,
            computeUnitLimit: transaction.computeUnitLimit,
            computeUnitPriceMicroLamports: transaction.computeUnitPriceMicroLamports,
            inlineTipLamports: transaction.inlineTipLamports,
            inlineTipAccount: transaction.inlineTipAccount.clone(),
            bundleId: None,
            attemptedBundleIds: vec![],
        });
    }
    Ok((results, warnings))
}

fn validate_helius_sender_transaction(transaction: &CompiledTransaction) -> Result<(), String> {
    if transaction.inlineTipLamports.unwrap_or(0) < 200_000 {
        return Err(format!(
            "Transaction {} is missing the required inline Helius Sender tip.",
            transaction.label
        ));
    }
    if transaction.computeUnitPriceMicroLamports.unwrap_or(0) == 0 {
        return Err(format!(
            "Transaction {} is missing the required compute unit price for Helius Sender.",
            transaction.label
        ));
    }
    Ok(())
}

pub async fn send_transactions_helius_sender(
    rpc_url: &str,
    endpoints: &[String],
    transactions: &[CompiledTransaction],
    commitment: &str,
    track_send_block_height: bool,
) -> Result<(Vec<SentResult>, Vec<String>), String> {
    let mut results = Vec::new();
    let mut warnings = Vec::new();
    if endpoints.is_empty() {
        return Err("Helius Sender endpoint is not configured.".to_string());
    }
    for transaction in transactions {
        validate_helius_sender_transaction(transaction)?;
        let mut successful_endpoints = Vec::new();
        let mut signatures = Vec::new();
        let mut errors = Vec::new();
        for endpoint in endpoints {
            match rpc_request(
                endpoint,
                "sendTransaction",
                json!([
                    transaction.serializedBase64,
                    {
                        "encoding": "base64",
                        "skipPreflight": true,
                        "maxRetries": 0,
                    }
                ]),
            )
            .await
            {
                Ok(result) => {
                    let signature = result
                        .as_str()
                        .ok_or_else(|| "Helius Sender did not return a signature.".to_string())?
                        .to_string();
                    successful_endpoints.push(endpoint.clone());
                    signatures.push(signature);
                }
                Err(error) => errors.push(format!("{endpoint}: {error}")),
            }
        }
        if signatures.is_empty() {
            return Err(format!(
                "Helius Sender failed for all endpoints in the selected profile: {}",
                errors.join(" | ")
            ));
        }
        if !errors.is_empty() {
            warnings.push(format!(
                "Helius Sender fanout had partial failures for {}: {}",
                transaction.label,
                errors.join(" | ")
            ));
        }
        let signature = signatures
            .first()
            .cloned()
            .ok_or_else(|| "Helius Sender did not return a signature.".to_string())?;
        let send_observed_block_height = if track_send_block_height {
            fetch_current_block_height(rpc_url, commitment).await.ok()
        } else {
            None
        };
        let confirmation =
            wait_for_confirmation(rpc_url, &signature, commitment, 20, track_send_block_height)
                .await?;
        results.push(SentResult {
            label: transaction.label.clone(),
            format: transaction.format.clone(),
            signature: Some(signature.clone()),
            explorerUrl: Some(format!("https://solscan.io/tx/{signature}")),
            transportType: "helius-sender".to_string(),
            endpoint: successful_endpoints.first().cloned(),
            attemptedEndpoints: endpoints.to_vec(),
            skipPreflight: true,
            maxRetries: 0,
            confirmationStatus: confirmation
                .status
                .get("confirmationStatus")
                .and_then(Value::as_str)
                .map(str::to_string),
            sendObservedBlockHeight: send_observed_block_height,
            confirmedObservedBlockHeight: confirmation.confirmed_observed_block_height,
            confirmedSlot: confirmation.confirmed_slot,
            computeUnitLimit: transaction.computeUnitLimit,
            computeUnitPriceMicroLamports: transaction.computeUnitPriceMicroLamports,
            inlineTipLamports: transaction.inlineTipLamports,
            inlineTipAccount: transaction.inlineTipAccount.clone(),
            bundleId: None,
            attemptedBundleIds: vec![],
        });
    }
    Ok((results, warnings))
}

async fn jito_request(endpoint: &str, method: &str, params: Value) -> Result<Value, String> {
    let client = Client::new();
    let response = client
        .post(endpoint)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let payload: Value = response.json().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(payload
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or(&format!(
                "Jito bundle request failed with status {}.",
                status
            ))
            .to_string());
    }
    if let Some(error) = payload.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Jito request failed.")
            .to_string());
    }
    Ok(payload.get("result").cloned().unwrap_or(Value::Null))
}

pub async fn send_transactions_bundle(
    rpc_url: &str,
    endpoints: &[JitoBundleEndpoint],
    transactions: &[CompiledTransaction],
    commitment: &str,
    track_send_block_height: bool,
) -> Result<(Vec<SentResult>, Vec<String>), String> {
    if transactions.is_empty() {
        return Ok((vec![], vec![]));
    }
    if transactions.len() > 5 {
        return Err(format!(
            "Jito bundles support at most 5 transactions. Got: {}",
            transactions.len()
        ));
    }
    if endpoints.is_empty() {
        return Err("No Jito bundle endpoints configured.".to_string());
    }
    let encoded: Vec<String> = transactions
        .iter()
        .map(|entry| entry.serializedBase64.clone())
        .collect();
    let mut attempts = Vec::new();
    let mut send_errors = Vec::new();
    for endpoint in endpoints {
        match jito_request(
            &endpoint.send,
            "sendBundle",
            json!([encoded, { "encoding": "base64" }]),
        )
        .await
        {
            Ok(result) => {
                let bundle_id = result
                    .as_str()
                    .ok_or_else(|| "Jito sendBundle did not return a bundle id.".to_string())?
                    .to_string();
                attempts.push((endpoint.clone(), bundle_id));
            }
            Err(error) => send_errors.push(format!("{}: {}", endpoint.name, error)),
        }
    }
    if attempts.is_empty() {
        return Err(format!(
            "Jito bundle submission failed for all endpoints in the selected profile: {}",
            send_errors.join(" | ")
        ));
    }
    let mut warnings = Vec::new();
    if !send_errors.is_empty() {
        warnings.push(format!(
            "Jito fanout had partial submission failures: {}",
            send_errors.join(" | ")
        ));
    }
    let send_observed_block_height = if track_send_block_height {
        fetch_current_block_height(rpc_url, commitment).await.ok()
    } else {
        None
    };

    for _ in 0..20 {
        let mut observed_errors = Vec::new();
        for (endpoint, bundle_id) in &attempts {
            let status_payload = jito_request(
                &endpoint.status,
                "getBundleStatuses",
                json!([[bundle_id.clone()]]),
            )
            .await?;
            let maybe_status = status_payload
                .get("value")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items.iter().find(|entry| {
                        entry.get("bundle_id").and_then(Value::as_str) == Some(bundle_id.as_str())
                    })
                })
                .cloned();
            if let Some(status) = maybe_status {
                if let Some(err) = status.get("err") {
                    if !err.is_null() {
                        observed_errors.push(format!("{}:{}={}", endpoint.name, bundle_id, err));
                        continue;
                    }
                }
                let actual = status
                    .get("confirmation_status")
                    .and_then(Value::as_str)
                    .unwrap_or("processed");
                if !commitment_satisfied(actual, commitment) {
                    continue;
                }
                let signatures = status
                    .get("transactions")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let attempted_endpoints = attempts
                    .iter()
                    .map(|(attempt_endpoint, _)| attempt_endpoint.send.clone())
                    .collect::<Vec<_>>();
                let attempted_bundle_ids = attempts
                    .iter()
                    .map(|(_, attempt_bundle_id)| attempt_bundle_id.clone())
                    .collect::<Vec<_>>();
                let confirmed_observed_block_height = if track_send_block_height {
                    fetch_current_block_height(rpc_url, commitment).await.ok()
                } else {
                    None
                };
                let confirmed_slot = status.get("slot").and_then(Value::as_u64);
                let results = transactions
                    .iter()
                    .enumerate()
                    .map(|(index, transaction)| {
                        let signature = signatures
                            .get(index)
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        SentResult {
                            label: transaction.label.clone(),
                            format: transaction.format.clone(),
                            explorerUrl: signature
                                .as_ref()
                                .map(|value| format!("https://solscan.io/tx/{value}")),
                            signature,
                            transportType: "jito-bundle".to_string(),
                            endpoint: Some(endpoint.send.clone()),
                            attemptedEndpoints: attempted_endpoints.clone(),
                            skipPreflight: true,
                            maxRetries: 0,
                            confirmationStatus: Some(actual.to_string()),
                            sendObservedBlockHeight: send_observed_block_height,
                            confirmedObservedBlockHeight: confirmed_observed_block_height,
                            confirmedSlot: confirmed_slot,
                            computeUnitLimit: transaction.computeUnitLimit,
                            computeUnitPriceMicroLamports: transaction.computeUnitPriceMicroLamports,
                            inlineTipLamports: transaction.inlineTipLamports,
                            inlineTipAccount: transaction.inlineTipAccount.clone(),
                            bundleId: Some(bundle_id.clone()),
                            attemptedBundleIds: attempted_bundle_ids.clone(),
                        }
                    })
                    .collect::<Vec<_>>();
                warnings.push(format!(
                    "Sent via Jito bundle {} using {} after fanout to {} endpoints.",
                    bundle_id,
                    endpoint.name,
                    attempts.len()
                ));
                if !observed_errors.is_empty() {
                    warnings.push(format!(
                        "Jito fanout observed non-winning endpoint errors: {}",
                        observed_errors.join(" | ")
                    ));
                }
                return Ok((results, warnings));
            }
        }
        sleep(Duration::from_millis(1500)).await;
    }
    Err(format!(
        "Timed out waiting for fanout Jito bundle submissions to reach {}. Bundle ids: {}",
        commitment,
        attempts
            .iter()
            .map(|(_, bundle_id)| bundle_id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

pub async fn send_transactions_for_transport(
    rpc_url: &str,
    transport_plan: &TransportPlan,
    transactions: &[CompiledTransaction],
    commitment: &str,
    skip_preflight: bool,
    track_send_block_height: bool,
) -> Result<(Vec<SentResult>, Vec<String>), String> {
    match transport_plan.transportType.as_str() {
        "jito-bundle" => {
            send_transactions_bundle(
                rpc_url,
                &transport_plan.jitoBundleEndpoints,
                transactions,
                commitment,
                track_send_block_height,
            )
            .await
        }
        "helius-sender" => {
            send_transactions_helius_sender(
                rpc_url,
                &transport_plan.heliusSenderEndpoints,
                transactions,
                commitment,
                track_send_block_height,
            )
            .await
        }
        _ => {
            send_transactions_sequential(
                rpc_url,
                transactions,
                commitment,
                skip_preflight,
                track_send_block_height,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::State, routing::post};
    use serde_json::json;
    use std::{net::SocketAddr, sync::Arc};
    use tokio::sync::Mutex;

    fn compiled_tx(label: &str) -> CompiledTransaction {
        CompiledTransaction {
            label: label.to_string(),
            format: "legacy".to_string(),
            blockhash: "test-blockhash".to_string(),
            lastValidBlockHeight: 123,
            serializedBase64: "AQID".to_string(),
            lookupTablesUsed: vec![],
            computeUnitLimit: Some(1_000_000),
            computeUnitPriceMicroLamports: Some(1),
            inlineTipLamports: Some(200_000),
            inlineTipAccount: Some("tip-account".to_string()),
        }
    }

    async fn start_jsonrpc_server() -> SocketAddr {
        async fn handler(Json(payload): Json<Value>) -> Json<Value> {
            let method = payload
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let response = match method {
                "getBlockHeight" => json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": 345678
                }),
                "simulateTransaction" => json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "value": {
                            "err": null,
                            "unitsConsumed": 4242,
                            "logs": ["program log: ok"]
                        }
                    }
                }),
                "sendTransaction" => json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "sig-test-123"
                }),
                "getSignatureStatuses" => json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "value": [{
                            "confirmationStatus": "confirmed",
                            "err": null,
                            "slot": 456789
                        }]
                    }
                }),
                _ => json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "error": { "message": format!("unsupported method: {method}") }
                }),
            };
            Json(response)
        }

        let app = Router::new().route("/", post(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test rpc listener");
        let addr = listener.local_addr().expect("read local addr");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve rpc test app");
        });
        addr
    }

    async fn start_jito_server(
        send_calls: Arc<Mutex<Vec<Value>>>,
        status_calls: Arc<Mutex<Vec<Value>>>,
        bundle_id: &'static str,
    ) -> (SocketAddr, SocketAddr) {
        async fn send_handler(
            State((send_calls, bundle_id)): State<(Arc<Mutex<Vec<Value>>>, &'static str)>,
            Json(payload): Json<Value>,
        ) -> Json<Value> {
            send_calls.lock().await.push(payload);
            Json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": bundle_id
            }))
        }

        async fn status_handler(
            State((status_calls, bundle_id)): State<(Arc<Mutex<Vec<Value>>>, &'static str)>,
            Json(payload): Json<Value>,
        ) -> Json<Value> {
            status_calls.lock().await.push(payload);
            Json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "value": [{
                        "bundle_id": bundle_id,
                        "confirmation_status": "confirmed",
                        "err": null,
                        "slot": 567890,
                        "transactions": ["sig-1", "sig-2"]
                    }]
                }
            }))
        }

        let send_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind jito send listener");
        let send_addr = send_listener.local_addr().expect("read send addr");
        tokio::spawn(async move {
            let app = Router::new()
                .route("/api/v1/bundles", post(send_handler))
                .with_state((send_calls, bundle_id));
            axum::serve(send_listener, app)
                .await
                .expect("serve jito send app");
        });

        let status_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind jito status listener");
        let status_addr = status_listener.local_addr().expect("read status addr");
        tokio::spawn(async move {
            let app = Router::new()
                .route("/api/v1/getBundleStatuses", post(status_handler))
                .with_state((status_calls, bundle_id));
            axum::serve(status_listener, app)
                .await
                .expect("serve jito status app");
        });

        (send_addr, status_addr)
    }

    async fn start_sender_server(calls: Arc<Mutex<Vec<Value>>>) -> SocketAddr {
        async fn handler(
            State(calls): State<Arc<Mutex<Vec<Value>>>>,
            Json(payload): Json<Value>,
        ) -> Json<Value> {
            calls.lock().await.push(payload);
            Json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "sender-sig-123"
            }))
        }

        let app = Router::new()
            .route("/", post(handler))
            .with_state(calls);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sender listener");
        let addr = listener.local_addr().expect("read sender addr");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve sender app");
        });
        addr
    }

    #[tokio::test]
    async fn simulates_transactions_via_rpc() {
        let addr = start_jsonrpc_server().await;
        let rpc_url = format!("http://{addr}/");
        let (results, warnings) =
            simulate_transactions(&rpc_url, &[compiled_tx("launch")], "confirmed")
                .await
                .expect("simulate should succeed");
        assert!(warnings.is_empty());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].label, "launch");
        assert_eq!(results[0].unitsConsumed, Some(4242));
        assert_eq!(results[0].logs, vec!["program log: ok".to_string()]);
    }

    #[tokio::test]
    async fn sends_transactions_sequentially_via_rpc() {
        let addr = start_jsonrpc_server().await;
        let rpc_url = format!("http://{addr}/");
        let (results, warnings) =
            send_transactions_sequential(
                &rpc_url,
                &[compiled_tx("launch")],
                "confirmed",
                false,
                true,
            )
                .await
                .expect("send should succeed");
        assert!(warnings.is_empty());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].signature.as_deref(), Some("sig-test-123"));
        assert_eq!(results[0].sendObservedBlockHeight, Some(345678));
        assert_eq!(results[0].confirmedObservedBlockHeight, Some(345678));
        assert_eq!(results[0].confirmedSlot, Some(456789));
        assert_eq!(
            results[0].explorerUrl.as_deref(),
            Some("https://solscan.io/tx/sig-test-123")
        );
    }

    #[tokio::test]
    async fn sends_bundle_transactions_via_jito_endpoints() {
        let send_calls_one = Arc::new(Mutex::new(Vec::new()));
        let status_calls_one = Arc::new(Mutex::new(Vec::new()));
        let send_calls_two = Arc::new(Mutex::new(Vec::new()));
        let status_calls_two = Arc::new(Mutex::new(Vec::new()));
        let (send_addr_one, status_addr_one) =
            start_jito_server(send_calls_one.clone(), status_calls_one.clone(), "bundle-123").await;
        let (send_addr_two, status_addr_two) =
            start_jito_server(send_calls_two.clone(), status_calls_two.clone(), "bundle-456").await;
        let endpoints = vec![
            JitoBundleEndpoint {
                name: "local-one".to_string(),
                send: format!("http://{send_addr_one}/api/v1/bundles"),
                status: format!("http://{status_addr_one}/api/v1/getBundleStatuses"),
            },
            JitoBundleEndpoint {
                name: "local-two".to_string(),
                send: format!("http://{send_addr_two}/api/v1/bundles"),
                status: format!("http://{status_addr_two}/api/v1/getBundleStatuses"),
            },
        ];
        let transactions = vec![compiled_tx("launch"), compiled_tx("jito-tip")];
        let rpc_addr = start_jsonrpc_server().await;
        let rpc_url = format!("http://{rpc_addr}/");
        let (results, warnings) =
            send_transactions_bundle(&rpc_url, &endpoints, &transactions, "confirmed", true)
                .await
                .expect("bundle send should succeed");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].signature.as_deref(), Some("sig-1"));
        assert_eq!(results[1].signature.as_deref(), Some("sig-2"));
        assert_eq!(results[0].sendObservedBlockHeight, Some(345678));
        assert_eq!(results[0].confirmedObservedBlockHeight, Some(345678));
        assert_eq!(results[0].confirmedSlot, Some(567890));
        assert!(warnings.iter().any(|warning| warning.contains("fanout to 2 endpoints")));
        assert!(warnings.iter().any(|warning| warning.contains("Sent via Jito bundle")));
        assert_eq!(results[0].attemptedEndpoints.len(), 2);
        assert_eq!(results[0].attemptedBundleIds.len(), 2);
        assert_eq!(send_calls_one.lock().await.len(), 1);
        assert_eq!(send_calls_two.lock().await.len(), 1);
        assert!(!status_calls_one.lock().await.is_empty() || !status_calls_two.lock().await.is_empty());
    }

    #[tokio::test]
    async fn sends_transactions_via_helius_sender_with_required_flags() {
        let rpc_addr = start_jsonrpc_server().await;
        let calls_one = Arc::new(Mutex::new(Vec::new()));
        let calls_two = Arc::new(Mutex::new(Vec::new()));
        let sender_addr_one = start_sender_server(calls_one.clone()).await;
        let sender_addr_two = start_sender_server(calls_two.clone()).await;
        let rpc_url = format!("http://{rpc_addr}/");
        let endpoint_one = format!("http://{sender_addr_one}/");
        let endpoint_two = format!("http://{sender_addr_two}/");
        let (results, warnings) = send_transactions_helius_sender(
            &rpc_url,
            &[endpoint_one.clone(), endpoint_two.clone()],
            &[compiled_tx("launch")],
            "confirmed",
            true,
        )
        .await
        .expect("sender send should succeed");
        assert!(warnings.is_empty());
        assert_eq!(results[0].signature.as_deref(), Some("sender-sig-123"));
        assert_eq!(results[0].sendObservedBlockHeight, Some(345678));
        assert_eq!(results[0].confirmedObservedBlockHeight, Some(345678));
        assert_eq!(results[0].confirmedSlot, Some(456789));
        assert_eq!(results[0].attemptedEndpoints.len(), 2);

        let recorded = calls_one.lock().await;
        assert_eq!(recorded.len(), 1);
        let params = recorded[0]
            .get("params")
            .and_then(Value::as_array)
            .expect("sender params");
        let options = params[1].as_object().expect("sender options");
        assert_eq!(options.get("skipPreflight"), Some(&Value::Bool(true)));
        assert_eq!(options.get("maxRetries"), Some(&Value::from(0)));
        assert_eq!(calls_two.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn helius_sender_rejects_transactions_without_inline_tip() {
        let rpc_addr = start_jsonrpc_server().await;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sender_addr = start_sender_server(calls).await;
        let rpc_url = format!("http://{rpc_addr}/");
        let endpoint = format!("http://{sender_addr}/");
        let mut transaction = compiled_tx("launch");
        transaction.inlineTipLamports = None;
        let error = send_transactions_helius_sender(
            &rpc_url,
            &[endpoint.clone()],
            &[transaction],
            "confirmed",
            false,
        )
        .await
        .expect_err("sender should reject missing inline tip");
        assert!(error.contains("required inline Helius Sender tip"));
    }
}
