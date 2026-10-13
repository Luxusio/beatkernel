//! Actual converted histories and admitted DI coordinates, without native ACK permission.
use super::*;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    gameplay::output::{
        application::owner::{
            source_control_owner_fixtures as source_control,
            source_playback_owner_fixtures as source,
        },
        ports::{
            NativeTargetPlaybackFacts, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
            TargetPlaybackPacketError,
        },
    },
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_audio_startup::new_coordinate_audio_presentation,
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::ConvertedPlaybackFacts,
    native_end::NativeEnd,
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, ClockPair, Timestamp},
};
use beatkernel_platform::audio::{
    ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
    presentation::validation::OriginalNativePresentationEvidence,
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
// Intentionally neither Copy, Clone nor Debug; control facts never duplicate packet E.
struct Evidence {
    words: [u64; 2],
}
struct Rig {
    owner: ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    id: SourcePlaybackIdentity,
    facts: NativeTargetPlaybackFacts,
    presentation: NativeAudioPresentation,
    _producer: CommandProducer,
}
fn presentation(epoch: u64, basis: TargetFrameBasis) -> NativeAudioPresentation {
    new_coordinate_audio_presentation(
        epoch,
        AudioAuthorityCoordinateBasis::TargetFrame(basis),
        ClockDomainId(1),
        point(3, 500),
        AudioAuthorityConfig::default(),
    )
    .unwrap()
}
fn rig() -> Rig {
    let format = AudioFormat::new(3, 1).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
            AudioLimits::new(8, 2, 8, 64, 8).unwrap(),
        ),
        SampleBank::new(format, PcmLimits::new(128, 256, 1).unwrap()).unwrap(),
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 1]).unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    mixer.render(&mut [0.0; 1]).unwrap();
    let id = SourcePlaybackIdentity::new(3, 1, 1).unwrap();
    let mut owner = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(7, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("valid conversion"));
    owner.enable_phase_history(8).unwrap();
    let basis = owner.target_frame_basis();
    let report = owner.render_pending(7).unwrap();
    owner.admit(5).unwrap();
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
        id,
        facts,
        presentation: presentation(19, basis),
        _producer: producer,
    }
}
fn coordinate(
    epoch: u64,
    basis: TargetFrameBasis,
    frame: u64,
    host_domain: u32,
    host: i64,
) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: point(host_domain, host),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}
fn observation_with(
    r: &Rig,
    epoch: u64,
    basis: TargetFrameBasis,
    frame: u64,
    domain: u32,
    host: i64,
    q: InterpolatedTargetTime,
    facts: NativeTargetPlaybackFacts,
) -> NativeTargetPlaybackObservation<Evidence> {
    NativeTargetPlaybackObservation::new(
        coordinate(epoch, basis, frame, domain, host),
        NativeTargetPlaybackQuery::new(
            epoch,
            basis,
            r.id,
            q,
            facts,
            Some(Evidence { words: [31, 47] }),
        )
        .unwrap(),
    )
    .unwrap()
}
fn observation(r: &Rig, frame: u64, host: i64) -> NativeTargetPlaybackObservation<Evidence> {
    observation_with(
        r,
        19,
        r.basis,
        frame,
        1,
        host,
        exact(r.basis.start_time().checked_add_frames(frame, 7).unwrap()),
        r.facts,
    )
}
fn admitted() -> (
    Rig,
    NativeTargetPlaybackObservation<Evidence>,
    TargetCoordinateControlFacts,
) {
    let mut r = rig();
    let observation = observation(&r, 1, 1_000_000_000);
    r.presentation
        .admit_target_coordinate(observation.coordinate())
        .unwrap();
    let facts =
        TargetCoordinateControlFacts::from_target(&observation, &r.presentation, r.id).unwrap();
    (r, observation, facts)
}

