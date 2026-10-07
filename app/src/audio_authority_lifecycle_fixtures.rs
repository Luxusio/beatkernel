//! Retained raw pause transitions from real Mixer acknowledgements and original intervals.
use super::*;
use crate::{native_start::StartInterval, playback_pause::PausePhase};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits,
        PcmSample, RenderReport, SampleBank, SampleId, VoiceId, command_queue,
    },
    time::ClockDomainId,
};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn raw(ns: i64) -> ClockPoint {
    point(11, ns)
}
fn host(ns: i64) -> ClockPoint {
    point(22, ns)
}
fn pair(ns: i64) -> LivePauseObservation {
    LivePauseObservation::Point(ClockPair {
        source: raw(ns),
        target: host(ns),
    })
}
fn mixer() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm = PcmLimits::new(256, 512, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 64], pcm).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    (
        producer,
        Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(11),
                Timestamp::ZERO,
                AudioLimits::new(8, 2, 8, 32, 8).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap(),
    )
}
fn update(
    p: &mut NativePause,
    evidence: LivePauseObservation,
    report: Option<RenderReport>,
    desired: Option<bool>,
) -> Result<AudioLivePauseUpdate, PauseError> {
    update_live_audio_pause(p, evidence, report, desired, Timestamp::ZERO, 1000)
}
fn observed(render: RenderReport, before: i64, after: i64) -> PauseIntervalObservation {
    // This is the original block's known physical start on a 1000-Hz grid.
    PauseIntervalObservation {
        output_origin: raw(0),
        sample_rate: 1000,
        render,
        clock: StartInterval::new(
            raw(render.start_frame as i64 * 1_000_000),
            host(before),
            host(after),
        )
        .unwrap(),
    }
}
fn interval(value: Option<PauseIntervalObservation>, now: i64) -> LivePauseObservation {
    LivePauseObservation::Interval {
        observation: value,
        now: host(now),
    }
}

#[test]
fn point_pause_resume_keeps_actual_physical_marker_separate_from_playback_song() {
    let (mut producer, mut mixer) = mixer();
    let first = mixer.render(&mut [0.0; 4]).unwrap();
    let mut p = NativePause::new(raw(0), ClockDomainId(22), 1000).unwrap();
    assert_eq!(p.last_transition_output().unwrap(), None);
    let request = update(&mut p, pair(4_000_000), Some(first), Some(true)).unwrap();
    assert_eq!(request.requested, Some(true));
    assert!(request.boundary.is_none());
    producer.request_pause(true);
    let mut silence = [99.0; 3];
    let frozen = mixer.render(&mut silence).unwrap();
    assert_eq!(silence, [0.0; 3]);
    let boundary = update(&mut p, pair(7_000_000), Some(frozen), None)
        .unwrap()
        .boundary
        .unwrap();
    assert_eq!(boundary.epoch, 0);
    assert_eq!(boundary.raw_output, raw(4_000_000));
    assert_eq!(boundary.original.playback_frame, 4);
    assert_eq!(boundary.original.song, Timestamp::from_nanos(4_000_000));
    assert_eq!(boundary.original.at, host(4_000_000));
    assert!(boundary.original.paused);
    assert_eq!(p.last_transition_output().unwrap(), Some(raw(4_000_000)));
    let later = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(mixer.frame_cursor(), 9);
    assert_eq!(mixer.playback_frame_cursor(), 4);
    let same = update(&mut p, pair(9_000_000), Some(later), Some(true)).unwrap();
    assert!(same.boundary.is_none());
    assert!(same.requested.is_none());
    assert_eq!(p.last_transition_output().unwrap(), Some(raw(4_000_000)));
    assert_eq!(
        update(&mut p, pair(9_000_000), Some(later), Some(false))
            .unwrap()
            .requested,
        Some(false)
    );
    producer.request_pause(false);
    let resumed = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!((resumed.start_frame, resumed.playback_start_frame), (9, 4));
    let boundary = update(&mut p, pair(11_000_000), Some(resumed), None)
        .unwrap()
        .boundary
        .unwrap();
    assert!(!boundary.original.paused);
    assert_eq!(boundary.raw_output, raw(9_000_000));
    assert_eq!(boundary.original.at, host(9_000_000));
    assert_eq!(boundary.original.playback_frame, 4);
    assert_eq!(boundary.original.song, Timestamp::from_nanos(4_000_000));
    assert_ne!(boundary.raw_output.timestamp, boundary.original.song);
    assert_eq!(p.phase(), PausePhase::Running);
    assert_eq!(p.last_transition_output().unwrap(), Some(raw(9_000_000)));
}

