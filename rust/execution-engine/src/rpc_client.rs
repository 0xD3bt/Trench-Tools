use base64::Engine as _;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;
use std::sync::OnceLock;
use tokio::time::{Duration, sleep};

use crate::launchdeck_bridge::{
    map_compiled_transaction_to_shared, map_sent_result, map_sent_result_to_shared,
    map_transport_plan,
};
use crate::shared_config::configured_env_value;
use crate::transport::{TransportPlan, transport_environment_snapshot};
use shared_transaction_submit::{
    confirm_submitted_transactions_for_transport as confirm_shared_submitted_transactions_for_transport,
    submit_independent_transactions_for_transport as submit_shared_independent_transactions_for_transport,
};

const DEFAULT_RPC_URL: &str = "http://127.0.0.1:8899";
const DEFAULT_COMMITMENT: &str = "confirmed";
const DEFAULT_CONFIRM_MAX_ATTEMPTS: u32 = 80;
const DEFAULT_CONFIRM_POLL_INTERVAL_MS: u64 = 500;
const TOKEN_ACCOUNT_AMOUNT_OFFSET: usize = 64;
const TOKEN_ACCOUNT_AMOUNT_LEN: usize = 8;
const ATA_BALANCE_RETRY_DELAYS_MS: [u64; 4] = [80, 160, 320, 480];

#[derive(Debug, Clone)]
pub struct TokenBalance {
    pub amount_raw: u64,
    pub decimals: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledTransaction {
    pub label: String,
    pub format: String,
    pub serialized_base64: String,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub lookup_tables_used: Vec<String>,
    #[serde(default)]
    pub compute_unit_limit: Option<u64>,
    #[serde(default)]
    pub compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    pub inline_tip_lamports: Option<u64>,
    #[serde(default)]
    pub inline_tip_account: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationResult {
    pub label: String,
    pub format: String,
    pub err: Option<Value>,
    pub units_consumed: Option<u64>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentResult {
    pub label: String,
    pub format: String,
    #[serde(default)]
    pub signature: Option<String>,
    pub transport_type: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub attempted_endpoints: Vec<String>,
    pub skip_preflight: bool,
    pub max_retries: u32,
    #[serde(default)]
    pub confirmation_status: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub bundle_id: Option<String>,
    #[serde(default)]
    pub attempted_bundle_ids: Vec<String>,
    #[serde(default)]
    pub transaction_subscribe_account_required: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTimingBreakdown {
    pub submit_ms: u128,
    pub confirm_ms: u128,
}

pub fn configured_rpc_url() -> String {
    configured_env_value(&["SOLANA_RPC_URL"]).unwrap_or_else(|| DEFAULT_RPC_URL.to_string())
}

pub fn configured_warm_rpc_url() -> String {
    configured_env_value(&["WARM_RPC_URL", "SOLANA_RPC_URL"])
        .unwrap_or_else(|| configured_rpc_url())
}

fn normalized_commitment(commitment: &str) -> String {
    let normalized = commitment.trim().to_lowercase();
    if normalized.is_empty() {
        DEFAULT_COMMITMENT.to_string()
    } else {
        normalized
    }
}

/// Shared, process-wide HTTP client for JSON-RPC requests.
///
/// Historically this function constructed a fresh `reqwest::Client` on every
/// call, which threw away TLS session state and connection pooling between
/// RPC hops — a noticeable latency tax on every trade. The client is now a
/// lazily-initialized singleton so that repeated calls reuse the same
/// connection pool, matching the shared-client pattern in `launchdeck-engine`.
///
/// The tuned builder can in principle fail (e.g. if the TLS backend is
/// missing). We fall back to `Client::new()` — which is infallible in
/// practice — so a builder rejection never panics the whole host. The
/// fallback loses pool tuning but keeps RPC traffic working.
pub fn shared_rpc_http_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        match Client::builder()
            .timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(32)
            .pool_idle_timeout(Some(Duration::from_secs(90)))
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                eprintln!(
                    "[execution-engine][rpc] tuned HTTP client build failed ({error}); \
                     falling back to default `reqwest::Client::new()`. \
                     Connection pool tuning is disabled for this process."
                );
                Client::new()
            }
        }
    })
}

