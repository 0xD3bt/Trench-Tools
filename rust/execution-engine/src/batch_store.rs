use std::{collections::HashMap, fs, path::PathBuf};

use axum::http::StatusCode;

use crate::extension_api::BatchStatusResponse;

const DEFAULT_BATCH_HISTORY_FILE: &str = "batch-history.json";
const MAX_BATCH_HISTORY: usize = 500;

pub fn batch_history_path(data_root: &str) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(data_root)
        .join(DEFAULT_BATCH_HISTORY_FILE)
}

pub fn load_batch_history(path: &PathBuf) -> HashMap<String, BatchStatusResponse> {
    let Ok(contents) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(entries) = serde_json::from_str::<Vec<BatchStatusResponse>>(&contents) else {
        return HashMap::new();
    };
    entries
        .into_iter()
        .map(|batch| (batch.batch_id.clone(), batch))
        .collect()
}

pub fn persist_batch_history(
    path: &PathBuf,
    batches: &HashMap<String, BatchStatusResponse>,
) -> Result<(), (StatusCode, String)> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(internal_error)?;
    }
    let serialized =
        serde_json::to_string_pretty(&history_entries(batches)).map_err(internal_error)?;
    fs::write(path, serialized).map_err(internal_error)?;
    Ok(())
}

pub fn history_entries(batches: &HashMap<String, BatchStatusResponse>) -> Vec<BatchStatusResponse> {
    let mut entries = batches.values().cloned().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .updated_at_unix_ms
            .cmp(&left.updated_at_unix_ms)
            .then_with(|| right.created_at_unix_ms.cmp(&left.created_at_unix_ms))
    });
    if entries.len() > MAX_BATCH_HISTORY {
        entries.truncate(MAX_BATCH_HISTORY);
    }
    entries
}

fn internal_error(error: impl ToString) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
