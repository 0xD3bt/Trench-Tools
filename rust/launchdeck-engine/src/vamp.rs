#![allow(non_snake_case, dead_code)]

use crate::image_library::{SerializedImageRecord, save_image_bytes};
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportedTokenData {
    pub name: String,
    pub symbol: String,
    pub description: String,
    pub website: String,
    pub twitter: String,
    pub telegram: String,
    pub imageUrl: String,
    pub metadataUri: String,
    pub source: String,
}

fn normalize_http_url(raw_value: &str) -> String {
    let raw = raw_value.trim();
    if raw.is_empty() {
        return String::new();
    }
    let with_protocol = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{}", raw.trim_start_matches("//"))
    };
    reqwest::Url::parse(&with_protocol)
        .ok()
        .filter(|url| url.scheme() == "http" || url.scheme() == "https")
        .map(|url| url.to_string())
        .unwrap_or_default()
}

fn normalize_social_url(raw_value: &str, kind: &str) -> String {
    let raw = raw_value.trim();
    if raw.is_empty() {
        return String::new();
    }
    if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("//") {
        return normalize_http_url(raw);
    }
    let normalized = raw.trim_start_matches('@').trim_start_matches('/').trim();
    if normalized.is_empty() {
        return String::new();
    }
    match kind {
        "twitter" => format!("https://x.com/{normalized}"),
        "telegram" => format!("https://t.me/{normalized}"),
        _ => normalize_http_url(raw),
    }
}

fn merge_imported(base: ImportedTokenData, overlay: ImportedTokenData) -> ImportedTokenData {
    ImportedTokenData {
        name: if !base.name.is_empty() { base.name } else { overlay.name },
        symbol: if !base.symbol.is_empty() { base.symbol } else { overlay.symbol },
        description: if !base.description.is_empty() { base.description } else { overlay.description },
        website: if !base.website.is_empty() { base.website } else { overlay.website },
        twitter: if !base.twitter.is_empty() { base.twitter } else { overlay.twitter },
        telegram: if !base.telegram.is_empty() { base.telegram } else { overlay.telegram },
        imageUrl: if !base.imageUrl.is_empty() { base.imageUrl } else { overlay.imageUrl },
        metadataUri: if !base.metadataUri.is_empty() { base.metadataUri } else { overlay.metadataUri },
        source: if !base.source.is_empty() { base.source } else { overlay.source },
    }
}

