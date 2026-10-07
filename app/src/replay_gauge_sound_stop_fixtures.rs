//! Deferred real replay/queue/Mixer fixtures; typed preparation does not admit mine files.
use crate::{
    bgm::{BgmConfig, BgmFeeder},
    gauge::GaugeFailure,
    mine_audio_consumers_fixtures::{data, recorded, replay_limits, Action},
    replay_audio::{completed_render_cursor, plan_audio, plan_section_audio, ReplayAudioError},
    section_start::prepare_at,
    step_gameplay::{StepGameplay, StepGameplayConfig},
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleId, VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::HazardOutcome,
    replay::codec::decode_replay,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::BmsInputMode;

const SECOND: i64 = 1_000_000_000;
const HOST: i64 = 10 * SECOND;
const OUTPUT: i64 = SECOND;
const FATAL: &str = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 note\n#WAV02 press\n#00011:00010101\n#00031:02\n#00032:02\n#000D1:00ZZ0000\n#000D2:00010000\n#00001:00020000";
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn play(sample: u64, voice: u64, ns: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(ns),
        gain,
    }
}
fn stop(voice: u64, ns: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(ns),
    }
}
fn fatal_actions() -> [Action; 7] {
    [
        Action::Press(0, 92),
        Action::Bgm(0),
        Action::Press(SECOND, 91),
        Action::Release(SECOND, 91),
        Action::Press(2 * SECOND, 91),
        Action::Release(2 * SECOND, 91),
        Action::Press(3 * SECOND, 91),
    ]
}

