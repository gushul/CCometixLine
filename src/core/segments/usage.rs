use super::usage_api::{self, ResetTimeFormat};
use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId};
use chrono::Utc;

#[derive(Default)]
pub struct UsageSegment;

impl UsageSegment {
    pub fn new() -> Self {
        Self
    }
}

impl Segment for UsageSegment {
    fn collect(&self, _input: &InputData) -> Option<SegmentData> {
        let snapshot = usage_api::fetch_or_cached(SegmentId::Usage)?;
        let reset_format = reset_format_from_config(SegmentId::Usage);
        let mut data = usage_api::format_segment_data(
            snapshot.five_hour_utilization,
            snapshot.five_hour_resets_at.as_deref(),
            reset_format,
            Utc::now(),
        );
        // Carry the weekly value through metadata so consumers (color
        // thresholds, hooks) can read it without re-fetching.
        data.metadata.insert(
            "seven_day_utilization".to_string(),
            snapshot.seven_day_utilization.to_string(),
        );
        Some(data)
    }

    fn id(&self) -> SegmentId {
        SegmentId::Usage
    }
}

fn reset_format_from_config(segment_id: SegmentId) -> ResetTimeFormat {
    crate::config::Config::load()
        .ok()
        .and_then(|c| {
            c.segments
                .iter()
                .find(|s| s.id == segment_id)
                .and_then(|sc| sc.options.get("reset_time_format").cloned())
        })
        .and_then(|v| v.as_str().map(ResetTimeFormat::from_option_str))
        .unwrap_or_default()
}
