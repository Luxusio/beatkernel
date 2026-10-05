//! Deferred actual capture, ACK, queue and Mixer fixtures. Presentation is supplied
//! as evidence; none of these source fixtures claims physical device completion.
use crate::{
    gauge::GaugeFailure,
    mine_audio_consumers_fixtures::{data, recorded, replay_limits, Action},
    replay_audio::{completed_render_cursor, plan_section_audio, ReplayAudioError},
    replay_playback::reconstruct_section,
    step_gameplay::{validate_section_output_evidence, StepAudioBatch, StepGameplayError},
    step_replay::{StepReplay, StepReplayConfig, StepReplayError},
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioLimits, CommandProducer, Mixer, MixerConfig,
        QueuePushError, RenderReport, SampleBank, SampleId, VoiceId,
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::BmsInputMode;

const SECOND: i64 = 1_000_000_000;
const OUTPUT: i64 = SECOND;
const TEXT: &str =
    "#BPM 60\n#WAV01 note\n#WAV02 press\n#00011:01000100\n#00031:02\n#000D1:00ZZ0000";
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn output(relative: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(2),
        timestamp: ts(OUTPUT + relative),
    }
}
fn config() -> StepReplayConfig {
    StepReplayConfig {
        output_origin: output(0),
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(3 * SECOND),
        max_pending: 8,
    }
}
fn play(voice: u64, relative: i64) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(1),
        voice: VoiceId(voice),
        at: ts(OUTPUT + relative),
        gain: 1.0,
    }
}
fn stop(voice: u64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(OUTPUT + SECOND),
    }
}
fn owner(text: &str, end: Option<i64>) -> (StepReplay, SampleBank) {
    let prepared = data(text, false);
    let actual = recorded(
        &prepared,
        &[
            Action::Press(0, 91),
            Action::Advance(SECOND),
            Action::Release(SECOND, 91),
            Action::Press(2 * SECOND, 91),
            Action::Release(2 * SECOND, 91),
            Action::Advance(2_100_000_000),
        ],
        BmsInputMode::ButtonOnly,
        0,
        end,
        0,
        0,
    );
    assert_eq!(
        actual
            .reports
            .iter()
            .map(|report| report.judge_events.len())
            .sum::<usize>(),
        2
    );
    // The legacy recording really includes the post-fatal hit. Reconstruction must
    // retain it even though the audio plan and failed visual gauge stop progressing.
    let rebuilt =
        reconstruct_section(&prepared.source, actual.file.clone(), replay_limits()).unwrap();
    assert_eq!(rebuilt.engine().stable_hash().unwrap(), actual.hash);
    let plan = plan_section_audio(
        &prepared,
        actual.file.clone(),
        replay_limits(),
        output(0),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(plan.final_judge_hash, actual.hash);
    assert_eq!(plan.judge_events.len(), 2);
    StepReplay::new(prepared, actual.file, replay_limits(), config()).unwrap()
}
fn mixer(bank: SampleBank, capacity: usize, end: Option<u64>) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut config = MixerConfig::new(
        bank.format(),
        ClockDomainId(2),
        ts(OUTPUT),
        AudioLimits::new(capacity, 8, 16, 64, capacity).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn batch(owner: &mut StepReplay) -> StepAudioBatch {
    let batch = owner.take_commands(8).unwrap().unwrap();
    assert_eq!(batch.commands, [play(11, 0), stop(11), stop(12), stop(13)]);
    batch
}
fn admit(owner: &mut StepReplay, producer: &mut CommandProducer) {
    let batch = batch(owner);
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    owner.acknowledge(batch.sequence, 4, true).unwrap();
    assert_eq!(owner.acknowledged_stop_commands(), 3);
}
fn rejected_raw(error: StepReplayError, expected: RenderReport, presented: Option<ClockPoint>) {
    match error {
        StepReplayError::Output {
            rendered: Some(actual),
            presented: point,
            ..
        }
        | StepReplayError::Audio {
            rendered: actual,
            presented: point,
            ..
        } => {
            assert_eq!(actual, expected);
            assert_eq!(point, presented);
        }
        error => panic!("missing original output evidence: {error:?}"),
    }
}

#[test]
fn actual_acknowledged_stop_prefix_is_distinct_from_planning_and_invalid_ack_earns_no_credit() {
    let (mut replay, bank) = owner(TEXT, None);
    assert_eq!(replay.acknowledged_stop_commands(), 0);
    let retained = batch(&mut replay);
    assert_eq!(replay.bgm_report().total_admitted, 4);
    assert_eq!(
        replay.acknowledged_stop_commands(),
        0,
        "feeder callback is only outbound planning"
    );
    assert!(
        matches!(replay.take_commands(8), Err(StepReplayError::OutstandingBatch { sequence }) if sequence == retained.sequence)
    );
    let (mut producer, _mixer) = mixer(bank, 8, None);
    for &command in &retained.commands {
        producer.try_push(command).unwrap();
    }
    assert_eq!(
        replay.acknowledged_stop_commands(),
        0,
        "the owner still needs its matching ACK"
    );
    replay.acknowledge(retained.sequence, 4, true).unwrap();
    assert_eq!(replay.acknowledged_stop_commands(), 3);

    for (admitted, stops) in [(1usize, 0u64), (2, 1), (4, 3)] {
        let (mut replay, bank) = owner(TEXT, None);
        let retained = batch(&mut replay);
        let (mut producer, _mixer) = mixer(bank, admitted, None);
        for &command in &retained.commands[..admitted] {
            producer.try_push(command).unwrap();
        }
        if admitted < 4 {
            let error = producer.try_push(retained.commands[admitted]).unwrap_err();
            assert_eq!(error.command, retained.commands[admitted]);
            assert_eq!(error.reason, QueuePushError::Full);
        }
        match replay
            .acknowledge(retained.sequence, admitted, false)
            .unwrap_err()
        {
            StepReplayError::Acknowledgement(StepGameplayError::AudioRejected {
                batch,
                admitted: count,
            }) => {
                assert_eq!(batch, retained);
                assert_eq!(count, admitted);
            }
            error => panic!("lost valid partial ACK: {error:?}"),
        }
        assert!(replay.failed());
        assert_eq!(replay.acknowledged_stop_commands(), stops);
        assert!(matches!(
            replay.acknowledge(retained.sequence, 4, true),
            Err(StepReplayError::Failed)
        ));
        assert!(matches!(
            replay.take_commands(8),
            Err(StepReplayError::Failed)
        ));
        assert_eq!(
            replay.acknowledged_stop_commands(),
            stops,
            "the rejected suffix never gains credit"
        );
    }
    for case in 0..5 {
        let (mut replay, bank) = owner(TEXT, None);
        let retained = if case == 0 {
            None
        } else {
            Some(batch(&mut replay))
        };
        let sequence = retained.as_ref().map_or(1, |batch| batch.sequence);
        let mut credited = 0;
        if case == 4 {
            let (mut producer, _mixer) = mixer(bank, 8, None);
            for &command in &retained.as_ref().unwrap().commands {
                producer.try_push(command).unwrap();
            }
            replay.acknowledge(sequence, 4, true).unwrap();
            credited = 3;
        }
        let (received, count) = match case {
            1 => (sequence + 1, 4),
            2 => (sequence, 5),
            3 => (sequence, 3),
            _ => (sequence, 4),
        };
        match replay.acknowledge(received, count, true).unwrap_err() {
            StepReplayError::Acknowledgement(StepGameplayError::InvalidAcknowledgement {
                sequence,
                admitted,
                success,
                batch,
            }) => {
                assert_eq!((sequence, admitted, success), (received, count, true));
                assert_eq!(batch, if case == 4 { None } else { retained });
            }
            error => panic!("invalid ACK changed its error: {error:?}"),
        }
        assert_eq!(replay.acknowledged_stop_commands(), credited);
        assert!(replay.failed());
    }
}

#[test]
fn inactive_stop_output_needs_ack_and_unlimited_finish_needs_a_later_idle_presented_block() {
    let (mut unacknowledged, bank) = owner(TEXT, None);
    let outbound = batch(&mut unacknowledged);
    let (mut producer, mut output_mixer) = mixer(bank, 8, None);
    for &command in &outbound.commands {
        producer.try_push(command).unwrap();
    }
    let raw = output_mixer.render(&mut [0.0; 22]).unwrap();
    assert_eq!(
        (raw.counters.commands_applied, raw.counters.unknown_stops),
        (4, 3)
    );
    let error = unacknowledged
        .observe_output(Some(raw), Some(output(2_200_000_000)))
        .unwrap_err();
    rejected_raw(error, raw, Some(output(2_200_000_000)));
    assert_eq!(unacknowledged.acknowledged_stop_commands(), 0);
    assert_eq!(unacknowledged.song_time(), ts(0));
    assert_eq!(unacknowledged.score().hits, 0);
    assert!(unacknowledged.drain_events().is_empty());

    let (mut replay, bank) = owner(TEXT, None);
    let (mut producer, mut output_mixer) = mixer(bank, 8, None);
    admit(&mut replay, &mut producer);
    let mut pcm = [0.0; 22];
    let first = output_mixer.render(&mut pcm).unwrap();
    let mut expected = [0.0; 22];
    expected[0] = 1.0;
    expected[1] = -1.0;
    assert_eq!(pcm, expected);
    assert!(
        matches!(completed_render_cursor(&first), Err(ReplayAudioError::RejectedRender(actual)) if actual == first)
    );
    assert!(
        validate_section_output_evidence(
            output(0),
            10,
            None,
            None,
            Some(first),
            Some(output(2_200_000_000)),
            None
        )
        .is_err(),
        "generic validators remain strict"
    );
    assert!(
        !replay
            .observe_output(Some(first), Some(output(2_200_000_000)))
            .unwrap()
    );
    assert_eq!((replay.score().hits, replay.score().misses), (2, 0));
    assert_eq!(
        replay.drain_events().len(),
        2,
        "post-fatal legacy judging remains in the visual prefix"
    );
    assert_eq!(
        replay.gauge().snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    assert_eq!(replay.recorded_until(), Some(ts(2_100_000_000)));
    assert!(replay.take_commands(8).unwrap().is_none()); // Retire rendered feeder credits.
    assert!(
        !replay
            .observe_output(Some(first), Some(output(2_200_000_000)))
            .unwrap()
    );
    assert!(!replay.observe_output(None, None).unwrap());
    let idle = output_mixer.render(&mut [0.0]).unwrap();
    assert_eq!(idle.counters.unknown_stops, 3);
    assert!(!replay.observe_output(Some(idle), None).unwrap());
    assert!(
        !replay
            .observe_output(Some(idle), Some(output(2_299_999_999)))
            .unwrap()
    );
    assert!(
        replay
            .observe_output(Some(idle), Some(output(2_300_000_000)))
            .unwrap()
    );
    assert!(replay.drain_events().is_empty());
    assert_eq!(replay.acknowledged_stop_commands(), 3);
    assert!(
        !replay.observe_output(None, None).unwrap(),
        "an ACK does not manufacture a new output observation"
    );
}

#[test]
fn invalid_ack_bounded_render_evidence_cannot_mutate_visual_or_feeder_frontiers() {
    for case in 0..10 {
        let (mut replay, bank) = owner(TEXT, None);
        let (mut producer, mut output_mixer) = mixer(bank, 8, None);
        admit(&mut replay, &mut producer);
        let first = output_mixer.render(&mut [0.0; 5]).unwrap();
        assert!(
            !replay
                .observe_output(Some(first), Some(output(500_000_000)))
                .unwrap()
        );
        assert_eq!(replay.drain_events().len(), 1);
        let next = output_mixer.render(&mut [0.0; 17]).unwrap();
        let mut bad = next;
        let mut presented = output(2_200_000_000);
        match case {
            0 => bad.counters.unknown_stops = 4, // More than the three acknowledged Stops.
            1 => bad.counters.commands_applied = 2, // Unknown count fits ACKs but exceeds execution.
            2 => bad.counters.unknown_samples = 1,
            3 => bad.counters.late_commands = 1,
            4 => {
                bad = first;
                bad.active_voices = 1;
            }
            5 => {
                replay.observe_output(Some(next), Some(presented)).unwrap();
                assert_eq!(replay.drain_events().len(), 1);
                bad = output_mixer.render(&mut [0.0]).unwrap();
                bad.counters.unknown_stops = 2; // A formerly admitted cumulative count regressed.
                presented = output(2_300_000_000);
            }
            6 => presented.domain = ClockDomainId(99),
            7 => bad.playback_start_frame += 1,
            8 => {
                bad.start_frame = 4;
                bad.playback_start_frame = 4;
                bad.counters.rendered_frames = 21;
            }
            9 => presented = output(499_999_999),
            _ => unreachable!(),
        }
        let song = replay.song_time();
        let score = replay.score().clone();
        let gauge = replay.gauge().clone();
        let mines = *replay.mine_damage();
        let feed = replay.bgm_report();
        let error = replay
            .observe_output(Some(bad), Some(presented))
            .unwrap_err();
        rejected_raw(error, bad, Some(presented));
        assert_eq!(replay.song_time(), song, "case {case}");
        assert_eq!(replay.score(), &score);
        assert_eq!(replay.gauge(), &gauge);
        assert_eq!(*replay.mine_damage(), mines);
        assert_eq!(replay.bgm_report(), feed);
        assert_eq!(replay.acknowledged_stop_commands(), 3);
        assert!(replay.drain_events().is_empty());
        assert!(replay.failed());
    }
}

#[test]
fn finite_endpoint_requires_full_execution_retired_credits_and_presentation_with_legacy_unchanged()
{
    for mines in [true, false] {
        let text = if mines {
            TEXT.to_owned()
        } else {
            TEXT.replace("\n#000D1:00ZZ0000", "")
        };
        let (mut replay, bank) = owner(&text, Some(3 * SECOND));
        assert_eq!(replay.playback_end_frame(), Some(30));
        let (mut producer, mut output_mixer) = mixer(bank, 8, Some(30));
        let outbound = replay.take_commands(8).unwrap().unwrap();
        if mines {
            assert_eq!(
                outbound.commands,
                [play(11, 0), stop(11), stop(12), stop(13)]
            );
        } else {
            assert_eq!(outbound.commands, [play(11, 0), play(12, 2 * SECOND)]);
        }
        for &command in &outbound.commands {
            producer.try_push(command).unwrap();
        }
        replay
            .acknowledge(outbound.sequence, outbound.commands.len(), true)
            .unwrap();
        assert_eq!(
            replay.acknowledged_stop_commands(),
            if mines { 3 } else { 0 }
        );
        let mut pcm = [0.0; 29];
        let before = output_mixer.render(&mut pcm).unwrap();
        let mut expected = [0.0; 29];
        expected[0] = 1.0;
        expected[1] = -1.0;
        if !mines {
            expected[20] = 1.0;
            expected[21] = -1.0;
        }
        assert_eq!(pcm, expected);
        assert!(
            !replay
                .observe_output(Some(before), Some(output(2_900_000_000)))
                .unwrap()
        );
        assert_eq!(replay.drain_events().len(), 2);
        assert_eq!((replay.score().hits, replay.score().misses), (2, 0));
        assert!(replay.take_commands(8).unwrap().is_none());
        let marker = output_mixer.render(&mut [0.0; 3]).unwrap();
        assert_eq!(marker.playback_end_physical_frame, Some(30));
        assert_eq!(marker.playback_frames, 1);
        assert_eq!(marker.counters.commands_consumed, if mines { 4 } else { 2 });
        assert_eq!(marker.counters.commands_applied, if mines { 4 } else { 2 });
        assert_eq!(marker.counters.unknown_stops, if mines { 3 } else { 0 });
        assert!(!replay.observe_output(Some(marker), None).unwrap());
        assert!(
            !replay
                .observe_output(None, Some(output(2_999_999_999)))
                .unwrap()
        );
        assert!(
            replay
                .observe_output(None, Some(output(3 * SECOND)))
                .unwrap()
        );
        assert_eq!(replay.song_time(), ts(3 * SECOND));
        assert!(replay.drain_events().is_empty());
    }
    let (mut replay, bank) = owner(TEXT, Some(3 * SECOND));
    let (mut producer, mut output_mixer) = mixer(bank, 8, Some(30));
    admit(&mut replay, &mut producer);
    let before = output_mixer.render(&mut [0.0; 29]).unwrap();
    replay
        .observe_output(Some(before), Some(output(2_900_000_000)))
        .unwrap();
    replay.drain_events();
    assert!(replay.take_commands(8).unwrap().is_none());
    let mut bad = output_mixer.render(&mut [0.0; 3]).unwrap();
    // Monotonic, internally ordered counters still cannot add an unplanned fifth command.
    bad.counters.commands_consumed = 5;
    bad.counters.commands_applied = 5;
    let old_song = replay.song_time();
    let old_score = replay.score().clone();
    let error = replay
        .observe_output(Some(bad), Some(output(3 * SECOND)))
        .unwrap_err();
    rejected_raw(error, bad, Some(output(3 * SECOND)));
    assert_eq!(replay.song_time(), old_song);
    assert_eq!(replay.score(), &old_score);
    assert!(replay.drain_events().is_empty());
    assert_eq!(replay.acknowledged_stop_commands(), 3);
}
