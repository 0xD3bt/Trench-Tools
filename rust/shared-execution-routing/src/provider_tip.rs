use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Helius Sender currently validates the tip transfer against static message keys only.
///
/// The active shared ALT intentionally excludes provider tip accounts so v0 compilation leaves
/// the recipient static. ALT-loaded provider tip recipients can simulate on chain but be rejected
/// by the submitting endpoint before forwarding.
pub const HELIUS_SENDER_TIP_ACCOUNTS: [&str; 1] = ["D1Mc6j9xQWgR1o1Z7yU5nVVXFQiAYx7FG9AW1aVfwrUM"];

/// Helius accounts that are useful for fee attribution/history, but must not be selected
/// as Sender destinations unless they are confirmed accepted as static tip recipients.
pub const HELIUS_SENDER_ALT_REJECTED_TIP_ACCOUNTS: [&str; 10] = [
    "4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE",
    "D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ",
    "9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta",
    "5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn",
    "2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD",
    "2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ",
    "wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF",
    "3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT",
    "4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey",
    "4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or",
];

pub const JITO_TIP_ACCOUNTS: [&str; 8] = [
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
    "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
    "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
    "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];

pub const HELLOMOON_TIP_ACCOUNTS: [&str; 10] = [
    "moon17L6BgxXRX5uHKudAmqVF96xia9h8ygcmG2sL3F",
    "moon26Sek222Md7ZydcAGxoKG832DK36CkLrS3PQY4c",
    "moon7fwyajcVstMoBnVy7UBcTx87SBtNoGGAaH2Cb8V",
    "moonBtH9HvLHjLqi9ivyrMVKgFUsSfrz9BwQ9khhn1u",
    "moonCJg8476LNFLptX1qrK8PdRsA1HD1R6XWyu9MB93",
    "moonF2sz7qwAtdETnrgxNbjonnhGGjd6r4W4UC9284s",
    "moonKfftMiGSak3cezvhEqvkPSzwrmQxQHXuspC96yj",
    "moonQBUKBpkifLcTd78bfxxt4PYLwmJ5admLW6cBBs8",
    "moonXwpKwoVkMegt5Bc776cSW793X1irL5hHV1vJ3JA",
    "moonZ6u9E2fgk6eWd82621eLPHt9zuJuYECXAYjMY1C",
];

pub fn provider_tip_accounts(provider: &str) -> &'static [&'static str] {
    match provider.trim() {
        "helius-sender" => &HELIUS_SENDER_TIP_ACCOUNTS,
        "hellomoon" => &HELLOMOON_TIP_ACCOUNTS,
        "jito-bundle" => &JITO_TIP_ACCOUNTS,
        _ => &[],
    }
}

pub fn pick_tip_account_for_provider(provider: &str) -> String {
    let accounts = provider_tip_accounts(provider);
    if accounts.is_empty() {
        return String::new();
    }
    let index = next_tip_account_index(accounts.len());
    accounts[index].to_string()
}

fn next_tip_account_index(len: usize) -> usize {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default();
    (nanos ^ counter.rotate_left(17)) as usize % len
}

/// Every tip-account address the engine may rotate through, across all known providers.
///
/// Used by fee parsers to attribute explicit tip lamports even when the submitting transport
/// rotates across the full provider account pool.
pub fn all_known_tip_accounts() -> impl Iterator<Item = &'static str> {
    HELIUS_SENDER_TIP_ACCOUNTS
        .iter()
        .copied()
        .chain(HELIUS_SENDER_ALT_REJECTED_TIP_ACCOUNTS.iter().copied())
        .chain(JITO_TIP_ACCOUNTS.iter().copied())
        .chain(HELLOMOON_TIP_ACCOUNTS.iter().copied())
}

pub fn provider_required_tip_lamports(provider: &str) -> Option<u64> {
    match provider.trim() {
        "helius-sender" => Some(200_000),
        "jito-bundle" => Some(1_000),
        "hellomoon" => Some(1_000_000),
        _ => None,
    }
}

pub fn provider_min_tip_sol_label(provider: &str) -> &'static str {
    match provider.trim() {
        "hellomoon" => "0.001",
        "jito-bundle" => "0.000001",
        _ => "0.0002",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_tip_accounts_expose_full_pools() {
        assert_eq!(provider_tip_accounts("helius-sender").len(), 1);
        assert_eq!(provider_tip_accounts("hellomoon").len(), 10);
        assert_eq!(provider_tip_accounts("jito-bundle").len(), 8);
    }

    #[test]
    fn tip_picker_only_returns_configured_accounts() {
        for provider in ["helius-sender", "hellomoon", "jito-bundle"] {
            let accounts = provider_tip_accounts(provider);
            for _ in 0..32 {
                let picked = pick_tip_account_for_provider(provider);
                assert!(accounts.contains(&picked.as_str()));
            }
        }
    }
}
