//! Real Mixer events qualified by original native DI evidence and acquired input.
//! Supplied native observations exercise ownership, not physical audio latency.
use super::*;
use crate::audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis as Basis};
use crate::native_output_coordinate::NativeOutputCoordinate;
use beatkernel::{
    audio::*,
    input::{ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent},
    time::{ClockDomainId, ClockPair, Duration, Timestamp},
    transport::Rate,
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn mixer(hz: u32) -> (Mixer, PracticeController) {
    let format = AudioFormat::new(hz, 1).unwrap();
    let limits = PcmLimits::new(128, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, (0..32).map(|i| i as f32 / 32.0).collect(), limits).unwrap(),
    )
    .unwrap();
    let program = PreparedPracticeProgram::new_with_runtime_rates(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        PracticeLimits::new(8, 1, 64, 8, 16).unwrap(),
        Rate::new(1, 2).unwrap(),
        Duration::ZERO,
        false,
        14045,
    )
    .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
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
            PracticeRegion::new(
                Timestamp::ZERO,
                Timestamp::from_nanos(10_000_000_000),
                false,
            )
            .unwrap(),
        )
        .unwrap();
    (mixer, controller)
}
fn direct() -> (ProjectedPracticeEvent, OutputFrameBasis) {
    let (mut mixer, mut controller) = mixer(3);
    let basis = mixer.output_frame_basis();
    mixer.render(&mut [0.0; 1]).unwrap();
    assert!(matches!(
        controller.try_pop_projected_event().unwrap().event,
        PracticeEvent::Boundary(PracticeReceipt {
            kind: PracticeBoundaryKind::Started,
            ..
        })
    ));
    let prepared = controller.prepare_rate(Rate::new(3, 2).unwrap()).unwrap();
    controller.try_set_rate_next(12, 1, &prepared).unwrap();
    let mut suffix = [0.0; 3];
    mixer.render(&mut suffix).unwrap();
    assert_eq!(suffix, [0.015625, 0.0625, 0.109375]);
    let projected = controller.try_pop_projected_event().unwrap();
    assert_eq!(projected.target_frame, 1);
    match projected.event {
        PracticeEvent::Rate(rate) => {
            assert_eq!(
                (rate.request_id, rate.physical_frame, rate.playback_frame),
                (12, 1, 1)
            );
            assert_eq!(rate.active_elapsed_nanos, 333_333_333);
            assert_eq!(rate.outcome, PracticeRateOutcome::Applied);
        }
        event => panic!("expected genuine Rate, got {event:?}"),
    }
    (projected, basis)
}
fn source_owner(source: OutputFrameBasis, epoch_id: u64) -> NativeAudioPresentation {
    let epoch = AudioAuthorityEpoch {
        id: epoch_id,
        stream_origin: source.point_at_stream_frame(0).unwrap(),
        logical_origin: point(3, 5_000_000_000),
        host_domain: ClockDomainId(1),
    };
    NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig {
                history_capacity: 8,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                ..Default::default()
            },
            epoch,
            Basis::SourceFrame(source),
        )
        .unwrap(),
        NativePresentationValidator::new(epoch_id, epoch.stream_origin, epoch.host_domain),
    )
    .unwrap()
}
fn source_observe(
    owner: &mut NativeAudioPresentation,
    source: OutputFrameBasis,
    frame: u64,
    at: i64,
) {
    owner
        .admit_coordinate(
            CoordinateNativeAudioSnapshot::new(
                NativeAudioSnapshot {
                    epoch: owner.authority().epoch().id,
                    basis: source,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                        source: source.point_at_stream_frame(frame).unwrap(),
                        target: host(at),
                    }),
                },
                NativeOutputCoordinate::StreamFrame { frame },
            )
            .unwrap(),
        )
        .unwrap();
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}
fn input(at: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(at), sequence),
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    })
}

