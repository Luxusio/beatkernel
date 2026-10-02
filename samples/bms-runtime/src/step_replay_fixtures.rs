//! Canonical captured operations and actual Mixer output; no native or file I/O.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    bgm::BgmFeedError,
    competition::ScoreSummary,
    prepare_from_source,
    replay_audio::{ReplayAudioError, plan_audio},
    replay_capture::LiveReplayCapture,
    replay_playback::{decode_chart_setup, reconstruct},
    section_start::{prepare_at, prepare_replay},
    step_gameplay::StepGameplayError,
    step_replay::{StepReplay, StepReplayConfig, StepReplayError},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        QueuePushError, SampleBank, command_queue,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId,
        DeviceSelector, EventMeta, GameControlId, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeEvent, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::{ReplayCodecLimits, ReplayFile, decode_replay, encode_replay},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{Rate, Transport},
};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn config() -> StepReplayConfig {
    StepReplayConfig {
        output_origin: point(22, 604_800_000_000_017),
        preroll: Duration::from_nanos(250_000_000),
        lookahead: Duration::from_nanos(1_000_000_000),
        max_pending: 8,
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn pcm_limits() -> PcmLimits {
    PcmLimits::new(1024, 4096, 8).unwrap()
}
fn wav(samples: &[i16]) -> Vec<u8> {
    let size = u32::try_from(samples.len() * 2).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for value in samples {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}
fn prepared(lines: &str, seed: u64) -> PreparedBms {
    let chart = format!("#BPM 60\n#VOLWAV 50\n#WAV01 key.wav\n#WAV02 bgm.wav\n{lines}");
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files.insert("pack/key.wav", wav(&[8192, -4096])).unwrap();
    files
        .insert(
            "pack/bgm.wav",
            wav(&[
                1024, 2048, 3072, 4096, 5120, 6144, 7168, 8192, 9216, 10240, 11264, 12288,
            ]),
        )
        .unwrap();
    prepare_from_source(
        chart.as_bytes(),
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(8, 1).unwrap(),
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
fn captured(lines: &str, seed: u64, start: i64, operations: &[Operation]) -> Capture {
    let (prepared, _) = prepare_at(
        prepared(lines, seed),
        Timestamp::from_nanos(start),
        pcm_limits(),
    )
    .unwrap();
    let judge = JudgeEngine::new(
        prepared.compiled.chart,
        prepared.source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let host_origin = point(11, 10_000_000_000);
    let output_origin = point(22, 0);
    let song_origin = start - 250_000_000;
    let mut capture = LiveReplayCapture::new_at_with_chart_seed(
        &judge,
        host_origin.domain,
        limits(),
        Timestamp::from_nanos(start),
        seed,
    )
    .unwrap();
    let bindings = BindingMap::from_bindings((0u16..3).map(|index| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4 + index),
        game_control: GameControlId(u32::from(0x11 + index)),
    }))
    .unwrap();
    let (producer, _consumer) = command_queue(64).unwrap();
    let mut runtime = Runtime::new(
        host_origin.domain,
        output_origin.domain,
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
            target: output_origin,
        },
        ClockInterval {
            start: host_origin.timestamp,
            end: Timestamp::from_nanos(30_000_000_000),
        },
    )
    .unwrap();
    let mut score = ScoreSummary::default();
    let mut events = Vec::new();
    for (index, &operation) in operations.iter().enumerate() {
        let elapsed = operation.song() - song_origin;
        let host = point(11, host_origin.timestamp.as_nanos() + elapsed);
        let output = point(22, elapsed);
        let report = match operation {
            Operation::Advance(_) => runtime.advance_to(host, &mapper, output).unwrap(),
            Operation::Key(_, key, state) => {
                let mut meta = EventMeta::new(DeviceId(77), host, index as u64);
                meta.native = Some(NativeEventMeta {
                    backend: BackendId(7),
                    code: Some(42),
                    timestamp: Some(host),
                });
                runtime
                    .process_input(
                        PhysicalInputEvent::Button(ButtonEvent {
                            meta,
                            control: PhysicalControlId::keyboard(key),
                            state,
                        }),
                        &mapper,
                        output,
                    )
                    .unwrap()
            }
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
fn mixer(bank: SampleBank, chosen: StepReplayConfig, capacity: usize) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            bank.format(),
            chosen.output_origin.domain,
            chosen.output_origin.timestamp,
            AudioLimits::new(capacity, 16, 64, 64, capacity).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn deliver(
    replay: &mut StepReplay,
    producer: &mut CommandProducer,
    max: usize,
) -> Vec<AudioCommand> {
    let mut commands = Vec::new();
    while let Some(batch) = replay.take_commands(max).unwrap() {
        for command in &batch.commands {
            producer.try_push(*command).unwrap();
        }
        replay
            .acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        commands.extend(batch.commands);
    }
    commands
}
fn presented(chosen: StepReplayConfig, relative_ns: i64) -> ClockPoint {
    ClockPoint {
        domain: chosen.output_origin.domain,
        timestamp: chosen
            .output_origin
            .timestamp
            .checked_add(Duration::from_nanos(relative_ns))
            .unwrap(),
    }
}
fn assert_fenced(replay: &mut StepReplay) {
    let score = replay.score().clone();
    let song = replay.song_time();
    let pressed = replay.pressed_lanes();
    assert!(replay.failed());
    assert!(matches!(
        replay.take_commands(1),
        Err(StepReplayError::Failed)
    ));
    assert!(matches!(
        replay.acknowledge(1, 0, true),
        Err(StepReplayError::Failed)
    ));
    assert!(matches!(
        replay.observe_output(None, None),
        Err(StepReplayError::Failed)
    ));
    assert_eq!(replay.score(), &score);
    assert_eq!(replay.song_time(), song);
    assert_eq!(replay.pressed_lanes(), pressed);
}

#[test]
fn seeded_capture_reconstructs_identical_events_and_pcm_across_variable_render_blocks() {
    let lines = "#00011:0101\n#RANDOM 2\n#IF 1\n#00012:01\n#ELSE\n#00013:01\n#ENDIF\n#ENDRANDOM\n#00001:02020000\n#00101:02\n";
    let seed = u64::MAX;
    let original = prepared(lines, seed);
    let selected_lane = original
        .source
        .notes
        .iter()
        .find(|note| note.lane.channel() != 0x11)
        .unwrap()
        .lane
        .channel();
    let operations = [
        Operation::Advance(-250_000_000),
        Operation::Key(0, 4, ButtonState::Down),
        Operation::Key(0, u16::from(selected_lane - 0x11) + 4, ButtonState::Down),
        Operation::Key(0, 4, ButtonState::Up),
        Operation::Advance(1_000_000_000),
        Operation::Key(2_000_000_000, 4, ButtonState::Down),
        Operation::Advance(2_500_000_000),
    ];
    let capture = captured(lines, seed, 0, &operations);
    assert_eq!(
        decode_chart_setup(&capture.file.header.options).unwrap().2,
        seed
    );
    assert_eq!(
        capture.file.records[0].song_time,
        Timestamp::from_nanos(-250_000_000)
    );
    let canonical = reconstruct(&original.source, capture.file.clone(), limits()).unwrap();
    assert_eq!(canonical.results(), capture.events.as_slice());
    assert_eq!(canonical.engine().stable_hash().unwrap(), capture.hash);
    let chosen = config();
    let plan = plan_audio(
        &original,
        capture.file.clone(),
        limits(),
        chosen.output_origin,
        chosen.preroll,
    )
    .unwrap();
    assert_eq!(
        plan.commands.len(),
        5,
        "three actual hit sounds and two in-prefix BGM cues"
    );
    assert_eq!(plan.judge_events, capture.events);
    let (mut reference_producer, mut reference) = mixer(original.bank, chosen, 64);
    for command in &plan.commands {
        reference_producer.try_push(*command).unwrap();
    }
    let (mut replay, bank) =
        StepReplay::new(prepared(lines, seed), capture.file, limits(), chosen).unwrap();
    let (mut producer, mut output) = mixer(bank, chosen, 64);
    let mut admitted = deliver(&mut replay, &mut producer, 2);
    let mut actual_pcm = Vec::new();
    let mut reference_pcm = Vec::new();
    let mut events = Vec::new();
    let mut completed = false;
    // Total six seconds includes the excluded BGM at original song four seconds.
    for frames in [1, 3, 2, 1, 4, 2, 3, 1, 4, 2, 1, 4, 3, 2, 4, 3, 4, 4] {
        let mut actual = vec![0.0; frames];
        let mut expected = vec![0.0; frames];
        let report = output.render(&mut actual).unwrap();
        reference.render(&mut expected).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(report.counters.late_commands, 0);
        completed |= replay
            .observe_output(
                Some(report),
                Some(presented(
                    chosen,
                    i64::try_from(output.frame_cursor()).unwrap() * 125_000_000,
                )),
            )
            .unwrap();
        events.extend(replay.drain_events());
        admitted.extend(deliver(&mut replay, &mut producer, 2));
        actual_pcm.extend(actual);
        reference_pcm.extend(expected);
    }
    assert_eq!(
        admitted, plan.commands,
        "bounded batches retain native replay ordering and exact times"
    );
    assert_eq!(actual_pcm, reference_pcm);
    assert!(actual_pcm.iter().any(|&sample| sample != 0.0));
    assert_eq!(events, capture.events);
    assert_eq!(replay.score(), &capture.score);
    assert_eq!(
        replay.recorded_until(),
        Some(Timestamp::from_nanos(2_500_000_000))
    );
    assert!(completed);
    assert!(!replay.failed());
}

#[test]
fn only_genuine_presentation_advances_negative_preroll_and_equal_time_recorded_operations() {
    let lines = "#00011:01\n#00012:01\n#00113:01\n#00101:02\n";
    let capture = captured(
        lines,
        0,
        0,
        &[
            Operation::Advance(-250_000_000),
            Operation::Key(0, 4, ButtonState::Down),
            Operation::Key(0, 5, ButtonState::Down),
            Operation::Key(0, 4, ButtonState::Up),
            Operation::Advance(0),
        ],
    );
    let chosen = config();
    let (mut replay, bank) =
        StepReplay::new(prepared(lines, 0), capture.file, limits(), chosen).unwrap();
    let (mut producer, mut output) = mixer(bank, chosen, 64);
    assert_eq!(deliver(&mut replay, &mut producer, 8).len(), 2);
    let report = output.render(&mut [0.0; 8]).unwrap();
    assert!(!replay.observe_output(Some(report), None).unwrap());
    assert_eq!(replay.score(), &ScoreSummary::default());
    assert!(replay.drain_events().is_empty());
    assert_eq!(replay.song_time(), Timestamp::from_nanos(-250_000_000));
    assert!(
        !replay
            .observe_output(Some(report), Some(presented(chosen, 249_999_999)))
            .unwrap()
    );
    assert_eq!(replay.score().hits, 0);
    assert_eq!(replay.pressed_lanes(), 0);
    assert!(
        !replay
            .observe_output(Some(report), Some(presented(chosen, 250_000_000)))
            .unwrap()
    );
    assert_eq!(replay.score(), &capture.score);
    assert_eq!(
        replay.pressed_lanes(),
        2,
        "same-time release affects only its original key owner"
    );
    assert_eq!(replay.drain_events(), capture.events);
    assert!(
        !replay
            .observe_output(Some(report), Some(presented(chosen, 250_000_000)))
            .unwrap()
    );
    assert!(replay.drain_events().is_empty());
    assert!(deliver(&mut replay, &mut producer, 8).is_empty());
    let later = output.render(&mut [0.0; 48]).unwrap();
    replay
        .observe_output(Some(later), Some(presented(chosen, 7_000_000_000)))
        .unwrap();
    assert_eq!(
        replay.score().misses,
        0,
        "presentation beyond the prefix cannot invent a timeout advance"
    );
    assert!(replay.drain_events().is_empty());
    assert_eq!(
        replay.bgm_report().total_admitted,
        2,
        "future chart BGM is excluded from a prefix plan"
    );
}

#[test]
fn selected_section_keeps_original_targets_and_copies_the_original_bgm_tail_once() {
    let lines = "#00011:01\n#00012:0001\n#00113:01\n#00001:02\n#00101:02\n";
    let start = 1_125_000_000;
    let capture = captured(
        lines,
        19,
        start,
        &[
            Operation::Advance(start - 250_000_000),
            Operation::Key(2_000_000_000, 5, ButtonState::Down),
            Operation::Advance(2_125_000_000),
        ],
    );
    assert_eq!(
        decode_chart_setup(&capture.file.header.options).unwrap().1,
        Timestamp::from_nanos(start)
    );
    let selected =
        prepare_replay(prepared(lines, 19), &capture.file, limits(), pcm_limits()).unwrap();
    let chosen = config();
    let reference_prepared =
        prepare_replay(prepared(lines, 19), &capture.file, limits(), pcm_limits()).unwrap();
    let plan = plan_audio(
        &reference_prepared,
        capture.file.clone(),
        limits(),
        chosen.output_origin,
        chosen.preroll,
    )
    .unwrap();
    assert_eq!(plan.commands.len(), 2);
    assert_eq!(
        plan.commands[0].at(),
        presented(chosen, 375_000_000).timestamp
    );
    let (mut reference_producer, mut reference) = mixer(reference_prepared.bank, chosen, 64);
    for command in &plan.commands {
        reference_producer.try_push(*command).unwrap();
    }
    let (mut replay, bank) = StepReplay::new(selected, capture.file, limits(), chosen).unwrap();
    assert_eq!(
        replay.song_time(),
        Timestamp::from_nanos(start - 250_000_000)
    );
    let (mut producer, mut output) = mixer(bank, chosen, 64);
    let mut actual_pcm = Vec::new();
    let mut observed_events = Vec::new();
    for _ in 0..32 {
        deliver(&mut replay, &mut producer, 2);
        let mut actual = [0.0];
        let mut expected = [0.0];
        let report = output.render(&mut actual).unwrap();
        reference.render(&mut expected).unwrap();
        assert_eq!(actual, expected);
        replay
            .observe_output(
                Some(report),
                Some(presented(
                    chosen,
                    i64::try_from(output.frame_cursor()).unwrap() * 125_000_000,
                )),
            )
            .unwrap();
        observed_events.extend(replay.drain_events());
        actual_pcm.push(actual[0]);
    }
    assert_eq!(&actual_pcm[..3], &[0.0; 3]);
    assert_eq!(
        actual_pcm[3],
        6144.0 / 32768.0 * 0.5,
        "tail starts at ceil(1.125 s * 4 Hz), not a recursively cut suffix"
    );
    assert_eq!(observed_events, capture.events);
    assert_eq!(replay.score(), &capture.score);
    assert_eq!(replay.score().misses, 0);
    assert_eq!(replay.bgm_report().total_admitted, 2);
}

#[test]
fn empty_log_completion_needs_a_subsequent_idle_render_and_exact_presented_end() {
    let lines = "#00011:01\n#00001:02\n";
    let capture = captured(lines, 0, 0, &[]);
    let chosen = config();
    let (mut replay, bank) =
        StepReplay::new(prepared(lines, 0), capture.file, limits(), chosen).unwrap();
    let (_producer, mut output) = mixer(bank, chosen, 64);
    assert_eq!(replay.recorded_until(), None);
    assert!(replay.take_commands(8).unwrap().is_none());
    assert!(
        !replay
            .observe_output(None, Some(presented(chosen, 0)))
            .unwrap()
    );
    let zero = output.render(&mut []).unwrap();
    assert!(
        !replay
            .observe_output(Some(zero), Some(presented(chosen, 0)))
            .unwrap()
    );
    let first = output.render(&mut [0.0]).unwrap();
    assert!(
        !replay
            .observe_output(Some(first), Some(presented(chosen, 125_000_000)))
            .unwrap()
    );
    assert!(
        !replay
            .observe_output(Some(first), Some(presented(chosen, 125_000_000)))
            .unwrap()
    );
    let second = output.render(&mut [0.0]).unwrap();
    assert!(!replay.observe_output(Some(second), None).unwrap());
    assert!(
        !replay
            .observe_output(None, Some(presented(chosen, 249_999_999)))
            .unwrap()
    );
    assert!(
        !replay
            .observe_output(Some(second), Some(presented(chosen, 249_999_999)))
            .unwrap()
    );
    assert!(
        replay
            .observe_output(Some(second), Some(presented(chosen, 250_000_000)))
            .unwrap()
    );
    assert!(
        !replay.observe_output(None, None).unwrap(),
        "completion is evidence, not a sticky wall-clock flag"
    );
    assert!(replay.drain_events().is_empty());
    assert_eq!(replay.score(), &ScoreSummary::default());
    assert_eq!(replay.bgm_report().total_admitted, 0);
}

#[test]
fn retained_batch_acknowledgements_preserve_the_actual_admitted_prefix_without_retry() {
    let lines = "#00011:01\n#00012:01\n#00001:02\n";
    let capture = captured(
        lines,
        0,
        0,
        &[
            Operation::Key(0, 4, ButtonState::Down),
            Operation::Key(0, 5, ButtonState::Down),
        ],
    );
    let chosen = config();
    let (mut replay, bank) =
        StepReplay::new(prepared(lines, 0), capture.file.clone(), limits(), chosen).unwrap();
    let before = replay.bgm_report();
    for max in [0, chosen.max_pending + 1] {
        assert!(matches!(
            replay.take_commands(max),
            Err(StepReplayError::InvalidConfiguration(_))
        ));
        assert!(!replay.failed());
        assert_eq!(replay.bgm_report(), before);
    }
    let batch = replay.take_commands(8).unwrap().unwrap();
    assert_eq!(batch.commands.len(), 3);
    assert!(
        matches!(replay.take_commands(1), Err(StepReplayError::OutstandingBatch { sequence }) if sequence == batch.sequence)
    );
    assert!(!replay.failed());
    let (mut producer, mut output) = mixer(bank, chosen, 1);
    producer.try_push(batch.commands[0]).unwrap();
    let rejected = producer.try_push(batch.commands[1]).unwrap_err();
    assert_eq!(rejected.reason, QueuePushError::Full);
    assert_eq!(rejected.command, batch.commands[1]);
    match replay.acknowledge(batch.sequence, 1, false).unwrap_err() {
        StepReplayError::Acknowledgement(StepGameplayError::AudioRejected {
            batch: retained,
            admitted,
        }) => {
            assert_eq!(retained, batch);
            assert_eq!(admitted, 1);
        }
        error => panic!("unexpected acknowledgement evidence: {error:?}"),
    }
    assert_fenced(&mut replay);
    let mut pcm = [0.0; 3];
    let report = output.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0, 0.0, 1024.0 / 32768.0 * 0.5]);
    assert_eq!(
        report.counters.commands_applied, 1,
        "already admitted output is not rolled back or retried"
    );

    for case in 0..5 {
        let (mut replay, _bank) =
            StepReplay::new(prepared(lines, 0), capture.file.clone(), limits(), chosen).unwrap();
        let batch = if case == 0 {
            None
        } else {
            replay.take_commands(8).unwrap()
        };
        let sequence = batch.as_ref().map_or(1, |batch| batch.sequence);
        if case == 4 {
            replay.acknowledge(sequence, 3, true).unwrap();
        }
        let (sequence, admitted) = match case {
            1 => (sequence + 1, 3),
            2 => (sequence, 2),
            3 => (sequence, 4),
            _ => (sequence, 3),
        };
        match replay.acknowledge(sequence, admitted, true).unwrap_err() {
            StepReplayError::Acknowledgement(StepGameplayError::InvalidAcknowledgement {
                sequence: received,
                admitted: count,
                success,
                batch: retained,
            }) => {
                assert_eq!((received, count, success), (sequence, admitted, true));
                assert_eq!(retained, if case == 4 { None } else { batch });
            }
            error => panic!("unexpected malformed acknowledgement: {error:?}"),
        }
        assert_fenced(&mut replay);
    }
}

#[test]
fn acknowledgements_do_not_grant_render_credit_and_actual_late_execution_is_terminal() {
    let lines = "#00011:01\n#00012:01\n";
    let capture = captured(
        lines,
        0,
        0,
        &[
            Operation::Key(0, 4, ButtonState::Down),
            Operation::Key(0, 5, ButtonState::Down),
        ],
    );
    let chosen = StepReplayConfig {
        max_pending: 1,
        ..config()
    };
    let (mut replay, bank) =
        StepReplay::new(prepared(lines, 0), capture.file.clone(), limits(), chosen).unwrap();
    let (mut producer, mut output) = mixer(bank, chosen, 64);
    assert_eq!(deliver(&mut replay, &mut producer, 1).len(), 1);
    assert_eq!(replay.bgm_report().remaining, 1);
    assert_eq!(replay.bgm_report().outstanding, 1);
    assert!(
        replay.take_commands(1).unwrap().is_none(),
        "ACK is not evidence that the target frame rendered"
    );
    let report = output.render(&mut [0.0; 3]).unwrap();
    assert_eq!(report.counters.late_commands, 0);
    assert!(
        !replay
            .observe_output(Some(report), Some(presented(chosen, 375_000_000)))
            .unwrap()
    );
    assert_eq!(replay.score(), &capture.score);
    match replay.take_commands(1).unwrap_err() {
        StepReplayError::Bgm {
            error:
                BgmFeedError::Late {
                    target_frame,
                    rendered_frames,
                    command,
                },
            report,
        } => {
            assert_eq!((target_frame, rendered_frames), (2, 3));
            assert_eq!(command.at(), presented(chosen, 250_000_000).timestamp);
            assert_eq!(report.total_admitted, 1);
            assert_eq!(report.remaining, 1);
        }
        error => panic!("expected exact unadmitted late command: {error:?}"),
    }
    assert_eq!(replay.drain_events(), capture.events);
    assert_fenced(&mut replay);

    let chosen = config();
    let (mut replay, bank) =
        StepReplay::new(prepared(lines, 0), capture.file, limits(), chosen).unwrap();
    let batch = replay.take_commands(8).unwrap().unwrap();
    let (mut producer, mut output) = mixer(bank, chosen, 64);
    output.render(&mut [0.0; 3]).unwrap();
    for command in &batch.commands {
        producer.try_push(*command).unwrap();
    }
    replay
        .acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    let late = output.render(&mut [0.0]).unwrap();
    assert_eq!(late.counters.late_commands, 2);
    let presentation = Some(presented(chosen, 500_000_000));
    match replay.observe_output(Some(late), presentation).unwrap_err() {
        StepReplayError::Audio {
            error: ReplayAudioError::RejectedRender(rejected),
            rendered,
            presented,
        } => {
            assert_eq!(rejected, late);
            assert_eq!(rendered, late);
            assert_eq!(presented, presentation);
        }
        error => panic!("expected actual Mixer late evidence: {error:?}"),
    }
    assert_eq!(
        replay.score(),
        &ScoreSummary::default(),
        "invalid output cannot mutate presentation judging first"
    );
    assert!(replay.drain_events().is_empty());
    assert_fenced(&mut replay);
}

#[test]
fn output_faults_retain_exact_evidence_and_previously_committed_events_and_score() {
    let lines = "#00011:01\n";
    let capture = captured(lines, 0, 0, &[Operation::Key(0, 4, ButtonState::Down)]);
    for case in 0..11 {
        let chosen = config();
        let (mut replay, bank) =
            StepReplay::new(prepared(lines, 0), capture.file.clone(), limits(), chosen).unwrap();
        let (mut producer, mut output) = mixer(bank, chosen, 64);
        deliver(&mut replay, &mut producer, 8);
        let first = output.render(&mut [0.0; 3]).unwrap();
        replay
            .observe_output(Some(first), Some(presented(chosen, 250_000_000)))
            .unwrap();
        assert_eq!(replay.score(), &capture.score);
        let song = replay.song_time();
        let mut report = output.render(&mut [0.0]).unwrap();
        let mut presentation = presented(chosen, 500_000_000);
        match case {
            0 => presentation.domain = ClockDomainId(11),
            1 => presentation.timestamp = Timestamp::from_nanos(i64::MIN),
            2 => report.paused = true,
            3 => report.producer_disconnected = true,
            4 => report.playback_end_physical_frame = Some(4),
            5 => report.playback_frames = 0,
            6 => report.counters.rendered_frames -= 1,
            7 => report.counters.unknown_samples = 1,
            8 => {
                report.start_frame = 2;
                report.playback_start_frame = 2;
                report.counters.rendered_frames = 3;
            }
            9 => {
                report = first;
                report.counters.commands_applied += 1;
            }
            10 => presentation = presented(chosen, 249_999_999),
            _ => unreachable!(),
        }
        match replay
            .observe_output(Some(report), Some(presentation))
            .unwrap_err()
        {
            StepReplayError::Output {
                rendered,
                presented,
                ..
            } => {
                assert_eq!(rendered, Some(report));
                assert_eq!(presented, Some(presentation));
            }
            error => panic!("unexpected output validation failure in case {case}: {error:?}"),
        }
        assert_eq!(replay.song_time(), song);
        assert_eq!(replay.score(), &capture.score);
        assert_eq!(replay.drain_events(), capture.events);
        assert!(
            replay.drain_events().is_empty(),
            "committed events remain readable once after a fence"
        );
        assert_fenced(&mut replay);
    }
}

#[test]
fn setup_validates_the_whole_canonical_log_selected_chart_and_checked_configuration() {
    let lines = "#00011:01\n#00112:01\n";
    let capture = captured(
        lines,
        0,
        0,
        &[
            Operation::Key(0, 4, ButtonState::Down),
            Operation::Advance(1),
        ],
    );
    let chosen = config();
    for invalid in [
        StepReplayConfig {
            max_pending: 0,
            ..chosen
        },
        StepReplayConfig {
            max_pending: AudioLimits::MAX_COMMANDS + 1,
            ..chosen
        },
        StepReplayConfig {
            preroll: Duration::from_nanos(-1),
            ..chosen
        },
        StepReplayConfig {
            lookahead: Duration::ZERO,
            ..chosen
        },
    ] {
        assert!(matches!(
            StepReplay::new(prepared(lines, 0), capture.file.clone(), limits(), invalid),
            Err(StepReplayError::InvalidConfiguration(_))
        ));
    }
    let mut bad_ordinal = capture.file.clone();
    bad_ordinal.records[1].ordinal = 9;
    let mut bad_chronology = capture.file.clone();
    bad_chronology.records[1].song_time = Timestamp::from_nanos(-1);
    for invalid in [bad_ordinal, bad_chronology] {
        assert!(matches!(
            StepReplay::new(prepared(lines, 0), invalid, limits(), chosen),
            Err(StepReplayError::Setup(_))
        ));
    }
    assert!(matches!(
        StepReplay::new(prepared("#00012:01\n", 0), capture.file, limits(), chosen),
        Err(StepReplayError::Setup(_))
    ));
    let section = captured(
        lines,
        0,
        2_000_000_000,
        &[Operation::Advance(2_000_000_000)],
    );
    assert!(
        matches!(
            StepReplay::new(prepared(lines, 0), section.file, limits(), chosen),
            Err(StepReplayError::Setup(_))
        ),
        "unselected original PCM/chart cannot masquerade as prepared section replay"
    );
    let sound = captured(
        "#00011:01\n",
        0,
        0,
        &[Operation::Key(0, 4, ButtonState::Down)],
    );
    assert!(
        StepReplay::new(
            prepared("#00011:01\n", 0),
            sound.file,
            limits(),
            StepReplayConfig {
                output_origin: point(22, i64::MAX),
                ..chosen
            }
        )
        .is_err(),
        "output origin plus actual scheduled preroll is checked before a usable owner exists"
    );
}
