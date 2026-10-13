//! Original native Target epoch replacement on a retained, unique converted owner.
//! Injected native relations prove software publication, not a physical output crossing.
//! The 4 GHz cases exercise native-wrapper arithmetic only, not PCM/device support.
use super::*;
use crate::audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis};
use crate::native_output_coordinate::NativeOutputCoordinateError;
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, ChannelMatrix, Mixer, MixerConfig, PcmLimits, PracticeLimits,
        PracticeRegion, PreparedPracticeProgram, ResampleQuality, SampleBank, command_queue,
        practice_queue,
    },
    input::{ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, Duration, Timestamp},
};
use beatkernel_platform::audio::{
    AudioStreamSnapshot, AudioStreamStatus, ConvertedNativeOutputState, DeviceFormat,
    SampleEncoding, StreamCounters,
};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

fn host(nanos: i64) -> ClockPoint {
    point(1, nanos)
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

fn snapshot(
    epoch: u64,
    basis: TargetFrameBasis,
    frame: u64,
    at: ClockPoint,
) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: at,
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}

fn cold_owner(basis: TargetFrameBasis, coordinate: bool) -> NativeAudioPresentation {
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: basis.point_at_stream_frame(0).unwrap(),
        logical_origin: point(3, 500),
        host_domain: ClockDomainId(1),
    };
    let config = AudioAuthorityConfig {
        history_capacity: 8,
        max_observation_age: Duration::from_nanos(1000),
        ..AudioAuthorityConfig::default()
    };
    let authority = if coordinate {
        AudioAuthority::new_with_coordinates(
            config,
            epoch,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
        )
        .unwrap()
    } else {
        AudioAuthority::new(config, epoch).unwrap()
    };
    NativeAudioPresentation::new(
        authority,
        NativePresentationValidator::new(epoch.id, epoch.stream_origin, epoch.host_domain),
    )
    .unwrap()
}

/// The original source basis is captured from the actual Mixer before rendering.
/// Its next-frame basis advances later; it cannot replace that creation pin.
fn recovered_output() -> (
    ConvertedNativeOutputState,
    OutputFrameBasis,
    TargetFrameBasis,
) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(8, 256, 1).unwrap()).unwrap();
    let program =
        PreparedPracticeProgram::new(&bank, vec![], PracticeLimits::new(8, 1, 64, 8, 16).unwrap())
            .unwrap();
    let (_controller, endpoint) = practice_queue(&program).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
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
            PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), false)
                .unwrap(),
        )
        .unwrap();
    let original_mixer = mixer.output_frame_basis();
    let output = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("valid original retained converter"));
    let original_target = output.target_frame_basis();
    (output, original_mixer, original_target)
}

fn seed(
    target: TargetFrameBasis,
    original_mixer: OutputFrameBasis,
) -> (NativeAudioPresentation, InputMerger) {
    let mut owner = cold_owner(target, true);
    for (frame, at) in [(0, 100), (1, 200)] {
        assert_eq!(
            owner
                .admit_target_coordinate(snapshot(7, target, frame, host(at)))
                .unwrap(),
            NativeObservationAdmission::Progress
        );
    }
    owner.pin_practice_mixer_basis(original_mixer).unwrap();
    let mut inputs = merger();
    let original = event(125, 1);
    inputs.admit(original.clone(), host(250)).unwrap();
    owner
        .authority_mut()
        .record_acquired_prefix(host(250))
        .unwrap();
    let mapped = owner
        .authority()
        .prepare_coordinate_input(host(125), host(250))
        .unwrap()
        .unwrap();
    assert_eq!(inputs.pop_ready(host(250)).unwrap(), Some(original));
    owner
        .authority_mut()
        .commit_coordinate_input(mapped, host(250))
        .unwrap();
    let frontier = owner
        .authority()
        .prepare_coordinate_frontier(host(250), &inputs)
        .unwrap()
        .unwrap();
    owner
        .authority_mut()
        .commit_coordinate_frontier(frontier, host(250), &mut inputs)
        .unwrap();
    (owner, inputs)
}

