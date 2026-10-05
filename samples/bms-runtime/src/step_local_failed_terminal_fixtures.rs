//! Deferred cohort terminal evidence; failed members retain their canonical
//! unfinished state while the shared output queue and healthy members finish.
use crate::{
    gauge::GaugeFailure,
    local_players::ResolvedInputPlan,
    mine_audio_consumers_fixtures::{data, replay_limits},
    replay_playback::reconstruct_section,
    step_gameplay::{StepGameplayError, StepLocalGameplay, StepLocalGameplayError},
    step_local_stop_ack_fixtures::{FAILED, HEALTHY, SOURCES},
    step_solo_stop_ack_fixtures::{
        bindings, config, host, input, mixer, output, play, stop, ts, Identity, SECOND,
    },
};
use beatkernel::{
    audio::{CommandProducer, SampleBank},
    chart::ObjectId,
    input::{ButtonState, DeviceId, DeviceSelector},
    interaction::InteractionState,
    replay::{ReplayOperation, codec::decode_replay},
};
use beatkernel_bms::BmsInputMode;

const TEXT: &str = "#BPM 60\n#VOLWAV 50\n#WAV01 note\n#WAV02 music\n#00011:01000100\n#000D1:00ZZ0001\n#00001:00000002";

fn owner(end: Option<i64>) -> (StepLocalGameplay, SampleBank) {
    let plan = ResolvedInputPlan::new(vec![
        (FAILED, Some(DeviceId(SOURCES[0]))),
        (HEALTHY, Some(DeviceId(SOURCES[1]))),
    ])
    .unwrap();
    let (mut game, bank) = StepLocalGameplay::new_section(
        data(TEXT, false),
        config(),
        plan,
        SOURCES
            .iter()
            .map(|&source| bindings(DeviceSelector::Exact(DeviceId(source))))
            .collect(),
        ts(0),
        end.map(ts),
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    for player in [FAILED, HEALTHY] {
        game.configure_capture(player, replay_limits(), 0).unwrap();
    }
    game.activate(host(0)).unwrap();
    (game, bank)
}
fn fail_at_one(game: &mut StepLocalGameplay, both: bool) {
    assert_eq!(game.feed_audio(0, 8).unwrap().admitted, 0);
    for (index, source) in SOURCES.into_iter().enumerate() {
        game.process_input(
            input(source, 0, index as u64 + 1, ButtonState::Down),
            &Identity,
            output(0),
        )
        .unwrap();
    }
    if !both {
        game.process_input(
            input(SOURCES[1], 500_000_000, 3, ButtonState::Up),
            &Identity,
            output(500_000_000),
        )
        .unwrap();
    }
    let reports = game
        .advance_to(host(SECOND), &Identity, output(SECOND))
        .unwrap();
    assert_eq!(reports[0].player, FAILED);
    assert_eq!(reports[0].report.audio_commands, [stop(91), stop(92)]);
    assert_eq!(reports[1].player, HEALTHY);
    if both {
        assert_eq!(reports[1].report.audio_commands, [stop(93), stop(94)]);
    } else {
        assert!(reports[1].report.audio_commands.is_empty());
    }
    assert_eq!(game.gameplay_fence(FAILED), Some(ts(SECOND)));
    assert_eq!(game.gameplay_fence(HEALTHY), both.then_some(ts(SECOND)));
    assert_eq!(game.judge(FAILED).unwrap().remaining_hazards(), 1);
    assert_eq!(
        game.judge(FAILED).unwrap().state(ObjectId(2)),
        Some(InteractionState::Pending)
    );
}
fn admit_prefix(game: &mut StepLocalGameplay, producer: &mut CommandProducer, both: bool) {
    let batch = game.take_commands(16).unwrap().unwrap();
    let mut expected = vec![play(1, 91, 0, 0.5), play(1, 93, 0, 0.5), stop(91), stop(92)];
    if both {
        expected.extend([stop(93), stop(94)]);
    }
    assert_eq!(batch.commands, expected);
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    assert_eq!(game.acknowledged_stop_commands(), 0);
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    assert_eq!(game.acknowledged_stop_commands(), if both { 4 } else { 2 });
}

#[test]
fn all_failed_members_finish_unlimited_or_finite_output_without_advancing_their_captures() {
    for finite in [false, true] {
        let (mut game, bank) = owner(finite.then_some(4 * SECOND));
        fail_at_one(&mut game, true);
        let hashes = [
            game.judge(FAILED).unwrap().stable_hash().unwrap(),
            game.judge(HEALTHY).unwrap().stable_hash().unwrap(),
        ];
        let gauges = [
            game.gauge(FAILED).unwrap().clone(),
            game.gauge(HEALTHY).unwrap().clone(),
        ];
        let progress = game.group_progress().unwrap();
        let (mut producer, mut remote) = mixer(bank, 8, finite.then_some(40));
        admit_prefix(&mut game, &mut producer, true);
        let mut pcm = [0.0; 20];
        let first = remote.render(&mut pcm).unwrap();
        let mut expected = [0.0; 20];
        expected[0] = 1.0;
        expected[1] = -1.0;
        assert_eq!(pcm, expected);
        assert_eq!(
            (
                first.counters.commands_applied,
                first.counters.unknown_stops
            ),
            (6, 4)
        );
        assert!(
            !game
                .observe_completion(Some(first), Some(output(2 * SECOND)))
                .unwrap()
        );
        assert_eq!(game.bgm_report().remaining, 1);
        game.feed_audio(20, 8).unwrap();
        let music = game.take_commands(16).unwrap().unwrap();
        assert_eq!(music.commands, [play(2, 90, 3 * SECOND, 0.5)]);
        producer.try_push(music.commands[0]).unwrap();
        game.acknowledge(music.sequence, 1, true).unwrap();
        let mut pcm = [0.0; 20];
        let end = remote.render(&mut pcm).unwrap();
        let mut expected = [0.0; 20];
        expected[10] = 0.125;
        expected[11] = 0.375;
        assert_eq!(pcm, expected);
        assert_eq!(
            (
                end.counters.commands_consumed,
                end.counters.commands_applied,
                end.counters.unknown_stops
            ),
            (7, 7, 4)
        );
        assert!(
            !game
                .observe_completion(Some(end), Some(output(4 * SECOND - 1)))
                .unwrap()
        );
        assert_eq!(game.bgm_report().outstanding, 1);
        game.feed_audio(40, 8).unwrap();
        if finite {
            assert_eq!(end.playback_end_physical_frame, Some(40));
            assert!(
                !game
                    .observe_completion(Some(end), Some(output(4 * SECOND - 1)))
                    .unwrap()
            );
            assert!(
                game.observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap()
            );
        } else {
            assert!(
                !game
                    .observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap()
            );
            assert!(
                !game
                    .observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap()
            );
            let idle = remote.render(&mut [0.0]).unwrap();
            assert!(
                !game
                    .observe_completion(Some(idle), Some(output(4_100_000_000 - 1)))
                    .unwrap()
            );
            assert!(
                game.observe_completion(Some(idle), Some(output(4_100_000_000)))
                    .unwrap()
            );
        }
        assert_eq!(game.song_time(), ts(SECOND));
        assert_eq!(game.group_progress().unwrap(), progress);
        assert!(!game.failed());
        let later = game
            .advance_to(host(8 * SECOND), &Identity, output(8 * SECOND))
            .unwrap();
        assert!(later.iter().all(|row| row.report.song_time == ts(SECOND)
            && row.report.judge_events.is_empty()
            && row.report.hazard_events.is_empty()
            && row.report.audio_commands.is_empty()));
        for (index, player) in [FAILED, HEALTHY].into_iter().enumerate() {
            assert_eq!(
                game.judge(player).unwrap().stable_hash().unwrap(),
                hashes[index]
            );
            assert_eq!(game.judge(player).unwrap().remaining_hazards(), 1);
            assert_eq!(game.gauge(player).unwrap(), &gauges[index]);
            assert_eq!(
                game.gauge(player).unwrap().snapshot().failure,
                Some(GaugeFailure::InstantDeath)
            );
            assert!(!game.gauge(player).unwrap().can_clear());
        }
        game.fail();
        let source = beatkernel_bms::parse(TEXT, Default::default()).unwrap();
        for (index, player) in [FAILED, HEALTHY].into_iter().enumerate() {
            let file = decode_replay(&game.take_replay(player).unwrap().unwrap(), replay_limits())
                .unwrap();
            assert_eq!(file.records.len(), 2);
            assert_eq!(file.records[1].song_time, ts(SECOND));
            let ReplayOperation::Input(input) = &file.records[0].operation else {
                panic!("capture must retain its actual source");
            };
            assert_eq!(input.physical.meta().source.0, SOURCES[index]);
            let rebuilt = reconstruct_section(&source, file, replay_limits()).unwrap();
            assert_eq!(rebuilt.engine().stable_hash().unwrap(), hashes[index]);
        }
        assert!(matches!(
            game.observe_completion(Some(end), None),
            Err(StepLocalGameplayError::Control(StepGameplayError::Failed))
        ));
    }
}

#[test]
fn failed_primary_does_not_skip_healthy_notes_hazards_deadline_or_finite_end() {
    for finite in [false, true] {
        let (mut game, bank) = owner(finite.then_some(4 * SECOND));
        fail_at_one(&mut game, false);
        let failed_hash = game.judge(FAILED).unwrap().stable_hash().unwrap();
        let (mut producer, mut remote) = mixer(bank, 8, finite.then_some(40));
        admit_prefix(&mut game, &mut producer, false);
        let first = remote.render(&mut [0.0; 20]).unwrap();
        assert!(
            !game
                .observe_completion(Some(first), Some(output(2 * SECOND)))
                .unwrap()
        );
        game.process_input(
            input(SOURCES[1], 2 * SECOND, 4, ButtonState::Down),
            &Identity,
            output(2 * SECOND),
        )
        .unwrap();
        game.process_input(
            input(SOURCES[1], 2 * SECOND, 5, ButtonState::Up),
            &Identity,
            output(2 * SECOND),
        )
        .unwrap();
        assert_eq!(game.score(HEALTHY).unwrap().hits, 2);
        assert_eq!(
            game.judge(HEALTHY).unwrap().state(ObjectId(2)),
            Some(InteractionState::Completed)
        );
        assert_eq!(game.judge(HEALTHY).unwrap().remaining_hazards(), 1);
        game.feed_audio(20, 8).unwrap();
        let later = game.take_commands(16).unwrap().unwrap();
        assert_eq!(
            later.commands,
            [play(1, 94, 2 * SECOND, 0.5), play(2, 90, 3 * SECOND, 0.5)]
        );
        for &command in &later.commands {
            producer.try_push(command).unwrap();
        }
        game.acknowledge(later.sequence, 2, true).unwrap();
        assert_eq!(game.acknowledged_stop_commands(), 2);
        let mut pcm = [0.0; 20];
        let end = remote.render(&mut pcm).unwrap();
        let mut expected = [0.0; 20];
        expected[0] = 0.5;
        expected[1] = -0.5;
        expected[10] = 0.125;
        expected[11] = 0.375;
        assert_eq!(pcm, expected);
        assert_eq!(
            (end.counters.commands_applied, end.counters.unknown_stops),
            (6, 2)
        );
        game.feed_audio(40, 8).unwrap();
        assert!(
            !game
                .observe_completion(Some(end), Some(output(4 * SECOND)))
                .unwrap(),
            "a healthy pending hazard still blocks terminal readiness"
        );
        game.advance_to(host(3 * SECOND), &Identity, output(3 * SECOND))
            .unwrap();
        assert_eq!(game.judge(HEALTHY).unwrap().remaining_hazards(), 0);
        assert!(
            !game
                .observe_completion(Some(end), Some(output(4 * SECOND)))
                .unwrap(),
            "the exact final hazard time precedes the strict prepared threshold"
        );
        game.advance_to(host(3 * SECOND + 1), &Identity, output(3 * SECOND + 1))
            .unwrap();
        if finite {
            assert!(
                !game
                    .observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap(),
                "healthy finite members still must reach end"
            );
            game.advance_to(host(4 * SECOND), &Identity, output(4 * SECOND))
                .unwrap();
            assert!(
                game.observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap()
            );
        } else {
            assert!(
                !game
                    .observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap()
            );
            let idle = remote.render(&mut [0.0]).unwrap();
            assert!(
                !game
                    .observe_completion(Some(idle), Some(output(4_100_000_000 - 1)))
                    .unwrap()
            );
            assert!(
                game.observe_completion(Some(idle), Some(output(4_100_000_000)))
                    .unwrap()
            );
        }
        assert_eq!(game.member_song_time(FAILED), Some(ts(SECOND)));
        let healthy_song = if finite { 4 * SECOND } else { 3 * SECOND + 1 };
        assert_eq!(game.member_song_time(HEALTHY), Some(ts(healthy_song)));
        assert_eq!(game.song_time(), ts(healthy_song));
        assert_eq!(
            game.judge(FAILED).unwrap().stable_hash().unwrap(),
            failed_hash
        );
        assert_eq!(game.judge(FAILED).unwrap().remaining_hazards(), 1);
        assert_eq!(game.gauge(HEALTHY).unwrap().snapshot().failure, None);
        assert_eq!(game.gameplay_fence(HEALTHY), None);
        assert!(!game.failed());
        let healthy_hash = game.judge(HEALTHY).unwrap().stable_hash().unwrap();
        game.fail();
        let source = beatkernel_bms::parse(TEXT, Default::default()).unwrap();
        for (player, hash) in [(FAILED, failed_hash), (HEALTHY, healthy_hash)] {
            let file = decode_replay(&game.take_replay(player).unwrap().unwrap(), replay_limits())
                .unwrap();
            if player == FAILED {
                assert_eq!(file.records.len(), 2);
            } else {
                assert_eq!(file.records.last().unwrap().song_time, ts(healthy_song));
            }
            let rebuilt = reconstruct_section(&source, file, replay_limits()).unwrap();
            assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
        }
    }
}
