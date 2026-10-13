//! Real compound native admission and actual staged owners; no physical device proof.
use super::*;
use crate::{
    gameplay::output::{
        application::owner::{
            source_control_owner_fixtures as control, source_playback_owner_fixtures as source,
        },
        ports::{NativePlaybackObservation, SourcePlaybackIdentity},
    },
    audio_authority::AudioAuthorityCoordinateBasis,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
    native_end::NativeEnd,
    playback_pause::{NativePause, PausePhase},
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockMappingQuality, ClockPair},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters, presentation::validation::OriginalNativePresentationEvidence,
};

fn admitted() -> (
    CoordinateControlFacts,
    NativeAudioPresentation,
    SourcePlaybackIdentity,
    NativePlaybackObservation<source::Evidence>,
) {
    let (owner, mut p, _io, _producer, id) = control::low_rate_fixture();
    let output = owner.current().unwrap();
    let observation = control::frame_observation(output.basis, id, 1, 100, output.report);
    p.admit_coordinate(observation.coordinate()).unwrap();
    let facts = CoordinateControlFacts::from_source(&observation, &p, id).unwrap();
    (facts, p, id, observation)
}

#[test]
fn admitted_original_frame_creation_history_cut_pair_and_report_are_verbatim() {
    let (facts, p, id, observation) = admitted();
    assert_eq!(facts.epoch(), 7);
    assert_eq!(id.initial_physical(), 2);
    assert_eq!(id.initial_playback(), 2);
    assert_eq!(facts.identity(), id);
    assert_eq!(
        facts.basis(),
        AudioAuthorityCoordinateBasis::SourceFrame(observation.coordinate().snapshot().basis)
    );
    assert_eq!(facts.coordinate(), observation.coordinate());
    assert_eq!(
        facts.pair(),
        ClockPair {
            source: source::point(2, 1_000_000_100),
            target: source::point(1, 100)
        }
    );
    assert_eq!(facts.source(), observation.source().facts());
    assert_eq!(facts.source().revision, 2);
    assert_eq!(facts.source().render.unwrap().start_frame, 2);
    assert_eq!(
        facts.query().compare_interpolated_time(source::query(3, 3)),
        std::cmp::Ordering::Equal
    );
    facts.validate(&p, id).unwrap();
    assert_eq!(
        observation.source().native_evidence().unwrap().words,
        [19, 31]
    );
}

#[test]
fn arithmetic_packet_without_actual_native_admission_cannot_supply_control_facts() {
    let (owner, p, _io, _producer, id) = control::low_rate_fixture();
    let output = owner.current().unwrap();
    let observation = control::frame_observation(output.basis, id, 1, 100, output.report);
    let before = format!("{p:?}");
    assert!(CoordinateControlFacts::from_source(&observation, &p, id).is_err());
    assert_eq!(format!("{p:?}"), before);
}

#[test]
fn expected_history_identity_is_required_even_with_the_same_creation_basis() {
    let (facts, p, id, observation) = admitted();
    let before = format!("{p:?}");
    let wrong = SourcePlaybackIdentity::new(3, 1, 1).unwrap();
    assert!(CoordinateControlFacts::from_source(&observation, &p, wrong).is_err());
    assert!(facts.validate(&p, wrong).is_err());
    assert_eq!(format!("{p:?}"), before);
    facts.validate(&p, id).unwrap();
}

#[test]
fn later_admitted_original_observation_invalidates_previous_control_publication() {
    let (facts, mut p, id, old) = admitted();
    let next = control::frame_observation(
        old.coordinate().snapshot().basis,
        id,
        2,
        101,
        old.source().facts().render,
    );
    p.admit_coordinate(next.coordinate()).unwrap();
    let before = format!("{p:?}");
    assert!(facts.validate(&p, id).is_err());
    assert!(CoordinateControlFacts::from_source(&old, &p, id).is_err());
    CoordinateControlFacts::from_source(&next, &p, id).unwrap();
    assert_eq!(format!("{p:?}"), before);
}

#[test]
fn same_coordinate_with_different_host_is_not_the_admitted_publication() {
    let (_facts, p, id, admitted) = admitted();
    let changed = control::frame_observation(
        admitted.coordinate().snapshot().basis,
        id,
        1,
        101,
        admitted.source().facts().render,
    );
    let before = format!("{p:?}");
    assert!(CoordinateControlFacts::from_source(&changed, &p, id).is_err());
    assert_eq!(format!("{p:?}"), before);
}

