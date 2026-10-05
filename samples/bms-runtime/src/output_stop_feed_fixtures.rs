//! Deferred mapped-output supply through the actual queue and software Mixer.
//! Admission counts do not establish native presentation or Worklet acknowledgement.
use super::*;
use crate::replay_audio::completed_render_cursor;
use beatkernel::{
    audio::{
        command_queue, AudioFormat, CommandProducer, Mixer, MixerConfig, PcmLimits, PcmSample,
        QueuePushError, SampleBank, SampleId, VoiceId,
    },
    time::{ClockDomainId, Timestamp},
    transport::Rate,
};

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn config(origin: i64, pending: usize, lookahead: i64) -> BgmConfig {
    BgmConfig {
        output_origin: ClockPoint {
            domain: ClockDomainId(17),
            timestamp: ts(origin),
        },
        sample_rate: 1000,
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(lookahead),
        max_pending: pending,
    }
}
fn play(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(u64::MAX),
        voice: VoiceId(voice),
        at: ts(at),
        gain: 0.5,
    }
}
fn stop(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(at),
    }
}
fn rig(capacity: usize, origin: i64) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(16, 16, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(u64::MAX),
        PcmSample::new(format, vec![1.0, 0.5, 0.25, 0.0], limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(17),
            ts(origin),
            AudioLimits::new(capacity, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn push(
    producer: &mut CommandProducer,
    accepted: &mut Vec<AudioCommand>,
    command: AudioCommand,
) -> Result<(), CommandPushError> {
    producer.try_push(command)?;
    accepted.push(command);
    Ok(())
}

#[test]
fn caller_equal_time_play_stop_order_controls_pcm_without_hiding_inactive_stop_diagnostics() {
    for reversed in [false, true] {
        let commands = if reversed {
            vec![stop(u64::MAX, 1_000_000), play(u64::MAX, 1_000_000)]
        } else {
            vec![play(u64::MAX, 1_000_000), stop(u64::MAX, 1_000_000)]
        };
        let mut feeder =
            BgmFeeder::from_output_commands(commands.clone(), config(0, 2, 2_000_000)).unwrap();
        let (mut producer, mut mixer) = rig(2, 0);
        assert_eq!(feeder.admitted_stops(), 0);
        let mut accepted = Vec::new();
        assert_eq!(
            feeder
                .feed(0, 2, |command| push(&mut producer, &mut accepted, command))
                .unwrap(),
            BgmFeedReport {
                admitted: 2,
                total_admitted: 2,
                remaining: 0,
                outstanding: 2,
                deferred: false
            }
        );
        assert_eq!(accepted, commands);
        assert_eq!(feeder.admitted_stops(), 1);
        assert_eq!(mixer.counters().commands_consumed, 0); // Callback success has not executed either command.
        let mut output = [0.0; 5];
        let rendered = mixer.render(&mut output).unwrap();
        assert_eq!(rendered.counters.commands_applied, 2);
        assert_eq!(rendered.pending_commands, 0);
        if reversed {
            assert_eq!(output, [0.0, 0.5, 0.25, 0.125, 0.0]);
            assert_eq!(rendered.counters.unknown_stops, 1);
            assert!(completed_render_cursor(&rendered).is_err());
        } else {
            assert_eq!(output, [0.0; 5]);
            assert_eq!(rendered.counters.unknown_stops, 0);
            assert_eq!(completed_render_cursor(&rendered).unwrap(), 5);
        }
        // The actual render cursor retires credits even when a strict completion
        // validator rejects the preserved diagnostic; it is not a completion claim.
        let cursor = rendered.start_frame + rendered.frames as u64;
        assert_eq!(
            feeder
                .feed(cursor, 1, |_| panic!("schedule already exhausted"))
                .unwrap(),
            BgmFeedReport {
                admitted: 0,
                total_admitted: 2,
                remaining: 0,
                outstanding: 0,
                deferred: false
            }
        );
        let before = feeder.report();
        assert_eq!(
            feeder
                .feed(cursor, 1, |_| panic!("no automatic retry"))
                .unwrap(),
            before
        );
        assert_eq!(feeder.admitted_stops(), 1);
        let mut tail = [1.0; 3];
        mixer.render(&mut tail).unwrap();
        assert_eq!(tail, [0.0; 3]);
        assert_eq!(mixer.counters().commands_applied, 2);
    }
}

#[test]
fn rejected_stop_is_uncounted_until_explicit_retry_and_real_rendering_retires_shared_credits() {
    let commands = [
        play(u64::MAX, 2_000_000),
        stop(u64::MAX, 2_000_000),
        play(0, 5_000_000),
        stop(0, 5_000_000),
    ];
    let mut feeder =
        BgmFeeder::from_output_commands(commands.to_vec(), config(0, 2, 10_000_000)).unwrap();
    let (mut producer, mut mixer) = rig(1, 0);
    let mut accepted = Vec::new();
    let error = feeder
        .feed(0, 4, |command| push(&mut producer, &mut accepted, command))
        .unwrap_err();
    assert_eq!(
        error,
        BgmFeedError::Admission(CommandPushError {
            command: commands[1],
            reason: QueuePushError::Full
        })
    );
    assert_eq!(accepted, commands[..1]);
    assert_eq!(
        feeder.report(),
        BgmFeedReport {
            admitted: 0,
            total_admitted: 1,
            remaining: 3,
            outstanding: 1,
            deferred: true
        }
    );
    assert_eq!(feeder.admitted_stops(), 0);
    let mut pcm = [0.0];
    let first = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0]);
    assert_eq!(first.pending_commands, 1);
    assert_eq!(completed_render_cursor(&first).unwrap(), 1);
    let retried = feeder
        .feed(1, 1, |command| push(&mut producer, &mut accepted, command))
        .unwrap();
    assert_eq!(
        retried,
        BgmFeedReport {
            admitted: 1,
            total_admitted: 2,
            remaining: 2,
            outstanding: 2,
            deferred: true
        }
    );
    assert_eq!(feeder.admitted_stops(), 1);
    let blocked = feeder
        .feed(1, 1, |_| panic!("both pending credits remain owned"))
        .unwrap();
    assert_eq!(blocked.admitted, 0);
    assert_eq!(blocked.outstanding, 2);
    assert!(blocked.deferred);
    let mut pcm = [0.0; 2];
    let second = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0; 2]);
    assert_eq!(completed_render_cursor(&second).unwrap(), 3);
    let next = feeder
        .feed(3, 1, |command| push(&mut producer, &mut accepted, command))
        .unwrap();
    assert_eq!(
        next,
        BgmFeedReport {
            admitted: 1,
            total_admitted: 3,
            remaining: 1,
            outstanding: 1,
            deferred: true
        }
    );
    assert_eq!(feeder.admitted_stops(), 1); // Retirement does not decrement the cumulative count.
    assert_eq!(
        feeder
            .feed(3, 1, |command| push(&mut producer, &mut accepted, command))
            .unwrap_err(),
        BgmFeedError::Admission(CommandPushError {
            command: commands[3],
            reason: QueuePushError::Full
        })
    );
    assert_eq!(feeder.admitted_stops(), 1);
    let third = mixer.render(&mut [0.0]).unwrap();
    assert_eq!(completed_render_cursor(&third).unwrap(), 4);
    assert_eq!(
        feeder
            .feed(4, 1, |command| push(&mut producer, &mut accepted, command))
            .unwrap()
            .admitted,
        1
    );
    assert_eq!(feeder.admitted_stops(), 2);
    assert_eq!(accepted, commands);
    assert_eq!(producer.counters().accepted, 4);
    assert_eq!(producer.counters().full, 2);
    let mut pcm = [0.0; 2];
    let last = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0; 2]);
    assert_eq!(completed_render_cursor(&last).unwrap(), 6);
    assert_eq!(last.counters.commands_consumed, 4);
    assert_eq!(last.counters.commands_applied, 4);
    assert_eq!(
        feeder
            .feed(6, 1, |_| panic!("all original cues admitted once"))
            .unwrap(),
        BgmFeedReport {
            admitted: 0,
            total_admitted: 4,
            remaining: 0,
            outstanding: 0,
            deferred: false
        }
    );
    assert_eq!(feeder.admitted_stops(), 2);
}

