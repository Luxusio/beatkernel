//! Actual converter histories with scripted original ALSA timing.
//! This validates packet consistency, without device acquisition or acoustic proof.
use super::*;
use crate::gameplay::output::ports::{NativeTargetPlaybackQuery, TargetPlaybackNativeOutputBackend};
use beatkernel::audio::*;
use beatkernel::time::{ClockDomainId, ClockMappingQuality, ClockPoint, Timestamp};
use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};
use beatkernel_platform::linux::{
    AlsaConvertedPlaybackSnapshot, AlsaNativeTimestamp, ConvertedPhaseSnapshot,
    ConvertedPlaybackPhaseSnapshot,
};

fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(8),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn native(submitted: u64, played: u64) -> AlsaTimingSnapshot {
    AlsaTimingSnapshot {
        native_state: 3,
        submitted_frames: submitted,
        delay_frames: i64::try_from(submitted - played).unwrap(),
        available_frames: 91,
        native_htstamp: AlsaNativeTimestamp {
            seconds: 1,
            nanoseconds: 123,
        },
        native_timestamp: Some(point(1_000_000_123)),
        query_started: point(1_000_000_100),
        query_finished: point(1_000_000_140),
        estimated_played_frames: Some(played),
        quality: ClockMappingQuality::Unknown,
        timestamp_mode: 1,
        timestamp_type: 1,
    }
}
struct Rig {
    owner: ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    identity: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    snapshot: AlsaConvertedPlaybackSnapshot,
}
fn rig() -> Rig {
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(128, 256, 1).unwrap()).unwrap();
    let (_, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
            AudioLimits::new(8, 2, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 1]).unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    mixer.render(&mut [0.0; 1]).unwrap();
    let history = mixer.source_playback_history().unwrap();
    let identity = SourcePlaybackIdentity::new(
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
    .unwrap();
    let mut owner = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(7, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("valid unequal-rate converted owner"));
    owner.enable_phase_history(8).unwrap();
    let basis = owner.target_frame_basis();
    let report = owner.render_pending(7).unwrap();
    owner.admit(5).unwrap();
    let query = exact(basis.time_at_stream_frame(1).unwrap());
    let history = owner.phase_history().unwrap();
    let playback = owner
        .converter_owner()
        .source_playback_history()
        .unwrap()
        .lookup_at_source_time(exact(TargetTime::from_frames(2, 3).unwrap()))
        .unwrap();
    let snapshot = AlsaConvertedPlaybackSnapshot {
        phase: ConvertedPlaybackPhaseSnapshot {
            phase: ConvertedPhaseSnapshot {
                span: history.span(0).unwrap(),
                segment_end: history.end_time(),
                generated_end: history.end_time(),
                stream_start: basis.start_time(),
                submitted_end: owner.admitted_target_time().unwrap(),
                revision: 6,
                report: Some(report),
                boundaries: owner.boundaries(),
                last_real_source: owner.last_real_source_report(),
            },
            playback,
            pulled_source_end: owner.mixer().frame_cursor(),
        },
        timing: Some(native(5, 1)),
        submitted_target_frames: 5,
        revision: 20,
    };
    Rig {
        owner,
        basis,
        identity,
        query,
        snapshot,
    }
}
fn packet(
    r: &Rig,
    q: InterpolatedTargetTime,
    snapshot: AlsaConvertedPlaybackSnapshot,
) -> Result<NativeTargetPlaybackQuery<AlsaTimingSnapshot>, AlsaReplacementError> {
    qualify_alsa_target_packet(23, r.basis, r.identity, q, snapshot)
}

