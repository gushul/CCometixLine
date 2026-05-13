//! Append-only JSONL history of API usage snapshots.
//!
//! Each successful refresh of the OAuth usage cache appends one line:
//!
//! ```text
//! {"t":"2026-05-13T15:42:00Z","five_hour":23.0,"weekly":67.0}
//! ```
//!
//! Layout:
//! - File at `~/.claude/ccline/usage_history.jsonl`.
//! - Append-only on each refresh (cheap O(1)).
//! - Periodically pruned: on append, if the file is larger than
//!   [`PRUNE_TRIGGER_BYTES`], entries older than [`RETENTION_DAYS`] are
//!   filtered out.
//!
//! This module exposes pure helpers (`parse_history_jsonl`, `aggregate`,
//! `format_stats`) plus thin I/O wrappers. Tests cover the pure side.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

pub const RETENTION_DAYS: i64 = 90;
pub const PRUNE_TRIGGER_BYTES: u64 = 1_000_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryEntry {
    /// RFC3339 timestamp of when this snapshot was captured.
    pub t: String,
    /// Five-hour utilization percent (0..=100) at that time.
    pub five_hour: f64,
    /// Weekly utilization percent (0..=100) at that time.
    pub weekly: f64,
}

impl HistoryEntry {
    pub fn timestamp_utc(&self) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(&self.t)
            .ok()
            .map(|t| t.with_timezone(&Utc))
    }
}

/// Window of history to summarize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    Day,
    Week,
    Month,
}

impl Window {
    pub fn from_option_str(s: &str) -> Option<Self> {
        match s {
            "day" => Some(Window::Day),
            "week" => Some(Window::Week),
            "month" => Some(Window::Month),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Window::Day => "day",
            Window::Week => "week",
            Window::Month => "month",
        }
    }

    pub fn duration(&self) -> Duration {
        match self {
            Window::Day => Duration::days(1),
            Window::Week => Duration::days(7),
            Window::Month => Duration::days(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub window: Window,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub samples: usize,
    pub five_hour_avg: f64,
    pub five_hour_max: f64,
    pub weekly_avg: f64,
    pub weekly_max: f64,
    pub current_five_hour: Option<f64>,
    pub current_weekly: Option<f64>,
}

/// Parse JSONL history body. Malformed lines silently skipped — permissive
/// against future-schema drift.
pub fn parse_history_jsonl(content: &str) -> Vec<HistoryEntry> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<HistoryEntry>(trimmed).ok()
        })
        .collect()
}

/// Summarize entries inside `[now - window, now]`. Returns `samples: 0` and
/// NaN-safe zero defaults when the window has no entries.
pub fn aggregate(entries: &[HistoryEntry], window: Window, now: DateTime<Utc>) -> Stats {
    let cutoff = now - window.duration();
    let in_window: Vec<&HistoryEntry> = entries
        .iter()
        .filter(|e| match e.timestamp_utc() {
            Some(t) => t >= cutoff && t <= now,
            None => false,
        })
        .collect();

    if in_window.is_empty() {
        return Stats {
            window,
            from: cutoff,
            to: now,
            samples: 0,
            five_hour_avg: 0.0,
            five_hour_max: 0.0,
            weekly_avg: 0.0,
            weekly_max: 0.0,
            current_five_hour: None,
            current_weekly: None,
        };
    }

    let samples = in_window.len();
    let five_hour_sum: f64 = in_window.iter().map(|e| e.five_hour).sum();
    let weekly_sum: f64 = in_window.iter().map(|e| e.weekly).sum();
    let five_hour_max = in_window
        .iter()
        .map(|e| e.five_hour)
        .fold(f64::NEG_INFINITY, f64::max);
    let weekly_max = in_window
        .iter()
        .map(|e| e.weekly)
        .fold(f64::NEG_INFINITY, f64::max);

    // "Current" = the most recent entry in the window.
    let latest = in_window
        .iter()
        .max_by(|a, b| {
            a.timestamp_utc()
                .unwrap_or(cutoff)
                .cmp(&b.timestamp_utc().unwrap_or(cutoff))
        })
        .copied();

    Stats {
        window,
        from: cutoff,
        to: now,
        samples,
        five_hour_avg: five_hour_sum / samples as f64,
        five_hour_max,
        weekly_avg: weekly_sum / samples as f64,
        weekly_max,
        current_five_hour: latest.map(|e| e.five_hour),
        current_weekly: latest.map(|e| e.weekly),
    }
}

