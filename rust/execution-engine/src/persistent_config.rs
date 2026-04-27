use serde_json::{Value, json};
use std::env;

const DEFAULT_PROVIDER: &str = "helius-sender";
const DEFAULT_CREATION_PRIORITY_FEE_SOL: &str = "0.001";
const DEFAULT_CREATION_TIP_SOL: &str = "0.001";
const DEFAULT_TRADE_PRIORITY_FEE_SOL: &str = "0.001";
const DEFAULT_TRADE_TIP_SOL: &str = "0.001";
const DEFAULT_TRADE_SLIPPAGE_PERCENT: &str = "";
const DEFAULT_WRAPPER_FEE_BPS: u64 = crate::rollout::DEFAULT_WRAPPER_FEE_BPS as u64;
const DEFAULT_QUICK_DEV_BUY_AMOUNTS: [&str; 3] = ["0.5", "1", "2"];

const MAX_BUY_AMOUNT_ROWS: u64 = 2;
const BUY_AMOUNTS_PER_ROW: usize = 4;
// Sells share the buy shape (max 2 rows of 4 entries); aliases keep the
// per-side intent obvious at the call site without duplicating values.
const MAX_SELL_PERCENT_ROWS: u64 = MAX_BUY_AMOUNT_ROWS;
const SELL_PERCENTS_PER_ROW: usize = BUY_AMOUNTS_PER_ROW;

fn clamp_buy_amount_rows(value: u64) -> u8 {
    if value == 0 || value > MAX_BUY_AMOUNT_ROWS {
        1
    } else {
        value as u8
    }
}

fn clamp_sell_percent_rows(value: u64) -> u8 {
    if value == 0 || value > MAX_SELL_PERCENT_ROWS {
        1
    } else {
        value as u8
    }
}

fn read_buy_amount_rows(raw: Option<&Value>) -> u8 {
    let parsed = raw.and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    });
    parsed.map(clamp_buy_amount_rows).unwrap_or(1)
}

fn read_sell_percent_rows(raw: Option<&Value>) -> u8 {
    let parsed = raw.and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    });
    parsed.map(clamp_sell_percent_rows).unwrap_or(1)
}

fn infer_rows_from_shortcut_value(rows: u8, raw: Option<&Value>, values_per_row: usize) -> u8 {
    if rows == 2 {
        return 2;
    }
    let row2_has_value = raw
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .skip(values_per_row)
                .take(values_per_row)
                .any(|entry| !entry.as_str().unwrap_or_default().trim().is_empty())
        })
        .unwrap_or(false);
    if row2_has_value { 2 } else { rows }
}

fn configured_track_send_block_height_env_enabled() -> bool {
    matches!(
        env::var("LAUNCHDECK_TRACK_SEND_BLOCK_HEIGHT")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn benchmark_mode_allows_track_send_block_height_default(mode: &str) -> bool {
    matches!(mode.trim().to_ascii_lowercase().as_str(), "" | "full")
}

fn configured_track_send_block_height_default() -> bool {
    benchmark_mode_allows_track_send_block_height_default(
        &env::var("LAUNCHDECK_BENCHMARK_MODE").unwrap_or_default(),
    ) && configured_track_send_block_height_env_enabled()
}

fn legacy_provider_alias(provider: &str) -> String {
    match provider {
        "auto" | "helius" => "helius-sender".to_string(),
        "jito" => "jito-bundle".to_string(),
        "astralane" | "bloxroute" => "standard-rpc".to_string(),
        "hellomoon" => "hellomoon".to_string(),
        _ => provider.to_string(),
    }
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn bool_value(value: Option<&Value>, fallback: bool) -> bool {
    match value {
        None | Some(Value::Null) => fallback,
        Some(Value::Bool(v)) => *v,
        Some(Value::Number(n)) => n.as_i64().unwrap_or_default() != 0,
        Some(Value::String(v)) => match v.trim().to_lowercase().as_str() {
            "true" => true,
            "false" => false,
            "" => fallback,
            _ => true,
        },
        Some(_) => fallback,
    }
}

fn number_value(value: Option<&Value>, fallback: i64) -> i64 {
    match value {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(fallback),
        Some(Value::String(v)) => v.trim().parse::<i64>().unwrap_or(fallback),
        _ => fallback,
    }
}

fn object_value(value: Option<&Value>) -> Value {
    match value.and_then(Value::as_object) {
        Some(map) => Value::Object(map.clone()),
        None => json!({}),
    }
}

fn normalize_provider(provider: &str, fallback: &str) -> String {
    let normalized = provider.trim().to_lowercase();
    if normalized.is_empty() {
        return fallback.to_string();
    }
    let migrated = legacy_provider_alias(&normalized);
    match migrated.as_str() {
        "helius-sender" | "hellomoon" | "standard-rpc" | "jito-bundle" => migrated,
        _ => fallback.to_string(),
    }
}

fn normalize_decimal_string(value: &str, fallback: &str) -> String {
    let normalized = value.trim();
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        normalized.to_string()
    }
}

fn optional_string_field(value: &str) -> Option<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

/// UI + engine MEV routing mode for Hello Moon (and future providers). Must round-trip through
/// `normalize_persistent_config` so settings save does not drop user choices.
fn normalize_mev_mode(raw: Option<&Value>, fallback: &str) -> String {
    let fallback_norm = match fallback.trim().to_ascii_lowercase().as_str() {
        "reduced" => "reduced",
        "secure" => "secure",
        "off" | "" => "off",
        _ => "off",
    }
    .to_string();
    let Some(value) = raw else {
        return fallback_norm;
    };
    if let Some(text) = value.as_str() {
        return match text.trim().to_ascii_lowercase().as_str() {
            "reduced" => "reduced".to_string(),
            "secure" => "secure".to_string(),
            "off" => "off".to_string(),
            _ => fallback_norm,
        };
    }
    if let Some(flag) = value.as_bool() {
        // Legacy `mevProtect` boolean from older UI.
        return if flag {
            "reduced".to_string()
        } else {
            "off".to_string()
        };
    }
    fallback_norm
}

fn normalize_buy_funding_policy(raw: Option<&Value>, fallback: Option<&Value>) -> Option<String> {
    match first_non_empty(&[string_value(raw), string_value(fallback)])
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "sol_only" | "sol-only" | "sol only" => Some("sol_only".to_string()),
        "prefer_usd1_else_topup"
        | "prefer_usd1_else_top_up"
        | "prefer-usd1-else-topup"
        | "prefer-usd1-else-top-up"
        | "prefer usd1 else topup"
        | "prefer usd1 else top up" => Some("prefer_usd1_else_topup".to_string()),
        "usd1_only" | "usd1-only" | "usd1 only" => Some("usd1_only".to_string()),
        _ => None,
    }
}

fn normalize_wrapper_fee_bps_value(raw: Option<&Value>, fallback: Option<&Value>) -> Value {
    // Mirror the on-chain allow-list (0, 10, 20 bps).
    fn coerce(value: Option<&Value>) -> Option<u64> {
        match value? {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.trim().parse::<u64>().ok(),
            _ => None,
        }
    }
    let raw_or_fallback = coerce(raw)
        .or_else(|| coerce(fallback))
        .unwrap_or(DEFAULT_WRAPPER_FEE_BPS);
    let clamped = match raw_or_fallback {
        0 => 0,
        1..=10 => 10,
        11..=20 => 20,
        _ => 20,
    };
    Value::Number(serde_json::Number::from(clamped))
}

fn normalize_sell_settlement_policy(
    raw: Option<&Value>,
    fallback: Option<&Value>,
) -> Option<String> {
    match first_non_empty(&[string_value(raw), string_value(fallback)])
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "always_to_sol" | "always-to-sol" | "always to sol" => Some("always_to_sol".to_string()),
        "always_to_usd1" | "always-to-usd1" | "always to usd1" => {
            Some("always_to_usd1".to_string())
        }
        "match_stored_entry_preference"
        | "match-stored-entry-preference"
        | "match stored entry preference" => Some("match_stored_entry_preference".to_string()),
        _ => None,
    }
}

