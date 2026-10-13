//! Live request pins use original coordinates; software rigs do not prove device timing.
use super::*;
use crate::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis as Basis,
        AudioAuthorityEpoch, PreparedCoordinateControlCutoff,
    },
    gameplay::output::ports::SourcePlaybackIdentity,
    local_input::InputMerger,
    native_output_coordinate::NativeOutputCoordinate,
};
use beatkernel::{
    audio::*,
    time::{Duration, ExtrapolationPolicy},
};
use std::cmp::Ordering;

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn authority(basis: Basis) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            history_capacity: 16,
            max_observation_age: Duration::from_nanos(1_000_000_000),
            input_extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: basis.origin().unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        basis,
    )
    .unwrap()
}
fn observe(a: &mut AudioAuthority, units: u64, at: i64) {
    let basis = a.coordinate_basis().unwrap();
    let coordinate = match basis {
        Basis::SourceCounter { frequency, .. } => NativeOutputCoordinate::Counter {
            position: units,
            frequency,
        },
        _ => NativeOutputCoordinate::StreamFrame { frame: units },
    };
    let source = match basis {
        Basis::SourceFrame(basis) => basis.point_at_stream_frame(units).unwrap(),
        Basis::SourceCounter { basis, frequency } => {
            basis.point_at_native_counter(units, frequency).unwrap()
        }
        Basis::TargetFrame(basis) => basis.point_at_stream_frame(units).unwrap(),
    };
    a.observe_coordinate(
        7,
        ClockPair {
            source,
            target: host(at),
        },
        coordinate,
    )
    .unwrap();
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn retained(pause: &NativePause, expected: InterpolatedTargetTime) {
    let before = pause.clone();
    for _ in 0..8 {
        assert_eq!(
            pause
                .pending_coordinate_retention_query()
                .unwrap()
                .unwrap()
                .compare_interpolated_time(expected),
            Ordering::Equal
        );
    }
    assert_eq!(pause, &before);
}
fn no_pin(pause: &NativePause) {
    let before = pause.clone();
    assert!(pause
        .pending_coordinate_retention_query()
        .unwrap()
        .is_none());
    assert_eq!(pause, &before);
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(i64::MIN), vec![DeviceId(9)], 8).unwrap()
}
fn cutoff(
    a: &AudioAuthority,
    adoption: &PreparedCoordinatePauseAdoption,
    now: ClockPoint,
    m: &InputMerger,
) -> PreparedCoordinateControlCutoff {
    let at = a
        .presented_coordinate_boundary_host(&adoption.exact_cut(), now)
        .unwrap()
        .unwrap();
    if adoption.paused() {
        a.prepare_coordinate_control_cutoff(7, adoption.exact_cut(), at, now, m)
    } else {
        a.prepare_coordinate_resume_control_cutoff(7, adoption.exact_cut(), at, now, m)
    }
    .unwrap()
    .unwrap()
}
fn source_rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(3, 1).unwrap();
    let pcm = PcmLimits::new(256, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 64], pcm).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}

#[test]
fn source_request_stays_pinned_through_staging_then_releases_after_actual_commits() {
    let basis = Basis::SourceFrame(OutputFrameBasis::new(point(2, 0), 3, 0).unwrap());
    let mut a = authority(basis);
    observe(&mut a, 0, 100);
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 3)
        .unwrap()
        .with_coordinate_binding(7, basis, SourcePlaybackIdentity::new(3, 0, 0).unwrap())
        .unwrap();
    no_pin(&pause);
    let (mut producer, mut mixer) = source_rig();
    mixer.render(&mut [0.0; 1]).unwrap();
    assert!(pause.request_coordinate(true, &a).unwrap());
    let request = exact(TargetTime::from_frames(0, 3).unwrap());
    retained(&pause, request);
    producer.request_pause(true);
    let adoption = pause
        .prepare_coordinate_adoption(Some(mixer.render(&mut [0.0; 2]).unwrap()), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        adoption.exact_cut().compare_interpolated_time(request),
        Ordering::Greater
    );
    observe(&mut a, 1, 101);
    observe(&mut a, 3, 103);
    a.record_acquired_prefix(host(103)).unwrap();
    let m = merger();
    let prepared = cutoff(&a, &adoption, host(103), &m);
    retained(&pause, request);
    pause
        .validate_coordinate_adoption(&adoption, &a, &prepared, host(103), &m)
        .unwrap();
    pause
        .commit_coordinate_adoption(adoption, &a, &prepared, host(103), &m)
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
    assert!(pause.coordinate_reference.is_none());
    no_pin(&pause);
    a.commit_coordinate_control_cutoff(prepared, host(103), &m)
        .unwrap();

    assert!(pause.request_coordinate(false, &a).unwrap());
    assert_eq!(pause.phase(), PausePhase::Resuming);
    let resume_request = exact(TargetTime::from_frames(3, 3).unwrap());
    retained(&pause, resume_request);
    producer.request_pause(false);
    let adoption = pause
        .prepare_coordinate_adoption(Some(mixer.render(&mut [0.0; 1]).unwrap()), None)
        .unwrap()
        .unwrap();
    observe(&mut a, 4, 104);
    a.record_acquired_prefix(host(104)).unwrap();
    let prepared = cutoff(&a, &adoption, host(104), &m);
    retained(&pause, resume_request);
    pause
        .validate_coordinate_adoption(&adoption, &a, &prepared, host(104), &m)
        .unwrap();
    pause
        .commit_coordinate_adoption(adoption, &a, &prepared, host(104), &m)
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Running);
    assert!(pause.coordinate_reference.is_none());
    no_pin(&pause);
    a.commit_coordinate_control_cutoff(prepared, host(104), &m)
        .unwrap();
}