#[test]
fn changed_descriptor_epoch_raw_and_host_domains_refuse_without_mutation() {
    let (facts, p, id, old) = admitted();
    let basis = old.coordinate().snapshot().basis;
    for case in 0..4 {
        let b = if case == 1 {
            OutputFrameBasis::new(source::point(2, 101), 3, 2).unwrap()
        } else if case == 2 {
            OutputFrameBasis::new(source::point(4, 100), 3, 2).unwrap()
        } else {
            basis
        };
        let epoch = if case == 0 { 8 } else { 7 };
        let coordinate = CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch,
                basis: b,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: b.point_at_stream_frame(1).unwrap(),
                    target: source::point(if case == 3 { 4 } else { 1 }, 100),
                }),
            },
            NativeOutputCoordinate::StreamFrame { frame: 1 },
        )
        .unwrap();
        let packet = source::packet(
            b,
            epoch,
            id,
            source::query(3, 3),
            old.source().facts().render,
        );
        let changed = NativePlaybackObservation::new(coordinate, packet).unwrap();
        let before = format!("{p:?}");
        assert!(
            CoordinateControlFacts::from_source(&changed, &p, id).is_err(),
            "case {case}"
        );
        assert_eq!(format!("{p:?}"), before);
    }
    facts.validate(&p, id).unwrap();
}

fn counter_observation(
    basis: OutputFrameBasis,
    id: SourcePlaybackIdentity,
    position: u64,
    frequency: u64,
    host: i64,
) -> NativePlaybackObservation<source::Evidence> {
    let snapshot = AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position,
            frequency,
            qpc_100ns: host as u64,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(source::point(1, host)),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    };
    let coordinate = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::Wasapi {
                snapshot,
                basis: Some(basis),
            },
        },
        NativeOutputCoordinate::Counter {
            position,
            frequency,
        },
    )
    .unwrap();
    let q = InterpolatedTargetTime::new(
        TargetTime::from_frames(basis.start_physical_frame(), id.hz()).unwrap(),
        position,
        position,
        frequency,
        0,
        1,
    )
    .unwrap();
    NativePlaybackObservation::new(coordinate, source::packet(basis, 7, id, q, None)).unwrap()
}

#[test]
fn original_counter_frequency_and_fraction_are_retained_without_rounding() {
    let (owner, _p, _io, _producer, id) = control::low_rate_fixture();
    let basis = owner.current().unwrap().basis;
    let mut p = source::presentation(basis, Some(12));
    let observation = counter_observation(basis, id, 1, 12, 100);
    p.admit_coordinate(observation.coordinate()).unwrap();
    let facts = CoordinateControlFacts::from_source(&observation, &p, id).unwrap();
    assert_eq!(
        facts.basis(),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis,
            frequency: 12
        }
    );
    assert_eq!(
        facts.coordinate().coordinate(),
        NativeOutputCoordinate::Counter {
            position: 1,
            frequency: 12
        }
    );
    assert_eq!(
        facts
            .query()
            .compare_interpolated_time(observation.source().query()),
        std::cmp::Ordering::Equal
    );
    let mismatch = counter_observation(basis, id, 2, 24, 100);
    assert_eq!(
        mismatch
            .source()
            .query()
            .compare_interpolated_time(facts.query()),
        std::cmp::Ordering::Equal
    );
    assert!(CoordinateControlFacts::from_source(&mismatch, &p, id).is_err());
}