/// Render the summary as a plain-text columnar block suitable for printing
/// to a terminal.
pub fn format_stats_plain(stats: &Stats) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Window: last {} ({} → {})\n",
        stats.window.label(),
        stats.from.format("%Y-%m-%d"),
        stats.to.format("%Y-%m-%d")
    ));
    out.push_str(&format!("Samples: {}\n", stats.samples));
    if stats.samples == 0 {
        out.push_str(
            "No data in window. Try a wider window or wait for refresh to populate history.\n",
        );
        return out;
    }
    out.push_str(&format!(
        "5-hour:  avg {:>5.1}%   max {:>5.1}%   current {}\n",
        stats.five_hour_avg,
        stats.five_hour_max,
        format_optional_pct(stats.current_five_hour),
    ));
    out.push_str(&format!(
        "Weekly:  avg {:>5.1}%   max {:>5.1}%   current {}\n",
        stats.weekly_avg,
        stats.weekly_max,
        format_optional_pct(stats.current_weekly),
    ));
    out
}

fn format_optional_pct(p: Option<f64>) -> String {
    p.map(|v| format!("{:>5.1}%", v))
        .unwrap_or_else(|| "  —".to_string())
}

/// Render the summary as one JSON object (single line, no trailing newline).
pub fn format_stats_json(stats: &Stats) -> String {
    serde_json::json!({
        "window": stats.window.label(),
        "from": stats.from.to_rfc3339(),
        "to": stats.to.to_rfc3339(),
        "samples": stats.samples,
        "five_hour": {
            "avg": stats.five_hour_avg,
            "max": stats.five_hour_max,
            "current": stats.current_five_hour,
        },
        "weekly": {
            "avg": stats.weekly_avg,
            "max": stats.weekly_max,
            "current": stats.current_weekly,
        },
    })
    .to_string()
}

// ---- I/O wrappers (filesystem-side, not unit-tested) ----

pub fn get_history_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".claude")
            .join("ccline")
            .join("usage_history.jsonl"),
    )
}

/// Append a snapshot to history. Best-effort: I/O errors are swallowed (this
/// is called from the refresh subprocess; we don't want a doomed write to
/// cascade into broken renders). When the file is larger than
/// [`PRUNE_TRIGGER_BYTES`], entries older than [`RETENTION_DAYS`] are
/// rewritten out atomically before appending.
pub fn append(entry: &HistoryEntry) {
    let Some(path) = get_history_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Cheap pruning trigger: only when the file is already heavy.
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > PRUNE_TRIGGER_BYTES {
        prune(&path);
    }

    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| writeln!(f, "{}", line));
    let _ = result;
}

