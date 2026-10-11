//! Literal song-grid expectations from real mixer/converter reports.
//! Supplied presentation brackets are controlled port evidence, not hardware accuracy.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioCounters, AudioFormat, AudioLimits, ChannelMatrix,
        CommandProducer, Mixer, MixerConfig, PcmLimits, PcmSample, RenderReport, ResampleQuality,
        SampleBank, SampleId, TargetFrameBasis, VoiceId,
    },
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, Timestamp},
    transport::Rate,
};
use beatkernel_bms_runtime::{
    native_start::StartInterval,
    playback_pause::{PauseIntervalObservation, PausePhase},
    replay_pause::ReplayPause,
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn pair(ns: i64) -> ClockPair {
    ClockPair {
        source: point(11, 123_456 + ns),
        target: point(22, 1_000_000_000 + ns),
    }
}
fn rig(hz: u32, speed: Rate, origin: i64) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(hz, 1).unwrap();
    let limits = AudioLimits::new(8, 4, 8, 64, 8).unwrap();
    let pcm = PcmLimits::new(512, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, (1..=128).map(|n| n as f32 / 256.0).collect(), pcm).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::SetRate {
            at: Timestamp::from_nanos(origin),
            rate: speed,
        })
        .unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(origin),
            gain: 1.0,
        })
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(11),
            Timestamp::from_nanos(origin),
            limits,
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn owner(speed: Rate) -> ReplayPause {
    ReplayPause::new_with_rate(
        point(11, 123_456),
        ClockDomainId(22),
        1000,
        Timestamp::from_nanos(50_000_000),
        Duration::from_nanos(3_000_000),
        speed,
    )
    .unwrap()
}
fn song(owner: &ReplayPause, elapsed: i64) -> Option<i64> {
    owner
        .presentation_song(point(11, 123_456 + elapsed))
        .unwrap()
        .map(|s| s.as_nanos())
}
fn speeds() -> [(Rate, i64, i64, i64); 4] {
    [
        (Rate::new(1, 2).unwrap(), 48_500_000, 50_500_000, 53_500_000),
        (Rate::NORMAL, 47_000_000, 51_000_000, 57_000_000),
        (Rate::new(2, 1).unwrap(), 44_000_000, 52_000_000, 64_000_000),
        (Rate::new(3, 2).unwrap(), 45_500_000, 51_500_000, 60_500_000),
    ]
}

