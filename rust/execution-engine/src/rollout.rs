use std::str::FromStr;

use solana_sdk::pubkey::Pubkey;

use crate::trade_planner::TradeVenueFamily;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeExecutionBackend {
    Native,
}

pub const DEFAULT_WRAPPER_FEE_BPS: u16 = 10;

impl TradeExecutionBackend {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Native => "native",
        }
    }
}

pub fn family_execution_enabled(family: &TradeVenueFamily) -> bool {
    match family {
        TradeVenueFamily::PumpBondingCurve | TradeVenueFamily::PumpAmm => {
            read_bool_env("EXECUTION_ENGINE_ENABLE_PUMP_NATIVE", true)
        }
        TradeVenueFamily::RaydiumAmmV4 => {
            read_bool_env("EXECUTION_ENGINE_ENABLE_RAYDIUM_AMM_V4_NATIVE", true)
        }
        TradeVenueFamily::BonkLaunchpad | TradeVenueFamily::BonkRaydium => {
            read_bool_env("EXECUTION_ENGINE_ENABLE_BONK_NATIVE", true)
        }
        TradeVenueFamily::MeteoraDbc | TradeVenueFamily::MeteoraDammV2 => {
            read_bool_env("EXECUTION_ENGINE_ENABLE_METEORA_NATIVE", true)
        }
        TradeVenueFamily::TrustedStableSwap => {
            read_bool_env("EXECUTION_ENGINE_ENABLE_TRUSTED_STABLE_SWAP", true)
        }
    }
}

/// Family-scoped warm-cache kill switch.
pub fn family_warm_enabled(family: &TradeVenueFamily) -> bool {
    match family {
        TradeVenueFamily::PumpBondingCurve | TradeVenueFamily::PumpAmm => {
            read_bool_env("EXECUTION_ENGINE_WARM_PUMP", true)
        }
        TradeVenueFamily::RaydiumAmmV4 => {
            read_bool_env("EXECUTION_ENGINE_WARM_RAYDIUM_AMM_V4", true)
        }
        TradeVenueFamily::BonkLaunchpad | TradeVenueFamily::BonkRaydium => {
            read_bool_env("EXECUTION_ENGINE_WARM_BONK", true)
        }
        TradeVenueFamily::MeteoraDbc | TradeVenueFamily::MeteoraDammV2 => {
            read_bool_env("EXECUTION_ENGINE_WARM_BAGS", true)
        }
        TradeVenueFamily::TrustedStableSwap => {
            read_bool_env("EXECUTION_ENGINE_WARM_TRUSTED_STABLE_SWAP", true)
        }
    }
}

/// Per-family warm kill switch identified by a resolved family label.
pub fn family_warm_enabled_by_label(family_label: &str) -> bool {
    match family_label.trim().to_ascii_lowercase().as_str() {
        "pump" | "pumpfun" | "pump.fun" | "pump-amm" | "pump_bonding_curve" => {
            read_bool_env("EXECUTION_ENGINE_WARM_PUMP", true)
        }
        "raydium" | "raydium-amm-v4" | "raydium_amm_v4" | "amm-v4" => {
            read_bool_env("EXECUTION_ENGINE_WARM_RAYDIUM_AMM_V4", true)
        }
        "bonk" | "letsbonk" | "lets-bonk" | "bonk-fun" => {
            read_bool_env("EXECUTION_ENGINE_WARM_BONK", true)
        }
        "bags" | "bagsapp" | "bags-app" | "meteora" | "dbc" | "damm" | "damm-v2" => {
            read_bool_env("EXECUTION_ENGINE_WARM_BAGS", true)
        }
        _ => true,
    }
}

mod runtime_policy {
    use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
    static ALLOW_NON_CANONICAL_SET: AtomicBool = AtomicBool::new(false);
    static ALLOW_NON_CANONICAL_VALUE: AtomicBool = AtomicBool::new(false);

    static WRAPPER_FEE_BPS_SET: AtomicBool = AtomicBool::new(false);
    static WRAPPER_FEE_BPS_VALUE: AtomicU16 = AtomicU16::new(0);

    pub fn set_allow_non_canonical(value: bool) {
        ALLOW_NON_CANONICAL_VALUE.store(value, Ordering::Release);
        ALLOW_NON_CANONICAL_SET.store(true, Ordering::Release);
    }

    pub fn allow_non_canonical_override() -> Option<bool> {
        if ALLOW_NON_CANONICAL_SET.load(Ordering::Acquire) {
            Some(ALLOW_NON_CANONICAL_VALUE.load(Ordering::Acquire))
        } else {
            None
        }
    }

    pub fn set_wrapper_default_fee_bps(value: u16) {
        WRAPPER_FEE_BPS_VALUE.store(value, Ordering::Release);
        WRAPPER_FEE_BPS_SET.store(true, Ordering::Release);
    }

