//! Mathematical owner transactions on original native records; no device acquisition claim.
use super::*;
use crate::audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis};
use crate::native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate};
use beatkernel::audio::{AudioCounters, RenderReport, TargetTime};
use beatkernel::time::{ClockDomainId, ClockPair, Duration, Timestamp};
use beatkernel::time::presentation::ObservationAdmission;
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
    asio::{AsioPresentationObservation, MultimediaHostInterval},
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn raw(ns: i64) -> ClockPoint {
    point(2, ns)
}
fn basis(hz: u32, start: u64) -> OutputFrameBasis {
    OutputFrameBasis::new(raw(100), hz, start).unwrap()
}
fn owner(descriptor: AudioAuthorityCoordinateBasis, capacity: usize) -> NativeAudioPresentation {
    let origin = match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(basis)
        | AudioAuthorityCoordinateBasis::SourceCounter { basis, .. } => {
            basis.point_at_stream_frame(0).unwrap()
        }
        AudioAuthorityCoordinateBasis::TargetFrame(basis) => {
            basis.point_at_stream_frame(0).unwrap()
        }
    };
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: origin,
        logical_origin: point(3, 500),
        host_domain: ClockDomainId(1),
    };
    let authority = AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            history_capacity: capacity,
            max_observation_age: Duration::from_nanos(10),
            ..AudioAuthorityConfig::default()
        },
        epoch,
        descriptor,
    )
    .unwrap();
    NativeAudioPresentation::new(
        authority,
        NativePresentationValidator::new(7, origin, ClockDomainId(1)),
    )
    .unwrap()
}
fn state(owner: &NativeAudioPresentation) -> String {
    format!("{owner:?}")
}
fn frame_snapshot(basis: OutputFrameBasis, frame: u64, at: i64) -> CoordinateNativeAudioSnapshot {
    let snapshot = NativeAudioSnapshot {
        epoch: 7,
        basis,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: basis.point_at_stream_frame(frame).unwrap(),
            target: host(at),
        }),
    };
    CoordinateNativeAudioSnapshot::new(snapshot, NativeOutputCoordinate::StreamFrame { frame })
        .unwrap()
}
fn wasapi(
    basis: OutputFrameBasis,
    position: u64,
    frequency: u64,
    at: i64,
    qpc: u64,
) -> CoordinateNativeAudioSnapshot {
    let native = AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position,
            frequency,
            qpc_100ns: qpc,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(host(at)),
            mapping_quality: beatkernel::time::ClockMappingQuality::Unknown,
        }),
    };
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::Wasapi {
                snapshot: native,
                basis: Some(basis),
            },
        },
        NativeOutputCoordinate::Counter {
            position,
            frequency,
        },
    )
    .unwrap()
}
fn asio(
    basis: OutputFrameBasis,
    frame: u64,
    before: i64,
    after: i64,
) -> CoordinateNativeAudioSnapshot {
    let absolute = basis.start_physical_frame().checked_add(frame).unwrap();
    let observation = AsioPresentationObservation::from_render(
        RenderReport {
            start_frame: absolute,
            frames: 16,
            playback_start_frame: absolute,
            playback_frames: 16,
            paused: false,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        },
        basis.sample_rate(),
        MultimediaHostInterval {
            before: host(before),
            after: host(after),
        },
        0,
        0,
        basis.origin(),
    )
    .unwrap();
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::Asio {
                observation,
                basis: Some(basis),
            },
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}
fn refusal(owner: &mut NativeAudioPresentation, snapshot: CoordinateNativeAudioSnapshot) {
    let before = state(owner);
    assert!(owner.admit_coordinate(snapshot).is_err());
    assert_eq!(state(owner), before);
}