#[test]
fn source_repeated_real_pauses_remove_output_gap_before_scaling_and_preserve_resume_floor() {
    for (speed, initial, frozen_song, final_song) in speeds() {
        let (mut producer, mut mixer) = rig(1000, speed, 123_456);
        let mut pause = owner(speed);
        assert_eq!(song(&pause, 0), Some(initial));
        let active = mixer.render(&mut [0.0; 4]).unwrap();
        assert_eq!(active.counters.commands_applied, 2);
        pause.observe(Some(active), pair(0)).unwrap();
        assert!(pause.request(true, pair(3_000_000)).unwrap());
        producer.request_pause(true);
        let mut silence = [99.0; 3];
        let frozen = mixer.render(&mut silence).unwrap();
        assert_eq!(silence, [0.0; 3]);
        assert!(pause
            .observe(Some(frozen), pair(3_000_000))
            .unwrap()
            .is_none());
        assert_eq!(song(&pause, 7_000_000), None);
        let boundary = pause.observe(None, pair(4_000_000)).unwrap().unwrap();
        assert!(boundary.paused);
        assert_eq!(boundary.song.as_nanos(), frozen_song);
        pause.request(false, pair(6_000_000)).unwrap();
        producer.request_pause(false);
        let resumed = mixer.render(&mut [0.0; 2]).unwrap();
        let boundary = pause
            .observe(Some(resumed), pair(9_000_000))
            .unwrap()
            .unwrap();
        assert!(!boundary.paused);
        assert_eq!(boundary.song.as_nanos(), frozen_song);
        assert_eq!(song(&pause, 6_999_999), None);
        assert_eq!(song(&pause, 7_000_000), Some(frozen_song));
        pause.request(true, pair(9_000_000)).unwrap();
        producer.request_pause(true);
        let second = mixer.render(&mut [0.0; 2]).unwrap();
        pause
            .observe(Some(second), pair(10_000_000))
            .unwrap()
            .unwrap();
        pause.request(false, pair(10_000_000)).unwrap();
        producer.request_pause(false);
        let resumed = mixer.render(&mut [0.0; 4]).unwrap();
        pause
            .observe(Some(resumed), pair(15_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(pause.phase(), PausePhase::Running);
        assert_eq!(song(&pause, 10_999_999), None);
        assert_eq!(song(&pause, 15_000_000), Some(final_song));
    }
}

fn interval(render: RenderReport, before: i64, after: i64) -> PauseIntervalObservation {
    PauseIntervalObservation {
        output_origin: point(11, 123_456),
        sample_rate: 1000,
        render,
        clock: StartInterval::new(
            point(11, 123_456 + render.start_frame as i64 * 1_000_000),
            point(22, before),
            point(22, after),
        )
        .unwrap(),
    }
}

#[test]
fn assessed_interval_coalesced_real_reports_keep_first_boundary_and_original_upper_bracket() {
    for (speed, _, frozen_song, _) in speeds() {
        let (mut producer, mut mixer) = rig(1000, speed, 123_456);
        let first = interval(mixer.render(&mut [0.0; 4]).unwrap(), 100, 120);
        let mut pause = owner(speed);
        pause.request_interval(true, first).unwrap();
        producer.request_pause(true);
        mixer.render(&mut [0.0; 3]).unwrap(); // Unobserved first paused block.
        let frozen = interval(mixer.render(&mut [0.0; 2]).unwrap(), 300, 340);
        assert!(pause
            .observe_interval(Some(frozen), point(22, 339))
            .unwrap()
            .is_none());
        let boundary = pause
            .observe_interval(None, point(22, 340))
            .unwrap()
            .unwrap();
        assert_eq!(boundary.song.as_nanos(), frozen_song);
        assert_eq!(boundary.host.earliest(), point(22, 100));
        assert_eq!(boundary.host.latest(), point(22, 340));
        assert_eq!(song(&pause, 9_000_000), None);
        pause.request_interval(false, frozen).unwrap();
        producer.request_pause(false);
        mixer.render(&mut [0.0; 2]).unwrap(); // Unobserved first resumed block.
        let resumed = interval(mixer.render(&mut [0.0; 3]).unwrap(), 460, 500);
        assert!(pause
            .observe_interval(Some(resumed), point(22, 499))
            .unwrap()
            .is_none());
        let boundary = pause
            .observe_interval(None, point(22, 500))
            .unwrap()
            .unwrap();
        assert_eq!(boundary.song.as_nanos(), frozen_song);
        assert!(!boundary.paused);
        assert_eq!(boundary.host.earliest(), point(22, 300));
        assert_eq!(boundary.host.latest(), point(22, 500));
        assert_eq!(song(&pause, 8_999_999), None);
        assert_eq!(song(&pause, 9_000_000), Some(frozen_song));
    }
}

#[test]
fn default_constructor_and_explicit_normal_match_actual_pause_and_presentation() {
    let mut old = ReplayPause::new(
        point(11, 123_456),
        ClockDomainId(22),
        1000,
        Timestamp::from_nanos(50_000_000),
        Duration::from_nanos(3_000_000),
    )
    .unwrap();
    let mut explicit = owner(Rate::NORMAL);
    let (mut producer, mut mixer) = rig(1000, Rate::NORMAL, 123_456);
    let active = mixer.render(&mut [0.0; 4]).unwrap();
    for replay in [&mut old, &mut explicit] {
        replay.observe(Some(active), pair(0)).unwrap();
        replay.request(true, pair(3_000_000)).unwrap();
    }
    producer.request_pause(true);
    let frozen = mixer.render(&mut [0.0; 3]).unwrap();
    assert_eq!(
        old.observe(Some(frozen), pair(7_000_000)).unwrap(),
        explicit.observe(Some(frozen), pair(7_000_000)).unwrap()
    );
    assert_eq!(song(&old, 7_000_000), song(&explicit, 7_000_000));
}

fn target_pair(basis: TargetFrameBasis, frame: u64) -> ClockPair {
    ClockPair {
        source: basis.point_at_stream_frame(frame).unwrap(),
        target: point(22, 1_000_000_000 + (frame * 1_000_000_000 / 48_000) as i64),
    }
}

#[test]
fn actual_converter_source_pause_requires_target_crossing_and_keeps_selected_song_rate() {
    for (speed, expected, later) in [
        (Rate::new(1, 2).unwrap(), 48_541_667, 48_562_500),
        (Rate::NORMAL, 47_083_333, 47_125_000),
        (Rate::new(2, 1).unwrap(), 44_166_666, 44_250_000),
        (Rate::new(3, 2).unwrap(), 45_625_000, 45_687_500),
    ] {
        let (mut producer, mixer) = rig(24_000, speed, 0);
        let mut output = ConvertedNativeOutputState::new(
            mixer,
            DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            8,
        )
        .unwrap_or_else(|_| panic!("converter setup"));
        let basis = output.target_frame_basis();
        let mut pause = ReplayPause::new_with_rate(
            point(11, 0),
            ClockDomainId(22),
            24_000,
            Timestamp::from_nanos(50_000_000),
            Duration::from_nanos(3_000_000),
            speed,
        )
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
        let active = output.render_pending(1).unwrap();
        output.admit(1).unwrap();
        pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                active.source,
                target_pair(basis, 0),
            )
            .unwrap();
        pause
            .request_target(true, 7, basis, output.boundaries(), target_pair(basis, 1))
            .unwrap();
        producer.request_pause(true);
        output.render_pending(8).unwrap();
        let facts = output.boundaries();
        let source = output.last_real_source_report();
        assert!(source.unwrap().paused);
        assert_eq!(
            facts.pause.unwrap().target_time,
            beatkernel::audio::TargetTime::from_frames(4, 48_000).unwrap()
        );
        assert!(pause
            .observe_target(7, basis, facts, source, target_pair(basis, 3))
            .unwrap()
            .is_none());
        assert_eq!(pause.phase(), PausePhase::Pausing);
        assert!(pause
            .observe_target(8, basis, facts, source, target_pair(basis, 4))
            .is_err());
        let mut wrong = facts;
        wrong.origin = Some(point(11, 1));
        assert!(pause
            .observe_target(7, basis, wrong, source, target_pair(basis, 4))
            .is_err());
        let boundary = pause
            .observe_target(7, basis, facts, source, target_pair(basis, 4))
            .unwrap()
            .unwrap();
        assert!(boundary.paused);
        assert_eq!(boundary.song.as_nanos(), expected);
        assert_eq!(pause.phase(), PausePhase::Paused);
        assert!(pause
            .observe_target(7, basis, facts, source, target_pair(basis, 3))
            .is_err());
        output.admit(8).unwrap();
        pause
            .request_target(false, 7, basis, output.boundaries(), target_pair(basis, 9))
            .unwrap();
        producer.request_pause(false);
        let held = output.render_held_pending(4).unwrap();
        assert!(held.source.is_none());
        assert!(pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                target_pair(basis, 13)
            )
            .unwrap()
            .is_none());
        assert_eq!(pause.phase(), PausePhase::Resuming);
        output.admit(4).unwrap();
        let mut saw_unmapped_resume = false;
        for _ in 0..8 {
            let rendered = output.render_pending(1).unwrap();
            output.admit(1).unwrap();
            if rendered.resume_source_frame.is_some() && output.boundaries().resume.is_none() {
                saw_unmapped_resume = true;
                assert!(pause
                    .observe_target(
                        7,
                        basis,
                        output.boundaries(),
                        output.last_real_source_report(),
                        target_pair(basis, 13)
                    )
                    .unwrap()
                    .is_none());
            }
            if output.boundaries().resume.is_some() {
                break;
            }
        }
        assert!(saw_unmapped_resume);
        assert_eq!(
            output.boundaries().resume.unwrap().target_time,
            beatkernel::audio::TargetTime::from_frames(16, 48_000).unwrap()
        );
        assert!(pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                target_pair(basis, 15)
            )
            .unwrap()
            .is_none());
        let resumed = pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                target_pair(basis, 16),
            )
            .unwrap()
            .unwrap();
        assert!(!resumed.paused);
        assert_eq!(resumed.song.as_nanos(), expected);
        assert_eq!(pause.presentation_song(point(11, 333_332)).unwrap(), None);
        assert_eq!(
            pause
                .presentation_song(point(11, 375_000))
                .unwrap()
                .unwrap()
                .as_nanos(),
            later
        );
    }
}

