//! Independent native glue checks with real producer/core/output facts.
use super::*;
use beatkernel::{
    audio::{AudioCommand, CommandProducer, PcmSample, SampleBank, SampleId, VoiceId},
    transport::Rate,
};
use beatkernel_bms_runtime::finite_replay_completion::FiniteReplayCompletion;

#[path = "../../../tests/support/replay_speed.rs"]
mod capture;

fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: OUTPUT,
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}

fn args(extra: &[&str]) -> Vec<String> {
    [
        "--chart",
        "missing.bms",
        "--replay",
        "missing.bkr",
        "--device",
        "device",
        "--rate",
        "48000",
        "--channels",
        "1",
    ]
    .into_iter()
    .chain(extra.iter().copied())
    .map(str::to_owned)
    .collect()
}

#[test]
fn syntax_normalizes_speed_and_preserves_hz_seconds_and_default() {
    for (fraction, expected) in [
        (None, Rate::NORMAL),
        (Some("8/8"), Rate::NORMAL),
        (Some("0006/0004"), Rate::new(3, 2).unwrap()),
        (
            Some("9223372036854775807/18446744073709551615"),
            Rate::new(i64::MAX, u64::MAX).unwrap(),
        ),
    ] {
        let mut args = args(&["--seconds", "2"]);
        if let Some(fraction) = fraction {
            args.extend(["--speed".to_owned(), fraction.to_owned()]);
        }
        let options = parse(&args, Backend::Windows).unwrap();
        assert_eq!(options.speed.rate(), expected);
        assert_eq!(options.format.sample_rate(), 48_000);
        assert_eq!(options.seconds, Some(2));
    }
    let options = parse(
        &args(&[
            "--speed",
            "2/1",
            "--output-rate",
            "44100",
            "--buffer-frames",
            "256",
            "--period-frames",
            "128",
        ]),
        Backend::Linux,
    )
    .unwrap();
    assert_eq!(options.speed.to_string(), "2/1");
    assert_eq!(options.format.sample_rate(), 48_000);
    assert_eq!(options.output_rate, Some(44_100));
    assert_eq!(options.seconds, None);
}

#[test]
fn fallback_maps_output_preroll_once_at_four_literal_rates() {
    for (num, den, expected) in [
        (1, 2, [4_875_000_000, 5_000_000_000, 5_125_000_000]),
        (1, 1, [4_750_000_000, 5_000_000_000, 5_250_000_000]),
        (2, 1, [4_500_000_000, 5_000_000_000, 5_500_000_000]),
        (3, 2, [4_625_000_000, 5_000_000_000, 5_375_000_000]),
    ] {
        for (elapsed, expected) in [0, 250_000_000, 500_000_000].into_iter().zip(expected) {
            assert_eq!(
                presentation_song_with_rate(
                    point(elapsed),
                    ts(5_000_000_000),
                    Duration::from_nanos(250_000_000),
                    Rate::new(num, den).unwrap(),
                    None
                )
                .unwrap(),
                ts(expected)
            );
        }
    }
    assert_eq!(
        presentation_song(
            point(500_000_000),
            ts(5_000_000_000),
            Duration::from_nanos(250_000_000)
        )
        .unwrap(),
        ts(5_250_000_000)
    );
}

#[test]
fn fallback_wide_cap_precedes_narrowing_and_preserves_signed_span() {
    let speed = Rate::new(i64::MAX, 1).unwrap();
    assert_eq!(
        presentation_song_with_rate(point(i64::MAX), ts(0), Duration::ZERO, speed, Some(ts(17)))
            .unwrap(),
        ts(17)
    );
    assert!(
        presentation_song_with_rate(point(i64::MAX), ts(0), Duration::ZERO, speed, None).is_err()
    );
    assert_eq!(
        presentation_song_with_rate(
            point(i64::MAX),
            ts(i64::MIN),
            Duration::ZERO,
            Rate::new(2, 1).unwrap(),
            None
        )
        .unwrap(),
        ts(i64::MAX - 1)
    );
    for p in [
        point(-1),
        ClockPoint {
            domain: HOST,
            timestamp: ts(0),
        },
    ] {
        assert!(
            presentation_song_with_rate(p, ts(0), Duration::ZERO, Rate::NORMAL, Some(ts(17)))
                .is_err()
        );
    }
}

