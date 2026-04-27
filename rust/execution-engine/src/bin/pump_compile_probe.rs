use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use execution_engine::{
    extension_api::{
        BuyFundingPolicy, MevMode, SellSettlementPolicy, TradeSettlementAsset, TradeSide,
    },
    rpc_client::{configured_rpc_url, simulate_transactions},
    trade_runtime::{
        RuntimeExecutionPolicy, RuntimeSellIntent, TradeRuntimeRequest, compile_wallet_trade,
    },
};
use serde::Deserialize;
use serde_json::json;

const PACKET_LIMIT_BYTES: usize = 1232;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditCase {
    label: Option<String>,
    side: String,
    mint: String,
    wallet_key: String,
    buy_amount_sol: Option<String>,
    provider: Option<String>,
    tip_sol: Option<String>,
    sell_percent: Option<String>,
    sell_output_sol: Option<String>,
    simulate: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditCaseFile {
    cases: Vec<AuditCase>,
}

fn parse_args() -> Result<
    (
        TradeSide,
        String,
        String,
        String,
        String,
        Option<RuntimeSellIntent>,
    ),
    String,
> {
    let mut side = TradeSide::Buy;
    let mut mint = String::new();
    let mut wallet_key = String::new();
    let mut buy_amount_sol = "0.01".to_string();
    let mut provider = "standard-rpc".to_string();
    let mut sell_intent: Option<RuntimeSellIntent> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-simulate" => {}
            "--case-file" => {
                let _ = args.next();
            }
            "--side" => {
                side = match args.next().unwrap_or_default().as_str() {
                    "buy" => TradeSide::Buy,
                    "sell" => TradeSide::Sell,
                    other => return Err(format!("Unsupported side: {other}")),
                }
            }
            "--mint" => mint = args.next().unwrap_or_default(),
            "--wallet-key" => wallet_key = args.next().unwrap_or_default(),
            "--buy-amount-sol" => {
                buy_amount_sol = args.next().unwrap_or_else(|| "0.01".to_string())
            }
            "--provider" => provider = args.next().unwrap_or_else(|| "standard-rpc".to_string()),
            "--sell-percent" => {
                sell_intent = Some(RuntimeSellIntent::Percent(args.next().unwrap_or_default()))
            }
            "--sell-output-sol" => {
                sell_intent = Some(RuntimeSellIntent::SolOutput(
                    args.next().unwrap_or_default(),
                ))
            }
            other => return Err(format!("Unknown argument: {other}")),
        }
    }
    if mint.trim().is_empty() {
        return Err("--mint is required".to_string());
    }
    if wallet_key.trim().is_empty() {
        return Err("--wallet-key is required".to_string());
    }
    if matches!(side, TradeSide::Sell) && sell_intent.is_none() {
        sell_intent = Some(RuntimeSellIntent::Percent("10".to_string()));
    }
    Ok((
        side,
        mint,
        wallet_key,
        buy_amount_sol,
        provider,
        sell_intent,
    ))
}

fn side_from_str(value: &str) -> Result<TradeSide, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "buy" => Ok(TradeSide::Buy),
        "sell" => Ok(TradeSide::Sell),
        other => Err(format!("Unsupported side: {other}")),
    }
}

fn sell_intent_from_case(case: &AuditCase) -> Option<RuntimeSellIntent> {
    case.sell_output_sol
        .as_ref()
        .map(|value| RuntimeSellIntent::SolOutput(value.clone()))
        .or_else(|| {
            case.sell_percent
                .as_ref()
                .map(|value| RuntimeSellIntent::Percent(value.clone()))
        })
}

fn build_request(
    side: TradeSide,
    mint: String,
    buy_amount_sol: Option<String>,
    provider: String,
    tip_sol: String,
    sell_intent: Option<RuntimeSellIntent>,
) -> TradeRuntimeRequest {
    TradeRuntimeRequest {
        side: side.clone(),
        mint,
        buy_amount_sol: if matches!(side, TradeSide::Buy) {
            buy_amount_sol
        } else {
            None
        },
        sell_intent,
        policy: RuntimeExecutionPolicy {
            slippage_percent: "25".to_string(),
            mev_mode: MevMode::Off,
            auto_tip_enabled: false,
            fee_sol: "0.001".to_string(),
            tip_sol,
            provider,
            endpoint_profile: String::new(),
            commitment: "confirmed".to_string(),
            skip_preflight: false,
            track_send_block_height: false,
            buy_funding_policy: BuyFundingPolicy::SolOnly,
            sell_settlement_policy: SellSettlementPolicy::AlwaysToSol,
            sell_settlement_asset: TradeSettlementAsset::Sol,
        },
        platform_label: None,
        planned_route: None,
        planned_trade: None,
        pinned_pool: None,
        warm_key: None,
    }
}

