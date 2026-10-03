//! Actual portable preparation, gameplay, command queues and Mixer; no native I/O.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    bgm::BgmFeedError,
    local_runtime::FailureKind,
    prepare_from_source,
    replay_capture::CaptureError,
    replay_playback::{decode_chart_setup, reconstruct},
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        QueuePushError, SampleBank, SampleId, command_queue,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId,
        DeviceSelector, EventMeta, GameControlId, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent,
    },
    interaction::InteractionState,
    judge::{JudgeGrade, JudgeOutcome, JudgeStage, MissReason},
    replay::{
        ReplayOperation,
        codec::{ReplayCodecError, ReplayCodecLimits, decode_replay, encode_replay},
    },
    runtime::RuntimeError,
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_platform::audio::presentation::discipline::{
    DisciplineConfig, DisciplineError, DisciplineUpdate, ObservationAdmission,
};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(11, 10_000_000_000),
        output_origin: point(22, 100),
        preroll: Duration::from_nanos(250_000_001),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 2,
        bgm_lookahead: Duration::from_nanos(500_000_000),
        telemetry_capacity: 8,
    }
}

fn zero_preroll() -> StepGameplayConfig {
    StepGameplayConfig {
        output_origin: point(22, 0),
        preroll: Duration::ZERO,
        ..config()
    }
}

fn clocks(config: StepGameplayConfig) -> AffineClockMapper {
    AffineClockMapper::exact_offset(
        ClockPair {
            source: config.host_origin,
            target: config.output_origin,
        },
        ClockInterval {
            start: config.host_origin.timestamp,
            end: config
                .host_origin
                .timestamp
                .checked_add(Duration::from_nanos(20_000_000_000))
                .unwrap(),
        },
    )
    .unwrap()
}

fn host_at(config: StepGameplayConfig, song: i64) -> ClockPoint {
    ClockPoint {
        domain: config.host_origin.domain,
        timestamp: config
            .host_origin
            .timestamp
            .checked_add(config.preroll)
            .unwrap()
            .checked_add(Duration::from_nanos(song))
            .unwrap(),
    }
}

fn output_at(config: StepGameplayConfig, song: i64) -> ClockPoint {
    // Supply a host-domain scheduling observation independently of input time.
    let mut point = host_at(config, song);
    point.timestamp = point
        .timestamp
        .checked_add(Duration::from_nanos(100_000_000))
        .unwrap();
    point
}

fn input(
    config: StepGameplayConfig,
    device: u64,
    key: u16,
    sequence: u64,
    song: i64,
    state: ButtonState,
) -> PhysicalInputEvent {
    let at = host_at(config, song);
    let mut meta = EventMeta::new(DeviceId(device), at, sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(7),
        code: Some(42),
        timestamp: Some(at),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(key),
        state,
    })
}

fn bindings(fanout: bool) -> BindingMap {
    BindingMap::from_bindings((0..3).map(|index| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(if fanout { 4 } else { 4 + index }),
        game_control: GameControlId(u32::from(0x11 + index)),
    }))
    .unwrap()
}

