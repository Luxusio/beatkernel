//! Deferred result admission through actual stepped queue/ACK/Mixer completion.
use crate::{
    gauge::GaugeFailure,
    mine_audio_consumers_fixtures::{data, replay_limits},
    play_result::{PlayResultOutcome, PlayResultScope},
    replay_playback::reconstruct_section,
    section_start::prepare_at,
    step_gameplay::{StepGameplay, StepGameplayError},
    step_solo_stop_ack_fixtures::{
        bindings, config, host, input, mixer, output, owner, play, rejected_output, stop, ts,
        Identity, SECOND,
    },
};
use beatkernel::{
    audio::{CommandProducer, PcmLimits, SampleBank},
    chart::ObjectId,
    input::{ButtonState, DeviceSelector},
    interaction::InteractionState,
    replay::codec::decode_replay,
};
const TEXT: &str = "#BPM 60\n#WAV01 note\n#WAV02 press\n#00011:01000100\n#00031:02\n#000D1:00ZZ0001\n#00001:00000002";
fn fatal(end: Option<i64>) -> (StepGameplay, SampleBank) {
    let (mut game, bank) = owner(TEXT, end);
    assert!(game.completed_result().is_none());
    game.configure_capture(replay_limits(), 0).unwrap();
    game.process_input(
        input(u64::MAX, 0, 1, ButtonState::Down),
        &Identity,
        output(0),
    )
    .unwrap();
    game.advance_to(host(SECOND), &Identity, output(SECOND))
        .unwrap();
    assert_eq!(game.gameplay_fence(), Some(ts(SECOND)));
    assert!(
        game.completed_result().is_none(),
        "a numeric failure is not output completion"
    );
    (game, bank)
}
fn send(game: &mut StepGameplay, producer: &mut CommandProducer) {
    let batch = game.take_commands(16).unwrap().unwrap();
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
}

#[test]
fn solo_result_waits_for_future_music_ack_later_idle_and_presentation_then_preserves_history() {
    let (mut game, bank) = fatal(None);
    let hash = game.judge().stable_hash().unwrap();
    let score = game.score().clone();
    let gauge = game.gauge().clone();
    let (mut producer, mut remote) = mixer(bank, 8, None);
    let batch = game.take_commands(16).unwrap().unwrap();
    assert_eq!(
        batch.commands,
        [play(1, 11, 0, 1.0), stop(11), stop(12), stop(91)]
    );
    assert!(!game.observe_completion(None, None).unwrap());
    assert!(game.completed_result().is_none());
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    game.acknowledge(batch.sequence, 4, true).unwrap();
    assert_eq!(game.acknowledged_stop_commands(), 3);
    assert!(game.completed_result().is_none());
    let first = remote.render(&mut [0.0; 20]).unwrap();
    assert!(
        !game
            .observe_completion(Some(first), Some(output(2 * SECOND)))
            .unwrap()
    );
    assert!(game.completed_result().is_none());
    game.feed_audio(20, 8).unwrap();
    let music = game.take_commands(16).unwrap().unwrap();
    assert_eq!(music.commands, [play(2, 90, 3 * SECOND, 1.0)]);
    producer.try_push(music.commands[0]).unwrap();
    let before_music = remote.render(&mut [0.0; 10]).unwrap();
    assert!(
        !game
            .observe_completion(Some(before_music), Some(output(3 * SECOND)))
            .unwrap()
    );
    assert!(
        game.completed_result().is_none(),
        "pending remote receipt cannot become a result"
    );
    game.acknowledge(music.sequence, 1, true).unwrap();
    let mut pcm = [0.0; 2];
    let tail = remote.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.75]);
    game.feed_audio(32, 8).unwrap();
    assert!(
        !game
            .observe_completion(Some(tail), Some(output(3_200_000_000)))
            .unwrap()
    );
    assert!(game.completed_result().is_none());
    let idle = remote.render(&mut [0.0]).unwrap();
    assert!(!game.observe_completion(Some(idle), None).unwrap());
    assert!(
        !game
            .observe_completion(Some(idle), Some(output(3_300_000_000 - 1)))
            .unwrap()
    );
    assert!(game.completed_result().is_none());
    assert!(
        game.observe_completion(Some(idle), Some(output(3_300_000_000)))
            .unwrap()
    );
    let completed = *game.completed_result().unwrap();
    assert_eq!(completed.scope(), PlayResultScope::FullSong);
    assert_eq!(
        completed.outcome(),
        PlayResultOutcome::Failed(GaugeFailure::InstantDeath)
    );
    assert_eq!(completed.gauge(), *gauge.snapshot());
    assert!(!completed.whole_song_clear());
    assert!(
        game.observe_completion(Some(idle), Some(output(3_300_000_000)))
            .unwrap()
    );
    assert_eq!(game.completed_result(), Some(&completed));
    assert_eq!(game.song_time(), ts(SECOND));
    assert_eq!(game.judge().remaining_hazards(), 1);
    assert_eq!(
        game.judge().state(ObjectId(2)),
        Some(InteractionState::Pending)
    );
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
    assert_eq!(game.score(), &score);
    assert_eq!(game.gauge(), &gauge);

    let mut invalid = idle;
    invalid.counters.unknown_samples = 1;
    rejected_output(
        game.observe_completion(Some(invalid), Some(output(3_300_000_000)))
            .unwrap_err(),
        invalid,
        Some(output(3_300_000_000)),
    );
    assert_eq!(
        game.completed_result(),
        Some(&completed),
        "later output failure cannot erase proven history"
    );
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), replay_limits()).unwrap();
    assert_eq!(file.records.len(), 2);
    assert_eq!(file.records.last().unwrap().song_time, ts(SECOND));
    let source = beatkernel_bms::parse(TEXT, Default::default()).unwrap();
    assert_eq!(
        reconstruct_section(&source, file, replay_limits())
            .unwrap()
            .engine()
            .stable_hash()
            .unwrap(),
        hash
    );
    assert_eq!(game.completed_result(), Some(&completed));
    assert!(
        fatal(None).0.completed_result().is_none(),
        "a fresh owner cannot inherit this result"
    );
}