#[test]
fn actual_rate_is_native_qualified_without_fabricating_a_legacy_boundary() {
    let (projected, source) = direct();
    let mut owner = source_owner(source, 7);
    assert!(owner
        .prepare_projected_practice_event(projected, host(100))
        .is_err());
    source_observe(&mut owner, source, 0, 100);
    owner.pin_practice_mixer_basis(source).unwrap();
    assert!(owner
        .prepare_projected_practice_event(projected, host(100))
        .unwrap()
        .is_none());
    source_observe(&mut owner, source, 3, 1100);
    let cut = owner
        .prepare_projected_practice_event(projected, host(1100))
        .unwrap()
        .unwrap();
    assert_eq!(cut.projected_event(), projected);
    assert_eq!(cut.host(), host(434));
    assert_eq!(cut.raw_output(), point(2, 333_333_333));
    assert_eq!(cut.output(), point(3, 5_333_333_333));
    assert!(cut
        .coordinate_query()
        .unwrap()
        .unwrap()
        .compare_target_time(TargetTime::from_frames(1, 3).unwrap())
        .is_eq());
}

#[test]
fn rate_cut_needs_acquisition_and_pre_cut_drain_while_equal_input_survives() {
    let (projected, source) = direct();
    let mut owner = source_owner(source, 7);
    source_observe(&mut owner, source, 0, 100);
    owner.pin_practice_mixer_basis(source).unwrap();
    source_observe(&mut owner, source, 3, 1100);
    let now = host(1100);
    let cut = owner
        .prepare_projected_practice_event(projected, now)
        .unwrap()
        .unwrap();
    let mut inputs = merger();
    inputs.admit(input(433, 1), now).unwrap();
    inputs.admit(input(434, 2), now).unwrap();
    assert!(!owner
        .practice_event_boundary_ready(&cut, now, &inputs)
        .unwrap());
    owner.authority_mut().record_acquired_prefix(now).unwrap();
    assert!(!owner
        .practice_event_boundary_ready(&cut, now, &inputs)
        .unwrap());
    assert!(owner
        .commit_practice_event_boundary(cut, now, &inputs)
        .is_err());
    assert!(owner.authority().committed_coordinate_operation().is_none());
    assert_eq!(
        inputs.pop_ready(now).unwrap().unwrap().meta().timestamp,
        Timestamp::from_nanos(433)
    );
    assert!(owner
        .practice_event_boundary_ready(&cut, now, &inputs)
        .unwrap());
    owner
        .commit_practice_event_boundary(cut, now, &inputs)
        .unwrap();
    assert_eq!(owner.authority().committed_operation(), Some(cut.output()));
    assert_eq!(
        inputs.peek_ready(now).unwrap().unwrap().meta().timestamp,
        Timestamp::from_nanos(434)
    );
    assert_eq!(inputs.pending(), 1);
}

#[test]
fn mismatched_projection_epoch_and_clock_domain_leave_native_owner_unchanged() {
    let (projected, source) = direct();
    let mut owner = source_owner(source, 7);
    source_observe(&mut owner, source, 0, 100);
    owner.pin_practice_mixer_basis(source).unwrap();
    source_observe(&mut owner, source, 3, 1100);
    owner
        .authority_mut()
        .record_acquired_prefix(host(1100))
        .unwrap();
    let cut = owner
        .prepare_projected_practice_event(projected, host(1100))
        .unwrap()
        .unwrap();
    let before = format!("{owner:?}");
    let mut wrong_origin = projected;
    wrong_origin.origin.domain = ClockDomainId(99);
    let mut wrong_rate = projected;
    wrong_rate.target_rate += 1;
    let mut wrong_source = projected;
    wrong_source.boundary.source_frame += 1;
    let mut wrong_target = projected;
    wrong_target.target_frame += 1;
    let mut wrong_time = projected;
    wrong_time.boundary.target_time = TargetTime::from_frames(333_333_333, 1_000_000_000).unwrap();
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
    for bad in [
        wrong_origin,
        wrong_rate,
        wrong_source,
        wrong_target,
        wrong_time,
    ] {
        assert!(owner
            .prepare_projected_practice_event(bad, host(1100))
            .is_err());
        assert_eq!(format!("{owner:?}"), before);
    }
    assert!(owner
        .practice_event_boundary_ready(&cut, point(99, 1100), &merger())
        .is_err());
    let mut other_epoch = source_owner(source, 8);
    source_observe(&mut other_epoch, source, 0, 100);
    other_epoch.pin_practice_mixer_basis(source).unwrap();
    source_observe(&mut other_epoch, source, 3, 1100);
    other_epoch
        .authority_mut()
        .record_acquired_prefix(host(1100))
        .unwrap();
    let other_before = format!("{other_epoch:?}");
    assert!(other_epoch
        .practice_event_boundary_ready(&cut, host(1100), &merger())
        .is_err());
    assert!(other_epoch
        .commit_practice_event_boundary(cut, host(1100), &merger())
        .is_err());
    assert_eq!(format!("{other_epoch:?}"), other_before);
}

