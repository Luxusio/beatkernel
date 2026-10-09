//! Common owner-thread queue/BGM/mixer composition; no native device operations.
use crate::{
    bgm::{BgmConfig, BgmFeedError, BgmFeedReport, BgmFeeder},
    input_sounds::InputSoundPlan,
    mine_sounds::MineSoundPlan,
    native_gameplay::NativeGameplayResult,
    offline::OwnedStopEvidence,
    PreparedBms,
};
use beatkernel::{
    audio::{
        command_queue, command_queue_with_start_gate, AudioCommand, AudioLimits, CommandProducer,
        CommandPushError, Mixer, MixerConfig, RenderReport, SampleBank,
    },
    runtime::{hazard_sound::HazardSoundTimeline, input_sound::InputSoundTimeline},
    time::{ClockPoint, Duration, Timestamp},
};

/// Keeps the original native mixer counters when Stop ownership is insufficient.
#[derive(Debug)]
pub(crate) struct NativeStopEvidenceError {
    pub(crate) report: RenderReport,
    pub(crate) admitted_stops: u64,
}
impl std::fmt::Display for NativeStopEvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "native unknown Stops {} exceed admitted {} or applied commands {}",
            self.report.counters.unknown_stops,
            self.admitted_stops,
            self.report.counters.commands_applied,
        )
    }
}
impl std::error::Error for NativeStopEvidenceError {}

/// Queue admission permits only its own cumulative unknown-Stop observations.
/// Clock, grid and presentation authority stay with the existing native owners.
pub(crate) fn validate_stop_evidence(
    rendered: Option<RenderReport>,
    evidence: &OwnedStopEvidence,
) -> NativeGameplayResult<()> {
    if let Some(report) = rendered {
        if !evidence.permits_unknown_stops(report.counters.unknown_stops)
            || report.counters.unknown_stops > report.counters.commands_applied
        {
            return Err(Box::new(NativeStopEvidenceError {
                report,
                admitted_stops: evidence.admitted_stops(),
            }));
        }
    }
    Ok(())
}

/// Finite output is sealed by the real immutable endpoint. Retained BGM voices
/// cannot play again, but every queued command must actually have been applied.
/// NativeEnd's endpoint/presentation proof remains a separate required guard.
pub(crate) fn finite_terminal_output_ready(
    bgm: BgmFeedReport,
    rendered: Option<RenderReport>,
    admitted_commands: u64,
) -> bool {
    admitted_commands != u64::MAX
        && bgm.remaining == 0
        && bgm.outstanding == 0
        && rendered.is_some_and(|report| {
            report.paused
                && report.playback_end_physical_frame.is_some()
                && report.pending_commands == 0
                && report.counters.commands_consumed == admitted_commands
                && report.counters.commands_applied == admitted_commands
        })
}

/// Finite playback needs a real block after the latest Stop queue admission.
#[derive(Default)]
pub(crate) struct NativeStopBarrier {
    admitted_stops: u64,
    after_frame: Option<u64>,
}
impl NativeStopBarrier {
    pub(crate) fn observe(
        &mut self,
        evidence: &OwnedStopEvidence,
        rendered: Option<RenderReport>,
    ) -> NativeGameplayResult<bool> {
        if evidence.admitted_stops() != self.admitted_stops {
            self.admitted_stops = evidence.admitted_stops();
            self.after_frame = None;
        }
        if self.admitted_stops == 0 {
            return Ok(true);
        }
        let Some(report) = rendered.filter(|report| report.frames > 0) else {
            return Ok(false);
        };
        let end = report
            .start_frame
            .checked_add(u64::try_from(report.frames)?)
            .ok_or("native Stop render barrier overflow")?;
        match self.after_frame {
            Some(after) => Ok(report.start_frame >= after),
            None => {
                self.after_frame = Some(end);
                Ok(false)
            }
        }
    }
}
/// Validate native press sounds before moving PCM or starting output ownership.
/// Empty invisible sources retain the unconfigured legacy runtime path.
pub fn prepare_input_sounds(
    prepared: &PreparedBms,
) -> NativeGameplayResult<Option<InputSoundTimeline>> {
    if prepared.source.invisible.is_empty() {
        return Ok(None);
    }
    let plan = InputSoundPlan::prepare(
        &prepared.source,
        &prepared.sounds,
        &prepared.bgm_commands,
        beatkernel_bms::ParseOptions::default().max_objects,
    )?;
    for &sample in plan.samples() {
        if prepared.bank.get(sample).is_none() {
            return Err("native input sound sample is missing from PCM bank".into());
        }
    }
    Ok(Some(plan.timeline()))
}

