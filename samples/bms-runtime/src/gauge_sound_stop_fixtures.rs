//! Deferred typed stepped owners: sound-stop admission is not native output completion.
use crate::{
    PreparedBms,
    gauge::GaugeFailure,
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    replay_playback::reconstruct,
    replay_visual::ReplayVisual,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepLocalGameplay, StepLocalGameplayError},
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        PcmSample, QueuePushError, SampleBank, SampleId, VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{HazardOutcome, JudgeStage},
    replay::codec::{decode_replay, ReplayCodecLimits},
    runtime::SoundBinding,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{parse, BmsChart, BmsInputMode};
const SECOND: i64 = 1_000_000_000;
const HOST: i64 = 10 * SECOND;
const OUTPUT: i64 = 20 * SECOND;
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn config(capacity: usize) -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(1, HOST),
        output_origin: point(2, OUTPUT),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: capacity,
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(SECOND),
        telemetry_capacity: 0,
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn bindings(device: DeviceSelector, lanes: &[u32]) -> BindingMap {
    BindingMap::from_bindings(lanes.iter().map(|&lane| Binding {
        device,
        physical: PhysicalControlId::keyboard(91u16),
        game_control: GameControlId(lane),
    }))
    .unwrap()
}
fn input(device: u64, song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, HOST + song), sequence),
        control: PhysicalControlId::keyboard(91u16),
        state,
    })
}
fn prepared(source: BmsChart) -> PreparedBms {
    let compiled = source.compile().unwrap();
    assert!(compiled.bgm.is_empty());
    let format = AudioFormat::new(10, 1).unwrap();
    let limits = PcmLimits::new(160, 480, 3).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    for id in [0, 1, 2] {
        if source.samples.contains_key(&id) {
            bank.insert(
                SampleId(u64::from(id)),
                PcmSample::new(format, vec![0.25; 40], limits).unwrap(),
            )
            .unwrap();
        }
    }
    let sounds = source
        .notes
        .iter()
        .map(|note| {
            let object = compiled
                .chart
                .objects()
                .iter()
                .find(|object| object.id == note.object)
                .unwrap();
            SoundBinding {
                object: note.object,
                stage: if object.time.end.is_some() {
                    JudgeStage::HoldHead
                } else {
                    JudgeStage::Instant
                },
                sample: note.sample,
                voice: VoiceId(10),
                gain: 1.0,
            }
        })
        .collect();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands: vec![],
    }
}
fn stop(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(at),
    }
}

