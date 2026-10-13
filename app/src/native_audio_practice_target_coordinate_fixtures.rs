//! Genuine converted practice receipts; supplied native pairs exercise the DI
//! boundary, not acoustic timing or a fresh attempt's transport/capture wiring.
use super::*;
use crate::audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis as Basis};
use crate::native_output_coordinate::NativeOutputCoordinate;
use beatkernel::{
    audio::*,
    input::{ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent},
    time::{ClockDomainId, ClockPair, Duration, Timestamp},
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn host(nanos: i64) -> ClockPoint {
    point(1, nanos)
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}

fn receipt() -> (ProjectedPracticeReceipt, OutputFrameBasis, TargetFrameBasis) {
    let source = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(source, PcmLimits::new(8, 256, 1).unwrap()).unwrap();
    let program =
        PreparedPracticeProgram::new(&bank, vec![], PracticeLimits::new(8, 1, 64, 8, 16).unwrap())
            .unwrap();
    let (mut controller, endpoint) = practice_queue(&program).unwrap();
    let (_, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            source,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let region =
        PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), false).unwrap();
    mixer.install_practice(program, endpoint, region).unwrap();
    controller
        .try_request(PracticeRequest {
            id: 12,
            expected_generation: 1,
            at_playback_frame: 1,
            region: PracticeRegion::new(
                Timestamp::from_nanos(20_000_000),
                Timestamp::from_nanos(100_000_000),
                false,
            )
            .unwrap(),
        })
        .unwrap();
    let cold_source = mixer.output_frame_basis();
    let mut output = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("valid original converter"));
    let cold_target = output.target_frame_basis();
    output.render_pending(8).unwrap();
    output.admit(8).unwrap();
    assert_eq!(
        controller.try_pop_projected_receipt().unwrap().receipt.kind,
        PracticeBoundaryKind::Started
    );
    let projected = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(projected.receipt.request_id, 12);
    assert_eq!(projected.receipt.physical_frame, 1);
    assert_eq!(projected.target_frame, 2);
    assert_eq!(
        projected.boundary.target_time,
        TargetTime::from_frames(2, 1500).unwrap()
    );
    (projected, cold_source, cold_target)
}
fn cold_presentation(
    target: TargetFrameBasis,
    coordinate: bool,
    capacity: usize,
) -> NativeAudioPresentation {
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: target.point_at_stream_frame(0).unwrap(),
        logical_origin: point(3, 5_000_000_000),
        host_domain: ClockDomainId(1),
    };
    let config = AudioAuthorityConfig {
        history_capacity: capacity,
        max_observation_age: Duration::from_nanos(1_000_000_000),
        ..AudioAuthorityConfig::default()
    };
    let authority = if coordinate {
        AudioAuthority::new_with_coordinates(config, epoch, Basis::TargetFrame(target)).unwrap()
    } else {
        AudioAuthority::new(config, epoch).unwrap()
    };
    NativeAudioPresentation::new(
        authority,
        NativePresentationValidator::new(7, epoch.stream_origin, epoch.host_domain),
    )
    .unwrap()
}
fn presentation(
    source: OutputFrameBasis,
    target: TargetFrameBasis,
    coordinate: bool,
    capacity: usize,
) -> NativeAudioPresentation {
    let mut owner = cold_presentation(target, coordinate, capacity);
    observe(&mut owner, target, 0, 100);
    owner.pin_practice_mixer_basis(source).unwrap();
    owner
}
fn observe(owner: &mut NativeAudioPresentation, target: TargetFrameBasis, frame: u64, nanos: i64) {
    let snapshot = TargetNativeAudioSnapshot {
        epoch: 7,
        basis: target,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: target.point_at_stream_frame(frame).unwrap(),
            target: host(nanos),
        }),
    };
    if owner.authority().coordinate_basis().is_some() {
        owner
            .admit_target_coordinate(
                CoordinateTargetNativeAudioSnapshot::new(
                    snapshot,
                    NativeOutputCoordinate::StreamFrame { frame },
                )
                .unwrap(),
            )
            .unwrap();
    } else {
        owner.admit_target(snapshot).unwrap();
    }
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}
fn event(nanos: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(nanos), sequence),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}

#[test]
fn generated_and_admitted_receipt_waits_for_actual_native_coordinate_crossing() {
    let (projected, source, target) = receipt();
    let mut owner = cold_presentation(target, true, 8);
    // Before the first original observation, no native creation pin exists.
    assert!(owner
        .prepare_projected_practice_boundary(projected, host(100))
        .is_err());
    assert!(owner.pin_practice_mixer_basis(source).is_err());
    observe(&mut owner, target, 0, 100);
    owner.pin_practice_mixer_basis(source).unwrap();
    assert!(owner
        .prepare_projected_practice_boundary(projected, host(100))
        .unwrap()
        .is_none());
    observe(&mut owner, target, 1, 1100);
    assert!(owner
        .prepare_projected_practice_boundary(projected, host(1100))
        .unwrap()
        .is_none());
    observe(&mut owner, target, 2, 2100);
    let cut = owner
        .prepare_projected_practice_boundary(projected, host(2100))
        .unwrap()
        .unwrap();
    assert_eq!(cut.host(), host(2100));
    assert_eq!(cut.projected_receipt(), Some(projected));
    assert!(cut
        .coordinate_query()
        .unwrap()
        .unwrap()
        .compare_interpolated_time(exact(projected.boundary.target_time))
        .is_eq());
    assert!(owner
        .prepare_practice_boundary(projected.receipt, host(2100))
        .is_err());
}

