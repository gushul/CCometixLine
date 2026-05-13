//! Projected-exhaust segment — given two cache snapshots of the 5-hour
//! utilization at different times, project when the window will fill up.
//!
//! The maths is simple and intentionally honest about its assumptions:
//!
//! ```text
//! util_rate = (current_util - previous_util) / (now - previous_cached_at)
//! minutes_to_exhaust = (100 - current_util) / util_rate
//! ```
//!
//! If the projection extends past the next window reset, the segment reports
//! "after reset" rather than a nonsense future time — the user won't hit the
//! limit before the window flips.
//!
//! Implementation is layered for unit testing:
//!
//! - [`compute_projected_exhaust`] — pure, takes raw numbers + timestamps,
//!   returns [`ExhaustOutcome`]. No I/O.
//! - [`format_exhaust_display`] — pure, renders the outcome into a status-line
//!   string in either duration or clock form.
//! - [`ProjectedExhaustSegment`] — glue: pulls a [`UsageSnapshot`] from
//!   `usage_api`, calls the pure helpers, returns `SegmentData`.

use super::usage_api;
use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId};
use chrono::{DateTime, Duration, Local, Timelike, Utc};
use std::collections::HashMap;

/// History must span at least this many seconds for a projection to be
/// meaningful. Short windows give wildly noisy rates.
pub const DEFAULT_MIN_HISTORY_SECONDS: i64 = 300;

/// Result of the projection.
#[derive(Debug, Clone, PartialEq)]
pub enum ExhaustOutcome {
    /// Approximate time-to-exhaust in fractional minutes.
    InMinutes(f64),
    /// Projection extends past the next window reset — user won't hit the
    /// limit in this window.
    AfterReset,
    /// Not enough data to project (insufficient cache history, or utilization
    /// declined between snapshots — typically a window-reset boundary).
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// `"~38m"`, `"~2h15m"` — relative.
    Duration,
    /// `"@16:42"` — absolute local-time clock.
    Clock,
}

impl DisplayMode {
    fn from_option_str(s: &str) -> Self {
        match s {
            "clock" => DisplayMode::Clock,
            _ => DisplayMode::Duration,
        }
    }
}

/// Pure compute. See module-level docs for the formula.
///
/// - `current_util` — current 5h utilization in percent (0..=100).
/// - `prev_util` — utilization at `prev_at`. `None` ⇒ no history.
/// - `prev_at` — RFC3339 timestamp of the prior snapshot. `None` ⇒ no history.
/// - `now` — wall clock.
/// - `reset_at` — RFC3339 timestamp of the upcoming window reset. Used only
///   to clamp the projection.
/// - `min_history_seconds` — refuse to project on history shorter than this.
pub fn compute_projected_exhaust(
    current_util: f64,
    prev_util: Option<f64>,
    prev_at: Option<&str>,
    now: DateTime<Utc>,
    reset_at: Option<&str>,
    min_history_seconds: i64,
) -> ExhaustOutcome {
    let prev_util = match prev_util {
        Some(v) => v,
        None => return ExhaustOutcome::Unknown,
    };
    let prev_at_dt = match prev_at.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
        Some(t) => t.with_timezone(&Utc),
        None => return ExhaustOutcome::Unknown,
    };

    let elapsed = now.signed_duration_since(prev_at_dt);
    if elapsed.num_seconds() < min_history_seconds {
        return ExhaustOutcome::Unknown;
    }

    let delta_util = current_util - prev_util;
    if delta_util <= 0.0 {
        // Either flat (user idle) or negative (window reset between snapshots).
        // Either way, no usable rate.
        return ExhaustOutcome::Unknown;
    }

    let minutes = elapsed.num_seconds() as f64 / 60.0;
    let util_per_min = delta_util / minutes;

    let remaining_pct = 100.0 - current_util;
    if remaining_pct <= 0.0 {
        return ExhaustOutcome::InMinutes(0.0);
    }
    let minutes_to_exhaust = remaining_pct / util_per_min;

    if let Some(reset_dt) = reset_at.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
        let minutes_to_reset = reset_dt
            .with_timezone(&Utc)
            .signed_duration_since(now)
            .num_seconds() as f64
            / 60.0;
        if minutes_to_reset > 0.0 && minutes_to_exhaust > minutes_to_reset {
            return ExhaustOutcome::AfterReset;
        }
    }

    ExhaustOutcome::InMinutes(minutes_to_exhaust)
}