fn creation_settings(
    provider: &str,
    endpoint_profile: &str,
    tip_sol: &str,
    priority_fee_sol: &str,
    auto_fee: bool,
    max_fee_sol: &str,
    dev_buy_sol: &str,
    mev_mode: &str,
) -> Value {
    let mut settings = json!({
        "provider": normalize_provider(provider, DEFAULT_PROVIDER),
        "tipSol": normalize_decimal_string(tip_sol, DEFAULT_CREATION_TIP_SOL),
        "priorityFeeSol": normalize_decimal_string(priority_fee_sol, DEFAULT_CREATION_PRIORITY_FEE_SOL),
        "autoFee": auto_fee,
        "maxFeeSol": max_fee_sol.trim(),
        "devBuySol": normalize_decimal_string(dev_buy_sol, ""),
        "mevMode": mev_mode,
    });
    if let Some(endpoint_profile) = optional_string_field(endpoint_profile) {
        settings
            .as_object_mut()
            .expect("creation settings object")
            .insert(
                "endpointProfile".to_string(),
                Value::String(endpoint_profile),
            );
    }
    settings
}

fn trade_settings(
    provider: &str,
    endpoint_profile: &str,
    priority_fee_sol: &str,
    tip_sol: &str,
    slippage_percent: &str,
    auto_fee: bool,
    max_fee_sol: &str,
    mev_mode: &str,
) -> Value {
    let mut settings = json!({
        "provider": normalize_provider(provider, DEFAULT_PROVIDER),
        "priorityFeeSol": normalize_decimal_string(priority_fee_sol, DEFAULT_TRADE_PRIORITY_FEE_SOL),
        "tipSol": normalize_decimal_string(tip_sol, DEFAULT_TRADE_TIP_SOL),
        "slippagePercent": normalize_decimal_string(slippage_percent, DEFAULT_TRADE_SLIPPAGE_PERCENT),
        "autoFee": auto_fee,
        "maxFeeSol": max_fee_sol.trim(),
        "mevMode": mev_mode,
    });
    if let Some(endpoint_profile) = optional_string_field(endpoint_profile) {
        settings
            .as_object_mut()
            .expect("trade settings object")
            .insert(
                "endpointProfile".to_string(),
                Value::String(endpoint_profile),
            );
    }
    settings
}

fn empty_shortcut_values(length: usize) -> Vec<String> {
    vec![String::new(); length]
}

fn default_quick_dev_buy_amounts() -> Vec<String> {
    DEFAULT_QUICK_DEV_BUY_AMOUNTS
        .iter()
        .map(|value| value.to_string())
        .collect()
}