#[test]
fn legacy_full_log_keeps_hash_and_results_but_audio_stops_after_the_actual_fatal_operation() {
    for mode in [BmsInputMode::ButtonOnly, BmsInputMode::ButtonOrContact] {
        for (text, bank_zero, audible) in [
            (FATAL.to_owned(), true, true),
            (FATAL.replace("#WAV00 blast\n", ""), false, false),
            (FATAL.replace("#000D2:00010000\n", ""), false, false),
        ] {
            let prepared = data(&text, bank_zero);
            // This actual Runtime/capture intentionally records a legacy unfenced continuation.
            let legacy = recorded(&prepared, &fatal_actions(), mode, 0, None, 0, 0);
            assert_eq!(legacy.reports[1].hazard_events[0].value, 1295);
            assert_eq!(
                legacy.reports[1].hazard_events[0].outcome,
                HazardOutcome::Triggered
            );
            assert_eq!(
                legacy.reports[1].hazard_events[0].input,
                Some(*legacy.reports[1].input.as_ref().unwrap().meta())
            );
            assert!(legacy.commands.contains(&play(1, 12, 3 * SECOND, 0.5)));
            assert!(legacy.commands.contains(&play(1, 13, 4 * SECOND, 0.5)));
            let expected_events = legacy
                .reports
                .iter()
                .flat_map(|report| report.judge_events.clone())
                .collect::<Vec<_>>();
            let plan = plan_section_audio(
                &prepared,
                legacy.file.clone(),
                replay_limits(),
                point(2, OUTPUT),
                Duration::ZERO,
            )
            .unwrap();
            let mut expected = vec![
                play(2, 92, OUTPUT, 0.5),
                play(2, 90, 2 * SECOND, 0.5),
                play(1, 11, 2 * SECOND, 0.5),
            ];
            if audible {
                expected.push(play(0, 93, 2 * SECOND, 0.5));
            }
            expected.extend([
                stop(11, 2 * SECOND),
                stop(12, 2 * SECOND),
                stop(13, 2 * SECOND),
                stop(91, 2 * SECOND),
                stop(92, 2 * SECOND),
            ]);
            if audible {
                expected.push(stop(93, 2 * SECOND));
            }
            assert_eq!(plan.commands, expected);
            assert_eq!(plan.judge_events, expected_events);
            assert_eq!(plan.final_judge_hash, legacy.hash);
            assert_eq!(plan.recorded_until, Some(ts(3 * SECOND)));
            assert_eq!(
                plan.judge_events.len(),
                3,
                "later legacy hits are still reconstructed"
            );
            assert!(!plan.commands.iter().any(|command| matches!(
                command,
                AudioCommand::Stop {
                    voice: VoiceId(90),
                    ..
                }
            )));
            if audible {
                let missing = plan_section_audio(
                    &data(&text, false),
                    legacy.file,
                    replay_limits(),
                    point(2, OUTPUT),
                    Duration::ZERO,
                )
                .unwrap_err();
                assert!(matches!(
                    missing.downcast_ref::<ReplayAudioError>(),
                    Some(ReplayAudioError::MissingSample(SampleId(0)))
                ));
            }
        }
    }

    let silent = data("#BPM 60\n#WAV00 unused\n#000D1:00ZZ0000", false);
    let legacy = recorded(
        &silent,
        &[Action::Press(0, 91), Action::Advance(SECOND)],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let plan = plan_audio(
        &silent,
        legacy.file,
        replay_limits(),
        point(2, i64::MAX),
        Duration::ZERO,
    )
    .unwrap();
    assert!(
        plan.commands.is_empty(),
        "no voices means no invented overflowing output mapping"
    );
    assert_eq!(plan.final_judge_hash, legacy.hash);

    let recoverable = data(&FATAL.replace("00ZZ0000", "001E0000"), true);
    let actual = recorded(
        &recoverable,
        &fatal_actions(),
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let plan = plan_audio(
        &recoverable,
        actual.file,
        replay_limits(),
        point(2, OUTPUT),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(
        plan.commands, actual.commands,
        "recoverable zero is not a terminal gauge failure"
    );
    assert!(
        !plan
            .commands
            .iter()
            .any(|command| matches!(command, AudioCommand::Stop { .. }))
    );
    assert_eq!(plan.final_judge_hash, actual.hash);

    // Without mines, the old aggregate normal-before-fallback tie order is unchanged.
    let ordinary = data(
        "#BPM 60\n#WAV01 note\n#WAV02 press\n#00011:00010000\n#00032:02",
        false,
    );
    let actual = recorded(
        &ordinary,
        &[Action::Press(SECOND, 92), Action::Press(SECOND, 91)],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    assert_eq!(
        actual.commands,
        [play(2, 12, 2 * SECOND, 1.0), play(1, 11, 2 * SECOND, 1.0)]
    );
    let plan = plan_audio(
        &ordinary,
        actual.file,
        replay_limits(),
        point(2, OUTPUT),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(
        plan.commands,
        [play(1, 11, 2 * SECOND, 1.0), play(2, 12, 2 * SECOND, 1.0)]
    );
    assert_eq!(plan.final_judge_hash, actual.hash);
}

#[test]
fn section_offset_and_preroll_map_once_and_exclusive_rounded_end_filters_failure_stops() {
    let text = "#BPM 60\n#WAV00 blast\n#WAV02 press\n#00031:02\n#000D1:00ZZ0000\n#000D2:00010000";
    let limits = PcmLimits::new(256, 2048, 8).unwrap();
    for (offset, boundary, mapped) in [
        (100_000_000, 900_000_000, 1_500_000_000),
        (-100_000_000, 1_100_000_000, 1_700_000_000),
    ] {
        let original = data(text, true);
        let zero = original.bank.get(SampleId(0)).unwrap().samples().as_ptr();
        let (selected, _) = prepare_at(original, ts(500_000_000), limits).unwrap();
        assert_eq!(
            selected.bank.get(SampleId(0)).unwrap().samples().as_ptr(),
            zero
        );
        assert_eq!(selected.source.compile_mines().unwrap()[0].at, ts(SECOND));
        let actual = recorded(
            &selected,
            &[
                Action::Press(500_000_000, 91),
                Action::Press(500_000_000, 92),
                Action::Advance(boundary),
                Action::Advance(boundary),
            ],
            BmsInputMode::ButtonOnly,
            500_000_000,
            None,
            offset,
            100_000_000,
        );
        assert_eq!(actual.reports[2].hazard_events.len(), 2);
        assert!(
            actual.reports[2]
                .hazard_events
                .iter()
                .all(|event| event.input.is_none())
        );
        assert!(actual.reports[3].hazard_events.is_empty());
        let plan = plan_section_audio(
            &selected,
            actual.file,
            replay_limits(),
            point(2, OUTPUT),
            Duration::from_nanos(100_000_000),
        )
        .unwrap();
        assert_eq!(
            plan.commands,
            [
                play(2, 1, 1_100_000_000, 1.0),
                play(0, 2, mapped, 1.0),
                stop(1, mapped),
                stop(2, mapped)
            ]
        );
        assert_eq!(plan.final_judge_hash, actual.hash);
    }
    // End .91, start .5 and preroll .1 round to output frame 6 at 10 Hz.
    // Operation .900 executes in frame 5; .905 rounds to frame 6 and is excluded.
    for (boundary, included) in [(900_000_000, true), (905_000_000, false)] {
        let (selected, _) = prepare_at(data(text, true), ts(500_000_000), limits).unwrap();
        let actual = recorded(
            &selected,
            &[
                Action::Press(500_000_000, 91),
                Action::Press(500_000_000, 92),
                Action::Advance(boundary),
                Action::Advance(910_000_000),
            ],
            BmsInputMode::ButtonOnly,
            500_000_000,
            Some(910_000_000),
            100_000_000,
            100_000_000,
        );
        let plan = plan_section_audio(
            &selected,
            actual.file,
            replay_limits(),
            point(2, OUTPUT),
            Duration::from_nanos(100_000_000),
        )
        .unwrap();
        let expected = if included {
            vec![
                play(2, 1, 1_100_000_000, 1.0),
                play(0, 2, 1_500_000_000, 1.0),
                stop(1, 1_500_000_000),
                stop(2, 1_500_000_000),
            ]
        } else {
            vec![play(2, 1, 1_100_000_000, 1.0)]
        };
        assert_eq!(plan.commands, expected);
        assert_eq!(
            plan.final_judge_hash, actual.hash,
            "audio endpoint does not rewrite recorded judging"
        );
    }
    let collision_text =
        "#BPM 60\n#WAV01 note\n#WAV02 music\n#00011:01\n#00001:02\n#000D1:00ZZ0000";
    let mut collision = data(collision_text, false);
    let actual = recorded(
        &collision,
        &[Action::Press(0, 91), Action::Advance(SECOND)],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    collision.sounds[0].voice = VoiceId(90);
    let error = plan_audio(
        &collision,
        actual.file,
        replay_limits(),
        point(2, OUTPUT),
        Duration::ZERO,
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<ReplayAudioError>(),
        Some(ReplayAudioError::InvalidConfiguration(
            "gameplay failure Stop voice collides with BGM"
        ))
    ));
}

struct SameDomains;
impl ClockMapper for SameDomains {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn input(song: i64, sequence: u64, key: u16, down: bool) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), point(1, HOST + song), sequence),
        control: PhysicalControlId::keyboard(key),
        state: if down {
            ButtonState::Down
        } else {
            ButtonState::Up
        },
    })
}

#[test]
fn actual_stepped_failure_prefix_matches_replay_and_mapped_feeder_keeps_real_stop_diagnostics() {
    let text = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 note\n#WAV02 press\n#00011:01000100\n#00031:02\n#00032:02\n#000D1:01ZZ0000";
    let bindings = BindingMap::from_bindings([(91u16, 0x11), (92u16, 0x12)].map(
        |(key, control)| Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(key),
            game_control: GameControlId(control),
        },
    ))
    .unwrap();
    let (mut game, bank) = StepGameplay::new(
        data(text, true),
        StepGameplayConfig {
            host_origin: point(1, HOST),
            output_origin: point(2, OUTPUT),
            preroll: Duration::ZERO,
            early_ns: 0,
            late_ns: 0,
            offset_ns: 0,
            command_capacity: 32,
            bgm_pending: 1,
            bgm_lookahead: Duration::from_nanos(SECOND),
            telemetry_capacity: 0,
        },
        bindings,
    )
    .unwrap();
    game.configure_capture(replay_limits(), 0).unwrap();
    game.activate(point(1, HOST)).unwrap();
    let first = game
        .process_input(input(0, 1, 91, true), &SameDomains, point(2, OUTPUT))
        .unwrap();
    assert_eq!(
        first.audio_commands,
        [play(1, 11, OUTPUT, 0.5), play(0, 15, OUTPUT, 0.5)]
    );
    let second = game
        .process_input(input(0, 2, 92, true), &SameDomains, point(2, OUTPUT))
        .unwrap();
    assert_eq!(second.audio_commands, [play(2, 14, OUTPUT, 0.5)]);
    let fatal = game
        .advance_to(point(1, HOST + SECOND), &SameDomains, point(2, 2 * SECOND))
        .unwrap();
    assert_eq!(
        fatal.audio_commands,
        [
            stop(11, 2 * SECOND),
            stop(12, 2 * SECOND),
            stop(13, 2 * SECOND),
            stop(14, 2 * SECOND),
            stop(15, 2 * SECOND)
        ]
    );
    assert_eq!(
        game.gauge().snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    assert_eq!(game.gameplay_fence(), Some(ts(SECOND)));
    let retained_hash = game.judge().stable_hash().unwrap();
    let later = game
        .process_input(
            input(2 * SECOND, 3, 91, false),
            &SameDomains,
            point(2, 3 * SECOND),
        )
        .unwrap();
    assert!(later.judge_events.is_empty() && later.audio_commands.is_empty());
    assert_eq!(game.judge().stable_hash().unwrap(), retained_hash);
    let batch = game.take_commands(32).unwrap().unwrap();
    let mut expected = first.audio_commands;
    expected.extend_from_slice(&second.audio_commands);
    expected.extend_from_slice(&fatal.audio_commands);
    assert_eq!(batch.commands, expected);
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), replay_limits()).unwrap();
    assert_eq!(
        file.records.len(),
        3,
        "two bound inputs and the failure advance only"
    );
    let plan = plan_audio(
        &data(text, true),
        file,
        replay_limits(),
        point(2, OUTPUT),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(plan.commands, expected);
    assert_eq!(plan.final_judge_hash, retained_hash);

    let mut feeder = BgmFeeder::from_output_commands(
        plan.commands,
        BgmConfig {
            output_origin: point(2, OUTPUT),
            sample_rate: 10,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(2 * SECOND),
            max_pending: 16,
        },
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(32).unwrap();
    let admitted = feeder
        .feed(0, 32, |command| producer.try_push(command))
        .unwrap();
    assert_eq!(admitted.admitted, 8);
    assert_eq!(feeder.admitted_stops(), 5);
    let mut mixer = Mixer::new(
        MixerConfig::new(
            bank.format(),
            ClockDomainId(2),
            ts(OUTPUT),
            AudioLimits::new(32, 16, 32, 32, 32).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut output = [0.0; 20];
    let rendered = mixer.render(&mut output).unwrap();
    let mut expected_pcm = [0.0; 20];
    expected_pcm[0] = 0.875;
    expected_pcm[1] = -0.375;
    assert_eq!(output, expected_pcm);
    assert_eq!(rendered.counters.commands_applied, 8);
    assert_eq!(
        rendered.counters.unknown_stops, 5,
        "all three short sounds already ended, and voices 12/13 never played"
    );
    assert!(matches!(
        completed_render_cursor(&rendered),
        Err(ReplayAudioError::RejectedRender(_))
    ));
    assert_eq!(
        feeder.admitted_stops(),
        5,
        "admission cannot erase actual execution diagnostics"
    );
    assert!(game.gameplay_fence().is_some());
}
