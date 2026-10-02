//! Interval pause contracts exercised with actual Mixer reports and command ownership.
use super::*;
use crate::{native_start::interval::StartInterval, replay_pause::ReplayPause};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        PcmSample, SampleBank, SampleId, VoiceId, command_queue, command_queue_with_start_gate,
    },
    time::Duration,
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn frame_ns(frame: u64, rate: u32) -> i64 {
    i64::try_from(i128::from(frame) * 1_000_000_000 / i128::from(rate)).unwrap()
}
fn observation(
    render: RenderReport,
    rate: u32,
    origin: i64,
    before: i64,
    after: i64,
) -> PauseIntervalObservation {
    PauseIntervalObservation {
        output_origin: point(11, origin),
        sample_rate: rate,
        render,
        clock: StartInterval::new(
            point(11, origin + frame_ns(render.start_frame, rate)),
            point(22, before),
            point(22, after),
        )
        .unwrap(),
    }
}
fn mixer(rate: u32, origin: i64, end: Option<u64>, gated: bool) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(rate, 1).unwrap();
    let limits = AudioLimits::new(8, 4, 8, 64, 8).unwrap();
    let pcm = PcmLimits::new(256, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            format,
            (1..=128).map(|value| value as f32 / 256.0).collect(),
            pcm,
        )
        .unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = if gated {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(origin),
            gain: 1.0,
        })
        .unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(11),
        Timestamp::from_nanos(origin),
        limits,
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn assert_window(boundary: IntervalPauseBoundary, before: i64, after: i64) {
    assert_eq!(boundary.host.earliest(), point(22, before));
    assert_eq!(boundary.host.latest(), point(22, after));
}

#[test]
fn exact_anchor_pause_waits_for_original_upper_bound_and_freezes_pcm_and_commands() {
    let (mut producer, mut mixer) = mixer(1000, 0, None, false);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(4_000_000),
            gain: 1.0,
        })
        .unwrap();
    let mut samples = [99.0; 4];
    let first = observation(mixer.render(&mut samples).unwrap(), 1000, 0, 100, 120);
    assert_eq!(
        samples,
        [1.0 / 256.0, 2.0 / 256.0, 3.0 / 256.0, 4.0 / 256.0]
    );
    assert_eq!(first.render.counters.commands_applied, 1);
    assert_eq!(first.render.pending_commands, 1);
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    assert!(pause.request_interval(true, first).unwrap());
    producer.request_pause(true);
    let mut silence = [99.0; 3];
    let frozen = observation(mixer.render(&mut silence).unwrap(), 1000, 0, 200, 240);
    assert_eq!(silence, [0.0; 3]);
    assert_eq!(
        (
            frozen.render.start_frame,
            frozen.render.playback_start_frame
        ),
        (4, 4)
    );
    assert_eq!(frozen.render.playback_frames, 0);
    assert_eq!(frozen.render.counters.commands_applied, 1);
    assert_eq!(frozen.render.pending_commands, 1);
    assert!(
        pause
            .observe_interval(Some(frozen), point(22, 239))
            .unwrap()
            .is_none()
    );
    let refreshed = PauseIntervalObservation {
        clock: StartInterval {
            before: point(22, 210),
            after: point(22, 999),
            ..frozen.clock
        },
        ..frozen
    };
    assert!(
        pause
            .observe_interval(Some(refreshed), point(22, 239))
            .unwrap()
            .is_none()
    );
    let boundary = pause
        .observe_interval(None, point(22, 240))
        .unwrap()
        .unwrap();
    assert_window(boundary, 200, 240);
    assert!(boundary.paused);
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (4, 4));
    assert_eq!(pause.phase(), PausePhase::Paused);
    assert!(
        pause
            .observe_interval(None, point(22, 240))
            .unwrap()
            .is_none()
    );

    assert!(pause.request_interval(false, frozen).unwrap());
    producer.request_pause(false);
    let mut sound = [99.0; 2];
    let resumed = observation(mixer.render(&mut sound).unwrap(), 1000, 0, 300, 340);
    assert_eq!(sound, [6.0 / 256.0, 8.0 / 256.0]);
    assert_eq!(resumed.render.counters.commands_applied, 2);
    assert_eq!(resumed.render.pending_commands, 0);
    assert!(
        pause
            .observe_interval(Some(resumed), point(22, 339))
            .unwrap()
            .is_none()
    );
    let boundary = pause
        .observe_interval(None, point(22, 340))
        .unwrap()
        .unwrap();
    assert_window(boundary, 300, 340);
    assert!(!boundary.paused);
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (7, 4));
    assert_eq!(pause.phase(), PausePhase::Running);
    assert_eq!(
        pause
            .song_origin_after_pause(Timestamp::ZERO)
            .unwrap()
            .as_nanos(),
        -3_000_000
    );
    assert_eq!(
        pause.scheduling_point(resumed.render).unwrap(),
        point(11, 6_000_000)
    );
}

