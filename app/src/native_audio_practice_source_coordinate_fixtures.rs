//! Genuine direct-output practice receipts and original native DI evidence.
//! These tests prove owner transactions, not physical device acquisition.
use super::*;
use crate::audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis as Basis};
use crate::native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate};
use beatkernel::{
    audio::*,
    input::{ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent},
    time::{ClockDomainId, ClockPair, Duration, Timestamp},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
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
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}

fn direct_receipt() -> (ProjectedPracticeReceipt, OutputFrameBasis) {
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(8, 256, 1).unwrap()).unwrap();
    let program =
        PreparedPracticeProgram::new(&bank, vec![], PracticeLimits::new(8, 1, 64, 8, 16).unwrap())
            .unwrap();
    let (mut controller, endpoint) = practice_queue(&program).unwrap();
    let (_, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(2_000_000_000), false)
                .unwrap(),
        )
        .unwrap();
    controller
        .try_request(PracticeRequest {
            id: 12,
            expected_generation: 1,
            at_playback_frame: 1,
            region: PracticeRegion::new(
                Timestamp::from_nanos(333_333_333),
                Timestamp::from_nanos(2_000_000_000),
                false,
            )
            .unwrap(),
        })
        .unwrap();
    // Capture immutable creation metadata before any generation.
    let creation = mixer.output_frame_basis();
    mixer.render(&mut [0.0; 4]).unwrap();
    assert_eq!(
        controller.try_pop_projected_receipt().unwrap().receipt.kind,
        PracticeBoundaryKind::Started
    );
    let projected = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(projected.receipt.kind, PracticeBoundaryKind::Requested);
    assert_eq!(
        (
            projected.receipt.request_id,
            projected.receipt.physical_frame,
            projected.target_frame,
            projected.target_rate
        ),
        (12, 1, 1, 3)
    );
    assert_eq!(
        projected.boundary.target_time,
        TargetTime::from_frames(1, 3).unwrap()
    );
    (projected, creation)
}

fn cold(source: OutputFrameBasis, counter: bool, capacity: usize) -> NativeAudioPresentation {
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: source.point_at_stream_frame(0).unwrap(),
        logical_origin: point(3, 5_000_000_000),
        host_domain: ClockDomainId(1),
    };
    let descriptor = if counter {
        Basis::SourceCounter {
            basis: source,
            frequency: 7,
        }
    } else {
        Basis::SourceFrame(source)
    };
    NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig {
                history_capacity: capacity,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                ..AudioAuthorityConfig::default()
            },
            epoch,
            descriptor,
        )
        .unwrap(),
        NativePresentationValidator::new(7, epoch.stream_origin, epoch.host_domain),
    )
    .unwrap()
}
fn observe(owner: &mut NativeAudioPresentation, source: OutputFrameBasis, units: u64, at: i64) {
    let (evidence, coordinate) = match owner.authority().coordinate_basis().unwrap() {
        Basis::SourceFrame(_) => (
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: source.point_at_stream_frame(units).unwrap(),
                target: host(at),
            }),
            NativeOutputCoordinate::StreamFrame { frame: units },
        ),
        Basis::SourceCounter { frequency, .. } => (
            OriginalNativePresentationEvidence::Wasapi {
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters::default(),
                    render: None,
                    clock: Some(AudioClockSnapshot {
                        position: units,
                        frequency,
                        qpc_100ns: units + 1,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(host(at)),
                        mapping_quality: beatkernel::time::ClockMappingQuality::Unknown,
                    }),
                },
                basis: Some(source),
            },
            NativeOutputCoordinate::Counter {
                position: units,
                frequency,
            },
        ),
        _ => panic!("Source-only fixture"),
    };
    owner
        .admit_coordinate(
            CoordinateNativeAudioSnapshot::new(
                NativeAudioSnapshot {
                    epoch: 7,
                    basis: source,
                    evidence,
                },
                coordinate,
            )
            .unwrap(),
        )
        .unwrap();
}
fn seeded(source: OutputFrameBasis, counter: bool, capacity: usize) -> NativeAudioPresentation {
    let mut owner = cold(source, counter, capacity);
    observe(&mut owner, source, u64::from(counter), 100);
    owner.pin_practice_mixer_basis(source).unwrap();
    owner
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}
fn event(at: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(at), sequence),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}