fn retarget(output: &mut ConvertedNativeOutputState, rate: u32) -> TargetFrameBasis {
    let report = output.render_pending(8).unwrap();
    assert_eq!(
        report.target_start_time,
        TargetTime::from_frames(0, 1500).unwrap()
    );
    output.admit(8).unwrap();
    output
        .reconfigure(
            DeviceFormat::new(rate, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            16,
        )
        .unwrap();
    let basis = output.target_frame_basis();
    assert_eq!(basis.sample_rate(), rate);
    assert_eq!(
        basis.start_time(),
        TargetTime::from_frames(8, 1500).unwrap()
    );
    basis
}

fn next_epoch(owner: &NativeAudioPresentation, basis: TargetFrameBasis) -> AudioAuthorityEpoch {
    let previous = owner.authority().epoch();
    let stream_origin = basis.point_at_stream_frame(0).unwrap();
    AudioAuthorityEpoch {
        id: previous.id + 1,
        stream_origin,
        logical_origin: previous.logical_output(stream_origin).unwrap(),
        host_domain: previous.host_domain,
    }
}

fn originals(epoch: u64, basis: TargetFrameBasis) -> [CoordinateTargetNativeAudioSnapshot; 2] {
    [
        snapshot(epoch, basis, 0, host(260)),
        snapshot(epoch, basis, 1, host(300)),
    ]
}

fn prepare(
    owner: &NativeAudioPresentation,
    basis: TargetFrameBasis,
    original_mixer: OutputFrameBasis,
    inputs: &InputMerger,
) -> PreparedCoordinateTargetNativeAudioEpoch {
    let next = next_epoch(owner, basis);
    owner
        .prepare_coordinate_target_output_epoch(
            next,
            basis,
            originals(next.id, basis),
            original_mixer,
            host(320),
            inputs,
        )
        .unwrap()
}

fn state(owner: &NativeAudioPresentation) -> String {
    format!("{owner:?}")
}

fn watermarks(owner: &NativeAudioPresentation) -> String {
    let authority = owner.authority();
    format!(
        "{:?}",
        (
            authority.acquired_prefix(),
            authority.closed_host_prefix(),
            authority.committed_input_host(),
            authority.committed_operation(),
            authority.committed_presentation(),
            authority.committed_coordinate_operation(),
            authority.committed_coordinate_presentation(),
        )
    )
}

#[test]
fn same_and_cross_rate_original_native_commit_preserves_source_pin_and_exact_frontiers() {
    for rate in [1500, 2000] {
        let (mut output, original_mixer, original_target) = recovered_output();
        let (mut owner, mut inputs) = seed(original_target, original_mixer);
        let basis = retarget(&mut output, rate);
        assert_eq!(
            output.mixer().config().format().sample_rate(),
            original_mixer.sample_rate()
        );
        assert_eq!(
            output.mixer().output_frame_basis().origin(),
            original_mixer.origin()
        );
        assert!(output.mixer().frame_cursor() > original_mixer.start_physical_frame());
        assert_ne!(
            basis.point_at_stream_frame(0).unwrap(),
            original_mixer.point_at_stream_frame(0).unwrap()
        );
        let before = state(&owner);
        let before_inputs = format!("{inputs:?}");
        let accepted = watermarks(&owner);
        let next = next_epoch(&owner, basis);
        let observations = originals(next.id, basis);
        let token = prepare(&owner, basis, original_mixer, &inputs);
        assert_eq!(token.epoch(), next.id);
        assert_eq!(token.basis(), basis);
        assert_eq!(token.snapshots(), observations);
        assert_eq!(token.practice_mixer_basis(), original_mixer);
        assert_eq!(token.authority_token().next_epoch(), next);
        assert_eq!(
            token.authority_token().next_basis(),
            AudioAuthorityCoordinateBasis::TargetFrame(basis)
        );
        assert_eq!(
            token.authority_token().observations(),
            observations.map(|s| {
                let OriginalNativePresentationEvidence::SuppliedPair(pair) = s.snapshot().evidence
                else {
                    panic!("original native pair retained");
                };
                (pair, s.coordinate())
            })
        );
        assert_eq!(
            token.latest_record().evidence(),
            &observations[1].snapshot().evidence
        );
        owner
            .validate_coordinate_target_output_epoch(&token, host(325), &inputs)
            .unwrap();
        assert_eq!(state(&owner), before);
        assert_eq!(format!("{inputs:?}"), before_inputs);
        owner
            .commit_coordinate_target_output_epoch(token.clone(), host(325), &inputs)
            .unwrap();
        assert_eq!(owner.authority().epoch(), next);
        assert_eq!(
            owner.authority().coordinate_basis(),
            Some(AudioAuthorityCoordinateBasis::TargetFrame(basis))
        );
        assert_eq!(owner.target_basis(), Some(basis));
        assert_eq!(owner.basis(), None);
        assert!(owner.has_retained_practice());
        assert_eq!(owner.practice_mixer_basis, Some(original_mixer));
        assert_eq!(owner.latest_record(), Some(token.latest_record()));
        assert_eq!(
            owner.validator.latest_frame_position(),
            Some((NativeFrameBasis::Target(basis), 1))
        );
        assert_eq!(owner.authority().history_len(), 2);
        assert_eq!(
            owner
                .authority()
                .latest_coordinate_observation()
                .unwrap()
                .coordinate(),
            Some(observations[1].coordinate())
        );
        assert_eq!(watermarks(&owner), accepted);
        assert_eq!(format!("{inputs:?}"), before_inputs);

        // Independently combine 8/1500 seconds and half a NEW target frame.
        let numerator = 16 * u128::from(rate) + 1500;
        let denominator = 3000 * u128::from(rate);
        let expected = (numerator * 1_000_000_000 / denominator) as i64;
        let original = event(280, 2);
        inputs.admit(original.clone(), host(325)).unwrap();
        owner
            .authority_mut()
            .record_acquired_prefix(host(300))
            .unwrap();
        let mapped = owner
            .authority()
            .prepare_coordinate_input(host(280), host(325))
            .unwrap()
            .unwrap();
        assert_eq!(mapped.original(), host(280));
        assert_eq!(mapped.raw_output(), point(2, 100 + expected));
        assert_eq!(mapped.output(), point(3, 500 + expected));
        assert!(
            mapped
                .query()
                .compare_target_time(
                    TargetTime::new(0, numerator as u64, denominator as u64).unwrap()
                )
                .is_eq()
        );
        assert_eq!(mapped.mapper().quality(), ClockMappingQuality::Unknown);
        assert_eq!(
            mapped.mapper().map(host(280), ClockDomainId(3)),
            Some(Timestamp::from_nanos(500 + expected))
        );
        assert_eq!(inputs.pop_ready(host(300)).unwrap(), Some(original));
        owner
            .authority_mut()
            .commit_coordinate_input(mapped, host(325))
            .unwrap();
        assert_eq!(owner.authority().committed_input_host(), Some(host(280)));
    }
}

#[test]
fn original_native_frames_progress_when_both_rounded_nanosecond_points_are_equal() {
    let (mut output, original_mixer, original_target) = recovered_output();
    let (mut owner, mut inputs) = seed(original_target, original_mixer);
    let retained_target = retarget(&mut output, 2000);
    // The original source owner and its accumulated duration are genuine. This
    // high-rate descriptor exercises native arithmetic without opening 4 GHz PCM.
    let basis = TargetFrameBasis::new(
        retained_target.origin(),
        retained_target.start_time(),
        4_000_000_000,
    )
    .unwrap();
    let token = prepare(&owner, basis, original_mixer, &inputs);
    let [first, second] = token.authority_token().observations();
    assert_eq!(first.0.source, second.0.source);
    assert_ne!(first.1, second.1);
    assert!(first.0.target.timestamp < second.0.target.timestamp);
    owner
        .commit_coordinate_target_output_epoch(token, host(320), &inputs)
        .unwrap();
    assert_eq!(
        owner.validator.latest_frame_position(),
        Some((NativeFrameBasis::Target(basis), 1))
    );
    assert_eq!(owner.authority().history_len(), 2);
    let original = event(280, 2);
    inputs.admit(original.clone(), host(320)).unwrap();
    owner
        .authority_mut()
        .record_acquired_prefix(host(320))
        .unwrap();
    let mapped = owner
        .authority()
        .prepare_coordinate_input(host(280), host(320))
        .unwrap()
        .unwrap();
    assert!(
        mapped
            .query()
            .compare_target_time(basis.start_time())
            .is_gt()
    );
    assert_eq!(mapped.original(), host(280));
    assert_eq!(mapped.raw_output(), first.0.source);
    assert_eq!(owner.practice_mixer_basis, Some(original_mixer));
    assert_eq!(inputs.pop_ready(host(320)).unwrap(), Some(original));
    owner
        .authority_mut()
        .commit_coordinate_input(mapped, host(320))
        .unwrap();
    assert_eq!(owner.authority().committed_input_host(), Some(host(280)));
}

#[test]
fn scalar_and_coordinate_native_epoch_tokens_keep_their_separate_admission_guards() {
    let (mut output, original_mixer, original_target) = recovered_output();
    let (owner, inputs) = seed(original_target, original_mixer);
    let basis = retarget(&mut output, 2000);
    let next = next_epoch(&owner, basis);
    let before = state(&owner);
    assert!(
        owner
            .prepare_target_output_epoch(
                next,
                basis,
                originals(next.id, basis).map(|s| s.snapshot()),
                host(320),
                &inputs
            )
            .is_err()
    );
    assert_eq!(state(&owner), before);

    let mut scalar = cold_owner(original_target, false);
    for (frame, at) in [(0, 100), (1, 200)] {
        scalar
            .admit_target(snapshot(7, original_target, frame, host(at)).snapshot())
            .unwrap();
    }
    scalar.pin_practice_mixer_basis(original_mixer).unwrap();
    let scalar_before = state(&scalar);
    assert!(
        scalar
            .prepare_coordinate_target_output_epoch(
                next_epoch(&scalar, basis),
                basis,
                originals(8, basis),
                original_mixer,
                host(320),
                &merger()
            )
            .is_err()
    );
    assert_eq!(state(&scalar), scalar_before);
}

#[test]
fn torn_wrong_epoch_basis_and_host_native_snapshots_refuse_without_state_changes() {
    let (mut output, original_mixer, original_target) = recovered_output();
    let (owner, inputs) = seed(original_target, original_mixer);
    let basis = retarget(&mut output, 2000);
    let next = next_epoch(&owner, basis);
    let different = TargetFrameBasis::new(basis.origin(), basis.start_time(), 2001).unwrap();
    let before = state(&owner);
    for second in [
        snapshot(7, basis, 1, host(300)),
        snapshot(8, different, 1, host(300)),
        snapshot(8, basis, 1, point(4, 300)),
    ] {
        assert!(
            owner
                .prepare_coordinate_target_output_epoch(
                    next,
                    basis,
                    [snapshot(8, basis, 0, host(260)), second],
                    original_mixer,
                    host(320),
                    &inputs
                )
                .is_err()
        );
        assert_eq!(state(&owner), before);
    }
    let mut wrong_pair = snapshot(8, basis, 1, host(300)).snapshot();
    wrong_pair.evidence = OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
        source: basis.point_at_stream_frame(2).unwrap(),
        target: host(300),
    });
    assert_eq!(
        CoordinateTargetNativeAudioSnapshot::new(
            wrong_pair,
            NativeOutputCoordinate::StreamFrame { frame: 1 }
        ),
        Err(NativeOutputCoordinateError::CoordinateMismatch)
    );
    assert_eq!(state(&owner), before);
    let unsupported = TargetNativeAudioSnapshot {
        epoch: 8,
        basis,
        evidence: OriginalNativePresentationEvidence::Wasapi {
            snapshot: AudioStreamSnapshot {
                telemetry_available: true,
                status: AudioStreamStatus::Running,
                counters: StreamCounters::default(),
                render: None,
                clock: None,
            },
            basis: None,
        },
    };
    assert_eq!(
        CoordinateTargetNativeAudioSnapshot::new(
            unsupported,
            NativeOutputCoordinate::StreamFrame { frame: 1 }
        ),
        Err(NativeOutputCoordinateError::UnsupportedEvidence)
    );
    assert_eq!(state(&owner), before);
}

