//! Independent finite admission fixtures; no native IO or test execution.
use beatkernel::{audio::*, time::*};
use beatkernel_bms_runtime::bgm::{BgmConfig, BgmFeedError, BgmFeeder};

fn config() -> BgmConfig {
    BgmConfig {
        output_origin: ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::ZERO,
        },
        sample_rate: 4,
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(2_000_000_000),
        max_pending: 4,
    }
}
fn play(voice: u64, sample: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(sample),
        at: Timestamp::from_nanos(at),
        gain,
    }
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
fn actual_mixer_pcm_preserves_preroll_origin_and_equal_cue_identity_order() {
    let mut chosen = config();
    chosen.output_origin.timestamp = Timestamp::from_nanos(100);
    chosen.preroll = Duration::from_nanos(500_000_001);
    let mut feeder = BgmFeeder::new(
        vec![
            play(8, 1, 500_000_000, 1.0),
            play(7, 1, 0, 1.0),
            play(7, 2, 0, 0.5),
        ],
        chosen,
    )
    .unwrap();
    assert_eq!(feeder.config().output_origin, chosen.output_origin);
    assert_eq!(feeder.config().sample_rate, 4);
    let format = AudioFormat::new(4, 1).unwrap();
    let pcm_limits = PcmLimits::new(64, 128, 2).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0], pcm_limits).unwrap(),
    )
    .unwrap();
    bank.insert(
        SampleId(2),
        PcmSample::new(format, vec![-0.5], pcm_limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(4).unwrap();
    let mut admitted = Vec::new();
    let report = feeder
        .feed(0, 4, |command| {
            producer.try_push(command)?;
            admitted.push(command);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        (
            report.admitted,
            report.total_admitted,
            report.remaining,
            report.outstanding,
            report.deferred
        ),
        (3, 3, 0, 3, false)
    );
    assert_eq!(
        admitted,
        vec![
            play(7, 1, 500_000_101, 1.0),
            play(7, 2, 500_000_101, 0.5),
            play(8, 1, 1_000_000_101, 1.0)
        ]
    );
    let limits = AudioLimits::new(4, 2, 4, 8, 4).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(format, ClockDomainId(7), Timestamp::from_nanos(100), limits),
        bank,
        consumer,
    )
    .unwrap();
    let mut pcm = [0.0; 8];
    let render = mixer.render(&mut pcm).unwrap();
    // One nanosecond past exact frames 2/4 must ceil to frames 3/5.
    assert_eq!(pcm, [0.0, 0.0, 0.0, -0.25, 0.0, 1.0, 0.0, 0.0]);
    assert_eq!(render.counters.commands_applied, 3);
    assert_eq!(render.counters.late_commands, 0);
    assert_eq!(render.counters.voice_full, 0);
    let retired = feeder
        .feed(mixer.frame_cursor(), 1, |_| {
            panic!("schedule already admitted")
        })
        .unwrap();
    assert_eq!(
        (retired.admitted, retired.outstanding, retired.remaining),
        (0, 0, 0)
    );
}

#[test]
fn inclusive_lookahead_budget_and_render_end_credit_retirement_are_distinct() {
    let chosen = BgmConfig {
        lookahead: Duration::from_nanos(250_000_000),
        max_pending: 3,
        ..config()
    };
    let mut feeder = BgmFeeder::new(
        vec![
            play(1, 1, 0, 1.0),
            play(2, 1, 250_000_000, 1.0),
            play(3, 1, 500_000_000, 1.0),
        ],
        chosen,
    )
    .unwrap();
    let mut captured = Vec::new();
    let first = feeder
        .feed(0, 1, |command| {
            captured.push(command);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        (
            first.admitted,
            first.remaining,
            first.outstanding,
            first.deferred
        ),
        (1, 2, 1, true)
    );
    let at_horizon = feeder
        .feed(0, 1, |command| {
            captured.push(command);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        (
            at_horizon.admitted,
            at_horizon.remaining,
            at_horizon.outstanding,
            at_horizon.deferred
        ),
        (1, 1, 2, false)
    );
    assert_eq!(
        captured,
        vec![play(1, 1, 0, 1.0), play(2, 1, 250_000_000, 1.0)]
    );
    let next = feeder
        .feed(1, 1, |command| {
            captured.push(command);
            Ok(())
        })
        .unwrap();
    assert_eq!((next.admitted, next.remaining, next.outstanding), (1, 0, 2));
    // Target frame 2 equals render end: it has not been rendered/executed.
    assert_eq!(
        feeder
            .feed(2, 1, |_| panic!("all cues admitted"))
            .unwrap()
            .outstanding,
        1
    );
    assert_eq!(
        feeder
            .feed(3, 1, |_| panic!("all cues admitted"))
            .unwrap()
            .outstanding,
        0
    );
}

#[test]
fn one_credit_defers_eligible_work_until_its_target_is_strictly_before_cursor() {
    let chosen = BgmConfig {
        lookahead: Duration::from_nanos(250_000_000),
        max_pending: 1,
        ..config()
    };
    let mut feeder = BgmFeeder::new(
        vec![play(1, 1, 0, 1.0), play(2, 1, 250_000_000, 1.0)],
        chosen,
    )
    .unwrap();
    let first = feeder.feed(0, 4, |_| Ok(())).unwrap();
    assert_eq!(
        (
            first.admitted,
            first.remaining,
            first.outstanding,
            first.deferred
        ),
        (1, 1, 1, true)
    );
    let same = feeder
        .feed(0, 4, |_| panic!("no credit retired at equal cursor"))
        .unwrap();
    assert_eq!(
        (
            same.admitted,
            same.remaining,
            same.outstanding,
            same.deferred
        ),
        (0, 1, 1, true)
    );
    let next = feeder.feed(1, 4, |_| Ok(())).unwrap();
    assert_eq!(
        (
            next.admitted,
            next.remaining,
            next.outstanding,
            next.deferred
        ),
        (1, 0, 1, false)
    );
    assert_eq!(
        feeder
            .feed(1, 4, |_| panic!("no remaining cues"))
            .unwrap()
            .outstanding,
        1
    );
    assert_eq!(
        feeder
            .feed(2, 4, |_| panic!("no remaining cues"))
            .unwrap()
            .outstanding,
        0
    );
}

#[test]
fn sixty_five_thousand_sparse_cues_are_independent_of_one_pending_credit() {
    const COUNT: usize = 65_537;
    let chosen = BgmConfig {
        sample_rate: 1,
        lookahead: Duration::from_nanos(1_000_000_000),
        max_pending: 1,
        ..config()
    };
    let commands = (0..COUNT)
        .rev()
        .map(|index| play(index as u64 + 1, 1, index as i64 * 2_000_000_000, 0.25))
        .collect();
    let mut feeder = BgmFeeder::new(commands, chosen).unwrap();
    for index in 0..COUNT {
        let mut calls = 0;
        let report = feeder
            .feed(index as u64 * 2, 1, |command| {
                calls += 1;
                assert_eq!(
                    command,
                    play(index as u64 + 1, 1, index as i64 * 2_000_000_000, 0.25)
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(
            (
                report.admitted,
                report.total_admitted,
                report.remaining,
                report.outstanding,
                report.deferred
            ),
            (1, index + 1, COUNT - index - 1, 1, false)
        );
    }
    assert_eq!(
        feeder
            .feed(COUNT as u64 * 2, 1, |_| panic!("all sparse cues admitted"))
            .unwrap()
            .outstanding,
        0
    );
}

#[test]
fn exact_queue_full_preserves_prefix_and_does_not_retry_or_duplicate_it() {
    let commands = vec![play(1, 1, 0, 1.0), play(2, 1, 0, 0.5), play(3, 1, 0, -1.0)];
    let mut feeder = BgmFeeder::new(commands, config()).unwrap();
    let (mut producer, mut consumer) = command_queue(1).unwrap();
    let mut calls = 0;
    let failure = feeder
        .feed(0, 3, |command| {
            calls += 1;
            producer.try_push(command)
        })
        .err()
        .expect("second cue must see full queue");
    match failure {
        BgmFeedError::Admission(error) => {
            assert_eq!(error.reason, QueuePushError::Full);
            assert_eq!(error.command, play(2, 1, 0, 0.5));
        }
        _ => panic!("expected exact admission failure"),
    }
    assert_eq!(calls, 2);
    assert_eq!(
        (state(&feeder).0, state(&feeder).1, state(&feeder).2),
        (1, 2, 1)
    );
    assert_eq!(consumer.try_pop().unwrap(), play(1, 1, 0, 1.0));
    let failure = feeder
        .feed(0, 3, |command| producer.try_push(command))
        .err()
        .expect("third cue must see full queue");
    match failure {
        BgmFeedError::Admission(error) => assert_eq!(error.command, play(3, 1, 0, -1.0)),
        _ => panic!("expected third-cue failure"),
    }
    assert_eq!(
        (state(&feeder).0, state(&feeder).1, state(&feeder).2),
        (2, 1, 2)
    );
    assert_eq!(consumer.try_pop().unwrap(), play(2, 1, 0, 0.5));
    assert_eq!(
        feeder
            .feed(0, 1, |command| producer.try_push(command))
            .unwrap()
            .total_admitted,
        3
    );
    assert_eq!(consumer.try_pop().unwrap(), play(3, 1, 0, -1.0));
}

#[test]
fn disconnect_returns_exact_failed_command_and_retains_successful_prefix() {
    let mut feeder =
        BgmFeeder::new(vec![play(1, 1, 0, 1.0), play(2, 1, 0, -0.5)], config()).unwrap();
    let (mut producer, consumer) = command_queue(4).unwrap();
    let mut consumer = Some(consumer);
    let mut calls = 0;
    let failure = feeder
        .feed(0, 4, |command| {
            calls += 1;
            let result = producer.try_push(command);
            if result.is_ok() {
                drop(consumer.take());
            }
            result
        })
        .err()
        .expect("consumer disconnect must propagate");
    match failure {
        BgmFeedError::Admission(error) => {
            assert_eq!(error.reason, QueuePushError::Disconnected);
            assert_eq!(error.command, play(2, 1, 0, -0.5));
        }
        _ => panic!("expected exact disconnect"),
    }
    assert_eq!(calls, 2);
    assert_eq!(
        (state(&feeder).0, state(&feeder).1, state(&feeder).2),
        (1, 1, 1)
    );
}

#[test]
fn invalid_configuration_command_and_wide_origin_shift_have_explicit_results() {
    let valid = config();
    for invalid in [
        BgmConfig {
            sample_rate: 0,
            ..valid
        },
        BgmConfig {
            lookahead: Duration::ZERO,
            ..valid
        },
        BgmConfig {
            lookahead: Duration::from_nanos(-1),
            ..valid
        },
        BgmConfig {
            preroll: Duration::from_nanos(-1),
            ..valid
        },
        BgmConfig {
            max_pending: 0,
            ..valid
        },
        BgmConfig {
            max_pending: 65_537,
            ..valid
        },
    ] {
        assert!(matches!(
            BgmFeeder::new(vec![], invalid),
            Err(BgmFeedError::InvalidConfiguration(_))
        ));
    }
    let stop = AudioCommand::Stop {
        voice: VoiceId(1),
        at: Timestamp::ZERO,
    };
    match BgmFeeder::new(vec![stop], valid) {
        Err(BgmFeedError::InvalidCommand(command)) => assert_eq!(command, stop),
        _ => panic!("non-Play must reject"),
    }
    for gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        match BgmFeeder::new(vec![play(1, 1, 0, gain)], valid) {
            Err(BgmFeedError::InvalidCommand(AudioCommand::Play { gain: returned, .. })) => {
                assert_eq!(returned.to_bits(), gain.to_bits())
            }
            _ => panic!("nonfinite gain must reject"),
        }
    }
    assert!(matches!(
        BgmFeeder::new(vec![play(1, 1, -1, 1.0)], valid),
        Err(BgmFeedError::InvalidCommand(_))
    ));
    let mut wide = valid;
    wide.output_origin.timestamp = Timestamp::from_nanos(i64::MIN);
    wide.preroll = Duration::from_nanos(i64::MAX);
    wide.lookahead = Duration::from_nanos(i64::MAX);
    wide.sample_rate = 1;
    let mut feeder = BgmFeeder::new(vec![play(1, 1, 1, 1.0)], wide).unwrap();
    let mut mapped = None;
    feeder
        .feed(0, 1, |command| {
            mapped = Some(command);
            Ok(())
        })
        .unwrap();
    assert_eq!(mapped, Some(play(1, 1, 0, 1.0))); // i128 origin+song+preroll before narrowing.
    let overflowing_stamp = BgmConfig {
        output_origin: ClockPoint {
            timestamp: Timestamp::from_nanos(i64::MAX),
            ..valid.output_origin
        },
        ..valid
    };
    assert!(matches!(
        BgmFeeder::new(vec![play(1, 1, 1, 1.0)], overflowing_stamp),
        Err(BgmFeedError::Overflow)
    ));
    let overflowing_frame = BgmConfig {
        output_origin: ClockPoint {
            timestamp: Timestamp::from_nanos(i64::MIN),
            ..valid.output_origin
        },
        sample_rate: u32::MAX,
        ..valid
    };
    assert!(matches!(
        BgmFeeder::new(vec![play(1, 1, i64::MAX, 1.0)], overflowing_frame),
        Err(BgmFeedError::Overflow)
    ));
}

#[test]
fn invalid_budget_cursor_regression_and_horizon_overflow_do_not_mutate_progress() {
    let chosen = BgmConfig {
        sample_rate: 1,
        lookahead: Duration::from_nanos(1_000_000_000),
        ..config()
    };
    let mut feeder = BgmFeeder::new(vec![play(1, 1, 100_000_000_000, 1.0)], chosen).unwrap();
    feeder
        .feed(5, 1, |_| panic!("future cue outside horizon"))
        .unwrap();
    let before = state(&feeder);
    for budget in [0, 65_537] {
        assert!(matches!(
            feeder.feed(6, budget, |_| panic!("invalid budget must not admit")),
            Err(BgmFeedError::InvalidConfiguration(_))
        ));
        assert_eq!(state(&feeder), before);
    }
    assert!(matches!(
        feeder.feed(4, 1, |_| panic!("regressed cursor must not admit")),
        Err(BgmFeedError::CursorRegression {
            previous: 5,
            received: 4
        })
    ));
    assert_eq!(state(&feeder), before);
    assert_eq!(
        feeder
            .feed(5, 1, |_| panic!("unchanged future cue"))
            .unwrap()
            .admitted,
        0
    );
    let mut empty = BgmFeeder::new(vec![], chosen).unwrap();
    empty.feed(5, 1, |_| panic!("empty schedule")).unwrap();
    let before = state(&empty);
    assert!(matches!(
        empty.feed(u64::MAX, 1, |_| panic!("overflow must not admit")),
        Err(BgmFeedError::Overflow)
    ));
    assert_eq!(state(&empty), before);
    assert_eq!(
        empty
            .feed(5, 1, |_| panic!("overflow must not advance cursor"))
            .unwrap()
            .admitted,
        0
    );
}

#[test]
fn an_unadmitted_late_cue_is_not_retimestamped_and_failure_is_atomic() {
    let chosen = BgmConfig {
        max_pending: 1,
        ..config()
    };
    let mut feeder = BgmFeeder::new(vec![play(1, 1, 0, 1.0), play(2, 1, 0, 0.5)], chosen).unwrap();
    feeder.feed(0, 1, |_| Ok(())).unwrap();
    let before = state(&feeder);
    let error = feeder
        .feed(1, 1, |_| panic!("late cue must fail before attempt"))
        .err()
        .expect("late cue must reject");
    match error {
        BgmFeedError::Late {
            command,
            target_frame,
            rendered_frames,
        } => {
            assert_eq!(command, play(2, 1, 0, 0.5));
            assert_eq!((target_frame, rendered_frames), (0, 1));
        }
        _ => panic!("expected explicit stale cue"),
    }
    assert_eq!(state(&feeder), before);
    assert_eq!(
        feeder
            .feed(0, 1, |_| panic!("unrendered first credit remains occupied"))
            .unwrap()
            .admitted,
        0
    );
}
