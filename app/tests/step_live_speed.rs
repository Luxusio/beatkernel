//! Public live owners, real Mixer/queue credits, and controlled audio-port observations.
//! No host-clock observation is used as song progress or physical-device evidence.
#[path = "support/replay_speed.rs"]
mod fixture;

use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioLimits, CommandProducer, Mixer, MixerConfig, SampleBank,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    replay::{
        codec::{decode_replay, encode_replay},
        ReplayOperation,
    },
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp},
    transport::Rate,
};
use beatkernel_bms::BmsInputMode;
use beatkernel_bms_runtime::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    local_input::InputMerger,
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    replay_playback::{decode_section_setup, reconstruct_section},
    replay_speed::ReplaySpeed,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepLocalGameplay},
    PreparedBms,
};

const NOTES: &str = "#BPM 60000\n#WAV01 key.wav\n#00011:00010000\n#00012:00000001\n";
const MUSIC: &str = "#BPM 60\n#WAV01 music.wav\n#00001:01\n";
fn host(ns: i64) -> ClockPoint {
    point(11, 10_000_000_000 + ns)
}
fn raw(ns: i64) -> ClockPoint {
    point(22, 1_000_000_000 + ns)
}
fn logical(ns: i64) -> ClockPoint {
    point(33, 5_000_000_000 + ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn speed(n: i64, d: u64) -> ReplaySpeed {
    ReplaySpeed::new(Rate::new(n, d).unwrap()).unwrap()
}
fn config(preroll: i64) -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: host(0),
        output_origin: raw(0),
        preroll: Duration::from_nanos(preroll),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 64,
        bgm_pending: 8,
        bgm_lookahead: Duration::from_nanos(20_000_000_000),
        telemetry_capacity: 16,
    }
}
fn authority() -> AudioAuthority {
    AudioAuthority::new(
        AudioAuthorityConfig {
            history_capacity: 16,
            max_observation_age: Duration::from_nanos(30_000_000_000),
            input_extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: 1,
            stream_origin: raw(0),
            logical_origin: logical(0),
            host_domain: host(0).domain,
        },
    )
    .unwrap()
}
fn bindings(device: Option<DeviceId>) -> BindingMap {
    BindingMap::from_bindings(
        [0x11u32, 0x12]
            .into_iter()
            .enumerate()
            .map(|(i, lane)| Binding {
                device: device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
                physical: PhysicalControlId::keyboard(7 + i as u16),
                game_control: GameControlId(lane),
            }),
    )
    .unwrap()
}
fn empty() -> PreparedBms {
    fixture::prepared("#BPM 60\n", 1000, &[])
}
fn notes() -> PreparedBms {
    fixture::prepared(NOTES, 1000, &[(1, &[0.25, 0.5, -0.25])])
}
fn solo(
    p: PreparedBms,
    c: StepGameplayConfig,
    end: Option<i64>,
    s: ReplaySpeed,
) -> (StepGameplay, SampleBank) {
    StepGameplay::new_audio_section_with_speed(
        p,
        c,
        bindings(None),
        Timestamp::ZERO,
        end.map(Timestamp::from_nanos),
        BmsInputMode::ButtonOnly,
        authority(),
        s,
    )
    .unwrap()
}
fn local(
    count: usize,
    c: StepGameplayConfig,
    end: Option<i64>,
    s: ReplaySpeed,
) -> (StepLocalGameplay, SampleBank) {
    let members: Vec<_> = (1..=count)
        .map(|id| (PlayerId(id as u32), Some(DeviceId(id as u64))))
        .collect();
    let maps = members
        .iter()
        .map(|(_, device)| bindings(*device))
        .collect();
    StepLocalGameplay::new_audio_section_with_speed(
        notes(),
        c,
        ResolvedInputPlan::new(members).unwrap(),
        maps,
        Timestamp::ZERO,
        end.map(Timestamp::from_nanos),
        BmsInputMode::ButtonOnly,
        authority(),
        s,
    )
    .unwrap()
}
fn output(bank: SampleBank, c: StepGameplayConfig, end: Option<u64>) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(64).unwrap();
    let mut mc = MixerConfig::new(
        bank.format(),
        c.output_origin.domain,
        c.output_origin.timestamp,
        AudioLimits::new(64, 128, 64, 64, 64).unwrap(),
    );
    if let Some(end) = end {
        mc = mc.with_playback_end_frame(end);
    }
    (producer, Mixer::new(mc, bank, consumer).unwrap())
}
fn deliver(game: &mut StepGameplay, producer: &mut CommandProducer) -> Vec<AudioCommand> {
    let mut commands = Vec::new();
    while let Some(batch) = game.take_commands(64).unwrap() {
        for &command in &batch.commands {
            producer.try_push(command).unwrap();
        }
        game.acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        commands.extend(batch.commands);
    }
    commands
}
fn deliver_local(
    game: &mut StepLocalGameplay,
    producer: &mut CommandProducer,
) -> Vec<AudioCommand> {
    let mut commands = Vec::new();
    while let Some(batch) = game.take_commands(64).unwrap() {
        for &command in &batch.commands {
            producer.try_push(command).unwrap();
        }
        game.acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        commands.extend(batch.commands);
    }
    commands
}
fn input(
    delta: i64,
    device: u64,
    key: u16,
    sequence: u64,
    state: ButtonState,
) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), host(delta), sequence),
        control: PhysicalControlId::keyboard(key),
        state,
    })
}
fn observe_pair(game: &mut StepGameplay, output_ns: i64) {
    game.observe_audio_output(
        1,
        ClockPair {
            source: raw(0),
            target: host(0),
        },
    )
    .unwrap();
    game.observe_audio_output(
        1,
        ClockPair {
            source: raw(output_ns),
            target: host(output_ns),
        },
    )
    .unwrap();
}

