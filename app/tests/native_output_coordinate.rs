//! Coordinate descriptors preserve original evidence without claiming native admission.
use beatkernel::{
    audio::{AudioCounters, OutputFrameBasis, RenderReport, TargetFrameBasis, TargetTime},
    time::{ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Timestamp},
};
use beatkernel_bms_runtime::{
    native_audio_presentation::{NativeAudioSnapshot, TargetNativeAudioSnapshot},
    native_output_coordinate::{
        CoordinateNativeAudioSnapshot, CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate,
        NativeOutputCoordinateError,
    },
};
use beatkernel_platform::audio::{
    asio::{AsioPresentationObservation, MultimediaHostInterval},
    presentation::validation::OriginalNativePresentationEvidence,
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn grid() -> OutputFrameBasis {
    OutputFrameBasis::new(point(2, -900), 44_100, 2).unwrap()
}
fn counter(position: u64, frequency: u64) -> NativeOutputCoordinate {
    NativeOutputCoordinate::Counter {
        position,
        frequency,
    }
}
fn frame(frame: u64) -> NativeOutputCoordinate {
    NativeOutputCoordinate::StreamFrame { frame }
}
fn wasapi(position: u64, frequency: u64) -> NativeAudioSnapshot {
    NativeAudioSnapshot {
        epoch: 19,
        basis: grid(),
        evidence: OriginalNativePresentationEvidence::Wasapi {
            snapshot: AudioStreamSnapshot {
                telemetry_available: true,
                status: AudioStreamStatus::Running,
                counters: StreamCounters::default(),
                render: None,
                clock: Some(AudioClockSnapshot {
                    position,
                    frequency,
                    qpc_100ns: 71,
                    reading_quality: AudioClockReadingQuality::Accurate,
                    host_point: Some(point(1, 7117)),
                    mapping_quality: ClockMappingQuality::Unknown,
                }),
            },
            basis: Some(grid()),
        },
    }
}
fn supplied(basis: OutputFrameBasis, relative_frame: u64) -> NativeAudioSnapshot {
    NativeAudioSnapshot {
        epoch: 27,
        basis,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: basis.point_at_stream_frame(relative_frame).unwrap(),
            target: point(1, 100),
        }),
    }
}
fn asio() -> NativeAudioSnapshot {
    let basis = OutputFrameBasis::new(point(2, -900), 44_100, 10).unwrap();
    let render = RenderReport {
        start_frame: 12,
        frames: 8,
        playback_start_frame: 5,
        playback_frames: 8,
        paused: false,
        playback_end_physical_frame: None,
        active_voices: 0,
        pending_commands: 0,
        song_position: Timestamp::ZERO,
        producer_disconnected: false,
        counters: AudioCounters::default(),
    };
    let observation = AsioPresentationObservation::from_render(
        render,
        44_100,
        MultimediaHostInterval {
            before: point(1, 10_000),
            after: point(1, 10_017),
        },
        0,
        0,
        basis.origin(),
    )
    .unwrap();
    NativeAudioSnapshot {
        epoch: 31,
        basis,
        evidence: OriginalNativePresentationEvidence::Asio {
            observation,
            basis: Some(basis),
        },
    }
}
fn target(relative_frame: u64) -> TargetNativeAudioSnapshot {
    let basis = TargetFrameBasis::new(
        point(2, -900),
        TargetTime::from_frames(2, 44_100).unwrap(),
        48_000,
    )
    .unwrap();
    TargetNativeAudioSnapshot {
        epoch: 37,
        basis,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: basis.point_at_stream_frame(relative_frame).unwrap(),
            target: point(1, 100),
        }),
    }
}

#[test]
fn wasapi_preserves_original_counter_frequency_basis_epoch_and_complete_evidence() {
    let original = wasapi(17, 48_000);
    let descriptor = CoordinateNativeAudioSnapshot::new(original, counter(17, 48_000)).unwrap();
    assert_eq!(descriptor.snapshot(), original);
    assert_eq!(descriptor.coordinate(), counter(17, 48_000));
    assert_eq!(
        original.basis.point_at_native_counter(17, 48_000).unwrap(),
        point(2, 398_618)
    );
}

#[test]
fn descriptor_preserves_caller_epoch_without_authenticating_its_freshness() {
    let mut original = wasapi(17, 48_000);
    original.epoch = 0;
    let descriptor = CoordinateNativeAudioSnapshot::new(original, counter(17, 48_000)).unwrap();
    assert_eq!(descriptor.snapshot().epoch, 0);
}

#[test]
fn wasapi_rejects_counter_position_that_disagrees_with_original_metadata() {
    assert!(CoordinateNativeAudioSnapshot::new(wasapi(17, 48_000), counter(18, 48_000)).is_err());
}

#[test]
fn wasapi_rejects_frequency_that_disagrees_with_original_metadata() {
    assert!(CoordinateNativeAudioSnapshot::new(wasapi(17, 48_000), counter(17, 44_100)).is_err());
}

