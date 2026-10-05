//! Deferred shared remote-output fixtures; member readiness and actual device
//! presentation are separate from acknowledgement of scheduled Stops.
use crate::{
    gauge::GaugeFailure,
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    mine_audio_consumers_fixtures::data,
    replay_audio::{completed_render_cursor, ReplayAudioError},
    step_gameplay::{StepAudioBatch, StepGameplayError, StepLocalGameplay, StepLocalGameplayError},
    step_solo_stop_ack_fixtures::{
        bindings, config, host, input, mixer, output, play, rejected_output, stop, ts, Identity,
        SECOND,
    },
};
use beatkernel::{
    audio::{PcmLimits, PcmSample, QueuePushError, SampleBank, SampleId},
    input::{ButtonState, DeviceId, DeviceSelector},
    judge::HazardOutcome,
    time::ClockDomainId,
};
use beatkernel_bms::BmsInputMode;

const FAILED: PlayerId = PlayerId(7);
const HEALTHY: PlayerId = PlayerId(u32::MAX);
const SOURCES: [u64; 2] = [u64::MAX, u64::MAX - 11];
const TEXT: &str =
    "#BPM 60\n#VOLWAV 50\n#WAV01 note\n#WAV02 music\n#00011:01000100\n#000D1:00ZZ0000\n#00001:02";

