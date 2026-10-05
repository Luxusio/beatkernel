// Deferred native evidence bounds over actual producer admission and software Mixer.
use super::*;
use crate::offline::OwnedStopEvidence;

fn at(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn stop(voice: u64, ns: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: at(ns),
    }
}
fn output(capacity: usize) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            bank().format(),
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(capacity, 2, 8, 16, capacity).unwrap(),
        ),
        bank(),
        consumer,
    )
    .unwrap();
    (producer, mixer)
}

#[test]
fn actual_unknown_stop_requires_owned_admission_and_never_exceeds_executed_commands() {
    let (mut producer, mut mixer) = output(4);
    let mut evidence = OwnedStopEvidence::default();
    let head = AudioCommand::Play {
        sample: SampleId(1),
        voice: VoiceId(u64::MAX),
        at: at(0),
        gain: 1.0,
    };
    producer.try_push(head).unwrap();
    evidence.record_admitted(&[head]).unwrap();
    assert_eq!(evidence.admitted_stops(), 0);
    let silence = stop(u64::MAX, 3_000_000);
    producer.try_push(silence).unwrap();
    evidence.record_admitted(&[silence]).unwrap();
    let mut pcm = [0.0; 4];
    let raw = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.5, 0.0, 0.0]);
    assert_eq!(
        (raw.counters.commands_applied, raw.counters.unknown_stops),
        (2, 1)
    );
    let unowned = validate_stop_evidence(Some(raw), &OwnedStopEvidence::default()).unwrap_err();
    let unowned = unowned.downcast_ref::<NativeStopEvidenceError>().unwrap();
    assert_eq!(unowned.report, raw);
    assert_eq!(unowned.admitted_stops, 0);
    validate_stop_evidence(Some(raw), &evidence).unwrap();
    validate_stop_evidence(Some(raw), &evidence).unwrap();
    validate_stop_evidence(None, &evidence).unwrap();
    assert_eq!(
        evidence.admitted_stops(),
        1,
        "validating/repeating a report earns no new admission"
    );
    assert!(
        crate::replay_audio::completed_render_cursor(&raw).is_err(),
        "the generic validator remains strict"
    );
    for bad in [
        RenderReport {
            counters: AudioCounters {
                unknown_stops: 2,
                ..raw.counters
            },
            ..raw
        },
        RenderReport {
            counters: AudioCounters {
                commands_applied: 0,
                ..raw.counters
            },
            ..raw
        },
    ] {
        let failure = validate_stop_evidence(Some(bad), &evidence).unwrap_err();
        let failure = failure.downcast_ref::<NativeStopEvidenceError>().unwrap();
        assert_eq!(failure.report, bad);
        assert_eq!(failure.admitted_stops, 1);
        assert_eq!(evidence.admitted_stops(), 1);
    }
    let mut barrier = NativeStopBarrier::default();
    assert!(!barrier.observe(&evidence, Some(raw)).unwrap());
    assert!(!barrier.observe(&evidence, Some(raw)).unwrap());
    let idle = mixer.render(&mut [0.0]).unwrap();
    assert!(barrier.observe(&evidence, Some(idle)).unwrap());
    let next = stop(99, 5_000_000);
    producer.try_push(next).unwrap();
    evidence.record_admitted(&[next]).unwrap();
    assert!(!barrier.observe(&evidence, None).unwrap());
    assert!(
        !barrier.observe(&evidence, Some(idle)).unwrap(),
        "newly admitted Stop cannot reuse prior idle evidence"
    );
    let after = mixer.render(&mut [0.0]).unwrap();
    assert!(barrier.observe(&evidence, Some(after)).unwrap());
    assert_eq!(after.counters.unknown_stops, 2);
    validate_stop_evidence(Some(after), &evidence).unwrap();
}