/// Truncate a string to `max_chars` for logging purposes. JSON-RPC error
/// bodies from providers like Helius are usually short, but simulation
/// failure payloads can include long program logs that we don't want to
/// flood the operator's host.log with.
fn truncate_for_log(raw: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_chars));
    for (idx, ch) in raw.chars().enumerate() {
        if idx >= max_chars {
            out.push_str(&format!(
                "… <truncated; original {} chars>",
                raw.chars().count()
            ));
            break;
        }
        out.push(ch);
    }
    out
}

/// Extract a human-readable error message out of a JSON-RPC error body.
/// Handles the common Helius / Solana shapes:
///   { "error": { "code": -32002, "message": "..." } }
///   { "error": { "code": -32002, "message": "...", "data": { "logs": [...] } } }
fn format_json_rpc_error(body: &Value) -> Option<String> {
    let error = body.get("error")?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("<no message>");
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .map(|value| format!(" (code {value})"))
        .unwrap_or_default();
    // Simulation failures include a `data.logs` array with the on-chain
    // program logs — those are the most useful signal when a Helius 500 is
    // actually a program-side revert being relayed back to us.
    let logs_tail = error
        .get("data")
        .and_then(|value| value.get("logs"))
        .and_then(Value::as_array)
        .map(|items| {
            let rendered: Vec<String> = items
                .iter()
                .rev()
                .take(5)
                .rev()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
            if rendered.is_empty() {
                String::new()
            } else {
                format!(" | logs: {}", rendered.join(" // "))
            }
        })
        .unwrap_or_default();
    Some(format!("{message}{code}{logs_tail}"))
}

pub async fn rpc_request_with_client(
    client: &Client,
    rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
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
        .map_err(|error| format!("RPC {method} request failed: {error}"))?;

    let status = response.status();
    // Read the body as text so we never lose it on a non-2xx status. This is
    // what makes Helius Sender 500s actually diagnosable — their body carries
    // a JSON-RPC error payload describing *why* the transaction was rejected.
    let body_text = response
        .text()
        .await
        .map_err(|error| format!("RPC {method} response read failed: {error}"))?;

    let body_json: Option<Value> = if body_text.is_empty() {
        None
    } else {
        serde_json::from_str::<Value>(&body_text).ok()
    };

    if !status.is_success() {
        let detail = body_json
            .as_ref()
            .and_then(format_json_rpc_error)
            .unwrap_or_else(|| {
                if body_text.is_empty() {
                    "<empty response body>".to_string()
                } else {
                    format!("body: {}", truncate_for_log(&body_text, 800))
                }
            });
        eprintln!(
            "[execution-engine][rpc] {method} -> {rpc_url} HTTP {} :: {detail}",
            status.as_u16()
        );
        return Err(format!("RPC {method} HTTP {} :: {detail}", status.as_u16()));
    }

    let payload = body_json.ok_or_else(|| {
        format!(
            "RPC {method} response decode failed: non-JSON body ({} chars)",
            body_text.chars().count()
        )
    })?;

    if let Some(detail) = format_json_rpc_error(&payload) {
        eprintln!("[execution-engine][rpc] {method} -> {rpc_url} JSON-RPC error :: {detail}");
        return Err(format!("RPC {method} failed: {detail}"));
    }

    payload
        .get("result")
        .cloned()
        .ok_or_else(|| format!("RPC {method} did not return a result."))
}

async fn rpc_request(rpc_url: &str, method: &str, params: Value) -> Result<Value, String> {
    rpc_request_with_client(shared_rpc_http_client(), rpc_url, method, params).await
}

pub async fn fetch_account_data(
    rpc_url: &str,
    address: &str,
    commitment: &str,
) -> Result<Vec<u8>, String> {
    let result = rpc_request(
        rpc_url,
        "getAccountInfo",
        json!([
            address,
            {
                "encoding": "base64",
                "commitment": normalized_commitment(commitment)
            }
        ]),
    )
    .await?;

    let value = result
        .get("value")
        .ok_or_else(|| format!("RPC getAccountInfo did not return account data for {address}."))?;
    if value.is_null() {
        return Err(format!("Account {address} was not found."));
    }
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!("RPC getAccountInfo returned invalid account data for {address}.")
        })?;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| format!("Failed to decode account data for {address}: {error}"))
}

