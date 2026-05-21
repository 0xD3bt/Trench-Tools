use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Days, NaiveDate, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SOL_USD_DAILY_PRICE_FILE: &str = "sol-usd-daily-prices.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SolUsdDailyPrice {
    pub close: f64,
    pub source: String,
    pub fetched_at_unix_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct DailyPriceEnsureOutcome {
    pub requested_dates: Vec<NaiveDate>,
    pub fetched_dates: Vec<NaiveDate>,
    pub missing_dates: Vec<NaiveDate>,
}

pub fn sol_usd_daily_price_store_path(data_root: &str) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(data_root)
        .join(SOL_USD_DAILY_PRICE_FILE)
}

pub fn utc_date_from_unix_ms(unix_ms: u64) -> Option<NaiveDate> {
    let seconds = i64::try_from(unix_ms / 1_000).ok()?;
    let nanos = u32::try_from(unix_ms % 1_000)
        .ok()?
        .saturating_mul(1_000_000);
    DateTime::<Utc>::from_timestamp(seconds, nanos).map(|value| value.date_naive())
}

pub fn load_sol_usd_daily_price_store(path: &Path) -> BTreeMap<String, SolUsdDailyPrice> {
    let Ok(contents) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    serde_json::from_str::<BTreeMap<String, SolUsdDailyPrice>>(&contents).unwrap_or_default()
}

pub fn stored_prices_for_dates(
    path: &Path,
    dates: &BTreeSet<NaiveDate>,
) -> BTreeMap<NaiveDate, SolUsdDailyPrice> {
    let stored = load_sol_usd_daily_price_store(path);
    dates
        .iter()
        .filter_map(|date| {
            stored
                .get(&date.to_string())
                .filter(|entry| valid_price(entry.close))
                .cloned()
                .map(|entry| (*date, entry))
        })
        .collect()
}

pub async fn ensure_sol_usd_daily_prices_for_dates(
    data_root: &str,
    dates: BTreeSet<NaiveDate>,
) -> DailyPriceEnsureOutcome {
    let path = sol_usd_daily_price_store_path(data_root);
    let mut store = load_sol_usd_daily_price_store(&path);
    let requested_dates = dates.iter().copied().collect::<Vec<_>>();
    let missing_to_fetch = dates
        .iter()
        .filter(|date| {
            store
                .get(&date.to_string())
                .map(|entry| !valid_price(entry.close))
                .unwrap_or(true)
        })
        .copied()
        .collect::<Vec<_>>();

    let mut fetched_dates = Vec::new();
    if !missing_to_fetch.is_empty() {
        if let Ok(client) = Client::builder().timeout(Duration::from_secs(6)).build() {
            for date in missing_to_fetch {
                match fetch_sol_usd_daily_close(&client, date).await {
                    Ok(price) => {
                        store.insert(date.to_string(), price);
                        fetched_dates.push(date);
                    }
                    Err(error) => {
                        eprintln!(
                            "[execution-engine][sol-usd-daily] failed date={} err={}",
                            date, error
                        );
                    }
                }
            }
        }
    }

    if !fetched_dates.is_empty() {
        if let Err(error) = persist_sol_usd_daily_price_store(&path, &store) {
            eprintln!(
                "[execution-engine][sol-usd-daily] persist failed path={} err={}",
                path.display(),
                error
            );
        }
    }

    let missing_dates = requested_dates
        .iter()
        .filter(|date| {
            store
                .get(&date.to_string())
                .map(|entry| !valid_price(entry.close))
                .unwrap_or(true)
        })
        .copied()
        .collect::<Vec<_>>();

    DailyPriceEnsureOutcome {
        requested_dates,
        fetched_dates,
        missing_dates,
    }
}

pub async fn get_sol_usd_daily_close(date: NaiveDate) -> Result<f64, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|error| error.to_string())?;
    fetch_sol_usd_daily_close(&client, date)
        .await
        .map(|price| price.close)
}

async fn fetch_sol_usd_daily_close(
    client: &Client,
    date: NaiveDate,
) -> Result<SolUsdDailyPrice, String> {
    let providers = [
        DailyPriceProvider::Coinbase,
        DailyPriceProvider::Binance,
        DailyPriceProvider::CoinGecko,
    ];
    let mut last_error = String::new();
    for provider in providers {
        match fetch_from_provider(client, provider, date).await {
            Ok(close) if valid_price(close) => {
                return Ok(SolUsdDailyPrice {
                    close,
                    source: provider.source_name().to_string(),
                    fetched_at_unix_ms: now_unix_ms(),
                });
            }
            Ok(_) => {
                last_error = format!("{} returned invalid close", provider.source_name());
            }
            Err(error) => {
                last_error = error;
            }
        }
    }
    Err(last_error)
}

#[derive(Debug, Clone, Copy)]
enum DailyPriceProvider {
    Coinbase,
    Binance,
    CoinGecko,
}

impl DailyPriceProvider {
    fn source_name(self) -> &'static str {
        match self {
            Self::Coinbase => "coinbase",
            Self::Binance => "binance",
            Self::CoinGecko => "coingecko",
        }
    }
}

async fn fetch_from_provider(
    client: &Client,
    provider: DailyPriceProvider,
    date: NaiveDate,
) -> Result<f64, String> {
    let url = provider_url(provider, date)?;
    let payload = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("{} request error: {}", provider.source_name(), error))?
        .json::<Value>()
        .await
        .map_err(|error| format!("{} JSON error: {}", provider.source_name(), error))?;
    parse_provider_close(provider, &payload)
        .ok_or_else(|| format!("{} returned no daily close", provider.source_name()))
}

