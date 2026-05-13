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
/// Default upper bound on cache age — past this, the segment treats the cache
/// as too stale to serve. The refresh subprocess will still try to repopulate
/// it in the background.
pub const DEFAULT_REVALIDATE_AFTER_SECS: i64 = 1800;
/// Hold a refresh lock for at most this long; older locks are considered
/// abandoned (process died mid-fetch) and can be overridden.
pub const REFRESH_LOCK_TTL_SECS: i64 = 30;

/// SWR cache state inferred from the cached snapshot's age + the segment's
/// freshness thresholds. Used by [`fetch_or_cached`] to decide whether to
/// serve, refresh in the background, or fall back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheState {
    /// No cache file (or unparseable timestamp). No data to serve.
    Cold,
    /// Fresh enough — serve directly, no refresh needed.
    Hot,
    /// Past `hot_seconds` but inside `revalidate_seconds` — serve cache AND
    /// trigger a background refresh so the next render is fresher.
    SoftStale,
    /// Past `revalidate_seconds` — too old to trust; don't serve. Refresh.
    HardStale,
}

/// Refresh-lock contents — a tiny JSON file that flags "a refresh subprocess
/// is currently in flight". Older locks (past [`REFRESH_LOCK_TTL_SECS`]) are
/// considered orphaned and can be overridden.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub started_at: String,
}

impl LockInfo {
    pub fn parse(content: &str) -> Option<LockInfo> {
        serde_json::from_str(content).ok()
    }

    pub fn started_at_dt(&self) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(&self.started_at)
            .ok()
            .map(|t| t.with_timezone(&Utc))
    }

    /// A lock is stale if it can't be parsed, its timestamp is malformed, or
    /// the holder started more than `ttl_seconds` ago.
    pub fn is_stale(&self, now: DateTime<Utc>, ttl_seconds: i64) -> bool {
        match self.started_at_dt() {
            Some(t) => now.signed_duration_since(t).num_seconds() > ttl_seconds,
            None => true,
        }
    }
}

/// Classify a cached snapshot's age into one of the four [`CacheState`]
/// values. Pure — driven by `cached_at` + `now` + the two thresholds.
pub fn classify_cache(
    cached_at: Option<&str>,
    now: DateTime<Utc>,
    hot_seconds: i64,
    revalidate_seconds: i64,
) -> CacheState {
    let cached_at_dt = match cached_at.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
        Some(t) => t.with_timezone(&Utc),
        None => return CacheState::Cold,
    };
    let age = now.signed_duration_since(cached_at_dt).num_seconds();
    if age < hot_seconds {
        CacheState::Hot
    } else if age < revalidate_seconds {
        CacheState::SoftStale
    } else {
        CacheState::HardStale
    }
}

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

/// On-disk cache shape. Backwards-compatible with prior layouts: legacy
/// caches without `five_hour_resets_at` or the T05 `previous_*` fields still
/// parse — every additive field is `#[serde(default)]`.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiUsageCache {
    pub five_hour_utilization: f64,
    pub seven_day_utilization: f64,
    /// Historical name — was always populated from `seven_day.resets_at`.
    pub resets_at: Option<String>,
    #[serde(default)]
    pub five_hour_resets_at: Option<String>,
    pub cached_at: String,
    /// Five-hour utilization from the previous successful fetch. T05 uses
    /// `(current, previous)` over `(cached_at, previous_cached_at)` to derive
    /// percent-per-minute and project the exhaust time.
    #[serde(default)]
    pub previous_five_hour_utilization: Option<f64>,
    #[serde(default)]
    pub previous_cached_at: Option<String>,
}

impl ApiUsageCache {
    pub fn seven_day_resets_at(&self) -> Option<&str> {
        self.resets_at.as_deref()
    }
}

/// A snapshot of both utilization windows + their reset times, used by
/// segments to pick the value they want to render. T05 also surfaces the
/// previous five-hour reading so it can compute a projection without
/// re-reading the cache.
#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub five_hour_utilization: f64,
    pub five_hour_resets_at: Option<String>,
    pub seven_day_utilization: f64,
    pub seven_day_resets_at: Option<String>,
    pub previous_five_hour_utilization: Option<f64>,
    pub previous_cached_at: Option<String>,
}

