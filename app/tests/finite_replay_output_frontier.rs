//! Controlled output observations of actual mixer/converter execution, not hardware claims.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, ChannelMatrix, CommandProducer,
        Mixer, MixerConfig, PcmLimits, PcmSample, ResampleQuality, SampleBank, SampleId,
        TargetFrameBasis, VoiceId,
    },
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms_runtime::{
    bgm::{BgmConfig, BgmFeeder},
    finite_replay_completion::FiniteReplayCompletion,
    gameplay::output::ports::TargetOutputTelemetry,
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(22),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn config(origin: ClockPoint, rate: u32) -> BgmConfig {
    BgmConfig {
        output_origin: origin,
        sample_rate: rate,
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(1_000_000_000),
        max_pending: 8,
    }
}
fn rig(
    rate: u32,
    endpoint: u64,
    commands: Vec<AudioCommand>,
) -> (CommandProducer, Mixer, BgmFeeder) {
    let format = AudioFormat::new(rate, 1).unwrap();
    let pcm = PcmLimits::new(1024, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 100], pcm).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let feeder = BgmFeeder::from_output_commands(commands, config(point(0), rate)).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(22),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        )
        .with_playback_end_frame(endpoint),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer, feeder)
}
fn play() -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(1),
        sample: SampleId(1),
        at: Timestamp::ZERO,
        gain: 1.0,
    }
}

#[test]
fn finite_selected_replay_waits_for_actual_late_queue_retirement_and_source_presentation() {
    let (mut producer, mut mixer, mut feeder) = rig(1000, 1, vec![play()]);
    let mut completion = FiniteReplayCompletion::new(point(0), 1000, 1).unwrap();
    let marker = mixer.render(&mut [0.0; 2]).unwrap();
    // Planning/queue publication races the actual final callback, without
    // rewriting original replay targets or manufacturing consumer receipts.
    feeder.feed(0, 8, |c| producer.try_push(c)).unwrap();
    feeder.retire_completed(1).unwrap();
    assert!(!completion
        .observe(true, &feeder, Some(marker), Some(point(1_000_000)))
        .unwrap());
    assert!(!completion.output_finished(&feeder).unwrap());
    let mut pcm = [1.0; 1];
    let settled = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0]);
    assert_eq!(settled.counters.commands_consumed, 1);
    assert_eq!(settled.counters.commands_applied, 0);
    assert_eq!(settled.counters.commands_expired, 1);
    assert_eq!(settled.song_position, marker.song_position);
    assert_eq!(settled.active_voices, marker.active_voices);
    let mut unpresented = FiniteReplayCompletion::new(point(0), 1000, 1).unwrap();
    assert!(!unpresented
        .observe(true, &feeder, Some(marker), None)
        .unwrap());
    assert!(!unpresented
        .observe(true, &feeder, Some(settled), None)
        .unwrap());
    assert!(
        !unpresented.output_finished(&feeder).unwrap(),
        "actual expiry and source render completion still require presentation"
    );
    assert!(!unpresented
        .observe(true, &feeder, None, Some(point(999_999)))
        .unwrap());
    assert!(!unpresented.output_finished(&feeder).unwrap());
    assert!(unpresented
        .observe(true, &feeder, None, Some(point(1_000_000)))
        .unwrap());
    assert!(unpresented.output_finished(&feeder).unwrap());
    assert!(completion
        .observe(true, &feeder, Some(settled), Some(point(1_000_000)))
        .unwrap());
    assert!(completion.output_finished(&feeder).unwrap());
}