fn normalize_quick_dev_buy_amounts(raw: Option<&Value>, preset_items: &[Value]) -> Vec<String> {
    let fallback = default_quick_dev_buy_amounts();
    let mut values = raw
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|entry| entry.as_str().unwrap_or_default().trim().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if values.is_empty() {
        values = preset_items
            .iter()
            .take(fallback.len())
            .enumerate()
            .map(|(index, preset)| {
                string_value(
                    preset
                        .get("creationSettings")
                        .and_then(|settings| settings.get("devBuySol")),
                )
                .if_empty_then(fallback[index].clone())
            })
            .collect();
    }
    while values.len() < fallback.len() {
        values.push(fallback[values.len()].clone());
    }
    values.truncate(fallback.len());
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| value.if_empty_then(fallback[index].clone()))
        .collect()
}

fn preset_template(index: usize) -> Value {
    let mut buy = trade_settings("", "", "", "", "", false, "", "off");
    if let Some(object) = buy.as_object_mut() {
        object.insert(
            "snipeBuyAmountSol".to_string(),
            Value::String(String::new()),
        );
    }
    json!({
        "id": format!("preset{}", index + 1),
        "label": format!("Preset {}", index + 1),
        "buyAmountsSol": empty_shortcut_values(4),
        "sellAmountsPercent": empty_shortcut_values(4),
        "buyAmountRows": 1,
        "sellPercentRows": 1,
        "creationSettings": creation_settings("", "", "", "", false, "", "", "off"),
        "buySettings": buy,
        "sellSettings": trade_settings("", "", "", "", "", false, "", "off"),
        "postLaunchStrategy": "none",
    })
}

// Keep env access at the crate boundary so the config shape helpers stay pure.
fn create_default_persistent_config_with_track_send_block_height_default(
    track_send_block_height_default: bool,
) -> Value {
    json!({
        "defaults": {
            "launchpad": "pump",
            "mode": "regular",
            "activePresetId": "",
            "presetEditing": false,
            "quickDevBuyAmounts": default_quick_dev_buy_amounts(),
            "misc": {
                "trackSendBlockHeight": track_send_block_height_default,
                "defaultBuyFundingPolicy": "sol_only",
                "defaultSellSettlementPolicy": "always_to_sol",
                "wrapperDefaultFeeBps": DEFAULT_WRAPPER_FEE_BPS
            },
            "automaticDevSell": {
                "enabled": false,
                "percent": 100,
                "triggerFamily": "time",
                "triggerMode": "block-offset",
                "delayMs": 0,
                "targetBlockOffset": 0,
                "marketCapEnabled": false,
                "marketCapThreshold": "",
                "marketCapScanTimeoutSeconds": 30,
                "marketCapTimeoutAction": "stop"
            }
        },
        "presets": {
            "items": Vec::<Value>::new()
        }
    })
}