#[test]
fn nondivisor_target_and_fractional_host_crossing_round_only_host_upward() {
    let (projected, source, target) = receipt();
    let mut owner = presentation(source, target, true, 8);
    observe(&mut owner, target, 3, 1100);
    let cut = owner
        .prepare_projected_practice_boundary(projected, host(1100))
        .unwrap()
        .unwrap();
    // 2/3 of the original 1000ns HOST interval is 666+2/3ns.
    assert_eq!(cut.host(), host(767));
    assert_eq!(cut.raw_output(), point(2, 1_333_333));
    assert_eq!(cut.output(), point(3, 5_001_333_333));
    let query = cut.coordinate_query().unwrap().unwrap();
    assert!(query
        .compare_target_time(TargetTime::from_frames(2, 1500).unwrap())
        .is_eq());
    assert_ne!(
        (u128::from(projected.boundary.target_time.numerator()) * 1_000_000_000)
            % u128::from(projected.boundary.target_time.denominator()),
        0
    );
}

#[test]
fn acquired_prefix_and_strictly_earlier_input_gate_commit_equal_input_survives() {
    let (projected, source, target) = receipt();
    let mut owner = presentation(source, target, true, 8);
    observe(&mut owner, target, 3, 1100);
    let now = host(1100);
    let cut = owner
        .prepare_projected_practice_boundary(projected, now)
        .unwrap()
        .unwrap();
    let mut inputs = merger();
    inputs.admit(event(766, 1), now).unwrap();
    inputs.admit(event(767, 2), now).unwrap();
    assert!(!owner.practice_boundary_ready(&cut, now, &inputs).unwrap());
    owner.authority_mut().record_acquired_prefix(now).unwrap();
    assert!(!owner.practice_boundary_ready(&cut, now, &inputs).unwrap());
    assert!(owner.commit_practice_boundary(cut, now, &inputs).is_err());
    assert!(owner.authority().committed_coordinate_operation().is_none());
    assert_eq!(
        inputs.pop_ready(now).unwrap().unwrap().meta().timestamp,
        Timestamp::from_nanos(766)
    );
    assert!(owner.practice_boundary_ready(&cut, now, &inputs).unwrap());
    let before = (
        owner.authority().epoch(),
        owner.authority().coordinate_basis(),
        owner.latest_record(),
        owner.authority().history_len(),
        owner.authority().acquired_prefix(),
        owner.authority().closed_host_prefix(),
        owner.authority().committed_input_host(),
        owner.authority().committed_presentation(),
    );
    owner.commit_practice_boundary(cut, now, &inputs).unwrap();
    assert_eq!(
        (
            owner.authority().epoch(),
            owner.authority().coordinate_basis(),
            owner.latest_record(),
            owner.authority().history_len(),
            owner.authority().acquired_prefix(),
            owner.authority().closed_host_prefix(),
            owner.authority().committed_input_host(),
            owner.authority().committed_presentation()
        ),
        before
    );
    assert!(owner
        .authority()
        .committed_coordinate_operation()
        .unwrap()
        .compare_interpolated_time(cut.coordinate_query().unwrap().unwrap())
        .is_eq());
    assert_eq!(owner.authority().committed_operation(), Some(cut.output()));
    assert_eq!(
        inputs.peek_ready(now).unwrap().unwrap().meta().timestamp,
        Timestamp::from_nanos(767)
    );
    assert_eq!(inputs.pending(), 1);
}

#[test]
fn malformed_copied_projection_refuses_without_changing_native_or_authority_state() {
    let (projected, source, target) = receipt();
    let mut owner = presentation(source, target, true, 8);
    observe(&mut owner, target, 3, 1100);
    let before = (
        owner.latest_record(),
        owner.authority().latest_observation(),
        owner.authority().history_len(),
    );
    let mut wrong_rate = projected;
    wrong_rate.target_rate += 1;
    let mut wrong_origin = projected;
    wrong_origin.origin = point(99, 0);
    let mut wrong_source = projected;
    wrong_source.boundary.source_frame += 1;
    for invalid in [wrong_rate, wrong_origin, wrong_source] {
        assert!(owner
            .prepare_projected_practice_boundary(invalid, host(1100))
            .is_err());
        assert_eq!(
            (
                owner.latest_record(),
                owner.authority().latest_observation(),
                owner.authority().history_len()
            ),
            before
        );
        assert!(owner.authority().committed_coordinate_operation().is_none());
    }
}