#[test]
fn zero_native_frequency_is_refused() {
    assert_eq!(
        CoordinateNativeAudioSnapshot::new(wasapi(17, 0), counter(17, 0)).unwrap_err(),
        NativeOutputCoordinateError::InvalidFrequency
    );
}

#[test]
fn wasapi_cannot_be_relabeled_as_an_integer_stream_frame() {
    assert!(CoordinateNativeAudioSnapshot::new(wasapi(17, 48_000), frame(17)).is_err());
}

#[test]
fn wasapi_rejects_missing_original_clock() {
    let mut original = wasapi(17, 48_000);
    if let OriginalNativePresentationEvidence::Wasapi {
        ref mut snapshot, ..
    } = original.evidence
    {
        snapshot.clock = None;
    }
    assert!(CoordinateNativeAudioSnapshot::new(original, counter(17, 48_000)).is_err());
}

#[test]
fn native_counter_requires_available_running_accurate_original_telemetry() {
    for invalid in 0..3 {
        let mut original = wasapi(17, 48_000);
        if let OriginalNativePresentationEvidence::Wasapi {
            ref mut snapshot, ..
        } = original.evidence
        {
            match invalid {
                0 => snapshot.telemetry_available = false,
                1 => snapshot.status = AudioStreamStatus::Stopped,
                _ => {
                    snapshot.clock.as_mut().unwrap().reading_quality =
                        AudioClockReadingQuality::Degraded
                }
            }
        }
        assert!(CoordinateNativeAudioSnapshot::new(original, counter(17, 48_000)).is_err());
    }
}

#[test]
fn native_evidence_must_retain_its_original_inner_basis() {
    for mut original in [wasapi(17, 48_000), asio()] {
        let coordinate = match original.evidence {
            OriginalNativePresentationEvidence::Wasapi { ref mut basis, .. } => {
                *basis = None;
                counter(17, 48_000)
            }
            OriginalNativePresentationEvidence::Asio { ref mut basis, .. } => {
                *basis = None;
                frame(2)
            }
            _ => unreachable!(),
        };
        assert!(CoordinateNativeAudioSnapshot::new(original, coordinate).is_err());
    }
}

#[test]
fn wasapi_rejects_contradictory_inner_frame_basis() {
    let mut original = wasapi(17, 48_000);
    if let OriginalNativePresentationEvidence::Wasapi { ref mut basis, .. } = original.evidence {
        *basis = Some(OutputFrameBasis::new(grid().origin(), 44_100, 3).unwrap());
    }
    assert!(CoordinateNativeAudioSnapshot::new(original, counter(17, 48_000)).is_err());
}

#[test]
fn unrepresentable_native_counter_projection_is_refused() {
    assert!(CoordinateNativeAudioSnapshot::new(wasapi(u64::MAX, 1), counter(u64::MAX, 1)).is_err());
}

#[test]
fn asio_uses_original_physical_render_offset_and_preserves_full_host_bracket() {
    let original = asio();
    let descriptor = CoordinateNativeAudioSnapshot::new(original, frame(2)).unwrap();
    assert_eq!(descriptor.snapshot(), original);
    assert_eq!(descriptor.coordinate(), frame(2));
    if let OriginalNativePresentationEvidence::Asio { observation, .. } =
        descriptor.snapshot().evidence
    {
        assert_eq!(observation.host.before, point(1, 10_000));
        assert_eq!(observation.host.after, point(1, 10_017));
        assert_eq!(observation.render.playback_start_frame, 5);
    } else {
        panic!("original ASIO evidence was replaced");
    }
}

#[test]
fn asio_cannot_be_relabeled_as_a_native_hardware_counter() {
    assert!(CoordinateNativeAudioSnapshot::new(asio(), counter(2, 44_100)).is_err());
}

#[test]
fn asio_rejects_wrong_stream_frame_including_playback_frame_substitution() {
    assert!(CoordinateNativeAudioSnapshot::new(asio(), frame(5)).is_err());
}

#[test]
fn asio_rejects_render_before_stream_creation_basis() {
    let mut original = asio();
    original.basis = OutputFrameBasis::new(original.basis.origin(), 44_100, 13).unwrap();
    if let OriginalNativePresentationEvidence::Asio { ref mut basis, .. } = original.evidence {
        *basis = Some(original.basis);
    }
    assert!(CoordinateNativeAudioSnapshot::new(original, frame(0)).is_err());
}

#[test]
fn asio_rejects_original_rate_that_contradicts_creation_basis() {
    let mut original = asio();
    if let OriginalNativePresentationEvidence::Asio {
        ref mut observation,
        ..
    } = original.evidence
    {
        observation.sample_rate = 48_000;
    }
    assert!(CoordinateNativeAudioSnapshot::new(original, frame(2)).is_err());
}