#[test]
fn asio_interval_ack_retains_conservative_host_endpoints_and_raw_start_of_transition() {
    let (mut producer, mut mixer) = mixer();
    let first = observed(mixer.render(&mut [0.0; 4]).unwrap(), 0, 100_000);
    let mut p = NativePause::new(raw(0), ClockDomainId(22), 1000).unwrap();
    let missing = update(&mut p, interval(None, 0), Some(first.render), Some(true)).unwrap();
    assert!(!missing.observed);
    assert!(missing.boundary.is_none());
    assert_eq!(p.last_transition_output().unwrap(), None);
    let request = update(
        &mut p,
        interval(Some(first), 100_000),
        Some(first.render),
        Some(true),
    )
    .unwrap();
    assert_eq!(request.requested, Some(true));
    producer.request_pause(true);
    let frozen = observed(mixer.render(&mut [0.0; 3]).unwrap(), 3_900_000, 4_100_000);
    let later = observed(mixer.render(&mut [0.0; 2]).unwrap(), 6_900_000, 7_100_000);
    assert!(
        update(
            &mut p,
            interval(Some(frozen), 4_099_999),
            Some(later.render),
            None
        )
        .unwrap()
        .boundary
        .is_none()
    );
    assert_eq!(p.last_transition_output().unwrap(), None);
    let boundary = update(&mut p, interval(None, 4_100_000), Some(later.render), None)
        .unwrap()
        .boundary
        .unwrap();
    assert_eq!(boundary.raw_output, raw(4_000_000));
    assert_eq!(boundary.original.window.earliest(), host(3_900_000));
    assert_eq!(boundary.original.window.latest(), host(4_100_000));
    assert_eq!(boundary.original.at, host(3_900_000));
    assert_eq!(boundary.original.song, Timestamp::from_nanos(4_000_000));
    let requested = update(
        &mut p,
        interval(Some(later), 7_100_000),
        Some(later.render),
        Some(false),
    )
    .unwrap();
    assert_eq!(requested.requested, Some(false));
    producer.request_pause(false);
    let resumed = observed(mixer.render(&mut [0.0; 2]).unwrap(), 8_900_000, 9_100_000);
    assert!(
        update(&mut p, interval(Some(resumed), 9_099_999), None, None)
            .unwrap()
            .boundary
            .is_none()
    );
    assert_eq!(p.last_transition_output().unwrap(), Some(raw(4_000_000)));
    let boundary = update(&mut p, interval(None, 9_100_000), None, None)
        .unwrap()
        .boundary
        .unwrap();
    assert_eq!(boundary.raw_output, raw(9_000_000));
    assert_eq!(boundary.original.window.earliest(), host(8_900_000));
    assert_eq!(boundary.original.window.latest(), host(9_100_000));
    assert_eq!(boundary.original.at, host(9_100_000));
    assert_eq!(boundary.original.playback_frame, 4);
    assert_eq!(boundary.original.song, Timestamp::from_nanos(4_000_000));
    assert_eq!(boundary.epoch, 0);
}