    pub fn wrapper_default_fee_bps_override() -> Option<u16> {
        if WRAPPER_FEE_BPS_SET.load(Ordering::Acquire) {
            Some(WRAPPER_FEE_BPS_VALUE.load(Ordering::Acquire))
        } else {
            None
        }
    }

    #[cfg(test)]
    pub fn clear_wrapper_default_fee_bps_override() {
        WRAPPER_FEE_BPS_SET.store(false, Ordering::Release);
    }
}

/// Effective value of the non-canonical pool policy.
pub fn allow_non_canonical_pool_trades() -> bool {
    if let Some(value) = runtime_policy::allow_non_canonical_override() {
        return value;
    }
    read_bool_env("EXECUTION_ENGINE_ALLOW_NON_CANONICAL_POOL_TRADES", false)
}

pub fn set_allow_non_canonical_pool_trades(value: bool) {
    runtime_policy::set_allow_non_canonical(value);
}

pub fn family_guard_warning(family: &TradeVenueFamily) -> Option<String> {
    if family_execution_enabled(family) {
        return None;
    }
    Some(format!(
        "{} execution is currently disabled by rollout guard `{}`.",
        family.label(),
        family_guard_env_name(family)
    ))
}

pub fn preferred_execution_backend() -> TradeExecutionBackend {
    TradeExecutionBackend::Native
}

/// Default wrapper fee tier in basis points.
pub fn wrapper_default_fee_bps() -> u16 {
    if let Some(value) = runtime_policy::wrapper_default_fee_bps_override() {
        return clamp_fee_bps(value);
    }
    let raw = parse_trench_tool_fee_env()
        .or_else(parse_legacy_wrapper_fee_env)
        .unwrap_or(DEFAULT_WRAPPER_FEE_BPS);
    clamp_fee_bps(raw)
}

fn parse_trench_tool_fee_env() -> Option<u16> {
    let value = std::env::var("TRENCH_TOOL_FEE").ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed {
        "0" | "0.0" | "0.00" => Some(0),
        "0.1" | "0.10" => Some(10),
        "0.2" | "0.20" => Some(20),
        _ => trimmed.parse::<u16>().ok(),
    }
}

fn parse_legacy_wrapper_fee_env() -> Option<u16> {
    let value = std::env::var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS").ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        trimmed.parse::<u16>().ok()
    }
}

pub fn set_wrapper_default_fee_bps(value: u16) {
    runtime_policy::set_wrapper_default_fee_bps(clamp_fee_bps(value));
}

fn clamp_fee_bps(raw: u16) -> u16 {
    match raw {
        0 => 0,
        1..=10 => 10,
        11..=20 => 20,
        _ => 20,
    }
}

pub fn normalize_wrapper_fee_bps(raw: u16) -> u16 {
    clamp_fee_bps(raw)
}

pub fn runtime_execution_backend() -> TradeExecutionBackend {
    preferred_execution_backend()
}

const WRAPPER_FEE_VAULT_PUBKEY: &str = "7HKc2NAi2Q2ZG3eSN7VJrtBgGi7dNFAz9DLnPNDUncM2";

/// Resolve the fixed fee-vault pubkey.
pub fn wrapper_fee_vault_pubkey() -> Pubkey {
    Pubkey::from_str(WRAPPER_FEE_VAULT_PUBKEY)
        .expect("hardcoded wrapper fee vault pubkey must be valid")
}

pub fn family_guard_env_name(family: &TradeVenueFamily) -> &'static str {
    match family {
        TradeVenueFamily::PumpBondingCurve | TradeVenueFamily::PumpAmm => {
            "EXECUTION_ENGINE_ENABLE_PUMP_NATIVE"
        }
        TradeVenueFamily::RaydiumAmmV4 => "EXECUTION_ENGINE_ENABLE_RAYDIUM_AMM_V4_NATIVE",
        TradeVenueFamily::BonkLaunchpad | TradeVenueFamily::BonkRaydium => {
            "EXECUTION_ENGINE_ENABLE_BONK_NATIVE"
        }
        TradeVenueFamily::MeteoraDbc | TradeVenueFamily::MeteoraDammV2 => {
            "EXECUTION_ENGINE_ENABLE_METEORA_NATIVE"
        }
        TradeVenueFamily::TrustedStableSwap => "EXECUTION_ENGINE_ENABLE_TRUSTED_STABLE_SWAP",
    }
}

fn read_bool_env(key: &str, default_value: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => parse_bool_like(&value).unwrap_or(default_value),
        Err(_) => default_value,
    }
}