#[test]
fn four_rates_retain_cold_identity_and_literal_original_phase_after_activation() {
    for (n, d, before, after) in [
        (1, 2, -1_500_000, 2_000_000),
        (1, 1, -3_000_000, 4_000_000),
        (2, 1, -6_000_000, 8_000_000),
        (3, 2, -4_500_000, 6_000_000),
    ] {
        let s = speed(n, d);
        let (mut game, _) = solo(empty(), config(3_000_000), None, s);
        assert_eq!(game.playback_speed(), s);
        assert_eq!(game.song_time().as_nanos(), before);
        game.activate_audio().unwrap();
        assert_eq!(game.playback_speed(), s);
        observe_pair(&mut game, 7_000_000);
        let mut merger = InputMerger::new(host(0).domain, host(0), vec![DeviceId(1)], 16).unwrap();
        game.record_audio_prefix(host(7_000_000)).unwrap();
        let report = game
            .advance_audio_frontier(&mut merger, host(7_000_000), raw(7_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(report.song_time.as_nanos(), after, "{n}/{d}");
        assert_eq!(report.audio_at, raw(7_000_000));
    }
}

#[test]
fn odd_nanosecond_preroll_uses_one_signed_division_in_the_real_runtime() {
    let (mut game, _) = solo(empty(), config(1), None, speed(1, 2));
    game.activate_audio().unwrap();
    observe_pair(&mut game, 3);
    let mut merger = InputMerger::new(host(0).domain, host(0), vec![DeviceId(1)], 16).unwrap();
    game.record_audio_prefix(host(3)).unwrap();
    for (sequence, elapsed, expected) in [(0, 2, 0), (1, 3, 1)] {
        merger
            .admit(
                input(
                    elapsed,
                    1,
                    7,
                    sequence,
                    if sequence == 0 {
                        ButtonState::Down
                    } else {
                        ButtonState::Up
                    },
                ),
                host(3),
            )
            .unwrap();
        let report = game
            .process_next_audio_input(&mut merger, host(3), raw(3), None)
            .unwrap()
            .unwrap();
        assert_eq!(report.song_time.as_nanos(), expected);
    }
}

#[test]
fn actual_mixer_readhead_interpolation_and_rate_before_bgm_are_literal() {
    let cases = [
        (
            1,
            2,
            vec![
                0.0625, 0.09375, 0.125, 0.15625, 0.1875, 0.21875, 0.25, 0.28125, 0.3125, 0.34375,
                0.375, 0.40625, 0.4375, 0.46875, 0.5, 0.5,
            ],
        ),
        (
            1,
            1,
            vec![0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5],
        ),
        (2, 1, vec![0.0625, 0.1875, 0.3125, 0.4375]),
        (3, 2, vec![0.0625, 0.15625, 0.25, 0.34375, 0.4375, 0.5]),
    ];
    for (n, d, mut expected) in cases {
        expected.resize(24, 0.0);
        for block in [1, 3, 7, 24] {
            let s = speed(n, d);
            let c = config(0);
            let p = fixture::prepared(
                MUSIC,
                4,
                &[(
                    1,
                    &[0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5],
                )],
            );
            let (mut game, bank) = solo(p, c, None, s);
            game.activate_audio().unwrap();
            let (mut producer, mut mixer) = output(bank, c, None);
            let commands = deliver(&mut game, &mut producer);
            if s.rate() != Rate::NORMAL {
                assert!(
                    matches!(commands[0], AudioCommand::SetRate { rate, at } if rate == s.rate() && at == raw(0).timestamp)
                );
                assert_eq!(commands.len(), 2);
            } else {
                assert_eq!(commands.len(), 1);
            }
            let mut pcm = Vec::new();
            while pcm.len() < 24 {
                let mut buffer = vec![0.0; block.min(24 - pcm.len())];
                mixer.render(&mut buffer).unwrap();
                pcm.extend(buffer);
            }
            assert_eq!(pcm, expected, "{n}/{d}, block {block}");
        }
    }
}

#[test]
fn input_provenance_raw_keysound_and_original_capture_roundtrip_across_four_rates() {
    // Both chart notes are at 1 ms and 3 ms. P=3 ms is never multiplied in HOST acquisition.
    for (n, d, first, second) in [
        (1, 2, 5_000_000, 9_000_000),
        (1, 1, 4_000_000, 6_000_000),
        (2, 1, 3_500_000, 4_500_000),
        (3, 2, 3_666_667, 5_000_000),
    ] {
        let source = notes().source;
        let (mut game, bank) = solo(notes(), config(3_000_000), None, speed(n, d));
        game.configure_capture(fixture::limits(), 0).unwrap();
        game.activate_audio().unwrap();
        observe_pair(&mut game, 12_000_000);
        let mut merger = InputMerger::new(host(0).domain, host(0), vec![DeviceId(9)], 16).unwrap();
        game.record_audio_prefix(host(12_000_000)).unwrap();
        let (mut producer, mut mixer) = output(bank, config(3_000_000), None);
        let prefix = deliver(&mut game, &mut producer);
        assert_eq!(prefix.len(), usize::from(i128::from(n) != i128::from(d)));
        for (seq, elapsed, key, state, song) in [
            (0, first, 7, ButtonState::Down, 1_000_000),
            (
                1,
                first + 1,
                7,
                ButtonState::Up,
                if n == 3 {
                    1_000_002
                } else if n == 2 {
                    1_000_002
                } else if d == 2 {
                    1_000_000
                } else {
                    1_000_001
                },
            ),
            (2, second, 8, ButtonState::Down, 3_000_000),
            (
                3,
                second + 1,
                8,
                ButtonState::Up,
                if n == 3 {
                    3_000_001
                } else if n == 2 {
                    3_000_002
                } else if d == 2 {
                    3_000_000
                } else {
                    3_000_001
                },
            ),
        ] {
            let original = input(elapsed, 9, key, seq, state);
            merger.admit(original.clone(), host(12_000_000)).unwrap();
            let report = game
                .process_next_audio_input(&mut merger, host(12_000_000), raw(13_000_000), None)
                .unwrap()
                .unwrap();
            assert_eq!(report.song_time.as_nanos(), song);
            assert_eq!(report.audio_at, raw(13_000_000));
            let meta = report.input.as_ref().unwrap().meta();
            assert_eq!(meta.timestamp, logical(elapsed).timestamp);
            assert_eq!(meta.original_clock_point, Some(host(elapsed)));
            assert_eq!(meta.source, DeviceId(9));
            assert_eq!(meta.sequence, seq);
            if state == ButtonState::Down {
                assert_eq!(report.judge_events.len(), 1);
                assert!(report.audio_commands.iter().any(|c| matches!(c, AudioCommand::Play { at, .. } if *at == raw(13_000_000).timestamp)));
            }
        }
        assert_eq!(game.score().hits, 2);
        assert_eq!(game.score().misses, 0);
        deliver(&mut game, &mut producer);
        mixer.render(&mut [0.0; 32]).unwrap();
        let snapshot_hash = game.judge().stable_hash().unwrap();
        game.fail();
        let bytes = game.take_replay().unwrap().unwrap();
        let file = decode_replay(&bytes, fixture::limits()).unwrap();
        assert_eq!(
            decode_section_setup(&file.header.options)
                .unwrap()
                .source_speed,
            speed(n, d)
        );
        assert_eq!(file.records.len(), 4);
        assert_eq!(file.records[0].song_time.as_nanos(), 1_000_000);
        assert_eq!(file.records[2].song_time.as_nanos(), 3_000_000);
        let ReplayOperation::Input(saved) = &file.records[0].operation else {
            panic!("physical input missing");
        };
        assert_eq!(
            saved.physical.meta().original_clock_point,
            Some(host(first))
        );
        assert_eq!(encode_replay(&file, fixture::limits()).unwrap(), bytes);
        let mut restored = reconstruct_section(&source, file, fixture::limits()).unwrap();
        restored.seek_cursor(4).unwrap();
        let hash = restored.stable_hash().unwrap();
        restored.seek_cursor(0).unwrap();
        restored.seek_cursor(4).unwrap();
        assert_eq!(restored.stable_hash().unwrap(), hash);
        // The source snapshot must reconstruct using original song records, not source-speed multiplication.
        assert_eq!(restored.engine().stable_hash().unwrap(), snapshot_hash);
    }
}

#[test]
fn finite_literal_grid_requires_real_render_and_original_source_presentation() {
    for (n, d, end_frame) in [(1, 2, 11), (1, 1, 7), (2, 1, 5), (3, 2, 6)] {
        let c = config(3_000_000);
        let (mut game, bank) = solo(notes(), c, Some(4_000_000), speed(n, d));
        game.configure_capture(fixture::limits(), 0).unwrap();
        game.activate_audio().unwrap();
        assert_eq!(game.playback_end_frame(), Some(end_frame));
        assert!(!game
            .observe_completion(None, Some(raw(end_frame as i64 * 1_000_000)))
            .unwrap());
        assert_ne!(game.song_time().as_nanos(), 4_000_000);
        let (mut producer, mut mixer) = output(bank, c, Some(end_frame));
        let commands = deliver(&mut game, &mut producer);
        let report = mixer.render(&mut [0.0; 32]).unwrap();
        assert_eq!(report.playback_end_physical_frame, Some(end_frame));
        assert_eq!(report.counters.commands_consumed as usize, commands.len());
        assert_eq!(report.counters.commands_applied as usize, commands.len());
        assert!(game
            .observe_completion(Some(report), Some(raw(end_frame as i64 * 1_000_000)))
            .unwrap());
        assert_eq!(game.song_time().as_nanos(), 4_000_000);
        assert_eq!(game.score().misses, 2);
        game.fail();
        let bytes = game.take_replay().unwrap().unwrap();
        let file = decode_replay(&bytes, fixture::limits()).unwrap();
        let last = file.records.last().unwrap();
        assert_eq!(last.song_time.as_nanos(), 4_000_000);
        assert!(matches!(last.operation, ReplayOperation::Advance));
    }
}

#[test]
fn render_ahead_without_presentation_cannot_complete_or_synthesize_final_advance() {
    let c = config(3_000_000);
    let (mut game, bank) = solo(notes(), c, Some(4_000_000), speed(3, 2));
    game.activate_audio().unwrap();
    let (mut producer, mut mixer) = output(bank, c, Some(6));
    deliver(&mut game, &mut producer);
    let report = mixer.render(&mut [0.0; 32]).unwrap();
    assert!(!game.observe_completion(Some(report), None).unwrap());
    assert_ne!(game.song_time().as_nanos(), 4_000_000);
    assert!(!game.observe_completion(None, Some(raw(5_999_999))).unwrap());
    assert!(game.observe_completion(None, Some(raw(6_000_000))).unwrap());
    assert_eq!(game.song_time().as_nanos(), 4_000_000);
}

#[test]
fn quantized_zero_frame_fence_requires_actual_render_then_closes_original_one_nanosecond() {
    let c = config(0);
    let (mut game, bank) = solo(empty(), c, Some(1), speed(2, 1));
    game.configure_capture(fixture::limits(), 0).unwrap();
    game.activate_audio().unwrap();
    assert_eq!(game.playback_end_frame(), Some(0));
    assert!(!game.observe_completion(None, Some(raw(0))).unwrap());
    let (mut producer, mut mixer) = output(bank, c, Some(0));
    assert!(deliver(&mut game, &mut producer).is_empty());
    let report = mixer.render(&mut [0.0; 1]).unwrap();
    assert_eq!(report.playback_end_physical_frame, Some(0));
    assert!(game.observe_completion(Some(report), Some(raw(0))).unwrap());
    assert_eq!(game.song_time().as_nanos(), 1);
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), fixture::limits()).unwrap();
    assert_eq!(file.records.last().unwrap().song_time.as_nanos(), 1);
}