/// Render the outcome for the status line. `now` is needed only in clock mode
/// to convert "duration from now" into an absolute local-time string.
pub fn format_exhaust_display(
    outcome: &ExhaustOutcome,
    mode: DisplayMode,
    now: DateTime<Utc>,
) -> String {
    match outcome {
        ExhaustOutcome::Unknown => "—".to_string(),
        ExhaustOutcome::AfterReset => "after reset".to_string(),
        ExhaustOutcome::InMinutes(m) => match mode {
            DisplayMode::Duration => format_minutes(*m),
            DisplayMode::Clock => {
                let when = now + Duration::seconds((*m * 60.0) as i64);
                let local = when.with_timezone(&Local);
                format!("@{:02}:{:02}", local.hour(), local.minute())
            }
        },
    }
}

fn format_minutes(m: f64) -> String {
    if m < 1.0 {
        return "<1m".to_string();
    }
    let total = m.round() as i64;
    if total < 60 {
        format!("~{}m", total)
    } else {
        let hours = total / 60;
        let mins = total % 60;
        if mins == 0 {
            format!("~{}h", hours)
        } else {
            format!("~{}h{}m", hours, mins)
        }
    }
}

#[derive(Default)]
pub struct ProjectedExhaustSegment;

impl ProjectedExhaustSegment {
    pub fn new() -> Self {
        Self
    }
}

