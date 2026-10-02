//! Actual portable preparation, gameplay, command queues and Mixer; no native I/O.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    bgm::BgmFeedError,
    local_runtime::FailureKind,
    prepare_from_source,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        QueuePushError, SampleBank, SampleId, command_queue,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector,
        EventMeta, GameControlId, NativeEventMeta, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InteractionState,
    judge::{JudgeGrade, JudgeOutcome, JudgeStage, MissReason},
    runtime::RuntimeError,
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
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
        0,
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
    } = error
    else {
        panic!("expected committed partial report: {error:?}")
    };
    assert!(score_error.is_none());
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
