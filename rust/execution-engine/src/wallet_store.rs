use solana_sdk::signature::{Keypair, Signer};

use crate::shared_config::shared_config_manager;

pub fn load_solana_wallet_by_env_key(env_key: &str) -> Result<Keypair, String> {
    let key = env_key.trim();
    if key.is_empty() {
        return Err("Wallet key was empty.".to_string());
    }
    shared_config_manager().wallet_keypair(key)
}

pub fn public_key_string(keypair: &Keypair) -> String {
    keypair.pubkey().to_string()
}
