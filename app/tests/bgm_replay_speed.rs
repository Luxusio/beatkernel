//! Independent fixed-speed admission contracts through the original queue/Mixer.
use beatkernel::{audio::*, time::*, transport::Rate};
use beatkernel_bms_runtime::bgm::{BgmConfig, BgmFeedError, BgmFeeder};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn config(pending: usize) -> BgmConfig {
    BgmConfig {
        output_origin: ClockPoint {
            domain: ClockDomainId(19),
            timestamp: ts(-17),
        },
        sample_rate: 4,
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(2_000_000_000),
        max_pending: pending,
    }
}
fn control(rate: Rate, at: i64) -> AudioCommand {
    AudioCommand::SetRate { rate, at: ts(at) }
}
fn play(sample: u64, at: i64) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(7),
        sample: SampleId(sample),
        at: ts(at),
        gain: 1.0,
    }
}
fn stop(at: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(7),
        at: ts(at),
    }
}
fn rig(capacity: usize, end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(4, 1).unwrap();
    let pcm_limits = PcmLimits::new(32, 64, 2).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    for (id, samples) in [(1, vec![0.125, 0.25, 0.5, 0.75]), (2, vec![-0.5])] {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, samples, pcm_limits).unwrap(),
        )
        .unwrap();
    }
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut chosen = MixerConfig::new(
        format,
        ClockDomainId(19),
        ts(-17),
        AudioLimits::new(capacity, 2, 16, 32, 16).unwrap(),
    );
    if let Some(end) = end {
        chosen = chosen.with_playback_end_frame(end);
    }
    (producer, Mixer::new(chosen, bank, consumer).unwrap())
}

#[test]
fn actual_pcm_at_four_rates_uses_literal_readheads_and_original_output_timestamps() {
    for (num, den, mut expected) in [
        (
            1,
            2,
            vec![0.125, 0.1875, 0.25, 0.375, 0.5, 0.625, 0.75, 0.75],
        ),
        (1, 1, vec![0.125, 0.25, 0.5, 0.75]),
        (2, 1, vec![0.125, 0.5]),
        (3, 2, vec![0.125, 0.375, 0.75]),
    ] {
        expected.resize(12, 0.0);
        let rate = Rate::new(num, den).unwrap();
        let mut commands = Vec::new();
        if rate != Rate::NORMAL {
            commands.push(control(rate, -17));
        }
        commands.push(play(1, -17));
        for block in [1, 3, 12] {
            let mut feeder =
                BgmFeeder::from_output_commands_with_rate(commands.clone(), config(4), rate)
                    .unwrap();
            let (mut producer, mut mixer) = rig(4, None);
            let mut accepted = Vec::new();
            let report = feeder
                .feed(0, 4, |command| {
                    producer.try_push(command)?;
                    accepted.push(command);
                    Ok(())
                })
                .unwrap();
            assert_eq!(accepted, commands);
            assert_eq!(report.total_admitted, commands.len());
            assert_eq!(report.outstanding, commands.len());
            assert_eq!(feeder.admitted_stops(), 0);
            assert_eq!(mixer.counters().commands_applied, 0);
            let mut actual = Vec::new();
            while actual.len() < 12 {
                let mut pcm = vec![0.0; block.min(12 - actual.len())];
                mixer.render(&mut pcm).unwrap();
                actual.extend(pcm);
            }
            assert_eq!(actual, expected, "{num}/{den}, partition {block}");
            assert_eq!(mixer.rate(), rate);
            assert_eq!(mixer.counters().commands_consumed, commands.len() as u64);
            assert_eq!(mixer.counters().commands_applied, commands.len() as u64);
            assert_eq!(mixer.counters().late_commands, 0);
            let retired = feeder.retire_completed(mixer.frame_cursor()).unwrap();
            assert_eq!((retired.remaining, retired.outstanding), (0, 0));
        }
    }
}

#[test]
fn rate_is_before_tied_play_and_original_play_identity_order_is_stable() {
    let rate = Rate::new(1, 2).unwrap();
    let commands = vec![control(rate, -17), play(1, -17), play(2, -17)];
    let mut feeder =
        BgmFeeder::from_output_commands_with_rate(commands.clone(), config(3), rate).unwrap();
    let (mut producer, mut mixer) = rig(3, None);
    let mut accepted = Vec::new();
    feeder
        .feed(0, 3, |command| {
            producer.try_push(command)?;
            accepted.push(command);
            Ok(())
        })
        .unwrap();
    assert_eq!(accepted, commands);
    let mut pcm = [0.0; 4];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [-0.5, -0.5, 0.0, 0.0]);
    assert_eq!(mixer.counters().commands_applied, 3);
    assert_eq!(feeder.admitted_stops(), 0);
}

