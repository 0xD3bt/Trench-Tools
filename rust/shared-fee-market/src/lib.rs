use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared_execution_routing::provider_tip::{
    provider_min_tip_sol_label, provider_required_tip_lamports,
};

pub const DEFAULT_AUTO_FEE_HELIUS_PRIORITY_LEVEL: &str = "high";
pub const DEFAULT_AUTO_FEE_JITO_TIP_PERCENTILE: &str = "p99";
pub const PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT: u64 = 1_000_000;

const AUTO_FEE_TOTAL_CAP_TIP_BPS: u64 = 7_000;
const AUTO_FEE_TOTAL_CAP_BPS_DENOMINATOR: u64 = 10_000;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeeMarketSnapshot {
    pub helius_priority_lamports: Option<u64>,
    pub helius_launch_priority_lamports: Option<u64>,
    pub helius_trade_priority_lamports: Option<u64>,
    pub jito_tip_p99_lamports: Option<u64>,
}

pub mod runtime;
pub use runtime::{
    AutoFeeDegradation, AutoFeeResolutionInput, AutoFeeResolutionOutput,
    DEFAULT_AUTO_FEE_BUFFER_PERCENT, DEFAULT_AUTO_FEE_FALLBACK_LAMPORTS,
    DEFAULT_HELIUS_PRIORITY_REFRESH_INTERVAL_MS, DEFAULT_HELIUS_PRIORITY_STALE_MS,
    DEFAULT_JITO_TIP_REFRESH_INTERVAL_MS, DEFAULT_JITO_TIP_STALE_MS,
    HELIUS_REFRESH_RETRY_BACKOFF_MS, RefreshOutcome, SharedFeeMarketCacheFile,
    SharedFeeMarketConfig, SharedFeeMarketLeaseStatus, SharedFeeMarketRuntime,
    SharedFeeMarketSnapshotStatus, apply_auto_fee_estimate_buffer, configured_auto_fee_buffer_bps,
    configured_helius_priority_refresh_interval, read_shared_fee_market_snapshot,
    resolve_buffered_auto_fee_components, shared_fee_market_status_payload,
};

impl FeeMarketSnapshot {
    pub fn launch_priority_lamports(&self) -> Option<u64> {
        self.helius_launch_priority_lamports
            .or(self.helius_priority_lamports)
    }