fn wav(samples: &[i16]) -> Vec<u8> {
    let length = u32::try_from(samples.len() * 2).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn prepared(lines: &str) -> PreparedBms {
    prepared_seed(lines, 0)
}

fn prepared_seed(lines: &str, seed: u64) -> PreparedBms {
    let chart = format!("#BPM 60\n#VOLWAV 50\n#LNTYPE 1\n#WAV01 key.wav\n#WAV02 bgm.wav\n{lines}");
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files.insert("pack/key.wav", wav(&[8192, -4096])).unwrap();
    files.insert("pack/bgm.wav", wav(&[4096, 2048])).unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    prepare_from_source(
        chart.as_bytes(),
        &source,
        AudioFormat::new(4, 1).unwrap(),
        PcmLimits::new(256, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        seed,
        None,
    )
    .unwrap()
}

fn make_mixer(bank: SampleBank, chosen: StepGameplayConfig) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(16).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            bank.format(),
            chosen.output_origin.domain,
            chosen.output_origin.timestamp,
            AudioLimits::new(16, 8, 16, 8, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}

fn deliver(
    game: &mut StepGameplay,
    producer: &mut CommandProducer,
    max: usize,
) -> Vec<AudioCommand> {
    let mut commands = Vec::new();
    while let Some(batch) = game.take_commands(max).unwrap() {
        for command in &batch.commands {
            producer.try_push(*command).unwrap();
        }
        game.acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        commands.extend(batch.commands);
    }
    commands
}

fn assert_fenced(game: &mut StepGameplay, chosen: StepGameplayConfig) {
    let score = game.score().clone();
    let song = game.song_time();
    let hash = game.judge().stable_hash().unwrap();
    assert!(game.failed());
    assert!(matches!(
        game.take_commands(1),
        Err(StepGameplayError::Failed)
    ));
    assert!(matches!(
        game.acknowledge(1, 0, true),
        Err(StepGameplayError::Failed)
    ));
    assert!(matches!(
        game.feed_audio(0, 1),
        Err(StepGameplayError::Failed)
    ));
    assert!(matches!(
        game.observe_completion(None, None),
        Err(StepGameplayError::Failed)
    ));
    assert!(matches!(
        game.activate(chosen.host_origin),
        Err(StepGameplayError::Failed)
    ));
    assert!(matches!(
        game.advance_to(
            host_at(chosen, 9_000_000_000),
            &clocks(chosen),
            chosen.output_origin
        ),
        Err(StepGameplayError::Failed)
    ));
    game.fail();
    assert_eq!(game.score(), &score);
    assert_eq!(game.song_time(), song);
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
}

#[test]
fn actual_hit_hold_miss_and_rolling_bgm_preserve_preroll_provenance_and_mixer_pcm() {
    let prepared =
        prepared("#00011:01000000\n#00052:00010100\n#00013:00000001\n#00001:02000000\n#00101:02\n");
    let tap = prepared
        .source
        .notes
        .iter()
        .find(|note| note.lane.channel() == 0x11)
        .unwrap()
        .object;
    let hold = prepared
        .source
        .notes
        .iter()
        .find(|note| note.lane.channel() == 0x12)
        .unwrap()
        .object;
    let miss = prepared
        .source
        .notes
        .iter()
        .find(|note| note.lane.channel() == 0x13)
        .unwrap()
        .object;
    let chosen = config();
    let mapper = clocks(chosen);
    let (mut game, bank) = StepGameplay::new(prepared, chosen, bindings(false)).unwrap();
    assert_eq!(bank.get(SampleId(1)).unwrap().samples(), &[0.25, -0.125]);
    assert_eq!(bank.get(SampleId(2)).unwrap().samples(), &[0.125, 0.0625]);
    assert_eq!(game.song_time(), Timestamp::from_nanos(-250_000_001));
    assert_eq!(
        (
            game.bgm_report().total_admitted,
            game.bgm_report().remaining
        ),
        (1, 1)
    );
    let background = game.take_commands(8).unwrap().unwrap();
    assert_eq!(background.sequence, 1);
    assert_eq!(background.commands.len(), 1);
    assert!(
        matches!(background.commands[0], AudioCommand::Play { sample: SampleId(2), at, gain: 0.5, .. } if at == Timestamp::from_nanos(250_000_101))
    );

    let physical = input(chosen, 44, 4, 1, 0, ButtonState::Down);
    let hit = game
        .process_input(physical.clone(), &mapper, output_at(chosen, 0))
        .unwrap();
    assert_eq!(hit.input, Some(physical));
    assert_eq!(hit.song_time, Timestamp::ZERO);
    assert_eq!(hit.audio_at, point(22, 350_000_101));
    assert_eq!(hit.judge_events.len(), 1);
    assert_eq!(hit.judge_events[0].object, tap);
    assert_eq!(
        hit.judge_events[0].outcome,
        JudgeOutcome::Hit {
            grade: JudgeGrade(1),
            delta: Duration::ZERO
        }
    );
    assert_eq!(
        hit.judge_events[0].input.unwrap().native.unwrap().backend,
        BackendId(7)
    );
    assert_eq!(game.judge().state(tap), Some(InteractionState::Completed));
    assert!(matches!(
        game.take_commands(8),
        Err(StepGameplayError::OutstandingBatch { sequence: 1 })
    ));
    assert_eq!(game.feed_audio(0, 1).unwrap().admitted, 0);
    assert!(!game.failed());
    let (mut producer, mut mixer) = make_mixer(bank, chosen);
    producer.try_push(background.commands[0]).unwrap();
    game.acknowledge(background.sequence, background.commands.len(), true)
        .unwrap();
    let key = game.take_commands(8).unwrap().unwrap();
    assert_eq!(key.sequence, 2);
    assert_eq!(key.commands, hit.audio_commands);
    producer.try_push(key.commands[0]).unwrap();
    game.acknowledge(key.sequence, key.commands.len(), true)
        .unwrap();
    assert!(game.take_commands(8).unwrap().is_none());

    let head = game
        .process_input(
            input(chosen, 44, 5, 2, 1_000_000_000, ButtonState::Down),
            &mapper,
            output_at(chosen, 1_000_000_000),
        )
        .unwrap();
    assert_eq!(head.judge_events.len(), 1);
    assert_eq!(head.judge_events[0].stage, JudgeStage::HoldHead);
    assert_eq!(game.judge().state(hold), Some(InteractionState::Active));
    let stranger = game
        .process_input(
            input(chosen, 99, 5, 1, 2_000_000_000, ButtonState::Up),
            &mapper,
            output_at(chosen, 2_000_000_000),
        )
        .unwrap();
    assert!(stranger.judge_events.is_empty());
    assert_eq!(game.judge().state(hold), Some(InteractionState::Active));
    let tail = game
        .process_input(
            input(chosen, 44, 5, 3, 2_000_000_000, ButtonState::Up),
            &mapper,
            output_at(chosen, 2_000_000_000),
        )
        .unwrap();
    assert_eq!(tail.judge_events.len(), 1);
    assert_eq!(tail.judge_events[0].stage, JudgeStage::HoldTail);
    assert!(matches!(
        tail.judge_events[0].outcome,
        JudgeOutcome::Hit { .. }
    ));
    assert!(
        tail.audio_commands.is_empty(),
        "BMS tail tokens are not new keysounds"
    );
    let expired = game
        .advance_to(
            host_at(chosen, 3_000_000_001),
            &mapper,
            output_at(chosen, 3_000_000_001),
        )
        .unwrap();
    assert_eq!(expired.judge_events.len(), 1);
    assert_eq!(expired.judge_events[0].object, miss);
    assert_eq!(
        expired.judge_events[0].outcome,
        JudgeOutcome::Miss {
            reason: MissReason::HeadTimeout
        }
    );
    assert_eq!(
        (
            game.score().hits,
            game.score().misses,
            game.score().combo,
            game.score().max_combo
        ),
        (3, 1, 0, 3)
    );
    assert_eq!(game.score().grades.get(&1), Some(&3));
    for object in [tap, hold, miss] {
        assert_eq!(
            game.judge().state(object),
            Some(InteractionState::Completed)
        );
    }
    assert_eq!(deliver(&mut game, &mut producer, 8), head.audio_commands);
    let score = game.score().clone();
    let mut pcm = Vec::new();
    for _ in 0..5 {
        let mut block = [9.0; 4];
        let rendered = mixer.render(&mut block).unwrap();
        assert_eq!(rendered.counters.late_commands, 0);
        pcm.extend(block);
        game.feed_audio(mixer.frame_cursor(), 2).unwrap();
        deliver(&mut game, &mut producer, 8);
    }
    let mut expected = vec![0.0; 20];
    expected[2] = 0.1875;
    expected[3] = -0.03125;
    expected[6] = 0.125;
    expected[7] = -0.0625;
    expected[18] = 0.0625;
    expected[19] = 0.03125;
    assert_eq!(pcm, expected);
    assert_eq!(game.score(), &score);
    assert_eq!(game.song_time(), Timestamp::from_nanos(3_000_000_001));
    assert_eq!(
        (
            game.bgm_report().total_admitted,
            game.bgm_report().remaining,
            game.bgm_report().outstanding
        ),
        (2, 0, 0)
    );
}

#[test]
fn profile_offset_is_applied_once_and_output_scheduling_remains_independent() {
    let chosen = StepGameplayConfig {
        early_ns: 10,
        late_ns: 20,
        offset_ns: 7,
        ..config()
    };
    let (mut game, _) =
        StepGameplay::new(prepared("#00011:01\n"), chosen, bindings(false)).unwrap();
    let report = game
        .process_input(
            input(chosen, 1, 4, 1, -7, ButtonState::Down),
            &clocks(chosen),
            point(22, 900),
        )
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(-7));
    assert_eq!(report.judge_events[0].at, Timestamp::ZERO);
    assert_eq!(
        report.judge_events[0].outcome,
        JudgeOutcome::Hit {
            grade: JudgeGrade(1),
            delta: Duration::ZERO
        }
    );
    assert_eq!(report.audio_at, point(22, 900));
    let batch = game.take_commands(1).unwrap().unwrap();
    assert!(
        matches!(batch.commands[0], AudioCommand::Play { at, .. } if at == Timestamp::from_nanos(900))
    );
    assert_eq!(game.score().hits, 1);
}

#[test]
fn pristine_activation_moves_only_host_anchor_and_retains_pending_background_admission() {
    let chosen = config();
    let (mut game, _) =
        StepGameplay::new(prepared("#00011:01\n#00001:02\n"), chosen, bindings(false)).unwrap();
    let batch = game.take_commands(8).unwrap().unwrap();
    let score = game.score().clone();
    let hash = game.judge().stable_hash().unwrap();
    let bgm = game.bgm_report();
    for bad in [point(99, 15_000_000_000), point(11, i64::MAX)] {
        assert!(matches!(
            game.activate(bad),
            Err(StepGameplayError::InvalidConfiguration(_))
        ));
        assert!(!game.failed());
        assert_eq!(game.judge().stable_hash().unwrap(), hash);
        assert_eq!(game.bgm_report(), bgm);
    }
    let shifted = StepGameplayConfig {
        host_origin: point(11, 15_000_000_000),
        ..chosen
    };
    game.activate(shifted.host_origin).unwrap();
    assert_eq!(game.song_time(), Timestamp::from_nanos(-250_000_001));
    assert_eq!(game.score(), &score);
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
    assert_eq!(game.bgm_report(), bgm);
    assert!(matches!(
        game.take_commands(8),
        Err(StepGameplayError::OutstandingBatch { sequence: 1 })
    ));
    assert!(matches!(
        game.activate(chosen.host_origin),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    assert!(
        matches!(batch.commands[0], AudioCommand::Play { sample: SampleId(2), at, .. } if at == Timestamp::from_nanos(250_000_101))
    );
    let hit = game
        .process_input(
            input(shifted, 1, 4, 1, 0, ButtonState::Down),
            &clocks(shifted),
            output_at(shifted, 0),
        )
        .unwrap();
    assert_eq!(hit.song_time, Timestamp::ZERO);
    assert_eq!(hit.audio_at, point(22, 350_000_101));
    assert_eq!(game.score().hits, 1);
    let key = game.take_commands(8).unwrap().unwrap();
    assert_eq!(key.sequence, 2);
    assert_eq!(key.commands, hit.audio_commands);
    let hash = game.judge().stable_hash().unwrap();
    assert!(matches!(
        game.activate(chosen.host_origin),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
    assert!(!game.failed());

    let (mut advanced, _) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
    advanced
        .advance_to(chosen.host_origin, &clocks(chosen), chosen.output_origin)
        .unwrap();
    assert!(matches!(
        advanced.activate(shifted.host_origin),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert_eq!(advanced.song_time(), Timestamp::from_nanos(-250_000_001));
    assert!(!advanced.failed());
}

#[test]
fn local_preflight_and_outstanding_batch_do_not_consume_serial_or_block_judging() {
    let chosen = zero_preroll();
    let (mut game, _) =
        StepGameplay::new(prepared("#00011:01\n"), chosen, bindings(false)).unwrap();
    assert!(game.take_commands(1).unwrap().is_none());
    for max in [0, chosen.command_capacity + 1] {
        assert!(matches!(
            game.take_commands(max),
            Err(StepGameplayError::InvalidConfiguration(_))
        ));
    }
    for budget in [0, AudioLimits::MAX_COMMANDS + 1] {
        assert!(matches!(
            game.feed_audio(0, budget),
            Err(StepGameplayError::InvalidConfiguration(_))
        ));
    }
    assert!(!game.failed());
    let hit = game
        .process_input(
            input(chosen, 1, 4, 1, 0, ButtonState::Down),
            &clocks(chosen),
            chosen.output_origin,
        )
        .unwrap();
    let batch = game.take_commands(1).unwrap().unwrap();
    assert_eq!(batch.sequence, 1);
    assert_eq!(batch.commands, hit.audio_commands);
    assert!(matches!(
        game.take_commands(1),
        Err(StepGameplayError::OutstandingBatch { sequence: 1 })
    ));
    let advance = game
        .advance_to(host_at(chosen, 100), &clocks(chosen), chosen.output_origin)
        .unwrap();
    assert_eq!(advance.song_time, Timestamp::from_nanos(100));
    assert_eq!(
        game.feed_audio(0, AudioLimits::MAX_COMMANDS)
            .unwrap()
            .admitted,
        0
    );
    game.acknowledge(1, 1, true).unwrap();
    assert!(game.take_commands(1).unwrap().is_none());
    assert!(!game.failed());
    assert_eq!(game.score().hits, 1);

    let (mut split, _) =
        StepGameplay::new(prepared("#00001:02\n#00001:02\n"), chosen, bindings(false)).unwrap();
    let first = split.take_commands(1).unwrap().unwrap();
    assert_eq!((first.sequence, first.commands.len()), (1, 1));
    split.acknowledge(first.sequence, 1, true).unwrap();
    let second = split.take_commands(1).unwrap().unwrap();
    assert_eq!((second.sequence, second.commands.len()), (2, 1));
    assert_ne!(first.commands, second.commands);
    split.acknowledge(second.sequence, 1, true).unwrap();
    assert!(split.take_commands(1).unwrap().is_none());
    assert_eq!(
        split.bgm_report().outstanding,
        2,
        "queue ACK is not completed rendering"
    );
}

#[test]
fn rejected_or_malformed_acknowledgements_retain_exact_original_batch_and_fence_without_retry() {
    for admitted in 0..=2 {
        let chosen = StepGameplayConfig {
            bgm_pending: 3,
            ..zero_preroll()
        };
        let (mut game, _) =
            StepGameplay::new(prepared("#00001:02\n#00001:02\n"), chosen, bindings(false)).unwrap();
        let batch = game.take_commands(8).unwrap().unwrap();
        assert_eq!(batch.commands.len(), 2);
        match game
            .acknowledge(batch.sequence, admitted, false)
            .unwrap_err()
        {
            StepGameplayError::AudioRejected {
                batch: actual,
                admitted: prefix,
            } => {
                assert_eq!(actual, batch);
                assert_eq!(prefix, admitted);
            }
            error => panic!("expected exact remote prefix, got {error:?}"),
        }
        assert_fenced(&mut game, chosen);
    }
    for (sequence, admitted, success) in [(2, 2, true), (1, 3, false), (1, 1, true)] {
        let chosen = StepGameplayConfig {
            bgm_pending: 3,
            ..zero_preroll()
        };
        let (mut game, _) =
            StepGameplay::new(prepared("#00001:02\n#00001:02\n"), chosen, bindings(false)).unwrap();
        let batch = game.take_commands(8).unwrap().unwrap();
        match game.acknowledge(sequence, admitted, success).unwrap_err() {
            StepGameplayError::InvalidAcknowledgement {
                sequence: actual_sequence,
                admitted: actual_admitted,
                success: actual_success,
                batch: actual,
            } => {
                assert_eq!(
                    (actual_sequence, actual_admitted, actual_success),
                    (sequence, admitted, success)
                );
                assert_eq!(actual, Some(batch));
            }
            error => panic!("expected invalid acknowledgement, got {error:?}"),
        }
        assert_fenced(&mut game, chosen);
    }
    let chosen = zero_preroll();
    let (mut empty, _) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
    assert!(empty.take_commands(1).unwrap().is_none());
    assert!(matches!(
        empty.acknowledge(1, 0, true),
        Err(StepGameplayError::InvalidAcknowledgement { batch: None, .. })
    ));
    assert_fenced(&mut empty, chosen);
}

#[test]
fn actual_full_queue_preserves_committed_hit_report_score_and_rejected_sound() {
    let chosen = StepGameplayConfig {
        command_capacity: 2,
        bgm_pending: 1,
        ..zero_preroll()
    };
    let (mut game, _) = StepGameplay::new(
        prepared("#00011:01\n#00012:01\n#00001:02\n"),
        chosen,
        bindings(true),
    )
    .unwrap();
    assert_eq!(game.bgm_report().total_admitted, 1);
    let error = game
        .process_input(
            input(chosen, 1, 4, 1, 0, ButtonState::Down),
            &clocks(chosen),
            point(22, 123),
        )
        .unwrap_err();
    let StepGameplayError::Report {
        report,
        score_error,
        capture_error,
    } = error
    else {
        panic!("expected committed partial report: {error:?}")
    };
    assert!(score_error.is_none());
    assert!(capture_error.is_none());
    assert_eq!(report.judge_events.len(), 2);
    assert!(
        report
            .judge_events
            .iter()
            .all(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
    );
    assert!(report.judge_error.is_none());
    assert_eq!(report.audio_commands.len(), 1);
    assert_eq!(report.audio_failures.len(), 1);
    assert_eq!(report.audio_failures[0].reason, QueuePushError::Full);
    assert!(
        matches!(report.audio_failures[0].command, AudioCommand::Play { sample: SampleId(1), at, gain: 0.5, .. } if at == Timestamp::from_nanos(123))
    );
    assert_ne!(report.audio_commands[0], report.audio_failures[0].command);
    assert_eq!((game.score().hits, game.score().combo), (2, 2));
    assert_eq!(game.song_time(), report.song_time);
    assert_fenced(&mut game, chosen);
}

#[test]
fn actual_mixer_cursor_exposes_late_bgm_and_partial_feed_failure_without_retimestamping() {
    let chosen = StepGameplayConfig {
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(250_000_000),
        ..zero_preroll()
    };
    let source = prepared("#00001:02020000\n");
    let second = source.bgm_commands[1];
    let (mut game, bank) = StepGameplay::new(source, chosen, bindings(false)).unwrap();
    let (mut producer, mut mixer) = make_mixer(bank, chosen);
    deliver(&mut game, &mut producer, 8);
    mixer.render(&mut [0.0; 5]).unwrap();
    assert_eq!(mixer.frame_cursor(), 5);
    match game.feed_audio(mixer.frame_cursor(), 1).unwrap_err() {
        StepGameplayError::Bgm {
            error:
                BgmFeedError::Late {
                    command,
                    target_frame,
                    rendered_frames,
                },
            report,
        } => {
            assert_eq!(command, second);
            assert_eq!((target_frame, rendered_frames), (4, 5));
            assert_eq!((report.total_admitted, report.remaining), (1, 1));
        }
        error => panic!("expected unmodified late cue: {error:?}"),
    }
    assert_fenced(&mut game, chosen);

    let chosen = StepGameplayConfig {
        command_capacity: 4,
        bgm_pending: 3,
        ..zero_preroll()
    };
    let source =
        prepared("#00011:01\n#00012:01\n#00013:01\n#00001:02\n#00001:00020000\n#00001:00020000\n");
    let rejected = source.bgm_commands[2];
    let (mut game, bank) = StepGameplay::new(source, chosen, bindings(true)).unwrap();
    let (mut producer, mut mixer) = make_mixer(bank, chosen);
    deliver(&mut game, &mut producer, 4);
    mixer.render(&mut [0.0; 3]).unwrap();
    let hits = game
        .process_input(
            input(chosen, 1, 4, 1, 0, ButtonState::Down),
            &clocks(chosen),
            point(22, 750_000_000),
        )
        .unwrap();
    assert_eq!(hits.audio_commands.len(), 3);
    match game.feed_audio(mixer.frame_cursor(), 2).unwrap_err() {
        StepGameplayError::Bgm {
            error: BgmFeedError::Admission(error),
            report,
        } => {
            assert_eq!(error.reason, QueuePushError::Full);
            assert_eq!(error.command, rejected);
            assert_eq!(
                (
                    report.admitted,
                    report.total_admitted,
                    report.remaining,
                    report.outstanding
                ),
                (1, 2, 1, 1)
            );
            assert_eq!(game.bgm_report().total_admitted, report.total_admitted);
            assert_eq!(game.bgm_report().remaining, report.remaining);
        }
        error => panic!("expected retained BGM admission prefix: {error:?}"),
    }
    assert_eq!(game.score().hits, 3);
    assert_fenced(&mut game, chosen);
}

#[test]
fn completed_mixer_cursor_cannot_regress_or_change_logical_song_progress() {
    let chosen = zero_preroll();
    let (mut game, bank) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
    let (_producer, mut mixer) = make_mixer(bank, chosen);
    let mut first = [9.0; 2];
    mixer.render(&mut first).unwrap();
    assert_eq!(first, [0.0; 2]);
    let earlier = mixer.frame_cursor();
    game.feed_audio(earlier, 1).unwrap();
    mixer.render(&mut [0.0; 3]).unwrap();
    let latest = mixer.frame_cursor();
    game.feed_audio(latest, 1).unwrap();
    assert_eq!(game.song_time(), Timestamp::ZERO);
    match game.feed_audio(earlier, 1).unwrap_err() {
        StepGameplayError::Bgm {
            error: BgmFeedError::CursorRegression { previous, received },
            report,
        } => {
            assert_eq!((previous, received), (latest, earlier));
            assert_eq!(
                (report.admitted, report.total_admitted, report.remaining),
                (0, 0, 0)
            );
        }
        error => panic!("expected completed-cursor regression: {error:?}"),
    }
    assert_fenced(&mut game, chosen);
}

#[test]
fn core_domain_host_and_device_sequence_rejections_fence_without_extra_judge_mutation() {
    for case in 0..4 {
        let chosen = zero_preroll();
        let (mut game, _) =
            StepGameplay::new(prepared("#00011:01\n"), chosen, bindings(false)).unwrap();
        let mapper = clocks(chosen);
        game.process_input(
            input(chosen, 1, 99, 2, 0, ButtonState::Down),
            &mapper,
            chosen.output_origin,
        )
        .unwrap();
        let hash = game.judge().stable_hash().unwrap();
        let score = game.score().clone();
        let song = game.song_time();
        let error = match case {
            0 => game
                .advance_to(host_at(chosen, -1), &mapper, chosen.output_origin)
                .unwrap_err(),
            1 => game
                .process_input(
                    input(chosen, 1, 4, 1, 0, ButtonState::Down),
                    &mapper,
                    chosen.output_origin,
                )
                .unwrap_err(),
            2 => game
                .advance_to(point(33, 0), &mapper, chosen.output_origin)
                .unwrap_err(),
            _ => game
                .process_input(
                    input(chosen, 1, 4, 3, 0, ButtonState::Down),
                    &mapper,
                    point(33, 0),
                )
                .unwrap_err(),
        };
        let StepGameplayError::Runtime(error) = error else {
            panic!("expected shared runtime failure: {error:?}")
        };
        match (case, error.kind) {
            (0, FailureKind::Core(RuntimeError::NonMonotonicHost)) => {}
            (
                1,
                FailureKind::Core(RuntimeError::SequenceRegression {
                    device: DeviceId(1),
                    last: 2,
                    received: 1,
                }),
            ) => {}
            (
                2 | 3,
                FailureKind::Core(RuntimeError::UnmappedClock {
                    from: ClockDomainId(33),
                    ..
                }),
            ) => {}
            (_, kind) => panic!("unexpected core rejection: {kind:?}"),
        }
        assert!(error.completed_reports.is_empty());
        assert_eq!(game.score(), &score);
        assert_eq!(game.song_time(), song);
        assert_eq!(game.judge().stable_hash().unwrap(), hash);
        assert_fenced(&mut game, chosen);
    }
}

#[test]
fn config_and_checked_bgm_extents_reject_before_a_usable_session_is_returned() {
    let base = config();
    for invalid in [
        StepGameplayConfig {
            output_origin: base.host_origin,
            ..base
        },
        StepGameplayConfig {
            preroll: Duration::from_nanos(-1),
            ..base
        },
        StepGameplayConfig {
            command_capacity: 0,
            ..base
        },
        StepGameplayConfig {
            command_capacity: 1,
            ..base
        },
        StepGameplayConfig {
            command_capacity: AudioLimits::MAX_COMMANDS + 1,
            ..base
        },
        StepGameplayConfig {
            bgm_pending: 0,
            ..base
        },
        StepGameplayConfig {
            bgm_pending: base.command_capacity,
            ..base
        },
        StepGameplayConfig {
            bgm_lookahead: Duration::ZERO,
            ..base
        },
        StepGameplayConfig {
            telemetry_capacity: 65_537,
            ..base
        },
        StepGameplayConfig {
            host_origin: point(11, i64::MAX),
            ..base
        },
    ] {
        assert!(matches!(
            StepGameplay::new(prepared("#00011:01\n"), invalid, bindings(false)),
            Err(StepGameplayError::InvalidConfiguration(_))
        ));
    }
    for invalid in [
        StepGameplayConfig {
            early_ns: -1,
            ..base
        },
        StepGameplayConfig {
            late_ns: -1,
            ..base
        },
    ] {
        assert!(matches!(
            StepGameplay::new(prepared("#00011:01\n"), invalid, bindings(false)),
            Err(StepGameplayError::Setup(_))
        ));
    }
    let overflow = StepGameplayConfig {
        output_origin: point(22, i64::MAX),
        preroll: Duration::ZERO,
        ..base
    };
    assert!(matches!(
        StepGameplay::new(prepared("#00001:00020000\n"), overflow, bindings(false)),
        Err(StepGameplayError::Bgm {
            error: BgmFeedError::Overflow,
            ..
        })
    ));
    let chosen = zero_preroll();
    let (mut game, _) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
    // Deliberately malformed externally supplied completed-frame evidence.
    assert!(matches!(
        game.feed_audio(u64::MAX, 1),
        Err(StepGameplayError::Bgm {
            error: BgmFeedError::Overflow,
            ..
        })
    ));
    assert_fenced(&mut game, chosen);
}

#[test]
fn completion_waits_for_local_and_retained_commands_pcm_tail_and_exact_presented_end() {
    let week = 604_800_000_000_000;
    let chosen = StepGameplayConfig {
        output_origin: point(22, week),
        ..zero_preroll()
    };
    let mapper = clocks(chosen);
    let (mut game, bank) =
        StepGameplay::new(prepared("#00011:01\n"), chosen, bindings(false)).unwrap();
    game.process_input(
        input(chosen, 1, 4, 1, 0, ButtonState::Down),
        &mapper,
        chosen.output_origin,
    )
    .unwrap();
    game.advance_to(host_at(chosen, 1), &mapper, chosen.output_origin)
        .unwrap();
    assert_eq!(game.score().hits, 1);
    let (mut producer, mut mixer) = make_mixer(bank, chosen);
    let first = mixer.render(&mut [0.0]).unwrap();
    let before_end = Some(point(22, week + 999_999_999));
    assert!(!game.observe_completion(Some(first), before_end).unwrap());
    let before_admission = mixer.render(&mut [0.0]).unwrap();
    assert!(
        !game
            .observe_completion(Some(before_admission), before_end)
            .unwrap()
    );
    let batch = game.take_commands(8).unwrap().unwrap();
    assert_eq!(batch.commands.len(), 1);
    assert!(
        !game
            .observe_completion(Some(before_admission), before_end)
            .unwrap()
    );
    producer.try_push(batch.commands[0]).unwrap();
    game.acknowledge(batch.sequence, 1, true).unwrap();
    // A pre-admission idle report cannot finish merely because its batch ACK arrived.
    assert!(
        !game
            .observe_completion(Some(before_admission), before_end)
            .unwrap()
    );
    let mut pcm = [0.0];
    let head = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.125]);
    assert_eq!(head.active_voices, 1);
    assert!(!game.observe_completion(Some(head), before_end).unwrap());
    let tail = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [-0.0625]);
    assert_eq!(tail.active_voices, 0);
    assert!(!game.observe_completion(Some(tail), None).unwrap());
    assert!(!game.observe_completion(None, before_end).unwrap());
    let later = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0]);
    // First idle end is frame 4 = 1 s, independent of later buffered silence.
    assert!(!game.observe_completion(Some(later), before_end).unwrap());
    assert!(
        game.observe_completion(Some(later), Some(point(22, week + 1_000_000_000)))
            .unwrap()
    );
    assert_eq!(game.score().hits, 1);
    assert!(!game.failed());
}

#[test]
fn completion_uses_actual_miss_deadline_and_rolling_bgm_tail_not_the_last_note_time() {
    let chosen = zero_preroll();
    let mapper = clocks(chosen);
    let (mut game, bank) =
        StepGameplay::new(prepared("#00011:01\n#00101:02\n"), chosen, bindings(false)).unwrap();
    let (mut producer, mut mixer) = make_mixer(bank, chosen);
    game.advance_to(host_at(chosen, 0), &mapper, chosen.output_origin)
        .unwrap();
    assert_eq!(
        game.score().misses,
        0,
        "the zero-width deadline is inclusive"
    );
    assert!(!game.observe_completion(None, Some(point(22, 0))).unwrap());
    game.advance_to(host_at(chosen, 1), &mapper, chosen.output_origin)
        .unwrap();
    assert_eq!(game.score().misses, 1);
    assert_eq!(game.bgm_report().remaining, 1);
    for _ in 0..8 {
        let report = mixer.render(&mut [0.0; 2]).unwrap();
        game.feed_audio(mixer.frame_cursor(), 8).unwrap();
        deliver(&mut game, &mut producer, 8);
        assert!(
            !game
                .observe_completion(Some(report), Some(point(22, 0)))
                .unwrap()
        );
    }
    assert_eq!(mixer.frame_cursor(), 16);
    assert_eq!(game.bgm_report().remaining, 0);
    assert_eq!(game.bgm_report().outstanding, 1);
    let mut pcm = [0.0];
    let head = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0625]);
    game.feed_audio(mixer.frame_cursor(), 8).unwrap();
    assert_eq!(game.bgm_report().outstanding, 0);
    assert!(
        !game
            .observe_completion(Some(head), Some(point(22, 0)))
            .unwrap()
    );
    let tail = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.03125]);
    assert!(
        !game
            .observe_completion(Some(tail), Some(point(22, 4_499_999_999)))
            .unwrap()
    );
    let silence = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0]);
    assert!(
        game.observe_completion(Some(silence), Some(point(22, 4_500_000_000)))
            .unwrap()
    );
}

