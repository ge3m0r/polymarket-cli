use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tabled::settings::Style;
use tabled::{Table, Tabled};

use super::{OutputFormat, print_json};

#[derive(Debug, Serialize)]
pub struct WeatherMarketRow {
    pub bucket: String,
    pub model_probability: f64,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub raw_edge_vs_ask: Option<f64>,
    pub taker_fee_per_share: Option<f64>,
    pub effective_ask: Option<f64>,
    pub net_edge_after_taker_fee: Option<f64>,
    pub yes_token: Option<String>,
    pub no_token: Option<String>,
    pub slug: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeeSchedule {
    pub rate: f64,
    pub exponent: i32,
    pub taker_only: bool,
    pub rebate_rate: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct HedgeSummary {
    pub is_exhaustive_neg_risk_group: bool,
    pub quoted_markets: usize,
    pub complete_set_ask: Option<f64>,
    pub complete_set_bid: Option<f64>,
    pub gross_buy_all_edge: Option<f64>,
    pub gross_sell_all_edge: Option<f64>,
    pub complete_set_taker_fee: Option<f64>,
    pub complete_set_effective_cost: Option<f64>,
    pub net_buy_all_edge: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct TokyoWeatherReport {
    pub generated_at: DateTime<Utc>,
    pub target_date: NaiveDate,
    pub station: String,
    pub station_latitude: f64,
    pub station_longitude: f64,
    pub forecast_grid_latitude: Option<f64>,
    pub forecast_grid_longitude: Option<f64>,
    pub model: String,
    pub bias_c: f64,
    pub ensemble_members: usize,
    pub ensemble_max_min_c: f64,
    pub ensemble_max_mean_c: f64,
    pub ensemble_max_max_c: f64,
    pub last_year_date: NaiveDate,
    pub last_year_max_c: Option<f64>,
    pub observed_max_c: Option<f64>,
    pub latest_metar: Option<String>,
    pub resolution_url: String,
    pub forecast_url: String,
    pub observation_url: String,
    pub historical_weather_url: String,
    pub fee_schedule: Option<FeeSchedule>,
    pub markets: Vec<WeatherMarketRow>,
    pub hedge: HedgeSummary,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TokyoBacktestRow {
    pub date: NaiveDate,
    pub forecast_max_c: f64,
    pub reconstructed_member_max_min_c: Option<f64>,
    pub reconstructed_member_max_mean_c: Option<f64>,
    pub reconstructed_member_max_max_c: Option<f64>,
    pub predicted_bucket: String,
    pub winning_bucket: String,
    pub exact_hit: bool,
    pub selected_band: Vec<String>,
    pub legacy_band: Vec<String>,
    pub modeled_coverage: Option<f64>,
    pub expected_pnl_after_fee: Option<f64>,
    pub conservative_expected_pnl: Option<f64>,
    pub traded: bool,
    pub selected_band_hit: bool,
    pub entry_time_utc: Option<DateTime<Utc>>,
    pub entry_prices: Vec<f64>,
    pub position_size_per_bucket: f64,
    pub entry_cost: Option<f64>,
    pub taker_fee: Option<f64>,
    pub total_cost: Option<f64>,
    pub payout: Option<f64>,
    pub pnl: Option<f64>,
    pub conservative_total_cost: Option<f64>,
    pub conservative_pnl: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct TokyoBacktestReport {
    pub generated_at: DateTime<Utc>,
    pub model: String,
    pub lead_days: u8,
    pub since: NaiveDate,
    pub until: NaiveDate,
    pub missing_event_dates: Vec<NaiveDate>,
    pub sample_count: usize,
    pub exact_hits: usize,
    pub exact_accuracy: f64,
    pub priced_decision_count: usize,
    pub traded_event_count: usize,
    pub skipped_event_count: usize,
    pub selected_band_hits: usize,
    pub selected_band_accuracy: f64,
    pub position_size_per_bucket: f64,
    pub entry_hour_utc: u8,
    pub conservative_slippage_per_share: f64,
    pub legacy_weight: f64,
    pub profitable_events: usize,
    pub total_cost: f64,
    pub total_taker_fee: f64,
    pub total_payout: f64,
    pub total_pnl: f64,
    pub roi: f64,
    pub conservative_total_cost: f64,
    pub conservative_total_pnl: f64,
    pub conservative_roi: f64,
    pub rows: Vec<TokyoBacktestRow>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokyoPaperPosition {
    pub leg: String,
    pub bucket: String,
    pub yes_token: Option<String>,
    pub market_slug: Option<String>,
    pub shares: f64,
    pub best_ask: f64,
    pub taker_fee_per_share: f64,
    pub cost_after_fee: f64,
    pub conservative_fill_price: f64,
    pub conservative_fee_per_share: f64,
    pub conservative_cost: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TokyoSignalReport {
    pub generated_at: DateTime<Utc>,
    pub scheduled_entry_time_utc: DateTime<Utc>,
    pub quote_time_utc: DateTime<Utc>,
    pub entry_delay_seconds: i64,
    pub maximum_entry_delay_minutes: u16,
    pub entry_timing: String,
    pub target_date: NaiveDate,
    pub model: String,
    pub ensemble_members: usize,
    pub legacy_weight: f64,
    pub optimizer_weight: f64,
    pub position_size: f64,
    pub slippage_per_share: f64,
    pub minimum_conservative_expected_pnl: f64,
    pub legacy_band: Vec<String>,
    pub optimized_band: Vec<String>,
    pub legacy_coverage: f64,
    pub optimized_coverage: f64,
    pub blended_coverage: f64,
    pub total_cost_after_fee: f64,
    pub conservative_total_cost: f64,
    pub expected_pnl_after_fee: f64,
    pub conservative_expected_pnl: f64,
    pub action: String,
    pub positions: Vec<TokyoPaperPosition>,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TokyoPaperSettlementReport {
    pub settled_at: DateTime<Utc>,
    pub target_date: NaiveDate,
    pub signal_generated_at: DateTime<Utc>,
    pub scheduled_entry_time_utc: DateTime<Utc>,
    pub quote_time_utc: DateTime<Utc>,
    pub entry_delay_seconds: i64,
    pub entry_timing: String,
    pub signal_action: String,
    pub status: String,
    pub winning_bucket: Option<String>,
    pub observed_max_c: Option<f64>,
    pub selected_band_hit: Option<bool>,
    pub deployed_cost_after_fee: f64,
    pub conservative_deployed_cost: f64,
    pub payout: f64,
    pub realized_pnl_after_fee: f64,
    pub conservative_realized_pnl: f64,
    pub counterfactual_payout: Option<f64>,
    pub counterfactual_pnl_after_fee: Option<f64>,
    pub conservative_counterfactual_pnl: Option<f64>,
    pub positions: Vec<TokyoPaperPosition>,
    pub notes: Vec<String>,
}

#[derive(Tabled)]
struct WeatherRow {
    #[tabled(rename = "Bucket")]
    bucket: String,
    #[tabled(rename = "Model")]
    model: String,
    #[tabled(rename = "Bid")]
    bid: String,
    #[tabled(rename = "Ask")]
    ask: String,
    #[tabled(rename = "Fee")]
    fee: String,
    #[tabled(rename = "Effective")]
    effective: String,
    #[tabled(rename = "Net edge")]
    edge: String,
}

#[derive(Tabled)]
struct BacktestRow {
    #[tabled(rename = "Date")]
    date: String,
    #[tabled(rename = "Member daily-max range")]
    forecast: String,
    #[tabled(rename = "Predicted")]
    predicted: String,
    #[tabled(rename = "Winner")]
    winner: String,
    #[tabled(rename = "Exact")]
    exact: String,
    #[tabled(rename = "Selected 4-bucket band")]
    selected: String,
    #[tabled(rename = "Coverage")]
    coverage: String,
    #[tabled(rename = "Decision")]
    decision: String,
    #[tabled(rename = "Cons. exp.")]
    expected: String,
    #[tabled(rename = "Cost")]
    cost: String,
    #[tabled(rename = "P&L")]
    pnl: String,
}

fn probability(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

fn quote(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_string(), |v| format!("{v:.3}"))
}

pub fn print_tokyo_weather(
    report: &TokyoWeatherReport,
    output: OutputFormat,
) -> anyhow::Result<()> {
    if matches!(output, OutputFormat::Json) {
        return print_json(report);
    }

    println!("Tokyo daily-high research — {}", report.target_date);
    println!("Station: {}", report.station);
    println!(
        "Forecast: {} members via {} (bias {:+.2}°C)",
        report.ensemble_members, report.model, report.bias_c
    );
    println!(
        "Ensemble daily max: {:.1} / {:.1} / {:.1}°C (min / mean / max)",
        report.ensemble_max_min_c, report.ensemble_max_mean_c, report.ensemble_max_max_c
    );
    if let Some(last_year) = report.last_year_max_c {
        println!(
            "Last-year same-date reference: {:.1}°C on {} (reanalysis, not settlement data)",
            last_year, report.last_year_date
        );
    }
    if let Some(observed) = report.observed_max_c {
        println!("Observed METAR max so far: {observed:.1}°C");
    }
    println!();

    if report.markets.is_empty() {
        println!("No open Tokyo temperature markets found for this date.");
    } else {
        let rows = report.markets.iter().map(|market| WeatherRow {
            bucket: market.bucket.clone(),
            model: probability(market.model_probability),
            bid: quote(market.best_bid),
            ask: quote(market.best_ask),
            fee: quote(market.taker_fee_per_share),
            effective: quote(market.effective_ask),
            edge: market
                .net_edge_after_taker_fee
                .map_or_else(|| "—".to_string(), probability),
        });
        println!("{}", Table::new(rows).with(Style::rounded()));
    }

    println!();
    println!(
        "NegRisk exhaustive group: {}",
        if report.hedge.is_exhaustive_neg_risk_group {
            "yes"
        } else {
            "not verified"
        }
    );
    println!(
        "Complete-set YES ask / bid: {} / {}",
        quote(report.hedge.complete_set_ask),
        quote(report.hedge.complete_set_bid)
    );
    println!(
        "Gross buy-all / sell-all edge: {} / {}",
        report
            .hedge
            .gross_buy_all_edge
            .map_or_else(|| "—".to_string(), probability),
        report
            .hedge
            .gross_sell_all_edge
            .map_or_else(|| "—".to_string(), probability)
    );
    println!(
        "Complete-set taker fee / effective cost / net edge: {} / {} / {}",
        quote(report.hedge.complete_set_taker_fee),
        quote(report.hedge.complete_set_effective_cost),
        report
            .hedge
            .net_buy_all_edge
            .map_or_else(|| "—".to_string(), probability)
    );
    if let Some(fee) = &report.fee_schedule {
        println!(
            "Fee schedule: rate {:.3}, exponent {}, taker-only {}",
            fee.rate, fee.exponent, fee.taker_only
        );
    }
    println!();
    println!("Resolution: {}", report.resolution_url);
    println!("Warning: net edge includes taker fee, but excludes slippage and model error.");
    Ok(())
}

pub fn print_tokyo_backtest(
    report: &TokyoBacktestReport,
    output: OutputFormat,
) -> anyhow::Result<()> {
    if matches!(output, OutputFormat::Json) {
        return print_json(report);
    }

    println!(
        "Tokyo daily-high backtest — {} to {}",
        report.since, report.until
    );
    println!(
        "Model: {}, forecast lead: {} day(s), resolved events: {}",
        report.model, report.lead_days, report.sample_count
    );
    println!(
        "Capital mix: {:.0}% legacy lower-leaning + {:.0}% distribution optimizer",
        report.legacy_weight * 100.0,
        (1.0 - report.legacy_weight) * 100.0
    );
    if !report.missing_event_dates.is_empty() {
        println!(
            "Warning: missing event dates: {}",
            report
                .missing_event_dates
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!(
        "Exact bucket: {}/{} ({:.1}%)",
        report.exact_hits,
        report.sample_count,
        report.exact_accuracy * 100.0
    );
    println!(
        "Distribution-optimized 4-bucket trades: {}, skipped: {}, hits: {}/{} ({:.1}%)",
        report.traded_event_count,
        report.skipped_event_count,
        report.selected_band_hits,
        report.traded_event_count,
        report.selected_band_accuracy * 100.0
    );
    println!(
        "Paper P&L: cost {:.3}, payout {:.3}, P&L {:+.3}, ROI {:+.1}% ({} priced decisions)",
        report.total_cost,
        report.total_payout,
        report.total_pnl,
        report.roi * 100.0,
        report.priced_decision_count
    );
    println!(
        "Conservative (+{:.3}/share slippage): cost {:.3}, P&L {:+.3}, ROI {:+.1}%",
        report.conservative_slippage_per_share,
        report.conservative_total_cost,
        report.conservative_total_pnl,
        report.conservative_roi * 100.0
    );
    println!();

    let rows = report.rows.iter().map(|row| BacktestRow {
        date: row.date.to_string(),
        forecast: row
            .reconstructed_member_max_min_c
            .zip(row.reconstructed_member_max_max_c)
            .map_or_else(
                || format!("{:.1}°C", row.forecast_max_c),
                |(min, max)| format!("{min:.1}–{max:.1}°C"),
            ),
        predicted: row.predicted_bucket.clone(),
        winner: row.winning_bucket.clone(),
        exact: if row.exact_hit { "yes" } else { "no" }.to_string(),
        selected: if row.selected_band.is_empty() {
            "—".to_string()
        } else if row.legacy_band.is_empty() {
            format!(
                "opt: {} {}",
                row.selected_band.join(" / "),
                if row.selected_band_hit { "✓" } else { "✗" }
            )
        } else {
            format!(
                "old: {} + opt: {} {}",
                row.legacy_band.join(" / "),
                row.selected_band.join(" / "),
                if row.selected_band_hit { "✓" } else { "✗" }
            )
        },
        coverage: row
            .modeled_coverage
            .map_or_else(|| "—".to_string(), probability),
        decision: if row.traded { "BUY" } else { "SKIP" }.to_string(),
        expected: row
            .conservative_expected_pnl
            .map_or_else(|| "—".to_string(), |value| format!("{value:+.3}")),
        cost: row
            .total_cost
            .map_or_else(|| "—".to_string(), |value| format!("{value:.3}")),
        pnl: row
            .pnl
            .map_or_else(|| "—".to_string(), |value| format!("{value:+.3}")),
    });
    println!("{}", Table::new(rows).with(Style::rounded()));
    println!();
    println!(
        "Each day enumerates every contiguous four-bucket band and trades only when its modeled expected P&L remains positive after fee and configured slippage."
    );
    println!(
        "Historical GEFS members are reconstructed from the prior-day archived hourly ensemble mean/spread using 31 normal quantiles with rank preserved across hours; this is an approximation, not the original member archive."
    );
    println!(
        "Historical prices are sampled from CLOB price history; archived best asks are unavailable, so the conservative result adds the configured per-share slippage."
    );
    Ok(())
}

pub fn print_tokyo_signal(report: &TokyoSignalReport, output: OutputFormat) -> anyhow::Result<()> {
    if matches!(output, OutputFormat::Json) {
        return print_json(report);
    }

    println!("Tokyo paper signal — {}", report.target_date);
    println!("Scheduled entry: {}", report.scheduled_entry_time_utc);
    println!(
        "Quote captured: {} ({:+}s, {})",
        report.quote_time_utc, report.entry_delay_seconds, report.entry_timing
    );
    println!(
        "Strategy: {:.0}% legacy + {:.0}% optimizer; {} members",
        report.legacy_weight * 100.0,
        report.optimizer_weight * 100.0,
        report.ensemble_members
    );
    println!("Legacy band: {}", report.legacy_band.join(" / "));
    println!("Optimized band: {}", report.optimized_band.join(" / "));
    println!("Blended coverage: {:.1}%", report.blended_coverage * 100.0);
    println!(
        "Cost after fee / conservative cost: {:.3} / {:.3} USDC",
        report.total_cost_after_fee, report.conservative_total_cost
    );
    println!(
        "Expected P&L / conservative expected P&L: {:+.3} / {:+.3} USDC",
        report.expected_pnl_after_fee, report.conservative_expected_pnl
    );
    println!("Paper action: {}", report.action);
    Ok(())
}

pub fn print_tokyo_paper_settlement(
    report: &TokyoPaperSettlementReport,
    output: OutputFormat,
) -> anyhow::Result<()> {
    if matches!(output, OutputFormat::Json) {
        return print_json(report);
    }

    println!("Tokyo paper settlement — {}", report.target_date);
    println!("Status: {}", report.status);
    println!(
        "Entry quote: {} ({:+}s from schedule, {})",
        report.quote_time_utc, report.entry_delay_seconds, report.entry_timing
    );
    println!("Signal action: {}", report.signal_action);
    if let Some(winner) = &report.winning_bucket {
        println!("Winning bucket: {winner}");
    }
    if let Some(hit) = report.selected_band_hit {
        println!("Selected band hit: {}", if hit { "yes" } else { "no" });
    }
    println!(
        "Deployed cost / payout / realized P&L: {:.3} / {:.3} / {:+.3} USDC",
        report.deployed_cost_after_fee, report.payout, report.realized_pnl_after_fee
    );
    println!(
        "Conservative deployed cost / P&L: {:.3} / {:+.3} USDC",
        report.conservative_deployed_cost, report.conservative_realized_pnl
    );
    Ok(())
}