fn prune(path: &std::path::Path) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let cutoff = Utc::now() - Duration::days(RETENTION_DAYS);
    let kept: Vec<String> = content
        .lines()
        .filter(|line| {
            let entry: Result<HistoryEntry, _> = serde_json::from_str(line);
            match entry {
                Ok(e) => e.timestamp_utc().map(|t| t >= cutoff).unwrap_or(false),
                Err(_) => false,
            }
        })
        .map(String::from)
        .collect();

    let tmp = path.with_extension("jsonl.tmp");
    if std::fs::write(&tmp, kept.join("\n") + "\n").is_ok() {
        let _ = std::fs::rename(&tmp, path);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Read all history entries from disk. Returns an empty vec on any error.
pub fn load_all() -> Vec<HistoryEntry> {
    let Some(path) = get_history_path() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_history_jsonl(&content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap()
    }

    fn entry(t: DateTime<Utc>, five: f64, weekly: f64) -> HistoryEntry {
        HistoryEntry {
            t: t.to_rfc3339(),
            five_hour: five,
            weekly,
        }
    }

    // ---- parse_history_jsonl ----

    #[test]
    fn parse_empty_returns_empty() {
        assert_eq!(parse_history_jsonl("").len(), 0);
        assert_eq!(parse_history_jsonl("\n\n\n").len(), 0);
    }

    #[test]
    fn parse_valid_lines() {
        let jsonl = r#"{"t":"2026-05-13T12:00:00Z","five_hour":23.0,"weekly":67.0}
{"t":"2026-05-13T13:00:00Z","five_hour":25.0,"weekly":68.0}"#;
        let entries = parse_history_jsonl(jsonl);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].five_hour, 23.0);
        assert_eq!(entries[1].weekly, 68.0);
    }

    #[test]
    fn parse_skips_malformed_lines() {
        let jsonl = "not json\n{\"t\":\"2026-05-13T12:00:00Z\",\"five_hour\":23.0,\"weekly\":67.0}\n{\"incomplete\":\"x\"}\n";
        let entries = parse_history_jsonl(jsonl);
        assert_eq!(entries.len(), 1);
    }

    // ---- aggregate ----

    #[test]
    fn aggregate_empty_returns_zero_samples() {
        let stats = aggregate(&[], Window::Week, at(2026, 5, 13, 12));
        assert_eq!(stats.samples, 0);
        assert_eq!(stats.five_hour_avg, 0.0);
        assert!(stats.current_five_hour.is_none());
    }

    #[test]
    fn aggregate_picks_entries_inside_window() {
        let now = at(2026, 5, 13, 12);
        let entries = vec![
            entry(now - Duration::days(10), 10.0, 10.0), // outside week
            entry(now - Duration::days(3), 20.0, 50.0),  // inside
            entry(now - Duration::hours(2), 30.0, 60.0), // inside, current
        ];
        let stats = aggregate(&entries, Window::Week, now);
        assert_eq!(stats.samples, 2);
        assert!((stats.five_hour_avg - 25.0).abs() < 0.01);
        assert_eq!(stats.five_hour_max, 30.0);
        assert_eq!(stats.weekly_max, 60.0);
        assert_eq!(stats.current_five_hour, Some(30.0));
        assert_eq!(stats.current_weekly, Some(60.0));
    }

    #[test]
    fn aggregate_day_window_excludes_yesterday() {
        let now = at(2026, 5, 13, 12);
        let entries = vec![
            entry(now - Duration::hours(36), 50.0, 50.0), // outside day
            entry(now - Duration::hours(12), 20.0, 60.0), // inside
        ];
        let stats = aggregate(&entries, Window::Day, now);
        assert_eq!(stats.samples, 1);
        assert_eq!(stats.five_hour_avg, 20.0);
    }

    #[test]
    fn aggregate_excludes_entries_with_malformed_timestamps() {
        let now = at(2026, 5, 13, 12);
        let mut bad = entry(now - Duration::hours(2), 20.0, 50.0);
        bad.t = "not-a-timestamp".to_string();
        let entries = vec![bad, entry(now - Duration::hours(1), 30.0, 60.0)];
        let stats = aggregate(&entries, Window::Week, now);
        assert_eq!(stats.samples, 1);
        assert_eq!(stats.five_hour_max, 30.0);
    }

    // ---- Window ----

    #[test]
    fn window_round_trips_option_string() {
        assert_eq!(Window::from_option_str("day"), Some(Window::Day));
        assert_eq!(Window::from_option_str("week"), Some(Window::Week));
        assert_eq!(Window::from_option_str("month"), Some(Window::Month));
        assert_eq!(Window::from_option_str("year"), None);
    }

    // ---- format_stats_plain ----

    #[test]
    fn plain_format_shows_no_data_message_when_empty() {
        let stats = aggregate(&[], Window::Week, at(2026, 5, 13, 12));
        let out = format_stats_plain(&stats);
        assert!(out.contains("Samples: 0"));
        assert!(out.contains("No data"));
    }

    #[test]
    fn plain_format_includes_window_label_and_dates() {
        let now = at(2026, 5, 13, 12);
        let entries = vec![entry(now - Duration::hours(2), 23.0, 67.0)];
        let stats = aggregate(&entries, Window::Week, now);
        let out = format_stats_plain(&stats);
        assert!(out.contains("last week"));
        assert!(out.contains("2026-05-06"));
        assert!(out.contains("2026-05-13"));
        assert!(out.contains("Samples: 1"));
        assert!(out.contains("23.0%"));
        assert!(out.contains("67.0%"));
    }

    // ---- format_stats_json ----

    #[test]
    fn json_format_is_valid_json() {
        let now = at(2026, 5, 13, 12);
        let entries = vec![entry(now - Duration::hours(2), 23.5, 67.8)];
        let stats = aggregate(&entries, Window::Day, now);
        let out = format_stats_json(&stats);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(parsed["window"], "day");
        assert_eq!(parsed["samples"], 1);
        assert!((parsed["five_hour"]["avg"].as_f64().unwrap() - 23.5).abs() < 0.01);
        assert_eq!(parsed["weekly"]["current"], 67.8);
    }

    #[test]
    fn json_format_empty_window_has_null_currents() {
        let stats = aggregate(&[], Window::Week, at(2026, 5, 13, 12));
        let out = format_stats_json(&stats);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed["five_hour"]["current"].is_null());
        assert!(parsed["weekly"]["current"].is_null());
    }
}
