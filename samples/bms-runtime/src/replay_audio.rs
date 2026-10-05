//! Song-time audio planning from actual reconstructed BMS judge results.

use crate::{
    PreparedBms,
    bgm::BgmFeeder,
    gauge::BmsGauge,
    input_sounds::InputSoundPlan,
    mine_sounds::MineSoundPlan,
    offline::OwnedStopEvidence,
    practice::PracticeStart,
    practice_loop::PracticeLoop,
    replay_playback::{
        decode_section_setup, reconstruct, reconstruct_section, validate_section_setup,
        validate_setup,
    },
};
use beatkernel::{
    audio::{AudioCommand, RenderReport, SampleId},
    judge::{JudgeEvent, JudgeOutcome},
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, ReplayFile},
    },
    runtime::GameplaySoundStop,
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
    completed_render_cursor_with_admitted_stops(report, 0)
}

/// Checks a cursor using only Stops whose actual feeder callbacks succeeded.
/// Native callers must use the same retained feeder backed by their producer.
/// Admission is not execution or presentation evidence; remote owners still
/// require their acknowledged Stop ledger instead of this callback contract.
pub fn completed_render_cursor_for_feeder(
    report: &RenderReport,
    feeder: &BgmFeeder,
) -> Result<u64, ReplayAudioError> {
    let admitted =
        u64::try_from(feeder.admitted_stops()).map_err(|_| ReplayAudioError::Overflow)?;
    completed_render_cursor_with_admitted_stops(report, admitted)
}

/// Shared strict cursor checks with this owner's actual accepted Stop evidence.
pub(crate) fn completed_render_cursor_with_stops(
    report: &RenderReport,
    stops: &OwnedStopEvidence,
) -> Result<u64, ReplayAudioError> {
    completed_render_cursor_with_admitted_stops(report, stops.admitted_stops())
}