#[test]
fn rejected_source_authority_leaves_real_transition_available_for_retry() {
    let (mut producer, mut mixer) = rig(1000, Rate::new(3, 2).unwrap(), 123_456);
    let mut pause = owner(Rate::new(3, 2).unwrap());
    let first = mixer.render(&mut [0.0; 4]).unwrap();
    pause.observe(Some(first), pair(0)).unwrap();
    pause.request(true, pair(3_000_000)).unwrap();
    producer.request_pause(true);
    let frozen = mixer.render(&mut [0.0; 2]).unwrap();
    let old = pause.last_render_report();
    let mut wrong = pair(9_000_000);
    wrong.source.domain = ClockDomainId(99);
    assert!(pause.observe(Some(frozen), wrong).is_err());
    assert_eq!(pause.phase(), PausePhase::Pausing);
    assert_eq!(pause.last_render_report(), old);
    assert!(pause.observe(None, pair(3_000_000)).unwrap().is_none());
    assert_eq!(
        pause
            .observe(Some(frozen), pair(6_000_000))
            .unwrap()
            .unwrap()
            .song
            .as_nanos(),
        51_500_000
    );
    assert!(pause.observe(None, pair(5_000_000)).is_err());
    assert_eq!(pause.phase(), PausePhase::Paused);
    assert!(pause.presentation_song(point(99, 0)).is_err());
    assert!(pause.presentation_song(point(11, 123_455)).is_err());
}