#[test]
fn finite_or_nonzero_start_results_remain_practice_and_technical_failures_never_create_them() {
    for fault in 0..3 {
        let (mut game, bank) = fatal(Some(4 * SECOND));
        let (mut producer, mut remote) = mixer(bank, 8, Some(40));
        if fault == 1 {
            let batch = game.take_commands(16).unwrap().unwrap();
            for &command in &batch.commands[..2] {
                producer.try_push(command).unwrap();
            }
            assert!(matches!(
                game.acknowledge(batch.sequence, 2, false),
                Err(StepGameplayError::AudioRejected { admitted: 2, .. })
            ));
            let raw = remote.render(&mut [0.0; 40]).unwrap();
            assert!(matches!(
                game.observe_completion(Some(raw), Some(output(4 * SECOND))),
                Err(StepGameplayError::Failed)
            ));
            assert!(game.completed_result().is_none());
            continue;
        }
        send(&mut game, &mut producer);
        let first = remote.render(&mut [0.0; 20]).unwrap();
        assert!(
            !game
                .observe_completion(Some(first), Some(output(2 * SECOND)))
                .unwrap()
        );
        game.feed_audio(20, 8).unwrap();
        send(&mut game, &mut producer);
        let end = remote.render(&mut [0.0; 20]).unwrap();
        assert_eq!(end.playback_end_physical_frame, Some(40));
        game.feed_audio(40, 8).unwrap();
        if fault == 2 {
            let mut bad = end;
            bad.counters.commands_applied += 1;
            rejected_output(
                game.observe_completion(Some(bad), Some(output(4 * SECOND)))
                    .unwrap_err(),
                bad,
                Some(output(4 * SECOND)),
            );
            assert!(game.completed_result().is_none());
            continue;
        }
        assert!(
            !game
                .observe_completion(Some(end), Some(output(4 * SECOND - 1)))
                .unwrap()
        );
        assert!(game.completed_result().is_none());
        assert!(
            game.observe_completion(Some(end), Some(output(4 * SECOND)))
                .unwrap()
        );
        let result = game.completed_result().unwrap();
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
    }
    let selected = prepare_at(
        data("#BPM 60\n#WAV01 note\n#00011:00000100", false),
        ts(2 * SECOND),
        PcmLimits::new(256, 2048, 8).unwrap(),
    )
    .unwrap()
    .0;
    let (mut practice, bank) = StepGameplay::new_section(
        selected,
        config(),
        bindings(DeviceSelector::Any),
        ts(2 * SECOND),
        None,
    )
    .unwrap();
    assert!(practice.completed_result().is_none());
    practice.activate(host(0)).unwrap();
    practice
        .process_input(
            input(u64::MAX, 0, 1, ButtonState::Down),
            &Identity,
            output(0),
        )
        .unwrap();
    practice
        .advance_to(host(SECOND), &Identity, output(SECOND))
        .unwrap();
    let (mut producer, mut remote) = mixer(bank, 8, None);
    send(&mut practice, &mut producer);
    let tail = remote.render(&mut [0.0; 3]).unwrap();
    assert!(
        !practice
            .observe_completion(Some(tail), Some(output(300_000_000)))
            .unwrap()
    );
    let idle = remote.render(&mut [0.0]).unwrap();
    assert!(
        practice
            .observe_completion(Some(idle), Some(output(400_000_000)))
            .unwrap()
    );
    let result = practice.completed_result().unwrap();
    assert_eq!(
        result.scope(),
        PlayResultScope::PracticeSection {
            start: ts(2 * SECOND),
            end: None
        }
    );
    assert_eq!(result.outcome(), PlayResultOutcome::BelowClearThreshold);
    assert_eq!(result.gauge().level_units, 21_000_000);
    assert!(!result.whole_song_clear());
}
