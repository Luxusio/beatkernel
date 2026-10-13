//! Original target frame transactions on actual presentation/authority owners.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    native_output_coordinate::{
        CoordinateNativeAudioSnapshot, CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate,
        NativeOutputCoordinateError,
    },
};
use beatkernel::{
    audio::{TargetFrameBasis, TargetTime},
    input::DeviceId,
    time::{ClockDomainId, ClockPair, Duration, Timestamp, presentation::ObservationAdmission},
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn basis() -> TargetFrameBasis {
    TargetFrameBasis::new(
        point(2, 100),
        TargetTime::new(0, 1, 3).unwrap(),
        2_000_000_000,
    )
    .unwrap()
}
fn owner(basis: TargetFrameBasis, capacity: usize) -> NativeAudioPresentation {
    let origin = basis.point_at_stream_frame(0).unwrap();
    NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig {
                history_capacity: capacity,
                max_observation_age: Duration::from_nanos(10),
                ..AudioAuthorityConfig::default()
            },
            AudioAuthorityEpoch {
                id: 7,
                stream_origin: origin,
                logical_origin: point(3, 500),
                host_domain: ClockDomainId(1),
            },
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
        )
        .unwrap(),
        NativePresentationValidator::new(7, origin, ClockDomainId(1)),
    )
    .unwrap()
}
fn snapshot(basis: TargetFrameBasis, frame: u64, at: i64) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: point(1, at),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}
fn refuse(p: &mut NativeAudioPresentation, s: CoordinateTargetNativeAudioSnapshot) {
    let before = format!("{p:?}");
    assert!(p.admit_target_coordinate(s).is_err());
    assert_eq!(format!("{p:?}"), before);
}

