//! Song-time audio planning from actual reconstructed BMS judge results.

use crate::{
    PreparedBms,
    practice::PracticeStart,
    practice_loop::PracticeLoop,
    replay_playback::{decode_section_setup, reconstruct, reconstruct_section},
};
use beatkernel::{
    audio::{AudioCommand, RenderReport, SampleId},
    judge::{JudgeEvent, JudgeOutcome},
    replay::codec::{ReplayCodecLimits, ReplayFile},
    time::{ClockPoint, Duration, Timestamp},
};
use std::{collections::BTreeMap, error::Error};

/// Invalid prepared audio setup or unrepresentable output mapping.
#[derive(Debug)]
pub enum ReplayAudioError {
    /// A required sound has no prepared PCM sample.
    MissingSample(SampleId),
    /// Configuration is invalid for this forward audio plan.
    InvalidConfiguration(&'static str),
    /// The wide output mapping is outside the scalar timestamp range.
    Overflow,
    /// Explicit plan storage allocation failed.
    AllocationFailed,
    /// Actual mixer execution rejected a command; retain its complete evidence.
    RejectedRender(RenderReport),
}
impl std::fmt::Display for ReplayAudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BMS replay audio: {self:?}")
    }
}
impl Error for ReplayAudioError {}

/// Checks actual mixer execution before accepting its completed output cursor.
///
/// Admission, submitted native frames and physical presentation are different
/// observations and cannot substitute for this successful RenderReport.
pub fn completed_render_cursor(report: &RenderReport) -> Result<u64, ReplayAudioError> {
    let counters = report.counters;
    if counters.late_commands != 0
        || counters.pending_full != 0
        || counters.voice_full != 0
        || counters.unknown_samples != 0
        || counters.unknown_stops != 0
        || counters.invalid_gains != 0
        || counters.invalid_rates != 0
        || counters.invalid_times != 0
    {
        return Err(ReplayAudioError::RejectedRender(*report));
    }
    report
        .start_frame
        .checked_add(u64::try_from(report.frames).map_err(|_| ReplayAudioError::Overflow)?)
        .ok_or(ReplayAudioError::Overflow)
}

/// Off-thread planned commands, distinct from queue/native execution.
#[derive(Debug)]
pub struct ReplayAudioPlan {
    /// Stable chronological output-domain Play commands.
    pub commands: Vec<AudioCommand>,
    /// Actual full-log judge results with original input provenance.
    pub judge_events: Vec<JudgeEvent>,
    /// Last recorded operation's unoffset song time; absent for an empty log.
    pub recorded_until: Option<Timestamp>,
    /// Actual complete logical judge hash after all recorded operations.
    pub final_judge_hash: u64,
    /// Explicit output domain and origin used to map these commands.
    pub output_origin: ClockPoint,
}

fn output_time(
    song: i128,
    start: Timestamp,
    origin: ClockPoint,
    preroll: Duration,
) -> Result<Timestamp, ReplayAudioError> {
    let elapsed = song - i128::from(start.as_nanos()) + i128::from(preroll.as_nanos());
    if elapsed < 0 {
        return Err(ReplayAudioError::InvalidConfiguration(
            "sound precedes output origin; increase explicit preroll",
        ));
    }
    let at = i128::from(origin.timestamp.as_nanos()) + elapsed;
    Ok(Timestamp::from_nanos(
        i64::try_from(at).map_err(|_| ReplayAudioError::Overflow)?,
    ))
}

pub(crate) fn section_end_frame(
    start: Timestamp,
    end: Timestamp,
    preroll: Duration,
    sample_rate: u32,
) -> Result<u64, String> {
    let start = PracticeStart::from_nanoseconds(start.as_nanos())?;
    let end = PracticeStart::from_nanoseconds(end.as_nanos())?;
    PracticeLoop::new(start, end)?.playback_end_frame(start, preroll, sample_rate)
}

pub(crate) fn before_endpoint(
    at: Timestamp,
    output_origin: ClockPoint,
    sample_rate: u32,
    end: Option<u64>,
) -> Result<bool, ReplayAudioError> {
    let Some(end) = end else { return Ok(true) };
    let nanos = i128::from(at.as_nanos()) - i128::from(output_origin.timestamp.as_nanos());
    let scaled = nanos
        .checked_mul(i128::from(sample_rate))
        .ok_or(ReplayAudioError::Overflow)?;
    let frame =
        scaled.div_euclid(1_000_000_000) + i128::from(scaled.rem_euclid(1_000_000_000) != 0);
    Ok(frame < i128::from(end))
}

/// Reconstructs the same judge and maps its selected sounds onto an explicit output.
///
/// This recovers operation song time from effective judge time by removing the
/// profile offset once. Native scheduling points and past queue failures were
/// not recorded, so this cannot reproduce their physical sound/delay semantics.
/// Empty logs have no BGM; prefixes admit no BGM beyond their last operation.
/// For section recordings supply original assets through `section_start::prepare_replay`
/// first; output scheduling removes the recorded start once, then adds preroll.
pub fn plan_audio(
    prepared: &PreparedBms,
    file: ReplayFile,
    limits: ReplayCodecLimits,
    output_origin: ClockPoint,
    preroll: Duration,
) -> Result<ReplayAudioPlan, Box<dyn Error>> {
    plan_with_section(prepared, file, limits, output_origin, preroll, false)
}