pub async fn fetch_account_exists(
    rpc_url: &str,
    address: &str,
    commitment: &str,
) -> Result<bool, String> {
    let result = rpc_request(
        rpc_url,
        "getAccountInfo",
        json!([
            address,
            {
                "encoding": "base64",
                "commitment": normalized_commitment(commitment)
            }
        ]),
    )
    .await?;
    Ok(result.get("value").is_some_and(|value| !value.is_null()))
}

/// Fetch both the owning program and the raw account data for an arbitrary
/// Solana account, in a single RPC call. Used by the warm classifier to
/// distinguish an SPL Token mint / Token-2022 mint / Pump AMM pool / other
/// program-owned account from a pasted pair / pool address.
///
/// Returns `Ok(None)` when the account does not exist.
pub async fn fetch_account_owner_and_data(
    rpc_url: &str,
    address: &str,
    commitment: &str,
) -> Result<Option<(Pubkey, Vec<u8>)>, String> {
    let result = rpc_request(
        rpc_url,
        "getAccountInfo",
        json!([
            address,
            {
                "encoding": "base64",
                "commitment": normalized_commitment(commitment)
            }
        ]),
    )
    .await?;

    let value = result
        .get("value")
        .ok_or_else(|| format!("RPC getAccountInfo did not return account data for {address}."))?;
    if value.is_null() {
        return Ok(None);
    }
    let owner_str = value
        .get("owner")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("RPC getAccountInfo did not return an owner for {address}."))?;
    let owner = Pubkey::from_str(owner_str).map_err(|error| {
        format!("RPC getAccountInfo returned an invalid owner for {address}: {error}")
    })?;
    let data_str = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .unwrap_or("");
    let data = if data_str.is_empty() {
        Vec::new()
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(data_str)
            .map_err(|error| format!("Failed to decode account data for {address}: {error}"))?
    };
    Ok(Some((owner, data)))
}

/// Fetch the current block height. Used for blockhash runway checks so we
/// don't sign against a blockhash that is about to expire.
pub async fn fetch_block_height(rpc_url: &str, commitment: &str) -> Result<u64, String> {
    let result = rpc_request(
        rpc_url,
        "getBlockHeight",
        json!([
            {
                "commitment": normalized_commitment(commitment)
            }
        ]),
    )
    .await?;
    result
        .as_u64()
        .ok_or_else(|| "RPC getBlockHeight did not return a block height.".to_string())
}

pub async fn fetch_latest_blockhash(
    rpc_url: &str,
    commitment: &str,
) -> Result<(Hash, u64), String> {
    let result = rpc_request(
        rpc_url,
        "getLatestBlockhash",
        json!([
            {
                "commitment": normalized_commitment(commitment)
            }
        ]),
    )
    .await?;
    let value = result
        .get("value")
        .ok_or_else(|| "RPC getLatestBlockhash did not return a value.".to_string())?;
    let blockhash = value
        .get("blockhash")
        .and_then(Value::as_str)
        .ok_or_else(|| "RPC getLatestBlockhash did not include a blockhash.".to_string())?;
    let last_valid_block_height = value
        .get("lastValidBlockHeight")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            "RPC getLatestBlockhash did not include lastValidBlockHeight.".to_string()
        })?;
    let hash = Hash::from_str(blockhash).map_err(|error| {
        format!("RPC getLatestBlockhash returned an invalid blockhash: {error}")
    })?;
    Ok((hash, last_valid_block_height))
}

pub async fn fetch_latest_blockhash_fresh_or_recent(
    rpc_url: &str,
    commitment: &str,
    min_remaining_block_heights: u64,
) -> Result<(Hash, u64), String> {
    let (blockhash, last_valid_block_height) = fetch_latest_blockhash(rpc_url, commitment).await?;
    if min_remaining_block_heights == 0 {
        return Ok((blockhash, last_valid_block_height));
    }
    let current_block_height = fetch_block_height(rpc_url, commitment).await?;
    if last_valid_block_height.saturating_sub(current_block_height) < min_remaining_block_heights {
        return fetch_latest_blockhash(rpc_url, commitment).await;
    }
    Ok((blockhash, last_valid_block_height))
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
                transaction.serialized_base64,
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
            units_consumed: result
                .get("value")
                .and_then(|value| value.get("unitsConsumed"))
                .and_then(Value::as_u64),
            logs,
        });
    }
    Ok((results, warnings))
}