#[test]
fn control_uses_real_pending_credit_and_cannot_retire_at_its_equal_cursor() {
    let rate = Rate::new(2, 1).unwrap();
    let mut feeder = BgmFeeder::from_output_commands_with_rate(
        vec![control(rate, -17), play(1, -17)],
        config(1),
        rate,
    )
    .unwrap();
    let (mut producer, _consumer) = command_queue(2).unwrap();
    let first = feeder
        .feed(0, 2, |command| producer.try_push(command))
        .unwrap();
    assert_eq!(
        (
            first.admitted,
            first.total_admitted,
            first.remaining,
            first.outstanding,
            first.deferred
        ),
        (1, 1, 1, 1, true)
    );
    let blocked = feeder
        .feed(0, 2, |_| panic!("control still owns the sole credit"))
        .unwrap();
    assert_eq!(
        (
            blocked.admitted,
            blocked.outstanding,
            blocked.remaining,
            blocked.deferred
        ),
        (0, 1, 1, true)
    );
    assert_eq!(feeder.retire_completed(0).unwrap(), blocked);
    let before = feeder.report();
    assert_eq!(
        feeder
            .feed(1, 2, |_| panic!(
                "missed tied Play must not be retimestamped"
            ))
            .unwrap_err(),
        BgmFeedError::Late {
            command: play(1, -17),
            target_frame: 0,
            rendered_frames: 1
        }
    );
    assert_eq!(feeder.report(), before);
    assert_eq!(feeder.admitted_stops(), 0);
}

#[test]
fn actual_queue_full_preserves_control_prefix_and_retries_exact_failed_command() {
    let rate = Rate::new(3, 2).unwrap();
    let commands = [control(rate, -17), play(1, -17)];
    let mut feeder =
        BgmFeeder::from_output_commands_with_rate(commands.to_vec(), config(2), rate).unwrap();
    let (mut producer, mut consumer) = command_queue(1).unwrap();
    assert_eq!(
        feeder
            .feed(0, 2, |command| producer.try_push(command))
            .unwrap_err(),
        BgmFeedError::Admission(CommandPushError {
            command: commands[1],
            reason: QueuePushError::Full
        })
    );
    let prefix = feeder.report();
    assert_eq!(
        (
            prefix.total_admitted,
            prefix.remaining,
            prefix.outstanding,
            prefix.deferred
        ),
        (1, 1, 1, true)
    );
    assert_eq!(feeder.admitted_stops(), 0);
    assert_eq!(consumer.try_pop(), Ok(commands[0]));
    let retry = feeder
        .feed(0, 2, |command| producer.try_push(command))
        .unwrap();
    assert_eq!(
        (
            retry.admitted,
            retry.total_admitted,
            retry.remaining,
            retry.outstanding
        ),
        (1, 2, 0, 2)
    );
    assert_eq!(consumer.try_pop(), Ok(commands[1]));
    assert_eq!(consumer.try_pop(), Err(QueuePopError::Empty));
    assert_eq!(producer.counters().accepted, 2);
    assert_eq!(producer.counters().full, 1);
}

#[test]
fn control_does_not_grant_stop_allowance_and_rejected_stop_is_not_counted() {
    let rate = Rate::new(1, 2).unwrap();
    let commands = [control(rate, -17), play(1, -17), stop(499_999_983)];
    let mut feeder =
        BgmFeeder::from_output_commands_with_rate(commands.to_vec(), config(3), rate).unwrap();
    let (mut producer, mut mixer) = rig(2, None);
    assert_eq!(
        feeder
            .feed(0, 3, |command| producer.try_push(command))
            .unwrap_err(),
        BgmFeedError::Admission(CommandPushError {
            command: commands[2],
            reason: QueuePushError::Full
        })
    );
    assert_eq!(
        (feeder.report().total_admitted, feeder.report().remaining),
        (2, 1)
    );
    assert_eq!(feeder.admitted_stops(), 0);
    let mut head = [0.0];
    mixer.render(&mut head).unwrap();
    assert_eq!(head, [0.125]);
    assert_eq!(mixer.counters().commands_applied, 2);
    assert_eq!(
        feeder
            .feed(mixer.frame_cursor(), 1, |command| producer
                .try_push(command))
            .unwrap()
            .admitted,
        1
    );
    assert_eq!(feeder.admitted_stops(), 1);
    let mut tail = [0.0; 3];
    mixer.render(&mut tail).unwrap();
    assert_eq!(tail, [0.1875, 0.0, 0.0]);
    assert_eq!(mixer.counters().commands_applied, 3);
    assert_eq!(mixer.counters().unknown_stops, 0);
    feeder.retire_completed(mixer.frame_cursor()).unwrap();
    assert_eq!(feeder.admitted_stops(), 1);
    assert_eq!(feeder.report().outstanding, 0);
}

