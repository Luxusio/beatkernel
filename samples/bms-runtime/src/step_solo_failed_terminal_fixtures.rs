//! Deferred terminal-readiness fixtures over typed setup, actual queues and Mixer.
//! Completion is not chart clearance or physical presentation proof.
use crate::{
    gauge::GaugeFailure,
    mine_audio_consumers_fixtures::replay_limits,
    replay_playback::reconstruct_section,
    step_gameplay::{StepGameplay, StepGameplayError},
    step_solo_stop_ack_fixtures::{
        host, input, mixer, output, owner, play, rejected_output, stop, ts, Identity, SECOND,
    },
};
use beatkernel::{
    audio::{CommandProducer, SampleBank},
    chart::ObjectId,
    input::ButtonState,
    interaction::InteractionState,
    replay::{ReplayOperation, codec::decode_replay},
};

const TEXT: &str = "#BPM 60\n#WAV01 note\n#WAV02 press\n#00011:01000100\n#00031:02\n#000D1:00ZZ0001\n#00001:00000002";

fn failed_owner(end: Option<i64>) -> (StepGameplay, SampleBank) {
    let (mut game, bank) = owner(TEXT, end);
    game.configure_capture(replay_limits(), 0).unwrap();
    assert_eq!(game.feed_audio(0, 8).unwrap().admitted, 0);
    let hit = game
        .process_input(
            input(u64::MAX, 0, 1, ButtonState::Down),
            &Identity,
            output(0),
        )
        .unwrap();
    assert_eq!(hit.audio_commands, [play(1, 11, 0, 1.0)]);
    let fatal = game
        .advance_to(host(SECOND), &Identity, output(SECOND))
        .unwrap();
    assert_eq!(fatal.audio_commands, [stop(11), stop(12), stop(91)]);
    assert_eq!(game.gameplay_fence(), Some(ts(SECOND)));
    assert_eq!(
        game.gauge().snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    assert_eq!(game.judge().remaining_hazards(), 1);
    assert_eq!(
        game.judge().state(ObjectId(2)),
        Some(InteractionState::Pending)
    );
    (game, bank)
}
fn admit(game: &mut StepGameplay, producer: &mut CommandProducer) {
    let batch = game.take_commands(16).unwrap().unwrap();
    assert_eq!(
        batch.commands,
        [play(1, 11, 0, 1.0), stop(11), stop(12), stop(91)]
    );
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    assert_eq!(game.acknowledged_stop_commands(), 0);
    game.acknowledge(batch.sequence, 4, true).unwrap();
    assert_eq!(game.acknowledged_stop_commands(), 3);
}

#[test]
fn solo_failed_prefix_finishes_only_after_future_bgm_ack_credit_retirement_and_idle_presentation() {
    let (mut game, bank) = failed_owner(None);
    let hash = game.judge().stable_hash().unwrap();
    let pending = game.judge().state(ObjectId(2));
    let score = game.score().clone();
    let gauge = game.gauge().clone();
    let (mut producer, mut remote) = mixer(bank, 8, None);
    admit(&mut game, &mut producer);
    let mut pcm = [0.0; 20];
    let first = remote.render(&mut pcm).unwrap();
    let mut expected = [0.0; 20];
    expected[0] = 1.0;
    expected[1] = -1.0;
    assert_eq!(pcm, expected);
    assert_eq!(first.counters.unknown_stops, 3);
    assert!(
        !game
            .observe_completion(Some(first), Some(output(2 * SECOND)))
            .unwrap()
    );
    assert_eq!(
        game.bgm_report().remaining,
        1,
        "failure does not discard future BGM"
    );

    game.feed_audio(20, 8).unwrap();
    let music = game.take_commands(16).unwrap().unwrap();
    assert_eq!(music.commands, [play(2, 90, 3 * SECOND, 1.0)]);
    producer.try_push(music.commands[0]).unwrap();
    let before_music = remote.render(&mut [0.0; 10]).unwrap();
    assert_eq!(before_music.pending_commands, 1);
    assert!(
        !game
            .observe_completion(Some(before_music), Some(output(3 * SECOND)))
            .unwrap()
    );
    assert!(
        matches!(game.take_commands(16), Err(StepGameplayError::OutstandingBatch { sequence }) if sequence == music.sequence)
    );
    game.acknowledge(music.sequence, 1, true).unwrap();
    assert_eq!(game.acknowledged_stop_commands(), 3);
    let mut pcm = [0.0; 2];
    let tail = remote.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.75]);
    assert_eq!(
        (tail.counters.commands_applied, tail.counters.unknown_stops),
        (5, 3)
    );
    assert!(
        !game
            .observe_completion(Some(tail), Some(output(3_200_000_000)))
            .unwrap()
    );
    assert_eq!(game.bgm_report().outstanding, 1);
    game.feed_audio(32, 8).unwrap();
    assert_eq!(game.bgm_report().outstanding, 0);
    assert!(
        !game
            .observe_completion(Some(tail), Some(output(3_200_000_000)))
            .unwrap()
    );
    assert!(
        !game
            .observe_completion(Some(tail), Some(output(3_200_000_000)))
            .unwrap()
    );
    let idle = remote.render(&mut [0.0]).unwrap();
    assert!(!game.observe_completion(Some(idle), None).unwrap());
    assert!(
        !game
            .observe_completion(Some(idle), Some(output(3_300_000_000 - 1)))
            .unwrap()
    );
    assert!(
        game.observe_completion(Some(idle), Some(output(3_300_000_000)))
            .unwrap()
    );
    assert!(
        game.observe_completion(Some(idle), Some(output(3_300_000_000)))
            .unwrap()
    );

    let later = game
        .advance_to(host(8 * SECOND), &Identity, output(8 * SECOND))
        .unwrap();
    assert!(
        later.judge_events.is_empty()
            && later.hazard_events.is_empty()
            && later.audio_commands.is_empty()
    );
    assert_eq!(later.song_time, ts(SECOND));
    assert_eq!(game.song_time(), ts(SECOND));
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
    assert_eq!(game.judge().state(ObjectId(2)), pending);
    assert_eq!(game.judge().remaining_hazards(), 1);
    assert_eq!(game.score(), &score);
    assert_eq!(game.gauge(), &gauge);
    assert!(!game.gauge().can_clear());
    assert!(!game.failed());
    game.fail(); // Export still uses the existing explicit owner-stop contract.
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), replay_limits()).unwrap();
    assert_eq!(file.records.len(), 2);
    assert_eq!(
        (file.records[0].song_time, file.records[1].song_time),
        (ts(0), ts(SECOND))
    );
    let ReplayOperation::Input(input) = &file.records[0].operation else {
        panic!("first record must be actual input");
    };
    assert_eq!(input.physical.meta().source.0, u64::MAX);
    let source = beatkernel_bms::parse(TEXT, Default::default()).unwrap();
    let rebuilt = reconstruct_section(&source, file, replay_limits()).unwrap();
    assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
    assert_eq!(rebuilt.engine().remaining_hazards(), 1);
    assert!(matches!(
        game.observe_completion(Some(idle), None),
        Err(StepGameplayError::Failed)
    ));
}