#[test]
fn finite_cap_precedes_narrowing_for_presentation_and_real_source_pause_boundary() {
    let speed = Rate::new(i64::MAX, 1).unwrap();
    let fresh = || {
        ReplayPause::new_with_rate(
            point(11, 123_456),
            ClockDomainId(22),
            1000,
            Timestamp::from_nanos(i64::MAX - 2),
            Duration::ZERO,
            speed,
        )
        .unwrap()
    };
    let mut uncapped = fresh();
    assert!(uncapped.presentation_song(point(11, 123_457)).is_err());
    let capped = fresh()
        .with_song_end(Timestamp::from_nanos(i64::MAX))
        .unwrap();
    assert_eq!(
        capped.presentation_song(point(11, 123_457)).unwrap(),
        Some(Timestamp::from_nanos(i64::MAX))
    );
    let (mut producer, mut mixer) = rig(1000, speed, 123_456);
    let active = mixer.render(&mut [0.0; 1]).unwrap();
    uncapped.observe(Some(active), pair(0)).unwrap();
    uncapped.request(true, pair(0)).unwrap();
    let mut capped = capped;
    capped.observe(Some(active), pair(0)).unwrap();
    capped.request(true, pair(0)).unwrap();
    producer.request_pause(true);
    let frozen = mixer.render(&mut [0.0; 1]).unwrap();
    assert!(uncapped.observe(Some(frozen), pair(2_000_000)).is_err());
    assert_eq!(uncapped.phase(), PausePhase::Pausing);
    assert_eq!(uncapped.last_render_report(), Some(active));
    assert!(uncapped.observe(Some(frozen), pair(2_000_000)).is_err());
    assert_eq!(
        capped
            .observe(Some(frozen), pair(2_000_000))
            .unwrap()
            .unwrap()
            .song
            .as_nanos(),
        i64::MAX
    );
    assert_eq!(capped.phase(), PausePhase::Paused);
}