/// Validates optional native WAV00 sounds after press voice preparation, before
/// moving PCM or starting output. Unused WAV00 requires no sample or voice.
pub fn prepare_mine_sounds(
    prepared: &PreparedBms,
    input_sounds: Option<&InputSoundTimeline>,
) -> NativeGameplayResult<Option<HazardSoundTimeline>> {
    if prepared.source.mines.is_empty() {
        return Ok(None);
    }
    let plan = MineSoundPlan::prepare(
        &prepared.source,
        &prepared.sounds,
        &prepared.bgm_commands,
        input_sounds,
        beatkernel_bms::ParseOptions::default().max_objects,
    )?;
    for &sample in plan.samples() {
        if prepared.bank.get(sample).is_none() {
            return Err("native mine sound sample is missing from PCM bank".into());
        }
    }
    Ok(plan.timeline())
}

pub const LIVE_COMMAND_RESERVE: usize = 1024;
/// Cold conservative BGM voice overlap on the actual output sample grid.
/// Empty programs reserve no voices; gameplay uses the remaining total budget.
pub fn required_bgm_overlap(
    bank: &SampleBank,
    commands: &[AudioCommand],
) -> NativeGameplayResult<usize> {
    let mut events = Vec::new();
    events.try_reserve_exact(
        commands
            .len()
            .checked_mul(2)
            .ok_or("BGM overlap capacity overflow")?,
    )?;
    let output_rate = i128::from(bank.format().sample_rate());
    for &command in commands {
        let AudioCommand::Play {
            sample, at, gain, ..
        } = command
        else {
            return Err(BgmFeedError::InvalidCommand(command).into());
        };
        if !gain.is_finite() || at.as_nanos() < 0 {
            return Err(BgmFeedError::InvalidCommand(command).into());
        }
        let pcm = bank
            .get(sample)
            .ok_or("BGM sample is missing from original bank")?;
        if pcm.frames() == 0 {
            continue;
        }
        let source_rate = i128::from(pcm.format().sample_rate());
        let product = i128::try_from(pcm.frames())?
            .checked_mul(output_rate)
            .ok_or("BGM frame span overflow")?;
        let frames = product / source_rate + i128::from(product % source_rate != 0);
        let nanos = frames
            .checked_mul(1_000_000_000)
            .ok_or("BGM duration overflow")?;
        let duration = nanos / output_rate + i128::from(nanos % output_rate != 0);
        let start = i128::from(at.as_nanos());
        events.push((start, 1i64));
        events.push((
            start.checked_add(duration).ok_or("BGM endpoint overflow")?,
            -1,
        ));
    }
    events.sort_unstable();
    let mut active = 0i64;
    let mut peak = 0i64;
    for (_, delta) in events {
        active += delta;
        peak = peak.max(active);
    }
    Ok(usize::try_from(peak)?)
}
#[derive(Clone, Copy, Debug)]
pub struct NativeAudioConfig {
    pub output_origin: ClockPoint,
    pub start: Timestamp,
    pub preroll: Duration,
    pub lookahead: Duration,
    pub voices: usize,
    pub max_render_frames: usize,
    pub playback_end_frame: Option<u64>,
    pub gated_start: bool,
}
pub struct PreparedNativeAudio {
    pub producer: CommandProducer,
    pub bgm: BgmFeeder,
    pub mixer: Mixer,
}