#[test]
fn unacknowledged_commands_never_gain_completion_authority() {
    let c = config(0);
    let (mut game, bank) = solo(empty(), c, Some(1_000_000), speed(2, 1));
    game.activate_audio().unwrap();
    let (mut producer, mut mixer) = output(bank, c, game.playback_end_frame());
    let batch = game.take_commands(64).unwrap().unwrap();
    for command in batch.commands {
        producer.try_push(command).unwrap();
    }
    let report = mixer.render(&mut [0.0; 8]).unwrap();
    assert!(!game
        .observe_completion(Some(report), Some(raw(1_000_000)))
        .unwrap());
    assert!(game.completed_result().is_none());
}

#[test]
fn local_two_four_and_sixty_four_members_close_original_end_on_one_actual_output_fence() {
    for count in [2, 4, 64] {
        for (n, d, frame) in [(1, 2, 11), (1, 1, 7), (2, 1, 5), (3, 2, 6)] {
            let c = config(3_000_000);
            let s = speed(n, d);
            let (mut game, bank) = local(count, c, Some(4_000_000), s);
            assert_eq!(game.playback_speed(), s);
            game.activate_audio().unwrap();
            assert_eq!(game.playback_end_frame(), Some(frame));
            let (mut producer, mut mixer) = output(bank, c, Some(frame));
            deliver_local(&mut game, &mut producer);
            assert!(!game
                .observe_completion(None, Some(raw(frame as i64 * 1_000_000)))
                .unwrap());
            let report = mixer.render(&mut [0.0; 32]).unwrap();
            assert!(game
                .observe_completion(Some(report), Some(raw(frame as i64 * 1_000_000)))
                .unwrap());
            for id in 1..=count {
                let player = PlayerId(id as u32);
                assert_eq!(game.member_song_time(player).unwrap().as_nanos(), 4_000_000);
                assert_eq!(game.score(player).unwrap().misses, 2);
                assert!(game.completed_result(player).unwrap().is_some());
            }
        }
    }
}