#[test]
fn interval_overflow_refuses_ack_atomically_while_finite_cap_accepts_the_same_real_boundary() {
    let speed = Rate::new(i64::MAX, 1).unwrap();
    let fresh = || {
        ReplayPause::new_with_rate(
            point(11, 123_456),
            ClockDomainId(22),
            1000,
            Timestamp::from_nanos(i64::MAX - 2),
            Duration::ZERO,
            speed,
        )
        .unwrap()
    };
    let (mut producer, mut mixer) = rig(1000, speed, 123_456);
    let first = interval(mixer.render(&mut [0.0; 1]).unwrap(), 100, 120);
    let mut uncapped = fresh();
    let mut capped = fresh()
        .with_song_end(Timestamp::from_nanos(i64::MAX))
        .unwrap();
    uncapped.request_interval(true, first).unwrap();
    capped.request_interval(true, first).unwrap();
    producer.request_pause(true);
    let frozen = interval(mixer.render(&mut [0.0; 1]).unwrap(), 200, 240);
    assert!(uncapped
        .observe_interval(Some(frozen), point(22, 240))
        .is_err());
    assert_eq!(uncapped.last_render_report(), Some(first.render));
    assert_eq!(uncapped.phase(), PausePhase::Pausing);
    assert!(uncapped
        .observe_interval(Some(frozen), point(22, 239))
        .unwrap()
        .is_none());
    assert!(uncapped.observe_interval(None, point(22, 240)).is_err());
    assert_eq!(
        capped
            .observe_interval(Some(frozen), point(22, 240))
            .unwrap()
            .unwrap()
            .song
            .as_nanos(),
        i64::MAX
    );
}

#[test]
fn converted_boundary_finite_cap_uses_wide_song_value_and_uncapped_failure_keeps_source_prefix() {
    let speed = Rate::new(i64::MAX, 1).unwrap();
    let (mut producer, mixer) = rig(24_000, speed, 0);
    let mut output = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        8,
    )
    .unwrap_or_else(|_| panic!("converter setup"));
    let basis = output.target_frame_basis();
    let fresh = || {
        ReplayPause::new_with_rate(
            point(11, 0),
            ClockDomainId(22),
            24_000,
            Timestamp::from_nanos(i64::MAX - 2),
            Duration::ZERO,
            speed,
        )
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap()
    };
    let mut uncapped = fresh();
    let mut capped = fresh()
        .with_song_end(Timestamp::from_nanos(i64::MAX))
        .unwrap();
    let active = output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    for pause in [&mut uncapped, &mut capped] {
        pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                active.source,
                target_pair(basis, 0),
            )
            .unwrap();
        pause
            .request_target(true, 7, basis, output.boundaries(), target_pair(basis, 1))
            .unwrap();
    }
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    let source = output.last_real_source_report();
    let facts = output.boundaries();
    let old = uncapped.last_render_report();
    assert!(uncapped
        .observe_target(7, basis, facts, source, target_pair(basis, 4))
        .is_err());
    assert_eq!(uncapped.phase(), PausePhase::Pausing);
    assert_eq!(uncapped.last_render_report(), old);
    // Repeating the same genuine evidence must still fail in mapping, without adopting its ACK.
    assert!(uncapped
        .observe_target(7, basis, facts, source, target_pair(basis, 4))
        .is_err());
    let boundary = capped
        .observe_target(7, basis, facts, source, target_pair(basis, 4))
        .unwrap()
        .unwrap();
    assert_eq!(boundary.song.as_nanos(), i64::MAX);
    assert!(boundary.paused);
}