pub fn create_default_persistent_config() -> Value {
    create_default_persistent_config_with_track_send_block_height_default(
        configured_track_send_block_height_default(),
    )
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

fn normalize_shortcut_values(
    raw: Option<&Value>,
    fallback: &[String],
    length: usize,
) -> Vec<String> {
    let mut values = raw
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|entry| entry.as_str().unwrap_or_default().trim().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if values.is_empty() {
        values = fallback.to_vec();
    }
    while values.len() < length {
        values.push(String::new());
    }
    values.truncate(length);
    values
}

fn normalize_preset_shape(preset: Option<&Value>, fallback_preset: &Value, index: usize) -> Value {
    let preset_obj = preset.and_then(Value::as_object);
    let fallback_obj = fallback_preset.as_object().expect("fallback preset object");
    let fallback_buy_amounts = fallback_obj
        .get("buyAmountsSol")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|entry| entry.as_str().unwrap_or_default().trim().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![String::new(), String::new(), String::new(), String::new()]);
    let fallback_sell_amounts = fallback_obj
        .get("sellAmountsPercent")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|entry| entry.as_str().unwrap_or_default().trim().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| empty_shortcut_values(4));
    let fallback_creation = fallback_obj
        .get("creationSettings")
        .and_then(Value::as_object)
        .expect("creation settings");
    let fallback_buy = fallback_obj
        .get("buySettings")
        .and_then(Value::as_object)
        .expect("buy settings");
    let fallback_sell = fallback_obj
        .get("sellSettings")
        .and_then(Value::as_object)
        .expect("sell settings");
    let creation = preset_obj
        .and_then(|value| value.get("creationSettings"))
        .and_then(Value::as_object);
    let buy = preset_obj
        .and_then(|value| value.get("buySettings"))
        .and_then(Value::as_object);
    let sell = preset_obj
        .and_then(|value| value.get("sellSettings"))
        .and_then(Value::as_object);
    let id = preset_obj
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
        .if_empty_then(format!("preset{}", index + 1));
    let label = preset_obj
        .and_then(|value| value.get("label"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
        .if_empty_then(format!("P{}", index + 1));
    let creation_mev_mode = normalize_mev_mode(
        creation
            .and_then(|value| value.get("mevMode"))
            .or_else(|| creation.and_then(|value| value.get("mevProtect"))),
        &string_value(fallback_creation.get("mevMode")),
    );
    let creation_settings = creation_settings(
        creation
            .and_then(|value| value.get("provider"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_creation
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_PROVIDER),
            ),
        creation
            .and_then(|value| value.get("endpointProfile"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_creation
                    .get("endpointProfile")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        creation
            .and_then(|value| value.get("tipSol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_creation
                    .get("tipSol")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_CREATION_TIP_SOL),
            ),
        creation
            .and_then(|value| value.get("priorityFeeSol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_creation
                    .get("priorityFeeSol")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_CREATION_PRIORITY_FEE_SOL),
            ),
        bool_value(
            creation.and_then(|value| value.get("autoFee")),
            bool_value(fallback_creation.get("autoFee"), false),
        ),
        creation
            .and_then(|value| value.get("maxFeeSol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_creation
                    .get("maxFeeSol")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        creation
            .and_then(|value| value.get("devBuySol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_creation
                    .get("devBuySol")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        &creation_mev_mode,
    );
    let buy_mev_mode = normalize_mev_mode(
        buy.and_then(|value| value.get("mevMode"))
            .or_else(|| buy.and_then(|value| value.get("mevProtect"))),
        &string_value(fallback_buy.get("mevMode")),
    );
    let mut buy_settings = trade_settings(
        buy.and_then(|value| value.get("provider"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_buy
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_PROVIDER),
            ),
        buy.and_then(|value| value.get("endpointProfile"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_buy
                    .get("endpointProfile")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        buy.and_then(|value| value.get("priorityFeeSol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_buy
                    .get("priorityFeeSol")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_TRADE_PRIORITY_FEE_SOL),
            ),
        buy.and_then(|value| value.get("tipSol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_buy
                    .get("tipSol")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_TRADE_TIP_SOL),
            ),
        buy.and_then(|value| value.get("slippagePercent"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_buy
                    .get("slippagePercent")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_TRADE_SLIPPAGE_PERCENT),
            ),
        bool_value(
            buy.and_then(|value| value.get("autoFee")),
            bool_value(fallback_buy.get("autoFee"), false),
        ),
        buy.and_then(|value| value.get("maxFeeSol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_buy
                    .get("maxFeeSol")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        &buy_mev_mode,
    );
    buy_settings
        .as_object_mut()
        .expect("buy settings object")
        .insert(
            "snipeBuyAmountSol".to_string(),
            Value::String(normalize_decimal_string(
                buy.and_then(|value| value.get("snipeBuyAmountSol"))
                    .and_then(Value::as_str)
                    .unwrap_or(
                        fallback_buy
                            .get("snipeBuyAmountSol")
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                    ),
                "",
            )),
        );
    if let Some(policy) = normalize_buy_funding_policy(
        buy.and_then(|value| value.get("buyFundingPolicy")),
        fallback_buy.get("buyFundingPolicy"),
    ) {
        buy_settings
            .as_object_mut()
            .expect("buy settings object")
            .insert("buyFundingPolicy".to_string(), Value::String(policy));
    }
    let sell_mev_mode = normalize_mev_mode(
        sell.and_then(|value| value.get("mevMode"))
            .or_else(|| sell.and_then(|value| value.get("mevProtect"))),
        &string_value(fallback_sell.get("mevMode")),
    );
    let mut sell_settings = trade_settings(
        sell.and_then(|value| value.get("provider"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_sell
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_PROVIDER),
            ),
        sell.and_then(|value| value.get("endpointProfile"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_sell
                    .get("endpointProfile")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        sell.and_then(|value| value.get("priorityFeeSol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_sell
                    .get("priorityFeeSol")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_TRADE_PRIORITY_FEE_SOL),
            ),
        sell.and_then(|value| value.get("tipSol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_sell
                    .get("tipSol")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_TRADE_TIP_SOL),
            ),
        sell.and_then(|value| value.get("slippagePercent"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_sell
                    .get("slippagePercent")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_TRADE_SLIPPAGE_PERCENT),
            ),
        bool_value(
            sell.and_then(|value| value.get("autoFee")),
            bool_value(fallback_sell.get("autoFee"), false),
        ),
        sell.and_then(|value| value.get("maxFeeSol"))
            .and_then(Value::as_str)
            .unwrap_or(
                fallback_sell
                    .get("maxFeeSol")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        &sell_mev_mode,
    );
    if let Some(policy) = normalize_sell_settlement_policy(
        sell.and_then(|value| value.get("sellSettlementPolicy")),
        fallback_sell.get("sellSettlementPolicy"),
    ) {
        sell_settings
            .as_object_mut()
            .expect("sell settings object")
            .insert("sellSettlementPolicy".to_string(), Value::String(policy));
    }
    let post_launch_strategy = preset_obj
        .and_then(|value| value.get("postLaunchStrategy"))
        .and_then(Value::as_str)
        .unwrap_or("none")
        .trim()
        .to_string()
        .if_empty_then("none".to_string());
    let raw_buy_amounts = preset_obj.and_then(|value| value.get("buyAmountsSol"));
    let mut buy_amount_rows = infer_rows_from_shortcut_value(
        read_buy_amount_rows(preset_obj.and_then(|value| value.get("buyAmountRows"))),
        raw_buy_amounts,
        BUY_AMOUNTS_PER_ROW,
    );
    let buy_amounts_length = buy_amount_rows as usize * BUY_AMOUNTS_PER_ROW;
    let mut buy_amounts =
        normalize_shortcut_values(raw_buy_amounts, &fallback_buy_amounts, buy_amounts_length);
    if buy_amount_rows == 2
        && buy_amounts[BUY_AMOUNTS_PER_ROW..]
            .iter()
            .all(|value| value.is_empty())
    {
        buy_amount_rows = 1;
        buy_amounts.truncate(BUY_AMOUNTS_PER_ROW);
    }
    let raw_sell_percents = preset_obj.and_then(|value| value.get("sellAmountsPercent"));
    let mut sell_percent_rows = infer_rows_from_shortcut_value(
        read_sell_percent_rows(preset_obj.and_then(|value| value.get("sellPercentRows"))),
        raw_sell_percents,
        SELL_PERCENTS_PER_ROW,
    );
    let sell_percents_length = sell_percent_rows as usize * SELL_PERCENTS_PER_ROW;
    let mut sell_percents = normalize_shortcut_values(
        raw_sell_percents,
        &fallback_sell_amounts,
        sell_percents_length,
    );
    if sell_percent_rows == 2
        && sell_percents[SELL_PERCENTS_PER_ROW..]
            .iter()
            .all(|value| value.is_empty())
    {
        sell_percent_rows = 1;
        sell_percents.truncate(SELL_PERCENTS_PER_ROW);
    }
    json!({
        "id": id,
        "label": label,
        "buyAmountsSol": buy_amounts,
        "buyAmountRows": buy_amount_rows,
        "sellAmountsPercent": sell_percents,
        "sellPercentRows": sell_percent_rows,
        "creationSettings": creation_settings,
        "buySettings": buy_settings,
        "sellSettings": sell_settings,
        "postLaunchStrategy": post_launch_strategy
    })
}

fn migrate_legacy_config(parsed: &Value, track_send_block_height_default: bool) -> Value {
    let defaults = parsed.get("defaults").unwrap_or(&Value::Null);
    let legacy_auto_sell = defaults.get("automaticDevSell").unwrap_or(&Value::Null);
    let items = Vec::<Value>::new();

    let launchpad = string_value(defaults.get("launchpad")).if_empty_then("pump".to_string());
    let mode = string_value(defaults.get("mode")).if_empty_then("regular".to_string());
    let requested_active_preset_id = string_value(defaults.get("activePresetId"));
    let active_preset_id = if items
        .iter()
        .any(|preset| string_value(preset.get("id")) == requested_active_preset_id)
    {
        requested_active_preset_id
    } else {
        items
            .first()
            .and_then(|preset| preset.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let legacy_auto_sell_trigger_mode = if number_value(legacy_auto_sell.get("delaySeconds"), 0) > 0
    {
        "submit-delay".to_string()
    } else {
        "block-offset".to_string()
    };
    let legacy_auto_sell_market_cap_threshold =
        string_value(legacy_auto_sell.get("marketCapThreshold"));
    let legacy_auto_sell_market_cap_enabled =
        bool_value(
            legacy_auto_sell.get("marketCapEnabled"),
            !legacy_auto_sell_market_cap_threshold.trim().is_empty(),
        ) || !legacy_auto_sell_market_cap_threshold.trim().is_empty();
    let legacy_auto_sell_trigger_family = string_value(legacy_auto_sell.get("triggerFamily"))
        .if_empty_then(if legacy_auto_sell_market_cap_enabled {
            "market-cap".to_string()
        } else {
            "time".to_string()
        });
    json!({
        "defaults": {
            "launchpad": launchpad,
            "mode": mode,
            "activePresetId": active_preset_id,
            "presetEditing": bool_value(defaults.get("presetEditing"), false),
            "quickDevBuyAmounts": normalize_quick_dev_buy_amounts(
                defaults.get("quickDevBuyAmounts"),
                &items
            ),
            "misc": {
                "trackSendBlockHeight": track_send_block_height_default
            },
            "automaticDevSell": {
                "enabled": bool_value(legacy_auto_sell.get("enabled"), false),
                "percent": number_value(legacy_auto_sell.get("percent"), 100),
                "triggerFamily": legacy_auto_sell_trigger_family,
                "triggerMode": legacy_auto_sell_trigger_mode,
                "delayMs": number_value(legacy_auto_sell.get("delaySeconds"), 0) * 1000,
                "targetBlockOffset": 0,
                "marketCapEnabled": legacy_auto_sell_market_cap_enabled,
                "marketCapThreshold": legacy_auto_sell_market_cap_threshold,
                "marketCapScanTimeoutSeconds": number_value(
                    legacy_auto_sell.get("marketCapScanTimeoutSeconds"),
                    if legacy_auto_sell.get("marketCapScanTimeoutMinutes").is_some() {
                        number_value(legacy_auto_sell.get("marketCapScanTimeoutMinutes"), 15) * 60
                    } else {
                        30
                    }
                ),
                "marketCapTimeoutAction": string_value(
                    legacy_auto_sell.get("marketCapTimeoutAction")
                ).if_empty_then("stop".to_string())
            }
        },
        "presets": {
            "items": items
        }
    })
}

trait StringExt {
    fn if_empty_then(self, fallback: String) -> String;
}

impl StringExt for String {
    fn if_empty_then(self, fallback: String) -> String {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

fn normalize_persistent_config_with_track_send_block_height_default(
    parsed: Value,
    track_send_block_height_default: bool,
) -> Value {
    let base = create_default_persistent_config_with_track_send_block_height_default(
        track_send_block_height_default,
    );
    let has_new_shape = parsed
        .get("presets")
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
        .is_some();
    if !has_new_shape {
        return migrate_legacy_config(&parsed, track_send_block_height_default);
    }
    let base_items = base
        .get("presets")
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let merged_defaults = parsed.get("defaults").unwrap_or(&Value::Null);
    let merged_items = parsed
        .get("presets")
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let items = merged_items
        .iter()
        .enumerate()
        .map(|(index, existing)| {
            let fallback = base_items
                .get(index)
                .cloned()
                .unwrap_or_else(|| preset_template(index));
            normalize_preset_shape(Some(existing), &fallback, index)
        })
        .collect::<Vec<_>>();
    let launchpad =
        string_value(merged_defaults.get("launchpad")).if_empty_then("pump".to_string());
    let mode = string_value(merged_defaults.get("mode")).if_empty_then("regular".to_string());
    let requested_active_preset_id = string_value(merged_defaults.get("activePresetId"));
    let active_preset_id = if items
        .iter()
        .any(|preset| string_value(preset.get("id")) == requested_active_preset_id)
    {
        requested_active_preset_id
    } else {
        items
            .first()
            .and_then(|preset| preset.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let quick_dev_buy_amounts =
        normalize_quick_dev_buy_amounts(merged_defaults.get("quickDevBuyAmounts"), &items);
    let automatic_dev_sell_trigger_mode = {
        let mode = string_value(
            merged_defaults
                .get("automaticDevSell")
                .and_then(|value| value.get("triggerMode")),
        );
        if mode.is_empty() {
            "block-offset".to_string()
        } else {
            mode
        }
    };
    let automatic_dev_sell_market_cap_threshold = string_value(
        merged_defaults
            .get("automaticDevSell")
            .and_then(|value| value.get("marketCapThreshold")),
    );
    let automatic_dev_sell_market_cap_enabled =
        bool_value(
            merged_defaults
                .get("automaticDevSell")
                .and_then(|value| value.get("marketCapEnabled")),
            !automatic_dev_sell_market_cap_threshold.trim().is_empty(),
        ) || !automatic_dev_sell_market_cap_threshold.trim().is_empty();
    let automatic_dev_sell_trigger_family = string_value(
        merged_defaults
            .get("automaticDevSell")
            .and_then(|value| value.get("triggerFamily")),
    )
    .if_empty_then(if automatic_dev_sell_market_cap_enabled {
        "market-cap".to_string()
    } else {
        "time".to_string()
    });
    json!({
        "defaults": {
            "launchpad": launchpad,
            "mode": mode,
            "activePresetId": active_preset_id,
            "presetEditing": bool_value(merged_defaults.get("presetEditing"), false),
            "quickDevBuyAmounts": quick_dev_buy_amounts,
            "misc": {
                "sniperDraft": merged_defaults
                    .get("misc")
                    .and_then(|value| value.get("sniperDraft"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "feeSplitDraft": merged_defaults
                    .get("misc")
                    .and_then(|value| value.get("feeSplitDraft"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "agentSplitDraft": merged_defaults
                    .get("misc")
                    .and_then(|value| value.get("agentSplitDraft"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "sniperDraftsByLaunchpad": object_value(
                    merged_defaults
                        .get("misc")
                        .and_then(|value| value.get("sniperDraftsByLaunchpad"))
                ),
                "feeSplitDraftsByLaunchpad": object_value(
                    merged_defaults
                        .get("misc")
                        .and_then(|value| value.get("feeSplitDraftsByLaunchpad"))
                ),
                "agentSplitDraftsByLaunchpad": object_value(
                    merged_defaults
                        .get("misc")
                        .and_then(|value| value.get("agentSplitDraftsByLaunchpad"))
                ),
                "autoSellDraftsByLaunchpad": object_value(
                    merged_defaults
                        .get("misc")
                        .and_then(|value| value.get("autoSellDraftsByLaunchpad"))
                ),
                "defaultBuyFundingPolicy": normalize_buy_funding_policy(
                    merged_defaults
                        .get("misc")
                        .and_then(|value| value.get("defaultBuyFundingPolicy")),
                    base.get("defaults")
                        .and_then(|value| value.get("misc"))
                        .and_then(|value| value.get("defaultBuyFundingPolicy")),
                )
                .unwrap_or_else(|| "sol_only".to_string()),
                "defaultSellSettlementPolicy": normalize_sell_settlement_policy(
                    merged_defaults
                        .get("misc")
                        .and_then(|value| value.get("defaultSellSettlementPolicy")),
                    base.get("defaults")
                        .and_then(|value| value.get("misc"))
                        .and_then(|value| value.get("defaultSellSettlementPolicy")),
                )
                .unwrap_or_else(|| "always_to_sol".to_string()),
                "trackSendBlockHeight": bool_value(
                    merged_defaults
                        .get("misc")
                        .and_then(|value| value.get("trackSendBlockHeight")),
                    track_send_block_height_default
                ),
                "wrapperDefaultFeeBps": normalize_wrapper_fee_bps_value(
                    merged_defaults
                        .get("misc")
                        .and_then(|value| value.get("wrapperDefaultFeeBps")),
                    base.get("defaults")
                        .and_then(|value| value.get("misc"))
                        .and_then(|value| value.get("wrapperDefaultFeeBps")),
                ),
            },
            "automaticDevSell": {
                "enabled": bool_value(merged_defaults.get("automaticDevSell").and_then(|value| value.get("enabled")), false),
                "percent": number_value(merged_defaults.get("automaticDevSell").and_then(|value| value.get("percent")), 100),
                "triggerFamily": automatic_dev_sell_trigger_family,
                "triggerMode": automatic_dev_sell_trigger_mode,
                "delayMs": number_value(
                    merged_defaults.get("automaticDevSell").and_then(|value| value.get("delayMs")),
                    number_value(merged_defaults.get("automaticDevSell").and_then(|value| value.get("delaySeconds")), 0) * 1000
                ),
                "targetBlockOffset": number_value(merged_defaults.get("automaticDevSell").and_then(|value| value.get("targetBlockOffset")), 0),
                "marketCapEnabled": automatic_dev_sell_market_cap_enabled,
                "marketCapThreshold": automatic_dev_sell_market_cap_threshold,
                "marketCapScanTimeoutSeconds": number_value(
                    merged_defaults.get("automaticDevSell").and_then(|value| value.get("marketCapScanTimeoutSeconds")),
                    if merged_defaults
                        .get("automaticDevSell")
                        .and_then(|value| value.get("marketCapScanTimeoutMinutes"))
                        .is_some()
                    {
                        number_value(
                            merged_defaults
                                .get("automaticDevSell")
                                .and_then(|value| value.get("marketCapScanTimeoutMinutes")),
                            15
                        ) * 60
                    } else {
                        30
                    }
                ),
                "marketCapTimeoutAction": string_value(
                    merged_defaults
                        .get("automaticDevSell")
                        .and_then(|value| value.get("marketCapTimeoutAction"))
                ).if_empty_then("stop".to_string())
            }
        },
        "presets": {
            "items": items
        }
    })
}

pub fn normalize_persistent_config(parsed: Value) -> Value {
    normalize_persistent_config_with_track_send_block_height_default(
        parsed,
        configured_track_send_block_height_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_config_persistent_shape_preserves_endpoint_profile_and_strips_policy() {
        let normalized = normalize_persistent_config_with_track_send_block_height_default(
            json!({
                "defaults": {
                    "launchpad": "pump",
                    "mode": "regular",
                    "activePresetId": "preset1"
                },
                "presets": {
                    "items": [{
                        "id": "preset1",
                        "label": "P1",
                        "creationSettings": {
                            "provider": "helius-sender",
                            "endpointProfile": "eu",
                            "policy": "fast",
                            "tipSol": "0.02",
                            "priorityFeeSol": "0.003",
                            "devBuySol": "1"
                        },
                        "buySettings": {
                            "provider": "jito-bundle",
                            "endpointProfile": "asia",
                            "policy": "safe",
                            "priorityFeeSol": "0.02",
                            "tipSol": "0.01",
                            "slippagePercent": "42",
                            "snipeBuyAmountSol": "0.5",
                            "buyFundingPolicy": "sol_only"
                        },
                        "sellSettings": {
                            "provider": "helius-sender",
                            "endpointProfile": "fra",
                            "policy": "fast",
                            "priorityFeeSol": "0.01",
                            "tipSol": "0.02",
                            "slippagePercent": "33",
                            "sellSettlementPolicy": "always_to_sol"
                        }
                    }]
                }
            }),
            false,
        );

        let preset = &normalized["presets"]["items"][0];
        assert_eq!(preset["creationSettings"]["endpointProfile"], "eu");
        assert!(preset["creationSettings"].get("policy").is_none());
        assert_eq!(preset["buySettings"]["endpointProfile"], "asia");
        assert!(preset["buySettings"].get("policy").is_none());
        assert_eq!(preset["buySettings"]["buyFundingPolicy"], "sol_only");
        assert_eq!(preset["sellSettings"]["endpointProfile"], "fra");
        assert!(preset["sellSettings"].get("policy").is_none());
        assert_eq!(
            preset["sellSettings"]["sellSettlementPolicy"],
            "always_to_sol"
        );
        assert_eq!(preset["buySettings"]["snipeBuyAmountSol"], "0.5");
    }

    #[test]
    fn canonical_config_persistent_shape_promotes_quick_dev_buy_amounts_to_defaults() {
        let normalized = normalize_persistent_config_with_track_send_block_height_default(
            json!({
                "defaults": {
                    "launchpad": "pump",
                    "mode": "regular",
                    "activePresetId": "preset1"
                },
                "presets": {
                    "items": [
                        {
                            "id": "preset1",
                            "label": "P1",
                            "creationSettings": { "devBuySol": "0.25" }
                        },
                        {
                            "id": "preset2",
                            "label": "P2",
                            "creationSettings": { "devBuySol": "0.75" }
                        }
                    ]
                }
            }),
            false,
        );

        assert_eq!(
            normalized["defaults"]["quickDevBuyAmounts"],
            json!(["0.25", "0.75", "2"])
        );
    }

    #[test]
    fn persistent_preset_shape_infers_second_rows_from_values() {
        let normalized = normalize_persistent_config_with_track_send_block_height_default(
            json!({
                "defaults": {
                    "launchpad": "pump",
                    "mode": "regular",
                    "activePresetId": "preset1"
                },
                "presets": {
                    "items": [{
                        "id": "preset1",
                        "label": "P1",
                        "buyAmountRows": 9,
                        "buyAmountsSol": ["0.1", "0.2", "0.3", "0.4", "0.5", "", "", ""],
                        "sellAmountsPercent": ["10", "20", "30", "40", "", "60", "", ""]
                    }]
                }
            }),
            false,
        );

        let preset = &normalized["presets"]["items"][0];
        assert_eq!(preset["buyAmountRows"], json!(2));
        assert_eq!(preset["sellPercentRows"], json!(2));
        assert_eq!(
            preset["buyAmountsSol"]
                .as_array()
                .expect("buy amounts")
                .len(),
            8
        );
        assert_eq!(
            preset["sellAmountsPercent"]
                .as_array()
                .expect("sell amounts")
                .len(),
            8
        );
    }

    #[test]
    fn canonical_config_persistent_shape_preserves_global_quick_dev_buy_amounts() {
        let normalized = normalize_persistent_config_with_track_send_block_height_default(
            json!({
                "defaults": {
                    "launchpad": "pump",
                    "mode": "regular",
                    "quickDevBuyAmounts": ["0.1", "", "3", "5"]
                },
                "presets": {
                    "items": [{
                        "id": "preset1",
                        "label": "P1",
                        "creationSettings": { "devBuySol": "9" }
                    }]
                }
            }),
            false,
        );

        assert_eq!(
            normalized["defaults"]["quickDevBuyAmounts"],
            json!(["0.1", "1", "3"])
        );
    }

    #[test]
    fn canonical_config_persistent_shape_normalizes_policy_aliases_to_canonical_snake_case() {
        let normalized = normalize_persistent_config_with_track_send_block_height_default(
            json!({
                "defaults": {
                    "launchpad": "pump",
                    "mode": "regular",
                    "misc": {
                        "defaultBuyFundingPolicy": "prefer usd1 else topup",
                        "defaultSellSettlementPolicy": "always-to-usd1"
                    }
                },
                "presets": {
                    "items": [{
                        "id": "preset1",
                        "label": "P1",
                        "creationSettings": {},
                        "buySettings": {
                            "buyFundingPolicy": "prefer-usd1-else-topup"
                        },
                        "sellSettings": {
                            "sellSettlementPolicy": "match stored entry preference"
                        }
                    }]
                }
            }),
            false,
        );

        assert_eq!(
            normalized["defaults"]["misc"]["defaultBuyFundingPolicy"],
            "prefer_usd1_else_topup"
        );
        assert_eq!(
            normalized["defaults"]["misc"]["defaultSellSettlementPolicy"],
            "always_to_usd1"
        );
        let preset = &normalized["presets"]["items"][0];
        assert_eq!(
            preset["buySettings"]["buyFundingPolicy"],
            "prefer_usd1_else_topup"
        );
        assert_eq!(
            preset["sellSettings"]["sellSettlementPolicy"],
            "match_stored_entry_preference"
        );
    }

    #[test]
    fn canonical_config_persistent_shape_migrates_legacy_shape_to_empty_preset_list() {
        let normalized = normalize_persistent_config_with_track_send_block_height_default(
            json!({
                "defaults": {
                    "launchpad": "pump",
                    "mode": "regular",
                    "launchExecution": {
                        "provider": "helius-sender",
                        "endpointProfile": "eu",
                        "policy": "fast",
                        "tipSol": "0.02"
                    },
                    "buyExecution": {
                        "provider": "jito-bundle",
                        "endpointProfile": "asia",
                        "policy": "safe",
                        "tipSol": "0.03"
                    }
                },
                "presets": {
                    "launch": [{
                        "id": "preset1",
                        "label": "P1",
                        "execution": {
                            "provider": "helius-sender",
                            "endpointProfile": "us",
                            "policy": "safe"
                        }
                    }],
                    "sniper": [{
                        "id": "preset1",
                        "label": "P1",
                        "execution": {
                            "provider": "jito-bundle",
                            "endpointProfile": "ams",
                            "policy": "fast"
                        }
                    }]
                }
            }),
            false,
        );
        let items = normalized["presets"]["items"]
            .as_array()
            .expect("preset items array");
        assert!(items.is_empty());
    }

    #[test]
    fn canonical_config_persistent_shape_preserves_market_cap_auto_sell_fields() {
        let normalized = normalize_persistent_config_with_track_send_block_height_default(
            json!({
                "defaults": {
                    "launchpad": "pump",
                    "mode": "regular",
                    "activePresetId": "preset1",
                    "automaticDevSell": {
                        "enabled": true,
                        "percent": 65,
                        "triggerFamily": "market-cap",
                        "triggerMode": "block-offset",
                        "delayMs": 0,
                        "targetBlockOffset": 0,
                        "marketCapEnabled": true,
                        "marketCapThreshold": "100k",
                        "marketCapScanTimeoutSeconds": 45,
                        "marketCapTimeoutAction": "sell"
                    }
                },
                "presets": {
                    "items": [{
                        "id": "preset1",
                        "label": "P1",
                        "creationSettings": {},
                        "buySettings": {},
                        "sellSettings": {}
                    }]
                }
            }),
            false,
        );

        let auto_sell = &normalized["defaults"]["automaticDevSell"];
        assert_eq!(auto_sell["enabled"], true);
        assert_eq!(auto_sell["percent"], 65);
        assert_eq!(auto_sell["triggerFamily"], "market-cap");
        assert_eq!(auto_sell["marketCapEnabled"], true);
        assert_eq!(auto_sell["marketCapThreshold"], "100k");
        assert_eq!(auto_sell["marketCapScanTimeoutSeconds"], 45);
        assert_eq!(auto_sell["marketCapTimeoutAction"], "sell");
    }
}
