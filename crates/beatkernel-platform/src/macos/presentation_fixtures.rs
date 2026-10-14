//! Pure production presentation qualification with the cold callback maximum.
//! These macOS fixtures are compile-only on the Linux host, not HAL evidence.
use super::*;
use crate::macos::audio::CoreAudioRequest;
use beatkernel::{
    audio::AudioFormat,
    time::{ClockMappingQuality, Duration},
};

struct Mapping;
impl ClockMapper for Mapping {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        if from.domain != ClockDomainId(1) || to != ClockDomainId(3) {
            return None;
        }
        from.timestamp.checked_add(Duration::from_nanos(10))
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}

fn applied() -> CoreAudioApplied {
    let format = AudioFormat::new(48_000, 2).unwrap();
    CoreAudioApplied {
        request: CoreAudioRequest {
            device: 42,
            format,
            buffer_frames: 64,
        },
        format,
        buffer_frames: 64,
        buffer_channels: vec![1, 1],
        output_domain: ClockDomainId(2),
        output_origin: Timestamp::from_nanos(-10),
        native_clock: ClockDomainId(1),
    }
}

fn observation(frames: u32) -> CoreAudioPresentation {
    CoreAudioPresentation {
        host_ticks: 987,
        native: Some(point(1, 2_000_000_000)),
        native_sample_frame: Some(123.5),
        output_grid: Some(point(2, 1_999_990)),
        flags: 3,
        first_frame: 96,
        frames,
    }
}

#[test]
fn cold_maximum_accepts_actual_variable_frames_without_rewriting_nominal_or_original_clock_pair() {
    let applied = applied();
    let expected = ClockPair {
        source: point(2, 1_999_990),
        target: point(3, 2_000_000_010),
    };
    for frames in [1, 64, 65, 127, 128] {
        assert_eq!(
            coreaudio_presentation_pair_with_max_frames(
                observation(frames),
                &applied,
                128,
                ClockDomainId(3),
                &Mapping,
            ),
            Ok(Some(expected))
        );
    }
    assert_eq!(applied.buffer_frames, 64);
    assert_eq!(applied.request.buffer_frames, 64);
    assert_eq!(applied.buffer_channels, [1, 1]);
}

#[test]
fn actual_zero_and_above_cold_maximum_are_invalid_observations() {
    for frames in [0, 129, u32::MAX] {
        assert!(matches!(
            coreaudio_presentation_pair_with_max_frames(
                observation(frames),
                &applied(),
                128,
                ClockDomainId(3),
                &Mapping,
            ),
            Err(CoreAudioPresentationError::InvalidObservation(_))
        ));
    }
}

#[test]
fn zero_and_below_nominal_maximum_are_invalid_configurations() {
    for maximum in [0, 1, 63] {
        assert!(matches!(
            coreaudio_presentation_pair_with_max_frames(
                observation(128),
                &applied(),
                maximum,
                ClockDomainId(3),
                &Mapping,
            ),
            Err(CoreAudioPresentationError::InvalidConfiguration(_))
        ));
    }
    assert_eq!(
        coreaudio_presentation_pair_with_max_frames(
            observation(64),
            &applied(),
            64,
            ClockDomainId(3),
            &Mapping,
        ),
        coreaudio_presentation_pair(observation(64), &applied(), ClockDomainId(3), &Mapping)
    );
}

#[test]
fn legacy_helper_keeps_nominal_sixty_four_boundary_and_rejects_sixty_five() {
    assert!(
        coreaudio_presentation_pair(observation(64), &applied(), ClockDomainId(3), &Mapping)
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        coreaudio_presentation_pair(observation(65), &applied(), ClockDomainId(3), &Mapping),
        Err(CoreAudioPresentationError::InvalidObservation(_))
    ));
}

#[test]
fn larger_maximum_never_forgives_contradictory_exact_request_applied_or_native_layout() {
    let mut invalid = Vec::new();
    let mut value = applied();
    value.request.device = 0;
    invalid.push(value);
    let mut value = applied();
    value.request.format = AudioFormat::new(44_100, 2).unwrap();
    invalid.push(value);
    let mut value = applied();
    value.request.buffer_frames = 63;
    invalid.push(value);
    let mut value = applied();
    value.buffer_frames = 0;
    invalid.push(value);
    let mut value = applied();
    value.buffer_channels.clear();
    invalid.push(value);
    let mut value = applied();
    value.buffer_channels = vec![1; 33];
    invalid.push(value);
    let mut value = applied();
    value.buffer_channels = vec![0, 2];
    invalid.push(value);
    let mut value = applied();
    value.buffer_channels = vec![2, 1];
    invalid.push(value);
    for value in invalid {
        assert!(matches!(
            coreaudio_presentation_pair_with_max_frames(
                observation(128),
                &value,
                128,
                ClockDomainId(3),
                &Mapping,
            ),
            Err(CoreAudioPresentationError::InvalidConfiguration(_))
        ));
    }
}

#[test]
fn larger_maximum_preserves_flags_finite_grid_domain_mapping_and_missing_association_rules() {
    let mut invalid = Vec::new();
    for flags in [1, 2] {
        let mut value = observation(128);
        value.flags = flags;
        invalid.push(value);
    }
    let mut value = observation(128);
    value.native_sample_frame = None;
    invalid.push(value);
    for sample in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut value = observation(128);
        value.native_sample_frame = Some(sample);
        invalid.push(value);
    }
    let mut value = observation(128);
    value.output_grid = Some(point(2, 1_999_991));
    invalid.push(value);
    for value in invalid {
        assert!(matches!(
            coreaudio_presentation_pair_with_max_frames(
                value,
                &applied(),
                128,
                ClockDomainId(3),
                &Mapping,
            ),
            Err(CoreAudioPresentationError::InvalidObservation(_))
        ));
    }
    for native in [false, true] {
        let mut value = observation(128);
        if native {
            value.native.as_mut().unwrap().domain = ClockDomainId(9);
        } else {
            value.output_grid.as_mut().unwrap().domain = ClockDomainId(9);
        }
        assert_eq!(
            coreaudio_presentation_pair_with_max_frames(
                value,
                &applied(),
                128,
                ClockDomainId(3),
                &Mapping,
            ),
            Err(CoreAudioPresentationError::DomainMismatch {
                expected: ClockDomainId(if native { 1 } else { 2 }),
                received: ClockDomainId(9),
            })
        );
    }
    assert_eq!(
        coreaudio_presentation_pair_with_max_frames(
            observation(128),
            &applied(),
            128,
            ClockDomainId(9),
            &Mapping,
        ),
        Err(CoreAudioPresentationError::UnmappedHost {
            from: ClockDomainId(1),
            to: ClockDomainId(9),
        })
    );
    let mut value = observation(128);
    value.first_frame = u64::MAX;
    assert_eq!(
        coreaudio_presentation_pair_with_max_frames(
            value,
            &applied(),
            128,
            ClockDomainId(3),
            &Mapping,
        ),
        Err(CoreAudioPresentationError::Overflow)
    );
    for native in [false, true] {
        let mut value = observation(128);
        if native {
            value.native = None;
        } else {
            value.output_grid = None;
        }
        assert_eq!(
            coreaudio_presentation_pair_with_max_frames(
                value,
                &applied(),
                128,
                ClockDomainId(3),
                &Mapping,
            ),
            Ok(None)
        );
    }
}
