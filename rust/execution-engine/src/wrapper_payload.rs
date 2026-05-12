//! Translate runtime trade requests into wrapper instruction payloads.

use crate::{
    extension_api::TradeSide,
    rollout::wrapper_default_fee_bps,
    trade_planner::{LifecycleAndCanonicalMarket, PlannerQuoteAsset, TradeVenueFamily},
    trade_runtime::{RuntimeSellIntent, TradeRuntimeRequest},
    wrapper_abi::{
        ABI_VERSION, MAX_FEE_BPS, SwapRouteDirection, SwapRouteFeeMode, SwapRouteMode,
        SwapRouteSettlement, WrapperRouteKind,
    },
};

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
const LAMPORT_DECIMALS: usize = 9;

/// High-level classifier for whether a trade touches SOL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapperRouteClassification {
    /// Route does not touch SOL in either direction.
    NoSol,
    /// SOL flows in (buy or SOL-funded USD1 top-up).
    SolIn,
    /// SOL flows out (sell settling to SOL).
    SolOut,
}

impl WrapperRouteClassification {
    pub fn to_abi(self) -> Option<WrapperRouteKind> {
        match self {
            Self::NoSol => None,
            Self::SolIn => Some(WrapperRouteKind::SolIn),
            Self::SolOut => Some(WrapperRouteKind::SolOut),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NoSol => "no-sol",
            Self::SolIn => "sol-in",
            Self::SolOut => "sol-out",
        }
    }

    pub fn touches_sol(self) -> bool {
        !matches!(self, Self::NoSol)
    }
}

/// Convenience: does this trade touch SOL at all?
pub fn trade_touches_sol(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
) -> bool {
    classify_trade_route(selector, request).touches_sol()
}

/// Classifies whether a trade request touches SOL.
pub fn classify_trade_route(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
) -> WrapperRouteClassification {
    let quote_touches_sol = matches!(
        selector.quote_asset,
        PlannerQuoteAsset::Sol | PlannerQuoteAsset::Wsol
    );
    let trusted_stable = selector.family == TradeVenueFamily::TrustedStableSwap;
    match request.side {
        TradeSide::Buy => {
            if quote_touches_sol {
                WrapperRouteClassification::SolIn
            } else if !trusted_stable
                && matches!(
                    request.policy.buy_funding_policy,
                    crate::extension_api::BuyFundingPolicy::SolOnly
                        | crate::extension_api::BuyFundingPolicy::PreferUsd1ElseTopUp
                )
            {
                // A USD1 top-up can still consume SOL on input.
                WrapperRouteClassification::SolIn
            } else {
                WrapperRouteClassification::NoSol
            }
        }
        TradeSide::Sell => {
            if quote_touches_sol {
                WrapperRouteClassification::SolOut
            } else if !trusted_stable
                && matches!(
                    request.policy.sell_settlement_policy,
                    crate::extension_api::SellSettlementPolicy::AlwaysToSol
                        | crate::extension_api::SellSettlementPolicy::MatchStoredEntryPreference
                )
            {
                // Conservatively treat stored-preference sells as SolOut.
                WrapperRouteClassification::SolOut
            } else {
                WrapperRouteClassification::NoSol
            }
        }
    }
}

/// Compiled wrapper-side metadata for a single trade.
#[derive(Debug, Clone)]
pub struct WrapperInstructionPayload {
    pub route_classification: WrapperRouteClassification,
    pub route_metadata: WrapperRouteMetadata,
    pub fee_lamports_estimate: u64,
    pub gross_sol_in_lamports: u64,
}

/// Local route/fee metadata for the v3 wrapper compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrapperRouteMetadata {
    pub version: u8,
    pub route_mode: Option<SwapRouteMode>,
    pub direction: Option<SwapRouteDirection>,
    pub settlement: Option<SwapRouteSettlement>,
    pub fee_mode: Option<SwapRouteFeeMode>,
    pub fee_bps: u16,
    pub gross_sol_in_lamports: u64,
    pub gross_token_in_amount: u64,
    pub min_net_output: u64,
}

/// Best-effort exact SOL-to-lamport conversion.
pub(crate) fn parse_sol_amount_to_lamports(raw: &str) -> u64 {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return 0;
    }
    if trimmed.starts_with('-') {
        return 0;
    }

    let (whole, frac) = trimmed.split_once('.').unwrap_or((trimmed, ""));
    if whole.is_empty() && frac.is_empty() {
        return 0;
    }
    let whole_lamports = if whole.is_empty() {
        0
    } else {
        let Ok(whole_sol) = whole.parse::<u64>() else {
            return 0;
        };
        let Some(value) = whole_sol.checked_mul(LAMPORTS_PER_SOL) else {
            return 0;
        };
        value
    };

    let mut frac_digits = frac.chars().take(LAMPORT_DECIMALS).collect::<String>();
    if frac_digits.chars().any(|ch| !ch.is_ascii_digit()) {
        return 0;
    }
    while frac_digits.len() < LAMPORT_DECIMALS {
        frac_digits.push('0');
    }
    let frac_lamports = if frac_digits.is_empty() {
        0
    } else {
        let Ok(value) = frac_digits.parse::<u64>() else {
            return 0;
        };
        value
    };
    whole_lamports.checked_add(frac_lamports).unwrap_or(0)
}

