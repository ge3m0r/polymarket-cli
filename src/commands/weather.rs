use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, ensure};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use clap::{Args, Subcommand};
use polymarket_client_sdk_v2::gamma::{
    self,
    types::{request::SearchRequest, response::Market},
};
use rust_decimal::prelude::ToPrimitive;
use serde_json::Value;

use crate::output::OutputFormat;
use crate::output::weather::{
    FeeSchedule, HedgeSummary, TokyoBacktestReport, TokyoBacktestRow, TokyoPaperPosition,
    TokyoPaperSettlementReport, TokyoSignalReport, TokyoWeatherReport, WeatherMarketRow,
    print_tokyo_backtest, print_tokyo_paper_settlement, print_tokyo_signal, print_tokyo_weather,
};

const TOKYO_LATITUDE: f64 = 35.553;
const TOKYO_LONGITUDE: f64 = 139.781;
const STATION: &str = "RJTT (Tokyo Haneda Airport)";
const RESOLUTION_URL: &str = "https://www.weather.gov/wrh/timeseries?site=rjtt";
const FORECAST_URL: &str = "https://ensemble-api.open-meteo.com/v1/ensemble";
const PREVIOUS_RUNS_URL: &str = "https://previous-runs-api.open-meteo.com/v1/forecast";
const OBSERVATION_URL: &str = "https://aviationweather.gov/api/data/metar";
const GAMMA_MARKETS_URL: &str = "https://gamma-api.polymarket.com/markets";
const GAMMA_EVENTS_KEYSET_URL: &str = "https://gamma-api.polymarket.com/events/keyset";
const CLOB_PRICES_URL: &str = "https://clob.polymarket.com/prices";
const TOKYO_DAILY_WEATHER_SERIES_ID: &str = "10740";
const HISTORICAL_WEATHER_URL: &str = "https://archive-api.open-meteo.com/v1/archive";
const PAPER_ENTRY_HOUR_UTC: u32 = 5;
const PAPER_ENTRY_MINUTE_UTC: u32 = 17;

#[derive(Args)]
pub struct WeatherArgs {
    #[command(subcommand)]
    pub command: WeatherCommand,
}

#[derive(Subcommand)]
pub enum WeatherCommand {
    /// Compare Tokyo daily-high markets with an ensemble forecast (read-only)
    Tokyo {
        /// Tokyo-local forecast date (YYYY-MM-DD); defaults to tomorrow
        #[arg(long)]
        date: Option<NaiveDate>,

        /// Open-Meteo ensemble model identifier
        #[arg(long, default_value = "gfs_seamless")]
        model: String,

        /// Additive station-bias correction in degrees Celsius
        #[arg(long, default_value = "0")]
        bias_c: f64,
    },

    /// Generate a fee/slippage-aware Tokyo paper-trading signal (read-only)
    SignalTokyo {
        /// Tokyo-local forecast date (YYYY-MM-DD); defaults to tomorrow
        #[arg(long)]
        date: Option<NaiveDate>,

        /// Open-Meteo ensemble model identifier
        #[arg(long, default_value = "gfs_seamless")]
        model: String,

        /// Capital weight for the legacy lower-leaning band
        #[arg(long, default_value_t = 0.75)]
        legacy_weight: f64,

        /// Total shares paid out when both strategy legs cover the winner
        #[arg(long, default_value_t = 5.0)]
        size: f64,

        /// Extra assumed execution cost per share
        #[arg(long, default_value_t = 0.01)]
        slippage: f64,

        /// Minimum conservative expected P&L required for a paper BUY
        #[arg(long, default_value_t = 1.0)]
        min_expected_pnl: f64,

        /// Maximum delay after the scheduled quote time accepted into the forward ledger
        #[arg(long, default_value_t = 15)]
        max_entry_delay_minutes: u16,
    },

    /// Settle a saved Tokyo paper signal using the resolved Polymarket winner
    SettleTokyoPaper {
        /// JSON signal snapshot created at the simulated buy point
        #[arg(long)]
        signal: PathBuf,
    },

    /// Backtest Tokyo resolved markets against prior GFS forecasts (read-only)
    BacktestTokyo {
        /// First market date to include (YYYY-MM-DD)
        #[arg(long)]
        since: Option<NaiveDate>,

        /// Last market date to include (YYYY-MM-DD)
        #[arg(long)]
        until: Option<NaiveDate>,

        /// Use only the most recent N resolved events when no date range is given
        #[arg(long, default_value_t = 14)]
        recent: usize,

        /// Forecast lead time in whole days (1-7)
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=7))]
        lead_days: u8,

        /// Open-Meteo archived ensemble-mean model identifier
        #[arg(long, default_value = "ncep_gefs_ensemble_mean_seamless")]
        model: String,

        /// Simulated number of shares bought in each of the four buckets
        #[arg(long, default_value_t = 5.0)]
        size: f64,

        /// Simulated entry hour on the preceding day (UTC)
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u8).range(0..=23))]
        entry_hour_utc: u8,

        /// Extra cost per share for the conservative execution scenario
        #[arg(long, default_value_t = 0.01)]
        slippage: f64,

        /// Capital weight for the legacy lower-leaning band (0=current, 0.5=50/50 blend)
        #[arg(long, default_value_t = 0.0, value_parser = clap::value_parser!(f64))]
        legacy_weight: f64,
    },
}

#[derive(Clone, Copy, Debug)]
enum BucketKind {
    AtOrBelow,
    Exact,
    AtOrAbove,
}

#[derive(Clone, Copy, Debug)]
struct Bucket {
    threshold: i32,
    kind: BucketKind,
}

impl Bucket {
    fn contains(self, rounded_temperature: i32) -> bool {
        match self.kind {
            BucketKind::AtOrBelow => rounded_temperature <= self.threshold,
            BucketKind::Exact => rounded_temperature == self.threshold,
            BucketKind::AtOrAbove => rounded_temperature >= self.threshold,
        }
    }
}

fn default_tokyo_date() -> Result<NaiveDate> {
    let tokyo_now = Utc::now() + Duration::hours(9);
    tokyo_now
        .date_naive()
        .succ_opt()
        .context("Could not calculate tomorrow's Tokyo date")
}

fn parse_bucket(question: &str) -> Option<Bucket> {
    let after_be = question.split(" be ").nth(1)?;
    let temperature: i32 = after_be.split("°C").next()?.trim().parse().ok()?;
    let kind = if after_be.contains("or below") {
        BucketKind::AtOrBelow
    } else if after_be.contains("or higher") {
        BucketKind::AtOrAbove
    } else {
        BucketKind::Exact
    };
    Some(Bucket {
        threshold: temperature,
        kind,
    })
}

fn bucket_label(question: &str) -> String {
    question
        .split(" be ")
        .nth(1)
        .and_then(|s| s.split(" on ").next())
        .unwrap_or(question)
        .to_string()
}

#[derive(Debug)]
struct ResolvedTokyoEvent {
    date: NaiveDate,
    markets: Vec<Market>,
    winner_index: usize,
}

