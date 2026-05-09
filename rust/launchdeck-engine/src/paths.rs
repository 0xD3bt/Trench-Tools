#![allow(dead_code)]

use std::{env, path::PathBuf};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
fn test_reports_dir_override() -> &'static Mutex<Option<PathBuf>> {
    static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(crate) fn set_test_reports_dir(path: Option<PathBuf>) {
    let mut guard = test_reports_dir_override()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    *guard = path;
}

pub fn repo_root() -> PathBuf {
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

pub fn ui_dir() -> PathBuf {
    repo_root().join("ui")
}

pub fn local_root_dir() -> PathBuf {
    if let Ok(explicit) = env::var("LAUNCHDECK_LOCAL_DATA_DIR") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    repo_root().join(".local").join("launchdeck")
}

pub fn uploads_dir() -> PathBuf {
    local_root_dir().join("uploads")
}

pub fn reports_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Some(path) = test_reports_dir_override()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
        {
            return path;
        }
    }
    if let Ok(explicit) = env::var("LAUNCHDECK_SEND_LOG_DIR") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    local_root_dir().join("send-reports")
}

pub fn image_library_path() -> PathBuf {
    local_root_dir().join("image-library.json")
}

pub fn app_config_path() -> PathBuf {
    local_root_dir().join("app-config.json")
}

pub fn bags_credentials_path() -> PathBuf {
    local_root_dir().join("bags-credentials.json")
}

pub fn bags_session_path() -> PathBuf {
    local_root_dir().join("bags-session.json")
}

pub fn vanity_dir() -> PathBuf {
    local_root_dir().join("vanity")
}

pub fn vanity_queue_path(launchpad: &str) -> PathBuf {
    vanity_dir().join(format!("{}.txt", launchpad.trim().to_ascii_lowercase()))
}

pub fn vanity_used_state_path(launchpad: &str) -> PathBuf {
    vanity_dir().join(format!(
        "{}.used.jsonl",
        launchpad.trim().to_ascii_lowercase()
    ))
}

pub fn lookup_table_cache_path() -> PathBuf {
    shared_lookup_table_cache_path()
}

pub fn shared_lookup_table_cache_path() -> PathBuf {
    local_root_dir().join("shared-lookup-tables.json")
}

pub fn shared_fee_market_cache_path() -> PathBuf {
    local_root_dir().join("shared-fee-market.json")
}

pub fn legacy_bonk_lookup_table_cache_path() -> PathBuf {
    local_root_dir().join("bonk-lookup-tables.json")
}

pub fn bonk_lookup_table_cache_path() -> PathBuf {
    shared_lookup_table_cache_path()
}

pub fn runtime_state_path() -> PathBuf {
    if let Ok(explicit) = env::var("LAUNCHDECK_ENGINE_RUNTIME_PATH") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    repo_root().join(".local").join("engine-runtime.json")
}

pub fn follow_daemon_state_path() -> PathBuf {
    if let Ok(explicit) = env::var("LAUNCHDECK_FOLLOW_DAEMON_STATE_PATH") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    local_root_dir().join("follow-daemon-state.json")
}