#[test]
fn malformed_completion_evidence_fences_with_original_report_and_score_retained() {
    for case in 0..11 {
        let chosen = config();
        let (mut game, bank) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
        let (_producer, mut mixer) = make_mixer(bank, chosen);
        let mut rendered = mixer.render(&mut [0.0]).unwrap();
        let mut presented = point(22, 1_000_000_100);
        match case {
            0 => presented.domain = ClockDomainId(11),
            1 => presented.timestamp = Timestamp::from_nanos(i64::MIN),
            2 => rendered.paused = true,
            3 => rendered.producer_disconnected = true,
            4 => rendered.playback_end_physical_frame = Some(1),
            5 => rendered.playback_frames = 0,
            6 => rendered.counters.rendered_frames = 0,
            7 => rendered.counters.unknown_samples = 1,
            8 => {
                rendered.start_frame = u64::MAX;
                rendered.playback_start_frame = u64::MAX;
            }
            9 | 10 => {
                assert!(
                    !game
                        .observe_completion(Some(rendered), Some(presented))
                        .unwrap()
                );
                if case == 9 {
                    presented.timestamp = Timestamp::from_nanos(1_000_000_099);
                } else {
                    rendered.song_position = Timestamp::from_nanos(1);
                }
            }
            _ => unreachable!(),
        }
        let score = game.score().clone();
        let song = game.song_time();
        let hash = game.judge().stable_hash().unwrap();
        match game
            .observe_completion(Some(rendered), Some(presented))
            .unwrap_err()
        {
            StepGameplayError::Completion {
                rendered: actual_rendered,
                presented: actual_presented,
                ..
            } => {
                assert_eq!(actual_rendered, Some(rendered));
                assert_eq!(actual_presented, Some(presented));
            }
            error => panic!("expected retained completion evidence: {error:?}"),
        }
        assert_eq!(game.score(), &score);
        assert_eq!(game.song_time(), song);
        assert_eq!(game.judge().stable_hash().unwrap(), hash);
        assert_fenced(&mut game, chosen);
    }
}

