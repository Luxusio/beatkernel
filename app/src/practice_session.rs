//! Cold preparation of fresh practice owners from the immutable original chart.
//!
//! This module never owns PCM or an output endpoint. A prepared payload is not a
//! committed audio transition: the native pump must first qualify the actual
//! boundary and drain its acquired input prefix, then install all fresh owners.
use crate::{
    competition::ScoreSummary,
    gauge::BmsGauge,
    judgment_policy::BmsJudgmentPolicy,
    native_gameplay::NativeGameplayResult,
    native_judge::NativeJudgeConfig,
    play_policy::{GaugeSelection, OriginalGaugeContext, ResolvedPlayPolicy, ResolvedTimingPolicy},
    replay_capture::LiveReplayCapture,
    replay_speed::ReplaySpeed,
    session_launch::SessionLaunch,
};
use beatkernel::{
    judge::JudgeEngine,
    replay::codec::ReplayCodecLimits,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::{OutputPhaseBasis, OutputPhaseError, Rate, Transport},
};
use beatkernel_bms::BmsChart;
use std::path::PathBuf;

/// Original-song coordinates; `None` preserves full-song completion intent.
/// The output domain is retained across attempts and is not a new device epoch.
#[derive(Clone, Copy, Debug)]
pub struct PracticeAttemptConfig {
    pub start: Timestamp,
    pub end: Option<Timestamp>,
    pub domain: ClockDomainId,
    pub chart_seed: u64,
    pub capture_limits: Option<ReplayCodecLimits>,
}

/// Coherent cold owners. Prior attempts and captures are deliberately not
/// accepted here: retire/archive them at the qualified boundary before install.
pub struct PreparedPracticeAttempt {
    pub source: BmsChart,
    pub judge: JudgeEngine,
    pub gauge: BmsGauge,
    pub score: ScoreSummary,
    pub judgments: Option<BmsJudgmentPolicy>,
    pub timing: Option<ResolvedTimingPolicy>,
    pub selection: GaugeSelection,
    pub original_gauge: OriginalGaugeContext,
    pub capture: Option<LiveReplayCapture>,
    pub config: PracticeAttemptConfig,
    /// Canonical retry identity only; do not spawn these arguments to commit.
    /// Its original invocation remains pinned for later F5 or practice attempts.
    pub next_launch: SessionLaunch,
    pub recording_path: Option<PathBuf>,
    pub excluded_objects: usize,
    pub excluded_crossing_holds: usize,
    speed: ReplaySpeed,
}

impl PreparedPracticeAttempt {
    pub const fn speed(&self) -> ReplaySpeed {
        self.speed
    }

    /// Build a selected-rate zero-preroll transport at a qualified boundary.
    /// This scalar seam does not preserve fractional source/target frame phase:
    /// non-divisor grids and converted cuts require the shared phase port before
    /// native retained-speed pumps may use it as an exact timing authority.
    pub fn transport_at(&self, logical_boundary: Timestamp) -> Transport {
        Transport::new(logical_boundary, self.config.start, self.speed.rate())
    }

    /// Build a fresh zero-preroll transport with the selected output phase.
    /// This validates mathematical coherence only: the caller must qualify the
    /// original native boundary and active frame cut, including converter phase.
    pub fn transport_at_output_phase(
        &self,
        basis: OutputPhaseBasis,
        logical_boundary: ClockPoint,
    ) -> Result<Transport, OutputPhaseError> {
        if basis.domain() != self.config.domain || logical_boundary.domain != self.config.domain {
            return Err(OutputPhaseError::WrongDomain);
        }
        Transport::new_with_output_phase(
            basis,
            logical_boundary,
            self.config.start,
            Duration::ZERO,
            self.speed.rate(),
            self.config.end,
        )
    }

    pub fn transport_at_runtime_output_phase(
        &self,
        basis: OutputPhaseBasis,
        logical_boundary: ClockPoint,
    ) -> Result<Transport, OutputPhaseError> {
        if basis.domain() != self.config.domain || logical_boundary.domain != self.config.domain {
            return Err(OutputPhaseError::WrongDomain);
        }
        Transport::new_with_runtime_output_phase(
            basis,
            logical_boundary,
            self.config.start,
            Duration::ZERO,
            self.speed.rate(),
            self.config.end,
        )
    }
}

/// Prepare without modifying a current judge, launch, capture, queue or bank.
/// Always pass the original parsed chart, never a previous selected section.
/// Later heads remain in the original chart beyond a finite end, matching the
/// existing finite-prefix judge contract rather than forcing terminal misses.
pub fn prepare_attempt(
    original: &BmsChart,
    policy: &ResolvedPlayPolicy,
    current_launch: &SessionLaunch,
    config: PracticeAttemptConfig,
) -> NativeGameplayResult<PreparedPracticeAttempt> {
    prepare_attempt_inner(
        original,
        policy,
        current_launch,
        config,
        ReplaySpeed::new(Rate::NORMAL)?,
        false,
    )
}