#[test]
fn local_routes_inputs_without_scaling_acquisition_or_other_members() {
    let (mut game, _) = local(4, config(3_000_000), None, speed(2, 1));
    game.activate_audio().unwrap();
    for ns in [0, 8_000_000] {
        game.observe_audio_output(
            1,
            ClockPair {
                source: raw(ns),
                target: host(ns),
            },
        )
        .unwrap();
    }
    let mut merger =
        InputMerger::new(host(0).domain, host(0), (1..=4).map(DeviceId).collect(), 16).unwrap();
    game.record_audio_prefix(host(8_000_000)).unwrap();
    merger
        .admit(
            input(3_500_000, 3, 7, 0, ButtonState::Down),
            host(8_000_000),
        )
        .unwrap();
    let Some(InputResult::Processed(reports)) = game
        .process_next_audio_input(&mut merger, host(8_000_000), raw(9_000_000), None)
        .unwrap()
    else {
        panic!("routed input missing");
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].player, PlayerId(3));
    let report = &reports[0].report;
    assert_eq!(report.song_time.as_nanos(), 1_000_000);
    assert_eq!(report.audio_at, raw(9_000_000));
    assert_eq!(
        report.input.as_ref().unwrap().meta().original_clock_point,
        Some(host(3_500_000))
    );
    assert_eq!(game.score(PlayerId(3)).unwrap().hits, 1);
    for id in [1, 2, 4] {
        assert_eq!(game.score(PlayerId(id)).unwrap().hits, 0);
    }
}

