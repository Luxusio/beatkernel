//! Real converter generation and controlled DI evidence; no hardware permission.
use super::*;
use crate::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
    },
    native_audio_presentation::{NativeAudioPresentation, TargetNativeAudioSnapshot},
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::{ConvertedPlaybackFacts, NativePlaybackMapping},
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Timestamp},
    transport::OutputPhaseBasis,
};
use beatkernel_platform::audio::{
    ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
    presentation::validation::{NativePresentationValidator, OriginalNativePresentationEvidence},
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
struct Rig {
    owner: ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    identity: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    facts: NativeTargetPlaybackFacts,
}
fn rig() -> Rig {
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(128, 256, 1).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
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
    let identity = SourcePlaybackIdentity::new(3, 1, 1).unwrap();
    let mut owner = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(7, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("valid converter"));
    owner.enable_phase_history(8).unwrap();
    let basis = owner.target_frame_basis();
    let report = owner.render_pending(7).unwrap();
    owner.admit(5).unwrap();
    let query = exact(basis.start_time().checked_add_frames(1, 7).unwrap());
    let history = owner.phase_history().unwrap();
    let playback = owner
        .converter_owner()
        .source_playback_history()
        .unwrap()
        .lookup_at_source_time(exact(TargetTime::from_frames(2, 3).unwrap()))
        .unwrap();
    let facts = NativeTargetPlaybackFacts {
        playback: ConvertedPlaybackFacts {
            phase: history.span(0).unwrap(),
            segment_end: history.end_time(),
            generated_target_end: history.end_time(),
            submitted_target_end: owner.admitted_target_time().unwrap(),
            playback,
            pulled_source_end: owner.mixer().frame_cursor(),
            revision: 6,
        },
        source: owner.last_real_source_report(),
        converted: Some(report),
        boundaries: owner.boundaries(),
        revision: 20,
    };
    Rig {
        owner,
        basis,
        identity,
        query,
        facts,
    }
}
fn packet<E>(
    r: &Rig,
    q: InterpolatedTargetTime,
    facts: NativeTargetPlaybackFacts,
    evidence: Option<E>,
) -> Result<NativeTargetPlaybackQuery<E>, TargetPlaybackPacketError> {
    NativeTargetPlaybackQuery::new(19, r.basis, r.identity, q, facts, evidence)
}
fn coordinate(
    epoch: u64,
    basis: TargetFrameBasis,
    frame: u64,
) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: point(1, 2_000_000_000),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}