#[test]
fn pending_original_request_is_older_than_new_closed_future_input_minimum() {
    let basis = Basis::SourceFrame(OutputFrameBasis::new(point(2, 0), 3, 0).unwrap());
    let mut a = authority(basis);
    observe(&mut a, 1, 100);
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 3)
        .unwrap()
        .with_coordinate_binding(7, basis, SourcePlaybackIdentity::new(3, 0, 0).unwrap())
        .unwrap();
    assert!(pause.request_coordinate(true, &a).unwrap());
    let original = exact(TargetTime::from_frames(1, 3).unwrap());
    observe(&mut a, 2, 200);
    observe(&mut a, 3, 300);
    let mut m = merger();
    let query = exact(TargetTime::from_frames(3, 3).unwrap());
    a.record_acquired_prefix(host(300)).unwrap();
    let token = a
        .prepare_coordinate_control_cutoff(7, query, host(300), host(300), &m)
        .unwrap()
        .unwrap();
    a.commit_coordinate_control_cutoff(token, host(300), &m)
        .unwrap();
    let frontier = a
        .prepare_coordinate_held_frontier(host(300), &m)
        .unwrap()
        .unwrap();
    a.commit_coordinate_held_frontier(frontier, host(300), &mut m)
        .unwrap();
    assert_eq!(a.closed_host_prefix(), Some(host(300)));
    assert_eq!(
        a.future_input_retention_query()
            .unwrap()
            .compare_interpolated_time(original),
        Ordering::Greater
    );
    retained(&pause, original);
    assert!(!pause.request_coordinate(true, &a).unwrap());
    retained(&pause, original);
}

#[test]
fn maximum_frequency_counter_preserves_subnanosecond_original_request() {
    let basis = Basis::SourceCounter {
        basis: OutputFrameBasis::new(point(2, 0), 3, 2).unwrap(),
        frequency: u64::MAX,
    };
    let mut a = authority(basis);
    observe(&mut a, 1, 100);
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 3)
        .unwrap()
        .with_coordinate_binding(7, basis, SourcePlaybackIdentity::new(3, 0, 0).unwrap())
        .unwrap();
    assert!(pause.request_coordinate(true, &a).unwrap());
    let expected =
        InterpolatedTargetTime::new(TargetTime::from_frames(2, 3).unwrap(), 1, 1, u64::MAX, 0, 1)
            .unwrap();
    assert_eq!(
        expected.compare_target_time(TargetTime::from_frames(2, 3).unwrap()),
        Ordering::Greater
    );
    retained(&pause, expected);
    observe(&mut a, 2, 101);
    assert!(!pause.request_coordinate(true, &a).unwrap());
    retained(&pause, expected);
}

