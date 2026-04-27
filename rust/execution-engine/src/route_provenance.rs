use serde_json::{Value, json};

use crate::rpc_client::{rpc_request_with_client, shared_rpc_http_client};

#[derive(Debug, Clone)]
pub struct RpcSignatureInfo {
    pub signature: String,
    pub slot: Option<u64>,
    pub err: Option<Value>,
}

fn signature_from_value(value: &Value) -> Option<RpcSignatureInfo> {
    Some(RpcSignatureInfo {
        signature: value.get("signature")?.as_str()?.to_string(),
        slot: value.get("slot").and_then(Value::as_u64),
        err: value.get("err").cloned().filter(|value| !value.is_null()),
    })
}

pub async fn fetch_signatures_for_address_page(
    rpc_url: &str,
    address: &str,
    commitment: &str,
    before: Option<&str>,
    limit: usize,
) -> Result<Vec<RpcSignatureInfo>, String> {
    let mut options = serde_json::Map::new();
    options.insert("commitment".to_string(), json!(commitment));
    options.insert("limit".to_string(), json!(limit));
    if let Some(before) = before.filter(|value| !value.trim().is_empty()) {
        options.insert("before".to_string(), json!(before));
    }
    let result = rpc_request_with_client(
        shared_rpc_http_client(),
        rpc_url,
        "getSignaturesForAddress",
        json!([address, Value::Object(options)]),
    )
    .await?;
    let Some(items) = result.as_array() else {
        return Err("RPC getSignaturesForAddress returned a non-array result.".to_string());
    };
    Ok(items.iter().filter_map(signature_from_value).collect())
}

pub async fn fetch_oldest_signature_window_for_address(
    rpc_url: &str,
    address: &str,
    commitment: &str,
    page_limit: usize,
    max_pages: usize,
    oldest_window: usize,
) -> Result<Vec<RpcSignatureInfo>, String> {
    let page_limit = page_limit.clamp(1, 1_000);
    let max_pages = max_pages.max(1);
    let oldest_window = oldest_window.max(1);
    let mut before = None::<String>;
    let mut oldest_page = Vec::new();
    for _ in 0..max_pages {
        let page = fetch_signatures_for_address_page(
            rpc_url,
            address,
            commitment,
            before.as_deref(),
            page_limit,
        )
        .await?;
        if page.is_empty() {
            break;
        }
        before = page.last().map(|item| item.signature.clone());
        oldest_page = page;
        if oldest_page.len() < page_limit {
            break;
        }
    }
    if oldest_page.len() > oldest_window {
        Ok(oldest_page[oldest_page.len() - oldest_window..].to_vec())
    } else {
        Ok(oldest_page)
    }
}

pub async fn fetch_transaction_json(
    rpc_url: &str,
    signature: &str,
    commitment: &str,
) -> Result<Value, String> {
    rpc_request_with_client(
        shared_rpc_http_client(),
        rpc_url,
        "getTransaction",
        json!([
            signature,
            {
                "encoding": "jsonParsed",
                "commitment": commitment,
                "maxSupportedTransactionVersion": 0,
            }
        ]),
    )
    .await
}

pub fn transaction_account_keys(transaction: &Value) -> Vec<String> {
    transaction
        .get("transaction")
        .and_then(|value| value.get("message"))
        .and_then(|value| value.get("accountKeys"))
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(|key| {
                    key.as_str().map(str::to_string).or_else(|| {
                        key.get("pubkey")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn transaction_log_messages(transaction: &Value) -> Vec<String> {
    transaction
        .get("meta")
        .and_then(|value| value.get("logMessages"))
        .and_then(Value::as_array)
        .map(|logs| {
            logs.iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn transaction_succeeded(transaction: &Value) -> bool {
    transaction
        .get("meta")
        .and_then(|value| value.get("err"))
        .is_none_or(Value::is_null)
}

pub fn account_keys_contain_all(keys: &[String], required: &[String]) -> bool {
    required
        .iter()
        .all(|required_key| keys.iter().any(|key| key == required_key))
}