#[test]
fn repeated_noninteger_source_grid_rounds_cumulative_output_gap_once_before_rate_scaling() {
    let speed = Rate::new(3, 2).unwrap();
    let (mut producer, mut mixer) = rig(3, speed, 0);
    let mut pause = ReplayPause::new_with_rate(
        point(11, 0),
        ClockDomainId(22),
        3,
        Timestamp::from_nanos(5),
        Duration::from_nanos(2),
        speed,
    )
    .unwrap();
    let make_pair = |ns| ClockPair {
        source: point(11, ns),
        target: point(22, ns + 10_000),
    };
    let active = mixer.render(&mut [0.0; 1]).unwrap();
    pause.observe(Some(active), make_pair(0)).unwrap();
    let mut reference = make_pair(0);
    for (physical_pause, physical_resume, crossing_pause, crossing_resume, expected) in [
        (1, 2, 333_333_343, 666_666_676, 500_000_001),
        (3, 4, 1_000_000_010, 1_333_333_343, 1_000_000_001),
        (5, 6, 1_666_666_676, 2_000_000_010, 1_500_000_002),
    ] {
        pause.request(true, reference).unwrap();
        producer.request_pause(true);
        let frozen = mixer.render(&mut [0.0; 1]).unwrap();
        assert_eq!(frozen.start_frame, physical_pause);
        let boundary = pause
            .observe(Some(frozen), make_pair(crossing_pause))
            .unwrap()
            .unwrap();
        assert_eq!(boundary.song.as_nanos(), expected);
        pause.request(false, make_pair(crossing_pause)).unwrap();
        producer.request_pause(false);
        let resumed = mixer.render(&mut [0.0; 1]).unwrap();
        assert_eq!(resumed.start_frame, physical_resume);
        reference = make_pair(crossing_resume);
        let boundary = pause.observe(Some(resumed), reference).unwrap().unwrap();
        assert_eq!(boundary.song.as_nanos(), expected);
    }
    assert_eq!(
        pause.presentation_song(point(11, 2_000_000_000)).unwrap(),
        Some(Timestamp::from_nanos(1_500_000_002))
    );
}

#[test]
fn original_end_is_cold_validated_once_and_failed_authority_does_not_lock_setup() {
    let fresh = || owner(Rate::new(3, 2).unwrap());
    assert!(fresh()
        .with_song_end(Timestamp::from_nanos(49_999_999))
        .is_err());
    let capped = fresh()
        .with_song_end(Timestamp::from_nanos(50_000_000))
        .unwrap();
    assert!(capped
        .with_song_end(Timestamp::from_nanos(51_000_000))
        .is_err());
    let mut observed = fresh();
    observed.observe(None, pair(0)).unwrap();
    assert!(observed
        .with_song_end(Timestamp::from_nanos(51_000_000))
        .is_err());
    let mut requested = fresh();
    requested.request(true, pair(0)).unwrap();
    assert!(requested
        .with_song_end(Timestamp::from_nanos(51_000_000))
        .is_err());
    let mut rejected = fresh();
    let mut wrong = pair(0);
    wrong.target.domain = ClockDomainId(99);
    assert!(rejected.observe(None, wrong).is_err());
    assert!(rejected
        .with_song_end(Timestamp::from_nanos(51_000_000))
        .is_ok());
    let pure = fresh();
    assert_eq!(song(&pure, 0), Some(45_500_000));
    assert!(pure
        .with_song_end(Timestamp::from_nanos(51_000_000))
        .is_ok());
}

