//! Emit a machine-readable snapshot of usage-limit thresholds to a sidecar
//! JSON file (`~/.claude/ccline/.limits_state.json`) on every successful
//! render. External tooling — typically a Claude Code `SessionStart` hook —
//! reads it and decides whether to warn the user, switch model, etc.
//!
//! Levels are derived from the same `Threshold` config that drives the
//! visible color override in the status line (T03), so the file is a true
//! reflection of what the user sees.
//!
//! Layered for testability:
//! - [`compute_level`] — pure, takes percent + thresholds, returns [`Level`].
//! - [`build_state`] — pure, takes collected segment data + config, returns
//!   [`LimitsState`].
//! - [`write_state`] — I/O wrapper, atomic write via `.tmp` + rename.

use crate::config::{Config, SegmentConfig, SegmentId, Threshold};
use crate::core::segments::SegmentData;
use chrono::Utc;
use serde::Serialize;
use std::path::PathBuf;

/// Severity classification of a single percent reading against its configured
/// thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Warn,
    Critical,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Critical => "critical",
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct LimitWindow {
    pub percent: u8,
    pub level: Level,
}

#[derive(Debug, Serialize, Clone)]
pub struct LimitsState {
    pub updated_at: String,
    pub five_hour: Option<LimitWindow>,
    pub weekly: Option<LimitWindow>,
}

/// Classify a percent reading against the segment's threshold list.
///
/// - No thresholds → always `Ok`.
/// - Below the lowest threshold → `Ok`.
/// - Crossed the **highest** threshold → `Critical`.
/// - Crossed any other threshold but not the highest → `Warn`.
///
/// Robust to N thresholds (≥ 1). For the default 60/85 config this gives
/// the intuitive three bands.
pub fn compute_level(percent: f64, thresholds: &[Threshold]) -> Level {
    if thresholds.is_empty() {
        return Level::Ok;
    }
    let mut sorted: Vec<&Threshold> = thresholds.iter().collect();
    sorted.sort_by_key(|t| t.at);
    let highest = sorted.last().unwrap().at;
    if percent >= highest as f64 {
        Level::Critical
    } else if percent >= sorted.first().unwrap().at as f64 {
        Level::Warn
    } else {
        Level::Ok
    }
}

/// Build the limits-state record from the segments we've already collected
/// for this render. Pure.
pub fn build_state(
    config: &Config,
    segments_data: &[(SegmentConfig, SegmentData)],
    now_rfc3339: String,
) -> LimitsState {
    let mut five_hour = None;
    let mut weekly = None;
    for (sc, data) in segments_data {
        match sc.id {
            SegmentId::Usage => five_hour = build_window(sc, data),
            SegmentId::WeeklyUsage => weekly = build_window(sc, data),
            _ => {}
        }
    }
    // Honor config.segments option overrides even when a segment is disabled
    // — useful for hooks that want a snapshot regardless of visible UI.
    // (No-op in v1; placeholder for future "force_compute" option.)
    let _ = config;
    LimitsState {
        updated_at: now_rfc3339,
        five_hour,
        weekly,
    }
}

fn build_window(sc: &SegmentConfig, data: &SegmentData) -> Option<LimitWindow> {
    let percent = data.metadata.get("percent")?.parse::<f64>().ok()?;
    let thresholds = Threshold::list_from_options(&sc.options);
    let level = compute_level(percent, &thresholds);
    Some(LimitWindow {
        percent: percent.round().clamp(0.0, 255.0) as u8,
        level,
    })
}

/// Returns the worst level present in the state.
///
/// Used by the optional `exit_code_on_threshold` mode to map levels to
/// process exit codes (Ok → 0, Warn → 1, Critical → 2).
pub fn worst_level(state: &LimitsState) -> Level {
    let levels = [state.five_hour.as_ref(), state.weekly.as_ref()]
        .into_iter()
        .flatten()
        .map(|w| w.level);
    let mut worst = Level::Ok;
    for l in levels {
        worst = match (worst, l) {
            (Level::Critical, _) | (_, Level::Critical) => Level::Critical,
            (Level::Warn, _) | (_, Level::Warn) => Level::Warn,
            _ => Level::Ok,
        };
    }
    worst
}

fn get_state_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".claude")
            .join("ccline")
            .join(".limits_state.json"),
    )
}