#[test]
fn fractional_target_base_and_stream_frame_are_preserved_exactly() {
    let target = TargetFrameBasis::new(point(2, 0), TargetTime::new(0, 1, 3).unwrap(), 7).unwrap();
    let basis = Basis::TargetFrame(target);
    let mut a = authority(basis);
    observe(&mut a, 1, 100);
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 3)
        .unwrap()
        .with_target_basis(7, target)
        .unwrap()
        .with_coordinate_binding(7, basis, SourcePlaybackIdentity::new(3, 0, 0).unwrap())
        .unwrap();
    no_pin(&pause);
    assert!(pause.request_coordinate(true, &a).unwrap());
    let expected = InterpolatedTargetTime::new(target.start_time(), 1, 1, 7, 0, 1).unwrap();
    retained(&pause, expected);
    observe(&mut a, 2, 101);
    retained(&pause, expected);
}

#[test]
fn real_converted_pause_and_resume_release_target_request_after_ack() {
    let (mut producer, mut output) =
        crate::native_converted_test_support::rig(44_100, 48_000, None, None, 0);
    let target = output.target_frame_basis();
    let basis = Basis::TargetFrame(target);
    let mut a = authority(basis);
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 44_100)
        .unwrap()
        .with_target_basis(7, target)
        .unwrap()
        .with_coordinate_binding(7, basis, SourcePlaybackIdentity::new(44_100, 0, 0).unwrap())
        .unwrap();
    observe(&mut a, 0, 100);
    observe(&mut a, 1, 101);
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    assert!(pause.request_coordinate(true, &a).unwrap());
    let request = exact(TargetTime::from_frames(1, 48_000).unwrap());
    retained(&pause, request);
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    let adoption = pause
        .prepare_coordinate_adoption(output.last_real_source_report(), Some(output.boundaries()))
        .unwrap()
        .unwrap();
    observe(&mut a, 9, 109);
    a.record_acquired_prefix(host(109)).unwrap();
    let m = merger();
    let prepared = cutoff(&a, &adoption, host(109), &m);
    retained(&pause, request);
    pause
        .validate_coordinate_adoption(&adoption, &a, &prepared, host(109), &m)
        .unwrap();
    pause
        .commit_coordinate_adoption(adoption, &a, &prepared, host(109), &m)
        .unwrap();
    a.commit_coordinate_control_cutoff(prepared, host(109), &m)
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
    no_pin(&pause);
    output.admit(8).unwrap();
    assert!(pause.request_coordinate(false, &a).unwrap());
    let request = exact(TargetTime::from_frames(9, 48_000).unwrap());
    retained(&pause, request);
    producer.request_pause(false);
    let held = output.render_held_pending(4).unwrap();
    assert!(held.source.is_none());
    output.admit(4).unwrap();
    retained(&pause, request);
    let mut adoption = None;
    for _ in 0..32 {
        output.render_pending(1).unwrap();
        output.admit(1).unwrap();
        adoption = pause
            .prepare_coordinate_adoption(
                output.last_real_source_report(),
                Some(output.boundaries()),
            )
            .unwrap();
        if adoption.is_some() {
            break;
        }
    }
    let adoption = adoption.expect("actual converted resume adoption");
    observe(&mut a, 128, 228);
    a.record_acquired_prefix(host(228)).unwrap();
    let prepared = cutoff(&a, &adoption, host(228), &m);
    retained(&pause, request);
    pause
        .validate_coordinate_adoption(&adoption, &a, &prepared, host(228), &m)
        .unwrap();
    pause
        .commit_coordinate_adoption(adoption, &a, &prepared, host(228), &m)
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Running);
    assert!(pause.coordinate_reference.is_none());
    no_pin(&pause);
    a.commit_coordinate_control_cutoff(prepared, host(228), &m)
        .unwrap();
}

#[test]
fn scalar_running_and_live_pause_have_no_coordinate_retention_pin() {
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 3).unwrap();
    no_pin(&pause);
    assert!(pause
        .request(
            true,
            ClockPair {
                source: point(2, 0),
                target: host(100)
            }
        )
        .unwrap());
    assert_eq!(pause.phase(), PausePhase::Pausing);
    no_pin(&pause);
    let (mut producer, mut mixer) = source_rig();
    mixer.render(&mut [0.0; 1]).unwrap();
    producer.request_pause(true);
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    pause
        .observe(
            Some(report),
            ClockPair {
                source: point(2, 1_000_000_000),
                target: host(300),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
    no_pin(&pause);
    assert!(pause
        .request(
            false,
            ClockPair {
                source: point(2, 1_000_000_000),
                target: host(300)
            }
        )
        .unwrap());
    assert_eq!(pause.phase(), PausePhase::Resuming);
    no_pin(&pause);
}
