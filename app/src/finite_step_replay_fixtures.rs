//! Actual section PCM, captured operations, command receipts and Mixer evidence.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    bgm::BgmFeedError,
    competition::ScoreSummary,
    prepare_from_source,
    replay_audio::{plan_audio, plan_section_audio},
    replay_capture::LiveReplayCapture,
    replay_playback::reconstruct_section,
    replay_visual::ReplayVisual,
    section_start::{prepare_at, prepare_replay, prepare_section_replay},
    step_gameplay::StepGameplayError,
    step_replay::{StepReplay, StepReplayConfig, StepReplayError},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        RenderReport, SampleBank, SampleId, command_queue,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeWindow},
    replay::codec::{ReplayCodecLimits, ReplayFile, decode_replay, encode_replay},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{Rate, Transport},
};

const PREFIX: &str = "#BPM 60\n#00011:01\n#00112:01\n#00001:02\n#00101:02\n";
const SECTION: &str = "#BPM 60\n#00011:00010000\n#00012:00010000\n#00013:00000100\n#00001:02020000\n#00001:0000000200000000\n";

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn config() -> StepReplayConfig {
    StepReplayConfig {
        output_origin: point(22, 604_800_000_000_017),
        preroll: Duration::from_nanos(250_000_000),
        lookahead: Duration::from_nanos(2_000_000_000),
        max_pending: 8,
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65_536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn pcm_limits() -> PcmLimits {
    PcmLimits::new(4096, 16_384, 16).unwrap()
}
fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
    let size = u32::try_from(samples.len() * 2).unwrap();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
fn prepared(chart: &str, rate: u32, seed: u64) -> PreparedBms {
    let chart = format!("#VOLWAV 50\n#WAV01 key.wav\n#WAV02 music.wav\n{chart}");
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files
        .insert("pack/key.wav", wav(rate, &[8192, -4096, 2048, 1024]))
        .unwrap();
    let music = (1i16..=32).map(|value| value * 512).collect::<Vec<_>>();
    files.insert("pack/music.wav", wav(rate, &music)).unwrap();
    prepare_from_source(
        chart.as_bytes(),
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(rate, 1).unwrap(),
        pcm_limits(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        seed,
        None,
    )
    .unwrap()
}
#[derive(Clone, Copy)]
enum Operation {
    Advance(i64),
    Key(i64, u16, ButtonState),
}
impl Operation {
    fn song(self) -> i64 {
        match self {
            Self::Advance(song) | Self::Key(song, ..) => song,
        }
    }
}
struct Capture {
    file: ReplayFile,
    events: Vec<JudgeEvent>,
    score: ScoreSummary,
    hash: u64,
}
fn capture(
    chart: &str,
    rate: u32,
    seed: u64,
    start: i64,
    end: Option<i64>,
    offset: i64,
    operations: &[Operation],
) -> Capture {
    let (prepared, _) = prepare_at(
        prepared(chart, rate, seed),
        Timestamp::from_nanos(start),
        pcm_limits(),
    )
    .unwrap();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(7),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::from_nanos(offset),
    )
    .unwrap();
    let judge =
        JudgeEngine::new(prepared.compiled.chart, prepared.source.rules(), profile).unwrap();
    let mut capture = LiveReplayCapture::new_section(
        &judge,
        ClockDomainId(11),
        limits(),
        Timestamp::from_nanos(start),
        seed,
        end.map(Timestamp::from_nanos),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings((0u16..3).map(|index| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4 + index),
        game_control: GameControlId(u32::from(0x11 + index)),
    }))
    .unwrap();
    let (producer, _consumer) = command_queue(64).unwrap();
    let host_origin = point(11, 10_000_000_000);
    let song_origin = start - 250_000_000;
    let mut runtime = Runtime::new(
        ClockDomainId(11),
        ClockDomainId(22),
        Transport::new(
            host_origin.timestamp,
            Timestamp::from_nanos(song_origin),
            Rate::NORMAL,
        ),
        bindings,
        judge,
        producer,
        prepared.sounds,
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let mapper = AffineClockMapper::exact_offset(
        ClockPair {
            source: host_origin,
            target: point(22, 0),
        },
        ClockInterval {
            start: host_origin.timestamp,
            end: Timestamp::from_nanos(30_000_000_000),
        },
    )
    .unwrap();
    let mut events = Vec::new();
    let mut score = ScoreSummary::default();
    for (index, &operation) in operations.iter().enumerate() {
        let elapsed = operation.song() - song_origin;
        let host = point(11, host_origin.timestamp.as_nanos() + elapsed);
        let output = point(22, elapsed);
        let report = match operation {
            Operation::Advance(_) => runtime.advance_to(host, &mapper, output).unwrap(),
            Operation::Key(_, key, state) => runtime
                .process_input(
                    PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(DeviceId(77), host, index as u64),
                        control: PhysicalControlId::keyboard(key),
                        state,
                    }),
                    &mapper,
                    output,
                )
                .unwrap(),
        };
        assert!(report.judge_error.is_none());
        assert!(report.audio_failures.is_empty());
        capture.record_report(&report).unwrap();
        score.observe(&report.judge_events).unwrap();
        events.extend(report.judge_events);
    }
    let hash = runtime.judge().stable_hash().unwrap();
    let bytes = capture.into_bytes().unwrap();
    let file = decode_replay(&bytes, limits()).unwrap();
    assert_eq!(encode_replay(&file, limits()).unwrap(), bytes);
    Capture {
        file,
        events,
        score,
        hash,
    }
}
fn replay(
    chart: &str,
    rate: u32,
    seed: u64,
    file: &ReplayFile,
    chosen: StepReplayConfig,
) -> (StepReplay, SampleBank) {
    let prepared =
        prepare_section_replay(prepared(chart, rate, seed), file, limits(), pcm_limits()).unwrap();
    StepReplay::new(prepared, file.clone(), limits(), chosen).unwrap()
}
fn mixer(bank: SampleBank, chosen: StepReplayConfig, end: Option<u64>) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(64).unwrap();
    let mut config = MixerConfig::new(
        bank.format(),
        chosen.output_origin.domain,
        chosen.output_origin.timestamp,
        AudioLimits::new(64, 16, 64, 64, 64).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn render(mixer: &mut Mixer, frames: usize) -> (Vec<f32>, RenderReport) {
    let mut pcm = vec![0.0; frames];
    let report = mixer.render(&mut pcm).unwrap();
    (pcm, report)
}
fn deliver(
    owner: &mut StepReplay,
    producer: &mut CommandProducer,
    max: usize,
) -> Vec<AudioCommand> {
    let mut delivered = Vec::new();
    while let Some(batch) = owner.take_commands(max).unwrap() {
        for command in &batch.commands {
            producer.try_push(*command).unwrap();
        }
        owner
            .acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        delivered.extend(batch.commands);
    }
    delivered
}
fn presented(chosen: StepReplayConfig, relative: i64) -> ClockPoint {
    point(
        chosen.output_origin.domain.0,
        chosen.output_origin.timestamp.as_nanos() + relative,
    )
}
fn fenced(owner: &mut StepReplay) {
    let score = owner.score().clone();
    let song = owner.song_time();
    assert!(owner.failed());
    assert!(matches!(
        owner.take_commands(1),
        Err(StepReplayError::Failed)
    ));
    assert!(matches!(
        owner.observe_output(None, None),
        Err(StepReplayError::Failed)
    ));
    assert_eq!(owner.score(), &score);
    assert_eq!(owner.song_time(), song);
}

#[test]
fn section_pcm_and_equal_time_commands_keep_original_order_across_finite_render_partitions() {
    let seed = u64::MAX;
    let captured = capture(
        SECTION,
        8,
        seed,
        1_000_000_000,
        Some(1_500_000_000),
        0,
        &[
            Operation::Advance(750_000_000),
            Operation::Key(1_000_000_000, 4, ButtonState::Down),
            Operation::Key(1_000_000_000, 5, ButtonState::Down),
            Operation::Advance(1_500_000_000),
        ],
    );
    let chosen = config();
    let original = prepared(SECTION, 8, seed);
    let canonical = reconstruct_section(&original.source, captured.file.clone(), limits()).unwrap();
    assert_eq!(canonical.engine().stable_hash().unwrap(), captured.hash);
    let selected =
        prepare_section_replay(original, &captured.file, limits(), pcm_limits()).unwrap();
    assert_eq!(
        selected.bank.get(SampleId(3)).unwrap().samples(),
        &selected.bank.get(SampleId(2)).unwrap().samples()[8..],
        "crossing BGM copies the original PCM suffix once"
    );
    let plan = plan_section_audio(
        &selected,
        captured.file.clone(),
        limits(),
        chosen.output_origin,
        chosen.preroll,
    )
    .unwrap();
    assert_eq!(plan.final_judge_hash, captured.hash);
    assert_eq!(plan.judge_events, captured.events);
    assert_eq!(
        plan.commands.len(),
        4,
        "two in-section BGM voices and two actual hits; end-equality BGM is excluded"
    );
    let samples = plan
        .commands
        .iter()
        .map(|command| match command {
            AudioCommand::Play { sample, at, .. } => {
                assert_eq!(*at, presented(chosen, 250_000_000).timestamp);
                *sample
            }
            _ => panic!("audio plan contains a non-Play command"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        samples,
        [SampleId(3), SampleId(2), SampleId(1), SampleId(1)]
    );
    let (mut reference_producer, mut reference) = mixer(selected.bank, chosen, Some(6));
    for command in &plan.commands {
        reference_producer.try_push(*command).unwrap();
    }
    let (expected, _) = render(&mut reference, 9);
    let expected_literal = [
        0.0,
        0.0,
        (9 * 512 + 512 + 16384) as f32 / 65536.0,
        (10 * 512 + 2 * 512 - 8192) as f32 / 65536.0,
        (11 * 512 + 3 * 512 + 4096) as f32 / 65536.0,
        (12 * 512 + 4 * 512 + 2048) as f32 / 65536.0,
        0.0,
        0.0,
        0.0,
    ];
    assert_eq!(expected, expected_literal);
    for partitions in [vec![1, 1, 3, 4], vec![6, 3], vec![9]] {
        let (mut owner, bank) = replay(SECTION, 8, seed, &captured.file, chosen);
        assert_eq!(owner.end_ns(), Some(1_500_000_000));
        assert_eq!(owner.playback_end_frame(), Some(6));
        assert_eq!(owner.song_time(), Timestamp::from_nanos(750_000_000));
        let (mut producer, mut output) = mixer(bank, chosen, owner.playback_end_frame());
        assert_eq!(deliver(&mut owner, &mut producer, 2), plan.commands);
        let mut actual = Vec::new();
        let mut events = Vec::new();
        let mut elapsed_frames = 0;
        for frames in partitions {
            let (pcm, report) = render(&mut output, frames);
            elapsed_frames += frames;
            owner
                .observe_output(
                    Some(report),
                    Some(presented(chosen, elapsed_frames as i64 * 125_000_000)),
                )
                .unwrap();
            events.extend(owner.drain_events());
            assert!(deliver(&mut owner, &mut producer, 2).is_empty());
            if elapsed_frames >= 6 {
                assert!(
                    report.active_voices > 0,
                    "frozen music tails do not require idle completion"
                );
                assert_eq!(report.playback_end_physical_frame, Some(6));
            }
            actual.extend(pcm);
        }
        assert_eq!(actual, expected);
        assert_eq!(events, captured.events);
        assert_eq!(owner.score(), &captured.score);
        assert_eq!(owner.song_time(), Timestamp::from_nanos(1_500_000_000));
        assert!(owner.observe_output(None, None).unwrap());
    }
}

#[test]
fn rounded_exclusive_sound_targets_do_not_erase_judge_events_or_shortcut_the_presentation_fence() {
    let chart = "#BPM 80\n#00011:00010000\n#00001:00020000\n";
    let captured = capture(
        chart,
        3,
        0,
        0,
        Some(750_000_000),
        1,
        &[
            Operation::Advance(-100_000_000),
            Operation::Key(749_999_999, 4, ButtonState::Down),
            Operation::Advance(750_000_000),
        ],
    );
    let chosen = StepReplayConfig {
        preroll: Duration::from_nanos(100_000_000),
        ..config()
    };
    let original = prepared(chart, 3, 0);
    let plan = plan_section_audio(
        &original,
        captured.file.clone(),
        limits(),
        chosen.output_origin,
        chosen.preroll,
    )
    .unwrap();
    assert!(
        plan.commands.is_empty(),
        "both the equality BGM and the earlier input round to exclusive frame 3"
    );
    assert_eq!(plan.judge_events, captured.events);
    assert_eq!(plan.judge_events.len(), 1);
    assert!(matches!(
        plan.judge_events[0].outcome,
        JudgeOutcome::Hit {
            delta: Duration::ZERO,
            ..
        }
    ));
    assert_eq!(plan.final_judge_hash, captured.hash);
    let (mut owner, bank) = replay(chart, 3, 0, &captured.file, chosen);
    assert_eq!(owner.playback_end_frame(), Some(3));
    let (_producer, mut output) = mixer(bank, chosen, Some(3));
    assert!(owner.take_commands(8).unwrap().is_none());
    let (pcm, report) = render(&mut output, 4);
    assert_eq!(pcm, [0.0; 4]);
    assert!(
        !owner
            .observe_output(Some(report), Some(presented(chosen, 850_000_000)))
            .unwrap()
    );
    assert_eq!(owner.song_time(), Timestamp::from_nanos(750_000_000));
    assert_eq!(owner.drain_events(), captured.events);
    assert!(
        !owner
            .observe_output(None, Some(presented(chosen, 999_999_999)))
            .unwrap()
    );
    assert!(
        owner
            .observe_output(None, Some(presented(chosen, 1_000_000_000)))
            .unwrap()
    );
    assert_eq!(owner.score(), &captured.score);
    assert!(owner.drain_events().is_empty());
}

#[test]
fn finite_prefix_and_empty_logs_need_actual_marker_presentation_ack_and_retired_credits() {
    let chosen = config();
    let captured = capture(
        PREFIX,
        8,
        0,
        0,
        Some(1_000_000_000),
        0,
        &[Operation::Key(0, 4, ButtonState::Down)],
    );
    let (mut owner, bank) = replay(PREFIX, 8, 0, &captured.file, chosen);
    assert_eq!(owner.recorded_until(), Some(Timestamp::ZERO));
    let (mut producer, mut output) = mixer(bank, chosen, Some(10));
    assert!(!owner.observe_output(None, None).unwrap());
    let batch = owner.take_commands(8).unwrap().unwrap();
    assert_eq!(batch.commands.len(), 2);
    for command in &batch.commands {
        producer.try_push(*command).unwrap();
    }
    let (_, marker) = render(&mut output, 12);
    assert!(!owner.observe_output(Some(marker), None).unwrap());
    assert_eq!(
        owner.score().hits,
        0,
        "rendering cannot manufacture presentation"
    );
    assert!(
        !owner
            .observe_output(Some(marker), Some(presented(chosen, 1_500_000_000)))
            .unwrap()
    );
    assert_eq!(owner.drain_events(), captured.events);
    assert_eq!(owner.score().misses, 0);
    assert!(
        matches!(owner.take_commands(8), Err(StepReplayError::OutstandingBatch { sequence }) if sequence == batch.sequence)
    );
    owner.acknowledge(batch.sequence, 2, true).unwrap();
    assert!(
        !owner.observe_output(None, None).unwrap(),
        "remote admission alone does not retire feeder credits"
    );
    assert!(owner.take_commands(8).unwrap().is_none());
    assert!(owner.observe_output(None, None).unwrap());
    assert_eq!(owner.score(), &captured.score);
    assert_eq!(owner.song_time(), Timestamp::from_nanos(1_000_000_000));
    assert_eq!(owner.recorded_until(), Some(Timestamp::ZERO));
    let empty = capture(PREFIX, 8, 0, 0, Some(1_000_000_000), 0, &[]);
    let (mut owner, bank) = replay(PREFIX, 8, 0, &empty.file, chosen);
    let (_producer, mut output) = mixer(bank, chosen, Some(10));
    assert!(owner.take_commands(8).unwrap().is_none());
    let (_, before) = render(&mut output, 9);
    assert!(
        !owner
            .observe_output(Some(before), Some(presented(chosen, 1_125_000_000)))
            .unwrap()
    );
    let (_, marker) = render(&mut output, 1);
    assert!(!owner.observe_output(Some(marker), None).unwrap());
    assert!(
        owner
            .observe_output(None, Some(presented(chosen, 1_250_000_000)))
            .unwrap()
    );
    assert_eq!(owner.bgm_report().total_admitted, 0);
    assert_eq!(owner.recorded_until(), None);
    assert_eq!(owner.score(), &ScoreSummary::default());
    assert!(owner.drain_events().is_empty());
}

#[test]
fn explicit_section_consumers_preserve_unlimited_plans_and_legacy_consumers_refuse_finite_metadata()
{
    let chosen = config();
    let legacy = capture(
        PREFIX,
        8,
        0,
        0,
        None,
        0,
        &[Operation::Key(0, 4, ButtonState::Down)],
    );
    let old = prepare_replay(prepared(PREFIX, 8, 0), &legacy.file, limits(), pcm_limits()).unwrap();
    let new = prepare_section_replay(prepared(PREFIX, 8, 0), &legacy.file, limits(), pcm_limits())
        .unwrap();
    let old_plan = plan_audio(
        &old,
        legacy.file.clone(),
        limits(),
        chosen.output_origin,
        chosen.preroll,
    )
    .unwrap();
    let new_plan = plan_section_audio(
        &new,
        legacy.file.clone(),
        limits(),
        chosen.output_origin,
        chosen.preroll,
    )
    .unwrap();
    assert_eq!(old_plan.commands, new_plan.commands);
    assert_eq!(old_plan.judge_events, new_plan.judge_events);
    assert_eq!(old_plan.final_judge_hash, new_plan.final_judge_hash);
    let mut old_visual = ReplayVisual::new(&old.source, &legacy.file, limits()).unwrap();
    let mut new_visual = ReplayVisual::new_section(&new.source, &legacy.file, limits()).unwrap();
    assert_eq!(new_visual.end(), None);
    for song in [-250_000_000, 0, 0, 5_000_000_000] {
        assert_eq!(
            old_visual.advance_to(Timestamp::from_nanos(song)).unwrap(),
            new_visual.advance_to(Timestamp::from_nanos(song)).unwrap()
        );
    }
    let (mut owner, bank) = StepReplay::new(new, legacy.file, limits(), chosen).unwrap();
    assert_eq!(owner.end_ns(), None);
    assert_eq!(owner.playback_end_frame(), None);
    let (mut producer, mut output) = mixer(bank, chosen, None);
    let (mut reference_producer, mut reference) = mixer(old.bank, chosen, None);
    for command in old_plan.commands {
        reference_producer.try_push(command).unwrap();
    }
    assert_eq!(deliver(&mut owner, &mut producer, 8), new_plan.commands);
    let (actual, report) = render(&mut output, 40);
    let (expected, _) = render(&mut reference, 40);
    assert_eq!(actual, expected);
    assert!(
        actual[10..34].iter().any(|&sample| sample != 0.0),
        "unlimited music is not clipped to a default section"
    );
    assert!(
        !owner
            .observe_output(Some(report), Some(presented(chosen, 5_000_000_000)))
            .unwrap()
    );
    assert!(owner.take_commands(8).unwrap().is_none());
    let (_, later) = render(&mut output, 1);
    owner
        .observe_output(Some(later), Some(presented(chosen, 5_125_000_000)))
        .unwrap();
    let (_, later) = render(&mut output, 1);
    assert!(
        owner
            .observe_output(Some(later), Some(presented(chosen, 5_250_000_000)))
            .unwrap()
    );
    let finite = capture(PREFIX, 8, 0, 0, Some(1_000_000_000), 0, &[]);
    let source = prepared(PREFIX, 8, 0);
    assert!(ReplayVisual::new(&source.source, &finite.file, limits()).is_err());
    assert!(
        plan_audio(
            &source,
            finite.file.clone(),
            limits(),
            chosen.output_origin,
            chosen.preroll
        )
        .is_err()
    );
    assert!(prepare_replay(source, &finite.file, limits(), pcm_limits()).is_err());
}

#[test]
fn missing_execution_late_cues_and_partial_remote_admission_preserve_the_actual_prefix_and_fence() {
    let chosen = config();
    let captured = capture(
        PREFIX,
        8,
        0,
        0,
        Some(1_000_000_000),
        0,
        &[Operation::Key(0, 4, ButtonState::Down)],
    );
    let (mut owner, bank) = replay(PREFIX, 8, 0, &captured.file, chosen);
    let (mut producer, mut output) = mixer(bank, chosen, Some(10));
    let batch = owner.take_commands(8).unwrap().unwrap();
    producer.try_push(batch.commands[0]).unwrap();
    let error = owner.acknowledge(batch.sequence, 1, false).unwrap_err();
    assert!(
        matches!(error, StepReplayError::Acknowledgement(StepGameplayError::AudioRejected { batch: retained, admitted: 1 }) if retained == batch)
    );
    let (_, actual) = render(&mut output, 12);
    assert_eq!(actual.counters.commands_consumed, 1);
    assert_eq!(actual.counters.commands_applied, 1);
    assert_eq!(owner.score(), &ScoreSummary::default());
    fenced(&mut owner);
    let narrow = StepReplayConfig {
        max_pending: 1,
        ..chosen
    };
    let (mut owner, bank) = replay(PREFIX, 8, 0, &captured.file, narrow);
    let (mut producer, mut output) = mixer(bank, chosen, Some(10));
    assert_eq!(deliver(&mut owner, &mut producer, 1).len(), 1);
    let (_, rendered) = render(&mut output, 4);
    assert!(
        !owner
            .observe_output(Some(rendered), Some(presented(chosen, 500_000_000)))
            .unwrap()
    );
    assert_eq!(owner.drain_events(), captured.events);
    let error = owner.take_commands(1).unwrap_err();
    assert!(
        matches!(error, StepReplayError::Bgm { error: BgmFeedError::Late { target_frame: 2, rendered_frames: 4, .. }, report }
        if report.total_admitted == 1 && report.remaining == 1)
    );
    assert_eq!(owner.score(), &captured.score);
    fenced(&mut owner);
    let (mut owner, bank) = replay(PREFIX, 8, 0, &captured.file, chosen);
    let (mut producer, mut output) = mixer(bank, chosen, Some(10));
    let batch = owner.take_commands(8).unwrap().unwrap();
    let (_, marker) = render(&mut output, 12);
    assert!(
        !owner
            .observe_output(Some(marker), Some(presented(chosen, 1_500_000_000)))
            .unwrap()
    );
    for command in &batch.commands {
        producer.try_push(*command).unwrap();
    }
    owner
        .acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    assert!(owner.take_commands(8).unwrap().is_none());
    let (_, frozen) = render(&mut output, 1);
    assert_eq!(frozen.counters.commands_consumed, 0);
    assert_eq!(frozen.counters.commands_applied, 0);
    assert!(
        matches!(owner.observe_output(Some(frozen), None), Err(StepReplayError::Output { rendered: Some(actual), .. }) if actual == frozen),
        "queue ACK after the immutable fence cannot prove commands were ever rendered"
    );
    assert_eq!(owner.score(), &captured.score);
    fenced(&mut owner);
}

#[test]
fn invalid_finite_reports_and_presentation_history_do_not_commit_a_new_visual_prefix() {
    let chosen = config();
    let captured = capture(
        PREFIX,
        8,
        0,
        0,
        Some(1_000_000_000),
        0,
        &[Operation::Key(0, 4, ButtonState::Down)],
    );
    for case in 0..10 {
        let (mut owner, bank) = replay(PREFIX, 8, 0, &captured.file, chosen);
        let (mut producer, mut output) = mixer(bank, chosen, Some(10));
        deliver(&mut owner, &mut producer, 8);
        let (_, first) = render(&mut output, 4);
        assert!(
            !owner
                .observe_output(Some(first), Some(presented(chosen, 500_000_000)))
                .unwrap()
        );
        assert_eq!(owner.drain_events(), captured.events);
        assert!(
            !owner
                .observe_output(Some(first), Some(presented(chosen, 500_000_000)))
                .unwrap()
        );
        assert!(owner.drain_events().is_empty());
        let (_, mut bad) = render(&mut output, 8);
        let mut presentation = presented(chosen, 1_500_000_000);
        match case {
            0 => bad.playback_end_physical_frame = Some(11),
            1 => bad.paused = false,
            2 => bad.playback_frames += 1,
            3 => bad.counters.rendered_frames += 1,
            4 => presentation.domain = ClockDomainId(99),
            5 => bad.counters.commands_applied += 1,
            6 => {
                bad = first;
                bad.active_voices += 1;
            }
            7 => presentation = presented(chosen, 499_999_999),
            8 => {
                bad = output.render(&mut []).unwrap();
            }
            9 => {
                owner.observe_output(Some(bad), Some(presentation)).unwrap();
                let (_, later) = render(&mut output, 1);
                bad = later;
                bad.pending_commands += 1;
            }
            _ => unreachable!(),
        }
        let song = owner.song_time();
        let score = owner.score().clone();
        assert!(
            matches!(
                owner.observe_output(Some(bad), Some(presentation)),
                Err(StepReplayError::Output { .. }) | Err(StepReplayError::Audio { .. })
            ),
            "invalid finite evidence case {case}"
        );
        assert_eq!(owner.song_time(), song);
        assert_eq!(owner.score(), &score);
        assert!(owner.drain_events().is_empty());
        fenced(&mut owner);
    }
}