    pub fn trade_priority_lamports(&self) -> Option<u64> {
        self.helius_trade_priority_lamports
            .or(self.helius_priority_lamports)
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AutoFeeActionReport {
    pub enabled: bool,
    pub provider: String,
    pub prioritySource: String,
    pub priorityEstimateLamports: Option<u64>,
    pub resolvedPriorityLamports: Option<u64>,
    pub tipSource: String,
    pub tipEstimateLamports: Option<u64>,
    pub resolvedTipLamports: Option<u64>,
    pub capLamports: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AutoFeeReport {
    #[serde(rename = "jitoTipPercentile")]
    pub jito_tip_percentile: String,
    pub snapshot: FeeMarketSnapshot,
    pub creation: AutoFeeActionReport,
    pub buy: AutoFeeActionReport,
    pub sell: AutoFeeActionReport,
}

pub fn action_priority_estimate(
    snapshot: &FeeMarketSnapshot,
    action: &str,
) -> (Option<u64>, String) {
    match action {
        "creation" => snapshot
            .launch_priority_lamports()
            .map(|value| (Some(value), "launch-template".to_string()))
            .unwrap_or((None, "missing".to_string())),
        _ => snapshot
            .trade_priority_lamports()
            .map(|value| (Some(value), "trade-template".to_string()))
            .unwrap_or((None, "missing".to_string())),
    }
}

pub fn action_tip_estimate(
    snapshot: &FeeMarketSnapshot,
    jito_tip_percentile: &str,
) -> (Option<u64>, String) {
    if let Some(value) = snapshot.jito_tip_p99_lamports {
        (Some(value), format!("jito-{jito_tip_percentile}"))
    } else {
        (None, "missing".to_string())
    }
}

pub fn normalize_helius_priority_level(value: &str) -> String {
    let trimmed = value.trim().to_lowercase();
    match trimmed.as_str() {
        "none" => "Default".to_string(),
        "low" => "Low".to_string(),
        "medium" => "Medium".to_string(),
        "high" => "High".to_string(),
        "veryhigh" | "very_high" | "very-high" => "VeryHigh".to_string(),
        "unsafemax" | "unsafe_max" | "unsafe-max" => "UnsafeMax".to_string(),
        "recommended" => "recommended".to_string(),
        _ => "VeryHigh".to_string(),
    }
}

pub fn normalize_jito_tip_percentile(value: &str) -> String {
    let trimmed = value.trim().to_lowercase();
    match trimmed.as_str() {
        "p25" | "25" | "25th" => "p25".to_string(),
        "p50" | "50" | "50th" | "median" => "p50".to_string(),
        "p75" | "75" | "75th" => "p75".to_string(),
        "p95" | "95" | "95th" => "p95".to_string(),
        "p99" | "99" | "99th" => "p99".to_string(),
        _ => DEFAULT_AUTO_FEE_JITO_TIP_PERCENTILE.to_string(),
    }
}

pub fn parse_sol_decimal_to_lamports(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Some(0);
    }
    let normalized = trimmed.replace(',', ".");
    let mut parts = normalized.split('.');
    let whole = parts.next()?;
    let fractional = parts.next().unwrap_or("");
    if parts.next().is_some()
        || !whole.chars().all(|char| char.is_ascii_digit())
        || !fractional.chars().all(|char| char.is_ascii_digit())
    {
        return None;
    }
    let whole_value = whole.parse::<u64>().ok()?;
    let mut fractional_text = fractional.to_string();
    if fractional_text.len() > 9 {
        fractional_text.truncate(9);
    }
    while fractional_text.len() < 9 {
        fractional_text.push('0');
    }
    let fractional_value = if fractional_text.is_empty() {
        0
    } else {
        fractional_text.parse::<u64>().ok()?
    };
    whole_value
        .checked_mul(1_000_000_000)
        .and_then(|base| base.checked_add(fractional_value))
}

pub fn format_lamports_to_sol_decimal(value: u64) -> String {
    let whole = value / 1_000_000_000;
    let fractional = value % 1_000_000_000;
    if fractional == 0 {
        return whole.to_string();
    }
    let mut fractional_text = format!("{fractional:09}");
    while fractional_text.ends_with('0') {
        fractional_text.pop();
    }
    format!("{whole}.{fractional_text}")
}

pub fn lamports_to_priority_fee_micro_lamports(priority_fee_lamports: u64) -> u64 {
    if priority_fee_lamports == 0 {
        0
    } else {
        priority_fee_lamports
    }
}

pub fn provider_uses_auto_fee_priority(
    provider: &str,
    execution_class: &str,
    action: &str,
) -> bool {
    match provider.trim() {
        "standard-rpc" | "helius-sender" | "hellomoon" => true,
        "jito-bundle" => action != "creation" || execution_class != "bundle",
        _ => true,
    }
}

pub fn provider_uses_auto_fee_tip(provider: &str, action: &str) -> bool {
    let _ = action;
    matches!(
        provider.trim(),
        "helius-sender" | "hellomoon" | "jito-bundle"
    )
}

fn normalize_estimate_to_lamports(value: Option<&Value>) -> Option<u64> {
    let numeric = match value {
        Some(Value::Number(raw)) => raw.as_f64()?,
        Some(Value::String(raw)) => raw.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !numeric.is_finite() || numeric <= 0.0 {
        return None;
    }
    let lamports = if numeric < 1.0 {
        (numeric * 1_000_000_000.0).round()
    } else {
        numeric.round()
    };
    if lamports <= 0.0 {
        None
    } else {
        Some(lamports as u64)
    }
}

fn jito_tip_percentile_value<'a>(sample: &'a Value, percentile: &str) -> Option<&'a Value> {
    match percentile {
        "p25" => sample
            .get("p25")
            .or_else(|| sample.get("percentile25"))
            .or_else(|| sample.get("tipFloor25"))
            .or_else(|| sample.get("landed_tips_25th_percentile")),
        "p50" => sample
            .get("p50")
            .or_else(|| sample.get("percentile50"))
            .or_else(|| sample.get("tipFloor50"))
            .or_else(|| sample.get("landed_tips_50th_percentile")),
        "p75" => sample
            .get("p75")
            .or_else(|| sample.get("percentile75"))
            .or_else(|| sample.get("tipFloor75"))
            .or_else(|| sample.get("landed_tips_75th_percentile")),
        "p95" => sample
            .get("p95")
            .or_else(|| sample.get("percentile95"))
            .or_else(|| sample.get("tipFloor95"))
            .or_else(|| sample.get("landed_tips_95th_percentile")),
        _ => sample
            .get("p99")
            .or_else(|| sample.get("percentile99"))
            .or_else(|| sample.get("tipFloor99"))
            .or_else(|| sample.get("landed_tips_99th_percentile")),
    }
}

pub fn extract_jito_tip_floor_lamports(payload: &Value, percentile: &str) -> Option<u64> {
    if let Some(value) =
        normalize_estimate_to_lamports(jito_tip_percentile_value(payload, percentile))
    {
        return Some(value);
    }
    if let Some(value) = payload
        .get("params")
        .and_then(|params| params.get("result"))
        .and_then(|result| extract_jito_tip_floor_lamports(result, percentile))
    {
        return Some(value);
    }
    for key in ["data", "result", "value"] {
        if let Some(value) = payload
            .get(key)
            .and_then(|child| extract_jito_tip_floor_lamports(child, percentile))
        {
            return Some(value);
        }
    }
    payload.as_array().and_then(|entries| {
        entries
            .iter()
            .find_map(|entry| extract_jito_tip_floor_lamports(entry, percentile))
    })
}

pub fn helius_fee_estimate_options(helius_priority_level: &str) -> Value {
    if helius_priority_level == "recommended" {
        json!({
            "priorityLevel": "Medium",
            "recommended": true
        })
    } else {
        json!({
            "priorityLevel": helius_priority_level,
            "includeAllPriorityFeeLevels": true
        })
    }
}

fn helius_priority_level_value<'a>(
    result: &'a Value,
    helius_priority_level: &str,
) -> Option<&'a Value> {
    let levels = result.get("priorityFeeLevels")?;
    match helius_priority_level.trim().to_ascii_lowercase().as_str() {
        "default" => levels
            .get("medium")
            .or_else(|| levels.get("Medium"))
            .or_else(|| result.get("priorityFeeEstimate"))
            .or_else(|| result.get("recommended")),
        "low" => levels.get("low").or_else(|| levels.get("Low")),
        "medium" => levels.get("medium").or_else(|| levels.get("Medium")),
        "high" => levels.get("high").or_else(|| levels.get("High")),
        "veryhigh" | "very_high" | "very-high" => levels
            .get("veryHigh")
            .or_else(|| levels.get("VeryHigh"))
            .or_else(|| levels.get("veryhigh")),
        "unsafemax" | "unsafe_max" | "unsafe-max" => levels
            .get("unsafeMax")
            .or_else(|| levels.get("UnsafeMax"))
            .or_else(|| levels.get("unsafemax")),
        "recommended" => result
            .get("priorityFeeEstimate")
            .or_else(|| result.get("recommended")),
        selected_level => levels.get(selected_level),
    }
}