#[test]
fn coalesced_reports_recover_first_frames_and_keep_the_genuine_bracket_without_interpolation() {
    let (mut producer, mut mixer) = mixer(1000, 0, None, false);
    let first = observation(mixer.render(&mut [0.0; 4]).unwrap(), 1000, 0, 100, 120);
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    pause.request_interval(true, first).unwrap();
    producer.request_pause(true);
    let unseen = mixer.render(&mut [0.0; 3]).unwrap();
    assert_eq!(unseen.start_frame, 4);
    let crossed = observation(mixer.render(&mut [0.0; 2]).unwrap(), 1000, 0, 300, 340);
    assert_eq!(crossed.render.start_frame, 7);
    assert!(
        pause
            .observe_interval(Some(crossed), point(22, 339))
            .unwrap()
            .is_none()
    );
    let boundary = pause
        .observe_interval(None, point(22, 340))
        .unwrap()
        .unwrap();
    assert_window(boundary, 100, 340);
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (4, 4));
    let latest = observation(mixer.render(&mut [0.0; 3]).unwrap(), 1000, 0, 350, 360);
    assert!(
        pause
            .observe_interval(Some(latest), point(22, 360))
            .unwrap()
            .is_none()
    );
    pause.request_interval(false, latest).unwrap();
    producer.request_pause(false);
    let unseen = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!((unseen.start_frame, unseen.playback_start_frame), (12, 4));
    let crossed = observation(mixer.render(&mut [0.0; 3]).unwrap(), 1000, 0, 460, 500);
    assert_eq!(
        (
            crossed.render.start_frame,
            crossed.render.playback_start_frame
        ),
        (14, 6)
    );
    assert!(
        pause
            .observe_interval(Some(crossed), point(22, 499))
            .unwrap()
            .is_none()
    );
    let boundary = pause
        .observe_interval(None, point(22, 500))
        .unwrap()
        .unwrap();
    assert_window(boundary, 350, 500);
    assert!(!boundary.paused);
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (12, 4));
    assert_eq!(
        pause
            .song_origin_after_pause(Timestamp::ZERO)
            .unwrap()
            .as_nanos(),
        -8_000_000
    );
    assert_eq!(
        pause.scheduling_point(crossed.render).unwrap(),
        point(11, 9_000_000)
    );
}

#[test]
fn upper_host_plateaus_and_later_reports_do_not_replace_the_first_boundary_window() {
    let (mut producer, mut mixer) = mixer(1000, 0, None, false);
    let first = observation(mixer.render(&mut [0.0; 4]).unwrap(), 1000, 0, 100, 120);
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    pause.request_interval(true, first).unwrap();
    assert!(
        pause
            .observe_interval(None, point(22, 110))
            .unwrap()
            .is_none()
    );
    producer.request_pause(true);
    let frozen = observation(mixer.render(&mut [0.0; 2]).unwrap(), 1000, 0, 110, 120);
    assert!(
        pause
            .observe_interval(Some(frozen), point(22, 119))
            .unwrap()
            .is_none()
    );
    let later = observation(mixer.render(&mut [0.0; 2]).unwrap(), 1000, 0, 110, 120);
    assert!(
        pause
            .observe_interval(Some(later), point(22, 119))
            .unwrap()
            .is_none()
    );
    let boundary = pause
        .observe_interval(None, point(22, 120))
        .unwrap()
        .unwrap();
    assert_window(boundary, 110, 120);
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (4, 4));
    assert_eq!(pause.last_render_report(), Some(later.render));
}

