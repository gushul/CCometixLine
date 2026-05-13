//! Burn-rate segment — derives a tokens-per-minute estimate from recent
//! transcript turns. Pure compute, no network. The data source is the
//! `transcript_path` Claude Code already pipes in via `InputData`.
//!
//! Layered for testability:
//!
//! - [`compute_burn_rate`] — pure, takes samples + window thresholds, returns
//!   `Option<tokens_per_minute>`. All sliding-window logic lives here.
//! - [`parse_burn_samples_from_jsonl`] — pure, takes JSONL text, returns
//!   `Vec<BurnSample>` for entries that have both a timestamp and a token
//!   count. Used by the segment after reading the file.
//! - [`format_burn_rate_display`] — pure, takes a rate, returns the
//!   user-facing string (e.g. `"42k/m"`).
//!
//! [`BurnRateSegment`] is a thin shim that wires these together.

use super::{Segment, SegmentData};
use crate::config::{InputData, RawUsage, SegmentId, TranscriptEntry};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::path::Path;

/// Default sliding window (15 min): tokens generated in the last 15 minutes
/// drive the rate. Larger windows smooth out spikes but lag user behavior.
pub const DEFAULT_WINDOW_SECONDS: i64 = 900;

/// Don't trust a rate computed from less than 5 minutes of data — too noisy
/// at the start of a session.
pub const DEFAULT_MIN_DATA_SECONDS: i64 = 300;

/// Fewer than 3 turns is also too thin to extrapolate from.
pub const DEFAULT_MIN_SAMPLES: usize = 3;

/// Which subset of per-turn token counts feeds the burn rate.
///
/// `InputOutput` (the default) approximates real quota consumption: Anthropic's
/// cache reads are very cheap and don't drive the 5h limit, so summing them
/// inflates the rate by 1-2 orders of magnitude. `OutputOnly` is the strictest
/// — closest to what actually moves utilization in practice. `Total` preserves
/// the pre-T11 behavior (input + output + cache_creation + cache_read) for
/// users who want it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TokenBasis {
    #[default]
    InputOutput,
    OutputOnly,
    Total,
}

impl TokenBasis {
    pub fn from_option_str(s: &str) -> Self {
        match s {
            "output_only" | "output" => TokenBasis::OutputOnly,
            "total" => TokenBasis::Total,
            _ => TokenBasis::InputOutput,
        }
    }

