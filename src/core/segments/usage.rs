use super::usage_api;
use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId};

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
        let mut data = usage_api::format_segment_data(
            snapshot.five_hour_utilization,
            snapshot.five_hour_resets_at.as_deref(),
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