#[test]
fn original_frames_retain_flat_nanoseconds_and_full_record_in_the_single_authority() {
    let basis = basis(2_000_000_000, 0);
    let mut owner = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    let first = frame_snapshot(basis, 0, 100);
    let second = frame_snapshot(basis, 1, 101);
    assert_eq!(
        owner.admit_coordinate(first).unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(owner.basis(), Some(basis));
    assert_eq!(
        owner.admit_coordinate(second).unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(owner.authority().history_len(), 2);
    assert_eq!(
        owner.latest_record().unwrap().pair(),
        ClockPair {
            source: raw(100),
            target: host(101)
        }
    );
    assert_eq!(
        owner.latest_record().unwrap().evidence(),
        &second.snapshot().evidence
    );
    assert_eq!(
        owner
            .authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate(),
        Some(second.coordinate())
    );
    assert_eq!(
        owner.authority().latest_observation(),
        Some(owner.latest_record().unwrap().pair())
    );
    assert!(owner.authority().has_fresh_observation(host(111)).unwrap());
    assert!(!owner.authority().has_fresh_observation(host(112)).unwrap());
}

#[test]
fn wasapi_original_counter_progress_is_retained_even_with_equal_rounded_output() {
    let basis = basis(1000, 0);
    let frequency = 4_000_000_000;
    let mut owner = owner(
        AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency },
        4,
    );
    for (position, at, qpc) in [(1, 100, 1), (2, 101, 2)] {
        let snapshot = wasapi(basis, position, frequency, at, qpc);
        assert_eq!(
            owner.admit_coordinate(snapshot).unwrap(),
            NativeObservationAdmission::Progress
        );
        assert_eq!(
            owner.latest_record().unwrap().evidence(),
            &snapshot.snapshot().evidence
        );
    }
    assert_eq!(owner.authority().history_len(), 2);
    assert_eq!(owner.latest_record().unwrap().pair().source, raw(100));
    assert_eq!(
        owner
            .authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate(),
        Some(NativeOutputCoordinate::Counter {
            position: 2,
            frequency
        })
    );
}

#[test]
fn source_counter_combines_nonzero_creation_fraction_before_one_nanosecond_floor() {
    let basis = basis(3, 2);
    let mut owner = owner(
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 3,
        },
        4,
    );
    let snapshot = wasapi(basis, 1, 3, 100, 1);
    assert_eq!(
        owner.admit_coordinate(snapshot).unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(
        owner.latest_record().unwrap().pair().source,
        raw(1_000_000_100)
    );
    assert_ne!(
        owner.latest_record().unwrap().pair().source,
        raw(1_000_000_099)
    );
}

#[test]
fn duplicate_frames_and_counters_preserve_every_owner_field_and_accepted_age() {
    let basis = basis(1000, 0);
    let mut frames = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    frames
        .admit_coordinate(frame_snapshot(basis, 1, 100))
        .unwrap();
    let before = state(&frames);
    for at in [100, 101, 10_000] {
        assert_eq!(
            frames
                .admit_coordinate(frame_snapshot(basis, 1, at))
                .unwrap(),
            NativeObservationAdmission::Unchanged
        );
        assert_eq!(state(&frames), before);
    }
    assert!(!frames.authority().has_fresh_observation(host(111)).unwrap());
    let mut counters = owner(
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 1_000_000_000,
        },
        4,
    );
    counters
        .admit_coordinate(wasapi(basis, 1, 1_000_000_000, 100, 1))
        .unwrap();
    let before = state(&counters);
    for (at, qpc) in [(100, 1), (101, 2), (10_000, 3)] {
        assert_eq!(
            counters
                .admit_coordinate(wasapi(basis, 1, 1_000_000_000, at, qpc))
                .unwrap(),
            NativeObservationAdmission::Unchanged
        );
        assert_eq!(state(&counters), before);
    }
    assert!(!counters
        .authority()
        .has_fresh_observation(host(111))
        .unwrap());
}

