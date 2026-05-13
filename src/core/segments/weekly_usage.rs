use super::usage_api;
use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId};

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
        Some(data)
    }

    fn id(&self) -> SegmentId {
        SegmentId::WeeklyUsage
    }
}