/// Stale-while-revalidate hot path. Never blocks on network.
///
/// - **Hot** cache → serve from disk.
/// - **SoftStale** → serve from disk AND spawn a detached `ccline
///   --refresh-usage` so the next render is fresher.
/// - **HardStale** / **Cold** → return `None` and spawn refresh; the segment
///   either falls back to its placeholder or hides (depending on its impl).
///
/// `segment_id` is used only to look up the segment's `cache_duration` and
/// `revalidate_after_seconds` options.
pub fn fetch_or_cached(segment_id: SegmentId) -> Option<UsageSnapshot> {
    let config = crate::config::Config::load().ok()?;
    let opts = config.segments.iter().find(|s| s.id == segment_id);

    let hot_seconds = opts
        .and_then(|sc| sc.options.get("cache_duration"))
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_CACHE_DURATION_SECS as i64);
    let revalidate_seconds = opts
        .and_then(|sc| sc.options.get("revalidate_after_seconds"))
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_REVALIDATE_AFTER_SECS);

    let cached = load_cache();
    let now = Utc::now();
    let state = classify_cache(
        cached.as_ref().map(|c| c.cached_at.as_str()),
        now,
        hot_seconds,
        revalidate_seconds,
    );

    match state {
        CacheState::Hot => cached.map(cache_to_snapshot),
        CacheState::SoftStale => {
            spawn_detached_refresh();
            cached.map(cache_to_snapshot)
        }
        CacheState::HardStale | CacheState::Cold => {
            spawn_detached_refresh();
            None
        }
    }
}

fn cache_to_snapshot(c: ApiUsageCache) -> UsageSnapshot {
    UsageSnapshot {
        five_hour_utilization: c.five_hour_utilization,
        five_hour_resets_at: c.five_hour_resets_at.clone(),
        seven_day_utilization: c.seven_day_utilization,
        seven_day_resets_at: c.resets_at.clone(),
        previous_five_hour_utilization: c.previous_five_hour_utilization,
        previous_cached_at: c.previous_cached_at,
    }
}

/// Spawn a detached `ccline --refresh-usage` child. Returns immediately.
///
/// Coordinates with sibling segments in the same render: a single render can
/// have 4+ usage-bearing segments (Usage / WeeklyUsage / BurnRate /
/// ProjectedExhaust) all hitting SWR. The first segment to land here writes
/// the lock + spawns; the rest see a fresh lock and skip spawning.
///
/// The child unconditionally overwrites the lock with its own pid (via
/// `refresh_now`) — it does **not** try to "claim" it. That way:
/// - Concurrent siblings still fan in to one child (the parent's lock deters
///   re-spawns).
/// - The child always reaches `release_refresh_lock` at the end of its work,
///   so the lock doesn't leak even if the parent's claim races weirdly.
/// - On panic/crash, the 30s TTL self-heals.
fn spawn_detached_refresh() {
    let now = Utc::now();
    if let Some(existing) = read_lock_file() {
        if !existing.is_stale(now, REFRESH_LOCK_TTL_SECS) {
            // Another sibling in this (or another) render has already kicked
            // off a refresh. Don't pile on.
            return;
        }
    }
    let me = LockInfo {
        pid: std::process::id(),
        started_at: now.to_rfc3339(),
    };
    if !write_lock_file(&me) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        release_refresh_lock();
        return;
    };
    if std::process::Command::new(exe)
        .arg("--refresh-usage")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_err()
    {
        release_refresh_lock();
    }
    // Child overwrites our lock with its own pid and releases on completion.
}

/// Synchronous refresh used by the `ccline --refresh-usage` subprocess.
/// Unconditionally takes ownership of the lock (overwriting whatever the
/// parent claimed), fetches from the API, rotates history, writes the cache
/// atomically, then releases.
///
/// Overwriting (rather than checking) is intentional: when spawned by
/// `spawn_detached_refresh`, the parent has just written a "spawn in flight"
/// lock with its own pid. The child shouldn't bail at that point — it owns
/// the work. The cost of overwriting is just clobbering a JSON file with
/// nearly-identical contents.
///
/// Returns `Ok(())` on success; `Err` covers token / network / config
/// failures. The release runs regardless. Errors are non-fatal — the caller
/// (the subprocess `main`) prints nothing and exits 0 either way, so the
/// next render simply retries.
pub fn refresh_now() -> Result<(), String> {
    let me = LockInfo {
        pid: std::process::id(),
        started_at: Utc::now().to_rfc3339(),
    };
    let _ = write_lock_file(&me);
    let result = refresh_inner();
    release_refresh_lock();
    result
}

