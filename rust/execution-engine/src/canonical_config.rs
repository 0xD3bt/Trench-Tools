use serde_json::{Value, json};

use crate::extension_api::{
    BuyFundingPolicy, EngineSettings, MevMode, PresetSummary, SellSettlementPolicy,
};
use crate::persistent_config::{create_default_persistent_config, normalize_persistent_config};

pub const CANONICAL_CONFIG_SCHEMA_VERSION: u32 = 1;

pub fn default_canonical_config() -> Value {
    normalize_canonical_config(create_default_persistent_config())
}

pub fn normalize_canonical_config(parsed: Value) -> Value {
    let mut normalized = normalize_persistent_config(parsed);
    if let Some(object) = normalized.as_object_mut() {
        object.insert(
            "schemaVersion".to_string(),
            Value::Number(CANONICAL_CONFIG_SCHEMA_VERSION.into()),
        );
    }
    normalized
}

pub fn canonical_config_from_legacy(settings: &EngineSettings, presets: &[PresetSummary]) -> Value {
    let defaults = default_canonical_config();
    let preset_items = if presets.is_empty() {
        defaults
            .get("presets")
            .and_then(|value| value.get("items"))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()))
    } else {
        Value::Array(
            presets
                .iter()
                .enumerate()
                .map(|(index, preset)| legacy_preset_to_canonical(settings, preset, index))
                .collect(),
        )
    };
    normalize_canonical_config(json!({
        "defaults": {
            "launchpad": string_value(defaults.get("defaults").and_then(|value| value.get("launchpad")), "pump"),
            "mode": string_value(defaults.get("defaults").and_then(|value| value.get("mode")), "regular"),
            "activePresetId": preset_items
                .as_array()
                .and_then(|items| items.first())
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or(""),
            "presetEditing": false,
            "misc": {
                "trackSendBlockHeight": settings.track_send_block_height,
                "allowNonCanonicalPoolTrades": settings.allow_non_canonical_pool_trades,
                "defaultBuyFundingPolicy": canonical_buy_funding_policy_value(settings.default_buy_funding_policy),
                "defaultSellSettlementPolicy": canonical_sell_settlement_policy_value(settings.default_sell_settlement_policy),
                "wrapperDefaultFeeBps": settings.wrapper_default_fee_bps,
            },
            "automaticDevSell": defaults
                .get("defaults")
                .and_then(|value| value.get("automaticDevSell"))
                .cloned()
                .unwrap_or_else(|| json!({})),
        },
        "presets": {
            "items": preset_items,
        },
    }))
}

