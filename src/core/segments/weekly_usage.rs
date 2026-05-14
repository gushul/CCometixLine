use super::usage_api::{self, ResetTimeFormat};
use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId};
use crate::utils::usage_history;
use chrono::Utc;

/// Default tolerance window when looking up "this time last week" in history.
const DEFAULT_TREND_TOLERANCE_HOURS: i64 = 24;

#[derive(Default)]
pub struct WeeklyUsageSegment;

impl WeeklyUsageSegment {
    pub fn new() -> Self {
        Self
    }
}

impl Segment for WeeklyUsageSegment {
    fn collect(&self, _input: &InputData) -> Option<SegmentData> {
        let snapshot = usage_api::fetch_or_cached(SegmentId::WeeklyUsage)?;

        let config = crate::config::Config::load().ok();
        let opts = config
            .as_ref()
            .and_then(|c| c.segments.iter().find(|s| s.id == SegmentId::WeeklyUsage));

        let reset_format = opts
            .and_then(|sc| sc.options.get("reset_time_format"))
            .and_then(|v| v.as_str().map(ResetTimeFormat::from_option_str))
            .unwrap_or_default();
        let now = Utc::now();
        let mut data = usage_api::format_segment_data(
            snapshot.seven_day_utilization,
            snapshot.seven_day_resets_at.as_deref(),
            reset_format,
            now,
        );
        data.metadata.insert(
            "five_hour_utilization".to_string(),
            snapshot.five_hour_utilization.to_string(),
        );

        // T06 per-model breakdown: append `S X% O Y%` to primary when display
        // is "compact" and at least one of the per-model values is known.
        let display = opts
            .and_then(|sc| sc.options.get("display"))
            .and_then(|v| v.as_str())
            .unwrap_or("primary_only");
        if display == "compact" {
            if let Some(breakdown) = usage_history::format_per_model_breakdown(
                snapshot.seven_day_sonnet_utilization,
                snapshot.seven_day_opus_utilization,
            ) {
                data.primary = format!("{} ({})", data.primary, breakdown);
            }
        }
        if let Some(v) = snapshot.seven_day_sonnet_utilization {
            data.metadata
                .insert("seven_day_sonnet".to_string(), v.to_string());
        }
        if let Some(v) = snapshot.seven_day_opus_utilization {
            data.metadata
                .insert("seven_day_opus".to_string(), v.to_string());
        }

        // T10 trend: append `↑ Npp%` / `↓ Npp%` / `→ ~0%` to secondary text
        // when we have a history entry near ~7 days ago. Silent (no marker)
        // when data is too thin or the user opted out.
        let show_trend = opts
            .and_then(|sc| sc.options.get("show_trend"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let tolerance_hours = opts
            .and_then(|sc| sc.options.get("trend_tolerance_hours"))
            .and_then(|v| v.as_i64())
            .unwrap_or(DEFAULT_TREND_TOLERANCE_HOURS);

        if show_trend {
            let entries = usage_history::load_all();
            if let Some(past) =
                usage_history::entry_near_offset_days_ago(&entries, 7, Utc::now(), tolerance_hours)
            {
                let arrow =
                    usage_history::format_trend_arrow(snapshot.seven_day_utilization, past.weekly);
                data.secondary = if data.secondary.is_empty() {
                    arrow
                } else {
                    format!("{} {}", data.secondary, arrow)
                };
                data.metadata
                    .insert("trend_past_weekly".to_string(), past.weekly.to_string());
            }
        }

        Some(data)
    }

    fn id(&self) -> SegmentId {
        SegmentId::WeeklyUsage
    }
}