fn parse_bool_like(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_wrapper_default_fee_bps_override_for_tests() {
        runtime_policy::clear_wrapper_default_fee_bps_override();
    }

    #[test]
    fn parse_bool_like_handles_common_values() {
        assert_eq!(parse_bool_like("true"), Some(true));
        assert_eq!(parse_bool_like("OFF"), Some(false));
        assert_eq!(parse_bool_like("maybe"), None);
    }

    #[test]
    fn clamp_fee_bps_never_exceeds_cap() {
        assert_eq!(clamp_fee_bps(0), 0);
        assert_eq!(clamp_fee_bps(5), 10);
        assert_eq!(clamp_fee_bps(10), 10);
        assert_eq!(clamp_fee_bps(15), 20);
        assert_eq!(clamp_fee_bps(20), 20);
        assert_eq!(clamp_fee_bps(999), 20);
    }

    #[test]
    fn wrapper_default_fee_bps_runtime_override_is_clamped() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        set_wrapper_default_fee_bps(7);
        assert_eq!(wrapper_default_fee_bps(), 10);
        set_wrapper_default_fee_bps(100);
        assert_eq!(wrapper_default_fee_bps(), 20);
        set_wrapper_default_fee_bps(0);
        assert_eq!(wrapper_default_fee_bps(), 0);
        clear_wrapper_default_fee_bps_override_for_tests();
    }

    #[test]
    fn wrapper_default_fee_bps_defaults_to_ten_when_env_is_blank() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let prev = std::env::var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS").ok();
        let prev_trench = std::env::var("TRENCH_TOOL_FEE").ok();
        clear_wrapper_default_fee_bps_override_for_tests();
        unsafe {
            std::env::remove_var("TRENCH_TOOL_FEE");
            std::env::remove_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS");
        }
        assert_eq!(wrapper_default_fee_bps(), DEFAULT_WRAPPER_FEE_BPS);
        unsafe {
            std::env::set_var("TRENCH_TOOL_FEE", "");
        }
        assert_eq!(wrapper_default_fee_bps(), DEFAULT_WRAPPER_FEE_BPS);
        unsafe {
            std::env::remove_var("TRENCH_TOOL_FEE");
            std::env::set_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS", "");
        }
        assert_eq!(wrapper_default_fee_bps(), DEFAULT_WRAPPER_FEE_BPS);
        match prev {
            Some(value) => unsafe {
                std::env::set_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS", value);
            },
            None => unsafe {
                std::env::remove_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS");
            },
        }
        match prev_trench {
            Some(value) => unsafe {
                std::env::set_var("TRENCH_TOOL_FEE", value);
            },
            None => unsafe {
                std::env::remove_var("TRENCH_TOOL_FEE");
            },
        }
    }

    #[test]
    fn trench_tool_fee_env_accepts_percent_values() {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let prev = std::env::var("TRENCH_TOOL_FEE").ok();
        let prev_legacy = std::env::var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS").ok();
        clear_wrapper_default_fee_bps_override_for_tests();
        unsafe {
            std::env::remove_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS");
            std::env::set_var("TRENCH_TOOL_FEE", "0");
        }
        assert_eq!(wrapper_default_fee_bps(), 0);
        unsafe {
            std::env::set_var("TRENCH_TOOL_FEE", "0.1");
        }
        assert_eq!(wrapper_default_fee_bps(), 10);
        unsafe {
            std::env::set_var("TRENCH_TOOL_FEE", "0.2");
        }
        assert_eq!(wrapper_default_fee_bps(), 20);
        unsafe {
            std::env::set_var("TRENCH_TOOL_FEE", "20");
        }
        assert_eq!(wrapper_default_fee_bps(), 20);
        match prev {
            Some(value) => unsafe {
                std::env::set_var("TRENCH_TOOL_FEE", value);
            },
            None => unsafe {
                std::env::remove_var("TRENCH_TOOL_FEE");
            },
        }
        match prev_legacy {
            Some(value) => unsafe {
                std::env::set_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS", value);
            },
            None => unsafe {
                std::env::remove_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS");
            },
        }
    }

    #[test]
    fn preferred_backend_label_stays_native() {
        assert!(matches!(
            preferred_execution_backend(),
            TradeExecutionBackend::Native
        ));
    }

    #[test]
    fn normalize_wrapper_fee_bps_ladder_matches_onchain_cap() {
        // The wrapper program hardcodes 20 bps as the absolute ceiling;
        // `normalize_wrapper_fee_bps` is the engine's mirror of that
        // rule. Drift here means a settings payload could smuggle in a
        // tier the program rejects.
        assert_eq!(normalize_wrapper_fee_bps(0), 0);
        assert_eq!(normalize_wrapper_fee_bps(1), 10);
        assert_eq!(normalize_wrapper_fee_bps(10), 10);
        assert_eq!(normalize_wrapper_fee_bps(11), 20);
        assert_eq!(normalize_wrapper_fee_bps(20), 20);
        assert_eq!(normalize_wrapper_fee_bps(u16::MAX), 20);
    }
}