pub fn parse_helius_priority_estimate_result(
    result: &Value,
    helius_priority_level: &str,
) -> Option<u64> {
    normalize_estimate_to_lamports(
        helius_priority_level_value(result, helius_priority_level)
            .or_else(|| {
                result
                    .get("priorityFeeLevels")
                    .and_then(|levels| levels.get("veryHigh"))
            })
            .or_else(|| {
                result
                    .get("priorityFeeEstimate")
                    .or_else(|| result.get("recommended"))
            })
            .or_else(|| {
                result
                    .get("priorityFeeLevels")
                    .and_then(|levels| levels.get("high"))
            }),
    )
}

pub fn parse_auto_fee_cap_lamports(value: &str) -> Option<u64> {
    parse_sol_decimal_to_lamports(value).filter(|lamports| *lamports > 0)
}

fn cap_auto_fee_lamports(estimate_lamports: u64, cap_lamports: Option<u64>) -> u64 {
    match cap_lamports {
        Some(cap) if cap > 0 => estimate_lamports.min(cap),
        _ => estimate_lamports,
    }
}

pub fn resolve_auto_fee_components_with_total_cap(
    priority_estimate: Option<u64>,
    tip_estimate: Option<u64>,
    cap_lamports: Option<u64>,
    provider: &str,
    action_label: &str,
) -> Result<(Option<u64>, Option<u64>), String> {
    let priority_estimate = priority_estimate.map(|value| value.max(1));
    let has_priority = priority_estimate.is_some();
    let has_tip = tip_estimate.is_some();
    let minimum_tip_lamports = provider_required_tip_lamports(provider).unwrap_or(0);

    if let Some(cap) = cap_lamports {
        if has_priority && has_tip && minimum_tip_lamports > 0 && cap < minimum_tip_lamports {
            return Err(format!(
                "{} max auto fee is below the {} minimum tip of {} SOL.",
                action_label,
                provider,
                provider_min_tip_sol_label(provider)
            ));
        }
    }

    let resolved_priority = match priority_estimate {
        Some(estimate) if !has_tip => Some(cap_auto_fee_lamports(estimate, cap_lamports)),
        Some(estimate) => Some(estimate),
        None => None,
    };

    let resolved_tip = match tip_estimate {
        Some(estimate) if !has_priority => Some(clamp_auto_fee_tip_to_provider_minimum(
            cap_auto_fee_lamports(estimate, cap_lamports),
            provider,
            cap_lamports,
            action_label,
        )?),
        Some(estimate) => Some(estimate.max(minimum_tip_lamports)),
        None => None,
    };

    match (resolved_priority, resolved_tip, cap_lamports) {
        (Some(priority), Some(tip), Some(cap)) => {
            if u128::from(priority) + u128::from(tip) <= u128::from(cap) {
                return Ok((Some(priority), Some(tip)));
            }

            if minimum_tip_lamports > 0 && cap == minimum_tip_lamports {
                return Ok((Some(priority.min(1)), Some(minimum_tip_lamports)));
            }

            let ratio_tip_budget =
                cap.saturating_mul(AUTO_FEE_TOTAL_CAP_TIP_BPS) / AUTO_FEE_TOTAL_CAP_BPS_DENOMINATOR;
            let target_tip_budget = ratio_tip_budget
                .max(minimum_tip_lamports)
                .min(cap.saturating_sub(1));
            let target_priority_budget = cap.saturating_sub(target_tip_budget);

            let mut resolved_priority = priority.min(target_priority_budget);
            let mut resolved_tip = tip.min(target_tip_budget);
            let remaining_budget = cap
                .saturating_sub(resolved_priority)
                .saturating_sub(resolved_tip);

            if remaining_budget > 0 {
                if resolved_tip < target_tip_budget {
                    let extra_priority =
                        (priority.saturating_sub(resolved_priority)).min(remaining_budget);
                    resolved_priority = resolved_priority.saturating_add(extra_priority);
                } else if resolved_priority < target_priority_budget {
                    let extra_tip = (tip.saturating_sub(resolved_tip)).min(remaining_budget);
                    resolved_tip = resolved_tip.saturating_add(extra_tip);
                }
            }

            Ok((Some(resolved_priority), Some(resolved_tip)))
        }
        (priority, tip, _) => Ok((priority, tip)),
    }
}