#[test]
fn repeated_44100_hz_gaps_round_once_and_replay_ignores_pre_resume_presentation() {
    let rate = 44_100;
    let origin = 123_456;
    let (mut producer, mut mixer) = mixer(rate, origin, None, false);
    let mut reference = observation(mixer.render(&mut [0.0; 1]).unwrap(), rate, origin, 100, 120);
    let mut pause = NativePause::new(point(11, origin), ClockDomainId(22), rate).unwrap();
    let mut replay = ReplayPause::new(
        point(11, origin),
        ClockDomainId(22),
        rate,
        Timestamp::from_nanos(5_000_000),
        Duration::from_nanos(2_000_000),
    )
    .unwrap();
    assert_eq!(
        replay
            .presentation_song(point(11, origin))
            .unwrap()
            .unwrap()
            .as_nanos(),
        3_000_000
    );
    for cycle in 0..3u64 {
        pause.request_interval(true, reference).unwrap();
        replay.request_interval(true, reference).unwrap();
        producer.request_pause(true);
        let physical = 2 * cycle + 1;
        let lower = 200 + cycle as i64 * 200;
        let frozen = observation(
            mixer.render(&mut [0.0; 1]).unwrap(),
            rate,
            origin,
            lower,
            lower + 20,
        );
        let boundary = pause
            .observe_interval(Some(frozen), point(22, lower + 20))
            .unwrap()
            .unwrap();
        let song = replay
            .observe_interval(Some(frozen), point(22, lower + 20))
            .unwrap()
            .unwrap();
        assert!(song.paused);
        assert_eq!(song.host, boundary.host);
        assert_eq!(
            (boundary.physical_frame, boundary.playback_frame),
            (physical, cycle + 1)
        );
        assert_eq!(song.song.as_nanos(), 3_000_000 + frame_ns(cycle + 1, rate));
        assert_eq!(replay.presentation_song(frozen.clock.output).unwrap(), None);
        pause.request_interval(false, frozen).unwrap();
        replay.request_interval(false, frozen).unwrap();
        producer.request_pause(false);
        reference = observation(
            mixer.render(&mut [0.0; 1]).unwrap(),
            rate,
            origin,
            lower + 100,
            lower + 120,
        );
        let boundary = pause
            .observe_interval(Some(reference), point(22, lower + 120))
            .unwrap()
            .unwrap();
        let resumed = replay
            .observe_interval(Some(reference), point(22, lower + 120))
            .unwrap()
            .unwrap();
        assert!(!resumed.paused);
        assert_eq!(resumed.song, song.song);
        assert_eq!(resumed.host, boundary.host);
        assert_eq!(boundary.physical_frame, physical + 1);
        let resume_ns = origin + frame_ns(physical + 1, rate);
        assert_eq!(
            replay.presentation_song(point(11, resume_ns - 1)).unwrap(),
            None
        );
        assert_eq!(replay.presentation_song(point(11, origin)).unwrap(), None);
        assert_eq!(
            replay
                .presentation_song(point(11, resume_ns))
                .unwrap()
                .unwrap()
                .as_nanos(),
            3_000_000 + frame_ns(physical + 1, rate) - frame_ns(cycle + 1, rate)
        );
    }
    assert_eq!(
        pause
            .song_origin_after_pause(Timestamp::ZERO)
            .unwrap()
            .as_nanos(),
        -68_027
    );
    assert_eq!(replay.phase(), PausePhase::Running);
    assert_eq!(
        replay
            .presentation_song(point(11, origin + frame_ns(6, rate)))
            .unwrap()
            .unwrap()
            .as_nanos(),
        3_068_027
    );
}