#[test]
fn target_flat_ns_progress_retains_original_sidecar_and_exact_descriptor() {
    let b = basis();
    let mut p = owner(b, 4);
    for (frame, at) in [(0, 100), (1, 101)] {
        let s = snapshot(b, frame, at);
        assert_eq!(
            p.admit_target_coordinate(s).unwrap(),
            NativeObservationAdmission::Progress
        );
        assert_eq!(
            p.latest_record().unwrap().evidence(),
            &s.snapshot().evidence
        );
    }
    assert_eq!(p.basis(), None);
    assert_eq!(p.target_basis(), Some(b));
    assert_eq!(p.authority().history_len(), 2);
    assert_eq!(
        p.validator.latest_frame_position(),
        Some((NativeFrameBasis::Target(b), 1))
    );
    assert_eq!(
        p.authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 1 })
    );
    assert_eq!(
        p.authority().latest_observation(),
        Some(p.latest_record().unwrap().pair())
    );
}
#[test]
fn duplicate_target_frame_preserves_native_record_and_age() {
    let b = basis();
    let mut p = owner(b, 4);
    p.admit_target_coordinate(snapshot(b, 0, 100)).unwrap();
    let before = format!("{p:?}");
    for at in [100, 101, 10000] {
        assert_eq!(
            p.admit_target_coordinate(snapshot(b, 0, at)).unwrap(),
            NativeObservationAdmission::Unchanged
        );
        assert_eq!(format!("{p:?}"), before);
    }
    assert!(!p.authority().has_fresh_observation(point(1, 111)).unwrap());
}
#[test]
fn wrong_basis_epoch_and_host_are_checked_before_duplicate() {
    let b = basis();
    let mut p = owner(b, 4);
    p.admit_target_coordinate(snapshot(b, 0, 100)).unwrap();
    let wrong = TargetFrameBasis::new(b.origin(), b.start_time(), 1000).unwrap();
    refuse(&mut p, snapshot(wrong, 0, 101));
    let shifted = TargetFrameBasis::new(point(2, 101), b.start_time(), b.sample_rate()).unwrap();
    refuse(&mut p, snapshot(shifted, 0, 101));
    let original = snapshot(b, 0, 101).snapshot();
    refuse(
        &mut p,
        CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
                epoch: 8,
                ..original
            },
            NativeOutputCoordinate::StreamFrame { frame: 0 },
        )
        .unwrap(),
    );
    refuse(
        &mut p,
        CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: b.point_at_stream_frame(0).unwrap(),
                    target: point(9, 101),
                }),
                ..original
            },
            NativeOutputCoordinate::StreamFrame { frame: 0 },
        )
        .unwrap(),
    );
}
#[test]
fn real_capacity_refusal_does_not_commit_native_sidecar_or_basis() {
    let b = basis();
    let mut p = owner(b, 2);
    p.admit_target_coordinate(snapshot(b, 0, 100)).unwrap();
    p.admit_target_coordinate(snapshot(b, 1, 101)).unwrap();
    refuse(&mut p, snapshot(b, 2, 102));
    assert_eq!(
        p.validator.latest_frame_position(),
        Some((NativeFrameBasis::Target(b), 1))
    );
    assert_eq!(p.authority().history_len(), 2);
}
#[test]
fn independent_authority_advance_and_empty_restart_refuse_before_duplicate() {
    for restart in [false, true] {
        let b = basis();
        let mut p = owner(b, 4);
        p.admit_target_coordinate(snapshot(b, 0, 100)).unwrap();
        if restart {
            let merger =
                InputMerger::new(ClockDomainId(1), point(1, 0), vec![DeviceId(9)], 8).unwrap();
            let token = p.authority().prepare_correlation_restart(&merger).unwrap();
            p.authority_mut()
                .commit_correlation_restart(token, &merger)
                .unwrap();
        } else {
            let next = snapshot(b, 1, 101);
            let OriginalNativePresentationEvidence::SuppliedPair(pair) = next.snapshot().evidence
            else {
                unreachable!()
            };
            assert_eq!(
                p.authority_mut()
                    .observe_coordinate(7, pair, next.coordinate())
                    .unwrap(),
                ObservationAdmission::Retained
            );
        }
        refuse(&mut p, snapshot(b, 0, 100));
        refuse(&mut p, snapshot(b, 1, 101));
    }
}
#[test]
fn restarted_identical_pair_cannot_hide_different_original_target_frame() {
    let b = basis();
    let mut p = owner(b, 4);
    p.admit_target_coordinate(snapshot(b, 0, 100)).unwrap();
    let merger = InputMerger::new(ClockDomainId(1), point(1, 0), vec![DeviceId(9)], 8).unwrap();
    let token = p.authority().prepare_correlation_restart(&merger).unwrap();
    p.authority_mut()
        .commit_correlation_restart(token, &merger)
        .unwrap();
    let pair = p.latest_record().unwrap().pair();
    assert_eq!(b.point_at_stream_frame(1).unwrap(), pair.source);
    p.authority_mut()
        .observe_coordinate(7, pair, NativeOutputCoordinate::StreamFrame { frame: 1 })
        .unwrap();
    refuse(&mut p, snapshot(b, 1, 100));
    assert_eq!(
        p.validator.latest_frame_position(),
        Some((NativeFrameBasis::Target(b), 0))
    );
}
#[test]
fn target_wrappers_reject_counter_and_asio_evidence_without_hardware_grant() {
    let b = basis();
    let s = snapshot(b, 0, 100).snapshot();
    assert_eq!(
        CoordinateTargetNativeAudioSnapshot::new(
            s,
            NativeOutputCoordinate::Counter {
                position: 0,
                frequency: 1
            }
        ),
        Err(NativeOutputCoordinateError::UnsupportedEvidence)
    );
    let source = OutputFrameBasis::new(point(2, 100), 3, 1).unwrap();
    let observation = beatkernel_platform::audio::asio::AsioPresentationObservation::from_render(
        beatkernel::audio::RenderReport {
            start_frame: 1,
            frames: 1,
            playback_start_frame: 0,
            playback_frames: 1,
            paused: false,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: beatkernel::audio::AudioCounters::default(),
        },
        3,
        beatkernel_platform::audio::asio::MultimediaHostInterval {
            before: point(1, 99),
            after: point(1, 101),
        },
        0,
        0,
        source.origin(),
    )
    .unwrap();
    assert_eq!(
        CoordinateTargetNativeAudioSnapshot::new(
            TargetNativeAudioSnapshot {
                evidence: OriginalNativePresentationEvidence::Asio {
                    observation,
                    basis: Some(source)
                },
                ..s
            },
            NativeOutputCoordinate::StreamFrame { frame: 0 }
        ),
        Err(NativeOutputCoordinateError::UnsupportedEvidence)
    );
}
#[test]
fn source_coordinate_and_scalar_target_modes_cannot_mix() {
    let b = basis();
    let mut target = owner(b, 4);
    let before = format!("{target:?}");
    assert!(target.admit_target(snapshot(b, 0, 100).snapshot()).is_err());
    let source = OutputFrameBasis::new(point(2, 100), 3, 1).unwrap();
    let source_snapshot = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 7,
            basis: source,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: source.point_at_stream_frame(0).unwrap(),
                target: point(1, 100),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame: 0 },
    )
    .unwrap();
    assert!(target.admit_coordinate(source_snapshot).is_err());
    assert_eq!(format!("{target:?}"), before);
    let scalar_authority =
        AudioAuthority::new(AudioAuthorityConfig::default(), target.authority().epoch()).unwrap();
    let mut scalar = NativeAudioPresentation::new(
        scalar_authority,
        NativePresentationValidator::new(7, b.point_at_stream_frame(0).unwrap(), ClockDomainId(1)),
    )
    .unwrap();
    refuse(&mut scalar, snapshot(b, 0, 100));
}
#[test]
fn backward_native_host_and_frame_refuse_transactionally() {
    let b = basis();
    let mut p = owner(b, 4);
    p.admit_target_coordinate(snapshot(b, 2, 100)).unwrap();
    refuse(&mut p, snapshot(b, 1, 101));
    refuse(&mut p, snapshot(b, 3, 99));
}