pub fn clamp_auto_fee_tip_to_provider_minimum(
    resolved: u64,
    provider: &str,
    cap_lamports: Option<u64>,
    action_label: &str,
) -> Result<u64, String> {
    let Some(minimum_tip_lamports) = provider_required_tip_lamports(provider) else {
        return Ok(resolved);
    };
    if resolved >= minimum_tip_lamports {
        return Ok(resolved);
    }
    if cap_lamports.is_some() && cap_lamports.unwrap_or_default() < minimum_tip_lamports {
        return Err(format!(
            "{} max auto fee is below the {} minimum tip of {} SOL.",
            action_label,
            provider,
            provider_min_tip_sol_label(provider)
        ));
    }
    Ok(minimum_tip_lamports)
}

pub fn priority_price_micro_lamports_to_sol_equivalent(
    compute_unit_price_micro_lamports: u64,
) -> String {
    let total_lamports = (u128::from(compute_unit_price_micro_lamports)
        * u128::from(PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT))
        / 1_000_000;
    format_lamports_to_sol_decimal(total_lamports.min(u128::from(u64::MAX)) as u64)
}

pub fn format_priority_price_note(compute_unit_price_micro_lamports: u64) -> String {
    format!(
        "{} micro-lamports/CU (~{} SOL at {} CU)",
        compute_unit_price_micro_lamports,
        priority_price_micro_lamports_to_sol_equivalent(compute_unit_price_micro_lamports),
        PRIORITY_FEE_PRICE_BASE_COMPUTE_UNIT_LIMIT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_market_snapshot_prefers_launch_specific_estimate_for_creation() {
        let snapshot = FeeMarketSnapshot {
            helius_priority_lamports: Some(10),
            helius_launch_priority_lamports: Some(25),
            helius_trade_priority_lamports: Some(40),
            jito_tip_p99_lamports: Some(5),
        };
        assert_eq!(snapshot.launch_priority_lamports(), Some(25));
        assert_eq!(snapshot.trade_priority_lamports(), Some(40));
    }

    #[test]
    fn fee_market_snapshot_falls_back_to_generic_estimate_when_template_missing() {
        let snapshot = FeeMarketSnapshot {
            helius_priority_lamports: Some(10),
            helius_launch_priority_lamports: None,
            helius_trade_priority_lamports: None,
            jito_tip_p99_lamports: Some(5),
        };
        assert_eq!(snapshot.launch_priority_lamports(), Some(10));
        assert_eq!(snapshot.trade_priority_lamports(), Some(10));
    }

    #[test]
    fn hellomoon_auto_fee_policy_uses_both_priority_and_tip() {
        assert!(provider_uses_auto_fee_priority(
            "hellomoon",
            "single",
            "creation"
        ));
        assert!(provider_uses_auto_fee_tip("hellomoon", "creation"));
        assert!(provider_uses_auto_fee_priority(
            "hellomoon",
            "single",
            "buy"
        ));
        assert!(provider_uses_auto_fee_tip("hellomoon", "buy"));
        assert!(provider_uses_auto_fee_priority(
            "hellomoon",
            "single",
            "sell"
        ));
        assert!(provider_uses_auto_fee_tip("hellomoon", "sell"));
    }

    #[test]
    fn jito_bundle_auto_fee_policy_skips_priority_only_for_bundle_creation() {
        assert!(!provider_uses_auto_fee_priority(
            "jito-bundle",
            "bundle",
            "creation"
        ));
        assert!(provider_uses_auto_fee_tip("jito-bundle", "creation"));
        assert!(provider_uses_auto_fee_priority(
            "jito-bundle",
            "bundle",
            "buy"
        ));
        assert!(provider_uses_auto_fee_priority(
            "jito-bundle",
            "bundle",
            "sell"
        ));
    }

    #[test]
    fn helius_priority_estimate_parser_uses_selected_level_then_fallbacks() {
        let payload = json!({
            "priorityFeeLevels": {
                "medium": 222,
                "high": 1234,
                "veryHigh": 5678
            },
            "priorityFeeEstimate": 4321
        });
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "Medium"),
            Some(222)
        );
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "High"),
            Some(1234)
        );
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "veryhigh"),
            Some(5678)
        );
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "unsafeMax"),
            Some(5678)
        );
        assert_eq!(
            parse_helius_priority_estimate_result(&payload, "recommended"),
            Some(4321)
        );
    }

    #[test]
    fn jito_tip_floor_parser_handles_nested_payload_shapes() {
        let payload = json!({
            "params": {
                "result": [{
                    "data": {
                        "landed_tips_95th_percentile": 0.0015
                    }
                }]
            }
        });
        assert_eq!(
            extract_jito_tip_floor_lamports(&payload, "p95"),
            Some(1_500_000)
        );
    }

    #[test]
    fn clamp_auto_fee_tip_raises_zero_estimate_to_provider_minimum() {
        assert_eq!(
            clamp_auto_fee_tip_to_provider_minimum(0, "hellomoon", None, "Buy").unwrap(),
            1_000_000
        );
        assert_eq!(
            clamp_auto_fee_tip_to_provider_minimum(0, "helius-sender", None, "Buy").unwrap(),
            200_000
        );
    }

    #[test]
    fn clamp_auto_fee_tip_raises_subminimum_to_provider_floor() {
        assert_eq!(
            clamp_auto_fee_tip_to_provider_minimum(50_000, "hellomoon", None, "Sell").unwrap(),
            1_000_000
        );
    }

    #[test]
    fn clamp_auto_fee_tip_leaves_at_or_above_minimum_unchanged() {
        assert_eq!(
            clamp_auto_fee_tip_to_provider_minimum(2_000_000, "hellomoon", None, "Creation")
                .unwrap(),
            2_000_000
        );
    }

    #[test]
    fn clamp_auto_fee_tip_errors_when_cap_below_minimum() {
        let err = clamp_auto_fee_tip_to_provider_minimum(50_000, "hellomoon", Some(100_000), "Buy")
            .expect_err("cap below minimum");
        assert!(err.contains("max auto fee is below"));
        assert!(err.contains("hellomoon"));
    }

    #[test]
    fn clamp_auto_fee_tip_no_provider_minimum_passes_through() {
        assert_eq!(
            clamp_auto_fee_tip_to_provider_minimum(0, "standard-rpc", None, "Buy").unwrap(),
            0
        );
    }

    #[test]
    fn total_auto_fee_cap_is_shared_between_priority_and_tip() {
        let (priority, tip) = resolve_auto_fee_components_with_total_cap(
            Some(700_000),
            Some(1_500_000),
            Some(1_100_000),
            "standard-rpc",
            "Creation",
        )
        .unwrap();
        assert_eq!(priority, Some(330_000));
        assert_eq!(tip, Some(770_000));
    }

    #[test]
    fn total_auto_fee_cap_respects_tip_floor_when_ratio_is_too_low() {
        let (priority, tip) = resolve_auto_fee_components_with_total_cap(
            Some(700_000),
            Some(200_000),
            Some(1_200_000),
            "hellomoon",
            "Creation",
        )
        .unwrap();
        assert_eq!(priority, Some(200_000));
        assert_eq!(tip, Some(1_000_000));
    }

    #[test]
    fn total_auto_fee_cap_redistributes_unused_tip_share_to_priority() {
        let (priority, tip) = resolve_auto_fee_components_with_total_cap(
            Some(900_000),
            Some(100_000),
            Some(500_000),
            "standard-rpc",
            "Creation",
        )
        .unwrap();
        assert_eq!(priority, Some(400_000));
        assert_eq!(tip, Some(100_000));
    }

    #[test]
    fn total_auto_fee_cap_allows_exact_provider_tip_floor() {
        let (priority, tip) = resolve_auto_fee_components_with_total_cap(
            Some(700_000),
            Some(200_000),
            Some(1_000_000),
            "hellomoon",
            "Creation",
        )
        .expect("cap equal to provider minimum tip should resolve");

        assert_eq!(priority, Some(1));
        assert_eq!(tip, Some(1_000_000));
    }
}