/// Prepare canonical selected-speed owners without mutating an active attempt.
/// Exact fractional retained phase remains the native integration prerequisite.
pub fn prepare_attempt_with_speed(
    original: &BmsChart,
    policy: &ResolvedPlayPolicy,
    current_launch: &SessionLaunch,
    config: PracticeAttemptConfig,
    speed: ReplaySpeed,
) -> NativeGameplayResult<PreparedPracticeAttempt> {
    validate_launch_speed(current_launch, speed)?;
    prepare_attempt_inner(original, policy, current_launch, config, speed, false)
}

/// New sections inherit the actual retained rate, without rewriting the pinned
/// invocation that owns a later original restart.
pub fn prepare_attempt_with_runtime_speed(
    original: &BmsChart,
    policy: &ResolvedPlayPolicy,
    current_launch: &SessionLaunch,
    config: PracticeAttemptConfig,
    speed: ReplaySpeed,
) -> NativeGameplayResult<PreparedPracticeAttempt> {
    prepare_attempt_inner(original, policy, current_launch, config, speed, true)
}

pub(crate) fn validate_launch_speed(
    current_launch: &SessionLaunch,
    speed: ReplaySpeed,
) -> NativeGameplayResult<()> {
    let mut options = current_launch
        .args()
        .chunks_exact(2)
        .filter(|pair| pair[0] == "--speed");
    let selected = options
        .next()
        .map(|pair| pair[1].parse::<ReplaySpeed>())
        .transpose()?
        .unwrap_or(ReplaySpeed::new(Rate::NORMAL)?);
    if options.next().is_some() {
        return Err("practice retry invocation has duplicate speed options".into());
    }
    if selected != speed {
        return Err("practice selected speed differs from pinned retry invocation".into());
    }
    Ok(())
}

fn prepare_attempt_inner(
    original: &BmsChart,
    policy: &ResolvedPlayPolicy,
    current_launch: &SessionLaunch,
    config: PracticeAttemptConfig,
    speed: ReplaySpeed,
    runtime: bool,
) -> NativeGameplayResult<PreparedPracticeAttempt> {
    crate::live_speed::LiveSpeedSetup::new(
        config.start,
        beatkernel::time::Duration::ZERO,
        speed,
        config.end,
    )?;
    if config.start.as_nanos() < 0 || config.end.is_some_and(|end| end <= config.start) {
        return Err("practice requires a nonnegative start and a strictly later end".into());
    }
    // Reject unsupported live modes even if the caller omitted UI guards.
    if current_launch.args().chunks_exact(2).any(|pair| {
        matches!(
            pair[0].as_str(),
            "--replay" | "--mp-host" | "--mp-join" | "--mp-webtransport" | "--mp-room"
        )
    }) {
        return Err("retained practice requires live nonnetwork playback".into());
    }
    let next_launch = if runtime {
        current_launch.retry_with_speed(speed)?
    } else {
        current_launch.retry()?
    };
    let recording_path = next_launch
        .args()
        .chunks_exact(2)
        .find(|pair| pair[0] == "--record-replay")
        .map(|pair| PathBuf::from(&pair[1]));
    if recording_path.is_some() != config.capture_limits.is_some() {
        return Err("practice recording path and capture configuration differ".into());
    }
    let original_chart = original.source.compile()?;
    let excluded_objects = original_chart
        .objects()
        .iter()
        .filter(|object| object.time.start < config.start)
        .count();
    let excluded_crossing_holds = original_chart
        .objects()
        .iter()
        .filter(|object| {
            object.time.start < config.start
                && object.time.end.is_some_and(|end| end >= config.start)
        })
        .count();
    // Retains BPM/STOP, mine/invisible identity and original object timestamps.
    let source = crate::section_start::source_at(original, config.start)?;
    let chart = source.source.compile()?;
    let judge_config = NativeJudgeConfig {
        early: policy.judge().max_early().as_nanos(),
        late: policy.completion_late().as_nanos(),
        offset: policy.judge().input_offset().as_nanos(),
        preroll: 0,
        output: config.domain,
        end: config.end,
    };
    let judge = judge_config.judge_with_policy(&source, chart, policy)?;
    let capture = crate::native_judge::prepare_section_capture_for_policy_with_speed(
        &source,
        &judge,
        policy,
        config.domain,
        config.start,
        config.chart_seed,
        config.end,
        config.capture_limits,
        speed,
    )?;
    Ok(PreparedPracticeAttempt {
        source,
        judge,
        gauge: BmsGauge::new(policy.gauge().try_copy()?),
        score: ScoreSummary::default(),
        judgments: policy.judgments().cloned(),
        timing: policy.timing().cloned(),
        selection: policy.selection(),
        original_gauge: OriginalGaugeContext::from_source(original),
        capture,
        config,
        next_launch,
        recording_path,
        excluded_objects,
        excluded_crossing_holds,
        speed,
    })
}

#[cfg(test)]
#[path = "practice_session_fixtures.rs"]
mod fixtures;
