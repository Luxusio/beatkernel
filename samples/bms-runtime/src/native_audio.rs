//! Common owner-thread queue/BGM/mixer composition; no native device operations.
use crate::{
    PreparedBms,
    bgm::{BgmConfig, BgmFeeder},
    input_sounds::InputSoundPlan,
    mine_sounds::MineSoundPlan,
    native_gameplay::NativeGameplayResult,
};
use beatkernel::{
    audio::{
        AudioCommand, AudioLimits, CommandProducer, CommandPushError, Mixer, MixerConfig,
        RenderReport, SampleBank, command_queue, command_queue_with_start_gate,
    },
    runtime::{input_sound::InputSoundTimeline, hazard_sound::HazardSoundTimeline},
    time::{ClockPoint, Duration, Timestamp},
};
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
    let mut bgm = BgmFeeder::new(
        crate::section_start::relative_commands(commands, config.start)?,
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
/// Replenish only from successful active logical playback, never physical silence.
pub fn feed_rendered(
    bgm: &mut BgmFeeder,
    report: Option<RenderReport>,
    admit: impl FnMut(AudioCommand) -> Result<(), CommandPushError>,
) -> NativeGameplayResult<()> {
    if let Some(report) = report.filter(|report| !report.paused) {
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
        assert!(
            feed_rendered(&mut bgm, Some(report), |_| panic!(
                "overflow before admission"
            ))
            .is_err()
        );
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