pub fn config_track_send_block_height(config: &Value) -> bool {
    config
        .get("defaults")
        .and_then(|value| value.get("misc"))
        .and_then(|value| value.get("trackSendBlockHeight"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

pub fn config_allow_non_canonical_pool_trades(config: &Value) -> bool {
    config
        .get("defaults")
        .and_then(|value| value.get("misc"))
        .and_then(|value| value.get("allowNonCanonicalPoolTrades"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn config_default_buy_funding_policy(config: &Value) -> BuyFundingPolicy {
    parse_buy_funding_policy_value(
        config
            .get("defaults")
            .and_then(|value| value.get("misc"))
            .and_then(|value| value.get("defaultBuyFundingPolicy")),
    )
    .unwrap_or(BuyFundingPolicy::SolOnly)
}

pub fn config_default_sell_settlement_policy(config: &Value) -> SellSettlementPolicy {
    parse_sell_settlement_policy_value(
        config
            .get("defaults")
            .and_then(|value| value.get("misc"))
            .and_then(|value| value.get("defaultSellSettlementPolicy")),
    )
    .unwrap_or(SellSettlementPolicy::AlwaysToSol)
}

/// Reads the persisted wrapper default fee tier from canonical config.
pub fn config_wrapper_default_fee_bps(config: &Value) -> u16 {
    let raw = config
        .get("defaults")
        .and_then(|value| value.get("misc"))
        .and_then(|value| value.get("wrapperDefaultFeeBps"))
        .and_then(Value::as_u64)
        .unwrap_or(crate::rollout::DEFAULT_WRAPPER_FEE_BPS as u64);
    crate::rollout::normalize_wrapper_fee_bps(raw as u16)
}

pub fn set_wrapper_default_fee_bps_in_config(config: &Value, fee_bps: u16) -> Value {
    let clamped = crate::rollout::normalize_wrapper_fee_bps(fee_bps);
    let mut next = normalize_canonical_config(config.clone());
    if let Some(defaults) = next.get_mut("defaults").and_then(Value::as_object_mut) {
        let misc = defaults
            .entry("misc".to_string())
            .or_insert_with(|| json!({}));
        if let Some(object) = misc.as_object_mut() {
            object.insert(
                "wrapperDefaultFeeBps".to_string(),
                Value::Number(serde_json::Number::from(clamped)),
            );
        }
    }
    next
}

pub fn set_allow_non_canonical_pool_trades(config: &Value, enabled: bool) -> Value {
    let mut next = normalize_canonical_config(config.clone());
    if let Some(defaults) = next.get_mut("defaults").and_then(Value::as_object_mut) {
        let misc = defaults
            .entry("misc".to_string())
            .or_insert_with(|| json!({}));
        if let Some(object) = misc.as_object_mut() {
            object.insert(
                "allowNonCanonicalPoolTrades".to_string(),
                Value::Bool(enabled),
            );
        }
    }
    next
}

pub fn get_canonical_preset(config: &Value, preset_id: &str) -> Option<Value> {
    let items = config
        .get("presets")
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)?;
    let normalized_id = preset_id.trim();
    if normalized_id.is_empty() {
        return items.first().cloned();
    }
    items
        .iter()
        .find(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .map(|value| value == normalized_id)
                .unwrap_or(false)
        })
        .cloned()
        .or_else(|| items.first().cloned())
}

pub fn set_track_send_block_height(config: &Value, enabled: bool) -> Value {
    let mut next = normalize_canonical_config(config.clone());
    if let Some(defaults) = next.get_mut("defaults").and_then(Value::as_object_mut) {
        let misc = defaults
            .entry("misc".to_string())
            .or_insert_with(|| json!({}));
        if let Some(object) = misc.as_object_mut() {
            object.insert("trackSendBlockHeight".to_string(), Value::Bool(enabled));
        }
    }
    next
}

pub fn upsert_legacy_preset(
    config: &Value,
    settings: &EngineSettings,
    preset: &PresetSummary,
) -> Value {
    let mut next = normalize_canonical_config(config.clone());
    let canonical = legacy_preset_to_canonical(settings, preset, 0);
    let items = next
        .get_mut("presets")
        .and_then(Value::as_object_mut)
        .and_then(|presets| presets.get_mut("items"))
        .and_then(Value::as_array_mut);
    if let Some(items) = items {
        if let Some(existing) = items.iter_mut().find(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .map(|value| value == preset.id)
                .unwrap_or(false)
        }) {
            *existing = canonical;
        } else {
            items.push(canonical);
        }
    }
    normalize_canonical_config(next)
}

pub fn remove_legacy_preset(config: &Value, preset_id: &str) -> Value {
    let mut next = normalize_canonical_config(config.clone());
    if let Some(items) = next
        .get_mut("presets")
        .and_then(Value::as_object_mut)
        .and_then(|presets| presets.get_mut("items"))
        .and_then(Value::as_array_mut)
    {
        items.retain(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .map(|value| value != preset_id)
                .unwrap_or(true)
        });
    }
    normalize_canonical_config(next)
}

pub fn route_string_field(route: &Value, field: &str) -> String {
    route
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub fn route_bool_field(route: &Value, field: &str) -> bool {
    route.get(field).and_then(Value::as_bool).unwrap_or(false)
}

pub fn route_mev_mode(route: &Value) -> MevMode {
    match route
        .get("mevMode")
        .and_then(Value::as_str)
        .unwrap_or("off")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "reduced" => MevMode::Reduced,
        "secure" => MevMode::Secure,
        _ => MevMode::Off,
    }
}

pub fn route_quick_amounts(route: &Value, fallback_key: &str) -> Vec<String> {
    normalize_shortcut_list(route.get(fallback_key), "", 4)
}

fn legacy_preset_to_canonical(
    settings: &EngineSettings,
    preset: &PresetSummary,
    index: usize,
) -> Value {
    let mut buy_amount_rows = if preset.buy_amount_rows == 0 || preset.buy_amount_rows > 2 {
        1
    } else {
        preset.buy_amount_rows
    };
    buy_amount_rows = infer_rows_from_legacy_shortcuts(buy_amount_rows, &preset.buy_amounts_sol, 4);
    let buy_amounts_length = buy_amount_rows as usize * 4;
    let mut buy_amounts = normalize_legacy_shortcuts(
        &preset.buy_amounts_sol,
        &preset.buy_amount_sol,
        buy_amounts_length,
    );
    if buy_amount_rows == 2 && buy_amounts[4..].iter().all(|value| value.trim().is_empty()) {
        buy_amount_rows = 1;
        buy_amounts.truncate(4);
    }
    let mut sell_percent_rows = if preset.sell_percent_rows == 0 || preset.sell_percent_rows > 2 {
        1
    } else {
        preset.sell_percent_rows
    };
    sell_percent_rows =
        infer_rows_from_legacy_shortcuts(sell_percent_rows, &preset.sell_amounts_percent, 4);
    let sell_percents_length = sell_percent_rows as usize * 4;
    let mut sell_amounts = normalize_legacy_shortcuts(
        &preset.sell_amounts_percent,
        &preset.sell_percent,
        sell_percents_length,
    );
    if sell_percent_rows == 2
        && sell_amounts[4..]
            .iter()
            .all(|value| value.trim().is_empty())
    {
        sell_percent_rows = 1;
        sell_amounts.truncate(4);
    }
    let buy_mev = legacy_mev_to_canonical(&preset.buy_mev_mode, &settings.default_buy_mev_mode);
    let sell_mev = legacy_mev_to_canonical(&preset.sell_mev_mode, &settings.default_sell_mev_mode);
    let creation_provider = default_provider(&settings.execution_provider);
    let buy_provider_value = if preset.buy_provider.trim().is_empty() {
        default_provider(&settings.execution_provider)
    } else {
        preset.buy_provider.trim().to_string()
    };
    let buy_endpoint_profile_value = if preset.buy_endpoint_profile.trim().is_empty() {
        settings.execution_endpoint_profile.trim().to_string()
    } else {
        preset.buy_endpoint_profile.trim().to_string()
    };
    let sell_provider_value = if preset.sell_provider.trim().is_empty() {
        default_provider(&settings.execution_provider)
    } else {
        preset.sell_provider.trim().to_string()
    };
    let sell_endpoint_profile_value = if preset.sell_endpoint_profile.trim().is_empty() {
        settings.execution_endpoint_profile.trim().to_string()
    } else {
        preset.sell_endpoint_profile.trim().to_string()
    };
    let mut canonical = json!({
        "id": if preset.id.trim().is_empty() { format!("preset{}", index + 1) } else { preset.id.trim().to_string() },
        "label": if preset.label.trim().is_empty() { format!("P{}", index + 1) } else { preset.label.trim().to_string() },
        "buyAmountsSol": buy_amounts,
        "buyAmountRows": buy_amount_rows,
        "sellAmountsPercent": sell_amounts,
        "sellPercentRows": sell_percent_rows,
        "creationSettings": {
            "provider": creation_provider,
            "endpointProfile": settings.execution_endpoint_profile.trim(),
            "priorityFeeSol": preset.buy_fee_sol.trim(),
            "tipSol": preset.buy_tip_sol.trim(),
            "autoFee": preset.buy_auto_tip_enabled,
            "maxFeeSol": "",
            "devBuySol": preset.buy_amount_sol.trim(),
            "mevMode": buy_mev,
        },
        "buySettings": {
            "provider": buy_provider_value,
            "endpointProfile": buy_endpoint_profile_value,
            "priorityFeeSol": preset.buy_fee_sol.trim(),
            "tipSol": preset.buy_tip_sol.trim(),
            "slippagePercent": first_non_empty_str(&[
                preset.buy_slippage_percent.as_str(),
                preset.slippage_percent.as_str(),
                settings.default_buy_slippage_percent.as_str(),
            ]),
            "autoFee": preset.buy_auto_tip_enabled,
            "maxFeeSol": preset.buy_max_fee_sol.trim(),
            "mevMode": buy_mev,
            "snipeBuyAmountSol": "",
        },
        "sellSettings": {
            "provider": sell_provider_value,
            "endpointProfile": sell_endpoint_profile_value,
            "priorityFeeSol": preset.sell_fee_sol.trim(),
            "tipSol": preset.sell_tip_sol.trim(),
            "slippagePercent": first_non_empty_str(&[
                preset.sell_slippage_percent.as_str(),
                preset.slippage_percent.as_str(),
                settings.default_sell_slippage_percent.as_str(),
            ]),
            "autoFee": preset.sell_auto_tip_enabled,
            "maxFeeSol": preset.sell_max_fee_sol.trim(),
            "mevMode": sell_mev,
        },
        "postLaunchStrategy": "none",
    });
    if preset.buy_funding_policy_explicit || preset.buy_funding_policy != BuyFundingPolicy::SolOnly
    {
        canonical["buySettings"]["buyFundingPolicy"] =
            canonical_buy_funding_policy_value(preset.buy_funding_policy);
    }
    if preset.sell_settlement_policy_explicit
        || preset.sell_settlement_policy != SellSettlementPolicy::AlwaysToSol
    {
        canonical["sellSettings"]["sellSettlementPolicy"] =
            canonical_sell_settlement_policy_value(preset.sell_settlement_policy);
    }
    canonical
}

fn normalize_legacy_shortcuts(values: &[String], legacy_value: &str, length: usize) -> Vec<String> {
    let mut normalized = values
        .iter()
        .map(|value| value.trim().to_string())
        .take(length)
        .collect::<Vec<_>>();
    while normalized.len() < length {
        normalized.push(String::new());
    }
    if !normalized.iter().any(|value| !value.trim().is_empty()) && !legacy_value.trim().is_empty() {
        normalized[0] = legacy_value.trim().to_string();
    }
    normalized
}

fn infer_rows_from_legacy_shortcuts(rows: u8, values: &[String], values_per_row: usize) -> u8 {
    if rows == 2 {
        return 2;
    }
    let row2_has_value = values
        .iter()
        .skip(values_per_row)
        .take(values_per_row)
        .any(|value| !value.trim().is_empty());
    if row2_has_value { 2 } else { rows }
}

fn normalize_shortcut_list(raw: Option<&Value>, fallback: &str, length: usize) -> Vec<String> {
    let mut values = raw
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|entry| entry.as_str().unwrap_or_default().trim().to_string())
                .take(length)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    while values.len() < length {
        values.push(String::new());
    }
    if !values.iter().any(|value| !value.trim().is_empty()) && !fallback.trim().is_empty() {
        values[0] = fallback.trim().to_string();
    }
    values.truncate(length);
    values
}

fn string_value(value: Option<&Value>, fallback: &str) -> String {
    let normalized = value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        normalized
    }
}

