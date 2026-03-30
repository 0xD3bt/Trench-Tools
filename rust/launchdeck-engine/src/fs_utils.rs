use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn temp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("launchdeck-tmp");
    path.with_file_name(format!(".{}.{}.tmp", file_name, timestamp_ms()))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let tmp_path = temp_path_for(path);
    fs::write(&tmp_path, bytes).map_err(|error| error.to_string())?;
    fs::rename(&tmp_path, path).map_err(|error| {
        let _ = fs::remove_file(&tmp_path);
        error.to_string()
    })
}

pub fn quarantine_corrupt_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let corrupt_path = path.with_extension(format!(
        "{}.corrupt-{}",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("json"),
        timestamp_ms()
    ));
    fs::rename(path, &corrupt_path)
        .map_err(|error| format!("Failed to quarantine corrupt {label}: {error}"))?;
    Ok(corrupt_path)
}