#[test]
fn real_unequal_rate_nonzero_history_preserves_distinct_creation_and_revisions() {
    let r = rig();
    let p = packet(&r, r.query, r.facts, Some(())).unwrap();
    assert_eq!(p.epoch(), 19);
    assert_eq!(p.identity(), SourcePlaybackIdentity::new(3, 1, 1).unwrap());
    assert_eq!(p.basis(), r.basis);
    assert_eq!(r.basis.start_time(), TargetTime::from_frames(2, 3).unwrap());
    assert_eq!(p.facts().revision, 20);
    assert_eq!(p.facts().playback.revision, 6);
    assert_eq!(p.facts().source, r.owner.last_real_source_report());
    assert_eq!(p.facts().converted, r.facts.converted);
    assert_eq!(p.facts().boundaries, r.owner.boundaries());
}
#[test]
fn opaque_nonclone_evidence_survives_query_observation_and_extraction() {
    struct Evidence {
        words: [u64; 2],
    }
    let r = rig();
    let p = packet(&r, r.query, r.facts, Some(Evidence { words: [11, 29] })).unwrap();
    let observation = NativeTargetPlaybackObservation::new(coordinate(19, r.basis, 1), p).unwrap();
    assert_eq!(
        observation.playback().native_evidence().unwrap().words,
        [11, 29]
    );
    assert_eq!(observation.coordinate(), coordinate(19, r.basis, 1));
    assert_eq!(
        observation.into_playback().native_evidence().unwrap().words,
        [11, 29]
    );
}
#[test]
fn math_query_without_native_evidence_cannot_be_an_observation() {
    let r = rig();
    let p = packet::<()>(&r, r.query, r.facts, None).unwrap();
    assert!(matches!(
        NativeTargetPlaybackObservation::new(coordinate(19, r.basis, 1), p),
        Err(TargetPlaybackPacketError::MissingNativeEvidence)
    ));
}
#[test]
fn odd_and_exhausted_outer_publications_are_refused() {
    let r = rig();
    for revision in [1, 21, u64::MAX] {
        let mut facts = r.facts;
        facts.revision = revision;
        assert!(matches!(
            packet::<()>(&r, r.query, facts, None),
            Err(TargetPlaybackPacketError::InvalidRevision)
        ));
    }
}
#[test]
fn odd_inner_generation_and_wrong_source_rate_refuse_mapping() {
    let r = rig();
    let mut facts = r.facts;
    facts.playback.revision = 7;
    assert!(matches!(
        packet::<()>(&r, r.query, facts, None),
        Err(TargetPlaybackPacketError::InvalidMapping(_))
    ));
    let wrong = SourcePlaybackIdentity::new(4, 1, 1).unwrap();
    assert!(
        NativeTargetPlaybackQuery::<()>::new(19, r.basis, wrong, r.query, r.facts, None).is_err()
    );
}
#[test]
fn positive_submission_endpoint_is_inclusive_but_same_floor_fractional_future_refuses() {
    let r = rig();
    let endpoint = exact(r.facts.playback.submitted_target_end);
    assert!(packet::<()>(&r, endpoint, r.facts, None).is_ok());
    let future =
        InterpolatedTargetTime::new(r.facts.playback.submitted_target_end, 0, 1, u64::MAX, 1, 1)
            .unwrap();
    assert_eq!(
        future.point(r.basis.origin()).unwrap(),
        endpoint.point(r.basis.origin()).unwrap()
    );
    assert!(packet::<()>(&r, future, r.facts, None).is_err());
}
#[test]
fn missing_positive_admission_generated_overrun_and_before_creation_refuse() {
    let r = rig();
    let mut facts = r.facts;
    facts.playback.submitted_target_end = r.basis.start_time();
    assert!(packet::<()>(&r, r.query, facts, None).is_err());
    facts = r.facts;
    facts.playback.submitted_target_end = facts
        .playback
        .generated_target_end
        .checked_add_frames(1, 7)
        .unwrap();
    assert!(packet::<()>(&r, r.query, facts, None).is_err());
    assert!(packet::<()>(
        &r,
        exact(TargetTime::from_frames(1, 3).unwrap()),
        r.facts,
        None
    )
    .is_err());
}
#[test]
fn old_conversion_segment_cannot_own_exact_next_segment_boundary() {
    let r = rig();
    let mut facts = r.facts;
    facts.playback.segment_end = r.query_base();
    assert!(packet::<()>(&r, r.query, facts, None).is_err());
}
impl Rig {
    fn query_base(&self) -> TargetTime {
        self.basis.start_time().checked_add_frames(1, 7).unwrap()
    }
}
#[test]
fn invalid_source_history_ranges_refuse() {
    let r = rig();
    for playback in [
        SourcePlaybackSpan::new(0, None, 0, SourcePlaybackState::Running).unwrap(),
        SourcePlaybackSpan::new(
            2,
            Some(r.facts.playback.pulled_source_end + 1),
            2,
            SourcePlaybackState::Running,
        )
        .unwrap(),
        SourcePlaybackSpan::new(2, None, 0, SourcePlaybackState::Frozen).unwrap(),
    ] {
        let mut facts = r.facts;
        facts.playback.playback = playback;
        assert!(packet::<()>(&r, r.query, facts, None).is_err());
    }
}
#[test]
fn source_report_overflow_and_out_of_pull_extents_refuse() {
    let r = rig();
    for case in 0..5 {
        let mut facts = r.facts;
        let mut report = facts.source.unwrap();
        match case {
            0 => report.start_frame = u64::MAX,
            1 => report.frames = usize::MAX,
            2 => report.playback_frames = report.frames + 1,
            3 => report.playback_start_frame = report.start_frame + 1,
            _ => report.playback_end_physical_frame = Some(facts.playback.pulled_source_end + 1),
        }
        facts.source = Some(report);
        assert!(
            packet::<()>(&r, r.query, facts, None).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn converted_report_rate_time_and_pull_mismatches_refuse() {
    let r = rig();
    for case in 0..6 {
        let mut facts = r.facts;
        let mut report = facts.converted.unwrap();
        match case {
            0 => report.source_rate = 4,
            1 => report.target_rate = 0,
            2 => report.target_end_time = report.target_end_time.checked_add_frames(1, 7).unwrap(),
            3 => report.target_start_time = report.target_end_time,
            4 => report.pulled_source_frame_cursor = facts.playback.pulled_source_end + 1,
            _ => report.source_position.denominator = 0,
        }
        facts.converted = Some(report);
        assert!(
            packet::<()>(&r, r.query, facts, None).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn optional_reports_remain_optional_without_manufacturing_evidence() {
    let r = rig();
    let mut facts = r.facts;
    facts.source = None;
    facts.converted = None;
    assert!(packet::<()>(&r, r.query, facts, None).is_ok());
}
#[test]
fn boundary_origin_rate_and_source_prefix_mismatches_refuse() {
    let r = rig();
    for case in 0..3 {
        let mut facts = r.facts;
        match case {
            0 => facts.boundaries.origin = Some(point(2, 101)),
            1 => facts.boundaries.source_rate = 4,
            _ => {
                facts.boundaries.end = Some(TargetBoundary {
                    source_frame: facts.playback.pulled_source_end + 1,
                    target_frame_offset: 0,
                    target_time: r.query_base(),
                })
            }
        }
        assert!(
            matches!(
                packet::<()>(&r, r.query, facts, None),
                Err(TargetPlaybackPacketError::InvalidBoundary)
            ),
            "case {case}"
        );
    }
}
#[test]
fn generated_future_boundary_does_not_ack_or_require_already_presented_cut() {
    let r = rig();
    let mut facts = r.facts;
    let report = facts.converted.unwrap();
    let boundary = report.project_source_boundary(5).unwrap().unwrap();
    assert!(exact(boundary.target_time)
        .compare_target_time(facts.playback.submitted_target_end)
        .is_gt());
    facts.boundaries.end = Some(boundary);
    assert_eq!(
        packet::<()>(&r, r.query, facts, None)
            .unwrap()
            .facts()
            .boundaries
            .end,
        Some(boundary)
    );
}
#[test]
fn observation_refuses_other_epoch_and_other_immutable_basis() {
    let r = rig();
    assert!(matches!(
        NativeTargetPlaybackObservation::new(
            coordinate(20, r.basis, 1),
            packet(&r, r.query, r.facts, Some(())).unwrap()
        ),
        Err(TargetPlaybackPacketError::EpochMismatch)
    ));
    let wrong = TargetFrameBasis::new(point(2, 101), r.basis.start_time(), 7).unwrap();
    assert!(matches!(
        NativeTargetPlaybackObservation::new(
            coordinate(19, wrong, 1),
            packet(&r, r.query, r.facts, Some(())).unwrap()
        ),
        Err(TargetPlaybackPacketError::BasisMismatch)
    ));
}
#[test]
fn exact_fractional_query_cannot_alias_original_frame_through_rounded_point() {
    let r = rig();
    let aliased = InterpolatedTargetTime::new(r.query_base(), 0, 1, u64::MAX, 1, 1).unwrap();
    assert_eq!(
        aliased.point(r.basis.origin()).unwrap(),
        r.query.point(r.basis.origin()).unwrap()
    );
    let p = packet(&r, aliased, r.facts, Some(())).unwrap();
    assert!(matches!(
        NativeTargetPlaybackObservation::new(coordinate(19, r.basis, 1), p),
        Err(TargetPlaybackPacketError::CoordinateMismatch)
    ));
}
#[test]
fn mathematically_equal_encodings_match_original_target_frame() {
    let r = rig();
    let same = InterpolatedTargetTime::new(r.basis.start_time(), 0, 1, 7, 1, 1).unwrap();
    assert!(same.compare_interpolated_time(r.query).is_eq());
    assert!(NativeTargetPlaybackObservation::new(
        coordinate(19, r.basis, 1),
        packet(&r, same, r.facts, Some(())).unwrap()
    )
    .is_ok());
}
#[test]
fn from_target_reuses_actual_phase_math_and_leaves_authority_pristine() {
    let r = rig();
    let stream_origin = r.basis.point_at_stream_frame(0).unwrap();
    let presentation = NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig::default(),
            AudioAuthorityEpoch {
                id: 19,
                stream_origin,
                logical_origin: point(3, 500),
                host_domain: ClockDomainId(1),
            },
            AudioAuthorityCoordinateBasis::TargetFrame(r.basis),
        )
        .unwrap(),
        NativePresentationValidator::new(19, stream_origin, ClockDomainId(1)),
    )
    .unwrap();
    let logical = presentation
        .authority()
        .epoch()
        .logical_output(r.query.point(r.basis.origin()).unwrap())
        .unwrap();
    let attempt = OutputPhaseBasis::new(ClockDomainId(3), 11, 17, 2, 3).unwrap();
    let before = format!("{presentation:?}");
    let p = packet::<()>(&r, r.query, r.facts, None).unwrap();
    let mapped = NativePlaybackMapping::from_target(&p);
    assert_eq!(mapped.epoch(), 19);
    assert_eq!(mapped.identity(), r.identity);
    assert_eq!(
        mapped
            .phase(&presentation, r.identity, &r.query, attempt, logical)
            .unwrap()
            .elapsed_nanos(),
        142_857_142
    );
    assert_eq!(format!("{presentation:?}"), before);
}

#[test]
fn actual_held_generation_keeps_last_real_source_without_inventing_pause() {
    let mut r = rig();
    r.owner.admit(2).unwrap();
    let last_source = r.owner.last_real_source_report();
    let report = r.owner.render_held_pending(7).unwrap();
    assert_eq!(report.state, ConvertedOutputState::Held);
    assert_eq!(report.source, None);
    r.owner.admit(5).unwrap();
    let history = r.owner.phase_history().unwrap();
    r.facts.playback.phase = history.span(1).unwrap();
    r.facts.playback.segment_end = history.end_time();
    r.facts.playback.generated_target_end = history.end_time();
    r.facts.playback.submitted_target_end = r.owner.admitted_target_time().unwrap();
    r.facts.playback.pulled_source_end = r.owner.mixer().frame_cursor();
    r.facts.converted = Some(report);
    r.facts.source = last_source;
    r.facts.boundaries = r.owner.boundaries();
    r.query = exact(report.target_start_time.checked_add_frames(1, 7).unwrap());
    let p = packet::<()>(&r, r.query, r.facts, None).unwrap();
    assert_eq!(p.facts().source, last_source);
    assert_eq!(p.facts().converted.unwrap().source, None);
    assert_eq!(p.facts().boundaries.pause, None);
    assert_eq!(
        p.facts().playback.playback.state(),
        SourcePlaybackState::Running
    );
}

#[test]
fn actual_cached_empty_source_report_does_not_replace_last_real_pull() {
    let mut r = rig();
    r.owner.admit(2).unwrap();
    let mut seen = false;
    for _ in 0..12 {
        let report = r.owner.render_pending(1).unwrap();
        r.owner.admit(1).unwrap();
        if report.source.unwrap().frames == 0 {
            let last = r.owner.last_real_source_report().unwrap();
            assert!(last.frames > 0);
            assert_ne!(Some(last), report.source);
            let history = r.owner.phase_history().unwrap();
            r.facts.playback.phase = history.span(0).unwrap();
            r.facts.playback.segment_end = history.end_time();
            r.facts.playback.generated_target_end = history.end_time();
            r.facts.playback.submitted_target_end = r.owner.admitted_target_time().unwrap();
            r.facts.playback.pulled_source_end = r.owner.mixer().frame_cursor();
            r.facts.source = Some(last);
            r.facts.converted = Some(report);
            r.facts.boundaries = r.owner.boundaries();
            r.query = exact(report.target_start_time);
            let p = packet::<()>(&r, r.query, r.facts, None).unwrap();
            assert_eq!(p.facts().source, Some(last));
            assert_eq!(p.facts().converted.unwrap().source.unwrap().frames, 0);
            seen = true;
            break;
        }
    }
    assert!(
        seen,
        "unequal-rate converter must exercise an actual cached callback"
    );
}

#[test]
fn held_report_with_source_callback_and_active_report_without_it_refuse() {
    let r = rig();
    let mut facts = r.facts;
    let mut report = facts.converted.unwrap();
    report.source = None;
    facts.converted = Some(report);
    assert!(packet::<()>(&r, r.query, facts, None).is_err());
    report = r.facts.converted.unwrap();
    report.state = ConvertedOutputState::Held;
    facts.converted = Some(report);
    assert!(packet::<()>(&r, r.query, facts, None).is_err());
}

#[test]
fn static_target_port_preserves_backend_specific_evidence_type() {
    // Type checking this function exercises the static Port with an unconstrained
    // native evidence type; no Clone/Debug bound or independent telemetry read.
    #[allow(
        dead_code,
        reason = "Compile-only generic Port contract with no concrete native adapter"
    )]
    fn query<B, O>(
        backend: &B,
        output: &B::Output,
        r: &Rig,
    ) -> Result<Option<NativeTargetPlaybackQuery<B::NativeEvidence>>, B::Error>
    where
        B: TargetPlaybackNativeOutputBackend<O>,
    {
        backend.target_playback_at(output, &r.query, r.identity)
    }
    #[allow(
        dead_code,
        reason = "Compile-only generic Port contract with no concrete native adapter"
    )]
    fn observe<B, O>(
        backend: &mut B,
        output: &mut B::Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackObservation<B::NativeEvidence>>, B::Error>
    where
        B: TargetPlaybackNativeOutputBackend<O>,
    {
        backend.observe_native_target_playback(output, identity)
    }
    // Concrete packet evidence already runs above. These static signatures prove
    // that platform evidence is returned unchanged through the Port contract.
    let _ = (
        std::mem::size_of::<NativeTargetPlaybackFacts>(),
        std::mem::size_of::<NativeTargetPlaybackQuery<()>>(),
    );
}

#[test]
fn converted_target_extent_overflow_refuses_without_saturating() {
    let r = rig();
    let mut facts = r.facts;
    let mut report = facts.converted.unwrap();
    report.target_start_time = TargetTime::new(u64::MAX, 0, 1).unwrap();
    report.target_frames = 7;
    report.target_frame_cursor = 7;
    facts.converted = Some(report);
    assert!(matches!(
        packet::<()>(&r, r.query, facts, None),
        Err(TargetPlaybackPacketError::Overflow)
    ));
}

#[test]
fn every_projected_boundary_refuses_a_cut_beyond_actual_generated_target_end() {
    let r = rig();
    let genuine = r
        .facts
        .converted
        .unwrap()
        .project_source_boundary(5)
        .unwrap()
        .unwrap();
    assert_eq!(genuine.target_time, r.facts.playback.generated_target_end);
    let invalid = TargetBoundary {
        target_time: r
            .facts
            .playback
            .generated_target_end
            .checked_add_frames(1, 7)
            .unwrap(),
        ..genuine
    };
    for kind in 0..4 {
        let mut facts = r.facts;
        match kind {
            0 => facts.boundaries.startup = Some(invalid),
            1 => facts.boundaries.pause = Some(invalid),
            2 => facts.boundaries.resume = Some(invalid),
            _ => facts.boundaries.end = Some(invalid),
        }
        assert!(
            matches!(
                packet::<()>(&r, r.query, facts, None),
                Err(TargetPlaybackPacketError::InvalidBoundary)
            ),
            "boundary kind {kind}"
        );
    }
}