fn provider_url(provider: DailyPriceProvider, date: NaiveDate) -> Result<String, String> {
    let start = day_start_unix_seconds(date)?;
    let end = day_start_unix_seconds(next_day(date)?)?;
    match provider {
        DailyPriceProvider::Coinbase => Ok(format!(
            "https://api.exchange.coinbase.com/products/SOL-USD/candles?granularity=86400&start={}&end={}",
            iso_day_start(date)?,
            iso_day_start(next_day(date)?)?
        )),
        DailyPriceProvider::Binance => Ok(format!(
            "https://api.binance.com/api/v3/klines?symbol=SOLUSDT&interval=1d&startTime={}&endTime={}&limit=1",
            start.saturating_mul(1_000),
            end.saturating_mul(1_000)
        )),
        DailyPriceProvider::CoinGecko => Ok(format!(
            "https://api.coingecko.com/api/v3/coins/solana/market_chart/range?vs_currency=usd&from={start}&to={end}"
        )),
    }
}

fn persist_sol_usd_daily_price_store(
    path: &Path,
    store: &BTreeMap<String, SolUsdDailyPrice>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

pub fn parse_coinbase_daily_close(payload: &Value) -> Option<f64> {
    payload
        .as_array()?
        .iter()
        .filter_map(|row| row.as_array())
        .filter_map(|row| row.get(4).and_then(Value::as_f64))
        .find(|price| valid_price(*price))
}

pub fn parse_binance_daily_close(payload: &Value) -> Option<f64> {
    payload
        .as_array()?
        .first()?
        .as_array()?
        .get(4)?
        .as_str()?
        .parse::<f64>()
        .ok()
        .filter(|price| valid_price(*price))
}

pub fn parse_coingecko_daily_close(payload: &Value) -> Option<f64> {
    payload
        .get("prices")?
        .as_array()?
        .iter()
        .filter_map(|row| row.as_array())
        .filter_map(|row| row.get(1).and_then(Value::as_f64))
        .filter(|price| valid_price(*price))
        .last()
}

fn parse_provider_close(provider: DailyPriceProvider, payload: &Value) -> Option<f64> {
    match provider {
        DailyPriceProvider::Coinbase => parse_coinbase_daily_close(payload),
        DailyPriceProvider::Binance => parse_binance_daily_close(payload),
        DailyPriceProvider::CoinGecko => parse_coingecko_daily_close(payload),
    }
}

fn valid_price(price: f64) -> bool {
    price.is_finite() && price > 0.0
}

fn next_day(date: NaiveDate) -> Result<NaiveDate, String> {
    date.checked_add_days(Days::new(1))
        .ok_or_else(|| format!("date overflow for {date}"))
}

fn iso_day_start(date: NaiveDate) -> Result<String, String> {
    Ok(format!(
        "{}Z",
        date.and_hms_opt(0, 0, 0)
            .ok_or_else(|| format!("invalid date {date}"))?
            .format("%Y-%m-%dT%H:%M:%S")
    ))
}

fn day_start_unix_seconds(date: NaiveDate) -> Result<u64, String> {
    let timestamp = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| format!("invalid date {date}"))?
        .and_utc()
        .timestamp();
    u64::try_from(timestamp).map_err(|_| format!("date is before unix epoch: {date}"))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_date_from_unix_ms_uses_utc_boundary() {
        let date = utc_date_from_unix_ms(1_708_214_399_000).expect("date");
        assert_eq!(date.to_string(), "2024-02-17");
        let next = utc_date_from_unix_ms(1_708_214_400_000).expect("date");
        assert_eq!(next.to_string(), "2024-02-18");
    }

    #[test]
    fn parses_coinbase_daily_close() {
        let payload = serde_json::json!([[1779235200, 160.0, 170.0, 165.0, 168.42, 123.0]]);
        assert_eq!(parse_coinbase_daily_close(&payload), Some(168.42));
    }

    #[test]
    fn parses_binance_daily_close() {
        let payload = serde_json::json!([[1779235200000u64, "165.0", "170.0", "160.0", "168.42"]]);
        assert_eq!(parse_binance_daily_close(&payload), Some(168.42));
    }

    #[test]
    fn parses_coingecko_daily_close_as_last_price() {
        let payload = serde_json::json!({
            "prices": [[1779235200000u64, 167.0], [1779321599000u64, 168.42]]
        });
        assert_eq!(parse_coingecko_daily_close(&payload), Some(168.42));
    }

    #[test]
    fn stores_and_filters_requested_dates() {
        let root = std::env::temp_dir().join(format!("tt-sol-usd-test-{}", now_unix_ms()));
        fs::create_dir_all(&root).expect("create temp dir");
        let path = root.join(SOL_USD_DAILY_PRICE_FILE);
        let mut store = BTreeMap::new();
        store.insert(
            "2026-02-14".to_string(),
            SolUsdDailyPrice {
                close: 168.42,
                source: "test".to_string(),
                fetched_at_unix_ms: 1,
            },
        );
        persist_sol_usd_daily_price_store(&path, &store).expect("persist");
        let dates = BTreeSet::from([
            NaiveDate::from_ymd_opt(2026, 2, 14).unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
        ]);
        let stored = stored_prices_for_dates(&path, &dates);
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored
                .get(&NaiveDate::from_ymd_opt(2026, 2, 14).unwrap())
                .map(|entry| entry.close),
            Some(168.42)
        );
        let _ = fs::remove_dir_all(root);
    }
}