fn refresh_inner() -> Result<(), String> {
    let token = credentials::get_oauth_token().ok_or("no OAuth token available")?;

    // Read api_base_url + timeout from any segment that has them — Usage is
    // the historical home. Defaults are fine if no segment configured them.
    let config = crate::config::Config::load().map_err(|e| format!("config: {}", e))?;
    let opts = config
        .segments
        .iter()
        .find(|s| s.id == SegmentId::Usage)
        .or_else(|| {
            config
                .segments
                .iter()
                .find(|s| s.id == SegmentId::WeeklyUsage)
        });
    let api_base_url = opts
        .and_then(|sc| sc.options.get("api_base_url"))
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_API_BASE_URL);
    let timeout = opts
        .and_then(|sc| sc.options.get("timeout"))
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_HTTP_TIMEOUT_SECS);

    let response =
        fetch_api_usage(api_base_url, &token, timeout).ok_or_else(|| "fetch failed".to_string())?;

    // Rotate: existing current values → previous slots.
    let cached = load_cache();
    let (previous_util, previous_at) = match cached.as_ref() {
        Some(c) => (Some(c.five_hour_utilization), Some(c.cached_at.clone())),
        None => (None, None),
    };
    let now = Utc::now();
    let cache = ApiUsageCache {
        five_hour_utilization: response.five_hour.utilization,
        seven_day_utilization: response.seven_day.utilization,
        resets_at: response.seven_day.resets_at,
        five_hour_resets_at: response.five_hour.resets_at,
        cached_at: now.to_rfc3339(),
        previous_five_hour_utilization: previous_util,
        previous_cached_at: previous_at,
    };
    save_cache(&cache);

    // Append one row to the long-term history JSONL (T08). Best-effort —
    // failure here doesn't fail the refresh.
    crate::utils::usage_history::append(&crate::utils::usage_history::HistoryEntry {
        t: now.to_rfc3339(),
        five_hour: cache.five_hour_utilization,
        weekly: cache.seven_day_utilization,
    });

    Ok(())
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
    // Generic key used by threshold rendering. Specific keys (like
    // `five_hour_utilization`, `seven_day_utilization`) are added by callers
    // when they want to expose both windows for hook consumers.
    metadata.insert("percent".to_string(), util_percent.to_string());

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

fn get_lock_path() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".claude")
            .join("ccline")
            .join(".usage_refresh.lock"),
    )
}

fn read_lock_file() -> Option<LockInfo> {
    let p = get_lock_path()?;
    let content = std::fs::read_to_string(&p).ok()?;
    LockInfo::parse(&content)
}

fn write_lock_file(lock: &LockInfo) -> bool {
    let Some(p) = get_lock_path() else {
        return false;
    };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    serde_json::to_string(lock)
        .ok()
        .and_then(|json| std::fs::write(&p, json).ok())
        .is_some()
}