fn rig(hz: u32, endpoint: u64, speed: Rate) -> (CommandProducer, Mixer, BgmFeeder) {
    let format = AudioFormat::new(hz, 1).unwrap();
    let limits = PcmLimits::new(1024, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 100], limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut commands = vec![];
    if endpoint != 0 {
        if speed != Rate::NORMAL {
            commands.push(AudioCommand::SetRate {
                at: ts(0),
                rate: speed,
            });
        }
        commands.push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: ts(0),
            gain: 1.0,
        });
    }
    let feeder = BgmFeeder::from_output_commands_with_rate(
        commands,
        BgmConfig {
            output_origin: point(0),
            sample_rate: hz,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(1_000_000_000),
            max_pending: 8,
        },
        speed,
    )
    .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            OUTPUT,
            ts(0),
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        )
        .with_playback_end_frame(endpoint),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer, feeder)
}

fn visual(end: i64) -> ReplayVisual {
    let prepared = capture::prepared("#BPM 60\n", 4, &[]);
    let file = capture::capture_events(
        &prepared,
        0,
        ts(0),
        Some(ts(end)),
        &beatkernel_bms_runtime::gauge::GaugeProfile::default(),
        &[],
        Some(end),
    );
    ReplayVisual::new_section(&prepared.source, &file, capture::limits()).unwrap()
}

#[test]
fn native_quantized_frontier_advances_actual_original_record_only_after_presented_retired_output() {
    let speed = Rate::new(3, 2).unwrap();
    let (mut producer, mut mixer, mut feeder) = rig(4, 1, speed);
    feeder
        .feed(0, 8, |command| producer.try_push(command))
        .unwrap();
    let mut completion = FiniteReplayCompletion::new(point(0), 4, 1).unwrap();
    let end = ts(375_000_001);
    let mut visual = visual(end.as_nanos());
    let mapped =
        presentation_song_with_rate(point(250_000_000), ts(0), Duration::ZERO, speed, Some(end))
            .unwrap();
    assert_eq!(mapped, ts(375_000_000));
    visual.advance_to(mapped).unwrap();
    assert!(!visual.finished());
    assert_eq!(
        finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, Some(end)).unwrap(),
        None
    );
    let mut pcm = [-1.0; 2];
    let terminal = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.0]);
    assert_eq!(terminal.counters.commands_applied, 2);
    assert!(!completion
        .observe(false, &feeder, Some(terminal), None)
        .unwrap());
    assert_eq!(
        finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, Some(end)).unwrap(),
        None,
        "render-ahead cannot advance records"
    );
    completion
        .observe(false, &feeder, None, Some(point(250_000_000)))
        .unwrap();
    assert_eq!(
        finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, Some(end)).unwrap(),
        None,
        "executed commands still require producer retirement"
    );
    feeder.retire_completed(1).unwrap();
    for phase in [
        PausePhase::Pausing,
        PausePhase::Paused,
        PausePhase::Resuming,
    ] {
        assert_eq!(
            finite_frontier_song(phase, Some(&completion), &feeder, Some(end)).unwrap(),
            None
        );
    }
    let genuine = finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, Some(end))
        .unwrap()
        .unwrap();
    assert_eq!(genuine, end);
    assert!(!completion.observe(false, &feeder, None, None).unwrap());
    visual.advance_to(genuine).unwrap();
    assert!(visual.finished());
    assert!(completion
        .observe(visual.finished(), &feeder, None, None)
        .unwrap());
    // --seconds may keep polling after natural output completion. The real
    // output frontier remains authoritative over the shorter rounded mapping.
    let silence = mixer.render(&mut [1.0; 2]).unwrap();
    completion
        .observe(false, &feeder, Some(silence), Some(point(250_000_000)))
        .unwrap();
    for _ in 0..3 {
        let song = finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, Some(end))
            .unwrap()
            .unwrap();
        assert_eq!(song, end);
        visual.advance_to(song).unwrap();
        assert!(visual.finished());
    }
}

