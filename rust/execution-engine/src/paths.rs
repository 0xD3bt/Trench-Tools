use std::{env, path::PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
        })
}

fn launchdeck_local_root_dir() -> PathBuf {
    if let Ok(explicit) = env::var("LAUNCHDECK_LOCAL_DATA_DIR") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    repo_root().join(".local").join("launchdeck")
}

fn launchdeck_shared_lookup_table_cache_path() -> PathBuf {
    launchdeck_local_root_dir().join("shared-lookup-tables.json")
}

pub fn shared_lookup_table_cache_path() -> PathBuf {
    launchdeck_shared_lookup_table_cache_path()
}

pub fn shared_fee_market_cache_path() -> PathBuf {
    launchdeck_local_root_dir().join("shared-fee-market.json")
}

pub fn legacy_bonk_lookup_table_cache_path() -> PathBuf {
    launchdeck_local_root_dir().join("bonk-lookup-tables.json")
}

pub fn bonk_lookup_table_cache_path() -> PathBuf {
    launchdeck_shared_lookup_table_cache_path()
}

pub fn bags_credentials_path() -> PathBuf {
    launchdeck_local_root_dir().join("bags-credentials.json")
}

pub fn bags_session_path() -> PathBuf {
    launchdeck_local_root_dir().join("bags-session.json")
}