#[test]
fn invalid_selected_rate_or_grid_and_negative_preroll_refuse_setup() {
    for speed in [Rate::new(0, 1).unwrap(), Rate::new(-1, 1).unwrap()] {
        assert!(ReplayPause::new_with_rate(
            point(11, 0),
            ClockDomainId(22),
            1000,
            Timestamp::ZERO,
            Duration::ZERO,
            speed
        )
        .is_err());
    }
    assert!(ReplayPause::new_with_rate(
        point(11, 0),
        ClockDomainId(22),
        0,
        Timestamp::ZERO,
        Duration::ZERO,
        Rate::NORMAL
    )
    .is_err());
    assert!(ReplayPause::new_with_rate(
        point(11, 0),
        ClockDomainId(11),
        1000,
        Timestamp::ZERO,
        Duration::ZERO,
        Rate::NORMAL
    )
    .is_err());
    assert!(ReplayPause::new_with_rate(
        point(11, 0),
        ClockDomainId(22),
        1000,
        Timestamp::ZERO,
        Duration::from_nanos(-1),
        Rate::NORMAL
    )
    .is_err());
}

#[test]
fn replay_pause_remains_send_and_sync_without_interior_mutable_clock_history() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<ReplayPause>();
}

#[test]
fn controlled_full_signed_span_gap_cancels_before_timestamp_narrowing() {
    // Supplementary declared business-port extent: executing 2^63 audio frames is
    // intentionally not claimed. Ordinary tests above establish actual Mixer PCM.
    let origin = point(11, i64::MIN);
    let mut pause = ReplayPause::new_with_rate(
        origin,
        ClockDomainId(22),
        1_000_000_000,
        Timestamp::from_nanos(50_000_000),
        Duration::from_nanos(3_000_000),
        Rate::new(3, 2).unwrap(),
    )
    .unwrap();
    let first_pair = ClockPair {
        source: origin,
        target: point(22, 0),
    };
    pause.request(true, first_pair).unwrap();
    let paused = RenderReport {
        start_frame: 0,
        frames: 1,
        playback_start_frame: 0,
        playback_frames: 0,
        paused: true,
        playback_end_physical_frame: None,
        active_voices: 0,
        pending_commands: 0,
        song_position: Timestamp::ZERO,
        producer_disconnected: false,
        counters: AudioCounters {
            rendered_frames: 1,
            ..AudioCounters::default()
        },
    };
    // Adoption arrives after the request in both original clock domains.
    let adopted_pair = ClockPair {
        source: point(11, i64::MIN + 1),
        target: point(22, 1),
    };
    let frozen = pause.observe(Some(paused), adopted_pair).unwrap().unwrap();
    assert_eq!(frozen.song.as_nanos(), 45_500_000);
    pause.request(false, adopted_pair).unwrap();
    let huge_gap = i64::MAX as u64 + 2;
    let resumed = RenderReport {
        start_frame: huge_gap,
        frames: 1,
        playback_start_frame: 0,
        playback_frames: 1,
        paused: false,
        counters: AudioCounters {
            rendered_frames: huge_gap + 1,
            ..AudioCounters::default()
        },
        ..paused
    };
    let crossing = ClockPair {
        source: point(11, 1),
        target: point(22, 3),
    };
    let boundary = pause.observe(Some(resumed), crossing).unwrap().unwrap();
    assert!(!boundary.paused);
    assert_eq!(boundary.song.as_nanos(), 45_500_000);
    assert_eq!(pause.presentation_song(point(11, 0)).unwrap(), None);
    assert_eq!(
        pause.presentation_song(point(11, 1)).unwrap(),
        Some(Timestamp::from_nanos(45_500_000))
    );
    assert_eq!(
        pause.presentation_song(point(11, 3)).unwrap(),
        Some(Timestamp::from_nanos(45_500_003))
    );
}
