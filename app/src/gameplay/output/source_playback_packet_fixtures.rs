//! Checked mathematical DI packets do not authenticate native delivery.
use super::*;
use crate::native_audio_presentation::NativeAudioSnapshot;
use crate::native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Timestamp},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters, presentation::validation::OriginalNativePresentationEvidence,
};

fn basis(hz: u32, start: u64) -> OutputFrameBasis {
    OutputFrameBasis::new(
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(-123),
        },
        hz,
        start,
    )
    .unwrap()
}
fn at(frame: u64, hz: u32) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, hz).unwrap(), 0, 0, 1, 0, 1).unwrap()
}
fn facts() -> SourcePlaybackFacts {
    SourcePlaybackFacts {
        playback: SourcePlaybackSpan::new(0, None, 0, SourcePlaybackState::Running).unwrap(),
        pulled_source_end: 8,
        submitted_source_end: 6,
        render: None,
        revision: 2,
    }
}
fn packet<E>(
    query: InterpolatedTargetTime,
    facts: SourcePlaybackFacts,
    evidence: Option<E>,
) -> Result<NativeSourcePlaybackQuery<E>, SourcePlaybackPacketError> {
    NativeSourcePlaybackQuery::new(
        19,
        basis(3, 2),
        SourcePlaybackIdentity::new(3, 0, 0).unwrap(),
        query,
        facts,
        evidence,
    )
}
fn coordinate(epoch: u64, grid: OutputFrameBasis, frame: u64) -> CoordinateNativeAudioSnapshot {
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch,
            basis: grid,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: grid.point_at_stream_frame(frame).unwrap(),
                target: ClockPoint {
                    domain: ClockDomainId(8),
                    timestamp: Timestamp::from_nanos(1_000_000_123),
                },
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}

