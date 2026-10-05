// Deferred binary boundary checks over portable queue/Mixer, without opening a driver.
use super::*;
use beatkernel::audio::{
    AudioCommand, AudioCounters, CommandProducer, PcmSample, SampleBank, SampleId, VoiceId,
};
use beatkernel_bms_runtime::replay_audio::{completed_render_cursor_for_feeder, ReplayAudioError};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn play(voice: u64, ns: i64) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(1),
        voice: VoiceId(voice),
        at: ts(ns),
        gain: 1.0,
    }
}
fn stop(voice: u64, ns: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(ns),
    }
}
fn setup(commands: Vec<AudioCommand>) -> (BgmFeeder, CommandProducer, Mixer) {
    let format = AudioFormat::new(10, 1).unwrap();
    let limits = PcmLimits::new(64, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(4).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            OUTPUT,
            Timestamp::ZERO,
            AudioLimits::new(4, 2, 8, 8, 4).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let feeder = BgmFeeder::from_output_commands(
        commands,
        BgmConfig {
            output_origin: ClockPoint {
                domain: OUTPUT,
                timestamp: Timestamp::ZERO,
            },
            sample_rate: 10,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(1_000_000_000),
            max_pending: 4,
        },
    )
    .unwrap();
    (feeder, producer, mixer)
}

#[test]
fn binary_cursor_uses_actual_feeder_receipts_and_retains_equal_frame_play_stop_order() {
    for reverse in [false, true] {
        let commands = if reverse {
            vec![stop(u64::MAX, 0), play(u64::MAX, 0)]
        } else {
            vec![play(u64::MAX, 0), stop(u64::MAX, 0)]
        };
        let (mut feeder, mut producer, mut mixer) = setup(commands);
        assert_eq!(feeder.admitted_stops(), 0);
        feeder
            .feed(0, 4, |command| producer.try_push(command))
            .unwrap();
        assert_eq!(feeder.admitted_stops(), 1);
        let mut pcm = [9.0];
        let raw = mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [if reverse { 0.25 } else { 0.0 }]);
        assert_eq!(
            (
                raw.counters.commands_consumed,
                raw.counters.commands_applied
            ),
            (2, 2)
        );
        assert_eq!(raw.counters.unknown_stops, u64::from(reverse));
        assert_eq!(playback_render_cursor_for_feeder(&raw, &feeder).unwrap(), 1);
        // Final-core diagnostics and polling both use this same retained owner.
        assert_eq!(
            completed_render_cursor_for_feeder(&raw, &feeder).unwrap(),
            1
        );
        if reverse {
            let failure = playback_render_cursor(&raw).unwrap_err();
            assert!(
                matches!(failure.downcast_ref::<ReplayAudioError>(), Some(ReplayAudioError::RejectedRender(original)) if *original == raw)
            );
        } else {
            assert_eq!(playback_render_cursor(&raw).unwrap(), 1);
        }
        feeder
            .feed(1, 4, |command| producer.try_push(command))
            .unwrap();
        let mut tail = [9.0; 2];
        let after = mixer.render(&mut tail).unwrap();
        assert_eq!(tail, if reverse { [0.5, 0.0] } else { [0.0, 0.0] });
        assert_eq!(
            playback_render_cursor_for_feeder(&after, &feeder).unwrap(),
            3
        );
        assert_eq!(
            completed_render_cursor_for_feeder(&after, &feeder).unwrap(),
            3
        );
        assert_eq!(producer.counters().accepted, 2);
        assert_eq!(
            feeder.admitted_stops(),
            1,
            "retirement and repeated diagnostics never earn another receipt"
        );
    }
    let (mut ordinary, mut producer, mut mixer) = setup(vec![play(1, 0)]);
    ordinary
        .feed(0, 4, |command| producer.try_push(command))
        .unwrap();
    let mut pcm = [0.0; 3];
    let raw = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.5, 0.0]);
    assert_eq!(ordinary.admitted_stops(), 0);
    assert_eq!(playback_render_cursor(&raw).unwrap(), 3);
    assert_eq!(
        playback_render_cursor_for_feeder(&raw, &ordinary).unwrap(),
        3
    );
}