#[test]
fn original_asio_brackets_defer_coarse_midpoints_without_publishing_candidate_progress() {
    let basis = basis(1000, 0);
    let mut owner = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    owner.admit_coordinate(asio(basis, 0, 90, 110)).unwrap();
    let before = state(&owner);
    assert_eq!(
        owner.admit_coordinate(asio(basis, 16, 95, 105)).unwrap(),
        NativeObservationAdmission::AwaitingHostProgress
    );
    assert_eq!(state(&owner), before);
    assert!(!owner.authority().has_fresh_observation(host(111)).unwrap());
    let next = asio(basis, 16, 190, 230);
    assert_eq!(
        owner.admit_coordinate(next).unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(
        owner.latest_record().unwrap().evidence(),
        &next.snapshot().evidence
    );
    assert_eq!(
        owner.latest_record().unwrap().pair(),
        ClockPair {
            source: raw(16_000_100),
            target: host(210)
        }
    );
    assert_eq!(
        owner
            .authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 16 })
    );
}

#[test]
fn descriptor_kind_and_frequency_rejections_are_atomic_before_first_basis_pin() {
    let basis = basis(1000, 0);
    let mut frames = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    refusal(&mut frames, wasapi(basis, 1, 1_000_000_000, 100, 1));
    assert_eq!(frames.basis(), None);
    let mut counters = owner(
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 1_000_000_000,
        },
        4,
    );
    refusal(&mut counters, frame_snapshot(basis, 0, 100));
    refusal(&mut counters, asio(basis, 0, 90, 110));
    refusal(&mut counters, wasapi(basis, 1, 2_000_000_000, 100, 1));
    assert_eq!(counters.basis(), None);
    assert_eq!(counters.authority().history_len(), 0);
    counters
        .admit_coordinate(wasapi(basis, 1, 1_000_000_000, 100, 1))
        .unwrap();
    assert_eq!(counters.basis(), Some(basis));
}

#[test]
fn changed_creation_grid_is_refused_even_for_zero_frame_duplicate_and_asio_deferral() {
    let basis = basis(1000, 0);
    let changed = OutputFrameBasis::new(basis.origin(), 2000, 0).unwrap();
    let mut frames = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    refusal(&mut frames, frame_snapshot(changed, 0, 100));
    frames
        .admit_coordinate(frame_snapshot(basis, 0, 100))
        .unwrap();
    refusal(&mut frames, frame_snapshot(changed, 0, 101));
    let mut native = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    native.admit_coordinate(asio(basis, 0, 90, 110)).unwrap();
    refusal(&mut native, asio(changed, 0, 90, 110));
    refusal(&mut native, asio(changed, 16, 95, 105));
}

#[test]
fn epoch_and_domains_are_checked_even_when_coordinate_would_be_unchanged() {
    let basis = basis(1000, 0);
    let mut owner = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    owner
        .admit_coordinate(frame_snapshot(basis, 0, 100))
        .unwrap();
    let original = frame_snapshot(basis, 0, 101).snapshot();
    refusal(
        &mut owner,
        CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: 8,
                ..original
            },
            NativeOutputCoordinate::StreamFrame { frame: 0 },
        )
        .unwrap(),
    );
    let bad_host = NativeAudioSnapshot {
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: raw(100),
            target: point(9, 101),
        }),
        ..original
    };
    refusal(
        &mut owner,
        CoordinateNativeAudioSnapshot::new(
            bad_host,
            NativeOutputCoordinate::StreamFrame { frame: 0 },
        )
        .unwrap(),
    );
    let bad_origin = OutputFrameBasis::new(point(9, 100), 1000, 0).unwrap();
    refusal(&mut owner, frame_snapshot(bad_origin, 0, 101));
    let shifted = OutputFrameBasis::new(raw(101), 1000, 0).unwrap();
    refusal(&mut owner, frame_snapshot(shifted, 0, 101));
}

#[test]
fn counter_frequency_change_is_refused_even_when_candidate_position_is_duplicate() {
    let basis = basis(1000, 0);
    let mut owner = owner(
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 1_000_000_000,
        },
        4,
    );
    owner
        .admit_coordinate(wasapi(basis, 1, 1_000_000_000, 100, 1))
        .unwrap();
    refusal(&mut owner, wasapi(basis, 1, 2_000_000_000, 100, 1));
    refusal(&mut owner, wasapi(basis, 1, 2_000_000_000, 101, 2));
}