#[test]
fn original_pair_duplicate_regression_future_and_expired_anchors_cannot_prime() {
    let (mut output, original_mixer, original_target) = recovered_output();
    let (owner, inputs) = seed(original_target, original_mixer);
    let basis = retarget(&mut output, 2000);
    let next = next_epoch(&owner, basis);
    let before = state(&owner);
    for (first_frame, first_host, second_frame, second_host, now) in [
        (0, 260, 0, 300, 320),
        (1, 260, 0, 300, 320),
        (0, 300, 1, 300, 320),
        (0, 300, 1, 260, 320),
        (0, 260, 1, 321, 320),
        (0, 260, 1, 1300, 1301),
        (0, 100, 1, 200, 320),
    ] {
        assert!(
            owner
                .prepare_coordinate_target_output_epoch(
                    next,
                    basis,
                    [
                        snapshot(8, basis, first_frame, host(first_host)),
                        snapshot(8, basis, second_frame, host(second_host))
                    ],
                    original_mixer,
                    host(now),
                    &inputs
                )
                .is_err()
        );
        assert_eq!(state(&owner), before);
    }
}

#[test]
fn creation_epoch_projection_origin_and_original_pin_are_not_replaced_by_new_frame_zero() {
    let (mut output, original_mixer, original_target) = recovered_output();
    let (mut owner, inputs) = seed(original_target, original_mixer);
    let basis = retarget(&mut output, 1500);
    let next = next_epoch(&owner, basis);
    let before = state(&owner);
    for invalid in [
        AudioAuthorityEpoch { id: 7, ..next },
        AudioAuthorityEpoch {
            stream_origin: point(2, next.stream_origin.timestamp.as_nanos() + 1),
            ..next
        },
        AudioAuthorityEpoch {
            logical_origin: point(3, next.logical_origin.timestamp.as_nanos() + 1),
            ..next
        },
        AudioAuthorityEpoch {
            host_domain: ClockDomainId(4),
            ..next
        },
    ] {
        assert!(
            owner
                .prepare_coordinate_target_output_epoch(
                    invalid,
                    basis,
                    originals(invalid.id, basis),
                    original_mixer,
                    host(320),
                    &inputs
                )
                .is_err()
        );
        assert_eq!(state(&owner), before);
    }
    let changed_origin =
        TargetFrameBasis::new(point(2, 101), basis.start_time(), basis.sample_rate()).unwrap();
    assert!(
        owner
            .prepare_coordinate_target_output_epoch(
                next_epoch(&owner, changed_origin),
                changed_origin,
                originals(8, changed_origin),
                original_mixer,
                host(320),
                &inputs
            )
            .is_err()
    );
    assert_eq!(state(&owner), before);
    let false_creation = OutputFrameBasis::new(basis.origin(), 1500, 8).unwrap();
    assert_eq!(
        false_creation.point_at_stream_frame(0).unwrap(),
        basis.point_at_stream_frame(0).unwrap()
    );
    for wrong_pin in [false_creation, output.mixer().output_frame_basis()] {
        assert_ne!(wrong_pin, original_mixer);
        assert!(
            owner
                .prepare_coordinate_target_output_epoch(
                    next,
                    basis,
                    originals(8, basis),
                    wrong_pin,
                    host(320),
                    &inputs
                )
                .is_err()
        );
        assert_eq!(state(&owner), before);
    }
    owner.practice_mixer_basis = None;
    let unpinned = state(&owner);
    assert!(
        owner
            .prepare_coordinate_target_output_epoch(
                next,
                basis,
                originals(8, basis),
                original_mixer,
                host(320),
                &inputs
            )
            .is_err()
    );
    assert_eq!(state(&owner), unpinned);
}