#[test]
fn malformed_and_conversion_refusals_leave_pending_ack_and_marker_atomic() {
    let (mut producer, mut mixer) = mixer();
    let first = mixer.render(&mut [0.0; 4]).unwrap();
    let mut p = NativePause::new(raw(0), ClockDomainId(22), 1000).unwrap();
    update(&mut p, pair(4_000_000), Some(first), Some(true)).unwrap();
    producer.request_pause(true);
    let frozen = mixer.render(&mut [0.0; 3]).unwrap();
    let before = format!("{:?}", p);
    let bad = LivePauseObservation::Point(ClockPair {
        source: raw(7_000_000),
        target: point(23, 7_000_000),
    });
    assert!(update(&mut p, bad, Some(frozen), None).is_err());
    assert_eq!(format!("{:?}", p), before);
    assert_eq!(p.last_transition_output().unwrap(), None);
    assert!(
        update_live_audio_pause(
            &mut p,
            pair(7_000_000),
            Some(frozen),
            None,
            Timestamp::from_nanos(i64::MAX - 3_000_000),
            1000
        )
        .is_err()
    );
    assert_eq!(format!("{:?}", p), before);
    assert_eq!(p.last_transition_output().unwrap(), None);
    assert!(
        update_live_audio_pause(
            &mut p,
            pair(7_000_000),
            Some(frozen),
            None,
            Timestamp::ZERO,
            0
        )
        .is_err()
    );
    assert_eq!(format!("{:?}", p), before);
    let boundary = update(&mut p, pair(7_000_000), Some(frozen), None)
        .unwrap()
        .boundary
        .unwrap();
    assert_eq!(boundary.raw_output, raw(4_000_000));
    let before = format!("{:?}", p);
    assert!(update(&mut p, pair(6_999_999), Some(frozen), None).is_err());
    assert_eq!(format!("{:?}", p), before);
    assert_eq!(p.last_transition_output().unwrap(), Some(raw(4_000_000)));
}

#[test]
fn successful_new_epoch_rebind_clears_marker_while_failed_rebind_retains_it() {
    let (mut producer, mut mixer) = mixer();
    let first = mixer.render(&mut [0.0; 4]).unwrap();
    let mut p = NativePause::new(raw(0), ClockDomainId(22), 1000).unwrap();
    update(&mut p, pair(4_000_000), Some(first), Some(true)).unwrap();
    producer.request_pause(true);
    let frozen = mixer.render(&mut [0.0; 3]).unwrap();
    update(&mut p, pair(7_000_000), Some(frozen), None).unwrap();
    let before = format!("{:?}", p);
    assert!(p.rebind_output(0, &mixer).is_err());
    assert_eq!(format!("{:?}", p), before);
    assert_eq!(p.last_transition_output().unwrap(), Some(raw(4_000_000)));
    p.rebind_output(1, &mixer).unwrap();
    assert_eq!(p.epoch(), 1);
    assert_eq!(p.last_transition_output().unwrap(), None);
    assert_eq!(p.phase(), PausePhase::Paused);
    let still = mixer.render(&mut [0.0; 2]).unwrap();
    let unchanged = update(&mut p, pair(9_000_000), Some(still), None).unwrap();
    assert!(unchanged.boundary.is_none());
    assert_eq!(p.last_transition_output().unwrap(), None);
    update(&mut p, pair(9_000_000), Some(still), Some(false)).unwrap();
    producer.request_pause(false);
    let resumed = mixer.render(&mut [0.0; 2]).unwrap();
    let boundary = update(&mut p, pair(11_000_000), Some(resumed), None)
        .unwrap()
        .boundary
        .unwrap();
    assert_eq!(boundary.epoch, 1);
    assert_eq!(boundary.raw_output, raw(9_000_000));
}

#[test]
fn repeated_real_cycles_replace_marker_only_when_each_transition_commits() {
    let (mut producer, mut mixer) = mixer();
    let first = mixer.render(&mut [0.0; 4]).unwrap();
    let mut p = NativePause::new(raw(0), ClockDomainId(22), 1000).unwrap();
    update(&mut p, pair(4_000_000), Some(first), Some(true)).unwrap();
    producer.request_pause(true);
    let frozen = mixer.render(&mut [0.0; 3]).unwrap();
    assert_eq!(
        update(&mut p, pair(7_000_000), Some(frozen), None)
            .unwrap()
            .boundary
            .unwrap()
            .raw_output,
        raw(4_000_000)
    );
    update(&mut p, pair(7_000_000), Some(frozen), Some(false)).unwrap();
    producer.request_pause(false);
    let resumed = mixer.render(&mut [0.0; 2]).unwrap();
    let resumed = update(&mut p, pair(9_000_000), Some(resumed), None)
        .unwrap()
        .boundary
        .unwrap();
    assert_eq!(resumed.raw_output, raw(7_000_000));
    let running = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(mixer.frame_cursor(), 11);
    assert_eq!(mixer.playback_frame_cursor(), 8);
    update(&mut p, pair(11_000_000), Some(running), Some(true)).unwrap();
    assert_eq!(p.last_transition_output().unwrap(), Some(raw(7_000_000)));
    producer.request_pause(true);
    let frozen = mixer.render(&mut [0.0; 3]).unwrap();
    let boundary = update(&mut p, pair(14_000_000), Some(frozen), None)
        .unwrap()
        .boundary
        .unwrap();
    assert_eq!(boundary.raw_output, raw(11_000_000));
    assert_eq!(boundary.original.playback_frame, 8);
    assert_eq!(boundary.original.song, Timestamp::from_nanos(8_000_000));
    assert_eq!(boundary.original.at, host(11_000_000));
    assert_eq!(p.last_transition_output().unwrap(), Some(raw(11_000_000)));
}