#[test]
fn full_real_authority_refusal_preserves_validator_record_basis_and_revision() {
    let basis = basis(2_000_000_000, 0);
    let mut owner = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 2);
    owner
        .admit_coordinate(frame_snapshot(basis, 0, 100))
        .unwrap();
    owner
        .admit_coordinate(frame_snapshot(basis, 1, 101))
        .unwrap();
    owner
        .authority_mut()
        .record_acquired_prefix(host(101))
        .unwrap();
    let before = state(&owner);
    let record = owner.latest_record();
    refusal(&mut owner, frame_snapshot(basis, 2, 102));
    assert_eq!(state(&owner), before);
    assert_eq!(owner.latest_record(), record);
    assert_eq!(owner.authority().history_len(), 2);
    assert_eq!(owner.basis(), Some(basis));
}

#[test]
fn independently_advanced_authority_cannot_commit_native_candidate_or_refresh_duplicate() {
    let basis = basis(1000, 0);
    let mut owner = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    owner
        .admit_coordinate(frame_snapshot(basis, 0, 100))
        .unwrap();
    let next = frame_snapshot(basis, 1, 101);
    let OriginalNativePresentationEvidence::SuppliedPair(pair) = next.snapshot().evidence else {
        unreachable!()
    };
    assert_eq!(
        owner
            .authority_mut()
            .observe_coordinate(7, pair, next.coordinate())
            .unwrap(),
        ObservationAdmission::Retained
    );
    refusal(&mut owner, next);
    refusal(&mut owner, frame_snapshot(basis, 0, 100));
    assert_eq!(
        owner.latest_record().unwrap().pair(),
        ClockPair {
            source: raw(100),
            target: host(100)
        }
    );
    assert_eq!(owner.authority().latest_observation(), Some(pair));
}

#[test]
fn independently_advanced_authority_refuses_asio_coarse_host_deferral() {
    let basis = basis(1000, 0);
    let mut owner = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    owner.admit_coordinate(asio(basis, 0, 90, 110)).unwrap();
    owner
        .authority_mut()
        .observe_coordinate(
            7,
            ClockPair {
                source: raw(16_000_100),
                target: host(101),
            },
            NativeOutputCoordinate::StreamFrame { frame: 16 },
        )
        .unwrap();
    refusal(&mut owner, asio(basis, 16, 95, 105));
}

#[test]
fn independently_restarted_empty_authority_cannot_refresh_or_replace_native_record() {
    let basis = basis(2_000_000_000, 0);
    let mut owner = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    owner
        .admit_coordinate(frame_snapshot(basis, 0, 100))
        .unwrap();
    let merger = InputMerger::new(
        ClockDomainId(1),
        host(0),
        vec![beatkernel::input::DeviceId(9)],
        8,
    )
    .unwrap();
    let restart = owner
        .authority()
        .prepare_correlation_restart(&merger)
        .unwrap();
    owner
        .authority_mut()
        .commit_correlation_restart(restart, &merger)
        .unwrap();
    assert_eq!(owner.authority().history_len(), 0);
    refusal(&mut owner, frame_snapshot(basis, 0, 100));
    refusal(&mut owner, frame_snapshot(basis, 1, 101));
    assert_eq!(
        owner.latest_record().unwrap().pair(),
        ClockPair {
            source: raw(100),
            target: host(100)
        }
    );
}