    fn tokens_from(&self, normalized: &crate::config::NormalizedUsage) -> u32 {
        match self {
            TokenBasis::InputOutput => normalized.input_tokens + normalized.output_tokens,
            TokenBasis::OutputOnly => normalized.output_tokens,
            TokenBasis::Total => normalized.total_for_cost(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BurnSample {
    pub at: DateTime<Utc>,
    pub tokens: u32,
}

/// Pure compute. Returns `Some(tokens_per_minute)` when the recent samples
/// cover at least `min_data_seconds` of wall-clock time AND contain at least
/// `min_samples` entries inside the sliding `window_seconds`. Returns `None`
/// when either threshold isn't met — segment renders `"—"` in that case.
pub fn compute_burn_rate(
    samples: &[BurnSample],
    now: DateTime<Utc>,
    window_seconds: i64,
    min_samples: usize,
    min_data_seconds: i64,
) -> Option<f64> {
    let cutoff = now - Duration::seconds(window_seconds);
    let in_window: Vec<&BurnSample> = samples
        .iter()
        .filter(|s| s.at >= cutoff && s.at <= now)
        .collect();

    if in_window.len() < min_samples {
        return None;
    }

    let earliest = in_window.iter().map(|s| s.at).min()?;
    let elapsed = now.signed_duration_since(earliest);
    if elapsed.num_seconds() < min_data_seconds {
        return None;
    }

    let total: u64 = in_window.iter().map(|s| s.tokens as u64).sum();
    let minutes = elapsed.num_seconds() as f64 / 60.0;
    Some(total as f64 / minutes)
}

/// Render the rate as a compact display string for the status line. Numbers
/// >= 1000 collapse to `Xk/m` form to keep the segment narrow.
pub fn format_burn_rate_display(tokens_per_min: f64) -> String {
    if tokens_per_min >= 1000.0 {
        let k = tokens_per_min / 1000.0;
        if k >= 10.0 {
            format!("{:.0}k/m", k)
        } else {
            format!("{:.1}k/m", k)
        }
    } else {
        format!("{}/m", tokens_per_min.round() as u64)
    }
}

/// Parse a JSONL transcript body into burn samples. Entries without a
/// timestamp or without normalizable usage are silently dropped — we want to
/// be permissive against schema drift, not panic.
///
/// Uses the default [`TokenBasis::InputOutput`]; callers wanting a different
/// basis use [`parse_burn_samples_from_jsonl_with`].
pub fn parse_burn_samples_from_jsonl(content: &str) -> Vec<BurnSample> {
    parse_burn_samples_from_jsonl_with(content, TokenBasis::default())
}

pub fn parse_burn_samples_from_jsonl_with(content: &str, basis: TokenBasis) -> Vec<BurnSample> {
    content
        .lines()
        .filter_map(|line| {
            let entry: TranscriptEntry = serde_json::from_str(line).ok()?;
            let at = entry
                .timestamp
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())?
                .with_timezone(&Utc);
            let usage: RawUsage = entry.message?.usage?;
            let tokens = basis.tokens_from(&usage.normalize());
            if tokens == 0 {
                return None;
            }
            Some(BurnSample { at, tokens })
        })
        .collect()
}

fn load_burn_samples(path: &Path, basis: TokenBasis) -> Vec<BurnSample> {
    std::fs::read_to_string(path)
        .map(|content| parse_burn_samples_from_jsonl_with(&content, basis))
        .unwrap_or_default()
}

#[derive(Default)]
pub struct BurnRateSegment;

impl BurnRateSegment {
    pub fn new() -> Self {
        Self
    }
}

impl Segment for BurnRateSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let config = crate::config::Config::load().ok()?;
        let opts = config.segments.iter().find(|s| s.id == SegmentId::BurnRate);

        let window_seconds = opts
            .and_then(|sc| sc.options.get("window_seconds"))
            .and_then(|v| v.as_i64())
            .unwrap_or(DEFAULT_WINDOW_SECONDS);
        let min_data_seconds = opts
            .and_then(|sc| sc.options.get("min_data_seconds"))
            .and_then(|v| v.as_i64())
            .unwrap_or(DEFAULT_MIN_DATA_SECONDS);
        let min_samples = opts
            .and_then(|sc| sc.options.get("min_samples"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_MIN_SAMPLES);
        let basis = opts
            .and_then(|sc| sc.options.get("token_basis"))
            .and_then(|v| v.as_str())
            .map(TokenBasis::from_option_str)
            .unwrap_or_default();

        let samples = load_burn_samples(Path::new(&input.transcript_path), basis);
        let rate = compute_burn_rate(
            &samples,
            Utc::now(),
            window_seconds,
            min_samples,
            min_data_seconds,
        );

        let primary = match rate {
            Some(r) => format_burn_rate_display(r),
            None => "—".to_string(),
        };

        let mut metadata = HashMap::new();
        if let Some(r) = rate {
            metadata.insert("tokens_per_minute".to_string(), r.to_string());
        }
        metadata.insert("samples".to_string(), samples.len().to_string());

        Some(SegmentData {
            primary,
            secondary: String::new(),
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::BurnRate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 13, 12, 0, 0).unwrap()
    }

    fn sample(seconds_ago: i64, tokens: u32) -> BurnSample {
        BurnSample {
            at: now() - Duration::seconds(seconds_ago),
            tokens,
        }
    }

    // ---------- compute_burn_rate ----------

    #[test]
    fn empty_samples_returns_none() {
        assert!(compute_burn_rate(&[], now(), 900, 3, 300).is_none());
    }

    #[test]
    fn fewer_than_min_samples_returns_none() {
        let samples = vec![sample(60, 1000), sample(120, 2000)];
        assert!(compute_burn_rate(&samples, now(), 900, 3, 300).is_none());
    }

    #[test]
    fn elapsed_below_min_data_returns_none() {
        // 3 samples but only 30s of data
        let samples = vec![sample(10, 1000), sample(20, 1000), sample(30, 1000)];
        assert!(compute_burn_rate(&samples, now(), 900, 3, 300).is_none());
    }

    #[test]
    fn computes_tokens_per_minute_for_clean_window() {
        // 3 samples spread 5 min back, 500 tokens each = 1500 tokens / 5 min = 300/min
        let samples = vec![sample(300, 500), sample(180, 500), sample(60, 500)];
        let rate = compute_burn_rate(&samples, now(), 900, 3, 300).expect("rate");
        assert!((rate - 300.0).abs() < 1.0, "got {}", rate);
    }

    #[test]
    fn window_excludes_samples_older_than_cutoff() {
        // 4 samples; window 60s catches only the two recent ones — below min_samples.
        let samples = vec![
            sample(3600, 9999), // 1h ago
            sample(1800, 9999), // 30m ago
            sample(30, 500),
            sample(15, 500),
        ];
        assert!(compute_burn_rate(&samples, now(), 60, 3, 30).is_none());
    }

    #[test]
    fn future_samples_excluded() {
        // Defensive: a sample with a future timestamp should not be counted.
        let future = BurnSample {
            at: now() + Duration::seconds(60),
            tokens: 9999,
        };
        let samples = vec![sample(300, 500), sample(180, 500), sample(60, 500), future];
        let rate = compute_burn_rate(&samples, now(), 900, 3, 300).expect("rate");
        // Same result as if future sample weren't there.
        assert!((rate - 300.0).abs() < 1.0, "got {}", rate);
    }

    // ---------- format_burn_rate_display ----------

    #[test]
    fn display_small_rates_show_per_minute() {
        assert_eq!(format_burn_rate_display(0.0), "0/m");
        assert_eq!(format_burn_rate_display(42.3), "42/m");
        assert_eq!(format_burn_rate_display(999.5), "1000/m");
    }

    #[test]
    fn display_kilo_rates_have_one_decimal() {
        assert_eq!(format_burn_rate_display(1234.0), "1.2k/m");
        assert_eq!(format_burn_rate_display(2500.0), "2.5k/m");
    }

    #[test]
    fn display_large_rates_drop_decimal() {
        assert_eq!(format_burn_rate_display(15_000.0), "15k/m");
        assert_eq!(format_burn_rate_display(100_500.0), "100k/m");
    }

    // ---------- parse_burn_samples_from_jsonl ----------

    #[test]
    fn parse_jsonl_extracts_timestamp_and_tokens() {
        let jsonl = r#"
{"type":"assistant","timestamp":"2026-05-13T11:55:00Z","message":{"usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-05-13T11:57:00Z","message":{"usage":{"input_tokens":200,"output_tokens":80}}}
"#;
        let samples = parse_burn_samples_from_jsonl(jsonl);
        assert_eq!(samples.len(), 2);
        // First sample: input(100) + output(50) = 150 via total_for_cost
        assert_eq!(samples[0].tokens, 150);
        assert_eq!(samples[1].tokens, 280);
    }

    #[test]
    fn parse_jsonl_skips_entries_without_timestamp() {
        let jsonl = r#"
{"type":"assistant","message":{"usage":{"input_tokens":100,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-05-13T11:57:00Z","message":{"usage":{"input_tokens":200,"output_tokens":80}}}
"#;
        let samples = parse_burn_samples_from_jsonl(jsonl);
        assert_eq!(samples.len(), 1);
    }

    #[test]
    fn parse_jsonl_skips_entries_with_zero_tokens() {
        let jsonl = r#"
{"type":"attachment","timestamp":"2026-05-13T11:55:00Z"}
{"type":"assistant","timestamp":"2026-05-13T11:57:00Z","message":{"usage":{"input_tokens":200,"output_tokens":80}}}
"#;
        let samples = parse_burn_samples_from_jsonl(jsonl);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].tokens, 280);
    }

    #[test]
    fn parse_jsonl_survives_malformed_lines() {
        let jsonl = "not json\n{\"type\":\"assistant\",\"timestamp\":\"2026-05-13T11:55:00Z\",\"message\":{\"usage\":{\"input_tokens\":100,\"output_tokens\":50}}}\n";
        let samples = parse_burn_samples_from_jsonl(jsonl);
        assert_eq!(samples.len(), 1);
    }

    #[test]
    fn parse_jsonl_empty_returns_empty() {
        assert_eq!(parse_burn_samples_from_jsonl("").len(), 0);
    }

    // ---------- T11: token_basis excludes cache reads ----------

    #[test]
    fn input_output_basis_excludes_cache_reads() {
        // Real-world shape: tiny actual I/O, huge cache hit.
        let jsonl = r#"{"type":"assistant","timestamp":"2026-05-13T11:55:00Z","message":{"usage":{"input_tokens":500,"output_tokens":500,"cache_read_input_tokens":100000}}}"#;
        let samples = parse_burn_samples_from_jsonl_with(jsonl, TokenBasis::InputOutput);
        assert_eq!(samples.len(), 1);
        // 500 + 500 = 1000. Cache read (100k) must NOT inflate this.
        assert_eq!(samples[0].tokens, 1000);
    }

    #[test]
    fn output_only_basis_picks_just_output() {
        let jsonl = r#"{"type":"assistant","timestamp":"2026-05-13T11:55:00Z","message":{"usage":{"input_tokens":500,"output_tokens":300,"cache_read_input_tokens":100000}}}"#;
        let samples = parse_burn_samples_from_jsonl_with(jsonl, TokenBasis::OutputOnly);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].tokens, 300);
    }

    #[test]
    fn total_basis_preserves_legacy_behavior() {
        let jsonl = r#"{"type":"assistant","timestamp":"2026-05-13T11:55:00Z","message":{"usage":{"input_tokens":500,"output_tokens":500,"cache_read_input_tokens":100000}}}"#;
        let samples = parse_burn_samples_from_jsonl_with(jsonl, TokenBasis::Total);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].tokens, 101_000);
    }

    #[test]
    fn token_basis_from_option_string_round_trips() {
        assert_eq!(
            TokenBasis::from_option_str("input_output"),
            TokenBasis::InputOutput
        );
        assert_eq!(
            TokenBasis::from_option_str("output_only"),
            TokenBasis::OutputOnly
        );
        assert_eq!(
            TokenBasis::from_option_str("output"),
            TokenBasis::OutputOnly
        );
        assert_eq!(TokenBasis::from_option_str("total"), TokenBasis::Total);
        // Anything unknown falls back to the (sensible) default.
        assert_eq!(
            TokenBasis::from_option_str("garbage"),
            TokenBasis::InputOutput
        );
    }
}