pub(crate) fn format_lamports_as_sol(lamports: u64) -> String {
    let whole = lamports / 1_000_000_000;
    let frac = lamports % 1_000_000_000;
    if frac == 0 {
        return whole.to_string();
    }
    let mut frac_text = format!("{frac:09}");
    while frac_text.ends_with('0') {
        frac_text.pop();
    }
    format!("{whole}.{frac_text}")
}

fn buy_input_lamports(request: &TradeRuntimeRequest) -> u64 {
    request
        .buy_amount_sol
        .as_deref()
        .map(parse_sol_amount_to_lamports)
        .unwrap_or(0)
}

fn clamp_fee_bps(raw: u16) -> u16 {
    if raw > MAX_FEE_BPS { MAX_FEE_BPS } else { raw }
}

fn floor_fee_lamports(gross: u64, fee_bps: u16) -> u64 {
    if fee_bps == 0 || gross == 0 {
        return 0;
    }
    let product = (gross as u128)
        .checked_mul(fee_bps as u128)
        .unwrap_or(u128::MAX);
    (product / 10_000) as u64
}

/// Build v3 route/fee metadata plus local diagnostics.
pub fn build_wrapper_instruction_payload(
    selector: &LifecycleAndCanonicalMarket,
    request: &TradeRuntimeRequest,
    _wallet_pubkey: String,
) -> WrapperInstructionPayload {
    let route_classification = classify_trade_route(selector, request);
    let fee_bps = clamp_fee_bps(wrapper_default_fee_bps());
    let gross_sol_in_lamports = match route_classification {
        WrapperRouteClassification::SolIn => buy_input_lamports(request),
        WrapperRouteClassification::SolOut | WrapperRouteClassification::NoSol => 0,
    };
    let min_net_output = match route_classification {
        WrapperRouteClassification::SolOut => sell_output_target_lamports(request),
        WrapperRouteClassification::SolIn | WrapperRouteClassification::NoSol => 0,
    };
    let route_metadata = WrapperRouteMetadata {
        version: ABI_VERSION,
        route_mode: match route_classification {
            WrapperRouteClassification::SolIn => Some(SwapRouteMode::SolIn),
            WrapperRouteClassification::SolOut => Some(SwapRouteMode::SolOut),
            WrapperRouteClassification::NoSol => None,
        },
        direction: match route_classification {
            WrapperRouteClassification::SolIn => Some(SwapRouteDirection::Buy),
            WrapperRouteClassification::SolOut => Some(SwapRouteDirection::Sell),
            WrapperRouteClassification::NoSol => None,
        },
        settlement: match route_classification {
            WrapperRouteClassification::SolIn => Some(SwapRouteSettlement::Token),
            WrapperRouteClassification::SolOut => Some(SwapRouteSettlement::NativeSol),
            WrapperRouteClassification::NoSol => None,
        },
        fee_mode: match route_classification {
            WrapperRouteClassification::SolIn => Some(SwapRouteFeeMode::SolPre),
            WrapperRouteClassification::SolOut => Some(SwapRouteFeeMode::NativeSolPost),
            WrapperRouteClassification::NoSol => None,
        },
        fee_bps,
        gross_sol_in_lamports,
        gross_token_in_amount: 0,
        min_net_output,
    };
    let fee_lamports_estimate = floor_fee_lamports(gross_sol_in_lamports, fee_bps);
    WrapperInstructionPayload {
        route_classification,
        route_metadata,
        fee_lamports_estimate,
        gross_sol_in_lamports,
    }
}

