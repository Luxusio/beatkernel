//! Deferred native callback admission over actual recorded replay and Mixer.
use crate::{
    bgm::{BgmConfig, BgmFeedError, BgmFeeder},
    completion::ReplayCompletion,
    mine_audio_consumers_fixtures::{data, recorded, replay_limits, Action},
    replay_audio::{
        completed_render_cursor, completed_render_cursor_for_feeder, plan_audio, ReplayAudioError,
    },
    PreparedBms,
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioCounters, AudioLimits, CommandProducer, Mixer,
        MixerConfig, QueuePushError, RenderReport, SampleId, VoiceId,
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::BmsInputMode;

const SECOND: i64 = 1_000_000_000;
const CHART: &str =
    "#BPM 60\n#WAV01 note\n#WAV02 music\n#00011:01000100\n#000D1:00ZZ0000\n#00001:00000002";
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(2),
        timestamp: ts(ns),
    }
}
fn play(sample: u64, voice: u64, ns: i64) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
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
fn config() -> BgmConfig {
    BgmConfig {
        output_origin: point(0),
        sample_rate: 10,
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(SECOND),
        max_pending: 4,
    }
}
fn output(prepared: PreparedBms, capacity: usize) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            prepared.bank.format(),
            ClockDomainId(2),
            ts(0),
            AudioLimits::new(capacity, 4, 8, 16, capacity).unwrap(),
        ),
        prepared.bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn rejected(raw: RenderReport, feeder: &BgmFeeder) {
    match completed_render_cursor_for_feeder(&raw, feeder).unwrap_err() {
        ReplayAudioError::RejectedRender(original) => assert_eq!(original, raw),
        other => panic!("expected original execution evidence, got {other:?}"),
    }
}