#[test]
fn held_mine_keeps_original_marker_damage_gauge_and_raw_explosion_time_at_four_rates() {
    // One hit at original zero, then a damage-01 mine at original 1 ms.
    // The physical button remains held through that marker and is genuinely released afterwards.
    let chart = "#BPM 60000\n#WAV00 blast.wav\n#WAV01 key.wav\n#00011:01\n#000D1:00010000\n";
    for (n, d, mine_output) in [
        (1, 2, 5_000_000),
        (1, 1, 4_000_000),
        (2, 1, 3_500_000),
        (3, 2, 3_666_667),
    ] {
        let p = fixture::prepared(chart, 1000, &[(0, &[0.5, -0.5]), (1, &[0.25])]);
        let (mut game, bank) = solo(p, config(3_000_000), None, speed(n, d));
        game.activate_audio().unwrap();
        observe_pair(&mut game, mine_output);
        let mut merger = InputMerger::new(host(0).domain, host(0), vec![DeviceId(9)], 16).unwrap();
        game.record_audio_prefix(host(mine_output)).unwrap();
        merger
            .admit(
                input(3_000_000, 9, 7, 0, ButtonState::Down),
                host(mine_output),
            )
            .unwrap();
        let press = game
            .process_next_audio_input(&mut merger, host(mine_output), raw(8_000_000), None)
            .unwrap()
            .unwrap();
        assert_eq!(press.song_time.as_nanos(), 0);
        assert_eq!(press.judge_events.len(), 1);
        assert!(press.hazard_events.is_empty());
        assert_eq!(game.gauge().snapshot().level_units, 21_000_000);
        let mine = game
            .advance_audio_frontier(&mut merger, host(mine_output), raw(9_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(mine.song_time.as_nanos(), 1_000_000);
        assert_eq!(mine.audio_at, raw(9_000_000));
        assert_eq!(mine.hazard_events.len(), 1);
        assert_eq!(mine.hazard_events[0].at.as_nanos(), 1_000_000);
        assert_eq!(mine.hazard_events[0].value, 1);
        assert_eq!(
            mine.hazard_events[0].outcome,
            beatkernel::judge::HazardOutcome::Triggered
        );
        assert!(mine.audio_commands.iter().any(|c| matches!(c,
            AudioCommand::Play { sample, at, .. } if *sample == beatkernel::audio::SampleId(0) && *at == raw(9_000_000).timestamp)));
        assert_eq!(game.mine_damage().triggered, 1);
        assert_eq!(game.mine_damage().half_percent_damage, 1);
        assert_eq!(game.gauge().snapshot().level_units, 20_500_000);
        assert_eq!(game.gauge().snapshot().failure, None);
        game.observe_audio_output(
            1,
            ClockPair {
                source: raw(mine_output + 1),
                target: host(mine_output + 1),
            },
        )
        .unwrap();
        game.record_audio_prefix(host(mine_output + 1)).unwrap();
        merger
            .admit(
                input(mine_output + 1, 9, 7, 1, ButtonState::Up),
                host(mine_output + 1),
            )
            .unwrap();
        let release = game
            .process_next_audio_input(&mut merger, host(mine_output + 1), raw(10_000_000), None)
            .unwrap()
            .unwrap();
        assert!(release.hazard_events.is_empty());
        assert_eq!(game.gauge().snapshot().level_units, 20_500_000);
        let (mut producer, mut mixer) = output(bank, config(3_000_000), None);
        let commands = deliver(&mut game, &mut producer);
        assert_eq!(
            commands.len(),
            if i128::from(n) == i128::from(d) { 2 } else { 3 }
        );
        let report = mixer.render(&mut [0.0; 16]).unwrap();
        assert_eq!(report.counters.commands_applied as usize, commands.len());
        assert_eq!(game.score().hits, 1);
    }
}

#[test]
fn old_normal_constructor_and_normal_optin_preserve_header_commands_and_pcm() {
    let c = config(0);
    let mut outputs = Vec::new();
    for mode in 0..3 {
        let p = fixture::prepared(MUSIC, 4, &[(1, &[0.25, 0.5, -0.25])]);
        let (mut game, bank) = if mode == 0 {
            StepGameplay::new_audio_section(
                p,
                c,
                bindings(None),
                Timestamp::ZERO,
                None,
                BmsInputMode::ButtonOnly,
                authority(),
            )
            .unwrap()
        } else {
            solo(
                p,
                c,
                None,
                speed(if mode == 1 { 1 } else { 8 }, if mode == 1 { 1 } else { 8 }),
            )
        };
        let header = game.competition_header(fixture::limits(), 0).unwrap();
        assert!(!header.options.starts_with(b"bms-speed-setup/"));
        game.activate_audio().unwrap();
        let (mut producer, mut mixer) = output(bank, c, None);
        let commands = deliver(&mut game, &mut producer);
        let mut pcm = [0.0; 8];
        mixer.render(&mut pcm).unwrap();
        outputs.push((header, commands, pcm));
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0], outputs[2]);
}

#[test]
fn finite_invalid_end_and_negative_preroll_are_cold_refusals() {
    for end in [0, -1] {
        assert!(StepGameplay::new_audio_section_with_speed(
            empty(),
            config(0),
            bindings(None),
            Timestamp::ZERO,
            Some(Timestamp::from_nanos(end)),
            BmsInputMode::ButtonOnly,
            authority(),
            speed(2, 1)
        )
        .is_err());
    }
    assert!(StepGameplay::new_audio_section_with_speed(
        empty(),
        config(-1),
        bindings(None),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        authority(),
        speed(2, 1)
    )
    .is_err());
    let mut c = config(i64::MAX);
    c.output_origin.timestamp = Timestamp::from_nanos(i64::MAX);
    assert!(StepGameplay::new_audio_section_with_speed(
        empty(),
        c,
        bindings(None),
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(1)),
        BmsInputMode::ButtonOnly,
        authority(),
        speed(1, 2)
    )
    .is_err());
}