#[test]
fn actual_converted_rate_waits_for_original_target_coordinate_not_source_hz() {
    let (mixer, mut controller) = mixer(1000);
    let source = mixer.output_frame_basis();
    let mut output = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("valid converter"));
    let target = output.target_frame_basis();
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    assert!(matches!(
        controller.try_pop_projected_event().unwrap().event,
        PracticeEvent::Boundary(PracticeReceipt {
            kind: PracticeBoundaryKind::Started,
            ..
        })
    ));
    let prepared = controller.prepare_rate(Rate::new(3, 2).unwrap()).unwrap();
    controller.try_set_rate_next(12, 1, &prepared).unwrap();
    output.render_pending(8).unwrap();
    output.admit(8).unwrap();
    let projected = controller.try_pop_projected_event().unwrap();
    assert!(matches!(
        projected.event,
        PracticeEvent::Rate(PracticeRateEvent {
            outcome: PracticeRateOutcome::Applied,
            ..
        })
    ));
    assert_eq!(projected.target_rate, 1500);
    assert!(projected.target_frame > 0);
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: target.point_at_stream_frame(0).unwrap(),
        logical_origin: point(3, 5_000_000_000),
        host_domain: ClockDomainId(1),
    };
    let mut owner = NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig {
                history_capacity: 8,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                ..Default::default()
            },
            epoch,
            Basis::TargetFrame(target),
        )
        .unwrap(),
        NativePresentationValidator::new(7, epoch.stream_origin, epoch.host_domain),
    )
    .unwrap();
    let observe = |owner: &mut NativeAudioPresentation, frame, at| {
        owner
            .admit_target_coordinate(
                CoordinateTargetNativeAudioSnapshot::new(
                    TargetNativeAudioSnapshot {
                        epoch: 7,
                        basis: target,
                        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                            source: target.point_at_stream_frame(frame).unwrap(),
                            target: host(at),
                        }),
                    },
                    NativeOutputCoordinate::StreamFrame { frame },
                )
                .unwrap(),
            )
            .unwrap();
    };
    observe(&mut owner, 0, 100);
    owner.pin_practice_mixer_basis(source).unwrap();
    observe(&mut owner, projected.target_frame - 1, 1100);
    assert!(owner
        .prepare_projected_practice_event(projected, host(1100))
        .unwrap()
        .is_none());
    observe(&mut owner, projected.target_frame, 2100);
    let cut = owner
        .prepare_projected_practice_event(projected, host(2100))
        .unwrap()
        .unwrap();
    assert_eq!(cut.host(), host(2100));
    assert!(cut
        .coordinate_query()
        .unwrap()
        .unwrap()
        .compare_target_time(projected.boundary.target_time)
        .is_eq());
    assert_eq!(
        projected.target_frame,
        (projected.boundary.source_frame * 3 + 1) / 2
    );
    assert_eq!(
        projected.boundary.target_time,
        TargetTime::from_frames(projected.target_frame, 1500).unwrap()
    );
    assert_ne!(projected.target_rate, source.sample_rate());
    owner
        .authority_mut()
        .record_acquired_prefix(host(2100))
        .unwrap();
    owner
        .commit_practice_event_boundary(cut, host(2100), &merger())
        .unwrap();
    assert_eq!(owner.authority().committed_operation(), Some(cut.output()));
}