#[test]
fn native_zero_fence_uses_actual_silent_core_marker_and_running_presentation() {
    let speed = Rate::new(2, 1).unwrap();
    let (_producer, mut mixer, feeder) = rig(4, 0, speed);
    let mut completion = FiniteReplayCompletion::new(point(0), 4, 0).unwrap();
    let end = ts(1);
    let mut visual = visual(1);
    visual
        .advance_to(
            presentation_song_with_rate(point(0), ts(0), Duration::ZERO, speed, Some(end)).unwrap(),
        )
        .unwrap();
    assert!(!visual.finished());
    completion
        .observe(false, &feeder, None, Some(point(0)))
        .unwrap();
    assert_eq!(
        finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, Some(end)).unwrap(),
        None
    );
    let mut pcm = [1.0; 2];
    let terminal = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0; 2]);
    assert_eq!(terminal.playback_end_physical_frame, Some(0));
    completion
        .observe(false, &feeder, Some(terminal), None)
        .unwrap();
    assert_eq!(
        finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, Some(end)).unwrap(),
        None
    );
    completion
        .observe(false, &feeder, None, Some(point(0)))
        .unwrap();
    let genuine = finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, Some(end))
        .unwrap()
        .unwrap();
    visual.advance_to(genuine).unwrap();
    assert!(visual.finished());
    let silence = mixer.render(&mut [1.0; 2]).unwrap();
    completion
        .observe(false, &feeder, Some(silence), Some(point(0)))
        .unwrap();
    for _ in 0..3 {
        assert_eq!(
            finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, Some(end))
                .unwrap(),
            Some(end)
        );
        visual.advance_to(end).unwrap();
    }
}

#[test]
fn native_frontier_requires_paired_finite_identity_even_when_paused() {
    let (_producer, _mixer, feeder) = rig(4, 0, Rate::NORMAL);
    let completion = FiniteReplayCompletion::new(point(0), 4, 0).unwrap();
    for phase in [PausePhase::Running, PausePhase::Paused] {
        assert!(finite_frontier_song(phase, Some(&completion), &feeder, None).is_err());
        assert!(finite_frontier_song(phase, None, &feeder, Some(ts(1))).is_err());
        assert_eq!(
            finite_frontier_song(phase, None, &feeder, None).unwrap(),
            None
        );
    }
}

#[test]
fn native_converted_frontier_cannot_use_source_presentation_as_target_crossing() {
    use beatkernel::audio::{ChannelMatrix, ResampleQuality};
    use beatkernel_bms_runtime::gameplay::output::ports::TargetOutputTelemetry;
    use beatkernel_platform::audio::ConvertedNativeOutputState;
    let (mut producer, mixer, mut feeder) = rig(44_100, 3, Rate::new(2, 1).unwrap());
    feeder
        .feed(0, 8, |command| producer.try_push(command))
        .unwrap();
    let mut output = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap_or_else(|_| panic!("converter setup"));
    let basis = output.target_frame_basis();
    let pair = |frame| ClockPair {
        source: basis.point_at_stream_frame(frame).unwrap(),
        target: ClockPoint {
            domain: HOST,
            timestamp: ts(1_000_000_000 + (frame * 1_000_000_000 / 48_000) as i64),
        },
    };
    let mut completion = FiniteReplayCompletion::new(point(0), 44_100, 3)
        .unwrap()
        .with_target_basis(7, basis, HOST)
        .unwrap();
    completion
        .observe_target(false, &feeder, 7, basis, None, Some(pair(0)))
        .unwrap();
    let converted = output.render_pending(8).unwrap();
    output.admit(8).unwrap();
    let telemetry = TargetOutputTelemetry {
        source: output.last_real_source_report(),
        converted: Some(converted),
        facts: output.boundaries(),
    };
    assert_eq!(telemetry.source.unwrap().counters.commands_applied, 2);
    feeder.retire_completed(3).unwrap();
    completion
        .observe_target(false, &feeder, 7, basis, Some(telemetry), None)
        .unwrap();
    let end = Some(ts(136_055));
    assert_eq!(
        finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, end).unwrap(),
        None
    );
    let before = completion.clone();
    assert!(completion
        .observe(false, &feeder, None, Some(point(68_028)))
        .is_err());
    assert_eq!(completion, before);
    assert!(completion
        .observe_target(false, &feeder, 8, basis, None, Some(pair(5)))
        .is_err());
    assert_eq!(completion, before);
    completion
        .observe_target(false, &feeder, 7, basis, None, Some(pair(3)))
        .unwrap();
    assert_eq!(
        finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, end).unwrap(),
        None
    );
    completion
        .observe_target(false, &feeder, 7, basis, None, Some(pair(5)))
        .unwrap();
    assert_eq!(
        finite_frontier_song(PausePhase::Paused, Some(&completion), &feeder, end).unwrap(),
        None
    );
    assert_eq!(
        finite_frontier_song(PausePhase::Running, Some(&completion), &feeder, end).unwrap(),
        end
    );
    assert!(!completion
        .observe_target(false, &feeder, 7, basis, None, None)
        .unwrap());
}