impl Segment for ProjectedExhaustSegment {
    fn collect(&self, _input: &InputData) -> Option<SegmentData> {
        let config = crate::config::Config::load().ok()?;
        let opts = config
            .segments
            .iter()
            .find(|s| s.id == SegmentId::ProjectedExhaust);

        let mode = opts
            .and_then(|sc| sc.options.get("format"))
            .and_then(|v| v.as_str())
            .map(DisplayMode::from_option_str)
            .unwrap_or(DisplayMode::Duration);
        let min_history_seconds = opts
            .and_then(|sc| sc.options.get("min_history_seconds"))
            .and_then(|v| v.as_i64())
            .unwrap_or(DEFAULT_MIN_HISTORY_SECONDS);

        let snapshot = usage_api::fetch_or_cached(SegmentId::ProjectedExhaust)?;
        let now = Utc::now();
        let outcome = compute_projected_exhaust(
            snapshot.five_hour_utilization,
            snapshot.previous_five_hour_utilization,
            snapshot.previous_cached_at.as_deref(),
            now,
            snapshot.five_hour_resets_at.as_deref(),
            min_history_seconds,
        );

        let primary = format_exhaust_display(&outcome, mode, now);

        let mut metadata = HashMap::new();
        if let ExhaustOutcome::InMinutes(m) = &outcome {
            metadata.insert("minutes_to_exhaust".to_string(), m.to_string());
        }
        metadata.insert(
            "outcome".to_string(),
            match outcome {
                ExhaustOutcome::Unknown => "unknown".to_string(),
                ExhaustOutcome::AfterReset => "after_reset".to_string(),
                ExhaustOutcome::InMinutes(_) => "minutes".to_string(),
            },
        );

        Some(SegmentData {
            primary,
            secondary: String::new(),
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::ProjectedExhaust
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 13, 12, 0, 0).unwrap()
    }

    fn rfc3339(dt: DateTime<Utc>) -> String {
        dt.to_rfc3339()
    }

    // ---------- compute_projected_exhaust ----------

    #[test]
    fn unknown_when_no_previous_snapshot() {
        let o = compute_projected_exhaust(30.0, None, None, now(), None, 300);
        assert_eq!(o, ExhaustOutcome::Unknown);
    }

    #[test]
    fn unknown_when_history_too_short() {
        // Prev was 60s ago — below default 300s threshold.
        let prev = now() - Duration::seconds(60);
        let o = compute_projected_exhaust(30.0, Some(28.0), Some(&rfc3339(prev)), now(), None, 300);
        assert_eq!(o, ExhaustOutcome::Unknown);
    }

    #[test]
    fn unknown_when_utilization_did_not_grow() {
        // Flat: 28 -> 28
        let prev = now() - Duration::seconds(600);
        assert_eq!(
            compute_projected_exhaust(28.0, Some(28.0), Some(&rfc3339(prev)), now(), None, 300),
            ExhaustOutcome::Unknown
        );
        // Declined (window reset between snapshots)
        assert_eq!(
            compute_projected_exhaust(5.0, Some(80.0), Some(&rfc3339(prev)), now(), None, 300),
            ExhaustOutcome::Unknown
        );
    }

    #[test]
    fn projects_minutes_at_steady_rate() {
        // 600s elapsed, util went 20 -> 25 (5 pp over 10 min ⇒ 0.5 pp/min).
        // Remaining 75 pp ⇒ ~150 min.
        let prev = now() - Duration::seconds(600);
        let o = compute_projected_exhaust(25.0, Some(20.0), Some(&rfc3339(prev)), now(), None, 300);
        match o {
            ExhaustOutcome::InMinutes(m) => assert!((m - 150.0).abs() < 0.5, "got {}", m),
            other => panic!("expected InMinutes, got {:?}", other),
        }
    }

    #[test]
    fn clamps_to_after_reset_when_projection_exceeds_window() {
        // Slow burn — exhaust would land in ~150 min, but reset in 30 min.
        let prev = now() - Duration::seconds(600);
        let reset = now() + Duration::seconds(30 * 60);
        let o = compute_projected_exhaust(
            25.0,
            Some(20.0),
            Some(&rfc3339(prev)),
            now(),
            Some(&rfc3339(reset)),
            300,
        );
        assert_eq!(o, ExhaustOutcome::AfterReset);
    }

    #[test]
    fn projection_inside_window_returns_minutes() {
        // Fast burn — exhaust in ~10 min, reset in 60 min. Should report minutes.
        let prev = now() - Duration::seconds(600);
        let reset = now() + Duration::seconds(60 * 60);
        let o = compute_projected_exhaust(
            90.0,
            Some(30.0),
            Some(&rfc3339(prev)),
            now(),
            Some(&rfc3339(reset)),
            300,
        );
        match o {
            ExhaustOutcome::InMinutes(m) => {
                // 60 pp / 10 min = 6 pp/min. Remaining 10 pp. = ~1.67 min.
                assert!(m > 0.0 && m < 5.0, "expected small minutes, got {}", m);
            }
            other => panic!("expected InMinutes, got {:?}", other),
        }
    }

    #[test]
    fn util_already_full_returns_zero_minutes() {
        let prev = now() - Duration::seconds(600);
        assert_eq!(
            compute_projected_exhaust(100.0, Some(80.0), Some(&rfc3339(prev)), now(), None, 300),
            ExhaustOutcome::InMinutes(0.0)
        );
    }

    #[test]
    fn malformed_prev_timestamp_returns_unknown() {
        let o =
            compute_projected_exhaust(30.0, Some(20.0), Some("not-a-timestamp"), now(), None, 300);
        assert_eq!(o, ExhaustOutcome::Unknown);
    }

    // ---------- format_exhaust_display ----------

    #[test]
    fn display_unknown_is_em_dash() {
        assert_eq!(
            format_exhaust_display(&ExhaustOutcome::Unknown, DisplayMode::Duration, now()),
            "—"
        );
    }

    #[test]
    fn display_after_reset_string() {
        assert_eq!(
            format_exhaust_display(&ExhaustOutcome::AfterReset, DisplayMode::Duration, now()),
            "after reset"
        );
    }

    #[test]
    fn display_duration_under_an_hour() {
        assert_eq!(
            format_exhaust_display(
                &ExhaustOutcome::InMinutes(38.0),
                DisplayMode::Duration,
                now()
            ),
            "~38m"
        );
    }

    #[test]
    fn display_duration_above_an_hour() {
        assert_eq!(
            format_exhaust_display(
                &ExhaustOutcome::InMinutes(125.0),
                DisplayMode::Duration,
                now()
            ),
            "~2h5m"
        );
        assert_eq!(
            format_exhaust_display(
                &ExhaustOutcome::InMinutes(120.0),
                DisplayMode::Duration,
                now()
            ),
            "~2h"
        );
    }

    #[test]
    fn display_duration_under_a_minute_shows_lt1m() {
        assert_eq!(
            format_exhaust_display(
                &ExhaustOutcome::InMinutes(0.3),
                DisplayMode::Duration,
                now()
            ),
            "<1m"
        );
    }

    #[test]
    fn display_clock_mode_returns_at_hh_mm_shape() {
        // We don't pin the exact hour (depends on local TZ) but the shape
        // should be `@HH:MM`.
        let s = format_exhaust_display(&ExhaustOutcome::InMinutes(42.0), DisplayMode::Clock, now());
        assert!(s.starts_with('@'), "{:?}", s);
        let body = &s[1..];
        let parts: Vec<&str> = body.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].parse::<u32>().is_ok());
        assert!(parts[1].parse::<u32>().is_ok());
    }

    #[test]
    fn display_mode_from_option_string() {
        assert_eq!(DisplayMode::from_option_str("clock"), DisplayMode::Clock);
        assert_eq!(
            DisplayMode::from_option_str("duration"),
            DisplayMode::Duration
        );
        // Anything unknown falls back to duration.
        assert_eq!(
            DisplayMode::from_option_str("anything-else"),
            DisplayMode::Duration
        );
    }
}
