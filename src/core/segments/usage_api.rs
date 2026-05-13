//! Shared Anthropic OAuth usage API helper used by both [`UsageSegment`] and
//! [`WeeklyUsageSegment`]. Centralizes the HTTP call, the on-disk cache, and
//! the pure formatting helpers.
//!
//! The cache file at `~/.claude/ccline/.api_usage_cache.json` is the
//! shared-state surface: whichever segment renders first on a cold cache pays
//! the HTTP cost, the other reads the freshly-written file. As long as both
//! segments use the same `cache_duration` option (default 180s), the cache
//! hit on the second segment is deterministic.
//!
//! [`UsageSegment`]: super::usage::UsageSegment
//! [`WeeklyUsageSegment`]: super::weekly_usage::WeeklyUsageSegment

use super::SegmentData;
use crate::config::SegmentId;
use crate::utils::credentials;
use chrono::{DateTime, Datelike, Duration, Local, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_API_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_CACHE_DURATION_SECS: u64 = 180;
pub const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 2;

/// Parsed response from `GET /api/oauth/usage`.
///
/// Only the two historically-known periods are named; everything else
/// (per-model weekly windows, internal feature-flag slots) falls through to
/// `extra` so feature work can introspect without a schema bump.
#[derive(Debug, Deserialize)]
pub struct ApiUsageResponse {
    pub five_hour: UsagePeriod,
    pub seven_day: UsagePeriod,
    #[serde(flatten)]
    #[allow(dead_code)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct UsagePeriod {
    pub utilization: f64,
    pub resets_at: Option<String>,
    #[serde(flatten)]
    #[allow(dead_code)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// On-disk cache shape. Backwards-compatible with the v1 layout (which only
/// had `resets_at`, populated from the 7d period). New `five_hour_resets_at`
/// is optional + defaulted so older cache files still parse.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiUsageCache {
    pub five_hour_utilization: f64,
    pub seven_day_utilization: f64,
    /// Historical name — was always populated from `seven_day.resets_at`.
    pub resets_at: Option<String>,
    #[serde(default)]
    pub five_hour_resets_at: Option<String>,
    pub cached_at: String,
}

impl ApiUsageCache {
    pub fn seven_day_resets_at(&self) -> Option<&str> {
        self.resets_at.as_deref()
    }
}

/// A snapshot of both utilization windows + their reset times, used by
/// segments to pick the value they want to render.
#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub five_hour_utilization: f64,
    pub five_hour_resets_at: Option<String>,
    pub seven_day_utilization: f64,
    pub seven_day_resets_at: Option<String>,
}

/// Fetch fresh data from the API or read it from the shared cache, whichever
/// is appropriate given the segment's `cache_duration` option.
///
/// `segment_id` is used only to look up the segment's options inside the
/// loaded Config — both `Usage` and `WeeklyUsage` are valid inputs.
pub fn fetch_or_cached(segment_id: SegmentId) -> Option<UsageSnapshot> {
    let token = credentials::get_oauth_token()?;
    let config = crate::config::Config::load().ok()?;
    let opts = config.segments.iter().find(|s| s.id == segment_id);

    let api_base_url = opts
        .and_then(|sc| sc.options.get("api_base_url"))
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_API_BASE_URL);

    let cache_duration = opts
        .and_then(|sc| sc.options.get("cache_duration"))
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_CACHE_DURATION_SECS);

    let timeout = opts
        .and_then(|sc| sc.options.get("timeout"))
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_HTTP_TIMEOUT_SECS);

    let cached = load_cache();
    let cache_is_hot = cached
        .as_ref()
        .map(|c| is_cache_valid(c, cache_duration))
        .unwrap_or(false);

    if cache_is_hot {
        let c = cached.unwrap();
        return Some(UsageSnapshot {
            five_hour_utilization: c.five_hour_utilization,
            five_hour_resets_at: c.five_hour_resets_at.clone(),
            seven_day_utilization: c.seven_day_utilization,
            seven_day_resets_at: c.resets_at.clone(),
        });
    }

    match fetch_api_usage(api_base_url, &token, timeout) {
        Some(response) => {
            let cache = ApiUsageCache {
                five_hour_utilization: response.five_hour.utilization,
                seven_day_utilization: response.seven_day.utilization,
                resets_at: response.seven_day.resets_at.clone(),
                five_hour_resets_at: response.five_hour.resets_at.clone(),
                cached_at: Utc::now().to_rfc3339(),
            };
            save_cache(&cache);
            Some(UsageSnapshot {
                five_hour_utilization: response.five_hour.utilization,
                five_hour_resets_at: response.five_hour.resets_at,
                seven_day_utilization: response.seven_day.utilization,
                seven_day_resets_at: response.seven_day.resets_at,
            })
        }
        None => cached.map(|c| UsageSnapshot {
            five_hour_utilization: c.five_hour_utilization,
            five_hour_resets_at: c.five_hour_resets_at.clone(),
            seven_day_utilization: c.seven_day_utilization,
            seven_day_resets_at: c.resets_at.clone(),
        }),
    }
}