#[test]
fn portable_large_frame_boundary_keeps_full_width_raw_transition() {
    let (mut producer, mut mixer) = mixer();
    mixer.render(&mut [0.0; 4]).unwrap();
    producer.request_pause(true);
    let mut report = mixer.render(&mut [0.0; 3]).unwrap();
    assert_eq!(report.start_frame, 4);
    assert!(report.paused);
    // Seed the public native-record integer boundary from an actual paused
    // report. This is not a claim that this Mixer rendered billions of frames.
    let large = 4_294_967_313u64;
    let shift = large - 4;
    report.start_frame += shift;
    report.playback_start_frame += shift;
    report.counters.rendered_frames += shift;
    let mut p = NativePause::new(raw(0), ClockDomainId(22), 1_000_000_000).unwrap();
    let lower = LivePauseObservation::Point(ClockPair {
        source: raw(4_294_967_312),
        target: host(4_294_967_312),
    });
    let request = update_live_audio_pause(
        &mut p,
        lower,
        None,
        Some(true),
        Timestamp::ZERO,
        1_000_000_000,
    )
    .unwrap();
    assert_eq!(request.requested, Some(true));
    let upper = LivePauseObservation::Point(ClockPair {
        source: raw(4_294_967_316),
        target: host(4_294_967_316),
    });
    let boundary = update_live_audio_pause(
        &mut p,
        upper,
        Some(report),
        None,
        Timestamp::ZERO,
        1_000_000_000,
    )
    .unwrap()
    .boundary
    .unwrap();
    assert_eq!(boundary.raw_output, raw(4_294_967_313));
    assert_eq!(boundary.original.playback_frame, large);
    assert_eq!(boundary.original.at, host(4_294_967_313));
    assert_eq!(
        p.last_transition_output().unwrap(),
        Some(raw(4_294_967_313))
    );
}

fn original_host(ns: i64) -> ClockPoint {
    host(10_000_000_000 + ns)
}
fn logical(ns: i64) -> ClockPoint {
    point(33, 5_000_000_000 + ns)
}
fn mapped_pair(raw_ns: i64, host_ns: i64) -> LivePauseObservation {
    LivePauseObservation::Point(ClockPair {
        source: raw(raw_ns),
        target: original_host(host_ns),
    })
}
fn mapped_pause() -> (
    crate::audio_authority::AudioAuthority,
    crate::local_input::InputMerger,
    Transport,
    AudioLivePauseBoundary,
    NativePause,
    CommandProducer,
    Mixer,
) {
    use crate::audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch};
    let (mut producer, mut mixer) = mixer();
    let first = mixer.render(&mut [0.0; 4]).unwrap();
    let mut pause = NativePause::new(raw(0), ClockDomainId(22), 1000).unwrap();
    let request = update(
        &mut pause,
        mapped_pair(4_000_000, 8_000_000),
        Some(first),
        Some(true),
    )
    .unwrap();
    assert_eq!(request.requested, Some(true));
    producer.request_pause(true);
    let frozen = mixer.render(&mut [0.0; 3]).unwrap();
    let boundary = update(
        &mut pause,
        mapped_pair(7_000_000, 14_000_000),
        Some(frozen),
        None,
    )
    .unwrap()
    .boundary
    .unwrap();
    assert_eq!(boundary.raw_output, raw(4_000_000));
    assert_eq!(boundary.original.at, original_host(8_000_000));
    let mut authority = AudioAuthority::new(
        AudioAuthorityConfig::default(),
        AudioAuthorityEpoch {
            id: 0,
            stream_origin: raw(0),
            logical_origin: logical(0),
            host_domain: ClockDomainId(22),
        },
    )
    .unwrap();
    authority
        .observe(
            0,
            ClockPair {
                source: raw(4_000_000),
                target: original_host(8_000_000),
            },
        )
        .unwrap();
    authority
        .observe(
            0,
            ClockPair {
                source: raw(7_000_000),
                target: original_host(14_000_000),
            },
        )
        .unwrap();
    authority
        .record_acquired_prefix(original_host(14_000_000))
        .unwrap();
    let merger = crate::local_input::InputMerger::new(
        ClockDomainId(22),
        original_host(0),
        vec![beatkernel::input::DeviceId(9)],
        8,
    )
    .unwrap();
    let transport = Transport::new(
        logical(0).timestamp,
        Timestamp::ZERO,
        beatkernel::transport::Rate::NORMAL,
    );
    (
        authority, merger, transport, boundary, pause, producer, mixer,
    )
}