fn completed_render_cursor_with_admitted_stops(
    report: &RenderReport,
    admitted_stops: u64,
) -> Result<u64, ReplayAudioError> {
    let counters = report.counters;
    if counters.late_commands != 0
        || counters.pending_full != 0
        || counters.voice_full != 0
        || counters.unknown_samples != 0
        || counters.unknown_stops > admitted_stops
        || counters.unknown_stops > counters.commands_applied
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
    /// Stable chronological output-domain Play and gameplay-failure Stop commands.
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
    // Keep one pristine actual judge when sound selection needs its ownership
    // or hazard reports. Reconstruction still owns complete results and hash.
    let mut selection_judge = None;
    let input_sounds = if prepared.source.invisible.is_empty() {
        None
    } else {
        selection_judge = Some(if allow_finite {
            validate_section_setup(&prepared.source, &file, limits)?
        } else {
            validate_setup(&prepared.source, &file, limits)?
        });
        let plan = InputSoundPlan::prepare(
            &prepared.source,
            &prepared.sounds,
            &prepared.bgm_commands,
            beatkernel_bms::ParseOptions::default().max_objects,
        )?;
        for &sample in plan.samples() {
            if prepared.bank.get(sample).is_none() {
                return Err(ReplayAudioError::MissingSample(sample).into());
            }
        }
        Some(plan.timeline())
    };
    let hazard_sounds = if prepared.source.mines.is_empty() {
        None
    } else {
        let plan = MineSoundPlan::prepare(
            &prepared.source,
            &prepared.sounds,
            &prepared.bgm_commands,
            input_sounds.as_ref(),
            beatkernel_bms::ParseOptions::default().max_objects,
        )?;
        if selection_judge.is_none() {
            selection_judge = Some(if allow_finite {
                validate_section_setup(&prepared.source, &file, limits)?
            } else {
                validate_setup(&prepared.source, &file, limits)?
            });
        }
        for &sample in plan.samples() {
            if prepared.bank.get(sample).is_none() {
                return Err(ReplayAudioError::MissingSample(sample).into());
            }
        }
        plan.timeline()
    };
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
    if !prepared.source.mines.is_empty() {
        let mut judge = selection_judge.expect("mines prepared a pristine judge");
        let mut voices = Vec::new();
        let voice_count = prepared
            .sounds
            .len()
            .checked_add(
                input_sounds
                    .as_ref()
                    .map_or(0, |timeline| timeline.markers().len()),
            )
            .and_then(|count| {
                count.checked_add(
                    hazard_sounds
                        .as_ref()
                        .map_or(0, |timeline| timeline.bindings().len()),
                )
            })
            .ok_or(ReplayAudioError::Overflow)?;
        voices
            .try_reserve_exact(voice_count)
            .map_err(|_| ReplayAudioError::AllocationFailed)?;
        voices.extend(prepared.sounds.iter().map(|sound| sound.voice));
        if let Some(timeline) = &input_sounds {
            voices.extend(timeline.markers().iter().map(|marker| marker.voice));
        }
        if let Some(timeline) = &hazard_sounds {
            voices.extend(timeline.bindings().iter().map(|binding| binding.voice));
        }
        let mut stops = GameplaySoundStop::new(voices);
        let mut gauge = BmsGauge::default();
        for record in session.records() {
            let was_failed = gauge.snapshot().failure.is_some();
            let (results, press_command) = match &record.operation {
                ReplayOperation::Advance => (judge.advance_to(record.song_time)?, None),
                ReplayOperation::Input(input) => {
                    let fresh = input_sounds.is_some() && judge.is_fresh_press(input);
                    let results = judge.push_input(input, record.song_time)?;
                    let command = input_sounds.as_ref().and_then(|timeline| {
                        timeline.command_for_press(
                            input,
                            fresh,
                            record.song_time,
                            record.song_time,
                            &results,
                        )
                    });
                    (results, command)
                }
            };
            gauge.observe(&results, judge.hazard_events())?;
            if was_failed {
                // Legacy logs may continue after failure. Keep judging every
                // record for exact results/hash, without selecting more sounds.
                continue;
            }
            for event in &results {
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
                    stops.observe_admitted(at);
                }
            }
            // Only selected commands require output mapping. Marker timestamps
            // stay in the judge report; delayed operations schedule at their own
            // recorded song time, after that operation's normal and press sounds.
            for command in press_command
                .into_iter()
                .chain(judge.hazard_events().iter().filter_map(|event| {
                    hazard_sounds
                        .as_ref()
                        .and_then(|timeline| timeline.command_for(event, record.song_time))
                }))
            {
                let AudioCommand::Play {
                    voice,
                    sample,
                    gain,
                    ..
                } = command
                else {
                    unreachable!("sound selectors return Play commands");
                };
                let at = output_time(
                    i128::from(record.song_time.as_nanos()),
                    start,
                    output_origin,
                    preroll,
                )?;
                if !before_endpoint(at, output_origin, sample_rate, end)? {
                    continue;
                }
                scheduled
                    .try_reserve(1)
                    .map_err(|_| ReplayAudioError::AllocationFailed)?;
                scheduled.push((
                    at,
                    true,
                    scheduled.len(),
                    AudioCommand::Play {
                        voice,
                        sample,
                        at,
                        gain,
                    },
                ));
                stops.observe_admitted(at);
            }
            if gauge.snapshot().failure.is_some() && !stops.voices().is_empty() {
                if prepared.bgm_commands.iter().any(|command| match command {
                    AudioCommand::Play { voice, .. } => stops.voices().binary_search(voice).is_ok(),
                    _ => false,
                }) {
                    return Err(ReplayAudioError::InvalidConfiguration(
                        "gameplay failure Stop voice collides with BGM",
                    )
                    .into());
                }
                let at = output_time(
                    i128::from(record.song_time.as_nanos()),
                    start,
                    output_origin,
                    preroll,
                )?;
                // Success here means off-thread planning only. The owner must
                // still feed these commands and observe actual output evidence.
                if let Some(report) = stops.attempt(at, |_| Ok(())) {
                    if before_endpoint(report.at, output_origin, sample_rate, end)? {
                        scheduled
                            .try_reserve(report.commands.len())
                            .map_err(|_| ReplayAudioError::AllocationFailed)?;
                        for command in report.commands {
                            scheduled.push((report.at, true, scheduled.len(), command));
                        }
                    }
                }
            }
        }
        if judge.stable_hash()? != final_judge_hash {
            return Err(ReplayAudioError::InvalidConfiguration(
                "sound selection judge differs from reconstructed state",
            )
            .into());
        }
    } else {
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
        if let Some(timeline) = input_sounds {
            let mut judge = selection_judge.expect("press sounds prepared a pristine judge");
            // One forward pass preserves contact/button freshness and equal-time
            // operation order without rebuilding through repeated replay seeks.
            for record in session.records() {
                let input = match &record.operation {
                    ReplayOperation::Advance => {
                        judge.advance_to(record.song_time)?;
                        continue;
                    }
                    ReplayOperation::Input(input) => input,
                };
                let fresh = judge.is_fresh_press(input);
                let results = judge.push_input(input, record.song_time)?;
                let Some(AudioCommand::Play {
                    voice,
                    sample,
                    gain,
                    ..
                }) = timeline.command_for_press(
                    input,
                    fresh,
                    record.song_time,
                    record.song_time,
                    &results,
                )
                else {
                    continue;
                };
                let at = output_time(
                    i128::from(record.song_time.as_nanos()),
                    start,
                    output_origin,
                    preroll,
                )?;
                if !before_endpoint(at, output_origin, sample_rate, end)? {
                    continue;
                }
                scheduled
                    .try_reserve(1)
                    .map_err(|_| ReplayAudioError::AllocationFailed)?;
                scheduled.push((
                    at,
                    true,
                    scheduled.len(),
                    AudioCommand::Play {
                        voice,
                        sample,
                        at,
                        gain,
                    },
                ));
            }
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