async fn fetch_resolved_events(
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    recent: usize,
) -> Result<Vec<ResolvedTokyoEvent>> {
    ensure!(recent > 0, "--recent must be greater than zero");
    let http = reqwest::Client::new();
    let mut by_date = BTreeMap::<NaiveDate, Vec<Market>>::new();
    let mut after_cursor: Option<String> = None;

    loop {
        let mut request = http.get(GAMMA_EVENTS_KEYSET_URL).query(&[
            ("series_id", TOKYO_DAILY_WEATHER_SERIES_ID),
            ("limit", "100"),
            ("closed", "true"),
        ]);
        if let Some(cursor) = after_cursor.as_deref() {
            request = request.query(&[("after_cursor", cursor)]);
        }
        let response = request
            .send()
            .await
            .context("Failed to request Tokyo series events")?
            .error_for_status()
            .context("Gamma keyset events request failed")?
            .json::<Value>()
            .await
            .context("Failed to parse Tokyo series events")?;
        let page = response
            .get("events")
            .and_then(Value::as_array)
            .context("Gamma keyset response is missing events")?;

        for event in page {
            let Some(date_text) = event
                .get("endDate")
                .and_then(Value::as_str)
                .and_then(|value| value.get(..10))
            else {
                continue;
            };
            let Ok(date) = NaiveDate::parse_from_str(date_text, "%Y-%m-%d") else {
                continue;
            };
            if since.is_some_and(|start| date < start)
                || until.is_some_and(|end| date > end)
                || !event
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| title.starts_with("Highest temperature in Tokyo"))
            {
                continue;
            }
            let markets = event
                .get("markets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|market| serde_json::from_value::<Market>(market.clone()).ok())
                .collect::<Vec<_>>();
            if by_date
                .get(&date)
                .is_none_or(|existing| markets.len() > existing.len())
            {
                by_date.insert(date, markets);
            }
        }

        after_cursor = response
            .get("next_cursor")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        if after_cursor.is_none() {
            break;
        }
    }

    let mut events = Vec::new();
    for (date, mut markets) in by_date {
        markets.sort_by_key(|market| {
            market
                .question
                .as_deref()
                .and_then(parse_bucket)
                .map_or(i32::MAX, |bucket| bucket.threshold)
        });
        let Some(winner_index) = markets.iter().position(|market| {
            market
                .outcome_prices
                .as_deref()
                .and_then(|prices| prices.first())
                .and_then(ToPrimitive::to_f64)
                .is_some_and(|yes| yes >= 0.99)
        }) else {
            continue;
        };
        if markets.len() >= 4 {
            events.push(ResolvedTokyoEvent {
                date,
                markets,
                winner_index,
            });
        }
    }
    if since.is_none() && until.is_none() && events.len() > recent {
        events.drain(..events.len() - recent);
    }
    Ok(events)
}

#[derive(Debug)]
struct HistoricalDistribution {
    central_max_c: f64,
    member_maxima_c: Vec<f64>,
}