#[test]
fn prepared_pause_transport_uses_logical_raw_cutoff_and_preserves_active_owners() {
    let (authority, merger, transport, boundary, _pause, _producer, mixer) = mapped_pause();
    let control = authority
        .prepare_control_cutoff(
            boundary.epoch,
            boundary.raw_output,
            boundary.original.at,
            original_host(14_000_000),
            &merger,
        )
        .unwrap()
        .unwrap();
    assert_eq!(control.epoch(), 0);
    assert_eq!(control.host(), original_host(8_000_000));
    assert_eq!(control.raw_output(), raw(4_000_000));
    assert_eq!(control.output(), logical(4_000_000));
    let before_transport = format!("{:?}", transport);
    let before_authority = format!("{:?}", authority);
    let candidate = prepare_live_audio_transport(
        &transport,
        boundary,
        &control,
        Timestamp::from_nanos(3_000_000),
    )
    .unwrap();
    assert!(candidate.is_paused());
    assert_eq!(candidate.anchor().host_time, logical(4_000_000).timestamp);
    assert_eq!(
        candidate.position_at(logical(3_000_000).timestamp).unwrap(),
        Timestamp::from_nanos(3_000_000)
    );
    assert_eq!(
        candidate
            .position_at(logical(100_000_000).timestamp)
            .unwrap(),
        Timestamp::from_nanos(4_000_000)
    );
    assert_eq!(mixer.playback_frame_cursor(), 4);
    assert_eq!(format!("{:?}", transport), before_transport);
    assert_eq!(format!("{:?}", authority), before_authority);
    assert_eq!(authority.committed_operation(), None);
    assert_eq!(authority.committed_input_host(), None);
}

