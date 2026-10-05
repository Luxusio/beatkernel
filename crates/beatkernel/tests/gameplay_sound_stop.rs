//! Deferred shared-policy fixtures. Queue admission is not proof of acoustic silence.
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandPushError, QueuePushError, SampleId, VoiceId},
    runtime::GameplaySoundStop,
    time::Timestamp,
};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn stop(voice: u64, ns: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(ns),
    }
}

#[test]
fn unique_voices_use_success_watermark_and_preserve_actual_partial_queue_refusals_once() {
    let mut owner =
        GameplaySoundStop::new(vec![VoiceId(u64::MAX), VoiceId(7), VoiceId(0), VoiceId(7)]);
    assert_eq!(owner.voices(), [VoiceId(0), VoiceId(7), VoiceId(u64::MAX)]);
    let (mut producer, mut consumer) = command_queue(4).unwrap();
    let admitted = [
        AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(7),
            at: ts(900),
            gain: 1.0,
        },
        AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(0),
            at: ts(-40),
            gain: 1.0,
        },
        AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(7),
            at: ts(500),
            gain: 1.0,
        },
    ];
    for command in admitted {
        producer.try_push(command).unwrap();
        owner.observe_admitted(command.at());
    }
    let mut attempted = Vec::new();
    let report = owner
        .attempt(ts(-100), |command| {
            attempted.push(command);
            producer.try_push(command)
        })
        .unwrap();
    assert_eq!(report.at, ts(900));
    assert_eq!(attempted, [stop(0, 900), stop(7, 900), stop(u64::MAX, 900)]);
    assert_eq!(report.commands, [stop(0, 900)]);
    assert_eq!(
        report.failures,
        [
            CommandPushError {
                command: stop(7, 900),
                reason: QueuePushError::Full
            },
            CommandPushError {
                command: stop(u64::MAX, 900),
                reason: QueuePushError::Full
            },
        ]
    );
    for command in admitted {
        assert_eq!(
            consumer.try_pop().unwrap(),
            command,
            "stopping does not flush accepted Plays"
        );
    }
    assert_eq!(consumer.try_pop().unwrap(), stop(0, 900));
    assert!(consumer.try_pop().is_err());
    assert!(
        owner
            .attempt(ts(i64::MAX), |_| panic!(
                "a drained queue does not authorize retry"
            ))
            .is_none()
    );

    owner.reset();
    assert_eq!(owner.voices(), [VoiceId(0), VoiceId(7), VoiceId(u64::MAX)]);
    let (mut producer, mut consumer) = command_queue(3).unwrap();
    let reset = owner
        .attempt(ts(-9), |command| producer.try_push(command))
        .unwrap();
    assert_eq!(
        reset.at,
        ts(-9),
        "reset discards the prior output watermark"
    );
    assert_eq!(
        reset.commands,
        [stop(0, -9), stop(7, -9), stop(u64::MAX, -9)]
    );
    assert!(reset.failures.is_empty());
    for command in reset.commands {
        assert_eq!(consumer.try_pop().unwrap(), command);
    }
    assert!(consumer.try_pop().is_err());
}

#[test]
fn empty_attempts_signed_extremes_and_callback_errors_are_owned_until_explicit_reset() {
    // The component does not invent a Runtime fence, judge commit or queue requirement.
    let mut empty = GameplaySoundStop::new(vec![]);
    let report = empty
        .attempt(ts(i64::MIN), |_| panic!("empty namespace"))
        .unwrap();
    assert_eq!(report.at, ts(i64::MIN));
    assert!(report.commands.is_empty() && report.failures.is_empty());
    assert!(
        empty
            .attempt(ts(0), |_| panic!("empty still consumes one attempt"))
            .is_none()
    );
    empty.reset();
    empty.observe_admitted(ts(i64::MAX));
    assert_eq!(
        empty.attempt(ts(i64::MIN), |_| unreachable!()).unwrap().at,
        ts(i64::MAX)
    );

    let mut first = GameplaySoundStop::new(vec![VoiceId(u64::MAX), VoiceId(3)]);
    let mut independent = GameplaySoundStop::new(vec![VoiceId(3)]);
    first.observe_admitted(ts(i64::MAX));
    let (mut disconnected, receiver) = command_queue(2).unwrap();
    drop(receiver);
    let report = first
        .attempt(ts(i64::MIN), |command| disconnected.try_push(command))
        .unwrap();
    assert!(report.commands.is_empty());
    assert_eq!(
        report.failures,
        [
            CommandPushError {
                command: stop(3, i64::MAX),
                reason: QueuePushError::Disconnected
            },
            CommandPushError {
                command: stop(u64::MAX, i64::MAX),
                reason: QueuePushError::Disconnected
            },
        ]
    );
    assert!(
        first
            .attempt(ts(0), |_| panic!("no implicit retry after all refusals"))
            .is_none()
    );
    let (mut producer, mut consumer) = command_queue(2).unwrap();
    let other = independent
        .attempt(ts(i64::MIN), |command| producer.try_push(command))
        .unwrap();
    assert_eq!(other.commands, [stop(3, i64::MIN)]);
    first.reset();
    first.observe_admitted(ts(-100));
    // A callback may reject one voice and accept a later one; all original evidence survives.
    let mixed = first
        .attempt(ts(-50), |command| match command {
            AudioCommand::Stop {
                voice: VoiceId(3), ..
            } => Err(CommandPushError {
                command,
                reason: QueuePushError::Full,
            }),
            _ => producer.try_push(command),
        })
        .unwrap();
    assert_eq!(mixed.at, ts(-50));
    assert_eq!(mixed.commands, [stop(u64::MAX, -50)]);
    assert_eq!(
        mixed.failures,
        [CommandPushError {
            command: stop(3, -50),
            reason: QueuePushError::Full
        }]
    );
    assert_eq!(consumer.try_pop().unwrap(), stop(3, i64::MIN));
    assert_eq!(consumer.try_pop().unwrap(), stop(u64::MAX, -50));
    assert!(consumer.try_pop().is_err());
}