#[test]
fn absent_and_repeated_output_cannot_complete_even_an_empty_or_multiweek_chart() {
    let chosen = zero_preroll();
    let (mut empty, bank) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
    let (_producer, mut mixer) = make_mixer(bank, chosen);
    let presented = Some(point(22, 1_000_000_000));
    assert!(!empty.observe_completion(None, presented).unwrap());
    let zero = mixer.render(&mut []).unwrap();
    assert!(!empty.observe_completion(Some(zero), presented).unwrap());
    let first = mixer.render(&mut [0.0]).unwrap();
    assert!(!empty.observe_completion(Some(first), presented).unwrap());
    assert!(!empty.observe_completion(Some(first), presented).unwrap());
    let idle = mixer.render(&mut [0.0]).unwrap();
    assert!(!empty.observe_completion(Some(idle), None).unwrap());
    assert!(!empty.observe_completion(None, presented).unwrap());
    assert!(empty.observe_completion(Some(idle), presented).unwrap());

    let long = prepared("#BPM 0.1\n#99911:01\n");
    let deadline = long.compiled.chart.objects()[0].time.start.as_nanos();
    assert!(deadline > 7 * 24 * 3600 * 1_000_000_000);
    let (mut game, bank) = StepGameplay::new(long, chosen, bindings(false)).unwrap();
    let (_producer, mut mixer) = make_mixer(bank, chosen);
    let far = Some(point(22, deadline + 10_000_000_000));
    let first = mixer.render(&mut [0.0]).unwrap();
    let idle = mixer.render(&mut [0.0]).unwrap();
    assert!(!game.observe_completion(Some(first), far).unwrap());
    assert!(!game.observe_completion(Some(idle), far).unwrap());
    assert_eq!((game.score().hits, game.score().misses), (0, 0));
    assert!(!game.failed());
}

fn clock_policy() -> DisciplineConfig {
    DisciplineConfig {
        capacity: 4,
        retention_interval: Duration::from_nanos(1_000_000_000),
        max_observation_age: Duration::from_nanos(1_000_000_000),
        ..DisciplineConfig::default()
    }
}

fn observed(chosen: StepGameplayConfig, output_ns: i64, host_ns: i64) -> ClockPair {
    ClockPair {
        source: point(22, chosen.output_origin.timestamp.as_nanos() + output_ns),
        target: point(11, chosen.host_origin.timestamp.as_nanos() + host_ns),
    }
}

#[test]
fn output_discipline_preserves_watermark_continuity_historical_phase_and_real_judging() {
    let chosen = StepGameplayConfig {
        early_ns: 1_000_000,
        late_ns: 1_000_000,
        ..zero_preroll()
    };
    let mapper = clocks(chosen);
    let (mut game, _) =
        StepGameplay::new(prepared("#00011:01\n#00112:01\n"), chosen, bindings(false)).unwrap();
    game.configure_output_clock(clock_policy()).unwrap();
    game.activate(chosen.host_origin).unwrap();
    let physical = input(chosen, 1, 4, 1, 0, ButtonState::Down);
    let first = game
        .process_input(physical.clone(), &mapper, chosen.output_origin)
        .unwrap();
    assert_eq!(first.song_time, Timestamp::ZERO);
    assert_eq!(game.score().hits, 1);
    game.observe_output_clock(observed(chosen, 1_000_100_000, 1_000_000_000))
        .unwrap();
    game.advance_to(
        host_at(chosen, 1_000_000_000),
        &mapper,
        chosen.output_origin,
    )
    .unwrap();
    assert_eq!(
        game.update_output_clock(host_at(chosen, 1_000_000_000))
            .unwrap(),
        Some(DisciplineUpdate::Warmup { span_ns: 0 })
    );
    game.observe_output_clock(observed(chosen, 2_000_200_000, 2_000_000_000))
        .unwrap();
    let at = host_at(chosen, 2_000_000_000);
    let boundary = game.advance_to(at, &mapper, chosen.output_origin).unwrap();
    let expected = DisciplineUpdate::Applied {
        base_rate_ppm: 100,
        correction_ppm: 20,
        applied_rate_ppm: 120,
        phase_error_ns: 200_000,
        limited: false,
    };
    assert_eq!(game.update_output_clock(at).unwrap(), Some(expected));
    assert_eq!(game.song_time(), boundary.song_time);
    assert_eq!(
        game.advance_to(at, &mapper, chosen.output_origin)
            .unwrap()
            .song_time,
        Timestamp::from_nanos(2_000_000_000)
    );
    let later = host_at(chosen, 3_000_000_000);
    assert_eq!(
        game.advance_to(later, &mapper, chosen.output_origin)
            .unwrap()
            .song_time,
        Timestamp::from_nanos(3_000_120_000)
    );
    // The latest observation still points at host 2 s. Its phase must use the
    // preserved historical segment, even after correcting the future at 2 s.
    assert_eq!(game.update_output_clock(later).unwrap(), Some(expected));
    let second = game
        .process_input(
            input(chosen, 1, 5, 2, 4_000_000_000, ButtonState::Down),
            &mapper,
            output_at(chosen, 4_000_000_000),
        )
        .unwrap();
    assert_eq!(second.song_time, Timestamp::from_nanos(4_000_240_000));
    assert_eq!(second.audio_at, point(22, 4_100_000_000));
    assert!(matches!(second.judge_events[0].outcome,
        JudgeOutcome::Hit { delta, .. } if delta == Duration::from_nanos(240_000)));
    assert_eq!(game.score().hits, 2);
    assert_eq!(first.input, Some(physical));
    assert_eq!(first.song_time, Timestamp::ZERO);
    assert!(!game.failed());
}

#[test]
fn missing_and_stale_observations_skip_correction_and_duplicates_do_not_refresh_age() {
    let chosen = zero_preroll();
    let mapper = clocks(chosen);
    let (mut game, _) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
    game.configure_output_clock(clock_policy()).unwrap();
    game.activate(chosen.host_origin).unwrap();
    game.advance_to(chosen.host_origin, &mapper, chosen.output_origin)
        .unwrap();
    assert_eq!(game.update_output_clock(chosen.host_origin).unwrap(), None);
    for second in 1..=2 {
        assert_eq!(
            game.observe_output_clock(observed(
                chosen,
                second * 1_000_000_000,
                second * 1_000_000_000
            ))
            .unwrap(),
            ObservationAdmission::Retained
        );
        let at = host_at(chosen, second * 1_000_000_000);
        game.advance_to(at, &mapper, chosen.output_origin).unwrap();
        assert!(game.update_output_clock(at).unwrap().is_some());
    }
    assert_eq!(
        game.observe_output_clock(observed(chosen, 2_000_000_000, 2_000_000_000))
            .unwrap(),
        ObservationAdmission::Unchanged
    );
    assert_eq!(
        game.observe_output_clock(observed(chosen, 2_000_000_000, 2_999_000_000))
            .unwrap(),
        ObservationAdmission::Unchanged
    );
    let stale = host_at(chosen, 3_000_000_001);
    game.advance_to(stale, &mapper, chosen.output_origin)
        .unwrap();
    assert_eq!(game.update_output_clock(stale).unwrap(), None);
    assert_eq!(game.song_time(), Timestamp::from_nanos(3_000_000_001));
    // More observations than the retained ring capacity preserve normal motion.
    for second in 4..=9 {
        assert_eq!(
            game.observe_output_clock(observed(
                chosen,
                second * 1_000_000_000,
                second * 1_000_000_000
            ))
            .unwrap(),
            ObservationAdmission::Retained
        );
        let at = host_at(chosen, second * 1_000_000_000);
        game.advance_to(at, &mapper, chosen.output_origin).unwrap();
        assert!(matches!(
            game.update_output_clock(at).unwrap(),
            Some(DisciplineUpdate::Applied {
                applied_rate_ppm: 0,
                phase_error_ns: 0,
                ..
            })
        ));
    }
    assert!(!game.failed());
}