/// Explicit retained-program storage and original-song initial interval.
#[derive(Clone, Copy, Debug)]
pub struct NativePracticeAudioConfig {
    pub region: beatkernel::audio::PracticeRegion,
    pub limits: beatkernel::audio::PracticeLimits,
}

pub struct PreparedRetainedNativeAudio {
    pub audio: PreparedNativeAudio,
    pub practice: beatkernel::audio::PracticeController,
}

/// Cold composition for one retained native output. Gameplay capacity is
/// `config.voices`; program overlap slots are reserved in addition to it.
/// The original bank and original-song cue times are never section-sliced.
pub fn prepare_retained_audio(
    bank: SampleBank,
    original_commands: Vec<AudioCommand>,
    config: NativeAudioConfig,
    practice: NativePracticeAudioConfig,
) -> NativeGameplayResult<PreparedRetainedNativeAudio> {
    use beatkernel::audio::{practice_queue, PracticeCue, PreparedPracticeProgram};
    if config.playback_end_frame.is_some() {
        return Err("retained audio requires a program region, not a finite mixer endpoint".into());
    }
    if config.start.as_nanos() < 0 || config.preroll.as_nanos() < 0 {
        return Err("retained audio start and preroll must be nonnegative".into());
    }
    let anchor =
        i64::try_from(i128::from(config.start.as_nanos()) - i128::from(config.preroll.as_nanos()))?;
    if practice.region.start != Timestamp::from_nanos(anchor) {
        return Err(
            "retained audio region must start at the exact start-minus-preroll anchor".into(),
        );
    }
    let voices = config
        .voices
        .checked_add(practice.limits.max_overlap)
        .ok_or("retained audio voice capacity overflow")?;
    // A BGM-only caller may reserve the entire total voice budget for BGM.
    if config.voices != 0 {
        AudioLimits::new(
            AudioLimits::MAX_COMMANDS,
            config.voices,
            AudioLimits::MAX_COMMANDS,
            config.max_render_frames,
            AudioLimits::MAX_COMMANDS,
        )?;
    }
    let limits = AudioLimits::new(
        AudioLimits::MAX_COMMANDS,
        voices,
        AudioLimits::MAX_COMMANDS,
        config.max_render_frames,
        AudioLimits::MAX_COMMANDS,
    )?;
    let mut cues = Vec::new();
    cues.try_reserve_exact(original_commands.len())?;
    for command in original_commands {
        let AudioCommand::Play {
            voice,
            sample,
            at,
            gain,
        } = command
        else {
            return Err(BgmFeedError::InvalidCommand(command).into());
        };
        if at.as_nanos() < 0 {
            return Err(BgmFeedError::InvalidCommand(command).into());
        }
        cues.push(PracticeCue {
            voice,
            sample,
            at,
            gain,
        });
    }
    let program = PreparedPracticeProgram::new(&bank, cues, practice.limits)?;
    program.validate_region(practice.region)?;
    let (controller, endpoint) = practice_queue(&program)?;
    let format = bank.format();
    // Retain the legacy feeder interface for platform owners, but no original
    // BGM cue can be re-enqueued by a delayed owner-thread feed after a loop.
    let bgm = BgmFeeder::new(
        Vec::new(),
        BgmConfig {
            output_origin: config.output_origin,
            sample_rate: format.sample_rate(),
            preroll: config.preroll,
            lookahead: config.lookahead,
            max_pending: AudioLimits::MAX_COMMANDS - LIVE_COMMAND_RESERVE,
        },
    )?;
    let (mut producer, consumer) = if config.gated_start {
        command_queue_with_start_gate(AudioLimits::MAX_COMMANDS)
    } else {
        command_queue(AudioLimits::MAX_COMMANDS)
    }?;
    producer.set_scope(beatkernel::audio::CommandScope(1));
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            config.output_origin.domain,
            config.output_origin.timestamp,
            limits,
        ),
        bank,
        consumer,
    )?;
    mixer.install_practice(program, endpoint, practice.region)?;
    Ok(PreparedRetainedNativeAudio {
        audio: PreparedNativeAudio {
            producer,
            bgm,
            mixer,
        },
        practice: controller,
    })
}
/// Prepare against the actual PCM bank, retaining explicit callback capacity.
pub fn prepare_audio(
    bank: SampleBank,
    commands: Vec<AudioCommand>,
    config: NativeAudioConfig,
) -> NativeGameplayResult<PreparedNativeAudio> {
    let capacity = AudioLimits::MAX_COMMANDS;
    let limits = AudioLimits::new(
        capacity,
        config.voices,
        capacity,
        config.max_render_frames,
        capacity,
    )?;
    let format = bank.format();
    let commands = crate::section_start::relative_commands(commands, config.start)?;
    let commands = if let Some(end) = config.playback_end_frame {
        let mut retained = Vec::new();
        retained.try_reserve_exact(commands.len())?;
        for command in commands {
            let AudioCommand::Play { at, gain, .. } = command else {
                return Err(BgmFeedError::InvalidCommand(command).into());
            };
            let elapsed = i128::from(at.as_nanos()) + i128::from(config.preroll.as_nanos());
            if !gain.is_finite() || elapsed < 0 {
                return Err(BgmFeedError::InvalidCommand(command).into());
            }
            let mapped =
                i64::try_from(i128::from(config.output_origin.timestamp.as_nanos()) + elapsed)
                    .map_err(|_| BgmFeedError::Overflow)?;
            if crate::replay_audio::before_endpoint(
                Timestamp::from_nanos(mapped),
                config.output_origin,
                format.sample_rate(),
                Some(end),
            )? {
                // Keep relative time: the real feeder performs its usual single
                // origin/preroll mapping after this exact frame-bound check.
                retained.push(command);
            }
        }
        retained
    } else {
        commands
    };
    let mut bgm = BgmFeeder::new(
        commands,
        BgmConfig {
            output_origin: config.output_origin,
            sample_rate: format.sample_rate(),
            preroll: config.preroll,
            lookahead: config.lookahead,
            max_pending: capacity - LIVE_COMMAND_RESERVE,
        },
    )?;
    let (mut producer, consumer) = if config.gated_start {
        command_queue_with_start_gate(capacity)
    } else {
        command_queue(capacity)
    }?;
    bgm.feed(0, capacity - LIVE_COMMAND_RESERVE, |command| {
        producer.try_push(command)
    })?;
    let mut mixer_config = MixerConfig::new(
        format,
        config.output_origin.domain,
        config.output_origin.timestamp,
        limits,
    );
    if let Some(end) = config.playback_end_frame {
        mixer_config = mixer_config.with_playback_end_frame(end);
    }
    let mixer = Mixer::new(mixer_config, bank, consumer)?;
    Ok(PreparedNativeAudio {
        producer,
        bgm,
        mixer,
    })
}
/// Replenish from completed logical playback, including the retained finite end.
/// Ordinary paused/startup silence cannot advance BGM admission or retirement.
pub fn feed_rendered(
    bgm: &mut BgmFeeder,
    report: Option<RenderReport>,
    admit: impl FnMut(AudioCommand) -> Result<(), CommandPushError>,
) -> NativeGameplayResult<()> {
    if let Some(report) =
        report.filter(|report| !report.paused || report.playback_end_physical_frame.is_some())
    {
        let cursor = report
            .playback_start_frame
            .checked_add(u64::try_from(report.playback_frames)?)
            .ok_or("BGM render cursor overflow")?;
        bgm.feed(cursor, 256, admit)?;
    }
    Ok(())
}