#[test]
fn genuine_direct_receipt_waits_for_source_native_crossing_and_plain_receipt_agrees() {
    let (projected, source) = direct_receipt();
    for counter in [false, true] {
        let mut owner = cold(source, counter, 8);
        assert!(owner
            .prepare_projected_practice_boundary(projected, host(100))
            .is_err());
        assert!(owner.pin_practice_mixer_basis(source).is_err());
        observe(&mut owner, source, u64::from(counter), 100);
        owner.pin_practice_mixer_basis(source).unwrap();
        assert!(owner
            .prepare_projected_practice_boundary(projected, host(100))
            .unwrap()
            .is_none());
        if counter {
            // Two 7-Hz counter ticks precede one 3-Hz Source sample.
            observe(&mut owner, source, 2, 600);
            assert!(owner
                .prepare_projected_practice_boundary(projected, host(600))
                .unwrap()
                .is_none());
            assert!(owner
                .prepare_practice_boundary(projected.receipt, host(600))
                .unwrap()
                .is_none());
        }
        observe(&mut owner, source, 3, 1100);
        let projected_cut = owner
            .prepare_projected_practice_boundary(projected, host(1100))
            .unwrap()
            .unwrap();
        let plain_cut = owner
            .prepare_practice_boundary(projected.receipt, host(1100))
            .unwrap()
            .unwrap();
        assert_eq!(plain_cut.host(), projected_cut.host());
        assert_eq!(plain_cut.raw_output(), projected_cut.raw_output());
        assert_eq!(plain_cut.projected_receipt(), None);
        assert_eq!(projected_cut.projected_receipt(), Some(projected));
        for cut in [projected_cut, plain_cut] {
            assert!(cut
                .coordinate_query()
                .unwrap()
                .unwrap()
                .compare_interpolated_time(exact(TargetTime::from_frames(1, 3).unwrap()))
                .is_eq());
        }
    }
}

#[test]
fn source_hz_and_counter_frequency_stay_distinct_and_host_alone_rounds_up() {
    let (projected, source) = direct_receipt();
    for (counter, expected_host) in [(false, 434), (true, 767)] {
        let mut owner = seeded(source, counter, 8);
        observe(&mut owner, source, 3, 1100);
        let cut = owner
            .prepare_projected_practice_boundary(projected, host(1100))
            .unwrap()
            .unwrap();
        // Frame: ceil(100 + 1000/3). Counter: ceil(100 + (7/3 - 1)*500).
        assert_eq!(cut.host(), host(expected_host));
        assert_eq!(cut.raw_output(), point(2, 333_333_333));
        assert_eq!(cut.output(), point(3, 5_333_333_333));
        let query = cut.coordinate_query().unwrap().unwrap();
        assert!(query
            .compare_target_time(TargetTime::from_frames(1, 3).unwrap())
            .is_eq());
        assert!(query
            .compare_target_time(TargetTime::from_frames(1, 7).unwrap())
            .is_gt());
        assert!(query
            .compare_target_time(TargetTime::from_frames(333_333_333, 1_000_000_000).unwrap())
            .is_gt());
    }
}