#[test]
fn clock_faults_fence_committed_score_without_retiming_or_replaying_commands() {
    for case in 0..9 {
        let chosen = zero_preroll();
        let mapper = clocks(chosen);
        let (mut game, _) =
            StepGameplay::new(prepared("#00011:01\n"), chosen, bindings(false)).unwrap();
        game.configure_output_clock(clock_policy()).unwrap();
        game.activate(chosen.host_origin).unwrap();
        game.process_input(
            input(chosen, 1, 4, 1, 0, ButtonState::Down),
            &mapper,
            chosen.output_origin,
        )
        .unwrap();
        game.observe_output_clock(observed(chosen, if case == 5 { 300_000_000 } else { 0 }, 0))
            .unwrap();
        let at = host_at(chosen, 1_000_000_000);
        game.advance_to(at, &mapper, chosen.output_origin).unwrap();
        if case == 8 {
            game.process_input(
                input(chosen, 1, 99, 2, 1_000_000_000, ButtonState::Down),
                &mapper,
                chosen.output_origin,
            )
            .unwrap();
        }
        let score = game.score().clone();
        let song = game.song_time();
        let hash = game.judge().stable_hash().unwrap();
        let (error, expected) = match case {
            0 => {
                let mut pair = observed(chosen, 1_000_000_000, 1_000_000_000);
                pair.source.domain = ClockDomainId(33);
                (
                    game.observe_output_clock(pair).unwrap_err(),
                    DisciplineError::DomainMismatch,
                )
            }
            1 => (
                game.observe_output_clock(observed(chosen, -1, 1))
                    .unwrap_err(),
                DisciplineError::NonIncreasing,
            ),
            2 => (
                game.observe_output_clock(observed(chosen, 1, -1))
                    .unwrap_err(),
                DisciplineError::NonIncreasing,
            ),
            3 => (
                game.observe_output_clock(observed(chosen, 1, 0))
                    .unwrap_err(),
                DisciplineError::NonIncreasing,
            ),
            4 | 5 => {
                game.observe_output_clock(observed(
                    chosen,
                    if case == 4 {
                        1_010_000_000
                    } else {
                        1_300_000_000
                    },
                    1_000_000_000,
                ))
                .unwrap();
                (
                    game.update_output_clock(at).unwrap_err(),
                    if case == 4 {
                        DisciplineError::BaseRateOutOfBounds
                    } else {
                        DisciplineError::PhaseErrorTooLarge
                    },
                )
            }
            6 => (
                game.update_output_clock(point(33, at.timestamp.as_nanos()))
                    .unwrap_err(),
                DisciplineError::DomainMismatch,
            ),
            7 => (
                game.update_output_clock(host_at(chosen, 999_999_999))
                    .unwrap_err(),
                DisciplineError::NonIncreasing,
            ),
            _ => (
                game.update_output_clock(at).unwrap_err(),
                DisciplineError::NonIncreasing,
            ),
        };
        assert!(matches!(error, StepGameplayError::Clock(actual) if actual == expected));
        assert_eq!(game.score(), &score);
        assert_eq!(game.song_time(), song);
        assert_eq!(game.judge().stable_hash().unwrap(), hash);
        assert_eq!(game.score().hits, 1);
        assert_fenced(&mut game, chosen);
    }
}