#[test]
fn fatal_recording_rolling_feeder_keeps_full_judge_prefix_and_requires_real_completion_barriers() {
    let prepared = data(CHART, false);
    let legacy = recorded(
        &prepared,
        &[
            Action::Press(0, 91),
            Action::Advance(SECOND),
            Action::Release(SECOND, 91),
            Action::Press(2 * SECOND, 91),
            Action::Bgm(0),
            Action::Advance(3 * SECOND),
        ],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let plan = plan_audio(
        &prepared,
        legacy.file.clone(),
        replay_limits(),
        point(0),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(
        plan.commands,
        [
            play(1, 11, 0),
            stop(11, SECOND),
            stop(12, SECOND),
            play(2, 90, 3 * SECOND)
        ]
    );
    assert_eq!(plan.final_judge_hash, legacy.hash);
    assert_eq!(plan.recorded_until, Some(ts(3 * SECOND)));
    assert_eq!(
        plan.judge_events.len(),
        2,
        "the later legacy hit remains recorded despite audio suppression"
    );
    assert_eq!(
        plan.judge_events,
        legacy
            .reports
            .iter()
            .flat_map(|report| report.judge_events.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(legacy.reports[1].hazard_events[0].value, 1295);
    let mut feeder = BgmFeeder::from_output_commands(plan.commands, config()).unwrap();
    assert_eq!(
        feeder.admitted_stops(),
        0,
        "a planned Stop is not queue evidence"
    );
    let (mut producer, mut mixer) = output(prepared, 4);
    let first = feeder
        .feed(0, 4, |command| producer.try_push(command))
        .unwrap();
    assert_eq!(
        (first.total_admitted, first.remaining, first.outstanding),
        (3, 1, 3)
    );
    assert_eq!(feeder.admitted_stops(), 2);
    let mut completion = ReplayCompletion::new(ClockDomainId(2), 10);
    let mut first_pcm = [0.0; 10];
    let first_render = mixer.render(&mut first_pcm).unwrap();
    assert_eq!(
        first_pcm,
        [1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    );
    assert_eq!(first_render.counters.commands_applied, 1);
    assert_eq!(first_render.counters.unknown_stops, 0);
    let first_cursor = completed_render_cursor_for_feeder(&first_render, &feeder).unwrap();
    assert_eq!(first_cursor, 10);
    feeder
        .feed(first_cursor, 4, |command| producer.try_push(command))
        .unwrap();
    assert!(
        !completion
            .observe(
                false,
                feeder.report(),
                Some(first_render),
                Some(point(SECOND))
            )
            .unwrap()
    );
    assert!(
        !completion
            .observe(
                true,
                feeder.report(),
                Some(first_render),
                Some(point(SECOND))
            )
            .unwrap()
    );

    let mut silence = [9.0; 10];
    let stopped = mixer.render(&mut silence).unwrap();
    assert_eq!(silence, [0.0; 10]);
    assert_eq!(
        (
            stopped.counters.commands_applied,
            stopped.counters.unknown_stops
        ),
        (3, 2)
    );
    assert!(
        matches!(completed_render_cursor(&stopped), Err(ReplayAudioError::RejectedRender(raw)) if raw == stopped)
    );
    assert_eq!(
        completed_render_cursor_for_feeder(&stopped, &feeder).unwrap(),
        20
    );
    let rolling = feeder
        .feed(20, 4, |command| producer.try_push(command))
        .unwrap();
    assert_eq!(
        (
            rolling.total_admitted,
            rolling.remaining,
            rolling.outstanding
        ),
        (4, 0, 1)
    );
    assert_eq!(feeder.admitted_stops(), 2);
    assert!(
        !completion
            .observe(true, rolling, Some(stopped), Some(point(2 * SECOND)))
            .unwrap()
    );
    let before_bgm = mixer.render(&mut silence).unwrap();
    assert_eq!(silence, [0.0; 10]);
    feeder
        .feed(
            completed_render_cursor_for_feeder(&before_bgm, &feeder).unwrap(),
            4,
            |command| producer.try_push(command),
        )
        .unwrap();
    assert_eq!(
        feeder.report().outstanding,
        1,
        "target equal to cursor cannot retire before execution"
    );
    assert!(
        !completion
            .observe(
                true,
                feeder.report(),
                Some(before_bgm),
                Some(point(3 * SECOND))
            )
            .unwrap()
    );

    let mut music = [0.0; 2];
    let tail = mixer.render(&mut music).unwrap();
    assert_eq!(
        music,
        [0.25, 0.75],
        "failure Stops leave independent BGM intact"
    );
    assert_eq!(
        (
            tail.counters.commands_consumed,
            tail.counters.commands_applied
        ),
        (4, 4)
    );
    feeder
        .feed(
            completed_render_cursor_for_feeder(&tail, &feeder).unwrap(),
            4,
            |command| producer.try_push(command),
        )
        .unwrap();
    assert_eq!(feeder.report().outstanding, 0);
    assert!(
        !completion
            .observe(
                true,
                feeder.report(),
                Some(tail),
                Some(point(3_200_000_000))
            )
            .unwrap()
    );
    let idle = mixer.render(&mut [0.0]).unwrap();
    assert!(
        !completion
            .observe(
                true,
                feeder.report(),
                Some(idle),
                Some(point(3_200_000_000))
            )
            .unwrap()
    );
    assert!(
        completion
            .observe(
                true,
                feeder.report(),
                Some(idle),
                Some(point(3_300_000_000))
            )
            .unwrap()
    );
    assert!(
        completion
            .observe(
                true,
                feeder.report(),
                Some(idle),
                Some(point(3_300_000_000))
            )
            .unwrap()
    );
    assert_eq!(feeder.admitted_stops(), 2);
    assert_eq!(
        mixer.counters().unknown_stops,
        2,
        "completion never normalizes raw counters"
    );
}

#[test]
fn refused_stop_has_no_credit_and_owned_validation_preserves_every_other_execution_error() {
    let prepared = data(CHART, false);
    let legacy = recorded(
        &prepared,
        &[Action::Press(0, 91), Action::Advance(SECOND)],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let plan = plan_audio(
        &prepared,
        legacy.file,
        replay_limits(),
        point(0),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(
        plan.commands,
        [play(1, 11, 0), stop(11, SECOND), stop(12, SECOND)]
    );
    let mut feeder = BgmFeeder::from_output_commands(plan.commands, config()).unwrap();
    let empty = BgmFeeder::from_output_commands(Vec::new(), config()).unwrap();
    let (mut producer, mut mixer) = output(prepared, 2);
    match feeder
        .feed(0, 4, |command| producer.try_push(command))
        .unwrap_err()
    {
        BgmFeedError::Admission(error) => {
            assert_eq!(error.command, stop(12, SECOND));
            assert_eq!(error.reason, QueuePushError::Full);
        }
        other => panic!("expected actual producer refusal, got {other:?}"),
    }
    assert_eq!(feeder.admitted_stops(), 1);
    assert_eq!(
        (feeder.report().total_admitted, feeder.report().remaining),
        (2, 1)
    );
    let before = mixer.render(&mut [0.0; 10]).unwrap();
    assert_eq!(before.counters.unknown_stops, 0);
    assert_eq!(
        completed_render_cursor_for_feeder(&before, &feeder).unwrap(),
        10
    );
    // Explicitly retry only the unadmitted suffix while its exact frame is live.
    feeder
        .feed(10, 4, |command| producer.try_push(command))
        .unwrap();
    assert_eq!(feeder.admitted_stops(), 2);
    assert_eq!(producer.counters().accepted, 3);
    let raw = mixer.render(&mut [0.0]).unwrap();
    assert_eq!(
        (raw.counters.commands_applied, raw.counters.unknown_stops),
        (3, 2)
    );
    assert_eq!(
        completed_render_cursor_for_feeder(&raw, &feeder).unwrap(),
        11
    );
    rejected(raw, &empty);
    for counters in [
        AudioCounters {
            unknown_stops: 3,
            ..raw.counters
        },
        AudioCounters {
            commands_applied: 1,
            ..raw.counters
        },
        AudioCounters {
            late_commands: 1,
            ..raw.counters
        },
        AudioCounters {
            pending_full: 1,
            ..raw.counters
        },
        AudioCounters {
            voice_full: 1,
            ..raw.counters
        },
        AudioCounters {
            unknown_samples: 1,
            ..raw.counters
        },
        AudioCounters {
            invalid_gains: 1,
            ..raw.counters
        },
        AudioCounters {
            invalid_rates: 1,
            ..raw.counters
        },
        AudioCounters {
            invalid_times: 1,
            ..raw.counters
        },
    ] {
        rejected(RenderReport { counters, ..raw }, &feeder);
    }
    assert!(matches!(
        completed_render_cursor_for_feeder(
            &RenderReport {
                start_frame: u64::MAX,
                frames: 1,
                ..raw
            },
            &feeder
        ),
        Err(ReplayAudioError::Overflow)
    ));
    assert_eq!(feeder.admitted_stops(), 2);
    assert_eq!(producer.counters().accepted, 3);
    assert_eq!(mixer.counters(), raw.counters);

    let ordinary = data("#BPM 60\n#WAV01 note\n#00011:01", false);
    let capture = recorded(
        &ordinary,
        &[Action::Press(0, 91)],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let plan = plan_audio(
        &ordinary,
        capture.file,
        replay_limits(),
        point(0),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(plan.commands, [play(1, 11, 0)]);
    assert_eq!(plan.final_judge_hash, capture.hash);
    let mut feeder = BgmFeeder::from_output_commands(plan.commands, config()).unwrap();
    let (mut producer, mut mixer) = output(ordinary, 2);
    feeder
        .feed(0, 4, |command| producer.try_push(command))
        .unwrap();
    let mut pcm = [0.0; 3];
    let raw = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [1.0, -1.0, 0.0]);
    assert_eq!(feeder.admitted_stops(), 0);
    assert_eq!(completed_render_cursor(&raw).unwrap(), 3);
    assert_eq!(
        completed_render_cursor_for_feeder(&raw, &feeder).unwrap(),
        3
    );
}
