fn normalized_env_value(key: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn trading_resource_mode() -> String {
    normalized_env_value("TRADING_RESOURCE_MODE")
}

pub fn always_on_resource_mode_enabled() -> bool {
    matches!(
        trading_resource_mode().as_str(),
        "always-on" | "always_on" | "unlimited"
    )
}

pub fn idle_suspension_enabled() -> bool {
    !always_on_resource_mode_enabled()
}
