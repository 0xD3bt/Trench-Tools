use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use solana_sdk::signature::Keypair;

type EncodedSigner = [u8; 32];

fn signer_cache() -> &'static Mutex<HashMap<String, Vec<EncodedSigner>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<EncodedSigner>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn remember_compiled_transaction_signers(serialized_base64: &str, extra_signers: &[&Keypair]) {
    if serialized_base64.is_empty() || extra_signers.is_empty() {
        return;
    }
    let mut cache = signer_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(
        serialized_base64.to_string(),
        extra_signers
            .iter()
            .map(|signer| {
                let bytes = signer.to_bytes();
                let mut secret = [0u8; 32];
                secret.copy_from_slice(&bytes[..32]);
                secret
            })
            .collect(),
    );
}

pub fn restore_compiled_transaction_signers(serialized_base64: &str) -> Vec<Keypair> {
    if serialized_base64.is_empty() {
        return Vec::new();
    }
    let cache = signer_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache
        .get(serialized_base64)
        .into_iter()
        .flatten()
        .map(|secret| Keypair::new_from_array(*secret))
        .collect()
}