#[test]
fn prepared_resume_transport_keeps_frozen_song_until_actual_logical_physical_resume() {
    let (mut authority, merger, transport, boundary, mut pause, mut producer, mut mixer) =
        mapped_pause();
    let control = authority
        .prepare_control_cutoff(
            0,
            boundary.raw_output,
            boundary.original.at,
            original_host(14_000_000),
            &merger,
        )
        .unwrap()
        .unwrap();
    let paused_candidate = prepare_live_audio_transport(
        &transport,
        boundary,
        &control,
        Timestamp::from_nanos(4_000_000),
    )
    .unwrap();
    let later = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(mixer.frame_cursor(), 9);
    assert_eq!(mixer.playback_frame_cursor(), 4);
    let request = update(
        &mut pause,
        mapped_pair(9_000_000, 18_000_000),
        Some(later),
        Some(false),
    )
    .unwrap();
    assert_eq!(request.requested, Some(false));
    producer.request_pause(false);
    let resumed = mixer.render(&mut [0.0; 2]).unwrap();
    let boundary = update(
        &mut pause,
        mapped_pair(11_000_000, 22_000_000),
        Some(resumed),
        None,
    )
    .unwrap()
    .boundary
    .unwrap();
    assert_eq!(boundary.raw_output, raw(9_000_000));
    assert_eq!(boundary.original.at, original_host(18_000_000));
    assert_eq!(boundary.original.song, Timestamp::from_nanos(4_000_000));
    authority
        .observe(
            0,
            ClockPair {
                source: raw(11_000_000),
                target: original_host(22_000_000),
            },
        )
        .unwrap();
    authority
        .record_acquired_prefix(original_host(22_000_000))
        .unwrap();
    let control = authority
        .prepare_control_cutoff(
            0,
            boundary.raw_output,
            boundary.original.at,
            original_host(22_000_000),
            &merger,
        )
        .unwrap()
        .unwrap();
    assert_eq!(control.output(), logical(9_000_000));
    let before_transport = format!("{:?}", paused_candidate);
    let before_authority = format!("{:?}", authority);
    let running = prepare_live_audio_transport(
        &paused_candidate,
        boundary,
        &control,
        Timestamp::from_nanos(4_000_000),
    )
    .unwrap();
    assert!(!running.is_paused());
    assert_eq!(running.anchor().host_time, logical(9_000_000).timestamp);
    assert_eq!(
        running.position_at(logical(8_000_000).timestamp).unwrap(),
        Timestamp::from_nanos(4_000_000)
    );
    assert_eq!(
        running.position_at(logical(11_000_000).timestamp).unwrap(),
        Timestamp::from_nanos(6_000_000)
    );
    assert_eq!(mixer.playback_frame_cursor(), 6);
    assert_eq!(format!("{:?}", paused_candidate), before_transport);
    assert_eq!(format!("{:?}", authority), before_authority);
    assert_eq!(authority.committed_operation(), None);
    assert!(
        prepare_live_audio_transport(
            &paused_candidate,
            boundary,
            &control,
            Timestamp::from_nanos(3_000_000)
        )
        .is_err()
    );
    assert!(
        prepare_live_audio_transport(
            &transport,
            boundary,
            &control,
            Timestamp::from_nanos(4_000_000)
        )
        .is_err()
    );
    assert_eq!(format!("{:?}", paused_candidate), before_transport);
    assert_eq!(format!("{:?}", authority), before_authority);
}

#[test]
fn logical_transport_stage_refuses_boundary_token_identity_and_frozen_prefix_mismatch() {
    let (authority, merger, transport, boundary, _pause, _producer, _mixer) = mapped_pause();
    let control = authority
        .prepare_control_cutoff(
            0,
            boundary.raw_output,
            boundary.original.at,
            original_host(14_000_000),
            &merger,
        )
        .unwrap()
        .unwrap();
    let before_transport = format!("{:?}", transport);
    let before_authority = format!("{:?}", authority);
    let mut shifted_host = boundary;
    shifted_host.original.window =
        HostStartWindow::new(original_host(8_000_001), original_host(8_000_001)).unwrap();
    shifted_host.original.at = original_host(8_000_001);
    for invalid in [
        AudioLivePauseBoundary {
            epoch: 1,
            ..boundary
        },
        AudioLivePauseBoundary {
            raw_output: raw(4_000_001),
            ..boundary
        },
        AudioLivePauseBoundary {
            raw_output: point(44, 4_000_000),
            ..boundary
        },
        shifted_host,
    ] {
        assert!(
            prepare_live_audio_transport(
                &transport,
                invalid,
                &control,
                Timestamp::from_nanos(4_000_000)
            )
            .is_err()
        );
        assert_eq!(format!("{:?}", transport), before_transport);
        assert_eq!(format!("{:?}", authority), before_authority);
    }
    assert!(
        prepare_live_audio_transport(
            &transport,
            boundary,
            &control,
            Timestamp::from_nanos(4_000_001)
        )
        .is_err()
    );
    let candidate = prepare_live_audio_transport(
        &transport,
        boundary,
        &control,
        Timestamp::from_nanos(4_000_000),
    )
    .unwrap();
    assert!(
        prepare_live_audio_transport(
            &candidate,
            boundary,
            &control,
            Timestamp::from_nanos(4_000_000)
        )
        .is_err()
    );
    assert_eq!(format!("{:?}", transport), before_transport);
    assert_eq!(format!("{:?}", authority), before_authority);
}