fn normalize_imported_metadata_payload(payload: &Value, source: &str) -> ImportedTokenData {
    let websites = payload
        .get("websites")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let socials = payload
        .get("socials")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let first_website = websites
        .iter()
        .find_map(|entry| entry.get("url").and_then(Value::as_str))
        .unwrap_or_default();
    let twitter_social = socials
        .iter()
        .find(|entry| {
            entry.get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .eq_ignore_ascii_case("twitter")
        })
        .and_then(|entry| entry.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let telegram_social = socials
        .iter()
        .find(|entry| {
            entry.get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .eq_ignore_ascii_case("telegram")
        })
        .and_then(|entry| entry.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    ImportedTokenData {
        name: payload
            .get("name")
            .or_else(|| payload.get("token_name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        symbol: payload
            .get("symbol")
            .or_else(|| payload.get("ticker"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        description: payload
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        website: normalize_http_url(
            payload
                .get("website")
                .or_else(|| payload.get("external_url"))
                .and_then(Value::as_str)
                .unwrap_or(first_website),
        ),
        twitter: normalize_social_url(
            payload
                .get("twitter")
                .and_then(Value::as_str)
                .unwrap_or(twitter_social),
            "twitter",
        ),
        telegram: normalize_social_url(
            payload
                .get("telegram")
                .and_then(Value::as_str)
                .unwrap_or(telegram_social),
            "telegram",
        ),
        imageUrl: normalize_http_url(
            payload
                .get("image_uri")
                .or_else(|| payload.get("image"))
                .or_else(|| payload.get("imageUrl"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        metadataUri: normalize_http_url(
            payload
                .get("metadataUri")
                .or_else(|| payload.get("metadata_uri"))
                .or_else(|| payload.get("uri"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        source: source.to_string(),
    }
}

async fn fetch_json_or_null(client: &Client, url: &str) -> Option<Value> {
    let response = client.get(url).header("accept", "application/json").send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<Value>().await.ok()
}

pub async fn fetch_imported_token_metadata(contract_address: &str) -> Result<ImportedTokenData, String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|error| error.to_string())?;
    let mut imported = ImportedTokenData::default();
    if let Some(pump_payload) = fetch_json_or_null(
        &client,
        &format!("https://frontend-api-v3.pump.fun/coins/{contract_address}"),
    )
    .await
    {
        imported = merge_imported(
            imported,
            normalize_imported_metadata_payload(&pump_payload, "pump.fun"),
        );
        if !imported.metadataUri.is_empty() {
            if let Some(metadata_payload) = fetch_json_or_null(&client, &imported.metadataUri).await {
                imported = merge_imported(
                    imported,
                    normalize_imported_metadata_payload(&metadata_payload, "metadata"),
                );
            }
        }
    }
    if let Some(dex_payload) = fetch_json_or_null(
        &client,
        &format!("https://api.dexscreener.com/latest/dex/tokens/{contract_address}"),
    )
    .await
    {
        if let Some(dex_pair) = dex_payload
            .get("pairs")
            .and_then(Value::as_array)
            .and_then(|entries| entries.iter().find(|entry| !entry.is_null()))
        {
            imported = merge_imported(
                imported,
                normalize_imported_metadata_payload(
                    &serde_json::json!({
                        "name": dex_pair.get("baseToken").and_then(|value| value.get("name")).and_then(Value::as_str).unwrap_or_default(),
                        "symbol": dex_pair.get("baseToken").and_then(|value| value.get("symbol")).and_then(Value::as_str).unwrap_or_default(),
                        "imageUrl": dex_pair.get("info").and_then(|value| value.get("imageUrl")).and_then(Value::as_str).unwrap_or_default(),
                        "websites": dex_pair.get("info").and_then(|value| value.get("websites")).cloned().unwrap_or(Value::Array(vec![])),
                        "socials": dex_pair.get("info").and_then(|value| value.get("socials")).cloned().unwrap_or(Value::Array(vec![])),
                    }),
                    "DexScreener",
                ),
            );
        }
    }
    if imported.name.is_empty()
        && imported.symbol.is_empty()
        && imported.imageUrl.is_empty()
        && imported.website.is_empty()
        && imported.twitter.is_empty()
        && imported.telegram.is_empty()
    {
        return Err("No token metadata was found for that contract address.".to_string());
    }
    Ok(imported)
}

pub async fn import_remote_image_to_library(
    image_url: &str,
    original_name: &str,
    record_name: &str,
) -> Result<Option<SerializedImageRecord>, String> {
    let safe_url = normalize_http_url(image_url);
    if safe_url.is_empty() {
        return Ok(None);
    }
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(&safe_url)
        .send()
        .await
        .map_err(|_| "Unable to download token image.".to_string())?;
    if !response.status().is_success() {
        return Err("Unable to download token image.".to_string());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let mut extension = match content_type.as_str() {
        "image/png" => Some(".png"),
        "image/jpeg" => Some(".jpg"),
        "image/webp" => Some(".webp"),
        "image/gif" => Some(".gif"),
        _ => None,
    };
    if extension.is_none() {
        extension = Path::new(reqwest::Url::parse(&safe_url).ok().and_then(|url| url.path_segments().and_then(|mut segments| segments.next_back()).map(|v| v.to_string())).unwrap_or_default().as_str())
            .extension()
            .and_then(|value| value.to_str())
            .map(|ext| match ext.to_ascii_lowercase().as_str() {
                "png" => ".png",
                "jpg" | "jpeg" => ".jpg",
                "webp" => ".webp",
                "gif" => ".gif",
                _ => "",
            })
            .filter(|value| !value.is_empty());
    }
    let Some(extension) = extension else {
        return Err("Imported token image format is not supported.".to_string());
    };
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "Imported token image was empty.".to_string())?;
    if bytes.is_empty() {
        return Err("Imported token image was empty.".to_string());
    }
    if bytes.len() > 8_000_000 {
        return Err("Imported token image is too large.".to_string());
    }
    save_image_bytes(
        &bytes,
        extension,
        original_name,
        if record_name.trim().is_empty() { None } else { Some(record_name) },
    )
    .map(Some)
}
