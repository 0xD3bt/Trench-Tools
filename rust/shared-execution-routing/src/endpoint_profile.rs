use std::collections::HashSet;

const AGGREGATES: &[&str] = &["global", "us", "eu", "asia"];

pub fn metro_token_canonical(token: &str) -> Option<&'static str> {
    match token.trim().to_lowercase().as_str() {
        "slc" => Some("slc"),
        "ewr" | "ny" => Some("ewr"),
        "lon" => Some("lon"),
        "fra" => Some("fra"),
        "ams" => Some("ams"),
        "sg" => Some("sg"),
        "tyo" => Some("tyo"),
        _ => None,
    }
}

fn is_aggregate(value: &str) -> bool {
    AGGREGATES.iter().any(|entry| *entry == value)
}

fn canonicalize_comma_metro_list(value: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for part in value.split(',') {
        let part = part.trim().to_lowercase();
        if part.is_empty() {
            continue;
        }
        if is_aggregate(&part) {
            return Err(
                "comma-separated profiles must list metro codes only (slc, ewr, lon, fra, ams, sg, tyo), not region groups"
                    .to_string(),
            );
        }
        let metro =
            metro_token_canonical(&part).ok_or_else(|| format!("unknown metro token '{part}'"))?;
        if seen.insert(metro) {
            out.push(metro.to_string());
        }
    }
    if out.is_empty() {
        return Err("empty metro list".to_string());
    }
    Ok(out.join(","))
}

pub fn parse_config_endpoint_profile(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty() {
        return Err("endpoint profile is empty".to_string());
    }
    if normalized == "west" {
        return Err(
            "'west' is no longer supported. Use global, us, eu, asia, or metros: slc, ewr, lon, fra, ams, sg, tyo"
                .to_string(),
        );
    }
    if normalized.contains(',') {
        return canonicalize_comma_metro_list(&normalized);
    }
    if is_aggregate(&normalized) {
        return Ok(normalized);
    }
    metro_token_canonical(&normalized)
        .map(|metro| metro.to_string())
        .ok_or_else(|| {
            format!(
                "unknown endpoint profile '{normalized}'. Use global, us, eu, asia, or metros: slc, ewr, lon, fra, ams, sg, tyo"
            )
        })
}

pub fn normalize_user_region(value: &str) -> Option<String> {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty() || normalized == "west" {
        return None;
    }
    if normalized.contains(',') {
        return canonicalize_comma_metro_list(&normalized).ok();
    }
    if is_aggregate(&normalized) {
        return Some(normalized);
    }
    metro_token_canonical(&normalized).map(|metro| metro.to_string())
}