#[test]
fn legacy_activation_retains_selected_rate_and_exact_preroll_instead_of_resetting_to_normal() {
    let c = config(1);
    let (mut game, _) = StepGameplay::new_section_with_input_mode_and_speed(
        empty(),
        c,
        bindings(None),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        speed(1, 2),
    )
    .unwrap();
    game.activate(host(50)).unwrap();
    assert_eq!(game.playback_speed(), speed(1, 2));
    struct SameDomain;
    impl beatkernel::time::ClockMapper for SameDomain {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> beatkernel::time::ClockMappingQuality {
            beatkernel::time::ClockMappingQuality::Exact
        }
    }
    let report = game.advance_to(host(52), &SameDomain, raw(2)).unwrap();
    assert_eq!(report.song_time.as_nanos(), 0);
    let report = game.advance_to(host(53), &SameDomain, raw(3)).unwrap();
    assert_eq!(report.song_time.as_nanos(), 1);
}

#[test]
fn delayed_pre_end_input_after_unaligned_source_marker_keeps_judgment_capture_and_mine_without_new_audio() {
    let chart = "#BPM 60000\n#WAV00 blast.wav\n#WAV01 key.wav\n#00011:00010000\n#00012:00000001\n#000D1:00010000\n";
    for (n, d, first, second, endpoint) in [(1, 2, 5_000_000, 9_000_000, 11),
        (1, 1, 4_000_000, 6_000_000, 7), (2, 1, 3_500_000, 4_500_000, 5),
        (3, 2, 3_666_667, 5_000_000, 6)] {
        let p = fixture::prepared(chart, 1000, &[(0, &[0.5, -0.5]), (1, &[0.25])]);
        let source = p.source.clone();
        let c = config(3_000_000);
        let (mut game, bank) = solo(p, c, Some(4_000_000), speed(n, d));
        game.configure_capture(fixture::limits(), 0).unwrap(); game.activate_audio().unwrap();
        let (mut producer, mut mixer) = output(bank, c, Some(endpoint));
        let prefix = deliver(&mut game, &mut producer);
        // A normal callback overshoots every endpoint; source playback has really ended.
        let marker = mixer.render(&mut [0.0; 13]).unwrap();
        assert_eq!(marker.playback_end_physical_frame, Some(endpoint));
        assert_eq!(marker.counters.commands_applied as usize, prefix.len());
        assert!(!game.observe_completion(Some(marker), None).unwrap());
        assert!(game.completed_result().is_none());
        assert_eq!(game.score().hits, 0);
        let acquired = endpoint as i64 * 1_000_000;
        observe_pair(&mut game, acquired);
        game.record_audio_prefix(host(acquired)).unwrap();
        let mut merger = InputMerger::new(host(0).domain, host(0), vec![DeviceId(9)], 16).unwrap();
        for (sequence, elapsed, key, state) in [(0, first, 7, ButtonState::Down),
            (1, first + 1, 7, ButtonState::Up), (2, second, 8, ButtonState::Down),
            (3, second + 1, 8, ButtonState::Up)] {
            merger.admit(input(elapsed, 9, key, sequence, state), host(acquired)).unwrap();
            let report = game.process_next_audio_input(&mut merger, host(acquired), raw(13_000_000), None).unwrap().unwrap();
            assert_eq!(report.audio_at, raw(13_000_000));
            assert_eq!(report.input.as_ref().unwrap().meta().original_clock_point, Some(host(elapsed)));
            assert!(report.audio_commands.is_empty()); assert!(report.audio_failures.is_empty());
            if sequence == 0 {
                assert_eq!(report.song_time.as_nanos(), 1_000_000);
                assert_eq!(report.hazard_events.len(), 1);
                assert_eq!(report.hazard_events[0].at.as_nanos(), 1_000_000);
                assert_eq!(report.hazard_events[0].outcome, beatkernel::judge::HazardOutcome::Triggered);
            }
        }
        assert_eq!(game.score().hits, 2); assert_eq!(game.score().misses, 0);
        assert_eq!(game.mine_damage().half_percent_damage, 1);
        assert_eq!(game.gauge().snapshot().level_units, 21_500_000);
        assert!(deliver(&mut game, &mut producer).is_empty());
        assert!(!game.observe_completion(None, Some(raw(acquired))).unwrap());
        let final_report = game.advance_audio_frontier(&mut merger, host(acquired), raw(13_000_000)).unwrap().unwrap();
        assert_eq!(final_report.song_time.as_nanos(), 4_000_000);
        assert!(final_report.audio_commands.is_empty());
        assert!(deliver(&mut game, &mut producer).is_empty());
        assert!(game.observe_completion(None, Some(raw(acquired))).unwrap());
        let hash = game.judge().stable_hash().unwrap();
        // Completion polls are idempotent; only the explicit frontier and first
        // authorized terminal closure are recorded as distinct equal-time Advances.
        assert!(game.observe_completion(None, Some(raw(acquired))).unwrap());
        game.fail();
        let bytes = game.take_replay().unwrap().unwrap();
        let file = decode_replay(&bytes, fixture::limits()).unwrap();
        let first_up_song = match (n, d) { (1, 2) => 1_000_000, (1, 1) => 1_000_001, _ => 1_000_002 };
        let second_up_song = match (n, d) { (1, 2) => 3_000_000, (2, 1) => 3_000_002, _ => 3_000_001 };
        let expected = [(1_000_000, Some((first, ButtonState::Down))),
            (first_up_song, Some((first + 1, ButtonState::Up))),
            (3_000_000, Some((second, ButtonState::Down))),
            (second_up_song, Some((second + 1, ButtonState::Up))),
            (4_000_000, None), (4_000_000, None)];
        assert_eq!(file.records.len(), expected.len());
        for (ordinal, (record, (song, physical))) in file.records.iter().zip(expected).enumerate() {
            assert_eq!(record.ordinal, ordinal as u64);
            assert_eq!(record.song_time.as_nanos(), song);
            match (&record.operation, physical) {
                (ReplayOperation::Input(saved), Some((elapsed, state))) => {
                    assert_eq!(saved.physical.meta().original_clock_point, Some(host(elapsed)));
                    assert_eq!(saved.physical.meta().source, DeviceId(9));
                    assert_eq!(saved.physical.meta().sequence, ordinal as u64);
                    let PhysicalInputEvent::Button(button) = &saved.physical else { panic!("expected original button"); };
                    assert_eq!(button.state, state);
                }
                (ReplayOperation::Advance, None) => {}
                _ => panic!("unexpected captured operation at ordinal {ordinal}"),
            }
        }
        let mut replay = reconstruct_section(&source, file, fixture::limits()).unwrap();
        replay.seek_cursor(6).unwrap(); assert_eq!(replay.engine().stable_hash().unwrap(), hash);
        let after = mixer.render(&mut [0.0; 1]).unwrap();
        assert_eq!(after.counters.commands_consumed, marker.counters.commands_consumed);
        assert_eq!(after.counters.commands_applied, marker.counters.commands_applied);
        assert_eq!(after.pending_commands, 0);
    }
}