#[test]
fn interval_metadata_chronology_and_mixed_modes_reject_atomically_before_valid_retry() {
    let (mut producer, mut mixer) = mixer(1000, 0, None, false);
    let first = observation(mixer.render(&mut [0.0; 4]).unwrap(), 1000, 0, 100, 120);
    producer.request_pause(true);
    let valid = observation(mixer.render(&mut [0.0; 2]).unwrap(), 1000, 0, 200, 240);
    let fresh = || NativePause::new(point(11, 0), ClockDomainId(22), 1000).unwrap();
    let mut baseline = fresh();
    baseline
        .observe_interval(Some(first), point(22, 120))
        .unwrap();
    baseline.request_interval(true, first).unwrap();
    let mut malformed = Vec::new();
    let mut value = valid;
    value.output_origin = point(11, 1);
    malformed.push(value);
    let mut value = valid;
    value.sample_rate = 0;
    malformed.push(value);
    let mut value = valid;
    value.sample_rate = 999;
    malformed.push(value);
    let mut value = valid;
    value.clock.output = point(11, 4_000_001);
    malformed.push(value);
    let mut value = valid;
    value.clock.output.domain = ClockDomainId(99);
    malformed.push(value);
    let mut value = valid;
    value.clock.before.domain = ClockDomainId(99);
    malformed.push(value);
    let mut value = valid;
    value.clock.after = point(22, 199);
    malformed.push(value);
    let mut value = valid;
    value.clock.before = point(22, 99);
    value.clock.after = point(22, 119);
    malformed.push(value);
    let mut value = valid;
    value.render.frames = 0;
    malformed.push(value);
    let mut value = valid;
    value.render.counters.rendered_frames -= 1;
    malformed.push(value);
    let mut value = valid;
    value.render.start_frame = u64::MAX;
    malformed.push(value);
    let mut value = valid;
    value.render.playback_start_frame = 3;
    malformed.push(value);
    let mut value = valid;
    value.render.playback_frames = 3;
    malformed.push(value);
    let mut value = first;
    value.render.active_voices += 1;
    malformed.push(value);
    for (index, malformed) in malformed.into_iter().enumerate() {
        let mut attempt = baseline.clone();
        assert!(
            attempt
                .observe_interval(Some(malformed), point(22, 1000))
                .is_err(),
            "case {index}"
        );
        assert_eq!(attempt.phase(), baseline.phase());
        assert_eq!(attempt.last_render_report(), baseline.last_render_report());
        assert_eq!(
            attempt.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::ZERO
        );
        // The failed call must not retain host time 1000, a bad report or a window.
        let boundary = attempt
            .observe_interval(Some(valid), point(22, 240))
            .unwrap()
            .unwrap();
        assert_window(boundary, 200, 240);
        assert_eq!((boundary.physical_frame, boundary.playback_frame), (4, 4));
    }
    for now in [point(99, 240), point(22, 119)] {
        let mut attempt = baseline.clone();
        assert!(attempt.observe_interval(Some(valid), now).is_err());
        assert_eq!(attempt.last_render_report(), baseline.last_render_report());
        assert!(
            attempt
                .observe_interval(Some(valid), point(22, 240))
                .unwrap()
                .is_some()
        );
    }
    let pair = ClockPair {
        source: point(11, 0),
        target: point(22, 120),
    };
    let mut interval = fresh();
    interval
        .observe_interval(Some(first), point(22, 120))
        .unwrap();
    assert!(interval.request(true, pair).is_err());
    assert!(interval.observe(None, pair).is_err());
    assert_eq!(interval.phase(), PausePhase::Running);
    assert!(interval.request_interval(true, first).unwrap());
    let mut point_mode = fresh();
    point_mode.observe(None, pair).unwrap();
    assert!(point_mode.request_interval(true, first).is_err());
    assert!(
        point_mode
            .observe_interval(Some(first), point(22, 120))
            .is_err()
    );
    assert!(point_mode.request(true, pair).unwrap());
    let mut rejected = fresh();
    let bad = PauseIntervalObservation {
        sample_rate: 0,
        ..first
    };
    assert!(rejected.request_interval(true, bad).is_err());
    assert!(rejected.request(true, pair).unwrap());
    let mut locked = fresh();
    assert!(!locked.request_interval(false, first).unwrap());
    assert!(locked.clone().with_start_frame(4).is_err());
    assert!(locked.with_playback_end_frame(4).is_err());
}