#[test]
fn asio_rejects_original_origin_that_contradicts_creation_basis() {
    let mut original = asio();
    if let OriginalNativePresentationEvidence::Asio {
        ref mut observation,
        ..
    } = original.evidence
    {
        observation.output_origin = point(2, -899);
    }
    assert!(CoordinateNativeAudioSnapshot::new(original, frame(2)).is_err());
}

#[test]
fn asio_rejects_inverted_original_host_bracket() {
    let mut original = asio();
    if let OriginalNativePresentationEvidence::Asio {
        ref mut observation,
        ..
    } = original.evidence
    {
        observation.host.after = point(1, 9_999);
    }
    assert!(CoordinateNativeAudioSnapshot::new(original, frame(2)).is_err());
}

#[test]
fn asio_rejects_split_host_domains_and_contradictory_block_projection() {
    for invalid in 0..2 {
        let mut original = asio();
        if let OriginalNativePresentationEvidence::Asio {
            ref mut observation,
            ..
        } = original.evidence
        {
            if invalid == 0 {
                observation.host.after.domain = ClockDomainId(99);
            } else {
                observation.output = point(2, -899);
            }
        }
        assert!(CoordinateNativeAudioSnapshot::new(original, frame(2)).is_err());
    }
}

#[test]
fn supplied_frame_preserves_nonzero_44100_basis_before_single_floor() {
    let original = supplied(grid(), 1);
    let descriptor = CoordinateNativeAudioSnapshot::new(original, frame(1)).unwrap();
    assert_eq!(descriptor.snapshot(), original);
    assert_eq!(descriptor.coordinate(), frame(1));
    if let OriginalNativePresentationEvidence::SuppliedPair(pair) = original.evidence {
        assert_eq!(pair.source, point(2, 67_127));
    } else {
        unreachable!();
    }
}

#[test]
fn supplied_pair_projection_checks_do_not_authenticate_host_acquisition() {
    let mut original = supplied(grid(), 1);
    if let OriginalNativePresentationEvidence::SuppliedPair(ref mut pair) = original.evidence {
        pair.target = point(19, -123);
    }
    assert_eq!(
        CoordinateNativeAudioSnapshot::new(original, frame(1))
            .unwrap()
            .snapshot(),
        original
    );
}

#[test]
fn supplied_pair_cannot_be_inverted_into_fabricated_counter_metadata() {
    assert!(CoordinateNativeAudioSnapshot::new(supplied(grid(), 1), counter(1, 44_100)).is_err());
}

#[test]
fn supplied_pair_refuses_timestamp_or_domain_projection_contradiction() {
    for bad in [point(2, 67_128), point(99, 67_127)] {
        let mut original = supplied(grid(), 1);
        if let OriginalNativePresentationEvidence::SuppliedPair(ref mut pair) = original.evidence {
            pair.source = bad;
        }
        assert!(CoordinateNativeAudioSnapshot::new(original, frame(1)).is_err());
    }
}

#[test]
fn supplied_stream_frame_addition_overflow_is_refused_before_projection() {
    assert!(CoordinateNativeAudioSnapshot::new(supplied(grid(), 1), frame(u64::MAX)).is_err());
}

#[test]
fn converted_target_preserves_exact_fractional_start_and_original_snapshot() {
    let original = target(3);
    let descriptor = CoordinateTargetNativeAudioSnapshot::new(original, frame(3)).unwrap();
    assert_eq!(descriptor.snapshot(), original);
    assert_eq!(descriptor.coordinate(), frame(3));
    assert_eq!(
        descriptor.snapshot().basis.start_time(),
        TargetTime::new(0, 1, 22_050).unwrap()
    );
    assert_eq!(
        descriptor
            .snapshot()
            .basis
            .point_at_stream_frame(3)
            .unwrap(),
        point(2, 106_951)
    );
}

#[test]
fn converted_target_refuses_wrong_frame_and_fabricated_counter() {
    let original = target(3);
    assert!(CoordinateTargetNativeAudioSnapshot::new(original, frame(2)).is_err());
    assert!(CoordinateTargetNativeAudioSnapshot::new(original, counter(3, 48_000)).is_err());
}

#[test]
fn converted_target_refuses_unrepresentable_coordinate() {
    assert!(CoordinateTargetNativeAudioSnapshot::new(target(3), frame(u64::MAX)).is_err());
}

#[test]
fn legacy_public_snapshot_literals_remain_independently_usable() {
    let direct = supplied(grid(), 1);
    let converted = target(3);
    let direct_copy = NativeAudioSnapshot {
        epoch: direct.epoch,
        basis: direct.basis,
        evidence: direct.evidence,
    };
    let target_copy = TargetNativeAudioSnapshot {
        epoch: converted.epoch,
        basis: converted.basis,
        evidence: converted.evidence,
    };
    assert_eq!(direct_copy, direct);
    assert_eq!(target_copy, converted);
}
