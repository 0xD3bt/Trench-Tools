#![allow(non_snake_case, dead_code)]

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};

use crate::transport::JitoBundleEndpoint;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledTransaction {
    pub label: String,
    pub format: String,
    pub blockhash: String,
    pub lastValidBlockHeight: u64,
    pub serializedBase64: String,
    #[serde(default)]
    pub lookupTablesUsed: Vec<String>,
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
    let payload: Value = response.json().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("RPC {} failed with status {}.", method, status));
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
) -> Result<Value, String> {
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
                return Ok(status);
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
) -> Result<(Vec<SentResult>, Vec<String>), String> {
    let mut results = Vec::new();
    let warnings = Vec::new();
    for transaction in transactions {
        let signature = rpc_request(
            rpc_url,
            "sendTransaction",
            json!([
                transaction.serializedBase64,
                {
                    "encoding": "base64",
                    "skipPreflight": skip_preflight,
                    "preflightCommitment": commitment,
                    "maxRetries": 3,
                }
            ]),
        )
        .await?
        .as_str()
        .ok_or_else(|| "RPC sendTransaction did not return a signature.".to_string())?
        .to_string();
        let _ = wait_for_confirmation(rpc_url, &signature, commitment, 20).await?;
        results.push(SentResult {
            label: transaction.label.clone(),
            format: transaction.format.clone(),
            signature: Some(signature.clone()),
            explorerUrl: Some(format!("https://solscan.io/tx/{signature}")),
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
    endpoints: &[JitoBundleEndpoint],
    transactions: &[CompiledTransaction],
    commitment: &str,
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
    let active_endpoint = endpoints
        .first()
        .ok_or_else(|| "No Jito bundle endpoints configured.".to_string())?;
    let encoded: Vec<String> = transactions
        .iter()
        .map(|entry| entry.serializedBase64.clone())
        .collect();
    let bundle_id = jito_request(
        &active_endpoint.send,
        "sendBundle",
        json!([encoded, { "encoding": "base64" }]),
    )
    .await?
    .as_str()
    .ok_or_else(|| "Jito sendBundle did not return a bundle id.".to_string())?
    .to_string();

    for _ in 0..20 {
        let status_payload = jito_request(
            &active_endpoint.status,
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
                    return Err(format!("Jito bundle failed: {}", err));
                }
            }
            let actual = status
                .get("confirmation_status")
                .and_then(Value::as_str)
                .unwrap_or("processed");
            if !commitment_satisfied(actual, commitment) {
                sleep(Duration::from_millis(1500)).await;
                continue;
            }
            let signatures = status
                .get("transactions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
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
                    }
                })
                .collect::<Vec<_>>();
            return Ok((
                results,
                vec![format!(
                    "Sent via Jito bundle {} using {}.",
                    bundle_id, active_endpoint.name
                )],
            ));
        }
        sleep(Duration::from_millis(1500)).await;
    }
    Err(format!(
        "Timed out waiting for Jito bundle {} to reach {}.",
        bundle_id, commitment
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::post};
    use serde_json::json;
    use std::net::SocketAddr;

    fn compiled_tx(label: &str) -> CompiledTransaction {
        CompiledTransaction {
            label: label.to_string(),
            format: "legacy".to_string(),
            blockhash: "test-blockhash".to_string(),
            lastValidBlockHeight: 123,
            serializedBase64: "AQID".to_string(),
            lookupTablesUsed: vec![],
        }
    }

    async fn start_jsonrpc_server() -> SocketAddr {
        async fn handler(Json(payload): Json<Value>) -> Json<Value> {
            let method = payload
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let response = match method {
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
                            "err": null
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

    async fn start_jito_server() -> (SocketAddr, SocketAddr) {
        async fn send_handler(Json(_payload): Json<Value>) -> Json<Value> {
            Json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "bundle-123"
            }))
        }

        async fn status_handler(Json(_payload): Json<Value>) -> Json<Value> {
            Json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "value": [{
                        "bundle_id": "bundle-123",
                        "confirmation_status": "confirmed",
                        "err": null,
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
            let app = Router::new().route("/api/v1/bundles", post(send_handler));
            axum::serve(send_listener, app)
                .await
                .expect("serve jito send app");
        });

        let status_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind jito status listener");
        let status_addr = status_listener.local_addr().expect("read status addr");
        tokio::spawn(async move {
            let app = Router::new().route("/api/v1/getBundleStatuses", post(status_handler));
            axum::serve(status_listener, app)
                .await
                .expect("serve jito status app");
        });

        (send_addr, status_addr)
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
            send_transactions_sequential(&rpc_url, &[compiled_tx("launch")], "confirmed", false)
                .await
                .expect("send should succeed");
        assert!(warnings.is_empty());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].signature.as_deref(), Some("sig-test-123"));
        assert_eq!(
            results[0].explorerUrl.as_deref(),
            Some("https://solscan.io/tx/sig-test-123")
        );
    }

    #[tokio::test]
    async fn sends_bundle_transactions_via_jito_endpoints() {
        let (send_addr, status_addr) = start_jito_server().await;
        let endpoints = vec![JitoBundleEndpoint {
            name: "local".to_string(),
            send: format!("http://{send_addr}/api/v1/bundles"),
            status: format!("http://{status_addr}/api/v1/getBundleStatuses"),
        }];
        let transactions = vec![compiled_tx("launch"), compiled_tx("jito-tip")];
        let (results, warnings) = send_transactions_bundle(&endpoints, &transactions, "confirmed")
            .await
            .expect("bundle send should succeed");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].signature.as_deref(), Some("sig-1"));
        assert_eq!(results[1].signature.as_deref(), Some("sig-2"));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Sent via Jito bundle"));
    }
}