#[test]
fn interval_startup_and_finite_markers_use_the_shared_logical_frame_rules() {
    for end in [0, 3] {
        let (mut producer, mut mixer) = mixer(1000, 0, Some(end), true);
        let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000)
            .unwrap()
            .with_start_frame(4)
            .unwrap()
            .with_playback_end_frame(end)
            .unwrap();
        let held = observation(mixer.render(&mut [0.0; 2]).unwrap(), 1000, 0, 100, 120);
        assert!(
            pause
                .observe_interval(Some(held), point(22, 120))
                .unwrap()
                .is_none()
        );
        assert_eq!(pause.phase(), PausePhase::Running);
        producer.schedule_start_at(4).unwrap();
        let crossed = observation(mixer.render(&mut [0.0; 6]).unwrap(), 1000, 0, 200, 240);
        assert_eq!(crossed.render.playback_end_physical_frame, Some(4 + end));
        assert!(
            pause
                .observe_interval(Some(crossed), point(22, 240))
                .unwrap()
                .is_none()
        );
        assert!(!pause.request_interval(true, crossed).unwrap());
        assert_eq!(pause.phase(), PausePhase::Running);
        assert_eq!(
            pause.scheduling_point(crossed.render).unwrap(),
            point(11, frame_ns(end, 1000))
        );
        assert_eq!(
            pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::ZERO
        );
    }

    let (_producer, mut mixer) = mixer(1000, 0, Some(3), false);
    let first = observation(mixer.render(&mut [0.0; 2]).unwrap(), 1000, 0, 100, 120);
    let mut pause = NativePause::new(point(11, 0), ClockDomainId(22), 1000)
        .unwrap()
        .with_playback_end_frame(3)
        .unwrap();
    pause.request_interval(true, first).unwrap();
    // The terminal marker wins while the host's manual request is pending.
    let terminal = observation(mixer.render(&mut [0.0; 4]).unwrap(), 1000, 0, 200, 240);
    assert_eq!(terminal.render.playback_end_physical_frame, Some(3));
    assert!(
        pause
            .observe_interval(Some(terminal), point(22, 240))
            .unwrap()
            .is_none()
    );
    assert!(
        pause
            .observe_interval(None, point(22, 300))
            .unwrap()
            .is_none()
    );
    let crossed = observation(mixer.render(&mut [0.0; 2]).unwrap(), 1000, 0, 320, 340);
    let boundary = pause
        .observe_interval(Some(crossed), point(22, 340))
        .unwrap()
        .unwrap();
    assert_window(boundary, 100, 340);
    assert_eq!((boundary.physical_frame, boundary.playback_frame), (3, 3));
    assert!(!pause.request_interval(false, crossed).unwrap());
    assert_eq!(
        pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
        Timestamp::ZERO
    );
}

#[test]
fn replay_boundary_song_overflow_does_not_commit_interval_ack_or_future_host_time() {
    let (mut producer, mut mixer) = mixer(1000, 0, None, false);
    let first = observation(mixer.render(&mut [0.0; 4]).unwrap(), 1000, 0, 100, 120);
    producer.request_pause(true);
    let frozen = observation(mixer.render(&mut [0.0; 2]).unwrap(), 1000, 0, 200, 240);
    let mut replay = ReplayPause::new(
        point(11, 0),
        ClockDomainId(22),
        1000,
        Timestamp::from_nanos(i64::MAX),
        Duration::ZERO,
    )
    .unwrap();
    replay.request_interval(true, first).unwrap();
    let previous = replay.last_render_report();
    assert!(
        replay
            .observe_interval(Some(frozen), point(22, 240))
            .is_err()
    );
    assert_eq!(replay.phase(), PausePhase::Pausing);
    assert_eq!(replay.last_render_report(), previous);
    assert!(
        replay
            .observe_interval(Some(frozen), point(22, 239))
            .unwrap()
            .is_none()
    );
    assert!(replay.observe_interval(None, point(22, 240)).is_err());
    assert_eq!(replay.phase(), PausePhase::Pausing);
    assert_eq!(replay.presentation_song(frozen.clock.output).unwrap(), None);
}
