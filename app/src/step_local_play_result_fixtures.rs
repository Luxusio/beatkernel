//! Deferred per-member result publication only after actual shared output completion.
use crate::{
    gauge::GaugeFailure,
    local_players::{PlayerId, ResolvedInputPlan},
    mine_audio_consumers_fixtures::{data, replay_limits},
    play_result::{PlayResultOutcome, PlayResultScope},
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
    replay::codec::decode_replay,
};
use beatkernel_bms::BmsInputMode;
const TEXT: &str = "#BPM 60\n#VOLWAV 50\n#WAV01 note\n#WAV02 music\n#00011:01000100\n#000D1:00ZZ0001\n#00001:00000002";
fn owner(end: Option<i64>, both: bool) -> (StepLocalGameplay, SampleBank) {
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
        assert!(game.completed_result(player).unwrap().is_none());
        game.configure_capture(player, replay_limits(), 0).unwrap();
    }
    game.activate(host(0)).unwrap();
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
    game.advance_to(host(SECOND), &Identity, output(SECOND))
        .unwrap();
    (game, bank)
}
fn no_results(game: &StepLocalGameplay) {
    for player in [FAILED, HEALTHY] {
        assert!(game.completed_result(player).unwrap().is_none());
    }
}
fn send(game: &mut StepLocalGameplay, producer: &mut CommandProducer) {
    let batch = game.take_commands(16).unwrap().unwrap();
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
}

#[test]
fn cohort_results_belong_to_each_actual_member_and_wait_for_survivor_and_shared_drain() {
    for both in [false, true] {
        let (mut game, bank) = owner(None, both);
        no_results(&game);
        assert!(matches!(
            game.completed_result(PlayerId(9)),
            Err(StepLocalGameplayError::UnknownPlayer(PlayerId(9)))
        ));
        let failed_hash = game.judge(FAILED).unwrap().stable_hash().unwrap();
        let (mut producer, mut remote) = mixer(bank, 8, None);
        let batch = game.take_commands(16).unwrap().unwrap();
        let mut expected = vec![play(1, 91, 0, 0.5), play(1, 93, 0, 0.5), stop(91), stop(92)];
        if both {
            expected.extend([stop(93), stop(94)]);
        }
        assert_eq!(batch.commands, expected);
        assert!(!game.observe_completion(None, None).unwrap());
        no_results(&game);
        for &command in &batch.commands {
            producer.try_push(command).unwrap();
        }
        game.acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
        let first = remote.render(&mut [0.0; 20]).unwrap();
        assert!(
            !game
                .observe_completion(Some(first), Some(output(2 * SECOND)))
                .unwrap()
        );
        no_results(&game);
        if !both {
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
        }
        game.feed_audio(20, 8).unwrap();
        send(&mut game, &mut producer);
        let mut pcm = [0.0; 20];
        let end = remote.render(&mut pcm).unwrap();
        let mut expected = [0.0; 20];
        if !both {
            expected[0] = 0.5;
            expected[1] = -0.5;
        }
        expected[10] = 0.125;
        expected[11] = 0.375;
        assert_eq!(pcm, expected);
        game.feed_audio(40, 8).unwrap();
        if !both {
            assert!(
                !game
                    .observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap()
            );
            no_results(&game);
            assert_eq!(game.judge(HEALTHY).unwrap().remaining_hazards(), 1);
            game.advance_to(host(3 * SECOND), &Identity, output(3 * SECOND))
                .unwrap();
            assert!(
                !game
                    .observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap()
            );
            no_results(&game);
            game.advance_to(host(3 * SECOND + 1), &Identity, output(3 * SECOND + 1))
                .unwrap();
        }
        assert!(
            !game
                .observe_completion(Some(end), Some(output(4 * SECOND)))
                .unwrap()
        );
        no_results(&game);
        let idle = remote.render(&mut [0.0]).unwrap();
        assert!(
            !game
                .observe_completion(Some(idle), Some(output(4_100_000_000 - 1)))
                .unwrap()
        );
        no_results(&game);
        assert!(
            game.observe_completion(Some(idle), Some(output(4_100_000_000)))
                .unwrap()
        );
        let results = [
            *game.completed_result(FAILED).unwrap().unwrap(),
            *game.completed_result(HEALTHY).unwrap().unwrap(),
        ];
        for (index, player) in [FAILED, HEALTHY].into_iter().enumerate() {
            assert_eq!(results[index].scope(), PlayResultScope::FullSong);
            assert_eq!(
                results[index].gauge(),
                *game.gauge(player).unwrap().snapshot()
            );
            assert!(!results[index].whole_song_clear());
            assert_eq!(
                results[index].outcome(),
                if both || player == FAILED {
                    PlayResultOutcome::Failed(GaugeFailure::InstantDeath)
                } else {
                    PlayResultOutcome::BelowClearThreshold
                }
            );
        }
        assert_eq!(results[0].gauge().level_units, 0);
        assert_eq!(
            results[1].gauge().level_units,
            if both { 0 } else { 22_000_000 },
            "the private controller's default gauge is not the member's result"
        );
        assert_eq!(game.member_song_time(FAILED), Some(ts(SECOND)));
        assert_eq!(
            game.judge(FAILED).unwrap().stable_hash().unwrap(),
            failed_hash
        );
        assert_eq!(
            game.judge(FAILED).unwrap().state(ObjectId(2)),
            Some(InteractionState::Pending)
        );
        assert_eq!(game.judge(FAILED).unwrap().remaining_hazards(), 1);
        let hashes = [
            failed_hash,
            game.judge(HEALTHY).unwrap().stable_hash().unwrap(),
        ];
        let progress = game.group_progress().unwrap();
        assert!(
            game.observe_completion(Some(idle), Some(output(4_100_000_000)))
                .unwrap()
        );
        assert_eq!(game.group_progress().unwrap(), progress);
        let mut invalid = idle;
        invalid.counters.unknown_samples = 1;
        assert!(
            matches!(game.observe_completion(Some(invalid), Some(output(4_100_000_000))), Err(StepLocalGameplayError::Control(StepGameplayError::Completion { rendered: Some(raw), .. })) if raw == invalid)
        );
        game.fail();
        let source = beatkernel_bms::parse(TEXT, Default::default()).unwrap();
        for (index, player) in [FAILED, HEALTHY].into_iter().enumerate() {
            assert_eq!(
                game.completed_result(player).unwrap(),
                Some(&results[index])
            );
            let file = decode_replay(&game.take_replay(player).unwrap().unwrap(), replay_limits())
                .unwrap();
            if both || player == FAILED {
                assert_eq!(file.records.len(), 2);
                assert_eq!(file.records.last().unwrap().song_time, ts(SECOND));
            }
            assert_eq!(
                reconstruct_section(&source, file, replay_limits())
                    .unwrap()
                    .engine()
                    .stable_hash()
                    .unwrap(),
                hashes[index]
            );
        }
    }
}