#[test]
fn clock_configuration_is_atomic_setup_and_actual_host_may_precede_nominal_activation() {
    let chosen = zero_preroll();
    let (mut game, _) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
    assert!(matches!(
        game.configure_output_clock(DisciplineConfig {
            capacity: 1,
            ..clock_policy()
        }),
        Err(StepGameplayError::Clock(DisciplineError::InvalidConfig))
    ));
    assert!(!game.failed());
    game.configure_output_clock(clock_policy()).unwrap();
    assert!(matches!(
        game.configure_output_clock(clock_policy()),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    let activated = StepGameplayConfig {
        host_origin: point(11, 15_000_000_000),
        ..chosen
    };
    game.activate(activated.host_origin).unwrap();
    assert_eq!(
        game.observe_output_clock(observed(activated, 0, -1_000_000))
            .unwrap(),
        ObservationAdmission::Retained
    );
    game.advance_to(
        activated.host_origin,
        &clocks(activated),
        chosen.output_origin,
    )
    .unwrap();
    assert_eq!(
        game.update_output_clock(activated.host_origin).unwrap(),
        Some(DisciplineUpdate::Warmup { span_ns: 0 })
    );
    game.observe_output_clock(observed(activated, 1_000_000_000, 999_000_000))
        .unwrap();
    let at = host_at(activated, 1_000_000_000);
    game.advance_to(at, &clocks(activated), chosen.output_origin)
        .unwrap();
    assert!(matches!(
        game.update_output_clock(at).unwrap(),
        Some(DisciplineUpdate::Applied {
            base_rate_ppm: 0,
            phase_error_ns: 1_000_000,
            applied_rate_ppm: 100,
            ..
        })
    ));
    assert_eq!(game.song_time(), Timestamp::from_nanos(1_000_000_000));

    let (mut nominal, _) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
    nominal
        .advance_to(chosen.host_origin, &clocks(chosen), chosen.output_origin)
        .unwrap();
    assert!(matches!(
        nominal.configure_output_clock(clock_policy()),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert_eq!(
        nominal.update_output_clock(chosen.host_origin).unwrap(),
        None
    );
    assert!(!nominal.failed());
    for observe in [false, true] {
        let (mut premature, _) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
        premature.configure_output_clock(clock_policy()).unwrap();
        let failure = if observe {
            premature
                .observe_output_clock(observed(chosen, 0, 0))
                .unwrap_err()
        } else {
            premature
                .update_output_clock(chosen.host_origin)
                .unwrap_err()
        };
        assert!(matches!(
            failure,
            StepGameplayError::Clock(DisciplineError::InvalidConfig)
        ));
        assert_fenced(&mut premature, chosen);
    }
}

fn capture_limits(bytes: usize, records: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        bytes,
        records,
        bytes.min(4096),
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}

#[test]
fn captured_seeded_and_drift_corrected_operations_reconstruct_the_same_actual_judge() {
    let seed = u64::MAX;
    let lines = "#00011:01\n#RANDOM 2\n#IF 1\n#00112:01\n#ELSE\n#00113:01\n#ENDIF\n#ENDRANDOM\n";
    let prepared = prepared_seed(lines, seed);
    let lane = prepared
        .source
        .notes
        .iter()
        .find(|note| note.lane.channel() != 0x11)
        .unwrap()
        .lane
        .channel();
    let source = prepared_seed(lines, seed).source;
    let chosen = StepGameplayConfig {
        early_ns: 1_000_000,
        late_ns: 1_000_000,
        ..zero_preroll()
    };
    let mapper = clocks(chosen);
    let limits = capture_limits(65536, 32);
    let (mut game, _) = StepGameplay::new(prepared, chosen, bindings(false)).unwrap();
    game.configure_capture(limits, seed).unwrap();
    game.configure_output_clock(clock_policy()).unwrap();
    game.activate(chosen.host_origin).unwrap();
    assert!(matches!(
        game.take_replay(),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    let first = input(chosen, 77, 4, 1, 0, ButtonState::Down);
    let mut judged = game
        .process_input(first.clone(), &mapper, chosen.output_origin)
        .unwrap()
        .judge_events;
    for (host, output) in [
        (1_000_000_000, 1_000_100_000),
        (2_000_000_000, 2_000_200_000),
    ] {
        game.observe_output_clock(observed(chosen, output, host))
            .unwrap();
        judged.extend(
            game.advance_to(host_at(chosen, host), &mapper, chosen.output_origin)
                .unwrap()
                .judge_events,
        );
        game.update_output_clock(host_at(chosen, host)).unwrap();
    }
    judged.extend(
        game.advance_to(
            host_at(chosen, 3_000_000_000),
            &mapper,
            chosen.output_origin,
        )
        .unwrap()
        .judge_events,
    );
    let last_input = input(
        chosen,
        77,
        u16::from(lane - 0x11) + 4,
        2,
        4_000_000_000,
        ButtonState::Down,
    );
    let report = game
        .process_input(
            last_input.clone(),
            &mapper,
            output_at(chosen, 4_000_000_000),
        )
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(4_000_240_000));
    judged.extend(report.judge_events);
    judged.extend(
        game.advance_to(
            host_at(chosen, 5_000_000_000),
            &mapper,
            chosen.output_origin,
        )
        .unwrap()
        .judge_events,
    );
    let live_hash = game.judge().stable_hash().unwrap();
    assert_eq!(game.score().hits, 2);
    game.fail();
    let bytes = game.take_replay().unwrap().unwrap();
    assert!(game.take_replay().unwrap().is_none());
    let file = decode_replay(&bytes, limits).unwrap();
    assert_eq!(encode_replay(&file, limits).unwrap(), bytes);
    assert_eq!(decode_chart_setup(&file.header.options).unwrap().2, seed);
    assert_eq!(
        file.header.seed, 0,
        "chart branch seed is separate from the builtin rule seed"
    );
    assert_eq!(file.header.normalized_clock, chosen.host_origin.domain);
    assert_eq!(file.records.len(), 6);
    assert_eq!(
        file.records[3].song_time,
        Timestamp::from_nanos(3_000_120_000)
    );
    assert_eq!(
        file.records[4].song_time,
        Timestamp::from_nanos(4_000_240_000)
    );
    for (index, expected) in [(0, &first), (4, &last_input)] {
        let ReplayOperation::Input(event) = &file.records[index].operation else {
            panic!("actual bound input missing")
        };
        assert_eq!(&event.physical, expected);
    }
    let replay = reconstruct(&source, file, limits).unwrap();
    assert_eq!(replay.results(), judged.as_slice());
    assert_eq!(replay.engine().stable_hash().unwrap(), live_hash);
}

#[test]
fn capture_record_and_byte_limits_preserve_the_exact_prefix_after_a_committed_report() {
    let chosen = zero_preroll();
    let (mut baseline, _) =
        StepGameplay::new(prepared("#00011:01\n"), chosen, bindings(false)).unwrap();
    let generous = capture_limits(65536, 32);
    baseline.configure_capture(generous, 0).unwrap();
    let event = input(chosen, 1, 4, 1, 0, ButtonState::Down);
    baseline
        .process_input(event.clone(), &clocks(chosen), chosen.output_origin)
        .unwrap();
    baseline.fail();
    let prefix = baseline.take_replay().unwrap().unwrap();
    for record_cap in [false, true] {
        let limits = if record_cap {
            capture_limits(65536, 1)
        } else {
            capture_limits(prefix.len(), 32)
        };
        let (mut game, _) =
            StepGameplay::new(prepared("#00011:01\n"), chosen, bindings(false)).unwrap();
        game.configure_capture(limits, 0).unwrap();
        game.process_input(event.clone(), &clocks(chosen), chosen.output_origin)
            .unwrap();
        let error = game
            .advance_to(host_at(chosen, 1), &clocks(chosen), chosen.output_origin)
            .unwrap_err();
        let StepGameplayError::Capture {
            error: CaptureError::Codec(error),
            report: Some(report),
        } = error
        else {
            panic!("expected committed report and canonical capture failure")
        };
        assert_eq!(
            error,
            if record_cap {
                ReplayCodecError::TooManyRecords
            } else {
                ReplayCodecError::FileTooLarge
            }
        );
        assert!(report.input.is_none());
        assert_eq!(report.song_time, Timestamp::from_nanos(1));
        assert_eq!(game.song_time(), report.song_time);
        assert_eq!(game.score().hits, 1);
        assert_fenced(&mut game, chosen);
        assert_eq!(game.take_replay().unwrap().unwrap(), prefix);
        assert!(game.take_replay().unwrap().is_none());
        let file = decode_replay(&prefix, generous).unwrap();
        assert_eq!(file.records.len(), 1);
        assert_eq!(file.records[0].song_time, Timestamp::ZERO);
    }
}

#[test]
fn simultaneous_audio_and_capture_failure_keeps_each_actual_committed_prefix_distinct() {
    let chosen = StepGameplayConfig {
        command_capacity: 2,
        bgm_pending: 1,
        ..zero_preroll()
    };
    let lines = "#00011:01\n#00012:01\n#00001:02\n";
    for records in [1, 8] {
        let limits = capture_limits(65536, records);
        let source = prepared(lines).source;
        let (mut game, _) = StepGameplay::new(prepared(lines), chosen, bindings(true)).unwrap();
        game.configure_capture(limits, 0).unwrap();
        let error = game
            .process_input(
                input(chosen, 1, 4, 1, 0, ButtonState::Down),
                &clocks(chosen),
                chosen.output_origin,
            )
            .unwrap_err();
        let StepGameplayError::Report {
            report,
            score_error,
            capture_error,
        } = error
        else {
            panic!("actual bounded audio queue should reject its second sound")
        };
        assert!(score_error.is_none());
        assert_eq!(report.audio_commands.len(), 1);
        assert_eq!(report.audio_failures.len(), 1);
        assert_eq!(report.judge_events.len(), 2);
        assert_eq!(game.score().hits, 2);
        if records == 1 {
            assert!(matches!(
                capture_error,
                Some(CaptureError::Codec(ReplayCodecError::TooManyRecords))
            ));
        } else {
            assert!(capture_error.is_none());
        }
        assert_fenced(&mut game, chosen);
        let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits).unwrap();
        assert_eq!(file.records.len(), if records == 1 { 0 } else { 2 });
        let replay = reconstruct(&source, file, limits).unwrap();
        if records == 1 {
            assert!(
                replay.results().is_empty(),
                "capture rejects a whole report, without inventing a saved partial fanout"
            );
        } else {
            assert_eq!(replay.results(), report.judge_events.as_slice());
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                game.judge().stable_hash().unwrap()
            );
        }
    }
}

#[test]
fn capture_is_optional_atomic_during_setup_and_exported_only_once_after_stop() {
    let chosen = config();
    let (mut game, _) =
        StepGameplay::new(prepared("#00011:01\n"), chosen, bindings(false)).unwrap();
    let tiny_header =
        ReplayCodecLimits::new(65536, 8, 1, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    assert!(matches!(
        game.configure_capture(tiny_header, 0),
        Err(StepGameplayError::Capture {
            error: CaptureError::Codec(ReplayCodecError::HeaderTooLarge),
            report: None,
        })
    ));
    assert!(!game.failed());
    game.activate(chosen.host_origin).unwrap();
    let limits = capture_limits(65536, 8);
    game.configure_capture(limits, 0).unwrap();
    assert!(matches!(
        game.configure_capture(limits, 0),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    game.advance_to(chosen.host_origin, &clocks(chosen), chosen.output_origin)
        .unwrap();
    assert!(matches!(
        game.take_replay(),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    let event = input(chosen, 1, 4, 1, 0, ButtonState::Down);
    let hit = game
        .process_input(event.clone(), &clocks(chosen), output_at(chosen, 0))
        .unwrap();
    let live_hash = game.judge().stable_hash().unwrap();
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits).unwrap();
    assert_eq!(file.records.len(), 2);
    assert_eq!(
        file.records[0].song_time,
        Timestamp::from_nanos(-250_000_001)
    );
    assert!(matches!(
        file.records[0].operation,
        ReplayOperation::Advance
    ));
    assert_eq!(file.records[1].song_time, Timestamp::ZERO);
    assert!(
        matches!(&file.records[1].operation, ReplayOperation::Input(recorded) if recorded.physical == event)
    );
    let replay = reconstruct(&prepared("#00011:01\n").source, file, limits).unwrap();
    assert_eq!(replay.results(), hit.judge_events.as_slice());
    assert_eq!(replay.engine().stable_hash().unwrap(), live_hash);
    assert!(game.take_replay().unwrap().is_none());

    let (mut disabled, _) = StepGameplay::new(prepared(""), chosen, bindings(false)).unwrap();
    assert!(matches!(
        disabled.take_replay(),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    disabled
        .advance_to(chosen.host_origin, &clocks(chosen), chosen.output_origin)
        .unwrap();
    assert!(matches!(
        disabled.configure_capture(limits, 0),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert!(!disabled.failed());
    disabled.fail();
    assert!(disabled.take_replay().unwrap().is_none());
    assert!(disabled.take_replay().unwrap().is_none());
}

fn section_selected(lines: &str, start: Timestamp, seed: u64) -> PreparedBms {
    crate::section_start::prepare_at(
        prepared_seed(lines, seed),
        start,
        PcmLimits::new(256, 1024, 8).unwrap(),
    )
    .unwrap()
    .0
}

#[test]
fn section_original_pcm_and_heads_map_once_onto_the_prerolled_output_grid() {
    let chart = "#BPM 60\n#VOLWAV 50\n#LNTYPE 1\n#WAV01 key.wav\n#WAV02 bgm.wav\n\
        #00012:01\n#00053:0101\n#00011:0000000100000000\n#00001:02\n";
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files.insert("key.wav", wav(&[8192, -4096])).unwrap();
    files
        .insert(
            "bgm.wav",
            wav(&[4096, 8192, 12288, 16384, 20480, 24576, 28672, 32767]),
        )
        .unwrap();
    let original = prepare_from_source(
        chart.as_bytes(),
        &files.scope("chart.bms").unwrap(),
        AudioFormat::new(4, 1).unwrap(),
        PcmLimits::new(256, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        0,
        None,
    )
    .unwrap();
    let original_source = original.source.clone();
    let original_pcm = original.bank.get(SampleId(2)).unwrap().samples().to_vec();
    let start = Timestamp::from_nanos(1_125_000_000);
    let (selected, selection) =
        crate::section_start::prepare_at(original, start, PcmLimits::new(256, 1024, 8).unwrap())
            .unwrap();
    assert_eq!(
        (
            selection.excluded_objects,
            selection.excluded_crossing_holds
        ),
        (2, 1)
    );
    assert_eq!(selection.tails.len(), 1);
    let tail = &selection.tails[0];
    assert_eq!(tail.frame, 5);
    assert_eq!(tail.applied_song_time, Timestamp::from_nanos(1_250_000_000));
    assert_eq!(tail.correction_ns, 125_000_000);
    assert_eq!(
        selected.bank.get(tail.source).unwrap().samples(),
        original_pcm
    );
    assert_eq!(
        selected.bank.get(tail.suffix).unwrap().samples(),
        &original_pcm[5..]
    );
    assert_eq!(original_source.notes.len(), 3);
    assert_eq!(selected.source.notes.len(), 1);
    assert_eq!(
        selected.compiled.chart.objects()[0].time.start,
        Timestamp::from_nanos(1_500_000_000)
    );
    let chosen = config();
    let (mut game, bank) = StepGameplay::new_at(selected, chosen, bindings(false), start).unwrap();
    assert_eq!(game.song_time(), Timestamp::from_nanos(874_999_999));
    let (mut producer, mut mixer) = make_mixer(bank, chosen);
    let background = deliver(&mut game, &mut producer, 8);
    assert!(
        matches!(background.as_slice(), [AudioCommand::Play { sample, at, gain: 0.5, .. }]
        if *sample == tail.suffix && *at == Timestamp::from_nanos(375_000_101))
    );
    let hit = game
        .process_input(
            input(chosen, 9, 4, 1, 375_000_000, ButtonState::Down),
            &clocks(chosen),
            point(22, 625_000_101),
        )
        .unwrap();
    assert_eq!(hit.song_time, Timestamp::from_nanos(1_500_000_000));
    assert!(matches!(
        hit.judge_events[0].outcome,
        JudgeOutcome::Hit {
            delta: Duration::ZERO,
            ..
        }
    ));
    deliver(&mut game, &mut producer, 8);
    let mut pcm = [0.0; 8];
    let rendered = mixer.render(&mut pcm).unwrap();
    assert_eq!(
        pcm,
        [
            0.0,
            0.0,
            0.375,
            0.5625,
            32767.0 / 65536.0 - 0.0625,
            0.0,
            0.0,
            0.0
        ]
    );
    assert_eq!(rendered.counters.late_commands, 0);
    game.advance_to(
        host_at(chosen, 975_000_000),
        &clocks(chosen),
        chosen.output_origin,
    )
    .unwrap();
    assert_eq!((game.score().hits, game.score().misses), (1, 0));
}

#[test]
fn section_capture_preserves_seed_original_song_times_and_canonical_reconstruction() {
    let seed = u64::MAX;
    let lines = "#00011:0101\n#RANDOM 2\n#IF 1\n#00112:01\n#ELSE\n#00113:01\n#ENDIF\n#ENDRANDOM\n";
    let source = prepared_seed(lines, seed).source;
    let start = Timestamp::from_nanos(2_000_000_000);
    let chosen = config();
    let limits = capture_limits(65536, 16);
    let (mut game, _) = StepGameplay::new_at(
        section_selected(lines, start, seed),
        chosen,
        bindings(false),
        start,
    )
    .unwrap();
    let header = game.competition_header(limits, seed).unwrap();
    let identity = game.competition_identity(limits, seed).unwrap();
    assert_eq!(
        identity,
        crate::multiplayer::competition_identity(&header, env!("CARGO_PKG_VERSION"), limits)
            .unwrap()
    );
    assert_eq!(
        (
            decode_chart_setup(&header.options).unwrap().1,
            decode_chart_setup(&header.options).unwrap().2
        ),
        (start, seed)
    );
    game.activate(chosen.host_origin).unwrap();
    game.configure_capture(limits, seed).unwrap();
    let mut events = game
        .advance_to(chosen.host_origin, &clocks(chosen), chosen.output_origin)
        .unwrap()
        .judge_events;
    let physical = input(chosen, 44, 4, 1, 0, ButtonState::Down);
    events.extend(
        game.process_input(physical.clone(), &clocks(chosen), output_at(chosen, 0))
            .unwrap()
            .judge_events,
    );
    events.extend(
        game.process_input(
            input(chosen, 44, 4, 2, 100_000_000, ButtonState::Up),
            &clocks(chosen),
            output_at(chosen, 100_000_000),
        )
        .unwrap()
        .judge_events,
    );
    events.extend(
        game.advance_to(
            host_at(chosen, 2_000_000_001),
            &clocks(chosen),
            chosen.output_origin,
        )
        .unwrap()
        .judge_events,
    );
    assert_eq!((game.score().hits, game.score().misses), (1, 1));
    let hash = game.judge().stable_hash().unwrap();
    game.fail();
    let encoded = game.take_replay().unwrap().unwrap();
    let file = decode_replay(&encoded, limits).unwrap();
    assert_eq!(file.header, header);
    assert_eq!(encode_replay(&file, limits).unwrap(), encoded);
    assert_eq!(
        file.records
            .iter()
            .map(|record| record.song_time.as_nanos())
            .collect::<Vec<_>>(),
        [1_749_999_999, 2_000_000_000, 2_100_000_000, 4_000_000_001]
    );
    assert!(
        matches!(&file.records[1].operation, ReplayOperation::Input(bound) if bound.physical == physical)
    );
    let replay = reconstruct(&source, file, limits).unwrap();
    assert_eq!(replay.results(), events);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    assert!(game.take_replay().unwrap().is_none());
}

#[test]
fn section_activation_and_output_drift_keep_original_song_anchor_and_committed_history() {
    let start = Timestamp::from_nanos(4_000_000_000);
    let chosen = StepGameplayConfig {
        early_ns: 1_000_000,
        late_ns: 1_000_000,
        ..config()
    };
    let (mut game, _) = StepGameplay::new_at(
        section_selected("#00013:01\n#00111:01\n#00212:01\n", start, 0),
        chosen,
        bindings(false),
        start,
    )
    .unwrap();
    let initial = game.song_time();
    game.configure_output_clock(clock_policy()).unwrap();
    let activated = StepGameplayConfig {
        host_origin: point(11, 15_000_000_000),
        ..chosen
    };
    game.activate(activated.host_origin).unwrap();
    assert_eq!(game.song_time(), initial);
    let first = game
        .advance_to(
            activated.host_origin,
            &clocks(activated),
            chosen.output_origin,
        )
        .unwrap();
    assert_eq!(first.song_time, Timestamp::from_nanos(3_749_999_999));
    game.process_input(
        input(activated, 1, 4, 1, 0, ButtonState::Down),
        &clocks(activated),
        chosen.output_origin,
    )
    .unwrap();
    for (host, output) in [
        (1_000_000_000, 1_000_100_000),
        (2_000_000_000, 2_000_200_000),
    ] {
        game.observe_output_clock(observed(activated, output, host))
            .unwrap();
        let at = point(11, activated.host_origin.timestamp.as_nanos() + host);
        let boundary = game
            .advance_to(at, &clocks(activated), chosen.output_origin)
            .unwrap();
        let update = game.update_output_clock(at).unwrap();
        assert_eq!(game.song_time(), boundary.song_time);
        if host == 2_000_000_000 {
            assert!(matches!(
                update,
                Some(DisciplineUpdate::Applied {
                    base_rate_ppm: 100,
                    correction_ppm: 20,
                    applied_rate_ppm: 120,
                    phase_error_ns: 200_000,
                    ..
                })
            ));
        }
    }
    let later = game
        .advance_to(
            point(11, 18_000_000_000),
            &clocks(activated),
            chosen.output_origin,
        )
        .unwrap();
    assert_eq!(later.song_time, Timestamp::from_nanos(6_750_119_999));
    let hit = game
        .process_input(
            input(activated, 1, 5, 2, 4_000_000_000, ButtonState::Down),
            &clocks(activated),
            output_at(activated, 4_000_000_000),
        )
        .unwrap();
    assert!(matches!(
        hit.judge_events[0].outcome,
        JudgeOutcome::Hit { .. }
    ));
    assert_eq!((game.score().hits, game.score().misses), (2, 0));
    assert_eq!(first.song_time, initial);
    assert!(!game.failed());
}

#[test]
fn section_constructor_refuses_unselected_material_and_invalid_clock_extents_before_ownership() {
    let start = Timestamp::from_nanos(2_000_000_000);
    for material in [
        prepared("#00011:01\n#00111:01\n"),
        prepared("#00001:02\n#00111:01\n"),
    ] {
        assert!(matches!(
            StepGameplay::new_at(material, config(), bindings(false), start),
            Err(StepGameplayError::InvalidConfiguration(_))
        ));
    }
    assert!(matches!(
        StepGameplay::new_at(
            prepared("#00111:01\n"),
            config(),
            bindings(false),
            Timestamp::from_nanos(-1)
        ),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    let original = prepared("#00011:01\n#00112:01\n");
    let stale = original.sounds[0].clone();
    let (mut selected, _) =
        crate::section_start::prepare_at(original, start, PcmLimits::new(256, 1024, 8).unwrap())
            .unwrap();
    selected.sounds.push(stale);
    assert!(matches!(
        StepGameplay::new_at(selected, config(), bindings(false), start),
        Err(StepGameplayError::Setup(_))
    ));
    for chosen in [
        StepGameplayConfig {
            host_origin: point(11, i64::MAX),
            ..config()
        },
        StepGameplayConfig {
            output_origin: point(22, i64::MAX),
            ..config()
        },
        StepGameplayConfig {
            preroll: Duration::from_nanos(-1),
            ..config()
        },
    ] {
        assert!(matches!(
            StepGameplay::new_at(
                section_selected("#00111:01\n", start, 0),
                chosen,
                bindings(false),
                start
            ),
            Err(StepGameplayError::InvalidConfiguration(_))
        ));
    }
    let (mut fresh, _) = StepGameplay::new_at(
        section_selected("#00111:01\n", start, 0),
        config(),
        bindings(false),
        start,
    )
    .unwrap();
    let initial = fresh.song_time();
    assert!(matches!(
        fresh.activate(point(11, i64::MAX)),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert_eq!(fresh.song_time(), initial);
    assert!(!fresh.failed());
    fresh.activate(config().host_origin).unwrap();
    assert_eq!(
        fresh
            .advance_to(
                config().host_origin,
                &clocks(config()),
                config().output_origin
            )
            .unwrap()
            .song_time,
        initial
    );
}

#[test]
fn zero_section_constructor_preserves_legacy_judging_pcm_headers_and_capture_bytes() {
    let chosen = config();
    let lines = "#00011:01\n#00001:02\n";
    let limits = capture_limits(65536, 16);
    let (mut legacy, legacy_bank) =
        StepGameplay::new(prepared(lines), chosen, bindings(false)).unwrap();
    let (mut explicit, explicit_bank) = StepGameplay::new_at(
        section_selected(lines, Timestamp::ZERO, 0),
        chosen,
        bindings(false),
        Timestamp::ZERO,
    )
    .unwrap();
    assert_eq!(
        legacy.competition_header(limits, 0).unwrap(),
        explicit.competition_header(limits, 0).unwrap()
    );
    let (mut legacy_producer, mut legacy_mixer) = make_mixer(legacy_bank, chosen);
    let (mut explicit_producer, mut explicit_mixer) = make_mixer(explicit_bank, chosen);
    for game in [&mut legacy, &mut explicit] {
        game.configure_capture(limits, 0).unwrap();
        game.advance_to(chosen.host_origin, &clocks(chosen), chosen.output_origin)
            .unwrap();
        game.process_input(
            input(chosen, 8, 4, 1, 0, ButtonState::Down),
            &clocks(chosen),
            output_at(chosen, 0),
        )
        .unwrap();
        game.advance_to(host_at(chosen, 1), &clocks(chosen), chosen.output_origin)
            .unwrap();
    }
    assert_eq!(legacy.score(), explicit.score());
    assert_eq!(
        legacy.judge().stable_hash().unwrap(),
        explicit.judge().stable_hash().unwrap()
    );
    assert_eq!(
        deliver(&mut legacy, &mut legacy_producer, 8),
        deliver(&mut explicit, &mut explicit_producer, 8)
    );
    let mut legacy_pcm = [0.0; 8];
    let mut explicit_pcm = [0.0; 8];
    legacy_mixer.render(&mut legacy_pcm).unwrap();
    explicit_mixer.render(&mut explicit_pcm).unwrap();
    assert_eq!(legacy_pcm, explicit_pcm);
    assert!(legacy_pcm.iter().any(|value| *value != 0.0));
    legacy.fail();
    explicit.fail();
    assert_eq!(
        legacy.take_replay().unwrap(),
        explicit.take_replay().unwrap()
    );
}

#[test]
fn section_completion_uses_original_judge_deadline_and_actual_relative_pcm_drain() {
    let start = Timestamp::from_nanos(4_000_000_000);
    let chosen = StepGameplayConfig {
        output_origin: point(22, 604_800_000_000_000),
        ..zero_preroll()
    };
    let (mut game, bank) = StepGameplay::new_at(
        section_selected("#00012:01\n#00111:01\n", start, 0),
        chosen,
        bindings(false),
        start,
    )
    .unwrap();
    game.process_input(
        input(chosen, 1, 4, 1, 0, ButtonState::Down),
        &clocks(chosen),
        chosen.output_origin,
    )
    .unwrap();
    assert_eq!(game.song_time(), start);
    let (mut producer, mut mixer) = make_mixer(bank, chosen);
    deliver(&mut game, &mut producer, 8);
    let mut pcm = [0.0];
    let head = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.125]);
    let end = point(22, chosen.output_origin.timestamp.as_nanos() + 750_000_000);
    let before_end = point(22, end.timestamp.as_nanos() - 1);
    assert!(
        !game
            .observe_completion(Some(head), Some(before_end))
            .unwrap()
    );
    game.advance_to(host_at(chosen, 1), &clocks(chosen), chosen.output_origin)
        .unwrap();
    assert_eq!(game.song_time(), Timestamp::from_nanos(4_000_000_001));
    let tail = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [-0.0625]);
    assert!(!game.observe_completion(Some(tail), None).unwrap());
    assert!(
        !game
            .observe_completion(Some(tail), Some(before_end))
            .unwrap()
    );
    let idle = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0]);
    assert!(
        !game
            .observe_completion(Some(idle), Some(before_end))
            .unwrap()
    );
    assert!(game.observe_completion(Some(idle), Some(end)).unwrap());
    assert_eq!((game.score().hits, game.score().misses), (1, 0));
}

fn prepared_long_type(kind: u8, rows: &str, seed: u64) -> PreparedBms {
    let chart = format!("#BPM 60\n#VOLWAV 50\n#LNTYPE {kind}\n#WAV01 head.wav\n{rows}");
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files.insert("head.wav", wav(&[8192, -4096])).unwrap();
    prepare_from_source(
        chart.as_bytes(),
        &files.scope("chart.bms").unwrap(),
        AudioFormat::new(4, 1).unwrap(),
        PcmLimits::new(256, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        seed,
        None,
    )
    .unwrap()
}

#[test]
fn resolved_pcm_aliases_keep_live_bgm_key_voices_and_canonical_replay_audio_identical() {
    let text = b"#BPM 60\n#VOLWAV 50\n#WAV01 shared.wav\n#WAV02 ./shared.wav\n#WAV03 shared.ogg\n#00011:01\n#00012:00020000\n#00001:03\n";
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files.insert("pack/chart.bms", text.to_vec()).unwrap();
    files
        .insert("pack/shared.wav", wav(&[8192, -4096]))
        .unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    let seed = u64::MAX;
    let prepare = || {
        prepare_from_source(
            text,
            &source,
            AudioFormat::new(4, 1).unwrap(),
            PcmLimits::new(256, 1024, 8).unwrap(),
            ChannelPolicy::Exact,
            &WavDecoder,
            AssetPathPolicy::AudioVariants,
            seed,
            None,
        )
        .unwrap()
    };
    let prepared = prepare();
    let original = prepared.source.clone();
    assert_eq!((prepared.bank.len(), prepared.bank.total_bytes()), (3, 24));
    assert_eq!(
        prepared
            .source
            .notes
            .iter()
            .map(|note| note.sample)
            .collect::<Vec<_>>(),
        [SampleId(1), SampleId(2)]
    );
    let voices = prepared
        .sounds
        .iter()
        .map(|sound| (sound.sample, sound.voice))
        .collect::<Vec<_>>();
    assert_ne!(voices[0].1, voices[1].1);
    let AudioCommand::Play {
        voice: bgm_voice,
        sample: SampleId(3),
        ..
    } = prepared.bgm_commands[0]
    else {
        panic!("the background alias must retain its own sample and voice");
    };
    assert!(voices.iter().all(|(_, voice)| *voice != bgm_voice));
    let chosen = StepGameplayConfig {
        preroll: Duration::from_nanos(250_000_000),
        ..config()
    };
    let mapper = clocks(chosen);
    let limits = capture_limits(65536, 16);
    let (mut game, bank) = StepGameplay::new(prepared, chosen, bindings(false)).unwrap();
    let header = game.competition_header(limits, seed).unwrap();
    let identity = game.competition_identity(limits, seed).unwrap();
    game.configure_capture(limits, seed).unwrap();
    let mut events = game
        .advance_to(chosen.host_origin, &mapper, chosen.output_origin)
        .unwrap()
        .judge_events;
    for (key, sequence, song) in [(4, 1, 0), (5, 2, 1_000_000_000)] {
        let report = game
            .process_input(
                input(chosen, 44, key, sequence, song, ButtonState::Down),
                &mapper,
                host_at(chosen, song),
            )
            .unwrap();
        assert_eq!(report.song_time, Timestamp::from_nanos(song));
        assert_eq!(
            report.audio_at.timestamp,
            chosen
                .output_origin
                .timestamp
                .checked_add(chosen.preroll)
                .unwrap()
                .checked_add(Duration::from_nanos(song))
                .unwrap()
        );
        events.extend(report.judge_events);
    }
    events.extend(
        game.advance_to(
            host_at(chosen, 1_000_000_001),
            &mapper,
            host_at(chosen, 1_000_000_001),
        )
        .unwrap()
        .judge_events,
    );
    assert_eq!((game.score().hits, game.score().misses), (2, 0));
    let hash = game.judge().stable_hash().unwrap();
    let (mut producer, mut mixer) = make_mixer(bank, chosen);
    let commands = deliver(&mut game, &mut producer, 2);
    assert_eq!(commands.len(), 3);
    for (command, (sample, voice, song)) in commands.iter().zip([
        (SampleId(3), bgm_voice, 0),
        (voices[0].0, voices[0].1, 0),
        (voices[1].0, voices[1].1, 1_000_000_000),
    ]) {
        assert_eq!(
            *command,
            AudioCommand::Play {
                sample,
                voice,
                gain: 0.5,
                at: chosen
                    .output_origin
                    .timestamp
                    .checked_add(chosen.preroll)
                    .unwrap()
                    .checked_add(Duration::from_nanos(song))
                    .unwrap()
            }
        );
    }
    let mut pcm = [0.0; 8];
    let rendered = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0, 0.25, -0.125, 0.0, 0.0, 0.125, -0.0625, 0.0]);
    assert_eq!(rendered.counters.commands_applied, 3);
    assert_eq!(
        (
            rendered.counters.late_commands,
            rendered.counters.unknown_samples
        ),
        (0, 0)
    );
    game.fail();
    let bytes = game.take_replay().unwrap().unwrap();
    let file = decode_replay(&bytes, limits).unwrap();
    assert_eq!(file.header, header);
    assert_eq!(decode_chart_setup(&file.header.options).unwrap().2, seed);
    assert_eq!(
        crate::multiplayer::competition_identity(&file.header, env!("CARGO_PKG_VERSION"), limits)
            .unwrap(),
        identity
    );
    assert_eq!(encode_replay(&file, limits).unwrap(), bytes);
    let replay = reconstruct(&original, file, limits).unwrap();
    assert_eq!(replay.results(), events);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    let prepared_replay = prepare();
    let plan = crate::replay_audio::plan_audio(
        &prepared_replay,
        decode_replay(&bytes, limits).unwrap(),
        limits,
        chosen.output_origin,
        chosen.preroll,
    )
    .unwrap();
    assert_eq!(plan.commands, commands);
    assert_eq!(plan.judge_events, events);
    assert_eq!(plan.final_judge_hash, hash);
    let (mut replay_producer, mut replay_mixer) = make_mixer(prepared_replay.bank, chosen);
    for command in plan.commands {
        replay_producer.try_push(command).unwrap();
    }
    let mut replay_pcm = [0.0; 8];
    let replay_rendered = replay_mixer.render(&mut replay_pcm).unwrap();
    assert_eq!(replay_pcm, pcm);
    assert_eq!(replay_rendered.counters.commands_applied, 3);
    assert_eq!(replay_rendered.counters.late_commands, 0);
}

#[test]
fn lntype2_normalized_hold_shares_real_judging_pcm_and_canonical_capture_with_paired_holds() {
    let chosen = zero_preroll();
    let limits = capture_limits(65536, 16);
    let mut outcomes = Vec::new();
    for (kind, rows) in [(1, "#00051:01000100\n"), (2, "#00051:01ZZ0000\n")] {
        let prepared = prepared_long_type(kind, rows, 3);
        assert_eq!(prepared.source.notes.len(), 1);
        let note = &prepared.source.notes[0];
        assert_eq!(note.sample, SampleId(1));
        assert_eq!(
            note.tail_sample.map(|sample| sample.0),
            if kind == 1 { Some(1) } else { None }
        );
        assert_eq!(prepared.sounds.len(), 1);
        assert_eq!(prepared.sounds[0].stage, JudgeStage::HoldHead);
        assert!(!prepared.source.samples.contains_key(&1295));
        assert_eq!(
            prepared.compiled.chart.objects()[0].time.end,
            Some(Timestamp::from_nanos(2_000_000_000))
        );
        let original = prepared.source.clone();
        let object = note.object;
        let (mut game, bank) = StepGameplay::new(prepared, chosen, bindings(false)).unwrap();
        game.configure_capture(limits, 3).unwrap();
        let physical = input(chosen, 44, 4, 1, 0, ButtonState::Down);
        let head = game
            .process_input(physical.clone(), &clocks(chosen), chosen.output_origin)
            .unwrap();
        assert_eq!(head.judge_events[0].stage, JudgeStage::HoldHead);
        assert_eq!(game.judge().state(object), Some(InteractionState::Active));
        let (mut producer, mut mixer) = make_mixer(bank, chosen);
        assert_eq!(deliver(&mut game, &mut producer, 8), head.audio_commands);
        let tail = game
            .process_input(
                input(chosen, 44, 4, 2, 2_000_000_000, ButtonState::Up),
                &clocks(chosen),
                output_at(chosen, 2_000_000_000),
            )
            .unwrap();
        assert_eq!(tail.judge_events[0].stage, JudgeStage::HoldTail);
        assert!(matches!(
            tail.judge_events[0].outcome,
            JudgeOutcome::Hit {
                delta: Duration::ZERO,
                ..
            }
        ));
        assert!(tail.audio_commands.is_empty());
        assert!(game.take_commands(8).unwrap().is_none());
        let advance = game
            .advance_to(
                host_at(chosen, 2_000_000_001),
                &clocks(chosen),
                chosen.output_origin,
            )
            .unwrap();
        assert!(advance.judge_events.is_empty());
        assert_eq!(
            game.judge().state(object),
            Some(InteractionState::Completed)
        );
        assert_eq!((game.score().hits, game.score().misses), (2, 0));
        let mut pcm = [0.0; 8];
        let report = mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.125, -0.0625, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(report.counters.commands_applied, 1);
        assert_eq!(report.counters.late_commands, 0);
        let events = [head.judge_events, tail.judge_events].concat();
        let hash = game.judge().stable_hash().unwrap();
        let score = game.score().clone();
        game.fail();
        let bytes = game.take_replay().unwrap().unwrap();
        let file = decode_replay(&bytes, limits).unwrap();
        assert_eq!(encode_replay(&file, limits).unwrap(), bytes);
        assert_eq!(decode_chart_setup(&file.header.options).unwrap().2, 3);
        assert!(
            matches!(&file.records[0].operation, ReplayOperation::Input(recorded) if recorded.physical == physical)
        );
        let replay = reconstruct(&original, file, limits).unwrap();
        assert_eq!(replay.results(), events);
        assert_eq!(replay.engine().stable_hash().unwrap(), hash);
        outcomes.push((score, events, pcm));
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "both encodings use the same core Hold behavior and sounded head"
    );
}

#[test]
fn lntype2_section_excludes_crossing_cells_and_replays_actual_early_release_without_extra_sounds() {
    let seed = 17;
    let original = prepared_long_type(2, "#00051:01ZZ0001\n", seed);
    let source = original.source.clone();
    let start = Timestamp::from_nanos(1_000_000_000);
    let (selected, selection) =
        crate::section_start::prepare_at(original, start, PcmLimits::new(256, 1024, 8).unwrap())
            .unwrap();
    assert_eq!(
        (
            selection.excluded_objects,
            selection.excluded_crossing_holds
        ),
        (1, 1)
    );
    assert_eq!(selected.source.notes.len(), 1);
    assert_eq!(
        selected.compiled.chart.objects()[0].time.start,
        Timestamp::from_nanos(3_000_000_000)
    );
    assert_eq!(
        selected.compiled.chart.objects()[0].time.end,
        Some(Timestamp::from_nanos(4_000_000_000))
    );
    let chosen = config();
    let limits = capture_limits(65536, 16);
    let (mut game, bank) = StepGameplay::new_at(selected, chosen, bindings(false), start).unwrap();
    game.configure_capture(limits, seed).unwrap();
    let initial = game
        .advance_to(chosen.host_origin, &clocks(chosen), chosen.output_origin)
        .unwrap();
    assert_eq!(initial.song_time, Timestamp::from_nanos(749_999_999));
    assert!(initial.judge_events.is_empty());
    let head = game
        .process_input(
            input(chosen, 77, 4, 1, 2_000_000_000, ButtonState::Down),
            &clocks(chosen),
            output_at(chosen, 2_000_000_000),
        )
        .unwrap();
    let release = game
        .process_input(
            input(chosen, 77, 4, 2, 2_500_000_000, ButtonState::Up),
            &clocks(chosen),
            output_at(chosen, 2_500_000_000),
        )
        .unwrap();
    assert_eq!(release.song_time, Timestamp::from_nanos(3_500_000_000));
    assert!(matches!(
        release.judge_events[0].outcome,
        JudgeOutcome::Miss {
            reason: MissReason::EarlyRelease
        }
    ));
    assert!(release.audio_commands.is_empty());
    let after = game
        .advance_to(
            host_at(chosen, 3_000_000_001),
            &clocks(chosen),
            chosen.output_origin,
        )
        .unwrap();
    assert!(after.judge_events.is_empty());
    assert_eq!((game.score().hits, game.score().misses), (1, 1));
    let (mut producer, mut mixer) = make_mixer(bank, chosen);
    assert_eq!(deliver(&mut game, &mut producer, 8), head.audio_commands);
    let mut pcm = Vec::new();
    for _ in 0..2 {
        let mut block = [0.0; 8];
        let report = mixer.render(&mut block).unwrap();
        assert_eq!(report.counters.late_commands, 0);
        pcm.extend(block);
    }
    let mut expected = vec![0.0; 16];
    expected[10] = 0.125;
    expected[11] = -0.0625;
    assert_eq!(pcm, expected);
    let events = [head.judge_events, release.judge_events].concat();
    let hash = game.judge().stable_hash().unwrap();
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits).unwrap();
    let (_, recorded_start, recorded_seed) = decode_chart_setup(&file.header.options).unwrap();
    assert_eq!((recorded_start, recorded_seed), (start, seed));
    assert_eq!(
        file.records
            .iter()
            .map(|record| record.song_time.as_nanos())
            .collect::<Vec<_>>(),
        [749_999_999, 3_000_000_000, 3_500_000_000, 4_000_000_001]
    );
    let replay = reconstruct(&source, file, limits).unwrap();
    assert_eq!(replay.results(), events);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
}
