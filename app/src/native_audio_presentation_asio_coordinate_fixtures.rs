//! Actual owners admit supplied ASIO frame coordinates; no SDK or native acquisition proof.
use super::*;
use crate::audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis};
use beatkernel::{
    audio::{AudioCounters, RenderReport},
    time::{ClockDomainId, Duration, Timestamp},
};
use beatkernel_platform::audio::asio::{AsioPresentationObservation, MultimediaHostInterval};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn basis(hz: u32) -> OutputFrameBasis {
    OutputFrameBasis::new(point(2, 100), hz, 0).unwrap()
}
fn owner(basis: OutputFrameBasis, coordinates: bool) -> NativeAudioPresentation {
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: point(2, 100),
        logical_origin: point(3, 500),
        host_domain: ClockDomainId(1),
    };
    let config = AudioAuthorityConfig {
        history_capacity: 8,
        max_observation_age: Duration::from_nanos(10),
        ..AudioAuthorityConfig::default()
    };
    let authority = if coordinates {
        AudioAuthority::new_with_coordinates(
            config,
            epoch,
            AudioAuthorityCoordinateBasis::SourceFrame(basis),
        )
        .unwrap()
    } else {
        AudioAuthority::new(config, epoch).unwrap()
    };
    NativeAudioPresentation::new(
        authority,
        NativePresentationValidator::new(7, epoch.stream_origin, epoch.host_domain),
    )
    .unwrap()
}
fn snapshot(
    basis: OutputFrameBasis,
    frame: u64,
    frames: usize,
    before: i64,
    after: i64,
) -> CoordinateNativeAudioSnapshot {
    let observation = AsioPresentationObservation::from_render(
        RenderReport {
            start_frame: frame,
            frames,
            playback_start_frame: frame,
            playback_frames: frames,
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
            before: point(1, before),
            after: point(1, after),
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
fn state(owner: &NativeAudioPresentation) -> String {
    format!("{owner:?}")
}

#[test]
fn actual_coordinate_owner_retains_original_asio_progress_with_equal_raw_nanoseconds() {
    for hz in [2_000_000_000, u32::MAX] {
        let basis = basis(hz);
        let mut owner = owner(basis, true);
        let first = snapshot(basis, 0, 1, 90, 110);
        let next = snapshot(basis, 1, 1, 100, 102);
        assert_eq!(
            owner.admit_coordinate(first).unwrap(),
            NativeObservationAdmission::Progress
        );
        assert_eq!(
            owner.admit_coordinate(next).unwrap(),
            NativeObservationAdmission::Progress
        );
        assert_eq!(owner.authority().history_len(), 2);
        assert_eq!(
            owner.authority().latest_observation().unwrap().source,
            point(2, 100)
        );
        assert_eq!(
            owner.authority().latest_observation().unwrap().target,
            point(1, 101)
        );
        assert_eq!(
            owner
                .authority()
                .latest_coordinate_observation()
                .unwrap()
                .coordinate(),
            Some(NativeOutputCoordinate::StreamFrame { frame: 1 })
        );
        assert_eq!(
            owner.latest_record().unwrap().evidence(),
            &next.snapshot().evidence
        );
        assert!(owner
            .authority()
            .has_fresh_observation(point(1, 111))
            .unwrap());
        assert!(!owner
            .authority()
            .has_fresh_observation(point(1, 112))
            .unwrap());
    }
}

#[test]
fn duplicate_and_equal_host_candidate_preserve_authority_age_and_original_native_record() {
    let basis = basis(2_000_000_000);
    let mut owner = owner(basis, true);
    owner
        .admit_coordinate(snapshot(basis, 0, 1, 90, 110))
        .unwrap();
    let before = state(&owner);
    assert_eq!(
        owner
            .admit_coordinate(snapshot(basis, 0, 1, 9_000, 11_000))
            .unwrap(),
        NativeObservationAdmission::Unchanged
    );
    assert_eq!(state(&owner), before);
    assert_eq!(
        owner
            .admit_coordinate(snapshot(basis, 1, 1, 95, 105))
            .unwrap(),
        NativeObservationAdmission::AwaitingHostProgress
    );
    assert_eq!(state(&owner), before);
    assert!(!owner
        .authority()
        .has_fresh_observation(point(1, 111))
        .unwrap());
    assert_eq!(
        owner
            .admit_coordinate(snapshot(basis, 1, 1, 100, 102))
            .unwrap(),
        NativeObservationAdmission::Progress
    );
}

#[test]
fn coordinate_owner_rejects_overlap_regression_and_other_epoch_transactionally() {
    let basis = basis(u32::MAX);
    let mut owner = owner(basis, true);
    owner
        .admit_coordinate(snapshot(basis, 1, 2, 90, 110))
        .unwrap();
    let before = state(&owner);
    for candidate in [
        snapshot(basis, 0, 1, 190, 210),
        snapshot(basis, 2, 1, 190, 210),
        snapshot(basis, 3, 1, 80, 100),
    ] {
        assert!(owner.admit_coordinate(candidate).is_err());
        assert_eq!(state(&owner), before);
    }
    let candidate = snapshot(basis, 3, 1, 190, 210);
    let wrong_epoch = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 8,
            ..candidate.snapshot()
        },
        candidate.coordinate(),
    )
    .unwrap();
    assert!(owner.admit_coordinate(wrong_epoch).is_err());
    assert_eq!(state(&owner), before);
}

#[test]
fn scalar_entry_cannot_grant_flat_nanosecond_block_progress_or_diverge_owners() {
    let basis = basis(2_000_000_000);
    let mut owner = owner(basis, false);
    owner
        .admit(snapshot(basis, 0, 1, 90, 110).snapshot())
        .unwrap();
    let before = state(&owner);
    assert!(owner
        .admit(snapshot(basis, 1, 1, 100, 102).snapshot())
        .is_err());
    assert_eq!(state(&owner), before);
    assert_eq!(owner.authority().history_len(), 1);
    assert_eq!(
        owner.authority().latest_observation(),
        Some(owner.latest_record().unwrap().pair())
    );
    // A later block that actually advances the scalar output still follows the legacy route.
    assert_eq!(
        owner
            .admit(snapshot(basis, 2, 1, 101, 103).snapshot())
            .unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(owner.authority().history_len(), 2);
    assert_eq!(
        owner.authority().latest_observation(),
        Some(owner.latest_record().unwrap().pair())
    );
    let before = state(&owner);
    assert_eq!(
        owner
            .admit(snapshot(basis, 2, 1, 9_000, 11_000).snapshot())
            .unwrap(),
        NativeObservationAdmission::Unchanged
    );
    assert_eq!(state(&owner), before);
}

#[test]
fn scalar_entry_is_refused_before_coordinate_owner_can_change_or_publish_new_record() {
    let basis = basis(2_000_000_000);
    let mut owner = owner(basis, true);
    owner
        .admit_coordinate(snapshot(basis, 0, 1, 90, 110))
        .unwrap();
    let before = state(&owner);
    assert!(owner
        .admit(snapshot(basis, 1, 1, 100, 102).snapshot())
        .is_err());
    assert_eq!(state(&owner), before);
}

#[test]
fn target_scalar_wrapper_still_rejects_asio_evidence_without_creating_authority() {
    let basis = basis(2_000_000_000);
    let target = TargetFrameBasis::new(
        basis.origin(),
        beatkernel::audio::TargetTime::new(0, 0, 1).unwrap(),
        basis.sample_rate(),
    )
    .unwrap();
    let mut owner = owner(basis, false);
    let before = state(&owner);
    assert!(owner
        .admit_target(TargetNativeAudioSnapshot {
            epoch: 7,
            basis: target,
            evidence: snapshot(basis, 0, 1, 90, 110).snapshot().evidence,
        })
        .is_err());
    assert_eq!(state(&owner), before);
    assert_eq!(owner.target_basis(), None);
    assert_eq!(owner.authority().history_len(), 0);
    owner
        .admit(snapshot(basis, 0, 1, 90, 110).snapshot())
        .unwrap();
    let before = state(&owner);
    assert!(owner
        .admit_target(TargetNativeAudioSnapshot {
            epoch: 7,
            basis: target,
            evidence: snapshot(basis, 1, 1, 100, 102).snapshot().evidence,
        })
        .is_err());
    assert_eq!(state(&owner), before);
}