#[test]
fn changed_authority_native_record_basis_pin_or_native_epoch_invalidate_publication_atomically() {
    for mutation in 0..5 {
        let (mut output, original_mixer, original_target) = recovered_output();
        let (mut owner, inputs) = seed(original_target, original_mixer);
        let basis = retarget(&mut output, 2000);
        let token = prepare(&owner, basis, original_mixer, &inputs);
        match mutation {
            0 => owner
                .authority_mut()
                .record_acquired_prefix(host(251))
                .unwrap(),
            1 => {
                let native = owner
                    .validator
                    .prepare_frame_pair(
                        7,
                        ClockPair {
                            source: original_target.point_at_stream_frame(2).unwrap(),
                            target: host(240),
                        },
                        NativeFrameBasis::Target(original_target),
                        2,
                    )
                    .unwrap();
                owner.validator.commit(native).unwrap();
            }
            2 => {
                owner.target_basis = Some(
                    TargetFrameBasis::new(
                        original_target.origin(),
                        original_target.start_time(),
                        1501,
                    )
                    .unwrap(),
                )
            }
            3 => owner.practice_mixer_basis = None,
            4 => {
                let native = owner
                    .validator
                    .prepare_rebind(8, owner.authority().epoch().stream_origin)
                    .unwrap();
                owner.validator.commit_rebind(native).unwrap();
            }
            _ => unreachable!(),
        }
        let before = state(&owner);
        assert!(
            owner
                .validate_coordinate_target_output_epoch(&token, host(325), &inputs)
                .is_err()
        );
        assert!(
            owner
                .commit_coordinate_target_output_epoch(token, host(325), &inputs)
                .is_err()
        );
        assert_eq!(state(&owner), before);
    }
}