#[test]
fn pause_adoption_stages_actual_mixer_report_without_acknowledging_owner() {
    let (mut owner, mut p, io, mut producer, id) = control::low_rate_fixture();
    let basis = owner.current().unwrap().basis;
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(control::frame_observation(
            basis,
            id,
            1,
            100,
            owner.current().unwrap().report,
        ))));
    owner.observe_native_control(&mut p, id).unwrap().unwrap();
    let mut pause = NativePause::new(basis.origin(), ClockDomainId(1), 3)
        .unwrap()
        .with_coordinate_binding(7, AudioAuthorityCoordinateBasis::SourceFrame(basis), id)
        .unwrap();
    pause.request_coordinate(true, p.authority()).unwrap();
    producer.request_pause(true);
    let report = owner
        .current_mut()
        .unwrap()
        .mixer
        .as_mut()
        .unwrap()
        .render(&mut [0.0; 1])
        .unwrap();
    let coordinate = control::frame_observation(basis, id, 2, 101, None).coordinate();
    let history = owner
        .current()
        .unwrap()
        .mixer
        .as_ref()
        .unwrap()
        .source_playback_history()
        .unwrap();
    let packet = crate::gameplay::output::ports::NativeSourcePlaybackQuery::new(
        7,
        basis,
        id,
        source::query(4, 3),
        crate::gameplay::output::ports::SourcePlaybackFacts {
            playback: history.span(0).unwrap(),
            pulled_source_end: 9,
            submitted_source_end: 9,
            render: Some(report),
            revision: 4,
        },
        Some(source::Evidence {
            words: vec![19, 31],
        }),
    )
    .unwrap();
    io.observations.borrow_mut().push_back(Ok(Some(
        NativePlaybackObservation::new(coordinate, packet).unwrap(),
    )));
    let facts = owner.observe_native_control(&mut p, id).unwrap().unwrap();
    let before = format!("{pause:?}");
    let prepared = pause
        .prepare_coordinate_adoption(facts.source().render, None)
        .unwrap()
        .unwrap();
    assert_eq!(prepared.physical_frame(), 8);
    assert_eq!(prepared.playback_frame(), 8);
    assert_eq!(prepared.source_identity(), id);
    assert_eq!(prepared.epoch(), facts.epoch());
    assert_eq!(pause.phase(), PausePhase::Pausing);
    assert_eq!(format!("{pause:?}"), before);
}

#[test]
fn end_source_observation_stages_without_scalar_emit_or_owner_commit() {
    let (facts, p, id, _observation) = admitted();
    let basis = match facts.basis() {
        AudioAuthorityCoordinateBasis::SourceFrame(b) => b,
        _ => unreachable!(),
    };
    let end = NativeEnd::new(basis.origin(), ClockDomainId(1), 3, 20)
        .unwrap()
        .with_coordinates(facts.epoch(), facts.basis(), id)
        .unwrap();
    let before = format!("{end:?}");
    let staged = end
        .stage_coordinate_source(
            facts.epoch(),
            facts.basis(),
            facts.identity(),
            facts.source().render,
            facts.pair(),
        )
        .unwrap();
    assert_eq!(staged.epoch(), 7);
    assert_eq!(staged.identity(), id);
    assert_eq!(staged.physical_frame(), None);
    assert!(staged.query().is_none());
    assert_eq!(format!("{end:?}"), before);
    facts.validate(&p, id).unwrap();
}

#[test]
fn target_descriptor_is_explicitly_unsupported_without_synthesized_source_control() {
    let (_facts, _source_p, id, observation) = admitted();
    let basis = observation.coordinate().snapshot().basis;
    let target =
        TargetFrameBasis::new(basis.origin(), TargetTime::from_frames(2, 3).unwrap(), 3).unwrap();
    let p = crate::native_audio_startup::new_coordinate_audio_presentation(
        7,
        AudioAuthorityCoordinateBasis::TargetFrame(target),
        ClockDomainId(1),
        source::point(3, 500),
        crate::audio_authority::AudioAuthorityConfig::default(),
    )
    .unwrap();
    let before = format!("{p:?}");
    assert_eq!(
        CoordinateControlFacts::from_source(&observation, &p, id).unwrap_err(),
        CoordinateControlError::Unsupported
    );
    assert_eq!(format!("{p:?}"), before);
}

#[test]
fn authority_only_observation_cannot_replace_actual_native_admitted_record() {
    let (facts, mut p, id, old) = admitted();
    let next = control::frame_observation(
        old.coordinate().snapshot().basis,
        id,
        2,
        101,
        old.source().facts().render,
    );
    let pair = match next.coordinate().snapshot().evidence {
        OriginalNativePresentationEvidence::SuppliedPair(pair) => pair,
        _ => unreachable!(),
    };
    p.authority_mut()
        .observe_coordinate(7, pair, next.coordinate().coordinate())
        .unwrap();
    let before = format!("{p:?}");
    assert!(CoordinateControlFacts::from_source(&next, &p, id).is_err());
    assert!(facts.validate(&p, id).is_err());
    assert_eq!(format!("{p:?}"), before);
}