fn sell_output_target_lamports(request: &TradeRuntimeRequest) -> u64 {
    match request.sell_intent.as_ref() {
        Some(RuntimeSellIntent::SolOutput(value)) => parse_sol_amount_to_lamports(value),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        extension_api::{
            BuyFundingPolicy, MevMode, SellSettlementPolicy, TradeSettlementAsset, TradeSide,
        },
        trade_planner::{
            PlannerQuoteAsset, PlannerVerificationSource, TradeLifecycle, TradeVenueFamily,
            WrapperAction,
        },
        trade_runtime::{RuntimeExecutionPolicy, RuntimeSellIntent, TradeRuntimeRequest},
    };

    fn policy(
        funding: BuyFundingPolicy,
        settlement: SellSettlementPolicy,
        settlement_asset: TradeSettlementAsset,
    ) -> RuntimeExecutionPolicy {
        RuntimeExecutionPolicy {
            slippage_percent: "5".to_string(),
            mev_mode: MevMode::Off,
            auto_tip_enabled: false,
            fee_sol: "0".to_string(),
            tip_sol: "0".to_string(),
            provider: "standard-rpc".to_string(),
            endpoint_profile: "global".to_string(),
            commitment: "confirmed".to_string(),
            skip_preflight: false,
            track_send_block_height: false,
            buy_funding_policy: funding,
            sell_settlement_policy: settlement,
            sell_settlement_asset: settlement_asset,
        }
    }

    fn selector(quote: PlannerQuoteAsset, family: TradeVenueFamily) -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PreMigration,
            family,
            canonical_market_key: "pool".to_string(),
            quote_asset: quote,
            verification_source: PlannerVerificationSource::OnchainDerived,
            wrapper_action: WrapperAction::PumpBondingCurveBuy,
            wrapper_accounts: vec!["pool".to_string()],
            market_subtype: None,
            direct_protocol_target: None,
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    fn buy_request(funding: BuyFundingPolicy) -> TradeRuntimeRequest {
        TradeRuntimeRequest {
            side: TradeSide::Buy,
            mint: "Mint1".to_string(),
            buy_amount_sol: Some("0.5".to_string()),
            sell_intent: None,
            policy: policy(
                funding,
                SellSettlementPolicy::AlwaysToSol,
                TradeSettlementAsset::Sol,
            ),
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: None,
            warm_key: None,
            fallback_mint_hint: None,
        }
    }

    fn sell_request(settlement: SellSettlementPolicy) -> TradeRuntimeRequest {
        TradeRuntimeRequest {
            side: TradeSide::Sell,
            mint: "Mint1".to_string(),
            buy_amount_sol: None,
            sell_intent: Some(RuntimeSellIntent::Percent("100".to_string())),
            policy: policy(
                BuyFundingPolicy::SolOnly,
                settlement,
                TradeSettlementAsset::Sol,
            ),
            platform_label: None,
            planned_route: None,
            planned_trade: None,
            pinned_pool: None,
            warm_key: None,
            fallback_mint_hint: None,
        }
    }

    #[test]
    fn floor_fee_lamports_rounds_down() {
        // 0.5 SOL = 500_000_000 lamports, 10 bps = 500_000 lamports
        assert_eq!(floor_fee_lamports(500_000_000, 10), 500_000);
        // Dust: 1 lamport * 10 bps -> 0 (floor)
        assert_eq!(floor_fee_lamports(1, 10), 0);
        // Zero fee bps -> zero
        assert_eq!(floor_fee_lamports(1_000_000, 0), 0);
    }

    #[test]
    fn buy_on_sol_route_classifies_as_sol_in() {
        let selector = selector(PlannerQuoteAsset::Sol, TradeVenueFamily::PumpBondingCurve);
        let request = buy_request(BuyFundingPolicy::SolOnly);
        assert_eq!(
            classify_trade_route(&selector, &request),
            WrapperRouteClassification::SolIn
        );
    }

    #[test]
    fn buy_on_usd1_route_with_sol_funding_classifies_as_sol_in() {
        let selector = selector(PlannerQuoteAsset::Usd1, TradeVenueFamily::BonkLaunchpad);
        let request = buy_request(BuyFundingPolicy::SolOnly);
        assert_eq!(
            classify_trade_route(&selector, &request),
            WrapperRouteClassification::SolIn
        );
    }

    #[test]
    fn buy_on_usd1_route_with_usd1_only_does_not_touch_sol() {
        let selector = selector(PlannerQuoteAsset::Usd1, TradeVenueFamily::BonkLaunchpad);
        let request = buy_request(BuyFundingPolicy::Usd1Only);
        assert_eq!(
            classify_trade_route(&selector, &request),
            WrapperRouteClassification::NoSol
        );
    }

    #[test]
    fn buy_on_usd1_route_with_prefer_usd1_else_topup_marks_as_sol_in() {
        let selector = selector(PlannerQuoteAsset::Usd1, TradeVenueFamily::BonkLaunchpad);
        let request = buy_request(BuyFundingPolicy::PreferUsd1ElseTopUp);
        assert_eq!(
            classify_trade_route(&selector, &request),
            WrapperRouteClassification::SolIn
        );
    }

    #[test]
    fn sell_settling_to_sol_classifies_as_sol_out() {
        let selector = selector(PlannerQuoteAsset::Usd1, TradeVenueFamily::BonkLaunchpad);
        let request = sell_request(SellSettlementPolicy::AlwaysToSol);
        assert_eq!(
            classify_trade_route(&selector, &request),
            WrapperRouteClassification::SolOut
        );
    }

    #[test]
    fn sell_settling_to_usd1_does_not_touch_sol() {
        let selector = selector(PlannerQuoteAsset::Usd1, TradeVenueFamily::BonkLaunchpad);
        let request = sell_request(SellSettlementPolicy::AlwaysToUsd1);
        assert_eq!(
            classify_trade_route(&selector, &request),
            WrapperRouteClassification::NoSol
        );
    }

    #[test]
    fn sell_match_stored_entry_is_conservatively_sol_out() {
        let selector = selector(PlannerQuoteAsset::Usd1, TradeVenueFamily::BonkLaunchpad);
        let request = sell_request(SellSettlementPolicy::MatchStoredEntryPreference);
        assert_eq!(
            classify_trade_route(&selector, &request),
            WrapperRouteClassification::SolOut
        );
    }

    #[test]
    fn build_payload_uses_neutral_v3_route_metadata() {
        let payload = build_wrapper_instruction_payload(
            &selector(PlannerQuoteAsset::Sol, TradeVenueFamily::PumpBondingCurve),
            &buy_request(BuyFundingPolicy::SolOnly),
            "wallet".to_string(),
        );

        assert_eq!(payload.route_metadata.version, ABI_VERSION);
        assert_eq!(
            payload.route_metadata.route_mode,
            Some(SwapRouteMode::SolIn)
        );
        assert_eq!(
            payload.route_metadata.direction,
            Some(SwapRouteDirection::Buy)
        );
        assert_eq!(
            payload.route_metadata.fee_mode,
            Some(SwapRouteFeeMode::SolPre)
        );
        assert_eq!(payload.route_metadata.gross_sol_in_lamports, 500_000_000);
    }

    #[test]
    fn sell_output_sol_sets_hard_net_output_floor() {
        let mut request = sell_request(SellSettlementPolicy::AlwaysToSol);
        request.sell_intent = Some(RuntimeSellIntent::SolOutput("1.25".to_string()));
        let payload = build_wrapper_instruction_payload(
            &selector(PlannerQuoteAsset::Sol, TradeVenueFamily::PumpBondingCurve),
            &request,
            "wallet".to_string(),
        );
        assert_eq!(payload.route_metadata.min_net_output, 1_250_000_000);
    }

    #[test]
    fn percent_sell_keeps_slippage_inferred_net_output_floor() {
        let payload = build_wrapper_instruction_payload(
            &selector(PlannerQuoteAsset::Sol, TradeVenueFamily::PumpBondingCurve),
            &sell_request(SellSettlementPolicy::AlwaysToSol),
            "wallet".to_string(),
        );
        assert_eq!(payload.route_metadata.min_net_output, 0);
    }

    #[test]
    fn parse_sol_amount_to_lamports_is_exact() {
        assert_eq!(parse_sol_amount_to_lamports("1"), 1_000_000_000);
        assert_eq!(parse_sol_amount_to_lamports("0.1"), 100_000_000);
        assert_eq!(parse_sol_amount_to_lamports("0.000000001"), 1);
        assert_eq!(parse_sol_amount_to_lamports("0.0000000019"), 1);
        assert_eq!(parse_sol_amount_to_lamports("-1"), 0);
    }

    #[test]
    fn build_payload_clamps_fee_bps_to_cap() {
        // Temporarily override the env so the default reader returns a
        // too-high value.
        // SAFETY: tests in this module run sequentially and restore env.
        let prev = std::env::var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS").ok();
        let prev_trench = std::env::var("TRENCH_TOOL_FEE").ok();
        // SAFETY: setting process-wide env during tests is acceptable
        // here because this module's tests run single-threaded via
        // cargo's default per-process isolation.
        unsafe {
            std::env::remove_var("TRENCH_TOOL_FEE");
            std::env::set_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS", "999");
        }
        let payload = build_wrapper_instruction_payload(
            &selector(PlannerQuoteAsset::Sol, TradeVenueFamily::PumpBondingCurve),
            &buy_request(BuyFundingPolicy::SolOnly),
            "wallet".to_string(),
        );
        assert!(payload.route_metadata.fee_bps <= MAX_FEE_BPS);
        match prev {
            Some(value) => unsafe {
                std::env::set_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS", value);
            },
            None => unsafe {
                std::env::remove_var("EXECUTION_ENGINE_WRAPPER_DEFAULT_FEE_BPS");
            },
        };
        match prev_trench {
            Some(value) => unsafe {
                std::env::set_var("TRENCH_TOOL_FEE", value);
            },
            None => unsafe {
                std::env::remove_var("TRENCH_TOOL_FEE");
            },
        };
    }
}