/// Build the segment's primary/secondary/metadata from a single utilization
/// percent and its reset time. Both segments use the same shape; only the
/// caller's choice of which utilization to pass differs.
pub fn format_segment_data(util_percent: f64, resets_at: Option<&str>) -> SegmentData {
    let percent = util_percent.round() as u8;
    let primary = format!("{}%", percent);
    let secondary = format!("· {}", format_reset_time(resets_at));

    let mut metadata = HashMap::new();
    metadata.insert(
        "dynamic_icon".to_string(),
        get_circle_icon(util_percent / 100.0),
    );
    metadata.insert("utilization".to_string(), util_percent.to_string());

    SegmentData {
        primary,
        secondary,
        metadata,
    }
}

/// Pick a circle-fill glyph for the given utilization fraction (0.0..=1.0).
pub fn get_circle_icon(utilization: f64) -> String {
    let percent = (utilization * 100.0) as u8;
    match percent {
        0..=12 => "\u{f0a9e}".to_string(),  // circle_slice_1
        13..=25 => "\u{f0a9f}".to_string(), // circle_slice_2
        26..=37 => "\u{f0aa0}".to_string(), // circle_slice_3
        38..=50 => "\u{f0aa1}".to_string(), // circle_slice_4
        51..=62 => "\u{f0aa2}".to_string(), // circle_slice_5
        63..=75 => "\u{f0aa3}".to_string(), // circle_slice_6
        76..=87 => "\u{f0aa4}".to_string(), // circle_slice_7
        _ => "\u{f0aa5}".to_string(),       // circle_slice_8
    }
}

/// Render an RFC3339 reset timestamp as "month-day-hour" in the user's local
/// timezone. Rounds the hour up when minute > 45 so the display reflects the
/// next-effective hour boundary.
pub fn format_reset_time(reset_time_str: Option<&str>) -> String {
    if let Some(time_str) = reset_time_str {
        if let Ok(dt) = DateTime::parse_from_rfc3339(time_str) {
            let mut local_dt = dt.with_timezone(&Local);
            if local_dt.minute() > 45 {
                local_dt += Duration::hours(1);
            }
            return format!(
                "{}-{}-{}",
                local_dt.month(),
                local_dt.day(),
                local_dt.hour()
            );
        }
    }
    "?".to_string()
}

// --- private cache + HTTP plumbing ---

fn get_cache_path() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".claude")
            .join("ccline")
            .join(".api_usage_cache.json"),
    )
}

fn load_cache() -> Option<ApiUsageCache> {
    let cache_path = get_cache_path()?;
    if !cache_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&cache_path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_cache(cache: &ApiUsageCache) {
    if let Some(cache_path) = get_cache_path() {
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(cache) {
            let _ = std::fs::write(&cache_path, json);
        }
    }
}

fn is_cache_valid(cache: &ApiUsageCache, cache_duration: u64) -> bool {
    DateTime::parse_from_rfc3339(&cache.cached_at)
        .map(|cached_at| {
            let elapsed = Utc::now().signed_duration_since(cached_at.with_timezone(&Utc));
            elapsed.num_seconds() < cache_duration as i64
        })
        .unwrap_or(false)
}

fn get_claude_code_version() -> String {
    use std::process::Command;

    let output = Command::new("npm")
        .args(["view", "@anthropic-ai/claude-code", "version"])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !version.is_empty() {
                return format!("claude-code/{}", version);
            }
        }
        _ => {}
    }

    "claude-code".to_string()
}

