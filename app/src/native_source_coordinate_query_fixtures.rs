//! Exact mathematical source queries from original DI coordinates; no native acquisition.
use super::*;
use beatkernel::audio::{InterpolatedTargetTime, OutputFrameBasis, TargetTime};
use beatkernel::time::{ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Timestamp};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
};
use std::cmp::Ordering;

fn basis() -> OutputFrameBasis {
    OutputFrameBasis::new(
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(-123),
        },
        3,
        8,
    )
    .unwrap()
}
fn frame(value: u64) -> CoordinateNativeAudioSnapshot {
    let grid = basis();
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 19,
            basis: grid,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: grid.point_at_stream_frame(value).unwrap(),
                target: ClockPoint {
                    domain: ClockDomainId(8),
                    timestamp: Timestamp::from_nanos(1100),
                },
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame: value },
    )
    .unwrap()
}
fn counter(position: u64, frequency: u64) -> CoordinateNativeAudioSnapshot {
    let grid = basis();
    let raw = AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position,
            frequency,
            qpc_100ns: 11,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(ClockPoint {
                domain: ClockDomainId(8),
                timestamp: Timestamp::from_nanos(1100),
            }),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    };
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 19,
            basis: grid,
            evidence: OriginalNativePresentationEvidence::Wasapi {
                snapshot: raw,
                basis: Some(grid),
            },
        },
        NativeOutputCoordinate::Counter {
            position,
            frequency,
        },
    )
    .unwrap()
}

#[test]
fn stream_source_query_preserves_absolute_nonzero_creation_frame_and_original_snapshot() {
    for relative in [0, 1, 7, u32::MAX as u64] {
        let native = frame(relative);
        let before = native;
        assert_eq!(
            native
                .source_query()
                .unwrap()
                .compare_source_time_frame(8 + relative, 3)
                .unwrap(),
            Ordering::Equal
        );
        assert_eq!(native, before);
        assert_eq!(native.snapshot().basis, basis());
    }
}

#[test]
fn counter_source_query_uses_native_u64_frequency_without_narrowing_or_source_hz_substitution() {
    for (position, frequency) in [(1, u64::MAX), (4_294_967_297, 4_294_967_299), (5, 7)] {
        let native = counter(position, frequency);
        let expected = InterpolatedTargetTime::new(
            TargetTime::from_frames(8, 3).unwrap(),
            position,
            position,
            frequency,
            0,
            1,
        )
        .unwrap();
        let actual = native.source_query().unwrap();
        assert_eq!(actual.compare_interpolated_time(expected), Ordering::Equal);
        assert_eq!(
            native.coordinate(),
            NativeOutputCoordinate::Counter {
                position,
                frequency
            }
        );
        let wrong = InterpolatedTargetTime::new(
            TargetTime::from_frames(8, 3).unwrap(),
            position,
            position,
            3,
            0,
            1,
        )
        .unwrap();
        assert_ne!(actual.compare_interpolated_time(wrong), Ordering::Equal);
    }
}

#[test]
fn counter_fraction_survives_even_when_display_nanoseconds_equal_creation_time() {
    let native = counter(1, u64::MAX);
    let actual = native.source_query().unwrap();
    let cold = TargetTime::from_frames(8, 3).unwrap();
    assert_eq!(
        actual.point(basis().origin()).unwrap(),
        cold.point(basis().origin()).unwrap()
    );
    assert_eq!(actual.compare_target_time(cold), Ordering::Greater);
    assert_eq!(native, counter(1, u64::MAX));
}