#[test]
fn partial_queue_refusal_credits_only_the_real_stop_prefix_and_preserves_raw_diagnostics() {
    let (mut producer, mut mixer) = output(2);
    let mut evidence = OwnedStopEvidence::default();
    let accepted = [play(0, 1.0), stop(77, 1_000_000)];
    for &command in &accepted {
        producer.try_push(command).unwrap();
        evidence
            .record_admitted(std::slice::from_ref(&command))
            .unwrap();
    }
    let rejected = producer.try_push(stop(88, 1_000_000)).unwrap_err();
    assert_eq!(rejected.command, stop(88, 1_000_000));
    assert_eq!(rejected.reason, QueuePushError::Full);
    assert_eq!(evidence.admitted_stops(), 1);
    let raw = mixer.render(&mut [0.0; 3]).unwrap();
    assert_eq!(
        (
            raw.counters.commands_consumed,
            raw.counters.commands_applied,
            raw.counters.unknown_stops
        ),
        (2, 2, 1)
    );
    validate_stop_evidence(Some(raw), &evidence).unwrap();
    // An unrelated command bypassing the owner's ledger cannot borrow credit
    // from the earlier refused Stop merely because a queue slot later opened.
    producer.try_push(stop(99, 3_000_000)).unwrap();
    let excess = mixer.render(&mut [0.0]).unwrap();
    assert_eq!(excess.counters.unknown_stops, 2);
    let failure = validate_stop_evidence(Some(excess), &evidence).unwrap_err();
    assert_eq!(
        failure
            .downcast_ref::<NativeStopEvidenceError>()
            .unwrap()
            .report,
        excess
    );
    assert_eq!(evidence.admitted_stops(), 1);
    assert_eq!(
        mixer.counters().unknown_stops,
        2,
        "the ledger never edits Mixer counters"
    );

    // Actual native preparation selects the exclusive endpoint after section,
    // preroll and ceil-frame mapping, including a negative output origin.
    for origin in [10_000_000, -2_000_000] {
        let mut cfg = config();
        cfg.output_origin.timestamp = at(origin);
        cfg.playback_end_frame = Some(4);
        let mut prepared = prepare_audio(
            bank(),
            vec![
                play(20_000_000, 1.0), // section-relative zero + preroll: frame 3
                play(20_000_001, 1.0), // one ns later rounds to excluded frame 4
                play(21_000_000, 1.0), // exactly the excluded endpoint
                play(22_000_000, 1.0), // beyond the endpoint
            ],
            cfg,
        )
        .unwrap();
        assert_eq!(prepared.producer.counters().accepted, 1);
        assert_eq!(
            (
                prepared.bgm.report().remaining,
                prepared.bgm.report().outstanding
            ),
            (0, 1)
        );
        let mut pcm = [9.0; 6];
        let end = prepared.mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.0, 0.0, 0.0, 0.25, 0.0, 0.0]);
        assert_eq!(end.playback_end_physical_frame, Some(4));
        assert!(
            end.paused,
            "the final active prefix is reported together with endpoint pause"
        );
        assert_eq!(
            (
                end.counters.commands_consumed,
                end.counters.commands_applied
            ),
            (1, 1)
        );
        feed_rendered(&mut prepared.bgm, Some(end), |command| {
            prepared.producer.try_push(command)
        })
        .unwrap();
        assert_eq!(
            (
                prepared.bgm.report().remaining,
                prepared.bgm.report().outstanding
            ),
            (0, 0)
        );
        assert_eq!(
            prepared.producer.counters().accepted,
            1,
            "retirement cannot readmit the last cue"
        );
        assert_eq!(
            end.active_voices, 1,
            "the immutable endpoint silences the still-active BGM tail"
        );
        assert!(finite_terminal_output_ready(
            prepared.bgm.report(),
            Some(end),
            1
        ));
        let mut after_pcm = [9.0; 2];
        let after_end = prepared.mixer.render(&mut after_pcm).unwrap();
        assert_eq!(after_pcm, [0.0, 0.0]);
        assert_eq!(after_end.playback_frames, 0);
        assert_eq!(after_end.counters.commands_applied, 1);
        feed_rendered(&mut prepared.bgm, Some(end), |command| {
            prepared.producer.try_push(command)
        })
        .unwrap();
        assert_eq!(prepared.producer.counters().accepted, 1);
    }
}