#[test]
fn delayed_local_pre_end_events_after_source_seal_preserve_all_members_and_exact_prior_credits() {
    for count in [2, 4] {
        for (n, d, first, second, endpoint) in [(1, 2, 5_000_000, 9_000_000, 11),
            (1, 1, 4_000_000, 6_000_000, 7), (2, 1, 3_500_000, 4_500_000, 5),
            (3, 2, 3_666_667, 5_000_000, 6)] {
            let c = config(3_000_000); let (mut game, bank) = local(count, c, Some(4_000_000), speed(n, d));
            for id in 1..=count { game.configure_capture(PlayerId(id as u32), fixture::limits(), 0).unwrap(); }
            game.activate_audio().unwrap();
            let (mut producer, mut mixer) = output(bank, c, Some(endpoint));
            let prefix = deliver_local(&mut game, &mut producer);
            let marker = mixer.render(&mut [0.0; 13]).unwrap();
            game.admit_output_evidence(Some(marker), None).unwrap();
            assert_eq!(marker.counters.commands_applied as usize, prefix.len());
            let acquired = endpoint as i64 * 1_000_000;
            for elapsed in [0, acquired] { game.observe_audio_output(1, ClockPair { source: raw(elapsed), target: host(elapsed) }).unwrap(); }
            game.record_audio_prefix(host(acquired)).unwrap();
            let mut merger = InputMerger::new(host(0).domain, host(0), (1..=count).map(|id| DeviceId(id as u64)).collect(), 64).unwrap();
            // Admit the complete late-acquired prefix before processing, preserving global chronological order.
            for id in 1..=count {
                for (sequence, elapsed, key, state) in [(0, first, 7, ButtonState::Down),
                    (1, first + 1, 7, ButtonState::Up), (2, second, 8, ButtonState::Down),
                    (3, second + 1, 8, ButtonState::Up)] {
                    merger.admit(input(elapsed, id as u64, key, sequence, state), host(acquired)).unwrap();
                }
            }
            for _ in 0..count * 4 {
                let Some(InputResult::Processed(reports)) = game.process_next_audio_input(&mut merger, host(acquired), raw(13_000_000), None).unwrap() else { panic!("late member input missing"); };
                assert_eq!(reports.len(), 1);
                assert!(reports[0].report.audio_commands.is_empty());
                assert!(reports[0].report.audio_failures.is_empty());
                assert_eq!(reports[0].report.audio_at, raw(13_000_000));
                assert_eq!(reports[0].report.input.as_ref().unwrap().meta().source.0, u64::from(reports[0].player.0));
            }
            assert!(deliver_local(&mut game, &mut producer).is_empty());
            assert!(!game.observe_completion(None, Some(raw(acquired))).unwrap());
            let reports = game.advance_audio_frontier(&mut merger, host(acquired), raw(13_000_000)).unwrap().unwrap();
            assert_eq!(reports.len(), count);
            assert!(reports.iter().all(|r| r.report.song_time.as_nanos() == 4_000_000 && r.report.audio_commands.is_empty()));
            assert!(game.observe_completion(None, Some(raw(acquired))).unwrap());
            let mut hashes = Vec::new();
            for id in 1..=count {
                let player = PlayerId(id as u32);
                assert_eq!(game.score(player).unwrap().hits, 2); assert_eq!(game.score(player).unwrap().misses, 0);
                assert_eq!(game.gauge(player).unwrap().snapshot().level_units, 22_000_000);
                assert!(game.completed_result(player).unwrap().is_some());
                hashes.push(game.judge(player).unwrap().stable_hash().unwrap());
            }
            assert!(game.observe_completion(None, Some(raw(acquired))).unwrap());
            // Inspect admissions before explicitly fencing the owner for export.
            // take_commands deliberately refuses a failed owner.
            assert!(deliver_local(&mut game, &mut producer).is_empty());
            game.fail();
            for id in 1..=count {
                let bytes = game.take_replay(PlayerId(id as u32)).unwrap().unwrap();
                let file = decode_replay(&bytes, fixture::limits()).unwrap();
                let first_up_song = match (n, d) { (1, 2) => 1_000_000, (1, 1) => 1_000_001, _ => 1_000_002 };
                let second_up_song = match (n, d) { (1, 2) => 3_000_000, (2, 1) => 3_000_002, _ => 3_000_001 };
                let expected = [(1_000_000, Some((first, ButtonState::Down))),
                    (first_up_song, Some((first + 1, ButtonState::Up))),
                    (3_000_000, Some((second, ButtonState::Down))),
                    (second_up_song, Some((second + 1, ButtonState::Up))),
                    (4_000_000, None), (4_000_000, None)];
                assert_eq!(file.records.len(), expected.len());
                for (ordinal, (record, (song, physical))) in file.records.iter().zip(expected).enumerate() {
                    assert_eq!(record.ordinal, ordinal as u64);
                    assert_eq!(record.song_time.as_nanos(), song);
                    match (&record.operation, physical) {
                        (ReplayOperation::Input(saved), Some((elapsed, state))) => {
                            assert_eq!(saved.physical.meta().original_clock_point, Some(host(elapsed)));
                            assert_eq!(saved.physical.meta().source, DeviceId(id as u64));
                            assert_eq!(saved.physical.meta().sequence, ordinal as u64);
                            let PhysicalInputEvent::Button(button) = &saved.physical else { panic!("expected original member button"); };
                            assert_eq!(button.state, state);
                        }
                        (ReplayOperation::Advance, None) => {}
                        _ => panic!("unexpected member captured operation at ordinal {ordinal}"),
                    }
                }
                let mut replay = reconstruct_section(&notes().source, file, fixture::limits()).unwrap();
                replay.seek_cursor(6).unwrap(); assert_eq!(replay.engine().stable_hash().unwrap(), hashes[id - 1]);
            }
            let after = mixer.render(&mut [0.0; 1]).unwrap();
            assert_eq!(after.counters.commands_applied, marker.counters.commands_applied);
            assert_eq!(after.counters.commands_consumed, marker.counters.commands_consumed);
            assert_eq!(after.pending_commands, 0);
        }
    }
}