#[test]
fn equal_record_with_changed_original_native_frame_metadata_is_still_stale() {
    let (_output, original_mixer, creation_target) = recovered_output();
    // Only the native descriptor uses 4 GHz. Its frame-zero point shares the
    // actual retained source creation origin; no converter/device is reconfigured.
    let original_target = TargetFrameBasis::new(
        creation_target.origin(),
        creation_target.start_time(),
        4_000_000_000,
    )
    .unwrap();
    let (mut owner, inputs) = seed(original_target, original_mixer);
    let basis = TargetFrameBasis::new(
        original_target.origin(),
        TargetTime::from_frames(8, 4_000_000_000).unwrap(),
        original_target.sample_rate(),
    )
    .unwrap();
    let token = prepare(&owner, basis, original_mixer, &inputs);
    let original_record = owner.latest_record().unwrap();
    let mut different_native = NativePresentationValidator::new(
        7,
        original_target.point_at_stream_frame(0).unwrap(),
        ClockDomainId(1),
    );
    let changed = different_native
        .prepare_frame_pair(
            7,
            original_record.pair(),
            NativeFrameBasis::Target(original_target),
            2,
        )
        .unwrap();
    different_native.commit(changed).unwrap();
    assert_eq!(different_native.latest_record(), owner.latest_record());
    assert_ne!(
        different_native.latest_frame_position(),
        owner.validator.latest_frame_position()
    );
    // Fault injection changes only the native exact-frame sidecar: equal rounded
    // record/epoch/origin/basis cannot certify the original authority's frame1.
    owner.validator = different_native;
    let before = state(&owner);
    assert!(
        owner
            .validate_coordinate_target_output_epoch(&token, host(325), &inputs)
            .is_err()
    );
    assert!(
        owner
            .commit_coordinate_target_output_epoch(token, host(325), &inputs)
            .is_err()
    );
    assert_eq!(state(&owner), before);
}