/// Remove the refresh lock file. Safe to call when no lock exists.
pub fn release_refresh_lock() {
    if let Some(p) = get_lock_path() {
        let _ = std::fs::remove_file(&p);
    }
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
    let Some(cache_path) = get_cache_path() else {
        return;
    };
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(json) = serde_json::to_string_pretty(cache) else {
        return;
    };
    // Atomic write: write to sibling .tmp file then rename. Rename is atomic
    // on POSIX; on Windows std::fs::rename overwrites since Rust 1.71. A
    // partial write to .tmp doesn't corrupt the live cache.
    let tmp_path = cache_path.with_extension("json.tmp");
    if std::fs::write(&tmp_path, json).is_err() {
        return;
    }
    if std::fs::rename(&tmp_path, &cache_path).is_err() {
        // Best-effort cleanup of the temp file if rename failed.
        let _ = std::fs::remove_file(&tmp_path);
    }
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
        assert_eq!(data.metadata.get("percent").unwrap(), "67");
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

    // ---------- classify_cache (T09) ----------

    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn classify_cold_when_no_timestamp() {
        let now = at(2026, 5, 13, 12, 0);
        assert_eq!(classify_cache(None, now, 180, 1800), CacheState::Cold);
    }

    #[test]
    fn classify_cold_when_malformed_timestamp() {
        let now = at(2026, 5, 13, 12, 0);
        assert_eq!(
            classify_cache(Some("not-a-date"), now, 180, 1800),
            CacheState::Cold
        );
        assert_eq!(classify_cache(Some(""), now, 180, 1800), CacheState::Cold);
    }

    #[test]
    fn classify_hot_when_within_freshness_window() {
        let now = at(2026, 5, 13, 12, 0);
        let cached = (now - Duration::seconds(60)).to_rfc3339();
        assert_eq!(
            classify_cache(Some(&cached), now, 180, 1800),
            CacheState::Hot
        );
    }

    #[test]
    fn classify_soft_stale_between_thresholds() {
        let now = at(2026, 5, 13, 12, 0);
        // 5 minutes old: past 180s hot window, inside 1800s revalidate window.
        let cached = (now - Duration::seconds(300)).to_rfc3339();
        assert_eq!(
            classify_cache(Some(&cached), now, 180, 1800),
            CacheState::SoftStale
        );
    }

    #[test]
    fn classify_hard_stale_past_revalidate() {
        let now = at(2026, 5, 13, 12, 0);
        let cached = (now - Duration::seconds(3600)).to_rfc3339(); // 1h old
        assert_eq!(
            classify_cache(Some(&cached), now, 180, 1800),
            CacheState::HardStale
        );
    }

    #[test]
    fn classify_exact_boundary_inclusive_hot() {
        let now = at(2026, 5, 13, 12, 0);
        // Cache exactly hot_seconds old → just past hot → SoftStale.
        let cached = (now - Duration::seconds(180)).to_rfc3339();
        assert_eq!(
            classify_cache(Some(&cached), now, 180, 1800),
            CacheState::SoftStale
        );
        // 1s younger → Hot.
        let cached = (now - Duration::seconds(179)).to_rfc3339();
        assert_eq!(
            classify_cache(Some(&cached), now, 180, 1800),
            CacheState::Hot
        );
    }

    // ---------- LockInfo (T09) ----------

    #[test]
    fn lock_info_parses_well_formed_json() {
        let json = r#"{"pid": 12345, "started_at": "2026-05-13T12:00:00Z"}"#;
        let lock = LockInfo::parse(json).expect("parse");
        assert_eq!(lock.pid, 12345);
        assert!(lock.started_at_dt().is_some());
    }

    #[test]
    fn lock_info_parse_malformed_returns_none() {
        assert!(LockInfo::parse("not json").is_none());
        assert!(LockInfo::parse("{}").is_none()); // missing fields
    }

    #[test]
    fn lock_info_is_stale_when_older_than_ttl() {
        let now = at(2026, 5, 13, 12, 0);
        let lock = LockInfo {
            pid: 1,
            started_at: (now - Duration::seconds(60)).to_rfc3339(),
        };
        assert!(lock.is_stale(now, 30));
    }

    #[test]
    fn lock_info_is_fresh_within_ttl() {
        let now = at(2026, 5, 13, 12, 0);
        let lock = LockInfo {
            pid: 1,
            started_at: (now - Duration::seconds(5)).to_rfc3339(),
        };
        assert!(!lock.is_stale(now, 30));
    }

    #[test]
    fn lock_info_with_malformed_timestamp_is_stale() {
        let lock = LockInfo {
            pid: 1,
            started_at: "garbage".to_string(),
        };
        assert!(lock.is_stale(at(2026, 5, 13, 12, 0), 30));
    }

    #[test]
    fn classify_future_cached_at_is_hot() {
        // Defensive: a future timestamp (clock skew) shouldn't classify as stale.
        let now = at(2026, 5, 13, 12, 0);
        let cached = (now + Duration::seconds(60)).to_rfc3339();
        assert_eq!(
            classify_cache(Some(&cached), now, 180, 1800),
            CacheState::Hot
        );
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
    fn cache_schema_round_trips_with_all_optional_fields() {
        let cache = ApiUsageCache {
            five_hour_utilization: 22.0,
            seven_day_utilization: 26.0,
            resets_at: Some("seven-day-reset".into()),
            five_hour_resets_at: Some("five-hour-reset".into()),
            cached_at: "now".into(),
            previous_five_hour_utilization: Some(20.0),
            previous_cached_at: Some("earlier".into()),
        };
        let json = serde_json::to_string(&cache).unwrap();
        let parsed: ApiUsageCache = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.resets_at.as_deref(), Some("seven-day-reset"));
        assert_eq!(
            parsed.five_hour_resets_at.as_deref(),
            Some("five-hour-reset")
        );
        assert_eq!(parsed.previous_five_hour_utilization, Some(20.0));
        assert_eq!(parsed.previous_cached_at.as_deref(), Some("earlier"));
    }

    #[test]
    fn cache_schema_loads_with_no_previous_fields() {
        // The v1/v2 caches that pre-date T05 still load cleanly; previous_*
        // come back as None.
        let mid = r#"{
            "five_hour_utilization": 23.0,
            "seven_day_utilization": 67.0,
            "resets_at": "2026-05-20T00:00:00Z",
            "five_hour_resets_at": "2026-05-13T20:00:00Z",
            "cached_at": "2026-05-13T15:00:00Z"
        }"#;
        let cache: ApiUsageCache = serde_json::from_str(mid).expect("parse");
        assert!(cache.previous_five_hour_utilization.is_none());
        assert!(cache.previous_cached_at.is_none());
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
