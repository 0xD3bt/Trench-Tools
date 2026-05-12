use execution_engine::{
    extension_api::{
        BuyFundingPolicy, MevMode, SellSettlementPolicy, TradeSettlementAsset, TradeSide,
    },
    rpc_client::fetch_account_owner_and_data,
    rpc_client::{rpc_request_with_client, shared_rpc_http_client},
    trade_dispatch::{RouteDescriptor, classify_route_input, resolve_trade_plan},
    trade_runtime::{RuntimeExecutionPolicy, TradeRuntimeRequest},
};
use serde_json::json;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

async fn run_history_probe(args: &[String]) -> Result<(), String> {
    let address = if let Some(address) = arg_value(args, "--history-address") {
        address
    } else if let Some(mint) = arg_value(args, "--pump-bonding-history") {
        let mint = Pubkey::from_str(&mint).map_err(|error| format!("invalid mint: {error}"))?;
        let program = Pubkey::from_str(PUMP_PROGRAM_ID)
            .map_err(|error| format!("invalid pump program: {error}"))?;
        Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &program)
            .0
            .to_string()
    } else {
        return Err("--history-address is required".to_string());
    };
    let limit = arg_value(args, "--limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5);
    let rpc_url = execution_engine::rpc_client::configured_rpc_url();
    let mut options = serde_json::Map::new();
    options.insert("commitment".to_string(), json!("confirmed"));
    options.insert("limit".to_string(), json!(limit));
    if let Some(before) = arg_value(args, "--before") {
        options.insert("before".to_string(), json!(before));
    }
    let signatures = rpc_request_with_client(
        shared_rpc_http_client(),
        &rpc_url,
        "getSignaturesForAddress",
        json!([address, serde_json::Value::Object(options)]),
    )
    .await?;
    let Some(items) = signatures.as_array() else {
        return Err("getSignaturesForAddress returned a non-array result".to_string());
    };
    if has_flag(args, "--signatures-only") {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "address": address,
                "signatures": items,
            }))
            .map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    let contains_key = arg_value(args, "--contains-key");
    let mut txs = Vec::new();
    for item in items {
        let Some(signature) = item.get("signature").and_then(|value| value.as_str()) else {
            continue;
        };
        let transaction = rpc_request_with_client(
            shared_rpc_http_client(),
            &rpc_url,
            "getTransaction",
            json!([
                signature,
                {
                    "encoding": "jsonParsed",
                    "commitment": "confirmed",
                    "maxSupportedTransactionVersion": 0,
                }
            ]),
        )
        .await
        .unwrap_or_else(|error| json!({ "error": error }));
        let account_keys = transaction
            .get("transaction")
            .and_then(|value| value.get("message"))
            .and_then(|value| value.get("accountKeys"))
            .and_then(|value| value.as_array())
            .map(|keys| {
                keys.iter()
                    .filter_map(|key| {
                        key.as_str().map(str::to_string).or_else(|| {
                            key.get("pubkey")
                                .and_then(|value| value.as_str())
                                .map(str::to_string)
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(required_key) = contains_key.as_deref() {
            if !account_keys.iter().any(|key| key == required_key) {
                continue;
            }
        }
        let logs = transaction
            .get("meta")
            .and_then(|value| value.get("logMessages"))
            .and_then(|value| value.as_array())
            .map(|logs| {
                logs.iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .filter(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower.contains("migrate")
                            || lower.contains("initialize")
                            || lower.contains("raydium")
                            || lower.contains("create")
                            || lower.contains("pool")
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        txs.push(json!({
            "signature": signature,
            "slot": item.get("slot"),
            "blockTime": item.get("blockTime"),
            "err": item.get("err"),
            "accountKeys": account_keys,
            "interestingLogs": logs,
            "fetchError": transaction.get("error"),
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "address": address,
            "transactions": txs,
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn side_from_args(args: &[String]) -> Result<TradeSide, String> {
    match arg_value(args, "--side")
        .unwrap_or_else(|| "buy".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "buy" => Ok(TradeSide::Buy),
        "sell" => Ok(TradeSide::Sell),
        other => Err(format!("unsupported --side {other}")),
    }
}

fn buy_funding_policy_from_args(args: &[String]) -> Result<BuyFundingPolicy, String> {
    match arg_value(args, "--buy-funding-policy")
        .unwrap_or_else(|| "sol_only".to_string())
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .as_str()
    {
        "sol_only" => Ok(BuyFundingPolicy::SolOnly),
        "prefer_usd1_else_topup" | "prefer_usd1_else_top_up" => {
            Ok(BuyFundingPolicy::PreferUsd1ElseTopUp)
        }
        "usd1_only" => Ok(BuyFundingPolicy::Usd1Only),
        other => Err(format!("unsupported --buy-funding-policy {other}")),
    }
}

fn sell_settlement_from_args(
    args: &[String],
) -> Result<(SellSettlementPolicy, TradeSettlementAsset), String> {
    match arg_value(args, "--sell-settlement")
        .unwrap_or_else(|| "sol".to_string())
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .as_str()
    {
        "sol" | "always_to_sol" => {
            Ok((SellSettlementPolicy::AlwaysToSol, TradeSettlementAsset::Sol))
        }
        "usd1" | "always_to_usd1" => Ok((
            SellSettlementPolicy::AlwaysToUsd1,
            TradeSettlementAsset::Usd1,
        )),
        other => Err(format!("unsupported --sell-settlement {other}")),
    }
}

fn request_from_args(args: &[String]) -> Result<TradeRuntimeRequest, String> {
    let side = side_from_args(args)?;
    let mint = arg_value(args, "--mint").ok_or_else(|| "--mint is required".to_string())?;
    let buy_funding_policy = buy_funding_policy_from_args(args)?;
    let (sell_settlement_policy, sell_settlement_asset) = sell_settlement_from_args(args)?;
    Ok(TradeRuntimeRequest {
        side: side.clone(),
        mint,
        buy_amount_sol: matches!(side, TradeSide::Buy)
            .then(|| arg_value(args, "--buy-amount-sol").unwrap_or_else(|| "0.01".to_string())),
        sell_intent: None,
        policy: RuntimeExecutionPolicy {
            slippage_percent: arg_value(args, "--slippage").unwrap_or_else(|| "5".to_string()),
            mev_mode: MevMode::Off,
            auto_tip_enabled: false,
            fee_sol: "0".to_string(),
            tip_sol: "0".to_string(),
            provider: arg_value(args, "--provider").unwrap_or_else(|| "standard-rpc".to_string()),
            endpoint_profile: arg_value(args, "--endpoint-profile").unwrap_or_default(),
            commitment: arg_value(args, "--commitment").unwrap_or_else(|| "confirmed".to_string()),
            skip_preflight: false,
            track_send_block_height: false,
            buy_funding_policy,
            sell_settlement_policy,
            sell_settlement_asset,
        },
        platform_label: arg_value(args, "--platform"),
        planned_route: None,
        planned_trade: None,
        pinned_pool: arg_value(args, "--pool"),
        warm_key: None,
        fallback_mint_hint: arg_value(args, "--fallback-mint"),
    })
}

fn descriptor_json(descriptor: RouteDescriptor) -> serde_json::Value {
    json!({
        "rawAddress": descriptor.raw_address,
        "inputKind": descriptor.resolved_input_kind.label(),
        "resolvedMint": descriptor.resolved_mint,
        "resolvedPair": descriptor.resolved_pair,
        "routeLockedPair": descriptor.route_locked_pair,
        "family": descriptor.family.map(|value| value.label().to_string()),
        "lifecycle": descriptor.lifecycle.map(|value| value.label().to_string()),
        "quoteAsset": descriptor.quote_asset.map(|value| value.label().to_string()),
        "canonicalMarketKey": descriptor.canonical_market_key,
        "nonCanonical": descriptor.non_canonical,
    })
}

async fn account_summary(
    rpc_url: &str,
    address: Option<&str>,
    commitment: &str,
) -> serde_json::Value {
    let Some(address) = address.map(str::trim).filter(|value| !value.is_empty()) else {
        return serde_json::Value::Null;
    };
    match fetch_account_owner_and_data(rpc_url, address, commitment).await {
        Ok(Some((owner, data))) => json!({
            "address": address,
            "owner": owner.to_string(),
            "dataLen": data.len(),
        }),
        Ok(None) => json!({
            "address": address,
            "missing": true,
        }),
        Err(error) => json!({
            "address": address,
            "error": error,
        }),
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let _ = dotenvy::dotenv();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args
        .iter()
        .any(|arg| arg == "--history-address" || arg == "--pump-bonding-history")
    {
        return run_history_probe(&args).await;
    }
    if has_flag(&args, "--help") {
        println!("route_probe --mint <mint-or-pool> [--pool <pool>] [--platform axiom]");
        return Ok(());
    }
    let request = request_from_args(&args)?;
    let rpc_url = execution_engine::rpc_client::configured_rpc_url();
    let classified =
        classify_route_input(&rpc_url, &request.mint, &request.policy.commitment).await?;
    let raw_account =
        account_summary(&rpc_url, Some(&request.mint), &request.policy.commitment).await;
    let pool_account = account_summary(
        &rpc_url,
        request.pinned_pool.as_deref(),
        &request.policy.commitment,
    )
    .await;
    let planned = resolve_trade_plan(&request).await;
    let output = match planned {
        Ok(plan) => json!({
            "ok": true,
            "request": {
                "mint": request.mint,
                "pool": request.pinned_pool,
            },
            "classified": classified.map(descriptor_json),
            "account": raw_account,
            "pinnedPoolAccount": pool_account,
            "plan": {
                "adapter": plan.adapter.label(),
                "rawAddress": plan.raw_address,
                "inputKind": plan.resolved_input_kind.label(),
                "resolvedMint": plan.resolved_mint,
                "resolvedPinnedPool": plan.resolved_pinned_pool,
                "nonCanonical": plan.non_canonical,
                "selector": plan.selector,
            }
        }),
        Err(error) => json!({
            "ok": false,
            "request": {
                "mint": request.mint,
                "pool": request.pinned_pool,
            },
            "classified": classified.map(descriptor_json),
            "account": raw_account,
            "pinnedPoolAccount": pool_account,
            "error": error,
        }),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|error| error.to_string())?
    );
    Ok(())
}