fn get_proxy_from_settings() -> Option<String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let settings_path = format!("{}/.claude/settings.json", home);

    let content = std::fs::read_to_string(&settings_path).ok()?;
    let settings: serde_json::Value = serde_json::from_str(&content).ok()?;

    settings
        .get("env")?
        .get("HTTPS_PROXY")
        .or_else(|| settings.get("env")?.get("HTTP_PROXY"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn fetch_api_usage(api_base_url: &str, token: &str, timeout_secs: u64) -> Option<ApiUsageResponse> {
    let url = format!("{}/api/oauth/usage", api_base_url);
    let user_agent = get_claude_code_version();

    let agent = if let Some(proxy_url) = get_proxy_from_settings() {
        if let Ok(proxy) = ureq::Proxy::new(&proxy_url) {
            ureq::Agent::config_builder()
                .proxy(Some(proxy))
                .build()
                .new_agent()
        } else {
            ureq::Agent::new_with_defaults()
        }
    } else {
        ureq::Agent::new_with_defaults()
    };

    let response = agent
        .get(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", &user_agent)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(timeout_secs)))
        .build()
        .call()
        .ok()?;

    response.into_body().read_json().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- get_circle_icon — boundaries ----------

    #[track_caller]
    fn assert_icon(util_fraction: f64, expected: &str) {
        assert_eq!(
            get_circle_icon(util_fraction),
            expected,
            "for utilization {}",
            util_fraction
        );
    }

    #[test]
    fn circle_icon_buckets() {
        assert_icon(0.0, "\u{f0a9e}");
        assert_icon(0.12, "\u{f0a9e}");
        assert_icon(0.13, "\u{f0a9f}");
        assert_icon(0.25, "\u{f0a9f}");
        assert_icon(0.26, "\u{f0aa0}");
        assert_icon(0.37, "\u{f0aa0}");
        assert_icon(0.38, "\u{f0aa1}");
        assert_icon(0.50, "\u{f0aa1}");
        assert_icon(0.51, "\u{f0aa2}");
        assert_icon(0.62, "\u{f0aa2}");
        assert_icon(0.63, "\u{f0aa3}");
        assert_icon(0.75, "\u{f0aa3}");
        assert_icon(0.76, "\u{f0aa4}");
        assert_icon(0.87, "\u{f0aa4}");
        assert_icon(0.88, "\u{f0aa5}");
        assert_icon(1.00, "\u{f0aa5}");
    }

    // ---------- format_reset_time ----------

    #[test]
    fn reset_time_none_returns_placeholder() {
        assert_eq!(format_reset_time(None), "?");
    }

    #[test]
    fn reset_time_malformed_returns_placeholder() {
        assert_eq!(format_reset_time(Some("not a date")), "?");
        assert_eq!(format_reset_time(Some("")), "?");
        assert_eq!(format_reset_time(Some("2026-13-99")), "?");
    }

    #[test]
    fn reset_time_valid_has_month_day_hour_shape() {
        let out = format_reset_time(Some("2026-05-13T15:30:00Z"));
        let parts: Vec<&str> = out.split('-').collect();
        assert_eq!(parts.len(), 3, "{:?}", out);
        for p in &parts {
            assert!(p.parse::<u32>().is_ok(), "non-numeric: {:?}", p);
        }
    }

    // ---------- format_segment_data ----------

    #[test]
    fn format_segment_data_renders_percent_and_keeps_dynamic_icon() {
        let data = format_segment_data(67.0, Some("2026-05-13T15:30:00Z"));
        assert_eq!(data.primary, "67%");
        assert!(data.secondary.starts_with("· "), "{:?}", data.secondary);
        assert!(data.metadata.contains_key("dynamic_icon"));
        // 67% → slice_6 (range 63..=75) → f0aa3
        assert_eq!(data.metadata.get("dynamic_icon").unwrap(), "\u{f0aa3}");
        assert_eq!(data.metadata.get("utilization").unwrap(), "67");
    }

    #[test]
    fn format_segment_data_rounds_percent() {
        let data = format_segment_data(22.6, None);
        assert_eq!(data.primary, "23%");
    }

    #[test]
    fn format_segment_data_uses_own_value_for_icon() {
        // Ensure the dynamic icon is keyed on the SAME value we display, not a
        // detached weekly utilization. This is the regression we explicitly
        // wanted to avoid carrying over from the pre-T02 Usage segment.
        let usage_data = format_segment_data(8.0, None);
        let weekly_data = format_segment_data(80.0, None);
        assert_eq!(
            usage_data.metadata.get("dynamic_icon").unwrap(),
            "\u{f0a9e}"
        ); // slice_1
        assert_eq!(
            weekly_data.metadata.get("dynamic_icon").unwrap(),
            "\u{f0aa4}"
        ); // slice_7
    }

    // ---------- cache schema backwards-compat ----------

    #[test]
    fn cache_schema_loads_legacy_v1_without_five_hour_resets_at() {
        let legacy = r#"{
            "five_hour_utilization": 23.0,
            "seven_day_utilization": 67.0,
            "resets_at": "2026-05-20T00:00:00Z",
            "cached_at": "2026-05-13T15:00:00Z"
        }"#;
        let cache: ApiUsageCache = serde_json::from_str(legacy).expect("legacy parse");
        assert_eq!(cache.five_hour_utilization, 23.0);
        assert_eq!(cache.seven_day_utilization, 67.0);
        assert!(cache.five_hour_resets_at.is_none());
    }

    #[test]
    fn cache_schema_round_trips_v2_with_both_reset_times() {
        let cache = ApiUsageCache {
            five_hour_utilization: 22.0,
            seven_day_utilization: 26.0,
            resets_at: Some("seven-day-reset".into()),
            five_hour_resets_at: Some("five-hour-reset".into()),
            cached_at: "now".into(),
        };
        let json = serde_json::to_string(&cache).unwrap();
        let parsed: ApiUsageCache = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.resets_at.as_deref(), Some("seven-day-reset"));
        assert_eq!(
            parsed.five_hour_resets_at.as_deref(),
            Some("five-hour-reset")
        );
    }

    // ---------- ApiUsageResponse forward-compat (moved from usage.rs::tests) ----------

    #[test]
    fn api_usage_response_preserves_unknown_top_level_fields() {
        let json = r#"{
            "five_hour":   { "utilization": 12.5, "resets_at": "2026-05-13T20:00:00Z" },
            "seven_day":   { "utilization": 45.0, "resets_at": "2026-05-20T00:00:00Z" },
            "weekly_opus": { "utilization": 8.0,  "resets_at": "2026-05-20T00:00:00Z" },
            "future_field": "anything"
        }"#;
        let parsed: ApiUsageResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.five_hour.utilization, 12.5);
        assert_eq!(parsed.seven_day.utilization, 45.0);
        assert!(parsed.extra.contains_key("weekly_opus"));
        assert!(parsed.extra.contains_key("future_field"));
    }

    #[test]
    fn usage_period_preserves_unknown_fields() {
        let json = r#"{ "utilization": 50.0, "resets_at": null, "model": "opus" }"#;
        let parsed: UsagePeriod = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.utilization, 50.0);
        assert!(parsed.extra.contains_key("model"));
    }

    #[test]
    fn api_usage_response_parses_real_world_sample() {
        let sample = include_str!("../../../docs/api-samples/usage-response.json");
        let parsed: ApiUsageResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.five_hour.utilization, 22.0);
        assert_eq!(parsed.seven_day.utilization, 26.0);
        assert!(parsed.extra.contains_key("seven_day_sonnet"));
        assert!(parsed.extra.contains_key("seven_day_opus"));
    }
}
