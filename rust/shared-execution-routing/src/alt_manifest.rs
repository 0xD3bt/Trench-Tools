use serde::{Deserialize, Serialize};

pub const SHARED_SUPER_LOOKUP_TABLE: &str = "7CaMLcAuSskoeN7HoRwZjsSthU8sMwKqxtXkyMiMjuc";
pub const RAYDIUM_AMM_V4_PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
pub const RAYDIUM_AMM_V4_AUTHORITY: &str = "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1";
pub const OPENBOOK_DEX_PROGRAM_ID: &str = "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX";
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
pub const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
pub const JITODONTFRONT_ACCOUNT: &str = "jitodontfront111111111111111111111111111111";
pub const RENT_SYSVAR_ID: &str = "SysvarRent111111111111111111111111111111111";

pub const PUMP_APR28_FEE_RECIPIENTS: [&str; 8] = [
    "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD",
    "9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7",
    "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL",
    "3BpXnfJaUTiwXnJNe7Ej1rcbzqTTQUvLShZaWazebsVR",
    "5cjcW9wExnJJiqgLjq7DEG75Pm6JBgE1hNv4B2vHXUW6",
    "EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL",
    "5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD",
    "A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW",
];

pub const SELECTED_PUMP_APR28_FEE_RECIPIENT: &str = PUMP_APR28_FEE_RECIPIENTS[0];
pub const SELECTED_PUMP_APR28_WSOL_FEE_RECIPIENT_ATA: &str =
    "HjQjngTDqoHE6aaGhUqfz9aQ7WZcBRjy5xB8PScLSr8i";
pub const WRAPPER_FEE_VAULT_WSOL_ATA: &str = "2HLoA8PQuxqUfNDVa6kCL8CZ1FkDMcqZZSE3HDEpKqSZ";
pub const ORCA_WHIRLPOOL_PROGRAM_ID: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AltManifestEntry {
    pub address: String,
    pub family: String,
    pub label: String,
    pub reason: String,
    pub required: bool,
}

impl AltManifestEntry {
    pub fn required(address: impl Into<String>, family: &str, label: &str, reason: &str) -> Self {
        Self {
            address: address.into(),
            family: family.to_string(),
            label: label.to_string(),
            reason: reason.to_string(),
            required: true,
        }
    }
}

pub fn pump_apr28_fee_recipient_manifest_entries() -> Vec<AltManifestEntry> {
    let mut entries = PUMP_APR28_FEE_RECIPIENTS
        .iter()
        .map(|address| {
            AltManifestEntry::required(
                *address,
                "pump-upgrade",
                "pump-apr28-common-fee-recipient",
                "Pump April 28 bonding-curve and AMM instructions emit one common fee recipient",
            )
        })
        .collect::<Vec<_>>();
    entries.push(AltManifestEntry::required(
        SELECTED_PUMP_APR28_WSOL_FEE_RECIPIENT_ATA,
        "pump-upgrade",
        "pump-apr28-wsol-fee-recipient-ata",
        "Execution-engine Pump AMM WSOL quote routes emit the selected April 28 fee-recipient ATA",
    ));
    entries
}

pub fn wrapper_alt_manifest_entries() -> Vec<AltManifestEntry> {
    vec![
        AltManifestEntry::required(
            WRAPPER_FEE_VAULT_WSOL_ATA,
            "wrapper",
            "wrapper-fee-vault-wsol-ata",
            "Wrapper SolOut routes can carry the fee-vault WSOL ATA as a fixed account",
        ),
        AltManifestEntry::required(
            ORCA_WHIRLPOOL_PROGRAM_ID,
            "trusted-stable",
            "orca-whirlpool-program",
            "Trusted stable SOL/USDC route can invoke the sealed Orca Whirlpool program",
        ),
    ]
}

