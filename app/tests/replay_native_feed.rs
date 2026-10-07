//! Portable native-admission fixtures; native backends are never constructed.
use beatkernel::{audio::*, time::*, transport::Rate};
use beatkernel_bms_runtime::{
    bgm::{BgmConfig, BgmFeedError, BgmFeeder},
    replay_audio::completed_render_cursor,
};

fn play(voice: u64, sample: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(sample),
        at: Timestamp::from_nanos(at),
        gain,
    }
}
fn config(origin: i64) -> BgmConfig {
    BgmConfig {
        output_origin: ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(origin),
        },
        sample_rate: 4,
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(2_000_000_000),
        max_pending: 4,
    }
}
fn rig(rate: u32, origin: i64, pending: usize, voices: usize) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(rate, 1).unwrap();
    let pcm_limits = PcmLimits::new(64, 256, 2).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0, 0.5], pcm_limits).unwrap(),
    )
    .unwrap();
    bank.insert(
        SampleId(2),
        PcmSample::new(format, vec![-0.5], pcm_limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(origin),
            AudioLimits::new(8, voices, pending, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn state(feeder: &BgmFeeder) -> (usize, usize, usize, bool) {
    let report = feeder.report();
    assert_eq!(report.admitted, 0);
    (
        report.total_admitted,
        report.remaining,
        report.outstanding,
        report.deferred,
    )
}

#[test]
fn mapped_commands_are_primed_unchanged_with_signed_origin_fractional_ceil_and_tied_order() {
    for origin in [100, -100] {
        // These already include the planner's origin and 500,000,001ns preroll.
        let first = origin + 500_000_001;
        let last = origin + 1_000_000_001;
        let expected = vec![
            play(7, 1, first, 1.0),
            play(7, 2, first, 0.5),
            play(8, 1, last, 1.0),
        ];
        let mut feeder = BgmFeeder::from_output_commands(
            vec![expected[2], expected[0], expected[1]],
            config(origin),
        )
        .unwrap();
        let (mut producer, mut mixer) = rig(4, origin, 4, 2);
        let mut admitted = Vec::new();
        let prime = feeder
            .feed(0, 4, |command| {
                producer.try_push(command)?;
                admitted.push(command);
                Ok(())
            })
            .unwrap();
        assert_eq!(admitted, expected);
        assert_eq!(
            (prime.admitted, prime.outstanding, prime.remaining),
            (3, 3, 0)
        );
        let mut pcm = [99.0; 8];
        let report = mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.0, 0.0, 0.0, -0.25, 0.0, 1.0, 0.5, 0.0]);
        assert_eq!(completed_render_cursor(&report).unwrap(), 8);
        assert_eq!(report.counters.commands_applied, 3);
        let final_feed = feeder
            .feed(8, 1, |_| panic!("all commands already primed"))
            .unwrap();
        assert_eq!(final_feed.outstanding, 0);
    }
}

#[test]
fn rolling_credit_uses_completed_frame_end_not_unrendered_equal_target() {
    let chosen = BgmConfig {
        max_pending: 1,
        lookahead: Duration::from_nanos(1_000_000_000),
        ..config(0)
    };
    let mut feeder = BgmFeeder::from_output_commands(
        vec![
            play(1, 2, 0, 1.0),
            play(2, 2, 1_000_000_000, 1.0),
            play(3, 2, 2_000_000_000, 1.0),
        ],
        chosen,
    )
    .unwrap();
    let (mut producer, mut mixer) = rig(4, 0, 1, 1);
    assert_eq!(
        feeder
            .feed(0, 1, |command| producer.try_push(command))
            .unwrap()
            .admitted,
        1
    );
    let mut first = [0.0];
    let first_report = mixer.render(&mut first).unwrap();
    assert_eq!(first, [-0.5]);
    assert_eq!(
        feeder
            .feed(
                completed_render_cursor(&first_report).unwrap(),
                1,
                |command| producer.try_push(command)
            )
            .unwrap()
            .admitted,
        1
    );
    let mut middle = [99.0; 3];
    let middle_report = mixer.render(&mut middle).unwrap();
    assert_eq!(middle, [0.0; 3]);
    let at_target = feeder
        .feed(completed_render_cursor(&middle_report).unwrap(), 1, |_| {
            panic!("frame4 has not rendered")
        })
        .unwrap();
    assert_eq!(
        (
            at_target.admitted,
            at_target.outstanding,
            at_target.remaining,
            at_target.deferred
        ),
        (0, 1, 1, true)
    );
    let mut at = [0.0];
    let at_report = mixer.render(&mut at).unwrap();
    assert_eq!(at, [-0.5]);
    assert_eq!(
        feeder
            .feed(completed_render_cursor(&at_report).unwrap(), 1, |command| {
                producer.try_push(command)
            })
            .unwrap()
            .admitted,
        1
    );
    let mut last = [99.0; 4];
    let last_report = mixer.render(&mut last).unwrap();
    assert_eq!(last, [0.0, 0.0, 0.0, -0.5]);
    assert_eq!(
        feeder
            .feed(
                completed_render_cursor(&last_report).unwrap(),
                1,
                |_| panic!("completed")
            )
            .unwrap()
            .outstanding,
        0
    );
}

#[test]
fn full_i64_timestamp_span_is_subtracted_widely_without_second_origin_mapping() {
    let chosen = BgmConfig {
        sample_rate: 1,
        lookahead: Duration::from_nanos(1),
        max_pending: 1,
        ..config(i64::MIN)
    };
    let command = play(1, 1, i64::MAX, 1.0);
    let mut feeder = BgmFeeder::from_output_commands(vec![command], chosen).unwrap();
    let target = 18_446_744_074_u64; // ceil(u64::MAX nanoseconds / 1e9)
    let report = feeder
        .feed(target - 1, 1, |actual| {
            assert_eq!(actual, command);
            Ok(())
        })
        .unwrap();
    assert_eq!(report.admitted, 1);
    assert_eq!(
        feeder
            .feed(target, 1, |_| panic!("already admitted"))
            .unwrap()
            .outstanding,
        1
    );
    assert_eq!(
        feeder
            .feed(target + 1, 1, |_| panic!("already admitted"))
            .unwrap()
            .outstanding,
        0
    );
    assert!(BgmFeeder::from_output_commands(
        vec![command],
        BgmConfig {
            sample_rate: u32::MAX,
            ..chosen
        },
    )
    .is_err());
}

#[test]
fn sixty_five_thousand_sparse_commands_fit_one_credit_without_total_schedule_cap() {
    const COUNT: usize = 65_537;
    let chosen = BgmConfig {
        sample_rate: 1,
        max_pending: 1,
        lookahead: Duration::from_nanos(1),
        ..config(-100)
    };
    let commands = (0..COUNT)
        .map(|index| play(index as u64, 1, index as i64 * 2_000_000_000 - 100, 1.0))
        .collect();
    let mut feeder = BgmFeeder::from_output_commands(commands, chosen).unwrap();
    for index in 0..COUNT {
        let report = feeder
            .feed(index as u64 * 2, 1, |command| {
                assert_eq!(
                    command,
                    play(index as u64, 1, index as i64 * 2_000_000_000 - 100, 1.0)
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(
            (report.admitted, report.outstanding, report.total_admitted),
            (1, 1, index + 1)
        );
    }
    assert_eq!(
        feeder
            .feed(COUNT as u64 * 2, 1, |_| panic!("completed schedule"))
            .unwrap()
            .outstanding,
        0
    );
}

#[test]
fn queue_full_and_disconnect_return_exact_commands_and_retain_successful_prefix() {
    let first = play(1, 1, 100, 1.0);
    let second = play(2, 2, 100, -0.5);
    let mut feeder = BgmFeeder::from_output_commands(vec![first, second], config(100)).unwrap();
    let (mut producer, mut consumer) = command_queue(1).unwrap();
    let error = feeder
        .feed(0, 2, |command| producer.try_push(command))
        .err()
        .unwrap();
    match error {
        BgmFeedError::Admission(error) => {
            assert_eq!(error.command, second);
            assert_eq!(error.reason, QueuePushError::Full);
        }
        other => panic!("unexpected failure: {other:?}"),
    }
    assert_eq!(
        (state(&feeder).0, state(&feeder).1, state(&feeder).2),
        (1, 1, 1)
    );
    assert_eq!(consumer.try_pop().unwrap(), first);
    drop(consumer);
    let error = feeder
        .feed(0, 1, |command| producer.try_push(command))
        .err()
        .unwrap();
    match error {
        BgmFeedError::Admission(error) => {
            assert_eq!(error.command, second);
            assert_eq!(error.reason, QueuePushError::Disconnected);
        }
        other => panic!("unexpected failure: {other:?}"),
    }
    assert_eq!(
        (state(&feeder).0, state(&feeder).1, state(&feeder).2),
        (1, 1, 1)
    );
}

#[test]
fn invalid_preroll_before_origin_and_late_or_regressed_cursor_errors_are_atomic() {
    for preroll in [Duration::from_nanos(1), Duration::from_nanos(-1)] {
        assert!(BgmFeeder::from_output_commands(
            vec![],
            BgmConfig {
                preroll,
                ..config(100)
            }
        )
        .is_err());
    }
    assert!(BgmFeeder::from_output_commands(vec![play(1, 1, 99, 1.0)], config(100)).is_err());
    let command = play(1, 1, 1_000_000_100, 1.0);
    let mut feeder = BgmFeeder::from_output_commands(vec![command], config(100)).unwrap();
    let before = state(&feeder);
    assert!(
        matches!(feeder.feed(5, 1, |_| panic!("late must reject before admission")), Err(BgmFeedError::Late { command: failed, target_frame: 4, rendered_frames: 5 }) if failed == command)
    );
    assert_eq!(state(&feeder), before);
    assert_eq!(feeder.feed(4, 1, |_| Ok(())).unwrap().admitted, 1);
    let before = state(&feeder);
    assert!(matches!(
        feeder.feed(3, 1, |_| panic!("regression")),
        Err(BgmFeedError::CursorRegression {
            previous: 4,
            received: 3
        })
    ));
    assert_eq!(state(&feeder), before);
    assert_eq!(
        feeder
            .feed(4, 1, |_| panic!("already admitted"))
            .unwrap()
            .outstanding,
        1
    );
}

#[test]
fn actual_core_rejections_never_produce_a_successful_completed_cursor() {
    for command in [
        AudioCommand::Stop {
            voice: VoiceId(999),
            at: Timestamp::ZERO,
        },
        play(1, 999, 0, 1.0),
        play(1, 1, 0, f32::NAN),
        play(1, 1, -1, 1.0),
    ] {
        let (mut producer, mut mixer) = rig(4, 0, 4, 2);
        producer.try_push(command).unwrap();
        let report = mixer.render(&mut [0.0]).unwrap();
        assert!(completed_render_cursor(&report).is_err(), "{command:?}");
    }
    let (mut producer, mut mixer) = rig(4, 0, 1, 1);
    producer.try_push(play(1, 1, 1_000_000_000, 1.0)).unwrap();
    producer.try_push(play(2, 1, 2_000_000_000, 1.0)).unwrap();
    let report = mixer.render(&mut [0.0]).unwrap();
    assert_eq!(report.counters.pending_full, 1);
    assert!(completed_render_cursor(&report).is_err());
    let (mut producer, mut mixer) = rig(4, 0, 4, 1);
    producer.try_push(play(1, 1, 0, 1.0)).unwrap();
    producer.try_push(play(2, 1, 0, 1.0)).unwrap();
    let report = mixer.render(&mut [0.0]).unwrap();
    assert_eq!(report.counters.voice_full, 1);
    assert!(completed_render_cursor(&report).is_err());
    let (mut producer, mut mixer) = rig(u32::MAX, i64::MIN, 4, 1);
    producer.try_push(play(1, 1, i64::MAX, 1.0)).unwrap();
    let report = mixer.render(&mut [0.0]).unwrap();
    assert_eq!(report.counters.invalid_times, 1);
    assert!(completed_render_cursor(&report).is_err());
    let (mut producer, mut mixer) = rig(4, 0, 4, 1);
    producer
        .try_push(AudioCommand::SetRate {
            rate: Rate::new(1, u64::MAX).unwrap(),
            at: Timestamp::ZERO,
        })
        .unwrap();
    producer.try_push(play(1, 1, 0, 1.0)).unwrap();
    let first = mixer.render(&mut [0.0]).unwrap();
    assert_eq!(completed_render_cursor(&first).unwrap(), 1);
    producer
        .try_push(AudioCommand::SetRate {
            rate: Rate::new(1, u64::MAX - 2).unwrap(),
            at: Timestamp::from_nanos(250_000_000),
        })
        .unwrap();
    let report = mixer.render(&mut [0.0]).unwrap();
    assert_eq!(report.counters.invalid_rates, 1);
    assert!(completed_render_cursor(&report).is_err());
}

#[test]
fn completed_cursor_is_checked_frame_sum_not_counter_or_last_timestamp() {
    let (_producer, mut mixer) = rig(4, 0, 4, 1);
    let first = mixer.render(&mut [0.0; 3]).unwrap();
    let second = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(completed_render_cursor(&first).unwrap(), 3);
    assert_eq!(completed_render_cursor(&second).unwrap(), 5);
    let mut public_report = second;
    public_report.start_frame = u64::MAX;
    public_report.frames = 1;
    assert!(completed_render_cursor(&public_report).is_err());
    public_report.frames = 0;
    assert_eq!(completed_render_cursor(&public_report).unwrap(), u64::MAX);
}
