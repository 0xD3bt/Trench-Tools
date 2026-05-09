//! Static routing metadata used by the wrapper compile path.

use std::str::FromStr;

use solana_sdk::{message::AddressLookupTableAccount, pubkey::Pubkey, sysvar};

use crate::{
    trade_planner::{LifecycleAndCanonicalMarket, TradeVenueFamily},
    wrapper_abi::{
        PROGRAM_ID as WRAPPER_PROGRAM_ID, TOKEN_PROGRAM_ID, WSOL_MINT, config_pda,
        instructions_sysvar_id,
    },
};

/// Inner venue program IDs the wrapper is allowed to invoke.
pub const ALLOWED_INNER_PROGRAMS: &[(&str, &str)] = &[
    (
        "pump-bonding-curve",
        "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
    ),
    ("pump-amm", "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"),
    (
        "raydium-amm-v4",
        "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
    ),
    (
        "bonk-launchpad",
        "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj",
    ),
    (
        "raydium-launchlab",
        "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj",
    ),
    (
        "raydium-clmm",
        "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",
    ),
    (
        "orca-whirlpool",
        "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",
    ),
    (
        "raydium-cpmm",
        "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C",
    ),
    ("meteora-dbc", "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN"),
    (
        "meteora-damm-v2",
        "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG",
    ),
];

/// Which inner program the wrapper must CPI into for a given route.
pub fn inner_program_for_selector(
    selector: &LifecycleAndCanonicalMarket,
) -> Result<Pubkey, String> {
    let label = inner_program_label_for_selector(selector)?;
    inner_program_by_label(label)
}

/// Stringly-typed counterpart of `inner_program_for_selector` used by
/// runtime diagnostics and preview surfaces.
pub fn inner_program_label_for_selector(
    selector: &LifecycleAndCanonicalMarket,
) -> Result<&'static str, String> {
    let subtype = selector
        .market_subtype
        .as_deref()
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    let target = selector
        .direct_protocol_target
        .as_deref()
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();

    Ok(match selector.family {
        TradeVenueFamily::PumpBondingCurve => "pump-bonding-curve",
        TradeVenueFamily::PumpAmm => "pump-amm",
        TradeVenueFamily::RaydiumAmmV4 => "raydium-amm-v4",
        TradeVenueFamily::RaydiumCpmm => "raydium-cpmm",
        TradeVenueFamily::RaydiumLaunchLab => "raydium-launchlab",
        TradeVenueFamily::BonkLaunchpad => "bonk-launchpad",
        TradeVenueFamily::BonkRaydium => {
            if subtype.contains("cpmm") || target.contains("cpmm") {
                "raydium-cpmm"
            } else {
                "raydium-clmm"
            }
        }
        TradeVenueFamily::TrustedStableSwap => {
            if subtype.contains("orca") || target.contains("whirlb") {
                "orca-whirlpool"
            } else {
                "raydium-clmm"
            }
        }
        TradeVenueFamily::MeteoraDbc => "meteora-dbc",
        TradeVenueFamily::MeteoraDammV2 => "meteora-damm-v2",
    })
}

/// Returns the pubkey registered for a given allowlist label.
pub fn inner_program_by_label(label: &str) -> Result<Pubkey, String> {
    ALLOWED_INNER_PROGRAMS
        .iter()
        .find(|(entry, _)| *entry == label)
        .ok_or_else(|| format!("Unknown wrapper inner program label: {label}"))
        .and_then(|(_, pk)| {
            Pubkey::from_str(pk).map_err(|error| format!("Invalid pubkey for {label}: {error}"))
        })
}

/// All inner program pubkeys the on-chain allowlist is expected to
/// contain.
pub fn allowed_inner_program_pubkeys() -> Vec<Pubkey> {
    ALLOWED_INNER_PROGRAMS
        .iter()
        .filter_map(|(_, pk)| Pubkey::from_str(pk).ok())
        .collect()
}

/// Fixed addresses that must live in the shared ALT.
pub fn required_wrapper_alt_addresses(fee_vault: Pubkey) -> Vec<Pubkey> {
    let mut required = vec![
        WRAPPER_PROGRAM_ID,
        config_pda().0,
        instructions_sysvar_id(),
        sysvar::rent::ID,
        // Keep the system program explicit so coverage checks catch drift.
        solana_system_interface::program::ID,
        TOKEN_PROGRAM_ID,
        WSOL_MINT,
        fee_vault,
    ];
    required.extend(allowed_inner_program_pubkeys());
    required
}

/// Errors reported by `check_wrapper_alt_coverage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperAltCoverageError {
    MissingAddresses { missing: Vec<String> },
}

impl std::fmt::Display for WrapperAltCoverageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAddresses { missing } => write!(
                f,
                "shared ALT is missing wrapper-required addresses: [{}]",
                missing.join(", ")
            ),
        }
    }
}

impl std::error::Error for WrapperAltCoverageError {}

