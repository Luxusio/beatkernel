//! Shared BMS mine timing and pristine judge construction.

use beatkernel::{
    chart::CompiledChart,
    judge::{HazardId, HazardMarker, HazardTimeline, JudgeEngine, JudgeProfile},
};
use beatkernel_bms::{BmsChart, BmsInputMode, ScheduledMine};

/// Validated original mine metadata and its immutable core hazard timeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinePlan {
    markers: Vec<ScheduledMine>,
    timeline: Option<HazardTimeline>,
}

impl MinePlan {
    /// Checks the caller's marker budget before compiling actual mine timing.
    /// Empty sources have no configured timeline and retain legacy judge identity.
    pub fn prepare(source: &BmsChart, max_markers: usize) -> Result<Self, String> {
        if source.mines.len() > max_markers {
            return Err("mine marker count exceeds configured capacity".into());
        }
        if source.mines.is_empty() {
            return Ok(Self {
                markers: Vec::new(),
                timeline: None,
            });
        }
        let markers = source.compile_mines().map_err(|error| error.to_string())?;
        let mut hazards = Vec::new();
        hazards
            .try_reserve_exact(markers.len())
            .map_err(|_| "mine hazard allocation failed")?;
        hazards.extend(markers.iter().map(|marker| HazardMarker {
            id: HazardId(marker.ordinal),
            at: marker.at,
            control: marker.lane.control(),
            value: u64::from(marker.damage.raw()),
        }));
        let timeline =
            HazardTimeline::new(hazards, max_markers).map_err(|error| error.to_string())?;
        Ok(Self {
            markers,
            timeline: Some(timeline),
        })
    }

    /// Exact source metadata in compiled time/ordinal order.
    pub fn markers(&self) -> &[ScheduledMine] {
        &self.markers
    }

    /// An owned core timeline, absent for an empty mine source.
    pub fn timeline(&self) -> Option<HazardTimeline> {
        self.timeline.clone()
    }
}

/// Constructs a pristine judge from the caller's matching ordinary chart and
/// selected source. No PCM, gauge, sound or prior occupancy is manufactured.
pub fn prepare_judge(
    source: &BmsChart,
    chart: CompiledChart,
    profile: JudgeProfile,
    mode: BmsInputMode,
    max_markers: usize,
) -> Result<JudgeEngine, String> {
    prepare_judge_with_timing(source, chart, profile, mode, max_markers, None)
}

/// Preserves source mine configuration while preparing explicit staged windows.
pub fn prepare_judge_with_timing(
    source: &BmsChart,
    chart: CompiledChart,
    profile: JudgeProfile,
    mode: BmsInputMode,
    max_markers: usize,
    timing: Option<&beatkernel_bms::BmsTimingProfiles>,
) -> Result<JudgeEngine, String> {
    let plan = MinePlan::prepare(source, max_markers)?;
    let constructor = if mode == BmsInputMode::ButtonOrContact
        && (!source.invisible.is_empty() || !source.mines.is_empty())
    {
        JudgeEngine::new_with_contacts
    } else {
        JudgeEngine::new
    };
    let rules = match timing {
        Some(timing) => source
            .rules_with_timing_profiles(mode, timing)
            .map_err(|error| error.to_string())?,
        None => source.rules_with_input_mode(mode),
    };
    let mut judge = constructor(chart, rules, profile).map_err(|error| error.to_string())?;
    if let Some(timeline) = plan.timeline {
        judge
            .configure_hazards(timeline)
            .map_err(|error| error.to_string())?;
    }
    Ok(judge)
}