fn transaction_summaries(
    transactions: &[execution_engine::rpc_client::CompiledTransaction],
) -> Vec<serde_json::Value> {
    transactions
        .iter()
        .map(|transaction| {
            let bytes = BASE64
                .decode(&transaction.serialized_base64)
                .map(|bytes| bytes.len())
                .ok();
            json!({
                "label": transaction.label,
                "format": transaction.format,
                "bytes": bytes,
                "packetHeadroom": bytes.map(|len| PACKET_LIMIT_BYTES as isize - len as isize),
                "lookupTablesUsed": transaction.lookup_tables_used,
                "computeUnitLimit": transaction.compute_unit_limit,
                "computeUnitPriceMicroLamports": transaction.compute_unit_price_micro_lamports,
                "inlineTipLamports": transaction.inline_tip_lamports,
                "inlineTipAccount": transaction.inline_tip_account,
            })
        })
        .collect()
}

async fn run_case_file(path: &str) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let case_file: AuditCaseFile = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    let mut results = Vec::new();
    for case in case_file.cases {
        let side = side_from_str(&case.side)?;
        let buy_amount_sol = case
            .buy_amount_sol
            .clone()
            .or_else(|| Some("0.01".to_string()));
        let request = build_request(
            side.clone(),
            case.mint.clone(),
            buy_amount_sol,
            case.provider
                .clone()
                .unwrap_or_else(|| "standard-rpc".to_string()),
            case.tip_sol.clone().unwrap_or_else(|| "0".to_string()),
            sell_intent_from_case(&case).or_else(|| {
                matches!(side, TradeSide::Sell)
                    .then(|| RuntimeSellIntent::Percent("10".to_string()))
            }),
        );
        let compiled = compile_wallet_trade(&request, &case.wallet_key).await?;
        let should_simulate = case.simulate.unwrap_or(false);
        let simulation = if should_simulate {
            let rpc_url = configured_rpc_url();
            let (simulation, warnings) =
                simulate_transactions(&rpc_url, &compiled.transactions, &request.policy.commitment)
                    .await?;
            Some(json!({ "results": simulation, "warnings": warnings }))
        } else {
            None
        };
        results.push(json!({
            "label": case.label.unwrap_or_else(|| format!("{}:{}", case.side, case.mint)),
            "selector": compiled.selector,
            "transactions": transaction_summaries(&compiled.transactions),
            "simulation": simulation,
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({ "cases": results }))
            .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn main() -> Result<(), String> {
    std::thread::Builder::new()
        .name("pump-compile-probe".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("Failed to build probe runtime: {error}"))?;
            runtime.block_on(async_main())
        })
        .map_err(|error| format!("Failed to spawn probe thread: {error}"))?
        .join()
        .map_err(|_| "Probe thread panicked.".to_string())?
}

async fn async_main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--case-file" {
            let path = args
                .next()
                .ok_or_else(|| "--case-file requires a path".to_string())?;
            return run_case_file(&path).await;
        }
    }

    let no_simulate = std::env::args().any(|arg| arg == "--no-simulate");
    let (side, mint, wallet_key, buy_amount_sol, provider, sell_intent) = parse_args()?;
    let request = build_request(
        side.clone(),
        mint.clone(),
        Some(buy_amount_sol.clone()),
        provider,
        "0".to_string(),
        sell_intent,
    );

    let compiled = compile_wallet_trade(&request, &wallet_key).await?;
    let simulation_output = if no_simulate {
        None
    } else {
        let rpc_url = configured_rpc_url();
        let (simulation, warnings) =
            simulate_transactions(&rpc_url, &compiled.transactions, &request.policy.commitment)
                .await?;
        Some(json!({ "results": simulation, "warnings": warnings }))
    };

    let output = json!({
        "mint": mint,
        "side": side,
        "walletKey": wallet_key,
        "selector": compiled.selector,
        "transactions": transaction_summaries(&compiled.transactions),
        "simulation": simulation_output,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|error| error.to_string())?
    );
    Ok(())
}