fn inverse_normal_cdf(probability: f64) -> f64 {
    // Peter J. Acklam's rational approximation. The input is always strictly (0, 1).
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const LOW: f64 = 0.02425;
    const HIGH: f64 = 1.0 - LOW;

    if probability < LOW {
        let q = (-2.0 * probability.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if probability <= HIGH {
        let q = probability - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - probability).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

fn reconstruct_member_maxima(hours: &[(f64, f64)], member_count: usize) -> Vec<f64> {
    (0..member_count)
        .filter_map(|index| {
            let probability = (index as f64 + 0.5) / member_count as f64;
            let z = inverse_normal_cdf(probability);
            hours
                .iter()
                .map(|(mean, spread)| mean + z * spread.max(0.0))
                .reduce(f64::max)
        })
        .collect()
}

async fn fetch_previous_run_distributions(
    since: NaiveDate,
    until: NaiveDate,
    lead_days: u8,
    model: &str,
) -> Result<BTreeMap<NaiveDate, HistoricalDistribution>> {
    let mean_variable = format!("temperature_2m_previous_day{lead_days}");
    let spread_variable = format!("temperature_2m_spread_previous_day{lead_days}");
    let hourly_variables = format!("{mean_variable},{spread_variable}");
    let response = reqwest::Client::new()
        .get(PREVIOUS_RUNS_URL)
        .query(&[
            ("latitude", TOKYO_LATITUDE.to_string()),
            ("longitude", TOKYO_LONGITUDE.to_string()),
            ("hourly", hourly_variables),
            ("models", model.to_string()),
            ("start_date", since.to_string()),
            ("end_date", until.to_string()),
            ("timezone", "Asia/Tokyo".to_string()),
        ])
        .send()
        .await
        .context("Failed to request historical GFS forecasts")?
        .error_for_status()
        .context("Open-Meteo previous-runs request failed")?
        .json::<Value>()
        .await
        .context("Failed to parse historical GFS forecasts")?;
    let hourly = response
        .get("hourly")
        .and_then(Value::as_object)
        .context("Previous-runs response is missing hourly data")?;
    let times = hourly
        .get("time")
        .and_then(Value::as_array)
        .context("Previous-runs response is missing timestamps")?;
    let temperatures = hourly
        .get(&mean_variable)
        .and_then(Value::as_array)
        .context("Previous-runs response is missing temperature data")?;
    let spreads = hourly
        .get(&spread_variable)
        .and_then(Value::as_array)
        .context("Previous-runs response is missing ensemble spread data")?;
    ensure!(
        times.len() == temperatures.len() && times.len() == spreads.len(),
        "Historical forecast timestamps, means, and spreads have different lengths"
    );

    let mut hourly_by_date = BTreeMap::<NaiveDate, Vec<(f64, f64)>>::new();
    for ((time, temperature), spread) in times.iter().zip(temperatures).zip(spreads) {
        let (Some(time), Some(temperature), Some(spread)) =
            (time.as_str(), temperature.as_f64(), spread.as_f64())
        else {
            continue;
        };
        let Some(date_text) = time.get(..10) else {
            continue;
        };
        let Ok(date) = NaiveDate::parse_from_str(date_text, "%Y-%m-%d") else {
            continue;
        };
        hourly_by_date
            .entry(date)
            .or_default()
            .push((temperature, spread));
    }
    Ok(hourly_by_date
        .into_iter()
        .filter_map(|(date, hours)| {
            let central_max_c = hours.iter().map(|(mean, _)| *mean).reduce(f64::max)?;
            let member_maxima_c = reconstruct_member_maxima(&hours, 31);
            Some((
                date,
                HistoricalDistribution {
                    central_max_c,
                    member_maxima_c,
                },
            ))
        })
        .collect())
}

fn predicted_market_index(event: &ResolvedTokyoEvent, forecast_max_c: f64) -> Option<usize> {
    let temperature = forecast_max_c.round() as i32;
    event.markets.iter().position(|market| {
        market
            .question
            .as_deref()
            .and_then(parse_bucket)
            .is_some_and(|bucket| bucket.contains(temperature))
    })
}

async fn fetch_entry_prices(
    client: &reqwest::Client,
    tokens: &[String],
    entry_timestamp: i64,
) -> Result<Option<Vec<f64>>> {
    let response = client
        .post("https://clob.polymarket.com/batch-prices-history")
        .json(&serde_json::json!({
            "markets": tokens,
            "start_ts": entry_timestamp - 7_200,
            "end_ts": entry_timestamp + 7_200,
            "interval": "max",
            "fidelity": 5,
        }))
        .send()
        .await
        .context("Failed to request historical CLOB prices")?
        .error_for_status()
        .context("CLOB batch price-history request failed")?
        .json::<Value>()
        .await
        .context("Failed to parse historical CLOB prices")?;
    let history = response
        .get("history")
        .and_then(Value::as_object)
        .context("CLOB response is missing price history")?;
    let mut prices = Vec::with_capacity(tokens.len());
    for token in tokens {
        let Some(points) = history.get(token).and_then(Value::as_array) else {
            return Ok(None);
        };
        let Some(point) = points.iter().min_by_key(|point| {
            point
                .get("t")
                .and_then(Value::as_i64)
                .map_or(u64::MAX, |timestamp| timestamp.abs_diff(entry_timestamp))
        }) else {
            return Ok(None);
        };
        let Some(price) = point.get("p").and_then(Value::as_f64) else {
            return Ok(None);
        };
        prices.push(price);
    }
    Ok(Some(prices))
}

async fn fetch_live_best_asks(markets: &[Market]) -> Result<Vec<f64>> {
    let tokens = markets
        .iter()
        .map(|market| {
            market
                .clob_token_ids
                .as_deref()
                .and_then(|ids| ids.first())
                .map(ToString::to_string)
                .context("Tokyo market is missing its YES token ID")
        })
        .collect::<Result<Vec<_>>>()?;
    let request = tokens
        .iter()
        .map(|token| serde_json::json!({"token_id": token, "side": "SELL"}))
        .collect::<Vec<_>>();
    let response = reqwest::Client::new()
        .post(CLOB_PRICES_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to request live CLOB best asks")?
        .error_for_status()
        .context("CLOB best-ask request failed")?
        .json::<Value>()
        .await
        .context("Failed to parse live CLOB best asks")?;

    tokens
        .iter()
        .map(|token| {
            let value = response
                .get(token)
                .and_then(|sides| sides.get("SELL"))
                .context("CLOB response is missing a YES best ask")?;
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|price| price.parse().ok()))
                .context("CLOB returned an invalid YES best ask")
        })
        .collect()
}

fn build_backtest_row(event: &ResolvedTokyoEvent, forecast_max_c: f64) -> Option<TokyoBacktestRow> {
    let predicted_index = predicted_market_index(event, forecast_max_c)?;
    Some(TokyoBacktestRow {
        date: event.date,
        forecast_max_c,
        reconstructed_member_max_min_c: None,
        reconstructed_member_max_mean_c: None,
        reconstructed_member_max_max_c: None,
        predicted_bucket: event.markets[predicted_index]
            .question
            .as_deref()
            .map(bucket_label)?,
        winning_bucket: event.markets[event.winner_index]
            .question
            .as_deref()
            .map(bucket_label)?,
        exact_hit: predicted_index == event.winner_index,
        selected_band: Vec::new(),
        legacy_band: Vec::new(),
        modeled_coverage: None,
        expected_pnl_after_fee: None,
        conservative_expected_pnl: None,
        traded: false,
        selected_band_hit: false,
        entry_time_utc: None,
        entry_prices: Vec::new(),
        position_size_per_bucket: 0.0,
        entry_cost: None,
        taker_fee: None,
        total_cost: None,
        payout: None,
        pnl: None,
        conservative_total_cost: None,
        conservative_pnl: None,
    })
}

#[derive(Debug)]
struct BandChoice {
    range: std::ops::Range<usize>,
    coverage: f64,
    entry_prices: Vec<f64>,
    entry_cost: f64,
    taker_fee: f64,
    total_cost: f64,
    expected_pnl: f64,
    conservative_total_cost: f64,
    conservative_expected_pnl: f64,
}

fn scheduled_paper_entry_time(target_date: NaiveDate) -> Result<DateTime<Utc>> {
    let entry_date = target_date
        .pred_opt()
        .context("Could not calculate the paper entry date")?;
    let entry = entry_date
        .and_hms_opt(PAPER_ENTRY_HOUR_UTC, PAPER_ENTRY_MINUTE_UTC, 0)
        .context("Could not calculate the paper entry time")?;
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(entry, Utc))
}

fn paper_entry_timing(
    quote_time: DateTime<Utc>,
    scheduled_time: DateTime<Utc>,
    maximum_delay_minutes: u16,
) -> (&'static str, i64) {
    let delay_seconds = (quote_time - scheduled_time).num_seconds();
    let timing = if delay_seconds < 0 {
        "EARLY"
    } else if delay_seconds <= i64::from(maximum_delay_minutes) * 60 {
        "ON_TIME"
    } else {
        "LATE"
    };
    (timing, delay_seconds)
}

fn build_paper_positions(
    markets: &[Market],
    choice: &BandChoice,
    leg: &str,
    shares: f64,
    slippage: f64,
) -> Vec<TokyoPaperPosition> {
    markets[choice.range.clone()]
        .iter()
        .zip(&choice.entry_prices)
        .filter_map(|(market, best_ask)| {
            let bucket = market.question.as_deref().map(bucket_label)?;
            let taker_fee_per_share = 0.05 * best_ask * (1.0 - best_ask);
            let conservative_fill_price = (best_ask + slippage).min(0.999);
            let conservative_fee_per_share =
                0.05 * conservative_fill_price * (1.0 - conservative_fill_price);
            Some(TokyoPaperPosition {
                leg: leg.to_string(),
                bucket,
                yes_token: market
                    .clob_token_ids
                    .as_deref()
                    .and_then(|ids| ids.first())
                    .map(ToString::to_string),
                market_slug: market.slug.clone(),
                shares,
                best_ask: *best_ask,
                taker_fee_per_share,
                cost_after_fee: shares * (best_ask + taker_fee_per_share),
                conservative_fill_price,
                conservative_fee_per_share,
                conservative_cost: shares * (conservative_fill_price + conservative_fee_per_share),
            })
        })
        .collect()
}

fn choose_four_bucket_band(
    event: &ResolvedTokyoEvent,
    member_maxima_c: &[f64],
    prices: &[f64],
    size: f64,
    slippage: f64,
) -> Option<BandChoice> {
    choose_four_bucket_band_for_markets(&event.markets, member_maxima_c, prices, size, slippage)
}

fn choose_four_bucket_band_for_markets(
    markets: &[Market],
    member_maxima_c: &[f64],
    prices: &[f64],
    size: f64,
    slippage: f64,
) -> Option<BandChoice> {
    if markets.len() < 4 || markets.len() != prices.len() || member_maxima_c.is_empty() {
        return None;
    }

    (0..=markets.len() - 4)
        .filter_map(|start| {
            evaluate_four_bucket_band_for_markets(
                markets,
                member_maxima_c,
                prices,
                start..start + 4,
                size,
                slippage,
            )
        })
        .max_by(|left, right| {
            left.conservative_expected_pnl
                .total_cmp(&right.conservative_expected_pnl)
                .then_with(|| left.coverage.total_cmp(&right.coverage))
        })
}

fn evaluate_four_bucket_band(
    event: &ResolvedTokyoEvent,
    member_maxima_c: &[f64],
    prices: &[f64],
    range: std::ops::Range<usize>,
    size: f64,
    slippage: f64,
) -> Option<BandChoice> {
    evaluate_four_bucket_band_for_markets(
        &event.markets,
        member_maxima_c,
        prices,
        range,
        size,
        slippage,
    )
}

fn evaluate_four_bucket_band_for_markets(
    markets: &[Market],
    member_maxima_c: &[f64],
    prices: &[f64],
    range: std::ops::Range<usize>,
    size: f64,
    slippage: f64,
) -> Option<BandChoice> {
    if range.len() != 4 || range.end > markets.len() || prices.len() != markets.len() {
        return None;
    }
    let buckets = markets[range.clone()]
        .iter()
        .filter_map(|market| market.question.as_deref().and_then(parse_bucket))
        .collect::<Vec<_>>();
    if buckets.len() != 4 {
        return None;
    }
    let covered = member_maxima_c
        .iter()
        .filter(|temperature| {
            let rounded = temperature.round() as i32;
            buckets.iter().any(|bucket| bucket.contains(rounded))
        })
        .count();
    let coverage = covered as f64 / member_maxima_c.len() as f64;
    let entry_prices = prices[range.clone()].to_vec();
    let entry_cost = size * entry_prices.iter().sum::<f64>();
    let taker_fee = size
        * entry_prices
            .iter()
            .map(|price| 0.05 * price * (1.0 - price))
            .sum::<f64>();
    let total_cost = entry_cost + taker_fee;
    let conservative_total_cost = size
        * entry_prices
            .iter()
            .map(|price| {
                let slipped = (price + slippage).min(0.999);
                slipped + 0.05 * slipped * (1.0 - slipped)
            })
            .sum::<f64>();
    Some(BandChoice {
        range,
        coverage,
        entry_prices,
        entry_cost,
        taker_fee,
        total_cost,
        expected_pnl: size * coverage - total_cost,
        conservative_total_cost,
        conservative_expected_pnl: size * coverage - conservative_total_cost,
    })
}

async fn fetch_markets(client: &gamma::Client, date: NaiveDate) -> Result<Vec<Market>> {
    let request = SearchRequest::builder()
        .q("highest temperature in Tokyo")
        .limit_per_type(100)
        .build();
    let results = client.search(&request).await?;

    let mut markets: Vec<Market> = results
        .events
        .unwrap_or_default()
        .into_iter()
        .flat_map(|event| event.markets.unwrap_or_default())
        .filter(|market| {
            market.end_date_iso == Some(date)
                && market
                    .question
                    .as_deref()
                    .is_some_and(|q| q.contains("highest temperature in Tokyo"))
                && market.closed != Some(true)
        })
        .collect();

    markets.sort_by_key(|market| {
        market
            .question
            .as_deref()
            .and_then(parse_bucket)
            .map_or(i32::MAX, |bucket| bucket.threshold)
    });
    Ok(markets)
}

async fn fetch_ensemble(date: NaiveDate, model: &str) -> Result<(Value, Vec<f64>)> {
    let client = reqwest::Client::new();
    let response = client
        .get(FORECAST_URL)
        .query(&[
            ("latitude", TOKYO_LATITUDE.to_string()),
            ("longitude", TOKYO_LONGITUDE.to_string()),
            ("hourly", "temperature_2m".to_string()),
            ("models", model.to_string()),
            ("start_date", date.to_string()),
            ("end_date", date.to_string()),
            ("timezone", "Asia/Tokyo".to_string()),
        ])
        .send()
        .await
        .context("Failed to request Open-Meteo ensemble forecast")?
        .error_for_status()
        .context("Open-Meteo ensemble request failed")?
        .json::<Value>()
        .await
        .context("Failed to parse Open-Meteo ensemble response")?;

    let hourly = response
        .get("hourly")
        .and_then(Value::as_object)
        .context("Open-Meteo response is missing hourly data")?;

    let mut series: BTreeMap<&str, &Vec<Value>> = BTreeMap::new();
    for (name, values) in hourly {
        if name == "temperature_2m" || name.starts_with("temperature_2m_member") {
            let values = values
                .as_array()
                .ok_or_else(|| anyhow!("Invalid ensemble series: {name}"))?;
            series.insert(name, values);
        }
    }
    ensure!(
        !series.is_empty(),
        "No ensemble temperature members returned"
    );

    let maxima = series
        .into_values()
        .filter_map(|values| values.iter().filter_map(Value::as_f64).reduce(f64::max))
        .collect::<Vec<_>>();
    ensure!(
        !maxima.is_empty(),
        "No usable ensemble temperatures returned"
    );

    Ok((response, maxima))
}

async fn fetch_last_year_max(date: NaiveDate) -> Result<(NaiveDate, Option<f64>)> {
    let last_year_date = date
        .with_year(date.year() - 1)
        .unwrap_or_else(|| date - Duration::days(365));
    let response = reqwest::Client::new()
        .get(HISTORICAL_WEATHER_URL)
        .query(&[
            ("latitude", TOKYO_LATITUDE.to_string()),
            ("longitude", TOKYO_LONGITUDE.to_string()),
            ("daily", "temperature_2m_max".to_string()),
            ("start_date", last_year_date.to_string()),
            ("end_date", last_year_date.to_string()),
            ("timezone", "Asia/Tokyo".to_string()),
        ])
        .send()
        .await
        .context("Failed to request last year's Tokyo temperature")?
        .error_for_status()
        .context("Open-Meteo historical weather request failed")?
        .json::<Value>()
        .await
        .context("Failed to parse last year's Tokyo temperature")?;
    let maximum = response
        .pointer("/daily/temperature_2m_max/0")
        .and_then(Value::as_f64);
    Ok((last_year_date, maximum))
}

async fn fetch_observed_max(date: NaiveDate) -> Result<(Option<f64>, Option<String>)> {
    let observations = reqwest::Client::new()
        .get(OBSERVATION_URL)
        .query(&[("ids", "RJTT"), ("format", "json"), ("hours", "72")])
        .send()
        .await
        .context("Failed to request RJTT METAR observations")?
        .error_for_status()
        .context("AviationWeather observation request failed")?
        .json::<Vec<Value>>()
        .await
        .context("Failed to parse RJTT METAR observations")?;

    let mut max_temperature: Option<f64> = None;
    let mut latest: Option<(i64, String)> = None;

    for observation in observations {
        let Some(timestamp) = observation.get("obsTime").and_then(Value::as_i64) else {
            continue;
        };
        let Some(utc_time) = DateTime::<Utc>::from_timestamp(timestamp, 0) else {
            continue;
        };
        if (utc_time + Duration::hours(9)).date_naive() != date {
            continue;
        }

        if let Some(temperature) = observation.get("temp").and_then(Value::as_f64) {
            max_temperature = Some(max_temperature.map_or(temperature, |m| m.max(temperature)));
        }
        if let Some(raw) = observation.get("rawOb").and_then(Value::as_str)
            && latest
                .as_ref()
                .is_none_or(|(latest_ts, _)| timestamp > *latest_ts)
        {
            latest = Some((timestamp, raw.to_string()));
        }
    }

    Ok((max_temperature, latest.map(|(_, raw)| raw)))
}

async fn settle_tokyo_paper_signal(signal_path: &PathBuf, output: OutputFormat) -> Result<()> {
    let contents = fs::read_to_string(signal_path)
        .with_context(|| format!("Failed to read paper signal {}", signal_path.display()))?;
    let signal: TokyoSignalReport = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse paper signal {}", signal_path.display()))?;
    ensure!(
        !signal.positions.is_empty(),
        "Paper signal has no immutable entry positions"
    );

    let events =
        fetch_resolved_events(Some(signal.target_date), Some(signal.target_date), 1).await?;
    let event = events.first();
    let winning_bucket = event.and_then(|resolved| {
        resolved.markets[resolved.winner_index]
            .question
            .as_deref()
            .map(bucket_label)
    });
    let (observed_max_c, _) = fetch_observed_max(signal.target_date).await?;
    let selected_band_hit = winning_bucket.as_ref().map(|winner| {
        signal
            .positions
            .iter()
            .any(|position| &position.bucket == winner)
    });
    let counterfactual_payout = winning_bucket.as_ref().map(|winner| {
        signal
            .positions
            .iter()
            .filter(|position| &position.bucket == winner)
            .map(|position| position.shares)
            .sum::<f64>()
    });
    let quoted_cost = signal
        .positions
        .iter()
        .map(|position| position.cost_after_fee)
        .sum::<f64>();
    let conservative_quoted_cost = signal
        .positions
        .iter()
        .map(|position| position.conservative_cost)
        .sum::<f64>();
    let traded = signal.action == "PAPER_BUY" && winning_bucket.is_some();
    let deployed_cost_after_fee = if traded { quoted_cost } else { 0.0 };
    let conservative_deployed_cost = if traded {
        conservative_quoted_cost
    } else {
        0.0
    };
    let payout = if traded {
        counterfactual_payout.unwrap_or(0.0)
    } else {
        0.0
    };
    let report = TokyoPaperSettlementReport {
        settled_at: Utc::now(),
        target_date: signal.target_date,
        signal_generated_at: signal.generated_at,
        scheduled_entry_time_utc: signal.scheduled_entry_time_utc,
        quote_time_utc: signal.quote_time_utc,
        entry_delay_seconds: signal.entry_delay_seconds,
        entry_timing: signal.entry_timing.clone(),
        signal_action: signal.action.clone(),
        status: if winning_bucket.is_some() {
            "SETTLED".to_string()
        } else {
            "PENDING".to_string()
        },
        winning_bucket,
        observed_max_c,
        selected_band_hit,
        deployed_cost_after_fee,
        conservative_deployed_cost,
        payout,
        realized_pnl_after_fee: payout - deployed_cost_after_fee,
        conservative_realized_pnl: payout - conservative_deployed_cost,
        counterfactual_payout,
        counterfactual_pnl_after_fee: counterfactual_payout.map(|value| value - quoted_cost),
        conservative_counterfactual_pnl: counterfactual_payout
            .map(|value| value - conservative_quoted_cost),
        positions: signal.positions,
        notes: vec![
            "Realized P&L uses only the immutable best-ask snapshot captured at the recorded quote time; prices are never reconstructed after settlement.".to_string(),
            "Historical backtest price proxies are intentionally excluded from this forward paper ledger.".to_string(),
        ],
    };
    print_tokyo_paper_settlement(&report, output)
}

async fn fetch_fee_schedule(market: Option<&Market>) -> Result<Option<FeeSchedule>> {
    let Some(slug) = market.and_then(|market| market.slug.as_deref()) else {
        return Ok(None);
    };
    let response = reqwest::Client::new()
        .get(GAMMA_MARKETS_URL)
        .query(&[("slug", slug)])
        .send()
        .await
        .context("Failed to request the market fee schedule")?
        .error_for_status()
        .context("Gamma fee schedule request failed")?
        .json::<Vec<Value>>()
        .await
        .context("Failed to parse the market fee schedule")?;
    let Some(schedule) = response
        .first()
        .and_then(|market| market.get("feeSchedule"))
    else {
        return Ok(None);
    };

    Ok(Some(FeeSchedule {
        rate: schedule
            .get("rate")
            .and_then(Value::as_f64)
            .context("Fee schedule is missing rate")?,
        exponent: schedule
            .get("exponent")
            .and_then(Value::as_i64)
            .context("Fee schedule is missing exponent")?
            .try_into()
            .context("Invalid fee exponent")?,
        taker_only: schedule
            .get("takerOnly")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        rebate_rate: schedule.get("rebateRate").and_then(Value::as_f64),
    }))
}

fn taker_fee_per_share(price: f64, schedule: Option<&FeeSchedule>) -> Option<f64> {
    schedule.map(|fee| fee.rate * (price * (1.0 - price)).powi(fee.exponent))
}

fn build_rows(
    markets: &[Market],
    maxima: &[f64],
    bias_c: f64,
    fee_schedule: Option<&FeeSchedule>,
) -> Vec<WeatherMarketRow> {
    markets
        .iter()
        .filter_map(|market| {
            let question = market.question.as_deref()?;
            let bucket = parse_bucket(question)?;
            let matches = maxima
                .iter()
                .filter(|temperature| bucket.contains((**temperature + bias_c).round() as i32))
                .count();
            let probability = matches as f64 / maxima.len() as f64;
            let best_bid = market.best_bid.and_then(|v| v.to_f64());
            let best_ask = market.best_ask.and_then(|v| v.to_f64());
            let taker_fee = best_ask.and_then(|ask| taker_fee_per_share(ask, fee_schedule));
            let effective_ask = best_ask.zip(taker_fee).map(|(ask, fee)| ask + fee);
            let tokens = market.clob_token_ids.as_deref().unwrap_or_default();

            Some(WeatherMarketRow {
                bucket: bucket_label(question),
                model_probability: probability,
                best_bid,
                best_ask,
                raw_edge_vs_ask: best_ask.map(|ask| probability - ask),
                taker_fee_per_share: taker_fee,
                effective_ask,
                net_edge_after_taker_fee: effective_ask.map(|cost| probability - cost),
                yes_token: tokens.first().map(ToString::to_string),
                no_token: tokens.get(1).map(ToString::to_string),
                slug: market.slug.clone(),
            })
        })
        .collect()
}

fn hedge_summary(markets: &[Market], fee_schedule: Option<&FeeSchedule>) -> HedgeSummary {
    let ask_values = markets
        .iter()
        .filter_map(|market| market.best_ask.and_then(|v| v.to_f64()))
        .collect::<Vec<_>>();
    let bid_values = markets
        .iter()
        .filter_map(|market| market.best_bid.and_then(|v| v.to_f64()))
        .collect::<Vec<_>>();
    let all_one_group = !markets.is_empty()
        && markets.iter().all(|market| market.neg_risk == Some(true))
        && markets
            .first()
            .and_then(|market| market.neg_risk_market_id)
            .is_some_and(|group| {
                markets
                    .iter()
                    .all(|market| market.neg_risk_market_id == Some(group))
            });

    let complete_set_ask =
        (ask_values.len() == markets.len()).then(|| ask_values.iter().sum::<f64>());
    let complete_set_bid =
        (bid_values.len() == markets.len()).then(|| bid_values.iter().sum::<f64>());
    let complete_set_taker_fee = (ask_values.len() == markets.len())
        .then(|| {
            ask_values
                .iter()
                .map(|price| taker_fee_per_share(*price, fee_schedule))
                .collect::<Option<Vec<_>>>()
        })
        .flatten()
        .map(|fees| fees.iter().sum::<f64>());
    let complete_set_effective_cost = complete_set_ask
        .zip(complete_set_taker_fee)
        .map(|(ask, fee)| ask + fee);

    HedgeSummary {
        is_exhaustive_neg_risk_group: all_one_group,
        quoted_markets: markets.len(),
        complete_set_ask,
        complete_set_bid,
        gross_buy_all_edge: complete_set_ask.map(|cost| 1.0 - cost),
        gross_sell_all_edge: complete_set_bid.map(|proceeds| proceeds - 1.0),
        complete_set_taker_fee,
        complete_set_effective_cost,
        net_buy_all_edge: complete_set_effective_cost.map(|cost| 1.0 - cost),
    }
}

pub async fn execute(
    client: &gamma::Client,
    args: WeatherArgs,
    output: OutputFormat,
) -> Result<()> {
    match args.command {
        WeatherCommand::Tokyo {
            date,
            model,
            bias_c,
        } => {
            let date = date.map_or_else(default_tokyo_date, Ok)?;
            let markets = fetch_markets(client, date).await?;
            let (forecast, maxima) = fetch_ensemble(date, &model).await?;
            let (last_year_date, last_year_max_c) = fetch_last_year_max(date).await?;
            let (observed_max_c, latest_metar) = fetch_observed_max(date).await?;
            let fee_schedule = fetch_fee_schedule(markets.first()).await?;
            let rows = build_rows(&markets, &maxima, bias_c, fee_schedule.as_ref());
            let mean = maxima.iter().sum::<f64>() / maxima.len() as f64;
            let min = maxima.iter().copied().reduce(f64::min).unwrap_or(mean);
            let max = maxima.iter().copied().reduce(f64::max).unwrap_or(mean);

            let report = TokyoWeatherReport {
                generated_at: Utc::now(),
                target_date: date,
                station: STATION.to_string(),
                station_latitude: TOKYO_LATITUDE,
                station_longitude: TOKYO_LONGITUDE,
                forecast_grid_latitude: forecast.get("latitude").and_then(Value::as_f64),
                forecast_grid_longitude: forecast.get("longitude").and_then(Value::as_f64),
                model,
                bias_c,
                ensemble_members: maxima.len(),
                ensemble_max_min_c: min + bias_c,
                ensemble_max_mean_c: mean + bias_c,
                ensemble_max_max_c: max + bias_c,
                last_year_date,
                last_year_max_c,
                observed_max_c,
                latest_metar,
                resolution_url: RESOLUTION_URL.to_string(),
                forecast_url: FORECAST_URL.to_string(),
                observation_url: OBSERVATION_URL.to_string(),
                historical_weather_url: HISTORICAL_WEATHER_URL.to_string(),
                fee_schedule: fee_schedule.clone(),
                markets: rows,
                hedge: hedge_summary(&markets, fee_schedule.as_ref()),
                notes: vec![
                    "Model probabilities are raw ensemble frequencies, not calibrated forecasts."
                        .to_string(),
                    "Net edge includes the current taker fee schedule but excludes slippage, fill risk, and settlement basis risk.".to_string(),
                    "METAR is a live cross-check; the Polymarket resolution page remains authoritative."
                        .to_string(),
                ],
            };
            print_tokyo_weather(&report, output)
        }
        WeatherCommand::SignalTokyo {
            date,
            model,
            legacy_weight,
            size,
            slippage,
            min_expected_pnl,
            max_entry_delay_minutes,
        } => {
            ensure!(
                (0.0..=1.0).contains(&legacy_weight),
                "--legacy-weight must be between 0 and 1"
            );
            ensure!(size > 0.0, "--size must be greater than zero");
            ensure!(slippage >= 0.0, "--slippage must not be negative");
            ensure!(
                min_expected_pnl >= 0.0,
                "--min-expected-pnl must not be negative"
            );

            let date = date.map_or_else(default_tokyo_date, Ok)?;
            let markets = fetch_markets(client, date).await?;
            ensure!(
                markets.len() >= 4,
                "No complete open Tokyo temperature market found for {date}"
            );
            let (_, maxima) = fetch_ensemble(date, &model).await?;
            let fee_schedule = fetch_fee_schedule(markets.first()).await?;
            let prices = fetch_live_best_asks(&markets).await?;
            let quote_time = Utc::now();
            let scheduled_entry_time = scheduled_paper_entry_time(date)?;
            let (entry_timing, entry_delay_seconds) =
                paper_entry_timing(quote_time, scheduled_entry_time, max_entry_delay_minutes);
            let optimized = choose_four_bucket_band_for_markets(
                &markets,
                &maxima,
                &prices,
                size * (1.0 - legacy_weight),
                slippage,
            )
            .context("Could not select an optimized four-bucket band")?;

            let ensemble_mean = maxima.iter().sum::<f64>() / maxima.len() as f64;
            let legacy_temperature = ensemble_mean.floor() as i32;
            let legacy_index = markets
                .iter()
                .position(|market| {
                    market
                        .question
                        .as_deref()
                        .and_then(parse_bucket)
                        .is_some_and(|bucket| bucket.contains(legacy_temperature))
                })
                .context("Ensemble mean did not match a Tokyo temperature bucket")?;
            let legacy_start = legacy_index.saturating_sub(2).min(markets.len() - 4);
            let legacy = evaluate_four_bucket_band_for_markets(
                &markets,
                &maxima,
                &prices,
                legacy_start..legacy_start + 4,
                size * legacy_weight,
                slippage,
            )
            .context("Could not evaluate the legacy four-bucket band")?;
            let labels = |range: std::ops::Range<usize>| {
                markets[range]
                    .iter()
                    .filter_map(|market| market.question.as_deref().map(bucket_label))
                    .collect::<Vec<_>>()
            };
            let expected_pnl = optimized.expected_pnl + legacy.expected_pnl;
            let conservative_expected_pnl =
                optimized.conservative_expected_pnl + legacy.conservative_expected_pnl;
            let mut positions =
                build_paper_positions(&markets, &legacy, "legacy", size * legacy_weight, slippage);
            positions.extend(build_paper_positions(
                &markets,
                &optimized,
                "optimizer",
                size * (1.0 - legacy_weight),
                slippage,
            ));
            let report = TokyoSignalReport {
                generated_at: quote_time,
                scheduled_entry_time_utc: scheduled_entry_time,
                quote_time_utc: quote_time,
                entry_delay_seconds,
                maximum_entry_delay_minutes: max_entry_delay_minutes,
                entry_timing: entry_timing.to_string(),
                target_date: date,
                model,
                ensemble_members: maxima.len(),
                legacy_weight,
                optimizer_weight: 1.0 - legacy_weight,
                position_size: size,
                slippage_per_share: slippage,
                minimum_conservative_expected_pnl: min_expected_pnl,
                legacy_band: labels(legacy.range),
                optimized_band: labels(optimized.range),
                legacy_coverage: legacy.coverage,
                optimized_coverage: optimized.coverage,
                blended_coverage: legacy.coverage * legacy_weight
                    + optimized.coverage * (1.0 - legacy_weight),
                total_cost_after_fee: legacy.total_cost + optimized.total_cost,
                conservative_total_cost: legacy.conservative_total_cost
                    + optimized.conservative_total_cost,
                expected_pnl_after_fee: expected_pnl,
                conservative_expected_pnl,
                action: if entry_timing != "ON_TIME" {
                    "SKIP_OFF_SCHEDULE".to_string()
                } else if conservative_expected_pnl >= min_expected_pnl {
                    "PAPER_BUY".to_string()
                } else {
                    "SKIP".to_string()
                },
                positions,
                notes: vec![
                    "Simulation only: this command never reads a wallet or places an order."
                        .to_string(),
                    "Forward-ledger P&L must use this immutable quote snapshot; a delayed or early run is excluded from standard strategy returns."
                        .to_string(),
                    "Entry asks come directly from one public CLOB batch-prices request (SELL side = best ask), not from Gamma display prices."
                        .to_string(),
                    format!(
                        "Market fee schedule: {}",
                        fee_schedule.map_or_else(
                            || "unavailable; 0.05 weather taker rate assumed".to_string(),
                            |fee| format!("rate {}, exponent {}", fee.rate, fee.exponent)
                        )
                    ),
                    "The optimized leg uses raw ensemble frequencies; probabilities are not calibrated."
                        .to_string(),
                ],
            };
            print_tokyo_signal(&report, output)
        }
        WeatherCommand::SettleTokyoPaper { signal } => {
            settle_tokyo_paper_signal(&signal, output).await
        }
        WeatherCommand::BacktestTokyo {
            since,
            until,
            recent,
            lead_days,
            model,
            size,
            entry_hour_utc,
            slippage,
            legacy_weight,
        } => {
            ensure!(size > 0.0, "--size must be greater than zero");
            ensure!(slippage >= 0.0, "--slippage must not be negative");
            ensure!(
                (0.0..=1.0).contains(&legacy_weight),
                "--legacy-weight must be between 0 and 1"
            );
            ensure!(
                since.zip(until).is_none_or(|(start, end)| start <= end),
                "--since must not be after --until"
            );
            let events = fetch_resolved_events(since, until, recent).await?;
            ensure!(
                !events.is_empty(),
                "No resolved Tokyo daily-high events found in the requested range"
            );
            let first_date = events.first().map(|event| event.date).unwrap();
            let last_date = events.last().map(|event| event.date).unwrap();
            let mut missing_event_dates = Vec::new();
            let mut cursor = first_date;
            while cursor <= last_date {
                if !events.iter().any(|event| event.date == cursor) {
                    missing_event_dates.push(cursor);
                }
                cursor = cursor
                    .succ_opt()
                    .context("Could not advance through backtest dates")?;
            }
            let distributions =
                fetch_previous_run_distributions(first_date, last_date, lead_days, &model).await?;
            let http = reqwest::Client::new();
            let mut rows = Vec::new();
            for event in &events {
                let Some(distribution) = distributions.get(&event.date) else {
                    continue;
                };
                let Some(mut row) = build_backtest_row(event, distribution.central_max_c) else {
                    continue;
                };
                row.reconstructed_member_max_min_c = distribution
                    .member_maxima_c
                    .iter()
                    .copied()
                    .reduce(f64::min);
                row.reconstructed_member_max_mean_c = (!distribution.member_maxima_c.is_empty())
                    .then(|| {
                        distribution.member_maxima_c.iter().sum::<f64>()
                            / distribution.member_maxima_c.len() as f64
                    });
                row.reconstructed_member_max_max_c = distribution
                    .member_maxima_c
                    .iter()
                    .copied()
                    .reduce(f64::max);
                row.position_size_per_bucket = size;
                let tokens = event
                    .markets
                    .iter()
                    .filter_map(|market| {
                        market
                            .clob_token_ids
                            .as_deref()
                            .and_then(|ids| ids.first())
                            .map(ToString::to_string)
                    })
                    .collect::<Vec<_>>();
                let Some(entry_date) = event.date.pred_opt() else {
                    rows.push(row);
                    continue;
                };
                let Some(entry_naive) = entry_date.and_hms_opt(u32::from(entry_hour_utc), 0, 0)
                else {
                    rows.push(row);
                    continue;
                };
                let entry_time = DateTime::<Utc>::from_naive_utc_and_offset(entry_naive, Utc);
                row.entry_time_utc = Some(entry_time);
                if tokens.len() == event.markets.len()
                    && let Some(prices) =
                        fetch_entry_prices(&http, &tokens, entry_time.timestamp()).await?
                    && let Some(optimized) = choose_four_bucket_band(
                        event,
                        &distribution.member_maxima_c,
                        &prices,
                        size * (1.0 - legacy_weight),
                        slippage,
                    )
                {
                    let legacy_temperature = distribution.central_max_c.floor() as i32;
                    let legacy_index = event
                        .markets
                        .iter()
                        .position(|market| {
                            market
                                .question
                                .as_deref()
                                .and_then(parse_bucket)
                                .is_some_and(|bucket| bucket.contains(legacy_temperature))
                        })
                        .context("Historical forecast did not match a market bucket")?;
                    let legacy_start = legacy_index.saturating_sub(2).min(event.markets.len() - 4);
                    let legacy = evaluate_four_bucket_band(
                        event,
                        &distribution.member_maxima_c,
                        &prices,
                        legacy_start..legacy_start + 4,
                        size * legacy_weight,
                        slippage,
                    )
                    .context("Could not evaluate legacy four-bucket band")?;

                    row.selected_band = event.markets[optimized.range.clone()]
                        .iter()
                        .filter_map(|market| market.question.as_deref().map(bucket_label))
                        .collect();
                    if legacy_weight > 0.0 {
                        row.legacy_band = event.markets[legacy.range.clone()]
                            .iter()
                            .filter_map(|market| market.question.as_deref().map(bucket_label))
                            .collect();
                    }
                    row.modeled_coverage = Some(
                        optimized.coverage * (1.0 - legacy_weight)
                            + legacy.coverage * legacy_weight,
                    );
                    let expected_pnl = optimized.expected_pnl + legacy.expected_pnl;
                    let conservative_expected_pnl =
                        optimized.conservative_expected_pnl + legacy.conservative_expected_pnl;
                    row.expected_pnl_after_fee = Some(expected_pnl);
                    row.conservative_expected_pnl = Some(conservative_expected_pnl);
                    let optimized_hit = optimized.range.contains(&event.winner_index);
                    let legacy_hit = legacy.range.contains(&event.winner_index);
                    row.selected_band_hit = optimized_hit || (legacy_weight > 0.0 && legacy_hit);
                    row.entry_prices = optimized.entry_prices.clone();

                    // A non-positive edge is a real decision: keep the simulated capital in cash.
                    row.traded = conservative_expected_pnl > 0.0;
                    let payout = if row.traded {
                        size * (1.0 - legacy_weight) * if optimized_hit { 1.0 } else { 0.0 }
                            + size * legacy_weight * if legacy_hit { 1.0 } else { 0.0 }
                    } else {
                        0.0
                    };
                    let multiplier = if row.traded { 1.0 } else { 0.0 };
                    let entry_cost = optimized.entry_cost + legacy.entry_cost;
                    let taker_fee = optimized.taker_fee + legacy.taker_fee;
                    let total_cost = optimized.total_cost + legacy.total_cost;
                    let conservative_total_cost =
                        optimized.conservative_total_cost + legacy.conservative_total_cost;
                    row.entry_cost = Some(entry_cost * multiplier);
                    row.taker_fee = Some(taker_fee * multiplier);
                    row.total_cost = Some(total_cost * multiplier);
                    row.payout = Some(payout);
                    row.pnl = Some(payout - total_cost * multiplier);
                    row.conservative_total_cost = Some(conservative_total_cost * multiplier);
                    row.conservative_pnl = Some(payout - conservative_total_cost * multiplier);
                }
                rows.push(row);
            }
            ensure!(
                !rows.is_empty(),
                "No historical forecast values matched the resolved events"
            );
            let sample_count = rows.len();
            let exact_hits = rows.iter().filter(|row| row.exact_hit).count();
            let priced_decision_count = rows
                .iter()
                .filter(|row| row.modeled_coverage.is_some())
                .count();
            let traded_rows = rows.iter().filter(|row| row.traded).collect::<Vec<_>>();
            let traded_event_count = traded_rows.len();
            let skipped_event_count = priced_decision_count.saturating_sub(traded_event_count);
            let selected_band_hits = traded_rows
                .iter()
                .filter(|row| row.selected_band_hit)
                .count();
            let profitable_events = traded_rows
                .iter()
                .filter(|row| row.pnl.is_some_and(|pnl| pnl > 0.0))
                .count();
            let total_cost = traded_rows
                .iter()
                .filter_map(|row| row.total_cost)
                .sum::<f64>();
            let total_taker_fee = traded_rows
                .iter()
                .filter_map(|row| row.taker_fee)
                .sum::<f64>();
            let total_payout = traded_rows.iter().filter_map(|row| row.payout).sum::<f64>();
            let total_pnl = total_payout - total_cost;
            let conservative_total_cost = traded_rows
                .iter()
                .filter_map(|row| row.conservative_total_cost)
                .sum::<f64>();
            let conservative_total_pnl = total_payout - conservative_total_cost;
            let report = TokyoBacktestReport {
                generated_at: Utc::now(),
                model,
                lead_days,
                since: first_date,
                until: last_date,
                missing_event_dates,
                sample_count,
                exact_hits,
                exact_accuracy: exact_hits as f64 / sample_count as f64,
                priced_decision_count,
                traded_event_count,
                skipped_event_count,
                selected_band_hits,
                selected_band_accuracy: if traded_event_count > 0 {
                    selected_band_hits as f64 / traded_event_count as f64
                } else {
                    0.0
                },
                position_size_per_bucket: size,
                entry_hour_utc,
                conservative_slippage_per_share: slippage,
                legacy_weight,
                profitable_events,
                total_cost,
                total_taker_fee,
                total_payout,
                total_pnl,
                roi: if total_cost > 0.0 {
                    total_pnl / total_cost
                } else {
                    0.0
                },
                conservative_total_cost,
                conservative_total_pnl,
                conservative_roi: if conservative_total_cost > 0.0 {
                    conservative_total_pnl / conservative_total_cost
                } else {
                    0.0
                },
                rows,
                notes: vec![
                    "Forecasts use prior-run GEFS ensemble means and spreads archived at a fixed lead time for each valid hour; settlement winners come from closed Polymarket markets.".to_string(),
                    "Because individual historical GEFS members are retained for only three days, 31 pseudo-members are reconstructed from normal quantiles of each hourly mean/spread with rank preserved across hours. This approximates, but does not reproduce, the original 31 members.".to_string(),
                    format!("Every contiguous four-bucket band is evaluated at historical entry prices. {:.0}% of capital follows the legacy lower-leaning band and {:.0}% follows the distribution optimizer; the blended trade is skipped when conservative expected P&L is not positive.", legacy_weight * 100.0, (1.0 - legacy_weight) * 100.0),
                    "CLOB price history is a price series, not an archived best-ask book. The conservative scenario adds the configured slippage to every share.".to_string(),
                ],
            };
            print_tokyo_backtest(&report, output)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exact_bucket() {
        let bucket =
            parse_bucket("Will the highest temperature in Tokyo be 25°C on September 5?").unwrap();
        assert!(bucket.contains(25));
        assert!(!bucket.contains(24));
    }

    #[test]
    fn parses_tail_buckets() {
        let low =
            parse_bucket("Will the highest temperature in Tokyo be 21°C or below on September 5?")
                .unwrap();
        let high =
            parse_bucket("Will the highest temperature in Tokyo be 31°C or higher on September 5?")
                .unwrap();
        assert!(low.contains(20));
        assert!(high.contains(32));
        assert!(!low.contains(22));
        assert!(!high.contains(30));
    }

    #[test]
    fn reconstructed_members_take_each_trajectory_daily_maximum() {
        let maxima = reconstruct_member_maxima(&[(20.0, 1.0), (22.0, 0.5), (21.0, 2.0)], 31);
        assert_eq!(maxima.len(), 31);
        assert!(maxima.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(maxima[15] >= 21.99 && maxima[15] <= 22.01);
    }

    #[test]
    fn paper_entry_time_is_fixed_on_preceding_day() {
        let target = NaiveDate::from_ymd_opt(2026, 9, 6).unwrap();
        let scheduled = scheduled_paper_entry_time(target).unwrap();
        assert_eq!(scheduled.to_rfc3339(), "2026-09-05T05:17:00+00:00");
    }

    #[test]
    fn delayed_paper_quote_is_excluded_from_standard_returns() {
        let scheduled = DateTime::parse_from_rfc3339("2026-09-05T05:17:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            paper_entry_timing(scheduled + Duration::minutes(15), scheduled, 15),
            ("ON_TIME", 900)
        );
        assert_eq!(
            paper_entry_timing(scheduled + Duration::minutes(16), scheduled, 15),
            ("LATE", 960)
        );
        assert_eq!(
            paper_entry_timing(scheduled - Duration::seconds(1), scheduled, 15),
            ("EARLY", -1)
        );
    }
}