pub async fn fetch_minimum_balance_for_rent_exemption(
    rpc_url: &str,
    commitment: &str,
    data_len: u64,
) -> Result<u64, String> {
    let result = rpc_request(
        rpc_url,
        "getMinimumBalanceForRentExemption",
        json!([
            data_len,
            {
                "commitment": normalized_commitment(commitment)
            }
        ]),
    )
    .await?;
    result.as_u64().ok_or_else(|| {
        "RPC getMinimumBalanceForRentExemption did not return a lamport value.".to_string()
    })
}

fn standard_rpc_submit_endpoints(primary_rpc_url: &str, extra_endpoints: &[String]) -> Vec<String> {
    let mut endpoints = Vec::new();
    let primary = primary_rpc_url.trim();
    if !primary.is_empty() {
        endpoints.push(primary.to_string());
    }
    for endpoint in extra_endpoints {
        let trimmed = endpoint.trim();
        if !trimmed.is_empty() && !endpoints.iter().any(|value| value == trimmed) {
            endpoints.push(trimmed.to_string());
        }
    }
    endpoints
}

async fn submit_single_transaction_standard_rpc_fanout(
    endpoints: &[String],
    transaction: &CompiledTransaction,
    commitment: &str,
    skip_preflight: bool,
    max_retries: u32,
) -> Result<(SentResult, Vec<String>), String> {
    let sig_preview = transaction
        .signature
        .as_deref()
        .map(|s| {
            if s.len() > 16 {
                s[..16].to_string()
            } else {
                s.to_string()
            }
        })
        .unwrap_or_else(|| "<unsigned>".to_string());
    eprintln!(
        "[execution-engine][standard-rpc] submit label={} sig={}… bytes_b64={} endpoints={} skip_preflight={} max_retries={}",
        transaction.label,
        sig_preview,
        transaction.serialized_base64.len(),
        endpoints.len(),
        skip_preflight,
        max_retries
    );

    let mut errors = Vec::new();
    let mut first_successful_endpoint = None;
    let mut returned_signature = None;
    for (idx, endpoint) in endpoints.iter().enumerate() {
        let attempt_started = std::time::Instant::now();
        match rpc_request(
            endpoint,
            "sendTransaction",
            json!([
                transaction.serialized_base64,
                {
                    "encoding": "base64",
                    "skipPreflight": skip_preflight,
                    "preflightCommitment": commitment,
                    "maxRetries": max_retries,
                }
            ]),
        )
        .await
        {
            Ok(result) => {
                let elapsed_ms = attempt_started.elapsed().as_millis();
                first_successful_endpoint = Some(endpoint.clone());
                returned_signature = result.as_str().map(str::to_string);
                eprintln!(
                    "[execution-engine][standard-rpc] OK attempt={}/{} endpoint={} elapsed_ms={} sig={}",
                    idx + 1,
                    endpoints.len(),
                    endpoint,
                    elapsed_ms,
                    returned_signature.as_deref().unwrap_or("<none>")
                );
                break;
            }
            Err(error) => {
                let elapsed_ms = attempt_started.elapsed().as_millis();
                eprintln!(
                    "[execution-engine][standard-rpc] FAIL attempt={}/{} endpoint={} elapsed_ms={} err={}",
                    idx + 1,
                    endpoints.len(),
                    endpoint,
                    elapsed_ms,
                    error
                );
                errors.push(format!("{endpoint}: {error}"));
            }
        }
    }

    if first_successful_endpoint.is_none() {
        eprintln!(
            "[execution-engine][standard-rpc] ABORT label={} all {} endpoints failed",
            transaction.label,
            endpoints.len()
        );
        return Err(format!(
            "Standard RPC submission failed for {} on all attempted endpoints: {}",
            transaction.label,
            errors.join(" | ")
        ));
    }

    let mut warnings = Vec::new();
    if !errors.is_empty() {
        warnings.push(format!(
            "Standard RPC fanout had partial failures for {}: {}",
            transaction.label,
            errors.join(" | ")
        ));
    }

    Ok((
        SentResult {
            label: transaction.label.clone(),
            format: transaction.format.clone(),
            signature: transaction.signature.clone().or(returned_signature),
            transport_type: if endpoints.len() > 1 {
                "standard-rpc-fanout".to_string()
            } else {
                "standard-rpc".to_string()
            },
            endpoint: first_successful_endpoint,
            attempted_endpoints: endpoints.to_vec(),
            skip_preflight,
            max_retries,
            confirmation_status: None,
            error: None,
            bundle_id: None,
            attempted_bundle_ids: vec![],
            transaction_subscribe_account_required: vec![],
        },
        warnings,
    ))
}

