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
        restrict_dir_permissions(parent);
    }
    let tmp_path = temp_path_for(path);
    fs::write(&tmp_path, bytes).map_err(|error| error.to_string())?;
    restrict_file_permissions(&tmp_path);
    replace_with_temp(&tmp_path, path)?;
    restrict_file_permissions(path);
    Ok(())
}

pub fn replace_with_temp(tmp_path: &Path, path: &Path) -> Result<(), String> {
    fs::rename(tmp_path, path).map_err(|error| {
        let _ = fs::remove_file(tmp_path);
        error.to_string()
    })
}

#[cfg(unix)]
pub fn restrict_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        eprintln!(
            "[launchdeck][fs] failed to restrict file permissions on {}: {error}",
            path.display()
        );
    }
}

#[cfg(unix)]
pub fn restrict_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o700)) {
        eprintln!(
            "[launchdeck][fs] failed to restrict directory permissions on {}: {error}",
            path.display()
        );
    }
}

#[cfg(windows)]
pub fn restrict_file_permissions(path: &Path) {
    restrict_windows_path_permissions(path, false);
}

#[cfg(windows)]
pub fn restrict_dir_permissions(path: &Path) {
    restrict_windows_path_permissions(path, true);
}

#[cfg(windows)]
fn restrict_windows_path_permissions(path: &Path, is_dir: bool) {
    let Some(user) = current_windows_user() else {
        return;
    };
    let grant = if is_dir {
        format!("{user}:(OI)(CI)F")
    } else {
        format!("{user}:F")
    };
    match std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(grant)
        .output()
    {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "[launchdeck][fs] failed to restrict Windows ACLs on {}: {}",
                path.display(),
                stderr.trim()
            );
        }
        Err(error) => {
            eprintln!(
                "[launchdeck][fs] failed to invoke icacls for {}: {error}",
                path.display()
            );
        }
    }
}

#[cfg(windows)]
fn current_windows_user() -> Option<String> {
    let username = std::env::var("USERNAME").ok()?.trim().to_string();
    if username.is_empty() {
        return None;
    }
    let domain = std::env::var("USERDOMAIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Some(match domain {
        Some(domain) => format!("{domain}\\{username}"),
        None => username,
    })
}

#[cfg(not(any(unix, windows)))]
pub fn restrict_file_permissions(_path: &Path) {}

#[cfg(not(any(unix, windows)))]
pub fn restrict_dir_permissions(_path: &Path) {}

pub fn create_private_dir_all(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| error.to_string())?;
    restrict_dir_permissions(path);
    Ok(())
}

#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir =
            std::env::temp_dir().join(format!("launchdeck-fs-utils-test-{}", std::process::id()));
        let path = dir.join("state.json");

        atomic_write(&path, b"first").expect("initial write");
        atomic_write(&path, b"second").expect("replacement write");

        assert_eq!(fs::read_to_string(&path).expect("read state"), "second");
        let _ = fs::remove_dir_all(&dir);
    }
}