fn canonical_buy_funding_policy_value(policy: BuyFundingPolicy) -> Value {
    Value::String(
        match policy {
            BuyFundingPolicy::SolOnly => "sol_only",
            BuyFundingPolicy::PreferUsd1ElseTopUp => "prefer_usd1_else_topup",
            BuyFundingPolicy::Usd1Only => "usd1_only",
        }
        .to_string(),
    )
}

fn parse_buy_funding_policy_value(value: Option<&Value>) -> Option<BuyFundingPolicy> {
    match value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "sol_only" | "sol-only" | "sol only" => Some(BuyFundingPolicy::SolOnly),
        "prefer_usd1_else_topup"
        | "prefer_usd1_else_top_up"
        | "prefer-usd1-else-topup"
        | "prefer-usd1-else-top-up"
        | "prefer usd1 else topup"
        | "prefer usd1 else top up" => Some(BuyFundingPolicy::PreferUsd1ElseTopUp),
        "usd1_only" | "usd1-only" | "usd1 only" => Some(BuyFundingPolicy::Usd1Only),
        _ => None,
    }
}

fn canonical_sell_settlement_policy_value(policy: SellSettlementPolicy) -> Value {
    Value::String(
        match policy {
            SellSettlementPolicy::AlwaysToSol => "always_to_sol",
            SellSettlementPolicy::AlwaysToUsd1 => "always_to_usd1",
            SellSettlementPolicy::MatchStoredEntryPreference => "match_stored_entry_preference",
        }
        .to_string(),
    )
}