/// Validates that `alt` contains every required wrapper address.
pub fn check_wrapper_alt_coverage(
    alt: &AddressLookupTableAccount,
    fee_vault: Pubkey,
) -> Result<(), WrapperAltCoverageError> {
    let required = required_wrapper_alt_addresses(fee_vault);
    let mut missing: Vec<String> = Vec::new();
    for pk in &required {
        if !alt.addresses.iter().any(|entry| entry == pk) {
            missing.push(pk.to_string());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(WrapperAltCoverageError::MissingAddresses { missing })
    }
}

/// Validates that every allowlist entry parses cleanly.
pub fn validate_allowed_inner_program_entries() -> Result<(), String> {
    if ALLOWED_INNER_PROGRAMS.is_empty() {
        return Err("wrapper allowlist cannot be empty".to_string());
    }
    for (label, pk) in ALLOWED_INNER_PROGRAMS {
        if label.trim().is_empty() {
            return Err("wrapper allowlist entry has empty label".to_string());
        }
        Pubkey::from_str(pk).map_err(|error| {
            format!("wrapper allowlist entry {label} has invalid pubkey {pk}: {error}")
        })?;
    }
    let mut labels: Vec<&&str> = ALLOWED_INNER_PROGRAMS
        .iter()
        .map(|(label, _)| label)
        .collect();
    labels.sort();
    labels.dedup();
    if labels.len() != ALLOWED_INNER_PROGRAMS.len() {
        return Err("wrapper allowlist contains duplicate labels".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trade_planner::{
        LifecycleAndCanonicalMarket, PlannerQuoteAsset, PlannerVerificationSource, TradeLifecycle,
        TradeVenueFamily, WrapperAction,
    };

    fn selector_for(
        family: TradeVenueFamily,
        subtype: Option<&str>,
    ) -> LifecycleAndCanonicalMarket {
        LifecycleAndCanonicalMarket {
            lifecycle: TradeLifecycle::PreMigration,
            family,
            canonical_market_key: "pool".to_string(),
            quote_asset: PlannerQuoteAsset::Sol,
            verification_source: PlannerVerificationSource::OnchainDerived,
            wrapper_action: WrapperAction::PumpBondingCurveBuy,
            wrapper_accounts: vec![],
            market_subtype: subtype.map(|s| s.to_string()),
            direct_protocol_target: None,
            input_amount_hint: None,
            minimum_output_hint: None,
            runtime_bundle: None,
        }
    }

    #[test]
    fn allowlist_entries_are_well_formed() {
        validate_allowed_inner_program_entries().expect("allowlist integrity");
    }

    #[test]
    fn inner_program_resolves_every_family() {
        let cases: &[(TradeVenueFamily, Option<&str>, &str)] = &[
            (
                TradeVenueFamily::PumpBondingCurve,
                None,
                "pump-bonding-curve",
            ),
            (TradeVenueFamily::PumpAmm, None, "pump-amm"),
            (
                TradeVenueFamily::RaydiumAmmV4,
                Some("amm-v4"),
                "raydium-amm-v4",
            ),
            (TradeVenueFamily::RaydiumCpmm, Some("cpmm"), "raydium-cpmm"),
            (
                TradeVenueFamily::RaydiumLaunchLab,
                None,
                "raydium-launchlab",
            ),
            (TradeVenueFamily::BonkLaunchpad, None, "bonk-launchpad"),
            (TradeVenueFamily::BonkRaydium, Some("clmm"), "raydium-clmm"),
            (TradeVenueFamily::BonkRaydium, Some("cpmm"), "raydium-cpmm"),
            (TradeVenueFamily::MeteoraDbc, None, "meteora-dbc"),
            (
                TradeVenueFamily::MeteoraDammV2,
                Some("damm-v2"),
                "meteora-damm-v2",
            ),
        ];
        for (family, subtype, expected_label) in cases {
            let selector = selector_for(family.clone(), *subtype);
            let label = inner_program_label_for_selector(&selector).expect("label");
            assert_eq!(
                label, *expected_label,
                "unexpected label for {family:?} / {subtype:?}"
            );
            let pubkey = inner_program_for_selector(&selector).expect("pubkey");
            assert_ne!(pubkey, Pubkey::default());
        }
    }

    #[test]
    fn alt_coverage_detects_missing_wrapper_addresses() {
        let fee_vault = Pubkey::new_unique();
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            // Intentionally empty — every wrapper-required address is missing.
            addresses: vec![],
        };
        let err = check_wrapper_alt_coverage(&alt, fee_vault).unwrap_err();
        let WrapperAltCoverageError::MissingAddresses { missing } = err;
        assert!(
            missing
                .iter()
                .any(|pk| pk == &WRAPPER_PROGRAM_ID.to_string()),
            "wrapper program id must be reported missing"
        );
        assert!(
            missing.iter().any(|pk| pk == &fee_vault.to_string()),
            "fee vault must be reported missing"
        );
        for (_, pk) in ALLOWED_INNER_PROGRAMS {
            assert!(
                missing.iter().any(|entry| entry == pk),
                "allowlist entry {pk} must be reported missing"
            );
        }
    }

    #[test]
    fn alt_coverage_passes_when_addresses_present() {
        let fee_vault = Pubkey::new_unique();
        let required = required_wrapper_alt_addresses(fee_vault);
        let alt = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: required,
        };
        check_wrapper_alt_coverage(&alt, fee_vault).expect("coverage pass");
    }

    #[test]
    fn required_addresses_include_all_allowlisted_programs() {
        let fee_vault = Pubkey::new_unique();
        let required = required_wrapper_alt_addresses(fee_vault);
        for (_, pk_str) in ALLOWED_INNER_PROGRAMS {
            let pk = Pubkey::from_str(pk_str).expect("pubkey parse");
            assert!(
                required.contains(&pk),
                "wrapper required ALT addresses must include allowlisted program {pk_str}"
            );
        }
    }
}