#[test]
fn changed_merger_pending_and_empty_after_admit_pop_both_refuse_the_original_token() {
    for change in 0..3 {
        let (mut output, original_mixer, original_target) = recovered_output();
        let (mut owner, mut inputs) = seed(original_target, original_mixer);
        let basis = retarget(&mut output, 2000);
        let token = prepare(&owner, basis, original_mixer, &inputs);
        match change {
            0 | 1 => {
                let original = event(280, 2);
                inputs.admit(original.clone(), host(320)).unwrap();
                if change == 1 {
                    assert_eq!(inputs.pop_ready(host(320)).unwrap(), Some(original));
                    assert_eq!(inputs.pending(), 0);
                }
            }
            2 => inputs.commit(host(251)).unwrap(),
            _ => unreachable!(),
        }
        let before = state(&owner);
        let before_inputs = format!("{inputs:?}");
        assert!(
            owner
                .validate_coordinate_target_output_epoch(&token, host(325), &inputs)
                .is_err()
        );
        assert!(
            owner
                .commit_coordinate_target_output_epoch(token, host(325), &inputs)
                .is_err()
        );
        assert_eq!(state(&owner), before);
        assert_eq!(format!("{inputs:?}"), before_inputs);
    }
}

#[test]
fn prepared_clock_and_once_published_native_epoch_are_rechecked_without_partial_commit() {
    let (mut output, original_mixer, original_target) = recovered_output();
    let (mut owner, inputs) = seed(original_target, original_mixer);
    let basis = retarget(&mut output, 2000);
    let token = prepare(&owner, basis, original_mixer, &inputs);
    let before = state(&owner);
    for now in [host(319), host(1301), point(4, 325)] {
        assert!(
            owner
                .validate_coordinate_target_output_epoch(&token, now, &inputs)
                .is_err()
        );
        assert!(
            owner
                .commit_coordinate_target_output_epoch(token.clone(), now, &inputs)
                .is_err()
        );
        assert_eq!(state(&owner), before);
    }
    owner
        .commit_coordinate_target_output_epoch(token.clone(), host(325), &inputs)
        .unwrap();
    let committed = state(&owner);
    assert!(
        owner
            .commit_coordinate_target_output_epoch(token, host(325), &inputs)
            .is_err()
    );
    assert_eq!(state(&owner), committed);
}