#[test]
fn equal_rounded_pair_after_restart_does_not_hide_different_original_frame() {
    let basis = basis(2_000_000_000, 0);
    let mut owner = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    owner
        .admit_coordinate(frame_snapshot(basis, 0, 100))
        .unwrap();
    let merger = InputMerger::new(
        ClockDomainId(1),
        host(0),
        vec![beatkernel::input::DeviceId(9)],
        8,
    )
    .unwrap();
    let restart = owner
        .authority()
        .prepare_correlation_restart(&merger)
        .unwrap();
    owner
        .authority_mut()
        .commit_correlation_restart(restart, &merger)
        .unwrap();
    let same_pair = owner.latest_record().unwrap().pair();
    owner
        .authority_mut()
        .observe_coordinate(
            7,
            same_pair,
            NativeOutputCoordinate::StreamFrame { frame: 1 },
        )
        .unwrap();
    // Both retained ClockPairs are exactly equal, but the authority now means
    // half a nanosecond and the native validator still means stream frame zero.
    assert_eq!(owner.authority().latest_observation(), Some(same_pair));
    refusal(&mut owner, frame_snapshot(basis, 1, 100));
    refusal(&mut owner, frame_snapshot(basis, 2, 101));
    assert_eq!(
        owner.validator.latest_frame_position(),
        Some((
            beatkernel_platform::audio::presentation::validation::NativeFrameBasis::Source(basis),
            0
        ))
    );
}

#[test]
fn coordinate_mode_refuses_legacy_scalar_and_target_entry_points_before_any_noop() {
    let basis = basis(1000, 0);
    let mut owner = owner(AudioAuthorityCoordinateBasis::SourceFrame(basis), 4);
    let original = frame_snapshot(basis, 0, 100).snapshot();
    let before = state(&owner);
    assert!(owner.admit(original).is_err());
    assert_eq!(state(&owner), before);
    let target_basis =
        TargetFrameBasis::new(raw(100), TargetTime::from_frames(0, 1000).unwrap(), 1000).unwrap();
    let target = TargetNativeAudioSnapshot {
        epoch: 7,
        basis: target_basis,
        evidence: original.evidence,
    };
    assert!(owner.admit_target(target).is_err());
    assert_eq!(state(&owner), before);
    owner
        .admit_coordinate(frame_snapshot(basis, 0, 100))
        .unwrap();
    let before = state(&owner);
    assert!(owner.admit(original).is_err());
    assert!(owner.admit_target(target).is_err());
    assert_eq!(state(&owner), before);
}

#[test]
fn scalar_owner_keeps_original_admission_and_rejects_coordinate_optin_without_changes() {
    let basis = basis(1000, 0);
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: raw(100),
        logical_origin: point(3, 500),
        host_domain: ClockDomainId(1),
    };
    let mut owner = NativeAudioPresentation::new(
        AudioAuthority::new(
            AudioAuthorityConfig {
                max_observation_age: Duration::from_nanos(10),
                ..AudioAuthorityConfig::default()
            },
            epoch,
        )
        .unwrap(),
        NativePresentationValidator::new(7, raw(100), ClockDomainId(1)),
    )
    .unwrap();
    refusal(&mut owner, frame_snapshot(basis, 0, 100));
    assert_eq!(
        owner
            .admit(frame_snapshot(basis, 0, 100).snapshot())
            .unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(
        owner
            .admit(frame_snapshot(basis, 1, 101).snapshot())
            .unwrap(),
        NativeObservationAdmission::Progress
    );
    let before = state(&owner);
    assert_eq!(
        owner
            .admit(frame_snapshot(basis, 1, 1000).snapshot())
            .unwrap(),
        NativeObservationAdmission::Unchanged
    );
    assert_eq!(state(&owner), before);
    refusal(&mut owner, frame_snapshot(basis, 2, 102));
}

#[test]
fn target_descriptor_cannot_be_reinterpreted_as_a_source_creation_owner() {
    let source = basis(1000, 0);
    let target =
        TargetFrameBasis::new(raw(100), TargetTime::from_frames(0, 1000).unwrap(), 1000).unwrap();
    let mut owner = owner(AudioAuthorityCoordinateBasis::TargetFrame(target), 4);
    refusal(&mut owner, frame_snapshot(source, 0, 100));
    assert_eq!(owner.basis(), None);
    assert_eq!(owner.authority().history_len(), 0);
}