fn owner() -> (StepLocalGameplay, SampleBank) {
    let mut prepared = data(TEXT, false);
    // Keep one real bank for both members. Music deliberately outlives the
    // failed member and the healthy member's second note.
    let format = prepared.bank.format();
    let limits = PcmLimits::new(256, 2048, 8).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0, -1.0], limits).unwrap(),
    )
    .unwrap();
    bank.insert(
        SampleId(2),
        PcmSample::new(format, vec![0.25; 40], limits).unwrap(),
    )
    .unwrap();
    prepared.bank = bank;
    let plan = ResolvedInputPlan::new(vec![
        (FAILED, Some(DeviceId(SOURCES[0]))),
        (HEALTHY, Some(DeviceId(SOURCES[1]))),
    ])
    .unwrap();
    let (mut game, bank) = StepLocalGameplay::new_section(
        prepared,
        config(),
        plan,
        SOURCES
            .iter()
            .map(|&source| bindings(DeviceSelector::Exact(DeviceId(source))))
            .collect(),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    game.activate(host(0)).unwrap();
    (game, bank)
}
fn fatal_owner() -> (StepLocalGameplay, SampleBank, StepAudioBatch) {
    let (mut game, bank) = owner();
    assert_eq!(game.players(), [FAILED, HEALTHY]);
    assert_eq!(game.feed_audio(0, 8).unwrap().admitted, 1);
    for (index, player, voice) in [(0usize, FAILED, 91u64), (1, HEALTHY, 93)] {
        let actual = game
            .process_input(
                input(SOURCES[index], 0, index as u64 + 1, ButtonState::Down),
                &Identity,
                output(0),
            )
            .unwrap();
        let InputResult::Processed(reports) = actual else {
            panic!("assigned source was not processed");
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, player);
        assert_eq!(reports[0].report.audio_commands, [play(1, voice, 0, 0.5)]);
    }
    game.process_input(
        input(SOURCES[1], 500_000_000, 3, ButtonState::Up),
        &Identity,
        output(500_000_000),
    )
    .unwrap();
    let reports = game
        .advance_to(host(SECOND), &Identity, output(SECOND))
        .unwrap();
    assert_eq!(
        reports.iter().map(|row| row.player).collect::<Vec<_>>(),
        [FAILED, HEALTHY]
    );
    assert_eq!(
        reports[0].report.hazard_events[0].outcome,
        HazardOutcome::Triggered
    );
    assert_eq!(reports[0].report.audio_commands, [stop(91), stop(92)]);
    assert_eq!(
        reports[1].report.hazard_events[0].outcome,
        HazardOutcome::Avoided
    );
    assert!(reports[1].report.audio_commands.is_empty());
    assert!(
        reports
            .iter()
            .all(|row| row.report.audio_failures.is_empty())
    );
    assert_eq!(game.gameplay_fence(FAILED), Some(ts(SECOND)));
    assert_eq!(game.gameplay_fence(HEALTHY), None);
    assert_eq!(
        game.gauge(FAILED).unwrap().snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    assert_eq!(game.gauge(HEALTHY).unwrap().snapshot().failure, None);
    assert_eq!(game.acknowledged_stop_commands(), 0);
    let batch = game.take_commands(16).unwrap().unwrap();
    assert_eq!(
        batch.commands,
        [
            play(2, 90, 0, 0.5),
            play(1, 91, 0, 0.5),
            play(1, 93, 0, 0.5),
            stop(91),
            stop(92)
        ]
    );
    assert_eq!(game.acknowledged_stop_commands(), 0);
    (game, bank, batch)
}

#[test]
fn local_shared_stop_ack_allows_real_output_while_survivor_and_bgm_remain_independent() {
    let (mut unacknowledged, bank, batch) = fatal_owner();
    let (mut producer, mut output_mixer) = mixer(bank, 8, None);
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    let raw = output_mixer.render(&mut [0.0; 20]).unwrap();
    assert_eq!(
        (raw.counters.commands_applied, raw.counters.unknown_stops),
        (5, 2)
    );
    match unacknowledged
        .observe_completion(Some(raw), Some(output(2 * SECOND)))
        .unwrap_err()
    {
        StepLocalGameplayError::Control(error) => {
            rejected_output(error, raw, Some(output(2 * SECOND)))
        }
        error => panic!("shared output error lost raw evidence: {error:?}"),
    }
    assert_eq!(unacknowledged.acknowledged_stop_commands(), 0);

    let (mut game, bank, batch) = fatal_owner();
    let failed_hash = game.judge(FAILED).unwrap().stable_hash().unwrap();
    let (mut producer, mut output_mixer) = mixer(bank, 8, None);
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    assert_eq!(
        game.acknowledged_stop_commands(),
        0,
        "shared queue admission still needs its matching ACK"
    );
    game.acknowledge(batch.sequence, 5, true).unwrap();
    assert_eq!(game.acknowledged_stop_commands(), 2);
    let mut pcm = [0.0; 20];
    let first = output_mixer.render(&mut pcm).unwrap();
    let mut expected = [0.125; 20];
    expected[0] = 1.0;
    expected[1] = -0.875;
    assert_eq!(pcm, expected);
    assert_eq!(
        (
            first.counters.commands_consumed,
            first.counters.commands_applied,
            first.counters.unknown_stops
        ),
        (5, 5, 2)
    );
    assert_eq!(
        first.active_voices, 1,
        "the shared BGM voice is not a member Stop target"
    );
    assert!(
        matches!(completed_render_cursor(&first), Err(ReplayAudioError::RejectedRender(actual)) if actual == first)
    );
    assert!(
        !game
            .observe_completion(Some(first), Some(output(2 * SECOND)))
            .unwrap()
    );
    game.feed_audio(20, 8).unwrap();

    let InputResult::Processed(frozen) = game
        .process_input(
            input(SOURCES[0], 2 * SECOND, 4, ButtonState::Up),
            &Identity,
            output(2 * SECOND),
        )
        .unwrap()
    else {
        panic!("failed owner acquisition disappeared");
    };
    assert_eq!(frozen[0].player, FAILED);
    assert!(frozen[0].report.bound_inputs.is_empty() && frozen[0].report.audio_commands.is_empty());
    let InputResult::Processed(survivor) = game
        .process_input(
            input(SOURCES[1], 2 * SECOND, 5, ButtonState::Down),
            &Identity,
            output(2 * SECOND),
        )
        .unwrap()
    else {
        panic!("survivor acquisition disappeared");
    };
    assert_eq!(survivor[0].player, HEALTHY);
    assert_eq!(
        survivor[0].report.audio_commands,
        [play(1, 94, 2 * SECOND, 0.5)]
    );
    let later = game.take_commands(16).unwrap().unwrap();
    assert_eq!(later.commands, [play(1, 94, 2 * SECOND, 0.5)]);
    producer.try_push(later.commands[0]).unwrap();
    game.acknowledge(later.sequence, 1, true).unwrap();
    assert_eq!(
        game.acknowledged_stop_commands(),
        2,
        "a healthy Play adds no Stop credit"
    );
    let mut pcm = [0.0; 5];
    let second = output_mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.625, -0.375, 0.125, 0.125, 0.125]);
    assert_eq!(
        (
            second.counters.commands_applied,
            second.counters.unknown_stops
        ),
        (6, 2)
    );
    assert!(
        !game
            .observe_completion(Some(second), Some(output(2_500_000_000)))
            .unwrap()
    );
    assert_eq!(
        game.judge(FAILED).unwrap().stable_hash().unwrap(),
        failed_hash
    );
    assert_eq!(
        (
            game.score(FAILED).unwrap().hits,
            game.score(HEALTHY).unwrap().hits
        ),
        (1, 2)
    );
    assert_eq!(game.member_song_time(FAILED), Some(ts(SECOND)));
    assert_eq!(game.member_song_time(HEALTHY), Some(ts(2 * SECOND)));
    assert_eq!(game.song_time(), ts(2 * SECOND));
    assert_eq!(game.gameplay_fence(FAILED), Some(ts(SECOND)));
    assert_eq!(game.gameplay_fence(HEALTHY), None);
    assert!(!game.failed());
    assert!(game.take_commands(16).unwrap().is_none());
}