#[test]
fn mapped_negative_origins_use_original_timestamps_and_subframe_ceiling_with_atomic_refusals() {
    let origin = -2_000_000;
    let chosen = config(origin, 4, 1);
    let commands = [play(u64::MAX, -1_999_999), stop(u64::MAX, -1_000_001)];
    // Input order is intentionally reversed; chronology precedes original tie order.
    let mut feeder =
        BgmFeeder::from_output_commands(vec![commands[1], commands[0]], chosen).unwrap();
    let (mut producer, mut mixer) = rig(4, origin);
    let mut accepted = Vec::new();
    assert_eq!(
        feeder
            .feed(0, 4, |command| push(&mut producer, &mut accepted, command))
            .unwrap()
            .admitted,
        2
    );
    assert_eq!(accepted, commands); // No second origin/preroll addition or timestamp rounding.
    let mut pcm = [0.0; 2];
    let rendered = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0; 2]); // Both positive subframe offsets ceil to frame 1.
    assert_eq!(rendered.counters.commands_applied, 2);
    assert_eq!(completed_render_cursor(&rendered).unwrap(), 2);
    feeder.feed(2, 1, |_| panic!("exhausted")).unwrap();
    let before = feeder.report();
    assert_eq!(
        feeder
            .feed(1, 1, |_| panic!("cursor refusal before callback"))
            .unwrap_err(),
        BgmFeedError::CursorRegression {
            previous: 2,
            received: 1
        }
    );
    assert!(matches!(
        feeder.feed(2, 0, |_| panic!("invalid budget")),
        Err(BgmFeedError::InvalidConfiguration(_))
    ));
    assert_eq!(
        feeder
            .feed(u64::MAX, 1, |_| panic!("horizon overflow before callback"))
            .unwrap_err(),
        BgmFeedError::Overflow
    );
    assert_eq!(feeder.report(), before);
    assert_eq!(feeder.admitted_stops(), 1);

    for command in [
        stop(0, origin - 1),
        play(0, origin - 1),
        AudioCommand::SetRate {
            rate: Rate::NORMAL,
            at: ts(origin),
        },
        AudioCommand::Seek {
            song_time: Timestamp::ZERO,
            at: ts(origin),
        },
    ] {
        assert!(
            matches!(BgmFeeder::from_output_commands(vec![command], chosen),
            Err(BgmFeedError::InvalidCommand(rejected)) if rejected == command)
        );
    }
    for command in [
        stop(u64::MAX, 0),
        AudioCommand::SetRate {
            rate: Rate::NORMAL,
            at: ts(0),
        },
        AudioCommand::Seek {
            song_time: ts(0),
            at: ts(0),
        },
        play(0, -1),
    ] {
        assert!(matches!(BgmFeeder::new(vec![command], config(0, 4, 1)),
            Err(BgmFeedError::InvalidCommand(rejected)) if rejected == command));
    }
    for preroll in [Duration::from_nanos(1), Duration::from_nanos(-1)] {
        assert!(matches!(
            BgmFeeder::from_output_commands(vec![stop(0, origin)], BgmConfig { preroll, ..chosen }),
            Err(BgmFeedError::InvalidConfiguration(_))
        ));
    }
    assert!(matches!(
        BgmFeeder::from_output_commands(
            vec![stop(0, i64::MAX)],
            BgmConfig {
                output_origin: ClockPoint {
                    domain: ClockDomainId(17),
                    timestamp: ts(i64::MIN)
                },
                sample_rate: u32::MAX,
                ..chosen
            }
        ),
        Err(BgmFeedError::Overflow)
    ));
    let mut late = BgmFeeder::from_output_commands(vec![stop(0, origin + 1)], chosen).unwrap();
    let untouched = late.report();
    assert_eq!(
        late.feed(completed_render_cursor(&rendered).unwrap(), 1, |_| panic!(
            "missed Stop may not be retimestamped"
        ))
        .unwrap_err(),
        BgmFeedError::Late {
            command: stop(0, origin + 1),
            target_frame: 1,
            rendered_frames: 2
        }
    );
    assert_eq!(late.report(), untouched);
    assert_eq!(late.admitted_stops(), 0);
    // Song-time Play compatibility retains its one real origin/preroll mapping.
    let mut legacy = BgmFeeder::new(
        vec![play(7, 1)],
        BgmConfig {
            preroll: Duration::from_nanos(1_000_000),
            lookahead: Duration::from_nanos(2_000_000),
            ..chosen
        },
    )
    .unwrap();
    let (mut queue, _consumer) = command_queue(1).unwrap();
    let mut accepted = Vec::new();
    legacy
        .feed(0, 1, |command| push(&mut queue, &mut accepted, command))
        .unwrap();
    assert_eq!(accepted, [play(7, -999_999)]);
    assert_eq!(legacy.admitted_stops(), 0);
}