#[cfg(test)]
mod fixtures {
    mod stop_evidence {
        include!("native_stop_evidence_fixtures.rs");
    }
    use super::*;
    use beatkernel::audio::*;
    use beatkernel::time::ClockDomainId;
    fn bank() -> SampleBank {
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = PcmLimits::new(64, 256, 1).unwrap();
        let mut bank = SampleBank::new(format, limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
        )
        .unwrap();
        bank
    }
    fn config() -> NativeAudioConfig {
        NativeAudioConfig {
            output_origin: ClockPoint {
                domain: ClockDomainId(2),
                timestamp: Timestamp::from_nanos(10_000_000),
            },
            start: Timestamp::from_nanos(20_000_000),
            preroll: Duration::from_nanos(3_000_000),
            lookahead: Duration::from_nanos(100_000_000),
            voices: 2,
            max_render_frames: 16,
            playback_end_frame: None,
            gated_start: false,
        }
    }
    fn play(ns: i64, gain: f32) -> AudioCommand {
        AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(ns),
            gain,
        }
    }
    fn practice_config(
        start: i64,
        preroll: i64,
        end: i64,
    ) -> (NativeAudioConfig, NativePracticeAudioConfig) {
        let mut audio = config();
        audio.start = Timestamp::from_nanos(start);
        audio.preroll = Duration::from_nanos(preroll);
        (
            audio,
            NativePracticeAudioConfig {
                region: PracticeRegion::new(
                    Timestamp::from_nanos(start - preroll),
                    Timestamp::from_nanos(end),
                    false,
                )
                .unwrap(),
                limits: PracticeLimits::new(8, 2, 64, 4, 8).unwrap(),
            },
        )
    }
    fn original_cues() -> Vec<AudioCommand> {
        vec![
            AudioCommand::Play {
                voice: VoiceId(101),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            },
            AudioCommand::Play {
                voice: VoiceId(102),
                sample: SampleId(1),
                at: Timestamp::from_nanos(1_000_000),
                gain: 1.0,
            },
        ]
    }
    #[test]
    fn retained_overlap_reserves_output_quantized_tails_and_releases_equal_endpoints() {
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = PcmLimits::new(8, 128, 1).unwrap();
        let mut bank = SampleBank::new(format, limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(AudioFormat::new(1500, 1).unwrap(), vec![0.5], limits).unwrap(),
        )
        .unwrap();
        // One source frame lasts 2/3 ms, but occupies one full output frame.
        assert_eq!(
            required_bgm_overlap(&bank, &[play(0, 1.0), play(750_000, 1.0)]).unwrap(),
            2
        );
        assert_eq!(
            required_bgm_overlap(&bank, &[play(0, 1.0), play(1_000_000, 1.0)]).unwrap(),
            1
        );
        assert_eq!(required_bgm_overlap(&bank, &[]).unwrap(), 0);
        assert!(required_bgm_overlap(&bank, &[play(-1, 1.0)]).is_err());
        assert!(required_bgm_overlap(&bank, &[play(0, f32::NAN)]).is_err());
    }
    #[test]
    fn retained_bgm_only_uses_total_budget_without_a_hidden_gameplay_slot() {
        let (mut config, mut practice) = practice_config(0, 0, 4_000_000);
        config.voices = 0;
        practice.limits = PracticeLimits::new(8, 2, 64, 4, 8).unwrap();
        assert_eq!(required_bgm_overlap(&bank(), &original_cues()).unwrap(), 2);
        let mut prepared =
            prepare_retained_audio(bank(), original_cues(), config, practice).unwrap();
        let mut output = [0.; 4];
        prepared.audio.mixer.render(&mut output).unwrap();
        assert_eq!(output, [0.25, 0.75, 0.5, 0.]);
        practice.limits = PracticeLimits::new(8, 1, 64, 4, 8).unwrap();
        assert!(prepare_retained_audio(bank(), original_cues(), config, practice).is_err());
    }
    #[test]
    fn retained_original_multicue_backward_request_uses_same_mixer_and_explicit_scope() {
        let (config, practice) = practice_config(1_000_000, 0, 4_000_000);
        let mut retained =
            prepare_retained_audio(bank(), original_cues(), config, practice).unwrap();
        assert_eq!(retained.audio.producer.scope(), CommandScope(1));
        assert_eq!(retained.audio.bgm.report().remaining, 0);
        let mut first = [9.; 3];
        retained.audio.mixer.render(&mut first).unwrap();
        assert_eq!(first, [0.75, 0.5, 0.]);
        let started = retained.practice.try_pop_receipt().unwrap();
        assert_eq!(started.applied_song_time, Timestamp::from_nanos(1_000_000));
        retained
            .practice
            .try_request(PracticeRequest {
                id: 1,
                expected_generation: 1,
                at_playback_frame: 3,
                region: PracticeRegion::new(
                    Timestamp::ZERO,
                    Timestamp::from_nanos(4_000_000),
                    false,
                )
                .unwrap(),
            })
            .unwrap();
        let mut second = [9.; 4];
        retained.audio.mixer.render(&mut second).unwrap();
        assert_eq!(second, [0.25, 0.75, 0.5, 0.]);
        let applied = retained.practice.try_pop_receipt().unwrap();
        assert_eq!(applied.generation, 2);
        assert_eq!(applied.physical_frame, 3);
        assert_eq!(applied.playback_frame, 3);
        assert_eq!(applied.applied_song_time, Timestamp::ZERO);
        // Producer identity never silently follows the callback generation.
        assert_eq!(retained.audio.producer.scope(), CommandScope(1));
    }
    #[test]
    fn retained_negative_preroll_anchor_and_start_gate_preserve_original_pcm() {
        let (mut config, practice) = practice_config(0, 2_000_000, 4_000_000);
        config.gated_start = true;
        let mut retained =
            prepare_retained_audio(bank(), original_cues(), config, practice).unwrap();
        let mut silence = [9.; 2];
        retained.audio.mixer.render(&mut silence).unwrap();
        assert_eq!(silence, [0., 0.]);
        assert_eq!(
            retained.practice.try_pop_receipt(),
            Err(PracticeError::Empty)
        );
        retained.audio.producer.schedule_start_at(2).unwrap();
        let mut output = [9.; 6];
        retained.audio.mixer.render(&mut output).unwrap();
        assert_eq!(output, [0., 0., 0.25, 0.75, 0.5, 0.]);
        let started = retained.practice.try_pop_receipt().unwrap();
        assert_eq!(started.applied_song_time, Timestamp::from_nanos(-2_000_000));
        assert_eq!(started.physical_frame, 2);
        assert_eq!(started.playback_frame, 0);
    }
    #[test]
    fn retained_cold_validation_never_drops_cues_or_accepts_finite_fence() {
        let (config, practice) = practice_config(0, 0, 4_000_000);
        let mut finite = config;
        finite.playback_end_frame = Some(4);
        assert!(prepare_retained_audio(bank(), original_cues(), finite, practice).is_err());
        let mut wrong_anchor = practice;
        wrong_anchor.region.start = Timestamp::from_nanos(1);
        assert!(prepare_retained_audio(bank(), original_cues(), config, wrong_anchor).is_err());
        let mut insufficient = practice;
        insufficient.limits.max_overlap = 1;
        assert!(prepare_retained_audio(bank(), original_cues(), config, insufficient).is_err());
        insufficient = practice;
        insufficient.limits.max_cues = 1;
        assert!(prepare_retained_audio(bank(), original_cues(), config, insufficient).is_err());
        let mut full = config;
        full.voices = AudioLimits::MAX_VOICES;
        assert!(prepare_retained_audio(bank(), original_cues(), full, practice).is_err());
        let mut cues = original_cues();
        cues[0] = AudioCommand::Stop {
            voice: VoiceId(101),
            at: Timestamp::ZERO,
        };
        assert!(prepare_retained_audio(bank(), cues, config, practice).is_err());
        let mut cues = original_cues();
        if let AudioCommand::Play { gain, .. } = &mut cues[0] {
            *gain = f32::NAN;
        }
        assert!(prepare_retained_audio(bank(), cues, config, practice).is_err());
    }
    fn render(parts: &[usize]) -> Vec<f32> {
        let mut prepared = prepare_audio(bank(), vec![play(22_000_000, 1.0)], config()).unwrap();
        let mut output = Vec::new();
        for &frames in parts {
            let mut block = vec![9.0; frames];
            prepared.mixer.render(&mut block).unwrap();
            output.extend(block);
        }
        output
    }
    #[test]
    fn section_preroll_nonzero_origin_and_partitions_preserve_exact_onset() {
        let expected = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.25, 0.5, 0.0, 0.0];
        assert_eq!(render(&[9]), expected);
        assert_eq!(render(&[1, 3, 2, 3]), expected);
        let prepared = prepare_audio(bank(), vec![play(22_000_000, 1.0)], config()).unwrap();
        assert_eq!(prepared.bgm.config().sample_rate, 1000);
        assert_eq!(prepared.bgm.config().output_origin, config().output_origin);
    }
    #[test]
    fn exact_render_capacity_and_validation_are_preserved() {
        for frames in [0, AudioLimits::MAX_RENDER_FRAMES + 1] {
            let mut cfg = config();
            cfg.max_render_frames = frames;
            assert!(prepare_audio(bank(), Vec::new(), cfg).is_err());
        }
        let mut cfg = config();
        cfg.voices = 0;
        assert!(prepare_audio(bank(), Vec::new(), cfg).is_err());
        cfg = config();
        cfg.max_render_frames = 3;
        let mut prepared = prepare_audio(bank(), vec![play(22_000_000, 1.0)], cfg).unwrap();
        let mut invalid = [9.0; 4];
        assert!(prepared.mixer.render(&mut invalid).is_err());
        assert_eq!(invalid, [9.0; 4]);
        assert_eq!(prepared.mixer.frame_cursor(), 0);
        assert!(prepared.mixer.render(&mut [0.0; 3]).is_ok());
    }
    #[test]
    fn held_gate_and_finite_fence_use_playback_grid_and_cannot_resume_past_end() {
        for end in [0, 2] {
            let mut cfg = config();
            cfg.start = Timestamp::ZERO;
            cfg.preroll = Duration::ZERO;
            cfg.gated_start = true;
            cfg.playback_end_frame = Some(end);
            let mut prepared = prepare_audio(bank(), vec![play(0, 1.0)], cfg).unwrap();
            let held = prepared.mixer.render(&mut [9.0; 2]).unwrap();
            assert_eq!(held.playback_frames, 0);
            assert_eq!(held.counters.commands_consumed, 0);
            prepared.producer.schedule_start_at(3).unwrap();
            let mut output = [9.0; 6];
            let report = prepared.mixer.render(&mut output).unwrap();
            assert_eq!(
                output,
                if end == 0 {
                    [0.0; 6]
                } else {
                    [0.0, 0.25, 0.5, 0.0, 0.0, 0.0]
                }
            );
            assert_eq!(report.playback_frames, end as usize);
            assert_eq!(report.playback_end_physical_frame, Some(3 + end));
            assert_eq!(
                prepared.producer.applied_start_frame(),
                if end == 0 { None } else { Some(3) }
            );
            prepared.producer.request_pause(false);
            let mut suffix = [9.0; 2];
            let latest = prepared.mixer.render(&mut suffix).unwrap();
            assert_eq!(suffix, [0.0; 2]);
            assert_eq!(latest.playback_frames, 0);
            assert_eq!(latest.playback_end_physical_frame, Some(3 + end));
        }
    }
    #[test]
    fn initial_dense_bgm_reserves_exact_live_credit_and_fifo_replacement_order() {
        let mut cfg = config();
        cfg.start = Timestamp::ZERO;
        cfg.preroll = Duration::ZERO;
        let count = AudioLimits::MAX_COMMANDS - LIVE_COMMAND_RESERVE;
        let mut commands = vec![play(0, 0.5); count];
        commands.push(play(0, 2.0));
        let mut prepared = prepare_audio(bank(), commands, cfg).unwrap();
        assert_eq!(prepared.bgm.report().total_admitted, count);
        assert_eq!(prepared.bgm.report().remaining, 1);
        for _ in 0..LIVE_COMMAND_RESERVE {
            prepared
                .producer
                .try_push(AudioCommand::Stop {
                    voice: VoiceId(9),
                    at: cfg.output_origin.timestamp,
                })
                .unwrap();
        }
        assert!(prepared.producer.try_push(play(0, 1.0)).is_err());
        let mut output = [9.0];
        prepared.mixer.render(&mut output).unwrap();
        assert_eq!(output, [0.125]);
        let mut ordinary = prepare_audio(bank(), vec![play(0, 0.5), play(0, 2.0)], cfg).unwrap();
        let mut output = [9.0; 2];
        ordinary.mixer.render(&mut output).unwrap();
        assert_eq!(output, [0.5, 1.0]);
    }
    fn feeder() -> BgmFeeder {
        BgmFeeder::new(
            vec![play(0, 1.0), play(1_000_000, 1.0)],
            BgmConfig {
                output_origin: config().output_origin,
                sample_rate: 1000,
                preroll: Duration::ZERO,
                lookahead: Duration::from_nanos(10_000_000),
                max_pending: 4,
            },
        )
        .unwrap()
    }
    fn report() -> RenderReport {
        let mut prepared = prepare_audio(bank(), Vec::new(), config()).unwrap();
        prepared.mixer.render(&mut [0.0]).unwrap()
    }
    #[test]
    fn rendered_feed_suppresses_pause_and_uses_logical_cursor_not_physical_displacement() {
        let mut bgm = feeder();
        feed_rendered(&mut bgm, None, |_| panic!("no report admits nothing")).unwrap();
        let mut report = report();
        report.paused = true;
        feed_rendered(&mut bgm, Some(report), |_| panic!("paused admits nothing")).unwrap();
        assert_eq!(bgm.report().total_admitted, 0);
        report.paused = false;
        report.start_frame = 500;
        report.frames = 50;
        report.playback_start_frame = 0;
        report.playback_frames = 0;
        let mut admitted = Vec::new();
        feed_rendered(&mut bgm, Some(report), |command| {
            admitted.push(command);
            Ok(())
        })
        .unwrap();
        assert_eq!(admitted.len(), 2);
        report.playback_start_frame = u64::MAX;
        report.playback_frames = 1;
        assert!(feed_rendered(&mut bgm, Some(report), |_| panic!(
            "overflow before admission"
        ))
        .is_err());
        assert_eq!(bgm.report().total_admitted, 2);
        report.playback_start_frame = 2;
        report.playback_frames = 0;
        feed_rendered(&mut bgm, Some(report), |_| Ok(())).unwrap();
        report.playback_start_frame = 1;
        assert!(feed_rendered(&mut bgm, Some(report), |_| Ok(())).is_err());
    }
    #[test]
    fn feed_failure_retains_actual_admitted_prefix_for_retry() {
        let mut bgm = feeder();
        let report = RenderReport {
            playback_frames: 0,
            ..report()
        };
        let (mut producer, mut consumer) = command_queue(1).unwrap();
        assert!(
            feed_rendered(&mut bgm, Some(report), |command| producer.try_push(command)).is_err()
        );
        assert_eq!(bgm.report().total_admitted, 1);
        assert_eq!(bgm.report().remaining, 1);
        assert_eq!(
            consumer.try_pop().unwrap().at(),
            config().output_origin.timestamp
        );
        feed_rendered(&mut bgm, Some(report), |command| producer.try_push(command)).unwrap();
        assert_eq!(bgm.report().remaining, 0);
        assert_eq!(
            consumer.try_pop().unwrap().at(),
            Timestamp::from_nanos(11_000_000)
        );
    }
}
