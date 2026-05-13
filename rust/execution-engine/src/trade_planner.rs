use serde::{Deserialize, Serialize};
use shared_extension_runtime::follow_contract::BagsLaunchMetadata;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeLifecycle {
    PreMigration,
    PostMigration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeVenueFamily {
    PumpBondingCurve,
    PumpAmm,
    RaydiumAmmV4,
    RaydiumCpmm,
    RaydiumLaunchLab,
    TrustedStableSwap,
    BonkLaunchpad,
    BonkRaydium,
    MeteoraDbc,
    MeteoraDammV2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerQuoteAsset {
    Sol,
    Wsol,
    Usd1,
    Usdc,
    Usdt,
}

impl TradeLifecycle {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PreMigration => "pre_migration",
            Self::PostMigration => "post_migration",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerVerificationSource {
    OnchainDerived,
    HybridDerived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WrapperAction {
    PumpBondingCurveBuy,
    PumpBondingCurveSell,
    PumpAmmBuy,
    PumpAmmSell,
    PumpAmmWsolBuy,
    PumpAmmWsolSell,
    RaydiumAmmV4WsolBuy,
    RaydiumAmmV4WsolSell,
    RaydiumCpmmWsolBuy,
    RaydiumCpmmWsolSell,
    RaydiumLaunchLabSolBuy,
    RaydiumLaunchLabSolSell,
    TrustedStableSwapBuy,
    TrustedStableSwapSell,
    BonkLaunchpadSolBuy,
    BonkLaunchpadSolSell,
    BonkLaunchpadUsd1Buy,
    BonkLaunchpadUsd1Sell,
    BonkRaydiumSolBuy,
    BonkRaydiumSolSell,
    BonkRaydiumUsd1Buy,
    BonkRaydiumUsd1Sell,
    MeteoraDbcBuy,
    MeteoraDbcSell,
    MeteoraDammV2Buy,
    MeteoraDammV2Sell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PumpBondingCurveRuntimeBundle {
    pub mint: String,
    pub bonding_curve: String,
    pub bonding_curve_v2: String,
    pub fee_sharing_config: String,
    pub creator_vault_authority: String,
    pub launch_creator: String,
    pub token_program: String,
    pub associated_bonding_curve: String,
    pub global_volume_accumulator: String,
    pub fee_config: String,
    #[serde(default)]
    pub quote_mint: String,
    #[serde(default)]
    pub quote_token_program: String,
    #[serde(default)]
    pub buyback_fee_recipient: String,
    #[serde(default)]
    pub associated_quote_fee_recipient: String,
    #[serde(default)]
    pub associated_quote_buyback_fee_recipient: String,
    #[serde(default)]
    pub associated_quote_bonding_curve: String,
    #[serde(default)]
    pub associated_quote_user: String,
    #[serde(default)]
    pub associated_creator_vault: String,
    #[serde(default)]
    pub associated_user_volume_accumulator: String,
    pub is_mayhem_mode: bool,
    pub is_cashback_coin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PumpAmmRuntimeBundle {
    pub pool: String,
    pub pool_creator: String,
    pub base_mint: String,
    pub quote_mint: String,
    pub pool_base_token_account: String,
    pub pool_quote_token_account: String,
    pub mint_token_program: String,
    pub global_config: String,
    pub fee_config: String,
    pub protocol_fee_recipient: String,
    pub protocol_fee_recipient_token_account: String,
    pub coin_creator: String,
    pub coin_creator_vault_ata: String,
    pub coin_creator_vault_authority: String,
    pub global_volume_accumulator: String,
    pub is_mayhem_mode: bool,
    pub is_cashback_coin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaydiumAmmV4RuntimeBundle {
    pub pool: String,
    pub authority: String,
    pub base_mint: String,
    pub quote_mint: String,
    pub base_vault: String,
    pub quote_vault: String,
    pub open_orders: String,
    pub target_orders: String,
    pub market_program: String,
    pub market: String,
    pub market_bids: String,
    pub market_asks: String,
    pub market_event_queue: String,
    pub market_base_vault: String,
    pub market_quote_vault: String,
    pub market_vault_signer: String,
    pub mint_token_program: String,
    pub trade_fee_numerator: u64,
    pub trade_fee_denominator: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaydiumCpmmRuntimeBundle {
    pub pool: String,
    pub config_id: String,
    pub vault_a: String,
    pub vault_b: String,
    pub token_0_mint: String,
    pub token_1_mint: String,
    pub token_0_program: String,
    pub token_1_program: String,
    pub observation_id: String,
    pub mint_decimals_a: u8,
    pub mint_decimals_b: u8,
    pub protocol_fees_mint_a: u64,
    pub protocol_fees_mint_b: u64,
    pub fund_fees_mint_a: u64,
    pub fund_fees_mint_b: u64,
    pub enable_creator_fee: bool,
    pub creator_fees_mint_a: u64,
    pub creator_fees_mint_b: u64,
    pub reserve_a: u64,
    pub reserve_b: u64,
    pub trade_fee_rate: u64,
    pub creator_fee_rate: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BagsRuntimeBundle {
    pub bags_launch: BagsLaunchMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedStableRuntimeBundle {
    pub pool: String,
    pub venue: String,
    pub buy_input_mint: String,
    pub buy_output_mint: String,
    pub sell_input_mint: String,
    pub sell_output_mint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum PlannerRuntimeBundle {
    PumpBondingCurve(PumpBondingCurveRuntimeBundle),
    PumpAmm(PumpAmmRuntimeBundle),
    RaydiumAmmV4(RaydiumAmmV4RuntimeBundle),
    RaydiumCpmm(RaydiumCpmmRuntimeBundle),
    TrustedStable(TrustedStableRuntimeBundle),
    Bags(BagsRuntimeBundle),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleAndCanonicalMarket {
    pub lifecycle: TradeLifecycle,
    pub family: TradeVenueFamily,
    pub canonical_market_key: String,
    pub quote_asset: PlannerQuoteAsset,
    pub verification_source: PlannerVerificationSource,
    pub wrapper_action: WrapperAction,
    #[serde(default)]
    pub wrapper_accounts: Vec<String>,
    #[serde(default)]
    pub market_subtype: Option<String>,
    #[serde(default)]
    pub direct_protocol_target: Option<String>,
    #[serde(default)]
    pub input_amount_hint: Option<String>,
    #[serde(default)]
    pub minimum_output_hint: Option<String>,
    #[serde(default)]
    pub runtime_bundle: Option<PlannerRuntimeBundle>,
}

impl TradeVenueFamily {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PumpBondingCurve => "pump-bonding-curve",
            Self::PumpAmm => "pump-amm",
            Self::RaydiumAmmV4 => "raydium-amm-v4",
            Self::RaydiumCpmm => "raydium-cpmm",
            Self::RaydiumLaunchLab => "raydium-launchlab",
            Self::TrustedStableSwap => "trusted-stable-swap",
            Self::BonkLaunchpad => "bonk-launchpad",
            Self::BonkRaydium => "bonk-raydium",
            Self::MeteoraDbc => "meteora-dbc",
            Self::MeteoraDammV2 => "meteora-damm-v2",
        }
    }
}

impl PlannerQuoteAsset {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Sol => "SOL",
            Self::Wsol => "WSOL",
            Self::Usd1 => "USD1",
            Self::Usdc => "USDC",
            Self::Usdt => "USDT",
        }
    }
}

impl LifecycleAndCanonicalMarket {
    pub fn same_route_as(&self, other: &Self) -> bool {
        if self.lifecycle != other.lifecycle
            || self.family != other.family
            || self.canonical_market_key != other.canonical_market_key
            || self.quote_asset != other.quote_asset
            || self.wrapper_action != other.wrapper_action
        {
            return false;
        }
        match self.family {
            TradeVenueFamily::PumpBondingCurve | TradeVenueFamily::PumpAmm => true,
            TradeVenueFamily::RaydiumAmmV4
            | TradeVenueFamily::RaydiumCpmm
            | TradeVenueFamily::RaydiumLaunchLab
            | TradeVenueFamily::TrustedStableSwap
            | TradeVenueFamily::BonkLaunchpad
            | TradeVenueFamily::BonkRaydium
            | TradeVenueFamily::MeteoraDbc
            | TradeVenueFamily::MeteoraDammV2 => {
                normalize_market_subtype(self.market_subtype.as_deref())
                    == normalize_market_subtype(other.market_subtype.as_deref())
                    && self.wrapper_accounts == other.wrapper_accounts
            }
        }
    }
}

fn normalize_market_subtype(value: Option<&str>) -> &str {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venue_family_labels_are_stable() {
        assert_eq!(TradeVenueFamily::BonkLaunchpad.label(), "bonk-launchpad");
        assert_eq!(TradeVenueFamily::MeteoraDammV2.label(), "meteora-damm-v2");
        assert_eq!(TradeVenueFamily::RaydiumAmmV4.label(), "raydium-amm-v4");
        assert_eq!(TradeVenueFamily::RaydiumCpmm.label(), "raydium-cpmm");
        assert_eq!(
            TradeVenueFamily::RaydiumLaunchLab.label(),
            "raydium-launchlab"
        );
    }

    #[test]
    fn pump_routes_ignore_optional_metadata() {
        let left = LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PreMigration,
            family: TradeVenueFamily::PumpBondingCurve,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: PlannerQuoteAsset::Sol,
            verification_source: PlannerVerificationSource::OnchainDerived,
            wrapper_action: WrapperAction::PumpBondingCurveBuy,
            wrapper_accounts: vec!["pool-1".to_string()],
            market_subtype: Some("regular".to_string()),
            direct_protocol_target: Some("bonk-launchpad".to_string()),
            input_amount_hint: Some("0.5".to_string()),
            minimum_output_hint: None,
            runtime_bundle: None,
        };
        let mut right = left.clone();
        right.market_subtype = Some("bonkers".to_string());
        right.direct_protocol_target = Some("override".to_string());
        right.input_amount_hint = None;
        assert!(left.same_route_as(&right));
    }

    #[test]
    fn bonk_routes_require_metadata_identity() {
        let left = LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PreMigration,
            family: TradeVenueFamily::BonkLaunchpad,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: PlannerQuoteAsset::Usd1,
            verification_source: PlannerVerificationSource::HybridDerived,
            wrapper_action: WrapperAction::BonkLaunchpadUsd1Buy,
            wrapper_accounts: vec!["pool-1".to_string(), "config-a".to_string()],
            market_subtype: Some("regular".to_string()),
            direct_protocol_target: Some("bonk-launchpad".to_string()),
            input_amount_hint: Some("0.5".to_string()),
            minimum_output_hint: None,
            runtime_bundle: None,
        };
        let mut right = left.clone();
        right.market_subtype = Some("bonkers".to_string());
        assert!(!left.same_route_as(&right));

        let mut right = left.clone();
        right.wrapper_accounts.push("creator-b".to_string());
        assert!(!left.same_route_as(&right));
    }

    #[test]
    fn meteora_routes_require_metadata_identity() {
        let left = LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PostMigration,
            family: TradeVenueFamily::MeteoraDammV2,
            canonical_market_key: "pool-1".to_string(),
            quote_asset: PlannerQuoteAsset::Sol,
            verification_source: PlannerVerificationSource::OnchainDerived,
            wrapper_action: WrapperAction::MeteoraDammV2Buy,
            wrapper_accounts: vec!["pool-1".to_string(), "config-a".to_string()],
            market_subtype: Some("damm-v2".to_string()),
            direct_protocol_target: Some("meteora-damm-v2".to_string()),
            input_amount_hint: Some("1.0".to_string()),
            minimum_output_hint: None,
            runtime_bundle: None,
        };
        let mut right = left.clone();
        right.wrapper_accounts[1] = "config-b".to_string();
        assert!(!left.same_route_as(&right));
    }
}