#[test]
fn identity_rejects_zero_rate_and_playback_ahead_of_physical() {
    assert!(SourcePlaybackIdentity::new(0, 0, 0).is_err());
    assert!(SourcePlaybackIdentity::new(3, 2, 3).is_err());
    let id = SourcePlaybackIdentity::new(3, 9, 4).unwrap();
    assert_eq!(
        (id.hz(), id.initial_physical(), id.initial_playback()),
        (3, 9, 4)
    );
}
#[test]
fn packet_accepts_noncopy_evidence_and_preserves_all_facts() {
    struct Evidence {
        native_words: Vec<u64>,
    }
    let raw = Evidence {
        native_words: vec![11, 29],
    };
    let value = packet(at(3, 3), facts(), Some(raw)).unwrap();
    assert_eq!(value.epoch(), 19);
    assert_eq!(value.basis(), basis(3, 2));
    assert_eq!(
        value.identity(),
        SourcePlaybackIdentity::new(3, 0, 0).unwrap()
    );
    assert_eq!(
        value.query().compare_source_time_frame(3, 3).unwrap(),
        std::cmp::Ordering::Equal
    );
    assert_eq!(value.facts().submitted_source_end, 6);
    assert_eq!(value.native_evidence().unwrap().native_words, [11, 29]);
}
#[test]
fn query_at_positive_submission_endpoint_is_inclusive_but_fractional_future_is_not() {
    assert!(packet::<()>(at(6, 3), facts(), None).is_ok());
    let future =
        InterpolatedTargetTime::new(TargetTime::from_frames(6, 3).unwrap(), 0, 1, u64::MAX, 1, 1)
            .unwrap();
    assert_eq!(
        future.point(basis(3, 2).origin()).unwrap(),
        at(6, 3).point(basis(3, 2).origin()).unwrap()
    );
    assert!(packet::<()>(future, facts(), None).is_err());
    assert!(packet::<()>(at(1, 3), facts(), None).is_err());
}
#[test]
fn packet_refuses_invalid_revision_and_submission_without_modifying_inputs() {
    for case in 0..6 {
        let mut invalid = facts();
        match case {
            0 => invalid.revision = 1,
            1 => invalid.revision = u64::MAX,
            2 => invalid.submitted_source_end = 2,
            3 => invalid.submitted_source_end = 1,
            4 => invalid.submitted_source_end = 9,
            _ => invalid.pulled_source_end = 1,
        }
        let saved = invalid;
        assert!(
            packet::<()>(at(3, 3), invalid, None).is_err(),
            "case {case}"
        );
        assert_eq!(invalid, saved);
    }
}
#[test]
fn identity_must_match_source_rate_and_not_start_after_stream_creation() {
    for id in [
        SourcePlaybackIdentity::new(4, 0, 0).unwrap(),
        SourcePlaybackIdentity::new(3, 3, 0).unwrap(),
    ] {
        assert!(
            NativeSourcePlaybackQuery::<()>::new(19, basis(3, 2), id, at(3, 3), facts(), None)
                .is_err()
        );
    }
}
#[test]
fn half_open_descriptor_selection_refuses_old_span_at_next_transition() {
    let mut bounded = facts();
    bounded.playback =
        SourcePlaybackSpan::new(0, Some(4), 0, SourcePlaybackState::Running).unwrap();
    assert!(packet::<()>(at(3, 3), bounded, None).is_ok());
    assert!(packet::<()>(at(4, 3), bounded, None).is_err());
    let mut next = facts();
    next.playback = SourcePlaybackSpan::new(4, None, 4, SourcePlaybackState::Frozen).unwrap();
    assert!(packet::<()>(at(4, 3), next, None).is_ok());
    assert!(packet::<()>(at(3, 3), next, None).is_err());
}
#[test]
fn descriptor_boundaries_cannot_precede_identity_or_exceed_actual_pulls() {
    let id = SourcePlaybackIdentity::new(3, 2, 1).unwrap();
    for span in [
        SourcePlaybackSpan::new(1, None, 1, SourcePlaybackState::Running).unwrap(),
        SourcePlaybackSpan::new(2, Some(9), 1, SourcePlaybackState::Frozen).unwrap(),
        SourcePlaybackSpan::new(9, None, 1, SourcePlaybackState::Frozen).unwrap(),
    ] {
        let mut invalid = facts();
        invalid.playback = span;
        assert!(
            NativeSourcePlaybackQuery::<()>::new(19, basis(3, 2), id, at(3, 3), invalid, None)
                .is_err()
        );
    }
}
fn real_report() -> RenderReport {
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(32, 64, 1).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 8]).unwrap()
}
#[test]
fn successful_real_render_report_is_optional_and_malformed_extents_are_refused() {
    let report = real_report();
    let mut value = facts();
    value.render = Some(report);
    assert_eq!(
        packet::<()>(at(3, 3), value, None).unwrap().facts().render,
        Some(report)
    );
    for case in 0..6 {
        let mut broken = report;
        match case {
            0 => broken.start_frame = u64::MAX,
            1 => broken.frames = 9,
            2 => broken.playback_frames = 9,
            3 => broken.playback_start_frame = 1,
            4 => broken.playback_end_physical_frame = Some(9),
            _ => broken.playback_start_frame = u64::MAX,
        }
        value.render = Some(broken);
        assert!(packet::<()>(at(3, 3), value, None).is_err(), "case {case}");
    }
}
#[test]
fn observation_requires_evidence_and_identical_epoch_and_basis() {
    assert!(NativePlaybackObservation::new(
        coordinate(19, basis(3, 2), 1),
        packet::<()>(at(3, 3), facts(), None).unwrap()
    )
    .is_err());
    for native in [
        coordinate(20, basis(3, 2), 1),
        coordinate(19, basis(3, 1), 2),
        coordinate(19, basis(4, 2), 1),
    ] {
        assert!(NativePlaybackObservation::new(
            native,
            packet(at(3, 3), facts(), Some(())).unwrap()
        )
        .is_err());
    }
    let observation = NativePlaybackObservation::new(
        coordinate(19, basis(3, 2), 1),
        packet(at(3, 3), facts(), Some(())).unwrap(),
    )
    .unwrap();
    assert_eq!(
        observation.coordinate().coordinate(),
        NativeOutputCoordinate::StreamFrame { frame: 1 }
    );
    assert_eq!(
        observation
            .into_source()
            .query()
            .compare_source_time_frame(3, 3)
            .unwrap(),
        std::cmp::Ordering::Equal
    );
}
#[test]
fn native_frame_query_must_be_exact_even_when_floored_points_match() {
    let extra =
        InterpolatedTargetTime::new(TargetTime::from_frames(3, 3).unwrap(), 0, 1, u64::MAX, 1, 1)
            .unwrap();
    assert_eq!(
        extra.point(basis(3, 2).origin()).unwrap(),
        at(3, 3).point(basis(3, 2).origin()).unwrap()
    );
    assert!(NativePlaybackObservation::new(
        coordinate(19, basis(3, 2), 1),
        packet(extra, facts(), Some(())).unwrap()
    )
    .is_err());
}
#[test]
fn source_counter_uses_extreme_frequency_without_common_denominator_or_floor_fallback() {
    let grid = basis(3, 2);
    let raw = AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        clock: Some(AudioClockSnapshot {
            position: 1,
            frequency: u64::MAX,
            qpc_100ns: 11,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(ClockPoint {
                domain: ClockDomainId(8),
                timestamp: Timestamp::from_nanos(1100),
            }),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
        render: None,
    };
    let native = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 19,
            basis: grid,
            evidence: OriginalNativePresentationEvidence::Wasapi {
                snapshot: raw,
                basis: Some(grid),
            },
        },
        NativeOutputCoordinate::Counter {
            position: 1,
            frequency: u64::MAX,
        },
    )
    .unwrap();
    let query =
        InterpolatedTargetTime::new(TargetTime::from_frames(2, 3).unwrap(), 1, 1, u64::MAX, 0, 1)
            .unwrap();
    assert!(
        NativePlaybackObservation::new(native, packet(query, facts(), Some(())).unwrap()).is_ok()
    );
    assert_eq!(
        query.point(grid.origin()).unwrap(),
        at(2, 3).point(grid.origin()).unwrap()
    );
    assert!(
        NativePlaybackObservation::new(native, packet(at(2, 3), facts(), Some(())).unwrap())
            .is_err()
    );
}