#[test]
fn binary_owned_cursor_keeps_real_pause_grids_and_rejects_excess_or_unrelated_raw_failures() {
    let (mut feeder, mut producer, mut mixer) =
        setup(vec![stop(u64::MAX, 0), play(1, 100_000_000)]);
    feeder
        .feed(0, 4, |command| producer.try_push(command))
        .unwrap();
    let first = mixer.render(&mut [0.0]).unwrap();
    assert_eq!(first.counters.unknown_stops, 1);
    producer.request_pause(true);
    let mut silence = [9.0; 2];
    let paused = mixer.render(&mut silence).unwrap();
    assert_eq!(silence, [0.0, 0.0]);
    assert!(paused.paused);
    assert_eq!(
        (
            paused.start_frame,
            paused.frames,
            paused.playback_start_frame,
            paused.playback_frames
        ),
        (1, 2, 1, 0)
    );
    assert_eq!(
        completed_render_cursor_for_feeder(&paused, &feeder).unwrap(),
        3
    );
    assert_eq!(
        playback_render_cursor_for_feeder(&paused, &feeder).unwrap(),
        1
    );
    assert!(playback_render_cursor(&paused).is_err());
    feeder
        .feed(
            playback_render_cursor_for_feeder(&paused, &feeder).unwrap(),
            4,
            |command| producer.try_push(command),
        )
        .unwrap();
    assert_eq!(
        feeder.report().outstanding,
        1,
        "physical pause silence cannot retire the queued playback-frame head"
    );
    assert_eq!(producer.counters().accepted, 2);
    producer.request_pause(false);
    let mut head = [0.0; 2];
    let resumed = mixer.render(&mut head).unwrap();
    assert_eq!(head, [0.25, 0.5]);
    assert_eq!(
        (
            resumed.start_frame,
            resumed.frames,
            resumed.playback_start_frame,
            resumed.playback_frames
        ),
        (3, 2, 1, 2)
    );
    assert!(!resumed.paused);
    assert_eq!(
        playback_render_cursor_for_feeder(&resumed, &feeder).unwrap(),
        3
    );
    assert_eq!(
        completed_render_cursor_for_feeder(&resumed, &feeder).unwrap(),
        5
    );
    for bad in [
        RenderReport {
            playback_start_frame: 4,
            ..resumed
        },
        RenderReport {
            playback_frames: 3,
            ..resumed
        },
        RenderReport {
            playback_frames: 1,
            ..resumed
        },
        RenderReport {
            playback_start_frame: u64::MAX,
            ..resumed
        },
    ] {
        assert!(playback_render_cursor_for_feeder(&bad, &feeder).is_err());
    }
    for counters in [
        AudioCounters {
            unknown_stops: 2,
            ..resumed.counters
        },
        AudioCounters {
            commands_applied: 0,
            ..resumed.counters
        },
        AudioCounters {
            unknown_samples: 1,
            ..resumed.counters
        },
        AudioCounters {
            invalid_times: 1,
            ..resumed.counters
        },
    ] {
        let bad = RenderReport {
            counters,
            ..resumed
        };
        let failure = playback_render_cursor_for_feeder(&bad, &feeder).unwrap_err();
        assert!(
            matches!(failure.downcast_ref::<ReplayAudioError>(), Some(ReplayAudioError::RejectedRender(original)) if *original == bad)
        );
    }
    let (unadmitted, _, _) = setup(Vec::new());
    assert!(playback_render_cursor_for_feeder(&resumed, &unadmitted).is_err());
    assert_eq!(
        playback_render_cursor_for_feeder(&resumed, &feeder).unwrap(),
        3
    );
    assert_eq!(feeder.admitted_stops(), 1);
    assert_eq!(mixer.counters(), resumed.counters);
}