#[test]
fn unequal_rates_original_history_coordinate_report_and_both_revisions_are_preserved() {
    let (r, original, facts) = admitted();
    assert_eq!(facts.epoch(), 19);
    assert_eq!(
        facts.basis(),
        AudioAuthorityCoordinateBasis::TargetFrame(r.basis)
    );
    assert_eq!(
        facts.identity(),
        SourcePlaybackIdentity::new(3, 1, 1).unwrap()
    );
    assert_eq!(r.basis.sample_rate(), 7);
    assert_eq!(r.basis.start_time(), TargetTime::from_frames(2, 3).unwrap());
    assert_eq!(facts.coordinate(), original.coordinate());
    assert_eq!(facts.pair(), r.presentation.latest_record().unwrap().pair());
    assert_eq!(
        facts
            .query()
            .compare_interpolated_time(original.playback().query()),
        std::cmp::Ordering::Equal
    );
    assert_eq!(facts.target().source, r.facts.source);
    assert_eq!(facts.target().converted, r.facts.converted);
    assert_eq!(facts.target().boundaries, r.facts.boundaries);
    assert_eq!(
        facts.target().playback.phase.to_words(),
        r.facts.playback.phase.to_words()
    );
    assert_eq!(facts.target().playback.playback, r.facts.playback.playback);
    assert_eq!(
        facts.target().playback.segment_end,
        r.facts.playback.segment_end
    );
    assert_eq!(
        facts.target().playback.generated_target_end,
        r.facts.playback.generated_target_end
    );
    assert_eq!(
        facts.target().playback.submitted_target_end,
        r.facts.playback.submitted_target_end
    );
    assert_eq!(
        facts.target().playback.pulled_source_end,
        r.facts.playback.pulled_source_end
    );
    assert_eq!(facts.target().revision, 20);
    assert_eq!(facts.target().playback.revision, 6);
    facts.validate(&r.presentation, r.id).unwrap();
}
#[test]
fn opaque_native_evidence_is_retained_in_original_packet_and_not_required_to_clone() {
    let (r, original, facts) = admitted();
    assert_eq!(
        original.playback().native_evidence().unwrap().words,
        [31, 47]
    );
    let common = CoordinatePlaybackControlFacts::from(facts);
    common.validate(&r.presentation, r.id).unwrap();
    assert_eq!(
        original.playback().native_evidence().unwrap().words,
        [31, 47]
    );
}
#[test]
fn packet_math_without_actual_admission_cannot_create_control_provenance() {
    let r = rig();
    let original = observation(&r, 1, 1_000_000_000);
    let before = format!("{:?}", r.presentation);
    assert!(TargetCoordinateControlFacts::from_target(&original, &r.presentation, r.id).is_err());
    assert_eq!(format!("{:?}", r.presentation), before);
}
#[test]
fn wrong_original_source_history_identity_refuses_without_mutation() {
    let (r, original, facts) = admitted();
    for wrong in [
        SourcePlaybackIdentity::new(3, 0, 0).unwrap(),
        SourcePlaybackIdentity::new(7, 1, 1).unwrap(),
    ] {
        let before = format!("{:?}", r.presentation);
        assert!(
            TargetCoordinateControlFacts::from_target(&original, &r.presentation, wrong).is_err()
        );
        assert!(facts.validate(&r.presentation, wrong).is_err());
        assert_eq!(format!("{:?}", r.presentation), before);
    }
    facts.validate(&r.presentation, r.id).unwrap();
}
#[test]
fn later_admission_invalidates_old_control_facts_and_old_packet() {
    let (mut r, old, facts) = admitted();
    let next = observation(&r, 2, 1_200_000_000);
    r.presentation
        .admit_target_coordinate(next.coordinate())
        .unwrap();
    assert!(facts.validate(&r.presentation, r.id).is_err());
    assert!(TargetCoordinateControlFacts::from_target(&old, &r.presentation, r.id).is_err());
    TargetCoordinateControlFacts::from_target(&next, &r.presentation, r.id).unwrap();
}
#[test]
fn same_frame_different_original_host_is_not_admitted_native_evidence() {
    let (r, _, facts) = admitted();
    let changed = observation(&r, 1, 1_000_000_001);
    assert!(TargetCoordinateControlFacts::from_target(&changed, &r.presentation, r.id).is_err());
    facts.validate(&r.presentation, r.id).unwrap();
}
#[test]
fn wrong_epoch_origin_grid_and_host_domain_are_not_admitted_owner() {
    let (r, _, facts) = admitted();
    for case in 0..4 {
        let basis = match case {
            1 => TargetFrameBasis::new(point(2, 101), r.basis.start_time(), 7).unwrap(),
            2 => TargetFrameBasis::new(r.basis.origin(), r.basis.start_time(), 14).unwrap(),
            _ => r.basis,
        };
        // The different-rate packet is still individually coherent: only its
        // immutable output owner differs from the actual admitted presentation.
        let mut payload = r.facts;
        if case == 1 {
            payload.boundaries.origin = Some(basis.origin());
        }
        if case == 2 {
            payload.converted = None;
        }
        let q = exact(
            basis
                .start_time()
                .checked_add_frames(1, basis.sample_rate())
                .unwrap(),
        );
        let changed = observation_with(
            &r,
            if case == 0 { 20 } else { 19 },
            basis,
            1,
            if case == 3 { 4 } else { 1 },
            1_000_000_000,
            q,
            payload,
        );
        let before = format!("{:?}", r.presentation);
        assert!(
            TargetCoordinateControlFacts::from_target(&changed, &r.presentation, r.id).is_err(),
            "case {case}"
        );
        assert_eq!(format!("{:?}", r.presentation), before);
    }
    facts.validate(&r.presentation, r.id).unwrap();
}
#[test]
fn authority_only_observation_cannot_replace_native_admitted_record() {
    let (mut r, old, facts) = admitted();
    let next = observation(&r, 2, 1_200_000_000);
    let pair = match next.coordinate().snapshot().evidence {
        OriginalNativePresentationEvidence::SuppliedPair(pair) => pair,
        _ => unreachable!(),
    };
    r.presentation
        .authority_mut()
        .observe_coordinate(19, pair, next.coordinate().coordinate())
        .unwrap();
    assert!(TargetCoordinateControlFacts::from_target(&next, &r.presentation, r.id).is_err());
    assert!(TargetCoordinateControlFacts::from_target(&old, &r.presentation, r.id).is_err());
    assert!(facts.validate(&r.presentation, r.id).is_err());
}
#[test]
fn mathematically_equal_fractional_query_is_valid_without_rounding() {
    let (r, _, _) = admitted();
    let equivalent = InterpolatedTargetTime::new(r.basis.start_time(), 2, 2, 14, 0, 1).unwrap();
    let original = observation_with(&r, 19, r.basis, 1, 1, 1_000_000_000, equivalent, r.facts);
    let facts =
        TargetCoordinateControlFacts::from_target(&original, &r.presentation, r.id).unwrap();
    assert_eq!(
        facts.query().compare_interpolated_time(exact(
            r.basis.start_time().checked_add_frames(1, 7).unwrap()
        )),
        std::cmp::Ordering::Equal
    );
}
#[test]
fn same_nanosecond_floor_cannot_forge_original_exact_coordinate_query() {
    let r = rig();
    let true_time = r.basis.start_time().checked_add_frames(1, 7).unwrap();
    let alias = InterpolatedTargetTime::new(true_time, 0, 1, 1_000_000_000_000, 1, 2).unwrap();
    assert_eq!(
        alias.point(r.basis.origin()).unwrap(),
        exact(true_time).point(r.basis.origin()).unwrap()
    );
    assert_ne!(
        alias.compare_interpolated_time(exact(true_time)),
        std::cmp::Ordering::Equal
    );
    let packet = NativeTargetPlaybackQuery::new(
        19,
        r.basis,
        r.id,
        alias,
        r.facts,
        Some(Evidence { words: [31, 47] }),
    )
    .unwrap();
    assert!(matches!(
        NativeTargetPlaybackObservation::new(coordinate(19, r.basis, 1, 1, 1_000_000_000), packet),
        Err(TargetPlaybackPacketError::CoordinateMismatch)
    ));
}
#[test]
fn target_common_getters_return_actual_reports_and_boundaries_without_source_fabrication() {
    let (r, _, facts) = admitted();
    let common = CoordinatePlaybackControlFacts::Target(facts);
    assert_eq!(common.epoch(), facts.epoch());
    assert_eq!(common.basis(), facts.basis());
    assert_eq!(common.identity(), facts.identity());
    assert_eq!(common.pair(), facts.pair());
    assert_eq!(
        common.query().compare_interpolated_time(facts.query()),
        std::cmp::Ordering::Equal
    );
    assert_eq!(common.report(), r.facts.source);
    assert_eq!(common.target_boundaries(), Some(r.facts.boundaries));
    common.validate(&r.presentation, r.id).unwrap();
}
#[test]
fn absent_target_source_report_stays_absent_in_common_control_facts() {
    let (r, _, _) = admitted();
    let mut payload = r.facts;
    payload.source = None;
    let original = observation_with(
        &r,
        19,
        r.basis,
        1,
        1,
        1_000_000_000,
        exact(r.basis.start_time().checked_add_frames(1, 7).unwrap()),
        payload,
    );
    let facts =
        TargetCoordinateControlFacts::from_target(&original, &r.presentation, r.id).unwrap();
    let common = CoordinatePlaybackControlFacts::from(facts);
    assert_eq!(common.report(), None);
    assert_eq!(common.target_boundaries(), Some(payload.boundaries));
}
#[test]
fn actual_cached_zero_report_is_preserved_instead_of_filtered_or_replaced() {
    let mut r = rig();
    r.owner.admit(2).unwrap();
    let mut seen = false;
    for _ in 0..12 {
        let report = r.owner.render_pending(1).unwrap();
        r.owner.admit(1).unwrap();
        if report.source.unwrap().frames != 0 {
            continue;
        }
        let history = r.owner.phase_history().unwrap();
        r.facts.playback.phase = history.span(0).unwrap();
        r.facts.playback.segment_end = history.end_time();
        r.facts.playback.generated_target_end = history.end_time();
        r.facts.playback.submitted_target_end = r.owner.admitted_target_time().unwrap();
        r.facts.playback.pulled_source_end = r.owner.mixer().frame_cursor();
        r.facts.source = report.source;
        r.facts.converted = Some(report);
        r.facts.boundaries = r.owner.boundaries();
        let original = observation(&r, 1, 1_000_000_000);
        r.presentation
            .admit_target_coordinate(original.coordinate())
            .unwrap();
        let facts =
            TargetCoordinateControlFacts::from_target(&original, &r.presentation, r.id).unwrap();
        assert_eq!(
            CoordinatePlaybackControlFacts::from(facts).report(),
            report.source
        );
        assert_eq!(facts.target().source.unwrap().frames, 0);
        assert!(r.owner.last_real_source_report().unwrap().frames > 0);
        seen = true;
        break;
    }
    assert!(
        seen,
        "actual 3->7 converter should exercise a cached callback"
    );
}
#[test]
fn source_variant_common_getters_preserve_existing_source_contract() {
    let (owner, mut p, _, _producer, id) = source_control::low_rate_fixture();
    let output = owner.current().unwrap();
    let observation = source_control::frame_observation(output.basis, id, 1, 100, output.report);
    p.admit_coordinate(observation.coordinate()).unwrap();
    let facts = CoordinateControlFacts::from_source(&observation, &p, id).unwrap();
    let common = CoordinatePlaybackControlFacts::from(facts);
    assert_eq!(common.epoch(), facts.epoch());
    assert_eq!(common.basis(), facts.basis());
    assert_eq!(common.identity(), id);
    assert_eq!(common.pair(), facts.pair());
    assert_eq!(
        common
            .query()
            .compare_interpolated_time(source::query(3, 3)),
        std::cmp::Ordering::Equal
    );
    assert_eq!(common.report(), facts.source().render);
    assert_eq!(common.target_boundaries(), None);
    common.validate(&p, id).unwrap();
    assert_eq!(facts.source(), observation.source().facts());
}
#[test]
fn target_common_handoff_stages_existing_end_without_native_ack_or_mutation() {
    let (r, _, facts) = admitted();
    let common = CoordinatePlaybackControlFacts::from(facts);
    let end = NativeEnd::new(r.basis.origin(), ClockDomainId(1), 3, 20)
        .unwrap()
        .with_target_basis(common.epoch(), r.basis)
        .unwrap()
        .with_coordinates(common.epoch(), common.basis(), common.identity())
        .unwrap();
    let before = format!("{end:?}");
    let staged = end
        .stage_coordinate_target(
            common.epoch(),
            common.basis(),
            common.identity(),
            common.target_boundaries().unwrap(),
            common.report(),
            common.pair(),
        )
        .unwrap();
    assert_eq!(staged.epoch(), common.epoch());
    assert_eq!(staged.identity(), common.identity());
    assert_eq!(staged.physical_frame(), None);
    assert!(staged.query().is_none());
    assert_eq!(format!("{end:?}"), before);
    common.validate(&r.presentation, r.id).unwrap();
}
#[test]
fn admitted_source_presentation_cannot_supply_target_control_facts() {
    let (r, original, _) = admitted();
    let (_owner, p, _, _producer, _) = source_control::low_rate_fixture();
    assert!(TargetCoordinateControlFacts::from_target(&original, &p, r.id).is_err());
}