#[test]
fn local_finite_results_are_atomic_after_exact_output_and_absent_after_partial_ack_or_invalid_output()
 {
    for fault in 0..3 {
        let (mut game, bank) = owner(Some(4 * SECOND), true);
        let (mut producer, mut remote) = mixer(bank, 8, Some(40));
        no_results(&game);
        if fault == 1 {
            let batch = game.take_commands(16).unwrap().unwrap();
            for &command in &batch.commands[..3] {
                producer.try_push(command).unwrap();
            }
            assert!(matches!(
                game.acknowledge(batch.sequence, 3, false),
                Err(StepLocalGameplayError::Control(
                    StepGameplayError::AudioRejected { admitted: 3, .. }
                ))
            ));
            assert_eq!(game.acknowledged_stop_commands(), 1);
            let raw = remote.render(&mut [0.0; 40]).unwrap();
            assert!(
                game.observe_completion(Some(raw), Some(output(4 * SECOND)))
                    .is_err()
            );
            no_results(&game);
            continue;
        }
        send(&mut game, &mut producer);
        let first = remote.render(&mut [0.0; 20]).unwrap();
        assert!(
            !game
                .observe_completion(Some(first), Some(output(2 * SECOND)))
                .unwrap()
        );
        no_results(&game);
        game.feed_audio(20, 8).unwrap();
        let music = game.take_commands(16).unwrap().unwrap();
        assert_eq!(music.commands, [play(2, 90, 3 * SECOND, 0.5)]);
        producer.try_push(music.commands[0]).unwrap();
        let end = remote.render(&mut [0.0; 20]).unwrap();
        assert_eq!(end.playback_end_physical_frame, Some(40));
        assert!(
            !game
                .observe_completion(Some(end), Some(output(4 * SECOND)))
                .unwrap()
        );
        no_results(&game);
        game.acknowledge(music.sequence, 1, true).unwrap();
        game.feed_audio(40, 8).unwrap();
        if fault == 2 {
            let mut bad = end;
            bad.counters.commands_applied += 1;
            assert!(
                matches!(game.observe_completion(Some(bad), Some(output(4 * SECOND))), Err(StepLocalGameplayError::Control(StepGameplayError::Completion { rendered: Some(raw), .. })) if raw == bad)
            );
            no_results(&game);
            continue;
        }
        // Earlier valid presentation was already exactly at the immutable end;
        // only receipt and BGM retirement remained outstanding then.
        assert!(
            game.observe_completion(Some(end), Some(output(4 * SECOND)))
                .unwrap()
        );
        for player in [FAILED, HEALTHY] {
            let result = *game.completed_result(player).unwrap().unwrap();
            assert_eq!(
                result.scope(),
                PlayResultScope::PracticeSection {
                    start: ts(0),
                    end: Some(ts(4 * SECOND))
                }
            );
            assert_eq!(
                result.outcome(),
                PlayResultOutcome::Failed(GaugeFailure::InstantDeath)
            );
            assert!(!result.whole_song_clear());
            assert_eq!(game.member_song_time(player), Some(ts(SECOND)));
        }
        game.fail();
        assert!(game.completed_result(FAILED).unwrap().is_some());
        assert!(game.completed_result(HEALTHY).unwrap().is_some());
    }
    let (fresh, _) = owner(None, false);
    no_results(&fresh);
}
