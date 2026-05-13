use super::usage_api;
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
        let mut data = usage_api::format_segment_data(
            snapshot.seven_day_utilization,
            snapshot.seven_day_resets_at.as_deref(),
        );
        data.metadata.insert(
            "five_hour_utilization".to_string(),
            snapshot.five_hour_utilization.to_string(),
        );

        // T10 trend: append `↑ Npp%` / `↓ Npp%` / `→ ~0%` to secondary text
        // when we have a history entry near ~7 days ago. Silent (no marker)
        // when data is too thin or the user opted out.
        let config = crate::config::Config::load().ok();
        let opts = config
            .as_ref()
            .and_then(|c| c.segments.iter().find(|s| s.id == SegmentId::WeeklyUsage));
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