#[test]
fn genuine_fractional_fence_finishes_output_without_finishing_original_records() {
    let (mut producer, mut mixer, mut feeder) = rig(3, 1, vec![play()]);
    feeder
        .feed(0, 8, |command| producer.try_push(command))
        .unwrap();
    let mut completion = FiniteReplayCompletion::new(point(0), 3, 1).unwrap();
    assert!(!completion.output_finished(&feeder).unwrap());
    let mut pcm = [-1.0; 2];
    let terminal = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.0]);
    assert_eq!(terminal.playback_end_physical_frame, Some(1));
    assert_eq!(terminal.counters.commands_applied, 1);
    feeder.retire_completed(1).unwrap();
    assert!(!completion
        .observe(false, &feeder, Some(terminal), None)
        .unwrap());
    assert!(
        !completion.output_finished(&feeder).unwrap(),
        "render-ahead is not presentation"
    );
    assert!(!completion
        .observe(false, &feeder, None, Some(point(333_333_333)))
        .unwrap());
    assert!(
        !completion.output_finished(&feeder).unwrap(),
        "fractional frame requires ceil timestamp"
    );
    assert!(!completion
        .observe(false, &feeder, None, Some(point(333_333_334)))
        .unwrap());
    assert!(completion.output_finished(&feeder).unwrap());
    assert!(
        !completion.observe(false, &feeder, None, None).unwrap(),
        "record completion remains independent"
    );
    assert!(completion.observe(true, &feeder, None, None).unwrap());
}

#[test]
fn zero_quantized_endpoint_still_requires_real_core_marker_and_adopted_presentation() {
    let (_producer, mut mixer, feeder) = rig(10, 0, vec![]);
    let mut completion = FiniteReplayCompletion::new(point(0), 10, 0).unwrap();
    assert!(!completion
        .observe(false, &feeder, None, Some(point(0)))
        .unwrap());
    assert!(
        !completion.output_finished(&feeder).unwrap(),
        "presentation without rendering is not adopted"
    );
    let mut pcm = [1.0; 2];
    let terminal = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0; 2]);
    assert_eq!(terminal.playback_end_physical_frame, Some(0));
    assert_eq!(terminal.counters.commands_applied, 0);
    assert!(!completion
        .observe(false, &feeder, Some(terminal), None)
        .unwrap());
    assert!(!completion.output_finished(&feeder).unwrap());
    assert!(!completion
        .observe(false, &feeder, None, Some(point(0)))
        .unwrap());
    assert!(completion.output_finished(&feeder).unwrap());
    assert!(!completion.observe(false, &feeder, None, None).unwrap());
}

#[test]
fn actual_execution_does_not_retire_the_producers_outstanding_credit() {
    let (mut producer, mut mixer, mut feeder) = rig(10, 3, vec![play()]);
    feeder
        .feed(0, 8, |command| producer.try_push(command))
        .unwrap();
    let terminal = mixer.render(&mut [0.0; 4]).unwrap();
    let mut completion = FiniteReplayCompletion::new(point(0), 10, 3).unwrap();
    assert!(!completion
        .observe(false, &feeder, Some(terminal), Some(point(300_000_000)))
        .unwrap());
    assert_eq!(feeder.report().outstanding, 1);
    assert!(!completion.output_finished(&feeder).unwrap());
    feeder.retire_completed(3).unwrap();
    assert!(completion.output_finished(&feeder).unwrap());
}

#[test]
fn unadmitted_plan_never_finishes_even_if_empty_mixer_reaches_its_endpoint() {
    let (_producer, mut mixer, feeder) = rig(10, 3, vec![play()]);
    let terminal = mixer.render(&mut [0.0; 4]).unwrap();
    assert_eq!(terminal.counters.commands_applied, 0);
    let mut completion = FiniteReplayCompletion::new(point(0), 10, 3).unwrap();
    assert!(!completion
        .observe(false, &feeder, Some(terminal), Some(point(300_000_000)))
        .unwrap());
    assert_eq!(feeder.report().remaining, 1);
    assert!(!completion.output_finished(&feeder).unwrap());
}