#[test]
fn empty_zero_fence_uses_no_control_credit_and_actual_mixer_marks_zero() {
    for rate in [
        Rate::new(1, 2).unwrap(),
        Rate::NORMAL,
        Rate::new(2, 1).unwrap(),
        Rate::new(3, 2).unwrap(),
    ] {
        let mut feeder =
            BgmFeeder::from_output_commands_with_rate(vec![], config(1), rate).unwrap();
        let (_producer, mut mixer) = rig(1, Some(0));
        let feed = feeder
            .feed(0, 1, |_| panic!("zero-fence plan has no command"))
            .unwrap();
        assert_eq!(
            (
                feed.admitted,
                feed.total_admitted,
                feed.remaining,
                feed.outstanding,
                feed.deferred
            ),
            (0, 0, 0, 0, false)
        );
        let mut pcm = [1.0; 2];
        let rendered = mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.0; 2]);
        assert_eq!(rendered.playback_end_physical_frame, Some(0));
        assert_eq!(rendered.counters.commands_applied, 0);
        assert_eq!(feeder.admitted_stops(), 0);
    }
}

#[test]
fn malformed_control_programs_are_rejected_before_any_admission() {
    let rate = Rate::new(2, 1).unwrap();
    let seek = AudioCommand::Seek {
        song_time: ts(9),
        at: ts(-17),
    };
    let cases = [
        vec![play(1, -17)], // missing control
        vec![control(Rate::new(1, 2).unwrap(), -17), play(1, -17)],
        vec![control(rate, -16), play(1, -16)], // wrong origin
        vec![control(rate, -18), play(1, -17)],
        vec![play(1, -17), control(rate, -17)], // later ordinal even though tied
        vec![control(rate, -17), control(rate, -17), play(1, -17)],
        vec![control(rate, -17), play(1, -17), control(rate, 249_999_983)],
        vec![control(rate, -17), seek],
        vec![seek, control(rate, -17)],
        vec![control(Rate::ZERO, -17), play(1, -17)],
        vec![control(Rate::REVERSE, -17), play(1, -17)],
    ];
    for commands in cases {
        assert!(
            BgmFeeder::from_output_commands_with_rate(commands.clone(), config(4), rate).is_err(),
            "must reject before a feeder exists: {commands:?}"
        );
    }
    for selected in [
        Rate::ZERO,
        Rate::REVERSE,
        Rate::new(i64::MIN, u64::MAX).unwrap(),
    ] {
        for commands in [vec![], vec![control(selected, -17)]] {
            assert!(
                BgmFeeder::from_output_commands_with_rate(commands, config(4), selected).is_err()
            );
        }
    }
    for preroll in [Duration::from_nanos(1), Duration::from_nanos(-1)] {
        assert!(BgmFeeder::from_output_commands_with_rate(
            vec![],
            BgmConfig {
                preroll,
                ..config(4)
            },
            rate
        )
        .is_err());
    }
}

#[test]
fn normal_opt_in_and_legacy_apis_keep_their_existing_control_refusal() {
    for rate in [Rate::NORMAL, Rate::new(8, 8).unwrap()] {
        for commands in [
            vec![control(rate, -17)],
            vec![control(rate, -17), play(1, -17)],
        ] {
            assert!(
                BgmFeeder::from_output_commands_with_rate(commands.clone(), config(4), rate)
                    .is_err()
            );
            assert!(BgmFeeder::from_output_commands(commands, config(4)).is_err());
        }
        let commands = vec![play(1, -17), stop(249_999_983)];
        let mut selected =
            BgmFeeder::from_output_commands_with_rate(commands.clone(), config(4), rate).unwrap();
        let mut legacy = BgmFeeder::from_output_commands(commands.clone(), config(4)).unwrap();
        let mut accepted_selected = Vec::new();
        let mut accepted_legacy = Vec::new();
        let a = selected
            .feed(0, 4, |command| {
                accepted_selected.push(command);
                Ok(())
            })
            .unwrap();
        let b = legacy
            .feed(0, 4, |command| {
                accepted_legacy.push(command);
                Ok(())
            })
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(accepted_selected, commands);
        assert_eq!(accepted_legacy, commands);
        assert_eq!(selected.admitted_stops(), 1);
    }
    for command in [
        control(Rate::new(2, 1).unwrap(), -17),
        AudioCommand::Seek {
            song_time: ts(0),
            at: ts(-17),
        },
    ] {
        assert!(BgmFeeder::from_output_commands(vec![command], config(4)).is_err());
    }
    for command in [
        control(Rate::NORMAL, 0),
        control(Rate::new(2, 1).unwrap(), 0),
        stop(0),
        AudioCommand::Seek {
            song_time: ts(0),
            at: ts(0),
        },
    ] {
        assert!(BgmFeeder::new(vec![command], config(4)).is_err());
    }
}