const HELIUS_SENDER_GLOBAL_FALLBACK: &str = "https://sender.helius-rpc.com/fast";

/// Short preview of a base64-encoded serialized transaction, for log lines.
fn serialized_preview(serialized_base64: &str) -> String {
    let len = serialized_base64.len();
    if len <= 24 {
        serialized_base64.to_string()
    } else {
        format!(
            "{}…{}",
            &serialized_base64[..12],
            &serialized_base64[len - 8..]
        )
    }
}

/// Helius Sender enforces a minimum 200,000 lamport tip to one of their
/// configured wallets. See `pump_native::HELIUS_SENDER_MIN_TIP_LAMPORTS`.
/// This mirrors the same constant without cross-module coupling.
const HELIUS_SENDER_REQUIRED_TIP_LAMPORTS: u64 = 200_000;

async fn submit_single_transaction_helius_sender(
    endpoints: &[String],
    transaction: &CompiledTransaction,
) -> Result<(SentResult, Vec<String>), String> {
    // Fail fast if the compiled transaction doesn't carry a Helius-compatible
    // inline tip — otherwise every endpoint returns an identical HTTP 500.
    let inline_tip = transaction.inline_tip_lamports.unwrap_or(0);
    if inline_tip < HELIUS_SENDER_REQUIRED_TIP_LAMPORTS {
        let detail = format!(
            "inline tip {} lamports < Helius Sender minimum {} lamports (transaction label: {}); this would be rejected by every Helius endpoint",
            inline_tip, HELIUS_SENDER_REQUIRED_TIP_LAMPORTS, transaction.label
        );
        eprintln!("[execution-engine][helius-sender] PRECHECK_FAIL {detail}");
        return Err(format!("Helius Sender precheck failed: {detail}"));
    }

    // Build the attempt list: the configured endpoints first, followed by the
    // global https endpoint as a last-resort fallback if the configured list
    // is all-regional. Regional endpoints occasionally return 5xx during
    // Helius maintenance; the global endpoint is more stable.
    let mut attempt_order: Vec<String> = endpoints.iter().cloned().collect();
    let already_has_global = attempt_order
        .iter()
        .any(|value| value.eq_ignore_ascii_case(HELIUS_SENDER_GLOBAL_FALLBACK));
    if !already_has_global {
        attempt_order.push(HELIUS_SENDER_GLOBAL_FALLBACK.to_string());
    }

    let tx_bytes = transaction.serialized_base64.len();
    let tx_preview = serialized_preview(&transaction.serialized_base64);
    let sig_preview = transaction
        .signature
        .as_deref()
        .map(|s| {
            if s.len() > 16 {
                s[..16].to_string()
            } else {
                s.to_string()
            }
        })
        .unwrap_or_else(|| "<unsigned>".to_string());
    eprintln!(
        "[execution-engine][helius-sender] submit label={} sig={}… bytes_b64={} tx_preview={} endpoints={}",
        transaction.label,
        sig_preview,
        tx_bytes,
        tx_preview,
        attempt_order.len()
    );

    let mut errors = Vec::new();
    let mut first_successful_endpoint = None;
    let mut returned_signature = None;
    for (idx, endpoint) in attempt_order.iter().enumerate() {
        let attempt_started = std::time::Instant::now();
        match rpc_request(
            endpoint,
            "sendTransaction",
            json!([
                transaction.serialized_base64,
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
                let elapsed_ms = attempt_started.elapsed().as_millis();
                first_successful_endpoint = Some(endpoint.clone());
                returned_signature = result.as_str().map(str::to_string);
                eprintln!(
                    "[execution-engine][helius-sender] OK attempt={}/{} endpoint={} elapsed_ms={} sig={}",
                    idx + 1,
                    attempt_order.len(),
                    endpoint,
                    elapsed_ms,
                    returned_signature.as_deref().unwrap_or("<none>")
                );
                break;
            }
            Err(error) => {
                let elapsed_ms = attempt_started.elapsed().as_millis();
                eprintln!(
                    "[execution-engine][helius-sender] FAIL attempt={}/{} endpoint={} elapsed_ms={} err={}",
                    idx + 1,
                    attempt_order.len(),
                    endpoint,
                    elapsed_ms,
                    error
                );
                errors.push(format!("{endpoint}: {error}"));
            }
        }
    }

    if first_successful_endpoint.is_none() {
        eprintln!(
            "[execution-engine][helius-sender] ABORT label={} all {} endpoints failed",
            transaction.label,
            attempt_order.len()
        );
        return Err(format!(
            "Helius Sender failed for {} on all attempted endpoints: {}",
            transaction.label,
            errors.join(" | ")
        ));
    }

    let mut warnings = Vec::new();
    if !errors.is_empty() {
        warnings.push(format!(
            "Helius Sender fanout had partial failures for {}: {}",
            transaction.label,
            errors.join(" | ")
        ));
    }

    Ok((
        SentResult {
            label: transaction.label.clone(),
            format: transaction.format.clone(),
            signature: transaction.signature.clone().or(returned_signature),
            transport_type: "helius-sender".to_string(),
            endpoint: first_successful_endpoint,
            attempted_endpoints: attempt_order,
            skip_preflight: true,
            max_retries: 0,
            confirmation_status: None,
            error: None,
            bundle_id: None,
            attempted_bundle_ids: vec![],
            transaction_subscribe_account_required: vec![],
        },
        warnings,
    ))
}

pub async fn submit_independent_transactions_for_transport(
    rpc_url: &str,
    transport_plan: &TransportPlan,
    transactions: &[CompiledTransaction],
) -> Result<(Vec<SentResult>, Vec<String>, u128), String> {
    if matches!(
        transport_plan.transport_type.as_str(),
        "hellomoon-quic" | "hellomoon-bundle" | "jito-bundle"
    ) {
        let shared_plan = map_transport_plan(transport_plan);
        let shared_transactions = transactions
            .iter()
            .map(map_compiled_transaction_to_shared)
            .collect::<Vec<_>>();
        let transport_environment = transport_environment_snapshot();
        let (results, warnings, elapsed_ms) = submit_shared_independent_transactions_for_transport(
            rpc_url,
            &shared_plan,
            &shared_transactions,
            &transport_plan.commitment,
            transport_plan.skip_preflight,
            transport_plan.track_send_block_height,
            &transport_environment,
        )
        .await?;
        return Ok((
            results.into_iter().map(map_sent_result).collect(),
            warnings,
            elapsed_ms,
        ));
    }
    let started = std::time::Instant::now();
    let mut results = Vec::with_capacity(transactions.len());
    let mut warnings = Vec::new();

    for transaction in transactions {
        let (result, entry_warnings) = if transport_plan.transport_type == "helius-sender" {
            submit_single_transaction_helius_sender(
                &transport_plan.helius_sender_endpoints,
                transaction,
            )
            .await?
        } else {
            let endpoints = standard_rpc_submit_endpoints(
                rpc_url,
                &transport_plan.standard_rpc_submit_endpoints,
            );
            submit_single_transaction_standard_rpc_fanout(
                &endpoints,
                transaction,
                &transport_plan.commitment,
                transport_plan.skip_preflight,
                transport_plan.max_retries,
            )
            .await?
        };
        results.push(result);
        warnings.extend(entry_warnings);
    }

    Ok((results, warnings, started.elapsed().as_millis()))
}

pub async fn confirm_submitted_transactions_for_transport(
    rpc_url: &str,
    transport_plan: &TransportPlan,
    submitted: &mut [SentResult],
) -> Result<(Vec<String>, u128), String> {
    if matches!(
        transport_plan.transport_type.as_str(),
        "hellomoon-quic" | "hellomoon-bundle" | "jito-bundle"
    ) {
        let shared_plan = map_transport_plan(transport_plan);
        let mut shared_submitted = submitted
            .iter()
            .cloned()
            .map(map_sent_result_to_shared)
            .collect::<Vec<_>>();
        let transport_environment = transport_environment_snapshot();
        let (warnings, elapsed_ms) = confirm_shared_submitted_transactions_for_transport(
            rpc_url,
            &shared_plan,
            &mut shared_submitted,
            &transport_plan.commitment,
            transport_plan.track_send_block_height,
            &transport_environment,
        )
        .await?;
        for (local, shared_result) in submitted.iter_mut().zip(shared_submitted.into_iter()) {
            let confirmation_status = shared_result.confirmationStatus.clone();
            local.confirmation_status = confirmation_status.clone();
            local.bundle_id = shared_result.bundleId.clone();
            local.attempted_bundle_ids = shared_result.attemptedBundleIds.clone();
            if confirmation_status.as_deref() == Some("failed") {
                local.error = Some(format!(
                    "Transaction {} failed during {} confirmation.",
                    shared_result
                        .signature
                        .unwrap_or_else(|| local.label.clone()),
                    transport_plan.transport_type
                ));
            }
        }
        return Ok((warnings, elapsed_ms));
    }
    let client = shared_rpc_http_client();
    let started = std::time::Instant::now();
    let mut warnings = transport_plan.warnings.clone();

    for result in submitted.iter_mut() {
        let signature = result.signature.clone().ok_or_else(|| {
            format!(
                "Submitted transaction {} is missing a signature.",
                result.label
            )
        })?;

        for _ in 0..DEFAULT_CONFIRM_MAX_ATTEMPTS {
            let status_result = rpc_request_with_client(
                client,
                rpc_url,
                "getSignatureStatuses",
                json!([
                    [signature],
                    {
                        "searchTransactionHistory": true
                    }
                ]),
            )
            .await?;

            let Some(status) = status_result
                .get("value")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .cloned()
            else {
                sleep(Duration::from_millis(DEFAULT_CONFIRM_POLL_INTERVAL_MS)).await;
                continue;
            };
            if status.is_null() {
                sleep(Duration::from_millis(DEFAULT_CONFIRM_POLL_INTERVAL_MS)).await;
                continue;
            }
            if status.get("err").is_some() && !status.get("err").unwrap_or(&Value::Null).is_null() {
                result.error = Some(format!(
                    "Transaction {signature} failed on-chain: {}",
                    status.get("err").cloned().unwrap_or(Value::Null)
                ));
                result.confirmation_status = Some("failed".to_string());
                break;
            }

            let confirmation = status
                .get("confirmationStatus")
                .and_then(Value::as_str)
                .unwrap_or("processed");
            result.confirmation_status = Some(confirmation.to_string());
            if matches!(confirmation, "confirmed" | "finalized") {
                break;
            }

            sleep(Duration::from_millis(DEFAULT_CONFIRM_POLL_INTERVAL_MS)).await;
        }

        if result.confirmation_status.is_none() {
            result.error = Some(format!(
                "Timed out waiting for transaction {} to reach {}.",
                signature, transport_plan.commitment
            ));
            warnings.push(format!(
                "Transport confirmation timed out for {} on {}.",
                result.label, transport_plan.transport_type
            ));
        }
    }

    Ok((warnings, started.elapsed().as_millis()))
}

pub async fn fetch_token_balance(owner: &str, mint: &str) -> Result<TokenBalance, String> {
    let rpc_url = configured_rpc_url();
    let result = rpc_request_with_client(
        shared_rpc_http_client(),
        &rpc_url,
        "getTokenAccountsByOwner",
        json!([
            owner,
            { "mint": mint },
            { "encoding": "jsonParsed", "commitment": DEFAULT_COMMITMENT }
        ]),
    )
    .await?;

    let accounts = result
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "RPC getTokenAccountsByOwner returned invalid account data.".to_string())?;

    let mut total_raw: u128 = 0;
    let mut decimals: Option<u8> = None;
    for entry in accounts {
        let token_amount = entry
            .get("account")
            .and_then(|value| value.get("data"))
            .and_then(|value| value.get("parsed"))
            .and_then(|value| value.get("info"))
            .and_then(|value| value.get("tokenAmount"))
            .ok_or_else(|| {
                "RPC getTokenAccountsByOwner returned invalid token amount data.".to_string()
            })?;

        let amount_raw = token_amount
            .get("amount")
            .and_then(Value::as_str)
            .ok_or_else(|| "RPC token balance response did not include raw amount.".to_string())?
            .parse::<u128>()
            .map_err(|error| format!("RPC token balance amount parse failed: {error}"))?;

        let token_decimals = token_amount
            .get("decimals")
            .and_then(Value::as_u64)
            .ok_or_else(|| "RPC token balance response did not include decimals.".to_string())?;

        if decimals.is_none() {
            decimals = Some(token_decimals as u8);
        }
        total_raw = total_raw.saturating_add(amount_raw);
    }

    let total_raw = u64::try_from(total_raw)
        .map_err(|_| format!("Token balance for {owner} exceeded u64 limits."))?;

    Ok(TokenBalance {
        amount_raw: total_raw,
        decimals: decimals.unwrap_or(0),
    })
}