pub fn raydium_amm_v4_alt_manifest_entries() -> Vec<AltManifestEntry> {
    vec![
        AltManifestEntry::required(
            RAYDIUM_AMM_V4_PROGRAM_ID,
            "raydium-amm-v4",
            "raydium-amm-v4-program",
            "Generic Raydium AMM v4 routes invoke the AMM program for every pool",
        ),
        AltManifestEntry::required(
            RAYDIUM_AMM_V4_AUTHORITY,
            "raydium-amm-v4",
            "raydium-amm-v4-authority",
            "Generic Raydium AMM v4 routes pass the fixed AMM authority",
        ),
        AltManifestEntry::required(
            OPENBOOK_DEX_PROGRAM_ID,
            "raydium-amm-v4",
            "openbook-dex-program",
            "Raydium AMM v4 market_program commonly resolves to OpenBook",
        ),
        AltManifestEntry::required(
            TOKEN_PROGRAM_ID,
            "raydium-amm-v4",
            "spl-token-program",
            "Raydium AMM v4 swaps and WSOL lifecycle instructions use SPL Token",
        ),
        AltManifestEntry::required(
            ASSOCIATED_TOKEN_PROGRAM_ID,
            "raydium-amm-v4",
            "associated-token-program",
            "Raydium AMM v4 buys create the output ATA idempotently",
        ),
        AltManifestEntry::required(
            SYSTEM_PROGRAM_ID,
            "raydium-amm-v4",
            "system-program",
            "Raydium AMM v4 WSOL setup creates a temporary token account",
        ),
        AltManifestEntry::required(
            COMPUTE_BUDGET_PROGRAM_ID,
            "raydium-amm-v4",
            "compute-budget-program",
            "Raydium AMM v4 transactions set compute limits and priority fees",
        ),
        AltManifestEntry::required(
            WSOL_MINT,
            "raydium-amm-v4",
            "wsol-mint",
            "Generic Raydium AMM v4 routing is restricted to WSOL pairs",
        ),
        AltManifestEntry::required(
            MEMO_PROGRAM_ID,
            "raydium-amm-v4",
            "memo-program",
            "Raydium AMM v4 transactions add a uniqueness memo",
        ),
        AltManifestEntry::required(
            JITODONTFRONT_ACCOUNT,
            "raydium-amm-v4",
            "jitodontfront-account",
            "Secure/reduced MEV Raydium AMM v4 routes append Jito's dont-front account",
        ),
        AltManifestEntry::required(
            RENT_SYSVAR_ID,
            "raydium-amm-v4",
            "rent-sysvar",
            "Wrapper v2 and token-account setup paths can reference the rent sysvar",
        ),
    ]
}

pub fn shared_alt_manifest_entries() -> Vec<AltManifestEntry> {
    let mut entries = vec![AltManifestEntry::required(
        SHARED_SUPER_LOOKUP_TABLE,
        "shared-alt",
        "active-shared-table",
        "Shared table used by execution-engine and LaunchDeck v0 compilers",
    )];
    entries.extend(pump_apr28_fee_recipient_manifest_entries());
    entries.extend(wrapper_alt_manifest_entries());
    entries.extend(raydium_amm_v4_alt_manifest_entries());
    entries
}

pub fn lookup_table_address_content_hash(addresses: &[String]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for address in addresses {
        for byte in address.as_bytes().iter().copied().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_tip::all_known_tip_accounts;

    #[test]
    fn shared_manifest_includes_all_pump_apr28_fee_recipients() {
        let entries = shared_alt_manifest_entries();
        for recipient in PUMP_APR28_FEE_RECIPIENTS {
            assert!(entries.iter().any(|entry| entry.address == recipient));
        }
    }

    #[test]
    fn shared_manifest_excludes_provider_tip_accounts() {
        let entries = shared_alt_manifest_entries();
        for tip_account in all_known_tip_accounts() {
            assert!(!entries.iter().any(|entry| entry.address == tip_account));
        }
    }

    #[test]
    fn shared_manifest_includes_trusted_stable_orca_program() {
        let entries = shared_alt_manifest_entries();
        assert!(
            entries
                .iter()
                .any(|entry| entry.address == ORCA_WHIRLPOOL_PROGRAM_ID)
        );
    }

    #[test]
    fn shared_manifest_includes_raydium_amm_v4_reusable_addresses() {
        let entries = shared_alt_manifest_entries();
        for address in [
            RAYDIUM_AMM_V4_PROGRAM_ID,
            RAYDIUM_AMM_V4_AUTHORITY,
            OPENBOOK_DEX_PROGRAM_ID,
            TOKEN_PROGRAM_ID,
            ASSOCIATED_TOKEN_PROGRAM_ID,
            SYSTEM_PROGRAM_ID,
            COMPUTE_BUDGET_PROGRAM_ID,
            WSOL_MINT,
            MEMO_PROGRAM_ID,
            JITODONTFRONT_ACCOUNT,
            RENT_SYSVAR_ID,
        ] {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.family == "raydium-amm-v4" && entry.address == address),
                "missing Raydium AMM v4 reusable ALT address {address}"
            );
        }
    }

    #[test]
    fn lookup_table_content_hash_changes_when_entries_change() {
        let first = lookup_table_address_content_hash(&["A".to_string(), "B".to_string()]);
        let second = lookup_table_address_content_hash(&["A".to_string(), "C".to_string()]);
        assert_ne!(first, second);
    }
}