#[test]
fn solo_fatal_advance_appends_future_safe_stops_and_capture_keeps_only_the_committed_prefix() {
    let source = parse("#BPM 60\n#LNTYPE 1\n#WAV00 mine\n#WAV01 head\n#WAV02 press\n#00051:01000001\n#00032:02\n#000D1:01ZZ0000",
        Default::default()).unwrap();
    let (mut game, bank) = StepGameplay::new(
        prepared(source.clone()),
        config(16),
        bindings(DeviceSelector::Any, &[0x11, 0x12]),
    )
    .unwrap();
    game.configure_capture(limits(), 0).unwrap();
    game.activate(point(1, HOST)).unwrap();
    let first = game
        .process_input(
            input(u64::MAX, 0, 1, ButtonState::Down),
            &Identity,
            point(2, OUTPUT + 2 * SECOND),
        )
        .unwrap();
    assert_eq!(first.judge_events[0].stage, JudgeStage::HoldHead);
    assert_eq!(first.audio_commands.len(), 3);
    assert!(matches!(
        first.audio_commands.as_slice(),
        [
            AudioCommand::Play {
                sample: SampleId(1),
                voice: VoiceId(10),
                ..
            },
            AudioCommand::Play {
                sample: SampleId(2),
                voice: VoiceId(11),
                ..
            },
            AudioCommand::Play {
                sample: SampleId(0),
                voice: VoiceId(12),
                ..
            },
        ]
    ));
    let fatal = game
        .advance_to(
            point(1, HOST + SECOND),
            &Identity,
            point(2, OUTPUT + SECOND),
        )
        .unwrap();
    assert_eq!(fatal.hazard_events.len(), 1);
    assert_eq!(fatal.hazard_events[0].value, 1295);
    assert_eq!(fatal.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert_eq!(fatal.audio_at, point(2, OUTPUT + SECOND));
    assert_eq!(
        fatal.audio_commands,
        [
            stop(10, OUTPUT + 2 * SECOND),
            stop(11, OUTPUT + 2 * SECOND),
            stop(12, OUTPUT + 2 * SECOND)
        ]
    );
    assert!(fatal.audio_failures.is_empty());
    assert_eq!(game.gameplay_fence(), Some(ts(SECOND)));
    assert_eq!(
        game.gauge().snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    assert!(!game.failed());
    let hash = game.judge().stable_hash().unwrap();
    let later = game
        .process_input(
            input(u64::MAX, 3 * SECOND, 2, ButtonState::Up),
            &Identity,
            point(2, OUTPUT + 3 * SECOND),
        )
        .unwrap();
    assert!(later.audio_commands.is_empty() && later.audio_failures.is_empty());
    assert!(later.bound_inputs.is_empty() && later.judge_events.is_empty());
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
    let batch = game.take_commands(16).unwrap().unwrap();
    let mut expected = first.audio_commands.clone();
    expected.extend_from_slice(&fatal.audio_commands);
    assert_eq!(batch.commands, expected);
    let (mut producer, consumer) = command_queue(16).unwrap();
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            AudioFormat::new(10, 1).unwrap(),
            ClockDomainId(2),
            ts(OUTPUT),
            AudioLimits::new(16, 4, 16, 32, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut output = [0.0; 30];
    let rendered = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0; 30]); // All three plays and stops execute in order at frame 20.
    assert_eq!(rendered.counters.commands_applied, 6);
    assert_eq!(rendered.counters.unknown_stops, 0);
    assert!(
        !game
            .observe_completion(Some(rendered), Some(point(2, OUTPUT + 3 * SECOND)))
            .unwrap()
    );
    assert!(!game.observe_completion(None, None).unwrap());
    assert!(game.take_commands(16).unwrap().is_none());
    // Stops did not synthesize the still-pending hold tail or turn the fence into completion.
    assert_eq!((game.score().hits, game.score().misses), (1, 0));
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
    assert_eq!(file.records.len(), 3); // Two successful bound pushes, then the real advance.
    assert_eq!(file.records.last().unwrap().song_time, ts(SECOND));
    let rebuilt = reconstruct(&source, file.clone(), limits()).unwrap();
    assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
    let mut visual = ReplayVisual::new(&source, &file, limits()).unwrap();
    visual.advance_to(ts(9 * SECOND)).unwrap();
    assert_eq!(visual.gauge(), game.gauge());
    assert_eq!(visual.mine_damage(), game.mine_damage());
}

#[test]
fn local_stop_admission_keeps_every_failed_member_report_and_does_not_stop_the_survivor() {
    let source = parse(
        "#BPM 60\n#WAV01 head\n#00011:01000100\n#000D1:00ZZ0000",
        Default::default(),
    )
    .unwrap();
    let players = [PlayerId(7), PlayerId(91), PlayerId(u32::MAX)];
    let devices = [u64::MAX, u64::MAX - 11, u64::MAX - 29];
    for capacity in [3, 4, 16] {
        let plan = ResolvedInputPlan::new(
            players
                .iter()
                .zip(devices)
                .map(|(&player, device)| (player, Some(DeviceId(device))))
                .collect(),
        )
        .unwrap();
        let maps = devices
            .iter()
            .map(|&device| bindings(DeviceSelector::Exact(DeviceId(device)), &[0x11]))
            .collect();
        let (mut game, bank) = StepLocalGameplay::new_section(
            prepared(source.clone()),
            config(capacity),
            plan,
            maps,
            ts(0),
            None,
            BmsInputMode::ButtonOnly,
        )
        .unwrap();
        assert_eq!(bank.len(), 1);
        for &player in &players {
            game.configure_capture(player, limits(), 0).unwrap();
        }
        game.activate(point(1, HOST)).unwrap();
        for (index, &device) in devices.iter().enumerate() {
            let InputResult::Processed(reports) = game
                .process_input(
                    input(device, 0, 1, ButtonState::Down),
                    &Identity,
                    point(2, OUTPUT),
                )
                .unwrap()
            else {
                panic!("assigned source")
            };
            assert!(matches!(reports[0].report.audio_commands.as_slice(),
                [AudioCommand::Play { voice, .. }] if *voice == VoiceId(index as u64 + 1)));
        }
        game.process_input(
            input(devices[0], SECOND / 2, 2, ButtonState::Up),
            &Identity,
            point(2, OUTPUT + SECOND / 2),
        )
        .unwrap();
        let result = game.advance_to(
            point(1, HOST + SECOND),
            &Identity,
            point(2, OUTPUT + SECOND),
        );
        let reports = if capacity == 16 {
            result.unwrap()
        } else {
            let StepLocalGameplayError::Operation {
                reports,
                group_error,
                member_errors,
            } = result.unwrap_err()
            else {
                panic!("original reports plus stop queue admission failure")
            };
            assert!(group_error.is_none() && member_errors.is_empty());
            reports
        };
        assert_eq!(reports.len(), 3);
        assert!(
            reports[0].report.audio_commands.is_empty()
                && reports[0].report.audio_failures.is_empty()
        );
        assert_eq!(
            reports[0].report.hazard_events[0].outcome,
            HazardOutcome::Avoided
        );
        assert_eq!(game.gameplay_fence(players[0]), None);
        for index in 1..3 {
            assert_eq!(
                reports[index].report.hazard_events[0].outcome,
                HazardOutcome::Triggered
            );
            assert_eq!(game.gameplay_fence(players[index]), Some(ts(SECOND)));
            assert_eq!(
                game.gauge(players[index]).unwrap().snapshot().failure,
                Some(GaugeFailure::InstantDeath)
            );
            let expected = stop(index as u64 + 1, OUTPUT + SECOND);
            if capacity == 16 || (capacity == 4 && index == 1) {
                assert_eq!(reports[index].report.audio_commands, [expected]);
                assert!(reports[index].report.audio_failures.is_empty());
            } else {
                assert!(reports[index].report.audio_commands.is_empty());
                assert_eq!(reports[index].report.audio_failures.len(), 1);
                assert_eq!(reports[index].report.audio_failures[0].command, expected);
                assert_eq!(
                    reports[index].report.audio_failures[0].reason,
                    QueuePushError::Full
                );
            }
        }
        assert_eq!(game.failed(), capacity != 16);
        if capacity == 16 {
            let InputResult::Processed(survivor) = game
                .process_input(
                    input(devices[0], 2 * SECOND, 3, ButtonState::Down),
                    &Identity,
                    point(2, OUTPUT + 2 * SECOND),
                )
                .unwrap()
            else {
                panic!("healthy survivor")
            };
            assert_eq!(survivor[0].report.judge_events.len(), 1);
            assert!(matches!(
                survivor[0].report.audio_commands.as_slice(),
                [AudioCommand::Play {
                    voice: VoiceId(1),
                    ..
                }]
            ));
            let InputResult::Processed(frozen) = game
                .process_input(
                    input(devices[2], 2 * SECOND, 2, ButtonState::Up),
                    &Identity,
                    point(2, OUTPUT + 2 * SECOND),
                )
                .unwrap()
            else {
                panic!("fenced acquisition")
            };
            assert!(
                frozen[0].report.audio_commands.is_empty()
                    && frozen[0].report.audio_failures.is_empty()
            );
            assert_eq!(game.song_time(), ts(2 * SECOND));
            game.fail();
        }
        for &player in &players[1..] {
            let hash = game.judge(player).unwrap().stable_hash().unwrap();
            let file =
                decode_replay(&game.take_replay(player).unwrap().unwrap(), limits()).unwrap();
            assert_eq!(file.records.len(), 2);
            assert_eq!(
                reconstruct(&source, file, limits())
                    .unwrap()
                    .engine()
                    .stable_hash()
                    .unwrap(),
                hash
            );
        }
    }
}