#[test]
fn actual_unequal_rates_and_distinct_source_creation_survive_target_qualification() {
    let r = rig();
    assert_eq!(r.identity, SourcePlaybackIdentity::new(3, 1, 1).unwrap());
    assert_eq!(r.basis.sample_rate(), 7);
    assert_eq!(r.basis.start_time(), TargetTime::from_frames(2, 3).unwrap());
    let p = packet(&r, r.query, r.snapshot).unwrap();
    let candidate = r.snapshot.timing.unwrap();
    let observation = qualify_alsa_target_observation_packet(23, r.basis, r.identity, candidate, p)
        .unwrap()
        .unwrap();
    assert_eq!(observation.playback().identity(), r.identity);
    assert_eq!(observation.playback().native_evidence(), Some(&candidate));
    assert_eq!(observation.coordinate().snapshot().basis, r.basis);
    assert_eq!(
        observation.coordinate().coordinate(),
        crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame { frame: 1 }
    );
    assert_eq!(
        observation
            .playback()
            .facts()
            .converted
            .unwrap()
            .source_rate,
        3
    );
    assert_eq!(
        observation
            .playback()
            .facts()
            .converted
            .unwrap()
            .target_rate,
        7
    );
}
#[test]
fn original_source_converted_boundaries_and_both_publication_revisions_are_retained() {
    let r = rig();
    let p = packet(&r, r.query, r.snapshot).unwrap();
    let facts = p.facts();
    assert_eq!(facts.revision, 20);
    assert_eq!(facts.playback.revision, 6);
    assert_eq!(
        facts.playback.phase.to_words(),
        r.snapshot.phase.phase.span.to_words()
    );
    assert_eq!(
        facts.playback.segment_end,
        r.owner.phase_history().unwrap().end_time()
    );
    assert_eq!(
        facts.playback.generated_target_end,
        r.owner.phase_history().unwrap().end_time()
    );
    assert_eq!(
        facts.playback.submitted_target_end,
        r.owner.admitted_target_time().unwrap()
    );
    assert_eq!(facts.playback.playback, r.snapshot.phase.playback);
    assert_eq!(
        facts.playback.pulled_source_end,
        r.owner.mixer().frame_cursor()
    );
    assert_eq!(facts.source, r.owner.last_real_source_report());
    assert_eq!(facts.converted, r.snapshot.phase.phase.report);
    assert_eq!(facts.boundaries, r.owner.boundaries());
    assert_eq!(
        p.native_evidence().unwrap().quality,
        ClockMappingQuality::Unknown
    );
}
#[test]
fn absent_timing_preserves_math_facts_without_manufacturing_a_native_observation() {
    let r = rig();
    let mut snapshot = r.snapshot;
    let candidate = snapshot.timing.take().unwrap();
    let p = packet(&r, r.query, snapshot).unwrap();
    assert!(p.native_evidence().is_none());
    assert_eq!(p.facts().source, r.owner.last_real_source_report());
    assert_eq!(p.facts().converted, snapshot.phase.phase.report);
    assert!(
        qualify_alsa_target_observation_packet(23, r.basis, r.identity, candidate, p)
            .unwrap()
            .is_none()
    );
}
#[test]
fn every_material_original_candidate_field_is_compared_before_observation() {
    let r = rig();
    for case in 0..16 {
        let mut candidate = r.snapshot.timing.unwrap();
        match case {
            0 => candidate.native_state = 4,
            1 => candidate.submitted_frames += 1,
            2 => candidate.delay_frames = -1,
            3 => candidate.available_frames += 1,
            4 => candidate.native_htstamp.nanoseconds += 1,
            5 => candidate.native_timestamp = None,
            6 => candidate.query_started.timestamp = Timestamp::from_nanos(1_000_000_101),
            7 => candidate.query_finished.timestamp = Timestamp::from_nanos(1_000_000_141),
            8 => candidate.estimated_played_frames = Some(2),
            9 => candidate.timestamp_mode = 0,
            10 => candidate.timestamp_type = 0,
            11 => candidate.query_finished.domain = ClockDomainId(9),
            12 => candidate.quality = ClockMappingQuality::Exact,
            13 => candidate.native_htstamp.seconds += 1,
            14 => candidate.query_started.domain = ClockDomainId(9),
            _ => candidate.native_timestamp.as_mut().unwrap().domain = ClockDomainId(9),
        }
        let p = packet(&r, r.query, r.snapshot).unwrap();
        assert!(
            qualify_alsa_target_observation_packet(23, r.basis, r.identity, candidate, p)
                .unwrap()
                .is_none(),
            "case {case}"
        );
    }
}
#[test]
fn matching_malformed_original_native_metadata_is_rejected() {
    let r = rig();
    for case in 0..10 {
        let mut snapshot = r.snapshot;
        let mut candidate = snapshot.timing.unwrap();
        match case {
            0 => candidate.native_state = 4,
            1 => candidate.delay_frames = -1,
            2 => candidate.native_htstamp.nanoseconds = 1_000_000_000,
            3 => candidate.native_htstamp.seconds = -1,
            4 => {
                candidate.native_timestamp.as_mut().unwrap().timestamp =
                    Timestamp::from_nanos(1_000_000_124)
            }
            5 => candidate.query_started.timestamp = Timestamp::from_nanos(1_000_000_141),
            6 => candidate.query_finished.domain = ClockDomainId(9),
            7 => candidate.timestamp_mode = 0,
            8 => candidate.quality = ClockMappingQuality::Exact,
            _ => candidate.timestamp_type = 0,
        }
        snapshot.timing = Some(candidate);
        let p = packet(&r, r.query, snapshot).unwrap();
        assert!(
            qualify_alsa_target_observation_packet(23, r.basis, r.identity, candidate, p).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn submitted_count_must_equal_original_timing_and_target_grid_endpoint() {
    let r = rig();
    for case in 0..4 {
        let mut snapshot = r.snapshot;
        match case {
            0 => snapshot.timing.as_mut().unwrap().submitted_frames = 4,
            1 => snapshot.submitted_target_frames = 4,
            2 => snapshot.phase.phase.submitted_end = r.basis.time_at_stream_frame(4).unwrap(),
            _ => {
                snapshot.submitted_target_frames = 0;
                snapshot.timing = None;
                snapshot.phase.phase.submitted_end = r.basis.start_time();
            }
        }
        assert!(packet(&r, r.query, snapshot).is_err(), "case {case}");
    }
}
#[test]
fn exact_submitted_endpoint_is_inclusive_but_floor_aliased_future_is_refused() {
    let r = rig();
    let end = r.snapshot.phase.phase.submitted_end;
    let endpoint = exact(end);
    assert!(packet(&r, endpoint, r.snapshot).is_ok());
    let future = InterpolatedTargetTime::new(end, 0, 1, u64::MAX, 1, 1).unwrap();
    assert_eq!(
        future.point(r.basis.origin()).unwrap(),
        endpoint.point(r.basis.origin()).unwrap()
    );
    assert!(packet(&r, future, r.snapshot).is_err());
}
#[test]
fn floor_aliased_fractional_query_cannot_replace_original_native_played_frame() {
    let r = rig();
    let query = InterpolatedTargetTime::new(
        r.basis.time_at_stream_frame(1).unwrap(),
        0,
        1,
        u64::MAX,
        1,
        1,
    )
    .unwrap();
    assert_eq!(
        query.point(r.basis.origin()).unwrap(),
        r.query.point(r.basis.origin()).unwrap()
    );
    let p = packet(&r, query, r.snapshot).unwrap();
    assert!(qualify_alsa_target_observation_packet(
        23,
        r.basis,
        r.identity,
        r.snapshot.timing.unwrap(),
        p
    )
    .is_err());
}
#[test]
fn mathematically_equal_query_encodings_preserve_original_native_coordinate() {
    let r = rig();
    let query = InterpolatedTargetTime::new(r.basis.start_time(), 0, 1, 7, 1, 1).unwrap();
    assert!(query.compare_interpolated_time(r.query).is_eq());
    let p = packet(&r, query, r.snapshot).unwrap();
    assert!(qualify_alsa_target_observation_packet(
        23,
        r.basis,
        r.identity,
        r.snapshot.timing.unwrap(),
        p
    )
    .unwrap()
    .is_some());
}
#[test]
fn packet_epoch_basis_and_original_identity_must_match_observation_request() {
    let r = rig();
    let wrong_basis = TargetFrameBasis::new(r.basis.origin(), r.basis.start_time(), 8).unwrap();
    for (epoch, basis, identity) in [
        (24, r.basis, r.identity),
        (23, wrong_basis, r.identity),
        (23, r.basis, SourcePlaybackIdentity::new(3, 0, 0).unwrap()),
    ] {
        let p = packet(&r, r.query, r.snapshot).unwrap();
        assert!(qualify_alsa_target_observation_packet(
            epoch,
            basis,
            identity,
            r.snapshot.timing.unwrap(),
            p
        )
        .is_err());
    }
}
#[test]
fn wrong_source_rate_and_initial_history_ranges_refuse_qualification() {
    let r = rig();
    for identity in [
        SourcePlaybackIdentity::new(4, 1, 1).unwrap(),
        SourcePlaybackIdentity::new(3, 2, 2).unwrap(),
    ] {
        assert!(qualify_alsa_target_packet(23, r.basis, identity, r.query, r.snapshot).is_err());
    }
    // Earlier history creation cannot be inferred from conversion's later origin.
    assert_eq!(r.snapshot.phase.playback.physical_start(), 1);
    assert_eq!(r.basis.start_time(), TargetTime::from_frames(2, 3).unwrap());
}
#[test]
fn outer_and_inner_invalid_revisions_remain_distinct_refusals() {
    let r = rig();
    for case in 0..4 {
        let mut snapshot = r.snapshot;
        match case {
            0 => snapshot.revision = 21,
            1 => snapshot.revision = u64::MAX,
            2 => snapshot.phase.phase.revision = 7,
            _ => snapshot.phase.phase.revision = u64::MAX,
        }
        assert!(packet(&r, r.query, snapshot).is_err(), "case {case}");
    }
}
#[test]
fn original_stream_creation_and_boundary_source_identity_cannot_be_replaced() {
    let r = rig();
    for case in 0..3 {
        let mut snapshot = r.snapshot;
        match case {
            0 => snapshot.phase.phase.stream_start = TargetTime::from_frames(1, 3).unwrap(),
            1 => {
                snapshot
                    .phase
                    .phase
                    .boundaries
                    .origin
                    .as_mut()
                    .unwrap()
                    .timestamp = Timestamp::from_nanos(101)
            }
            _ => snapshot.phase.phase.boundaries.source_rate = 7,
        }
        assert!(packet(&r, r.query, snapshot).is_err(), "case {case}");
    }
}
#[test]
fn source_and_converted_reports_are_checked_without_independent_fallback_reads() {
    let r = rig();
    for case in 0..4 {
        let mut snapshot = r.snapshot;
        match case {
            0 => {
                snapshot
                    .phase
                    .phase
                    .last_real_source
                    .as_mut()
                    .unwrap()
                    .start_frame = u64::MAX
            }
            1 => snapshot.phase.phase.report.as_mut().unwrap().source_rate = 7,
            2 => snapshot.phase.phase.report.as_mut().unwrap().target_rate = 3,
            _ => {
                snapshot
                    .phase
                    .phase
                    .report
                    .as_mut()
                    .unwrap()
                    .pulled_source_frame_cursor = snapshot.phase.pulled_source_end + 1
            }
        }
        assert!(packet(&r, r.query, snapshot).is_err(), "case {case}");
    }
}
#[test]
fn future_generated_boundary_is_retained_without_native_ack_or_presented_cut() {
    let r = rig();
    let mut snapshot = r.snapshot;
    let report = snapshot.phase.phase.report.unwrap();
    let boundary = report.project_source_boundary(5).unwrap().unwrap();
    assert!(exact(boundary.target_time)
        .compare_target_time(snapshot.phase.phase.submitted_end)
        .is_gt());
    snapshot.phase.phase.boundaries.end = Some(boundary);
    let p = packet(&r, r.query, snapshot).unwrap();
    let observation = qualify_alsa_target_observation_packet(
        23,
        r.basis,
        r.identity,
        snapshot.timing.unwrap(),
        p,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        observation.playback().facts().boundaries.end,
        Some(boundary)
    );
    assert_eq!(r.owner.boundaries().end, None);
    assert_eq!(r.owner.pending_frames(), 2);
}
#[test]
fn optional_reports_stay_optional_while_original_timing_stays_original() {
    let r = rig();
    let mut snapshot = r.snapshot;
    snapshot.phase.phase.last_real_source = None;
    snapshot.phase.phase.report = None;
    let p = packet(&r, r.query, snapshot).unwrap();
    assert!(p.facts().source.is_none());
    assert!(p.facts().converted.is_none());
    assert_eq!(p.native_evidence(), snapshot.timing.as_ref());
}
#[test]
fn actual_alsa_adapter_implements_the_static_target_playback_port() {
    fn requires_port<
        B: TargetPlaybackNativeOutputBackend<
            ConvertedNativeOutputState,
            NativeEvidence = AlsaTimingSnapshot,
        >,
    >() {
    }
    requires_port::<ConvertedAlsaReplacementBackend>();
}