#[test]
fn genuine_pre_endpoint_render_keeps_sound_admission_and_wrong_grid_report_cannot_complete() {
    let c = config(3_000_000);
    let (mut game, bank) = solo(notes(), c, Some(4_000_000), speed(2, 1));
    game.activate_audio().unwrap();
    let (mut producer, mut mixer) = output(bank, c, Some(5));
    deliver(&mut game, &mut producer);
    let early = mixer.render(&mut [0.0; 1]).unwrap();
    assert_eq!(early.playback_end_physical_frame, None);
    game.admit_output_evidence(Some(early), None).unwrap();
    observe_pair(&mut game, 4_000_000); game.record_audio_prefix(host(4_000_000)).unwrap();
    let mut merger = InputMerger::new(host(0).domain, host(0), vec![DeviceId(9)], 16).unwrap();
    merger.admit(input(3_500_000, 9, 7, 0, ButtonState::Down), host(4_000_000)).unwrap();
    let hit = game.process_next_audio_input(&mut merger, host(4_000_000), raw(4_000_000), None).unwrap().unwrap();
    assert_eq!(hit.song_time.as_nanos(), 1_000_000);
    assert!(hit.audio_commands.iter().any(|c| matches!(c, AudioCommand::Play { at, .. } if *at == raw(4_000_000).timestamp)));
    assert!(!game.observe_completion(None, Some(raw(1_000_000))).unwrap());

    // A genuine report from a different immutable grid remains invalid evidence.
    let (mut rejected, bank) = solo(notes(), c, Some(4_000_000), speed(2, 1));
    rejected.activate_audio().unwrap();
    let (_producer, mut wrong_mixer) = output(bank, c, Some(4));
    let wrong_marker = wrong_mixer.render(&mut [0.0; 13]).unwrap();
    assert_eq!(wrong_marker.playback_end_physical_frame, Some(4));
    assert!(rejected.admit_output_evidence(Some(wrong_marker), Some(raw(5_000_000))).is_err());
    assert!(rejected.completed_result().is_none());
    assert_eq!(rejected.score().hits, 0); assert_eq!(rejected.score().misses, 0);
    assert!(rejected.observe_completion(None, None).is_err());
}