#[test]
fn expired_original_qualification_cannot_commit_an_old_boundary() {
    let (projected, source, target) = receipt();
    let mut owner = presentation(source, target, true, 2);
    observe(&mut owner, target, 3, 1100);
    let cut = owner
        .prepare_projected_practice_boundary(projected, host(1100))
        .unwrap()
        .unwrap();
    let mut inputs = merger();
    owner
        .authority_mut()
        .record_acquired_prefix(host(1100))
        .unwrap();
    let frontier = owner
        .authority()
        .prepare_coordinate_held_frontier(host(1100), &inputs)
        .unwrap()
        .unwrap();
    owner
        .authority_mut()
        .commit_coordinate_held_frontier(frontier, host(1100), &mut inputs)
        .unwrap();
    assert_eq!(owner.authority().closed_host_prefix(), Some(host(1100)));
    assert!(owner.authority().committed_coordinate_operation().is_none());
    observe(&mut owner, target, 4, 2100);
    owner
        .authority_mut()
        .record_acquired_prefix(host(2100))
        .unwrap();
    let before = (
        owner.latest_record(),
        owner.authority().acquired_prefix(),
        owner.authority().closed_host_prefix(),
    );
    assert!(owner
        .practice_boundary_ready(&cut, host(2100), &inputs)
        .is_err());
    assert!(owner
        .commit_practice_boundary(cut, host(2100), &inputs)
        .is_err());
    assert_eq!(
        (
            owner.latest_record(),
            owner.authority().acquired_prefix(),
            owner.authority().closed_host_prefix()
        ),
        before
    );
    assert!(owner.authority().committed_coordinate_operation().is_none());
}

#[test]
fn token_epoch_and_creation_grid_binding_reject_corrupted_copies() {
    let (projected, source, target) = receipt();
    let mut owner = presentation(source, target, true, 8);
    observe(&mut owner, target, 3, 1100);
    owner
        .authority_mut()
        .record_acquired_prefix(host(1100))
        .unwrap();
    let cut = owner
        .prepare_projected_practice_boundary(projected, host(1100))
        .unwrap()
        .unwrap();
    let mut wrong_epoch = cut;
    wrong_epoch.epoch.id += 1;
    let mut wrong_basis = cut;
    wrong_basis.mixer_basis = OutputFrameBasis::new(source.origin(), 1001, 0).unwrap();
    let inputs = merger();
    let before = owner.latest_record();
    for invalid in [wrong_epoch, wrong_basis] {
        assert!(owner
            .practice_boundary_ready(&invalid, host(1100), &inputs)
            .is_err());
        assert!(owner
            .commit_practice_boundary(invalid, host(1100), &inputs)
            .is_err());
        assert_eq!(owner.latest_record(), before);
        assert!(owner.authority().committed_coordinate_operation().is_none());
    }
}

#[test]
fn scalar_projected_boundary_remains_scalar_and_source_coordinate_mode_refuses() {
    let (projected, source, target) = receipt();
    let mut scalar = presentation(source, target, false, 8);
    observe(&mut scalar, target, 3, 1100);
    let cut = scalar
        .prepare_projected_practice_boundary(projected, host(1100))
        .unwrap()
        .unwrap();
    assert!(cut.coordinate_query().unwrap().is_none());
    scalar
        .authority_mut()
        .record_acquired_prefix(host(1100))
        .unwrap();
    let inputs = merger();
    assert!(scalar
        .practice_boundary_ready(&cut, host(1100), &inputs)
        .unwrap());
    scalar
        .commit_practice_boundary(cut, host(1100), &inputs)
        .unwrap();
    assert!(scalar
        .authority()
        .committed_coordinate_operation()
        .is_none());
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: source.origin(),
        logical_origin: point(3, 5_000_000_000),
        host_domain: ClockDomainId(1),
    };
    let authority = AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig::default(),
        epoch,
        Basis::SourceFrame(source),
    )
    .unwrap();
    let mut owner = NativeAudioPresentation::new(
        authority,
        NativePresentationValidator::new(7, epoch.stream_origin, epoch.host_domain),
    )
    .unwrap();
    for (frame, nanos) in [(0, 100), (3, 1100)] {
        owner
            .admit_coordinate(
                CoordinateNativeAudioSnapshot::new(
                    NativeAudioSnapshot {
                        epoch: 7,
                        basis: source,
                        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                            source: source.point_at_stream_frame(frame).unwrap(),
                            target: host(nanos),
                        }),
                    },
                    NativeOutputCoordinate::StreamFrame { frame },
                )
                .unwrap(),
            )
            .unwrap();
    }
    owner.pin_practice_mixer_basis(source).unwrap();
    let before = owner.latest_record();
    assert!(owner
        .prepare_projected_practice_boundary(projected, host(1100))
        .is_err());
    assert_eq!(owner.latest_record(), before);
    assert!(owner.authority().committed_coordinate_operation().is_none());
}