/// Build the state from segment data + write atomically. Best-effort — I/O
/// errors are swallowed so a doomed write doesn't break the render that just
/// happened.
pub fn write_state(config: &Config, segments_data: &[(SegmentConfig, SegmentData)]) -> LimitsState {
    let state = build_state(config, segments_data, Utc::now().to_rfc3339());
    let Some(path) = get_state_path() else {
        return state;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnsiColor, ColorConfig, IconConfig, TextStyleConfig};
    use std::collections::HashMap;

    fn t(at: u8) -> Threshold {
        Threshold {
            at,
            color: AnsiColor::Color16 { c16: 0 },
        }
    }

    // ---- compute_level ----

    #[test]
    fn level_ok_when_no_thresholds() {
        assert_eq!(compute_level(99.0, &[]), Level::Ok);
    }

    #[test]
    fn level_ok_when_below_lowest() {
        assert_eq!(compute_level(50.0, &[t(60), t(85)]), Level::Ok);
        assert_eq!(compute_level(59.9, &[t(60), t(85)]), Level::Ok);
    }

    #[test]
    fn level_warn_when_between_thresholds() {
        assert_eq!(compute_level(60.0, &[t(60), t(85)]), Level::Warn);
        assert_eq!(compute_level(75.0, &[t(60), t(85)]), Level::Warn);
        assert_eq!(compute_level(84.9, &[t(60), t(85)]), Level::Warn);
    }

    #[test]
    fn level_critical_when_past_highest() {
        assert_eq!(compute_level(85.0, &[t(60), t(85)]), Level::Critical);
        assert_eq!(compute_level(99.0, &[t(60), t(85)]), Level::Critical);
    }

    #[test]
    fn level_single_threshold_skips_warn() {
        // With only one threshold, crossing it is "Critical" (it IS the highest).
        assert_eq!(compute_level(50.0, &[t(70)]), Level::Ok);
        assert_eq!(compute_level(70.0, &[t(70)]), Level::Critical);
    }

    #[test]
    fn level_unsorted_thresholds_still_classify_correctly() {
        assert_eq!(compute_level(75.0, &[t(85), t(60)]), Level::Warn);
        assert_eq!(compute_level(90.0, &[t(85), t(60)]), Level::Critical);
    }

    // ---- build_state ----

    fn synth_segment(
        id: SegmentId,
        percent: &str,
        thresholds: serde_json::Value,
    ) -> (SegmentConfig, SegmentData) {
        let mut opts = HashMap::new();
        opts.insert("thresholds".to_string(), thresholds);
        let cfg = SegmentConfig {
            id,
            enabled: true,
            icon: IconConfig {
                plain: "x".to_string(),
                nerd_font: "x".to_string(),
            },
            colors: ColorConfig {
                icon: None,
                text: None,
                background: None,
            },
            styles: TextStyleConfig::default(),
            options: opts,
        };
        let mut meta = HashMap::new();
        meta.insert("percent".to_string(), percent.to_string());
        let data = SegmentData {
            primary: format!("{}%", percent),
            secondary: String::new(),
            metadata: meta,
        };
        (cfg, data)
    }

    #[test]
    fn build_state_picks_usage_and_weekly_only() {
        let usage = synth_segment(
            SegmentId::Usage,
            "73",
            serde_json::json!([{ "at": 60, "color": { "c16": 3 } }, { "at": 85, "color": { "c16": 1 } }]),
        );
        let weekly = synth_segment(
            SegmentId::WeeklyUsage,
            "91",
            serde_json::json!([{ "at": 60, "color": { "c16": 3 } }, { "at": 85, "color": { "c16": 1 } }]),
        );
        let unrelated = synth_segment(SegmentId::ContextWindow, "20", serde_json::json!([]));
        let data = vec![usage, weekly, unrelated];
        let cfg = Config::default();
        let s = build_state(&cfg, &data, "now".to_string());
        assert_eq!(s.updated_at, "now");
        assert_eq!(s.five_hour.unwrap().level, Level::Warn);
        assert_eq!(s.weekly.unwrap().level, Level::Critical);
    }

    #[test]
    fn build_state_skips_segments_without_percent_metadata() {
        let cfg_only = synth_segment(SegmentId::Usage, "ignored", serde_json::json!([]));
        let (mut sc, mut data) = cfg_only;
        data.metadata.clear(); // strip percent
        let s = build_state(&Config::default(), &[(sc.clone(), data)], "now".into());
        assert!(s.five_hour.is_none());
        // Now restore percent but bad value
        sc.options.clear();
        let mut bad_data = SegmentData {
            primary: "".into(),
            secondary: "".into(),
            metadata: HashMap::new(),
        };
        bad_data
            .metadata
            .insert("percent".to_string(), "not a number".to_string());
        let s = build_state(&Config::default(), &[(sc, bad_data)], "now".into());
        assert!(s.five_hour.is_none());
    }

    #[test]
    fn build_state_handles_segment_with_no_thresholds_as_ok() {
        let usage = synth_segment(SegmentId::Usage, "99", serde_json::json!([]));
        let s = build_state(&Config::default(), &[usage], "now".into());
        let w = s.five_hour.unwrap();
        assert_eq!(w.percent, 99);
        assert_eq!(w.level, Level::Ok);
    }

    // ---- worst_level ----

    #[test]
    fn worst_level_is_critical_when_any() {
        let s = LimitsState {
            updated_at: "now".into(),
            five_hour: Some(LimitWindow {
                percent: 30,
                level: Level::Ok,
            }),
            weekly: Some(LimitWindow {
                percent: 90,
                level: Level::Critical,
            }),
        };
        assert_eq!(worst_level(&s), Level::Critical);
    }

    #[test]
    fn worst_level_is_warn_when_any_warn_no_critical() {
        let s = LimitsState {
            updated_at: "now".into(),
            five_hour: Some(LimitWindow {
                percent: 70,
                level: Level::Warn,
            }),
            weekly: Some(LimitWindow {
                percent: 30,
                level: Level::Ok,
            }),
        };
        assert_eq!(worst_level(&s), Level::Warn);
    }

    #[test]
    fn worst_level_is_ok_when_no_windows() {
        let s = LimitsState {
            updated_at: "now".into(),
            five_hour: None,
            weekly: None,
        };
        assert_eq!(worst_level(&s), Level::Ok);
    }

    // ---- JSON shape ----

    #[test]
    fn limits_state_serializes_with_lowercase_level() {
        let s = LimitsState {
            updated_at: "2026-05-13T20:14:32Z".into(),
            five_hour: Some(LimitWindow {
                percent: 73,
                level: Level::Warn,
            }),
            weekly: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"level\":\"warn\""));
        assert!(json.contains("\"percent\":73"));
        assert!(json.contains("\"weekly\":null"));
    }
}
