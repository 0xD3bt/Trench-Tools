#![allow(dead_code)]

use shared_extension_runtime::wallet::{self as shared_wallet, WalletRuntimeConfig};

pub use shared_extension_runtime::wallet::{WalletStatusSummary, WalletSummary};

pub(crate) fn ensure_wallet_runtime_configured() {
    shared_wallet::configure_wallet_runtime(
        WalletRuntimeConfig::new()
            .with_ata_cache_path(crate::paths::local_root_dir().join("wallet-ata-cache.json"))
            .with_before_rpc_request(crate::observability::record_outbound_provider_http_request),
    );
}

pub fn is_solana_wallet_env_key(key: &str) -> bool {
    ensure_wallet_runtime_configured();
    shared_wallet::is_solana_wallet_env_key(key)
}

pub fn read_keypair_bytes(raw: &str) -> Result<Vec<u8>, String> {
    ensure_wallet_runtime_configured();
    shared_wallet::read_keypair_bytes(raw)
}

pub fn public_key_from_secret(bytes: &[u8]) -> Result<String, String> {
    ensure_wallet_runtime_configured();
    shared_wallet::public_key_from_secret(bytes)
}

pub fn list_solana_env_wallets() -> Vec<WalletSummary> {
    ensure_wallet_runtime_configured();
    shared_wallet::list_solana_env_wallets()
}

pub fn selected_wallet_key_or_default(requested_key: &str) -> Option<String> {
    ensure_wallet_runtime_configured();
    shared_wallet::selected_wallet_key_or_default(requested_key)
}

pub fn selected_wallet_key_or_default_from_wallets(
    requested_key: &str,
    wallets: &[WalletSummary],
) -> Option<String> {
    ensure_wallet_runtime_configured();
    shared_wallet::selected_wallet_key_or_default_from_wallets(requested_key, wallets)
}

pub fn load_solana_wallet_by_env_key(env_key: &str) -> Result<Vec<u8>, String> {
    ensure_wallet_runtime_configured();
    shared_wallet::load_solana_wallet_by_env_key(env_key)
}

pub async fn fetch_balance_lamports(rpc_url: &str, public_key: &str) -> Result<u64, String> {
    ensure_wallet_runtime_configured();
    shared_wallet::fetch_balance_lamports(rpc_url, public_key).await
}

pub async fn fetch_token_balance(
    rpc_url: &str,
    public_key: &str,
    mint: &str,
    commitment: &str,
) -> Result<f64, String> {
    ensure_wallet_runtime_configured();
    shared_wallet::fetch_token_balance(rpc_url, public_key, mint, commitment).await
}

pub fn invalidate_wallet_balance_cache(env_keys: &[String]) {
    ensure_wallet_runtime_configured();
    shared_wallet::invalidate_wallet_balance_cache(env_keys)
}

pub async fn enrich_wallet_statuses(
    rpc_url: &str,
    usd1_mint: &str,
    wallets: &[WalletSummary],
) -> Vec<WalletStatusSummary> {
    ensure_wallet_runtime_configured();
    shared_wallet::enrich_wallet_statuses(rpc_url, usd1_mint, wallets).await
}

pub async fn enrich_wallet_statuses_with_options(
    rpc_url: &str,
    usd1_mint: &str,
    wallets: &[WalletSummary],
    force_refresh: bool,
) -> Vec<WalletStatusSummary> {
    ensure_wallet_runtime_configured();
    shared_wallet::enrich_wallet_statuses_with_options(rpc_url, usd1_mint, wallets, force_refresh)
        .await
}