#[test]
fn query_refuses_changed_origin_rate_and_actual_execution_count_atomically() {
    let (mut producer, mut mixer, mut feeder) = rig(10, 3, vec![play()]);
    feeder
        .feed(0, 8, |command| producer.try_push(command))
        .unwrap();
    let terminal = mixer.render(&mut [0.0; 4]).unwrap();
    feeder.retire_completed(3).unwrap();
    let mut completion = FiniteReplayCompletion::new(point(0), 10, 3).unwrap();
    completion
        .observe(false, &feeder, Some(terminal), Some(point(300_000_000)))
        .unwrap();
    let before = completion.clone();
    for other_config in [config(point(1), 10), config(point(0), 11)] {
        let other = BgmFeeder::from_output_commands(vec![], other_config).unwrap();
        assert!(completion.output_finished(&other).is_err());
        assert_eq!(completion, before);
    }
    // This separate producer admitted no command: the retained real execution of
    // one command cannot stand in for this producer's zero-command plan.
    let other = BgmFeeder::from_output_commands(vec![], config(point(0), 10)).unwrap();
    assert!(completion.output_finished(&other).is_err());
    assert_eq!(completion, before);
    assert!(completion.output_finished(&feeder).unwrap());
    assert!(completion
        .observe(false, &feeder, None, Some(point(400_000_001)))
        .is_err());
    assert_eq!(completion, before);
}

fn target_pair(basis: TargetFrameBasis, frame: u64) -> ClockPair {
    // Declared native observations on this converter's original creation grid.
    // These bracket real generated/admitted output; they are not host polling.
    ClockPair {
        source: basis.point_at_stream_frame(frame).unwrap(),
        target: ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(
                1_000_000_000 + (frame * 1_000_000_000 / 48_000) as i64,
            ),
        },
    }
}

#[test]
fn converted_frontier_needs_original_target_crossing_not_completed_source_alone() {
    let (mut producer, mixer, mut feeder) = rig(44_100, 3, vec![play()]);
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
    .unwrap_or_else(|_| panic!("actual converter setup"));
    let basis = output.target_frame_basis();
    let mut completion = FiniteReplayCompletion::new(point(0), 44_100, 3)
        .unwrap()
        .with_target_basis(7, basis, ClockDomainId(1))
        .unwrap();
    assert!(!completion
        .observe_target(false, &feeder, 7, basis, None, Some(target_pair(basis, 0)))
        .unwrap());
    let converted = output.render_pending(8).unwrap();
    output.admit(8).unwrap();
    let telemetry = TargetOutputTelemetry {
        source: output.last_real_source_report(),
        converted: Some(converted),
        facts: output.boundaries(),
    };
    assert_eq!(telemetry.facts.end.unwrap().source_frame, 3);
    assert_eq!(telemetry.source.unwrap().counters.commands_applied, 1);
    feeder.retire_completed(3).unwrap();
    assert!(!completion
        .observe_target(false, &feeder, 7, basis, Some(telemetry), None)
        .unwrap());
    assert!(!completion.output_finished(&feeder).unwrap());
    let before = completion.clone();
    assert!(completion
        .observe(false, &feeder, None, Some(point(68_028)))
        .is_err());
    assert_eq!(completion, before);
    assert!(completion
        .observe_target(false, &feeder, 8, basis, None, Some(target_pair(basis, 5)))
        .is_err());
    assert_eq!(completion, before);
    assert!(!completion
        .observe_target(false, &feeder, 7, basis, None, Some(target_pair(basis, 3)))
        .unwrap());
    assert!(!completion.output_finished(&feeder).unwrap());
    assert!(!completion
        .observe_target(false, &feeder, 7, basis, None, Some(target_pair(basis, 5)))
        .unwrap());
    assert!(completion.output_finished(&feeder).unwrap());
    assert!(!completion
        .observe_target(false, &feeder, 7, basis, None, None)
        .unwrap());
    assert!(completion
        .observe_target(true, &feeder, 7, basis, None, None)
        .unwrap());
}