/// Plan finite or unlimited section audio, excluding every command whose rounded
/// execution frame reaches the immutable endpoint. Recorded judge results stay exact.
pub fn plan_section_audio(
    prepared: &PreparedBms,
    file: ReplayFile,
    limits: ReplayCodecLimits,
    output_origin: ClockPoint,
    preroll: Duration,
) -> Result<ReplayAudioPlan, Box<dyn Error>> {
    plan_with_section(prepared, file, limits, output_origin, preroll, true)
}

fn plan_with_section(
    prepared: &PreparedBms,
    file: ReplayFile,
    limits: ReplayCodecLimits,
    output_origin: ClockPoint,
    preroll: Duration,
    allow_finite: bool,
) -> Result<ReplayAudioPlan, Box<dyn Error>> {
    if preroll.as_nanos() < 0 {
        return Err(ReplayAudioError::InvalidConfiguration("negative preroll").into());
    }
    let session = if allow_finite {
        reconstruct_section(&prepared.source, file, limits)?
    } else {
        reconstruct(&prepared.source, file, limits)?
    };
    let setup = decode_section_setup(&session.header().options)?;
    let start = setup.start;
    let sample_rate = prepared.bank.format().sample_rate();
    let end = setup
        .end
        .map(|end| section_end_frame(start, end, preroll, sample_rate))
        .transpose()?;
    if session.engine().chart() != &prepared.compiled.chart {
        return Err(ReplayAudioError::InvalidConfiguration(
            "prepared chart differs from reconstructed chart",
        )
        .into());
    }
    let offset = session.engine().profile().input_offset().as_nanos();
    let recorded_until = session.records().last().map(|record| record.song_time);
    let final_judge_hash = session.engine().stable_hash()?;
    let mut bindings = BTreeMap::new();
    for sound in &prepared.sounds {
        if !sound.gain.is_finite() {
            return Err(ReplayAudioError::InvalidConfiguration("nonfinite sound gain").into());
        }
        if prepared.bank.get(sound.sample).is_none() {
            return Err(ReplayAudioError::MissingSample(sound.sample).into());
        }
        let group: &mut Vec<_> = bindings.entry((sound.object, sound.stage)).or_default();
        group
            .try_reserve(1)
            .map_err(|_| ReplayAudioError::AllocationFailed)?;
        group.push(sound);
    }
    // Keys are output time, background-before-hit, and original admission order.
    let mut scheduled = Vec::new();
    for command in &prepared.bgm_commands {
        let AudioCommand::Play {
            voice,
            sample,
            at: song,
            gain,
        } = *command
        else {
            return Err(ReplayAudioError::InvalidConfiguration("BGM is not Play").into());
        };
        if !gain.is_finite() {
            return Err(ReplayAudioError::InvalidConfiguration("nonfinite BGM gain").into());
        }
        if prepared.bank.get(sample).is_none() {
            return Err(ReplayAudioError::MissingSample(sample).into());
        }
        if recorded_until.is_none_or(|end| song > end) {
            continue;
        }
        let at = output_time(i128::from(song.as_nanos()), start, output_origin, preroll)?;
        if !before_endpoint(at, output_origin, sample_rate, end)? {
            continue;
        }
        scheduled
            .try_reserve(1)
            .map_err(|_| ReplayAudioError::AllocationFailed)?;
        scheduled.push((
            at,
            false,
            scheduled.len(),
            AudioCommand::Play {
                voice,
                sample,
                at,
                gain,
            },
        ));
    }
    for event in session.results() {
        if !matches!(event.outcome, JudgeOutcome::Hit { .. }) {
            continue;
        }
        let Some(group) = bindings.get(&(event.object, event.stage)) else {
            continue;
        };
        let song = i128::from(event.at.as_nanos()) - i128::from(offset);
        let at = output_time(song, start, output_origin, preroll)?;
        if !before_endpoint(at, output_origin, sample_rate, end)? {
            continue;
        }
        for sound in group {
            let command = sound
                .command_for(event, at)
                .expect("indexed sound binding matches this hit");
            scheduled
                .try_reserve(1)
                .map_err(|_| ReplayAudioError::AllocationFailed)?;
            scheduled.push((at, true, scheduled.len(), command));
        }
    }
    scheduled.sort_unstable_by_key(|&(at, is_hit, ordinal, _)| (at, is_hit, ordinal));
    let mut commands = Vec::new();
    commands
        .try_reserve_exact(scheduled.len())
        .map_err(|_| ReplayAudioError::AllocationFailed)?;
    commands.extend(scheduled.into_iter().map(|(_, _, _, command)| command));
    let mut judge_events = Vec::new();
    judge_events
        .try_reserve_exact(session.results().len())
        .map_err(|_| ReplayAudioError::AllocationFailed)?;
    judge_events.extend_from_slice(session.results());
    Ok(ReplayAudioPlan {
        commands,
        judge_events,
        recorded_until,
        final_judge_hash,
        output_origin,
    })
}