#[test]
fn local_partial_and_invalid_ack_preserve_original_prefix_and_reject_unowned_output_evidence() {
    for (admitted, stops) in [(3usize, 0u64), (4, 1), (5, 2)] {
        let (mut game, bank, batch) = fatal_owner();
        let progress = game.group_progress().unwrap();
        let hash = game.judge(FAILED).unwrap().stable_hash().unwrap();
        let (mut producer, mut output_mixer) = mixer(bank, admitted, None);
        for &command in &batch.commands[..admitted] {
            producer.try_push(command).unwrap();
        }
        if admitted < 5 {
            let refusal = producer.try_push(batch.commands[admitted]).unwrap_err();
            assert_eq!(refusal.command, batch.commands[admitted]);
            assert_eq!(refusal.reason, QueuePushError::Full);
        }
        match game
            .acknowledge(batch.sequence, admitted, false)
            .unwrap_err()
        {
            StepLocalGameplayError::Control(StepGameplayError::AudioRejected {
                batch: actual,
                admitted: count,
            }) => {
                assert_eq!(actual, batch);
                assert_eq!(count, admitted);
            }
            error => panic!("partial shared ACK lost original prefix: {error:?}"),
        }
        let raw = output_mixer.render(&mut [0.0; 20]).unwrap();
        assert_eq!(raw.counters.commands_applied, admitted as u64);
        assert_eq!(raw.counters.unknown_stops, stops);
        assert_eq!(game.acknowledged_stop_commands(), stops);
        assert_eq!(game.group_progress().unwrap(), progress);
        assert_eq!(game.judge(FAILED).unwrap().stable_hash().unwrap(), hash);
        assert_eq!(game.gameplay_fence(FAILED), Some(ts(SECOND)));
        assert_eq!(game.gameplay_fence(HEALTHY), None);
        assert_eq!(game.gauge(HEALTHY).unwrap().snapshot().failure, None);
        assert!(game.failed());
        assert!(matches!(
            game.acknowledge(batch.sequence, admitted, false),
            Err(StepLocalGameplayError::Control(StepGameplayError::Failed))
        ));
        assert_eq!(game.acknowledged_stop_commands(), stops);
    }

    for case in 0..5 {
        let (mut game, _bank, retained) = if case == 0 {
            let (game, bank) = owner();
            (game, bank, None)
        } else {
            let (game, bank, batch) = fatal_owner();
            (game, bank, Some(batch))
        };
        let sequence = retained.as_ref().map_or(1, |batch| batch.sequence);
        let credited = if case == 4 {
            game.acknowledge(sequence, 5, true).unwrap();
            2
        } else {
            0
        };
        let (received, count) = match case {
            1 => (sequence + 1, 5),
            2 => (sequence, 6),
            3 => (sequence, 4),
            _ => (sequence, 5),
        };
        match game.acknowledge(received, count, true).unwrap_err() {
            StepLocalGameplayError::Control(StepGameplayError::InvalidAcknowledgement {
                sequence,
                admitted,
                success,
                batch,
            }) => {
                assert_eq!((sequence, admitted, success), (received, count, true));
                assert_eq!(batch, if case == 4 { None } else { retained });
            }
            error => panic!("invalid shared ACK lost original batch: {error:?}"),
        }
        assert_eq!(game.acknowledged_stop_commands(), credited);
        assert!(game.failed());
    }

    for case in 0..6 {
        let (mut game, bank, batch) = fatal_owner();
        let (mut producer, mut output_mixer) = mixer(bank, 8, None);
        for &command in &batch.commands {
            producer.try_push(command).unwrap();
        }
        game.acknowledge(batch.sequence, 5, true).unwrap();
        let first = output_mixer.render(&mut [0.0; 5]).unwrap();
        assert!(
            !game
                .observe_completion(Some(first), Some(output(500_000_000)))
                .unwrap()
        );
        let mut bad = output_mixer.render(&mut [0.0; 15]).unwrap();
        let mut presented = output(2 * SECOND);
        match case {
            0 => bad.counters.unknown_stops = 3,
            1 => bad.counters.commands_applied = 1,
            2 => bad.counters.unknown_samples = 1,
            3 => {
                bad = first;
                bad.active_voices += 1;
            }
            4 => presented.domain = ClockDomainId(99),
            5 => bad.playback_start_frame += 1,
            _ => unreachable!(),
        }
        let progress = game.group_progress().unwrap();
        let song = game.song_time();
        let feed = game.bgm_report();
        let hashes = [
            game.judge(FAILED).unwrap().stable_hash().unwrap(),
            game.judge(HEALTHY).unwrap().stable_hash().unwrap(),
        ];
        let gauges = [
            game.gauge(FAILED).unwrap().clone(),
            game.gauge(HEALTHY).unwrap().clone(),
        ];
        match game
            .observe_completion(Some(bad), Some(presented))
            .unwrap_err()
        {
            StepLocalGameplayError::Control(error) => rejected_output(error, bad, Some(presented)),
            error => panic!("invalid shared output lost raw evidence: {error:?}"),
        }
        assert_eq!(game.group_progress().unwrap(), progress);
        assert_eq!(game.song_time(), song);
        assert_eq!(game.bgm_report(), feed);
        for (index, player) in [FAILED, HEALTHY].into_iter().enumerate() {
            assert_eq!(
                game.judge(player).unwrap().stable_hash().unwrap(),
                hashes[index]
            );
            assert_eq!(game.gauge(player).unwrap(), &gauges[index]);
        }
        assert_eq!(game.acknowledged_stop_commands(), 2);
        assert!(game.failed());
    }
}