fn parse_token_account_raw_balance(data: &[u8]) -> Result<u64, String> {
    let end = TOKEN_ACCOUNT_AMOUNT_OFFSET + TOKEN_ACCOUNT_AMOUNT_LEN;
    if data.len() < end {
        return Err("Token account data was too short to contain a token amount.".to_string());
    }
    let amount_bytes: [u8; TOKEN_ACCOUNT_AMOUNT_LEN] = data[TOKEN_ACCOUNT_AMOUNT_OFFSET..end]
        .try_into()
        .map_err(|_| "Token account amount bytes were malformed.".to_string())?;
    Ok(u64::from_le_bytes(amount_bytes))
}

fn ata_balance_retry_commitments(commitment: &str) -> Vec<String> {
    let requested = normalized_commitment(commitment);
    if requested == "processed" {
        vec![requested]
    } else {
        vec![requested, "processed".to_string()]
    }
}

pub async fn fetch_token_balance_via_ata(
    owner: &str,
    mint: &str,
    decimals: u8,
    commitment: &str,
) -> Result<TokenBalance, String> {
    let rpc_url = configured_rpc_url();
    let retry_commitments = ata_balance_retry_commitments(commitment);
    let owner_pubkey =
        Pubkey::from_str(owner).map_err(|error| format!("Invalid owner {owner}: {error}"))?;
    let mint_pubkey =
        Pubkey::from_str(mint).map_err(|error| format!("Invalid mint {mint}: {error}"))?;
    let mut token_program = None;
    for read_commitment in &retry_commitments {
        if let Some((owner, _)) =
            fetch_account_owner_and_data(&rpc_url, mint, read_commitment).await?
        {
            token_program = Some(owner);
            break;
        }
    }
    let token_program =
        token_program.ok_or_else(|| format!("Mint account {mint} was not found."))?;
    let ata_address =
        get_associated_token_address_with_program_id(&owner_pubkey, &mint_pubkey, &token_program)
            .to_string();

    for read_commitment in &retry_commitments {
        for delay_ms in std::iter::once(0).chain(ATA_BALANCE_RETRY_DELAYS_MS.into_iter()) {
            if delay_ms > 0 {
                sleep(Duration::from_millis(delay_ms)).await;
            }
            match fetch_account_owner_and_data(&rpc_url, &ata_address, read_commitment).await? {
                Some((ata_owner, data)) => {
                    if ata_owner != token_program {
                        return Err(format!(
                            "Token account {ata_address} had owner {ata_owner}, expected token program {token_program}."
                        ));
                    }
                    let amount_raw = parse_token_account_raw_balance(&data).map_err(|error| {
                        format!("Invalid token account data for {ata_address}: {error}")
                    })?;
                    return Ok(TokenBalance {
                        amount_raw,
                        decimals,
                    });
                }
                None => continue,
            }
        }
    }

    Err(format!(
        "Token account {ata_address} for mint {mint} was not visible after retries at {} commitment(s). A recent buy may still be settling on RPC; try the sell again in a moment.",
        retry_commitments.join(" -> ")
    ))
}

pub async fn fetch_owned_token_mints(
    rpc_url: &str,
    owner: &str,
    commitment: &str,
    token_program_id: &str,
) -> Result<Vec<String>, String> {
    let result = rpc_request_with_client(
        shared_rpc_http_client(),
        rpc_url,
        "getTokenAccountsByOwner",
        json!([
            owner,
            { "programId": token_program_id },
            { "encoding": "jsonParsed", "commitment": normalized_commitment(commitment) }
        ]),
    )
    .await?;

    let accounts = result
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| "RPC getTokenAccountsByOwner returned invalid account data.".to_string())?;

    let mut mints = Vec::new();
    for entry in accounts {
        let mint = entry
            .get("account")
            .and_then(|value| value.get("data"))
            .and_then(|value| value.get("parsed"))
            .and_then(|value| value.get("info"))
            .and_then(|value| value.get("mint"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "RPC getTokenAccountsByOwner returned invalid parsed mint data.".to_string()
            })?;
        if !mints.iter().any(|existing| existing == mint) {
            mints.push(mint.to_string());
        }
    }
    Ok(mints)
}