#[test]
fn solo_finite_failure_still_needs_exact_endpoint_execution_and_recoverable_zero_stays_live() {
    for fault in 0..3 {
        let (mut game, bank) = failed_owner(Some(4 * SECOND));
        let hash = game.judge().stable_hash().unwrap();
        let (mut producer, mut remote) = mixer(bank, 8, Some(40));
        if fault == 2 {
            let batch = game.take_commands(16).unwrap().unwrap();
            for &command in &batch.commands[..2] {
                producer.try_push(command).unwrap();
            }
            assert!(matches!(
                game.acknowledge(batch.sequence, 2, false),
                Err(StepGameplayError::AudioRejected { admitted: 2, .. })
            ));
            let raw = remote.render(&mut [0.0; 40]).unwrap();
            assert_eq!(game.acknowledged_stop_commands(), 1);
            assert!(matches!(
                game.observe_completion(Some(raw), Some(output(4 * SECOND))),
                Err(StepGameplayError::Failed)
            ));
            assert_eq!(game.judge().stable_hash().unwrap(), hash);
            assert_eq!(game.gameplay_fence(), Some(ts(SECOND)));
            continue;
        }
        admit(&mut game, &mut producer);
        let first = remote.render(&mut [0.0; 20]).unwrap();
        assert!(
            !game
                .observe_completion(Some(first), Some(output(2 * SECOND)))
                .unwrap()
        );
        game.feed_audio(20, 8).unwrap();
        let music = game.take_commands(16).unwrap().unwrap();
        assert_eq!(music.commands, [play(2, 90, 3 * SECOND, 1.0)]);
        producer.try_push(music.commands[0]).unwrap();
        game.acknowledge(music.sequence, 1, true).unwrap();
        let mut pcm = [0.0; 20];
        let mut end = remote.render(&mut pcm).unwrap();
        let mut expected = [0.0; 20];
        expected[10] = 0.25;
        expected[11] = 0.75;
        assert_eq!(pcm, expected);
        assert_eq!(end.playback_end_physical_frame, Some(40));
        assert_eq!(
            (
                end.counters.commands_consumed,
                end.counters.commands_applied
            ),
            (5, 5)
        );
        if fault == 0 {
            assert!(
                !game
                    .observe_completion(Some(end), Some(output(4 * SECOND - 1)))
                    .unwrap()
            );
        }
        game.feed_audio(40, 8).unwrap();
        if fault == 1 {
            end.counters.commands_consumed = 6;
            end.counters.commands_applied = 6;
            rejected_output(
                game.observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap_err(),
                end,
                Some(output(4 * SECOND)),
            );
        } else {
            assert!(
                !game
                    .observe_completion(Some(end), Some(output(4 * SECOND - 1)))
                    .unwrap()
            );
            assert!(
                game.observe_completion(Some(end), Some(output(4 * SECOND)))
                    .unwrap()
            );
            assert_eq!(
                game.song_time(),
                ts(SECOND),
                "finite readiness must not advance the failed judge to end"
            );
        }
        assert_eq!(game.judge().stable_hash().unwrap(), hash);
        assert_eq!(game.judge().remaining_hazards(), 1);
        assert_eq!(
            game.gauge().snapshot().failure,
            Some(GaugeFailure::InstantDeath)
        );
    }

    let text = TEXT
        .replace("00ZZ0001", "001E0001")
        .replace("\n#00001:00000002", "");
    let (mut game, bank) = owner(&text, None);
    game.process_input(
        input(u64::MAX, 0, 1, ButtonState::Down),
        &Identity,
        output(0),
    )
    .unwrap();
    game.advance_to(host(SECOND), &Identity, output(SECOND))
        .unwrap();
    assert_eq!(game.gauge().snapshot().level_units, 0);
    assert_eq!(game.gauge().snapshot().failure, None);
    assert_eq!(game.gameplay_fence(), None);
    let batch = game.take_commands(16).unwrap().unwrap();
    assert_eq!(batch.commands, [play(1, 11, 0, 1.0)]);
    let (mut producer, mut remote) = mixer(bank, 8, None);
    producer.try_push(batch.commands[0]).unwrap();
    game.acknowledge(batch.sequence, 1, true).unwrap();
    let first = remote.render(&mut [0.0; 40]).unwrap();
    assert!(
        !game
            .observe_completion(Some(first), Some(output(4 * SECOND)))
            .unwrap()
    );
    let idle = remote.render(&mut [0.0]).unwrap();
    assert!(
        !game
            .observe_completion(Some(idle), Some(output(4_100_000_000)))
            .unwrap()
    );
    assert_eq!(game.judge().remaining_hazards(), 1);
    assert!(!game.failed());
    // The existing ACK suite also retains its ordinary no-mine finite completion case.
}