#[test]
fn source_cut_requires_acquisition_and_pre_cut_drain_preserving_equal_input_and_history() {
    let (projected, source) = direct_receipt();
    for counter in [false, true] {
        let mut owner = seeded(source, counter, 8);
        observe(&mut owner, source, 3, 1100);
        let now = host(1100);
        let cut = owner
            .prepare_projected_practice_boundary(projected, now)
            .unwrap()
            .unwrap();
        let at = cut.host().timestamp.as_nanos();
        let mut inputs = merger();
        inputs.admit(event(at - 1, 1), now).unwrap();
        inputs.admit(event(at, 2), now).unwrap();
        assert!(!owner.practice_boundary_ready(&cut, now, &inputs).unwrap());
        owner.authority_mut().record_acquired_prefix(now).unwrap();
        assert!(!owner.practice_boundary_ready(&cut, now, &inputs).unwrap());
        assert!(owner.commit_practice_boundary(cut, now, &inputs).is_err());
        assert!(owner.authority().committed_coordinate_operation().is_none());
        assert_eq!(
            inputs.pop_ready(now).unwrap().unwrap().meta().timestamp,
            Timestamp::from_nanos(at - 1)
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
            Timestamp::from_nanos(at)
        );
        assert_eq!(inputs.pending(), 1);
    }
}

#[test]
fn corrupted_direct_projection_and_prepared_identity_refuse_without_owner_mutation() {
    let (projected, source) = direct_receipt();
    for counter in [false, true] {
        let mut owner = seeded(source, counter, 8);
        observe(&mut owner, source, 3, 1100);
        owner
            .authority_mut()
            .record_acquired_prefix(host(1100))
            .unwrap();
        let before = format!("{owner:?}");
        let mut wrong_rate = projected;
        wrong_rate.target_rate += 1;
        let mut wrong_origin = projected;
        wrong_origin.origin = point(99, 0);
        let mut wrong_source = projected;
        wrong_source.boundary.source_frame += 1;
        let mut wrong_target = projected;
        wrong_target.target_frame += 1;
        let mut wrong_time = projected;
        wrong_time.boundary.target_time =
            TargetTime::from_frames(1_333_333_333, 4_000_000_000).unwrap();
        assert_ne!(
            wrong_time.boundary.target_time,
            projected.boundary.target_time
        );
        assert_eq!(
            wrong_time
                .boundary
                .target_time
                .point(projected.origin)
                .unwrap(),
            projected
                .boundary
                .target_time
                .point(projected.origin)
                .unwrap()
        );
        for invalid in [
            wrong_rate,
            wrong_origin,
            wrong_source,
            wrong_target,
            wrong_time,
        ] {
            assert!(owner
                .prepare_projected_practice_boundary(invalid, host(1100))
                .is_err());
            assert_eq!(format!("{owner:?}"), before);
        }
        let cut = owner
            .prepare_projected_practice_boundary(projected, host(1100))
            .unwrap()
            .unwrap();
        let mut wrong_epoch = cut;
        wrong_epoch.epoch.id += 1;
        let mut wrong_creation = cut;
        wrong_creation.mixer_basis = OutputFrameBasis::new(source.origin(), 4, 0).unwrap();
        let mut wrong_descriptor = cut;
        wrong_descriptor.coordinate_basis = Some(Basis::SourceCounter {
            basis: source,
            frequency: 11,
        });
        let inputs = merger();
        for invalid in [wrong_epoch, wrong_creation, wrong_descriptor] {
            assert!(owner
                .practice_boundary_ready(&invalid, host(1100), &inputs)
                .is_err());
            assert!(owner
                .commit_practice_boundary(invalid, host(1100), &inputs)
                .is_err());
            assert_eq!(format!("{owner:?}"), before);
        }
    }
}

#[test]
fn retained_original_source_qualification_expiry_rejects_prepared_boundary() {
    let (projected, source) = direct_receipt();
    for counter in [false, true] {
        let mut owner = seeded(source, counter, 2);
        observe(&mut owner, source, 3, 1100);
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
        observe(&mut owner, source, 4, 2100);
        owner
            .authority_mut()
            .record_acquired_prefix(host(2100))
            .unwrap();
        let before = format!("{owner:?}");
        assert!(owner
            .practice_boundary_ready(&cut, host(2100), &inputs)
            .is_err());
        assert!(owner
            .commit_practice_boundary(cut, host(2100), &inputs)
            .is_err());
        assert_eq!(format!("{owner:?}"), before);
        assert!(owner.authority().committed_coordinate_operation().is_none());
    }
}