fn parse_sell_settlement_policy_value(value: Option<&Value>) -> Option<SellSettlementPolicy> {
    match value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "always_to_sol" | "always-to-sol" | "always to sol" => {
            Some(SellSettlementPolicy::AlwaysToSol)
        }
        "always_to_usd1" | "always-to-usd1" | "always to usd1" => {
            Some(SellSettlementPolicy::AlwaysToUsd1)
        }
        "match_stored_entry_preference"
        | "match-stored-entry-preference"
        | "match stored entry preference" => Some(SellSettlementPolicy::MatchStoredEntryPreference),
        _ => None,
    }
}

fn default_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "helius" | "helius-sender" => "helius-sender".to_string(),
        "hellomoon" => "hellomoon".to_string(),
        "jito" | "jito-bundle" => "jito-bundle".to_string(),
        _ => "standard-rpc".to_string(),
    }
}

fn legacy_mev_to_canonical(mode: &MevMode, fallback: &MevMode) -> &'static str {
    match if matches!(mode, MevMode::Off) {
        fallback
    } else {
        mode
    } {
        MevMode::Reduced => "reduced",
        MevMode::Secure => "secure",
        MevMode::Off => "off",
    }
}

fn first_non_empty_str(values: &[&str]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_api::{BuyDistributionMode, PnlTrackingMode};

    fn sample_settings() -> EngineSettings {
        EngineSettings {
            default_buy_slippage_percent: "10".to_string(),
            default_sell_slippage_percent: "15".to_string(),
            default_buy_mev_mode: MevMode::Off,
            default_sell_mev_mode: MevMode::Off,
            execution_provider: "standard-rpc".to_string(),
            execution_endpoint_profile: "global".to_string(),
            execution_commitment: "confirmed".to_string(),
            execution_skip_preflight: false,
            track_send_block_height: true,
            max_active_batches: 8,
            rpc_url: String::new(),
            ws_url: String::new(),
            warm_rpc_url: String::new(),
            warm_ws_url: String::new(),
            shared_region: String::new(),
            helius_rpc_url: String::new(),
            helius_ws_url: String::new(),
            standard_rpc_send_urls: vec![],
            helius_sender_region: String::new(),
            default_distribution_mode: BuyDistributionMode::Each,
            allow_non_canonical_pool_trades: false,
            default_buy_funding_policy: BuyFundingPolicy::PreferUsd1ElseTopUp,
            default_sell_settlement_policy: SellSettlementPolicy::AlwaysToUsd1,
            pnl_tracking_mode: PnlTrackingMode::Local,
            pnl_include_fees: true,
            wrapper_default_fee_bps: crate::rollout::DEFAULT_WRAPPER_FEE_BPS,
        }
    }

    #[test]
    fn canonical_config_serializes_default_policy_settings() {
        let config = canonical_config_from_legacy(&sample_settings(), &[]);

        assert_eq!(
            config["defaults"]["misc"]["defaultBuyFundingPolicy"],
            json!("prefer_usd1_else_topup")
        );
        assert_eq!(
            config["defaults"]["misc"]["defaultSellSettlementPolicy"],
            json!("always_to_usd1")
        );
        assert_eq!(
            config_default_buy_funding_policy(&config),
            BuyFundingPolicy::PreferUsd1ElseTopUp
        );
        assert_eq!(
            config_default_sell_settlement_policy(&config),
            SellSettlementPolicy::AlwaysToUsd1
        );
    }

    #[test]
    fn canonical_config_round_trips_wrapper_default_fee_bps() {
        let mut settings = sample_settings();
        settings.wrapper_default_fee_bps = 10;
        let config = canonical_config_from_legacy(&settings, &[]);
        assert_eq!(
            config["defaults"]["misc"]["wrapperDefaultFeeBps"],
            json!(10)
        );
        assert_eq!(config_wrapper_default_fee_bps(&config), 10);
    }

    #[test]
    fn canonical_config_clamps_out_of_range_wrapper_fee() {
        let mut config = canonical_config_from_legacy(&sample_settings(), &[]);
        if let Some(misc) = config
            .get_mut("defaults")
            .and_then(|value| value.get_mut("misc"))
            .and_then(Value::as_object_mut)
        {
            misc.insert("wrapperDefaultFeeBps".to_string(), json!(500));
        }
        assert_eq!(
            config_wrapper_default_fee_bps(&config),
            20,
            "values above the 20 bps cap must be clamped back down"
        );
    }

    #[test]
    fn set_wrapper_default_fee_bps_updates_config() {
        let base = canonical_config_from_legacy(&sample_settings(), &[]);
        let updated = set_wrapper_default_fee_bps_in_config(&base, 10);
        assert_eq!(config_wrapper_default_fee_bps(&updated), 10);
        let clamped = set_wrapper_default_fee_bps_in_config(&base, 99);
        assert_eq!(config_wrapper_default_fee_bps(&clamped), 20);
    }

    #[test]
    fn missing_wrapper_default_fee_bps_defaults_to_ten() {
        let mut config = canonical_config_from_legacy(&sample_settings(), &[]);
        if let Some(misc) = config
            .get_mut("defaults")
            .and_then(|value| value.get_mut("misc"))
            .and_then(Value::as_object_mut)
        {
            misc.remove("wrapperDefaultFeeBps");
        }
        assert_eq!(
            config_wrapper_default_fee_bps(&config),
            crate::rollout::DEFAULT_WRAPPER_FEE_BPS
        );
    }
}
