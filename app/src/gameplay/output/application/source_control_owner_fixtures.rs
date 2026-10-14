//! Actual static owner and mixer reports; software DI is not hardware timing proof.
use super::*;
use super::{fixtures as memory, source_playback_owner_fixtures as source};
use crate::gameplay::output::ports::SourcePlaybackIdentity;
use beatkernel::audio::{
    command_queue, AudioFormat, AudioLimits, MixerConfig, PcmLimits, SampleBank,
};
use beatkernel::time::{ClockDomainId, Timestamp};
use std::{cell::RefCell, rc::Rc};

pub(crate) fn low_rate_fixture() -> (
    GameplayOutputOwner<source::Backend>,
    NativeAudioPresentation,
    Rc<source::Io>,
    beatkernel::audio::CommandProducer,
    SourcePlaybackIdentity,
) {
    let format = AudioFormat::new(3, 1).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
            AudioLimits::new(8, 2, 8, 64, 8).unwrap(),
        ),
        SampleBank::new(format, PcmLimits::new(256, 512, 1).unwrap()).unwrap(),
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 2]).unwrap();
    mixer.enable_source_playback_history(8).unwrap();
    let basis = mixer.output_frame_basis();
    let report = mixer.render(&mut [0.0; 6]).unwrap();
    let trace = Rc::new(RefCell::new(memory::Trace::default()));
    let output = memory::Output {
        mixer: Some(mixer),
        epoch: 7,
        basis,
        report: Some(report),
        quiet: 0,
        retired: false,
        trace: trace.clone(),
    };
    let io = Rc::new(source::Io::default());
    (
        GameplayOutputOwner::new(
            source::Backend {
                inner: memory::Backend { trace },
                io: io.clone(),
            },
            output,
        ),
        source::presentation(basis, None),
        io,
        producer,
        SourcePlaybackIdentity::new(3, 2, 2).unwrap(),
    )
}

pub(crate) fn frame_observation(
    basis: OutputFrameBasis,
    id: SourcePlaybackIdentity,
    frame: u64,
    host: i64,
    report: Option<RenderReport>,
) -> NativePlaybackObservation<source::Evidence> {
    let coordinate = crate::native_output_coordinate::CoordinateNativeAudioSnapshot::new(
        crate::native_audio_presentation::NativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: source::point(1, host),
            }),
        },
        crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap();
    NativePlaybackObservation::new(
        coordinate,
        source::packet(
            basis,
            7,
            id,
            source::query(basis.start_physical_frame() + frame, basis.sample_rate()),
            report,
        ),
    )
    .unwrap()
}

#[test]
fn control_acquires_one_compound_packet_and_preserves_its_original_report() {
    let (mut owner, mut p, io, _producer, id) = low_rate_fixture();
    let basis = owner.current().unwrap().basis;
    let report = owner.current().unwrap().report;
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(frame_observation(basis, id, 1, 100, report))));
    let facts = owner.observe_native_control(&mut p, id).unwrap().unwrap();
    assert_eq!(facts.identity(), id);
    assert_eq!(facts.source().render, report);
    assert_eq!(facts.source().revision, 2);
    assert_eq!(facts.source().pulled_source_end, 8);
    assert_eq!(facts.source().submitted_source_end, 8);
    assert_eq!(owner.render_report(), report);
    assert_eq!(io.observed_calls.get(), 1);
    assert_eq!(io.query_calls.get(), 0);
    assert_eq!(io.report_calls.get(), 0);
    assert_eq!(io.legacy_calls.get(), 0);
}

#[test]
fn unavailable_control_packet_preserves_prior_native_state_and_cache() {
    let (mut owner, mut p, io, _producer, id) = low_rate_fixture();
    let basis = owner.current().unwrap().basis;
    let report = owner.current().unwrap().report;
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(frame_observation(basis, id, 1, 100, report))));
    owner.observe_native_control(&mut p, id).unwrap().unwrap();
    let before = format!("{p:?}");
    assert!(owner.observe_native_control(&mut p, id).unwrap().is_none());
    assert_eq!(format!("{p:?}"), before);
    assert_eq!(owner.render_report(), report);
    assert_eq!(io.observed_calls.get(), 2);
    assert_eq!(io.report_calls.get(), 0);
}

#[test]
fn refused_coordinate_progress_preserves_cache_and_native_owner() {
    let (mut owner, mut p, io, _producer, id) = low_rate_fixture();
    let basis = owner.current().unwrap().basis;
    let report = owner.current().unwrap().report;
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(frame_observation(basis, id, 2, 101, report))));
    owner.observe_native_control(&mut p, id).unwrap().unwrap();
    let before = format!("{p:?}");
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(frame_observation(basis, id, 1, 102, None))));
    assert!(owner.observe_native_control(&mut p, id).is_err());
    assert_eq!(format!("{p:?}"), before);
    assert_eq!(owner.render_report(), report);
    assert_eq!(io.observed_calls.get(), 2);
    assert_eq!(io.report_calls.get(), 0);
}

#[test]
fn source_identity_and_live_epoch_mismatches_refuse_before_admission() {
    for change_epoch in [false, true] {
        let (mut owner, mut p, io, _producer, id) = low_rate_fixture();
        let basis = owner.current().unwrap().basis;
        let before = format!("{p:?}");
        if change_epoch {
            io.mutate_observe_identity.set(true);
        }
        let supplied = if change_epoch {
            id
        } else {
            SourcePlaybackIdentity::new(3, 1, 1).unwrap()
        };
        io.observations
            .borrow_mut()
            .push_back(Ok(Some(frame_observation(
                basis,
                supplied,
                1,
                100,
                owner.current().unwrap().report,
            ))));
        assert!(owner.observe_native_control(&mut p, id).is_err());
        assert_eq!(format!("{p:?}"), before);
        assert_eq!(owner.render_report(), None);
        assert_eq!(io.observed_calls.get(), 1);
        assert_eq!(io.report_calls.get(), 0);
    }
}

#[test]
fn wrong_coordinate_descriptor_refuses_without_scalar_fallback() {
    let (mut owner, _p, io, _producer, id) = low_rate_fixture();
    let basis = owner.current().unwrap().basis;
    let mut p = source::presentation(basis, Some(12));
    let before = format!("{p:?}");
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(frame_observation(
            basis,
            id,
            1,
            100,
            owner.current().unwrap().report,
        ))));
    assert!(owner.observe_native_control(&mut p, id).is_err());
    assert_eq!(format!("{p:?}"), before);
    assert_eq!(owner.render_report(), None);
    assert_eq!(io.observed_calls.get(), 1);
    assert_eq!(io.legacy_calls.get(), 0);
    assert_eq!(io.report_calls.get(), 0);
}

#[test]
fn missing_same_publication_report_is_not_replaced_by_independent_output_report() {
    let (mut owner, mut p, io, _producer, id) = low_rate_fixture();
    let basis = owner.current().unwrap().basis;
    assert!(owner.current().unwrap().report.is_some());
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(frame_observation(basis, id, 1, 100, None))));
    let facts = owner.observe_native_control(&mut p, id).unwrap().unwrap();
    assert_eq!(facts.source().render, None);
    assert_eq!(owner.render_report(), None);
    assert_eq!(io.report_calls.get(), 0);
}

#[test]
fn existing_scalar_observe_remains_compatible_without_control_acquisition() {
    let (mut owner, _p, io, _producer, _id) = low_rate_fixture();
    let basis = owner.current().unwrap().basis;
    let mut estimator = beatkernel::time::presentation::PresentationEstimator::new(
        memory::settings(),
        basis.point_at_stream_frame(0).unwrap(),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    estimator
        .rebind_output(
            owner.current().unwrap().epoch,
            basis.point_at_stream_frame(0).unwrap(),
            basis.point_at_stream_frame(0).unwrap(),
            Timestamp::ZERO,
        )
        .unwrap();
    owner.observe(&mut estimator).unwrap();
    assert_eq!(owner.render_report(), owner.current().unwrap().report);
    assert_eq!(io.legacy_calls.get(), 1);
    assert_eq!(io.observed_calls.get(), 0);
    assert_eq!(io.report_calls.get(), 1);
}

fn counter_observation(
    basis: OutputFrameBasis,
    id: SourcePlaybackIdentity,
    tick: u64,
    host: i64,
    qpc: u64,
    report: Option<RenderReport>,
) -> NativePlaybackObservation<source::Evidence> {
    use beatkernel_platform::audio::{
        AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
        StreamCounters,
    };
    let frequency = u64::from(basis.sample_rate());
    let coordinate = crate::native_output_coordinate::CoordinateNativeAudioSnapshot::new(
        crate::native_audio_presentation::NativeAudioSnapshot {
            epoch: 7,
            basis,
            evidence: OriginalNativePresentationEvidence::Wasapi {
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters::default(),
                    render: None,
                    clock: Some(AudioClockSnapshot {
                        position: tick,
                        frequency,
                        qpc_100ns: qpc,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(source::point(1, host)),
                        mapping_quality: beatkernel::time::ClockMappingQuality::Unknown,
                    }),
                },
                basis: Some(basis),
            },
        },
        crate::native_output_coordinate::NativeOutputCoordinate::Counter {
            position: tick,
            frequency,
        },
    )
    .unwrap();
    NativePlaybackObservation::new(
        coordinate,
        source::packet(
            basis,
            7,
            id,
            source::query(basis.start_physical_frame() + tick, basis.sample_rate()),
            report,
        ),
    )
    .unwrap()
}

#[test]
fn source_control_duplicates_wait_without_refreshing_accepted_frame_or_counter_age() {
    for counter in [false, true] {
        let (mut owner, mut p, io, _producer, id) = low_rate_fixture();
        let basis = owner.current().unwrap().basis;
        let report = owner.current().unwrap().report;
        if counter {
            p = source::presentation(basis, Some(u64::from(basis.sample_rate())));
        }
        let observation = |units, host, qpc| {
            if counter {
                counter_observation(basis, id, units, host, qpc, report)
            } else {
                frame_observation(basis, id, units, host, report)
            }
        };
        io.observations
            .borrow_mut()
            .push_back(Ok(Some(observation(2, 100, 1))));
        let first = owner.observe_native_control(&mut p, id).unwrap().unwrap();
        let accepted = format!("{p:?}");
        io.observations
            .borrow_mut()
            .push_back(Ok(Some(observation(2, 100, 1))));
        let repeated = owner.observe_native_control(&mut p, id).unwrap().unwrap();
        repeated.validate(&p, id).unwrap();
        assert_eq!(repeated.pair(), first.pair());
        assert_eq!(format!("{p:?}"), accepted);
        assert_eq!(io.observed_calls.get(), 2);
        if counter {
            io.observations
                .borrow_mut()
                .push_back(Ok(Some(observation(2, 100, 2))));
            assert!(owner.observe_native_control(&mut p, id).is_err());
            assert_eq!(format!("{p:?}"), accepted);
            assert_eq!(owner.render_report(), report);
            assert_eq!(io.observed_calls.get(), 3);
        }
        let duplicate_calls = io.observed_calls.get();
        let duplicates: &[(i64, u64)] = if counter {
            &[(101, 3), (2_000_000, 4)]
        } else {
            &[(101, 2), (2_000_000, 3)]
        };
        for (index, &(host, qpc)) in duplicates.iter().enumerate() {
            let candidate = observation(2, host, qpc);
            assert!(
                crate::native_coordinate_control::CoordinateControlFacts::from_source(
                    &candidate, &p, id
                )
                .is_err()
            );
            io.observations.borrow_mut().push_back(Ok(Some(candidate)));
            assert!(owner.observe_native_control(&mut p, id).unwrap().is_none());
            assert_eq!(format!("{p:?}"), accepted);
            assert_eq!(owner.render_report(), report);
            assert_eq!(io.observed_calls.get(), duplicate_calls + 1 + index);
        }
        assert!(!p
            .authority()
            .has_fresh_observation(source::point(1, 2_000_000))
            .unwrap());
        let calls = io.observed_calls.get();
        io.observations
            .borrow_mut()
            .push_back(Ok(Some(observation(3, 2_000_001, 5))));
        let progressed = owner.observe_native_control(&mut p, id).unwrap().unwrap();
        progressed.validate(&p, id).unwrap();
        assert_eq!(progressed.pair().target, source::point(1, 2_000_001));
        assert!(first.validate(&p, id).is_err());
        assert_eq!(io.observed_calls.get(), calls + 1);
        let progressed_state = format!("{p:?}");
        io.observations
            .borrow_mut()
            .push_back(Ok(Some(observation(2, 2_000_002, 6))));
        assert!(owner.observe_native_control(&mut p, id).is_err());
        assert_eq!(format!("{p:?}"), progressed_state);
        assert_eq!(io.observed_calls.get(), calls + 2);
        assert_eq!(io.query_calls.get(), 0);
        assert_eq!(io.report_calls.get(), 0);
        assert_eq!(io.legacy_calls.get(), 0);
    }
}

#[test]
fn ordinary_source_duplicate_returns_packet_with_new_real_render_report() {
    let (mut owner, mut p, io, _producer, id) = low_rate_fixture();
    let basis = owner.current().unwrap().basis;
    let initial = owner.current().unwrap().report;
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(frame_observation(basis, id, 2, 100, initial))));
    owner.observe_native_playback(&mut p, id).unwrap().unwrap();
    let accepted = format!("{p:?}");
    let mixer = owner.current_mut().unwrap().mixer.as_mut().unwrap();
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    let pulled = mixer.frame_cursor();
    assert_ne!(Some(report), initial);
    for (index, host) in [100, 100, 101].into_iter().enumerate() {
        let base = frame_observation(basis, id, 2, host, None);
        let mut facts = base.source().facts();
        facts.render = Some(report);
        facts.pulled_source_end = pulled;
        facts.submitted_source_end = pulled;
        facts.revision = 4;
        let packet = crate::gameplay::output::ports::NativeSourcePlaybackQuery::new(
            7,
            basis,
            id,
            base.source().query(),
            facts,
            Some(source::Evidence {
                words: vec![19, 31],
            }),
        )
        .unwrap();
        io.observations.borrow_mut().push_back(Ok(Some(
            NativePlaybackObservation::new(base.coordinate(), packet).unwrap(),
        )));
        if index == 0 {
            let returned = owner.observe_native_control(&mut p, id).unwrap().unwrap();
            returned.validate(&p, id).unwrap();
            assert_eq!(returned.source().render, Some(report));
            assert_eq!(returned.source().revision, 4);
        } else {
            let returned = owner.observe_native_playback(&mut p, id).unwrap().unwrap();
            assert_eq!(returned.source().facts().render, Some(report));
            assert_eq!(returned.source().facts().revision, 4);
        }
        assert_eq!(owner.render_report(), Some(report));
        assert_eq!(format!("{p:?}"), accepted);
        assert_eq!(io.observed_calls.get(), index + 2);
    }
    assert_eq!(io.query_calls.get(), 0);
    assert_eq!(io.report_calls.get(), 0);
    assert_eq!(io.legacy_calls.get(), 0);
}

// This portable analogue uses the actual complete state and Owner request path.
// The strict classifier models only Native(CoreAudioError::RecoveryUnavailable).
use crate::gameplay::output::application::replacement::coordinate_source::test_support as complete;
type NarrowHistoryOwner = GameplayOutputOwner<
    complete::Effects,
    beatkernel_platform::audio::NativeOutputState,
    OutputFrameBasis,
    complete::Ticket,
>;
fn narrow_history_request_setup() -> (
    complete::Rig,
    NarrowHistoryOwner,
    complete::Effects,
    NativePlaybackObservation<complete::Evidence>,
) {
    let mut rig = complete::Rig::with_frequency(false, None);
    let (effects, output) = rig.take_live_native_owner();
    let mut trace = effects.clone();
    trace.enable_history_service();
    trace.classify_only_recovery_unavailable();
    let mut owner = NarrowHistoryOwner::new_with_ticket(effects, output);
    let identity = rig.identity;
    let original = owner
        .observe_native_playback(rig.presentation_mut(), identity)
        .unwrap()
        .unwrap();
    trace.conditional_statuses([SourceRetirementStatus::Pending]);
    assert!(owner
        .queue(complete::Request::normal(None), 100_000_000)
        .is_ok());
    assert!(matches!(
        owner
            .publish_paused_coordinate_source_audio(
                rig.context(),
                complete::now(),
                identity,
                false,
                |_| Ok(()),
            )
            .unwrap(),
        CoordinateSourceOwnerPublication::Pending
    ));
    (rig, owner, trace, original)
}
fn conditional_probe_count(trace: &complete::Effects) -> usize {
    trace
        .calls()
        .iter()
        .filter(|call| **call == "conditional-poll")
        .count()
}

#[test]
fn source_control_owner_history_request_authorization_after_final_pending_probe_defers_only_classified_recovery_unavailable(
) {
    let (rig, mut owner, mut trace, original) = narrow_history_request_setup();
    trace.conditional_statuses([SourceRetirementStatus::Pending; 2]);
    trace.history_request_faults([complete::HistoryFault::AuthorizeRecoveryUnavailable]);
    let probes = conditional_probe_count(&trace);
    let before = format!("{:?}", rig.presentation());
    let report = owner.render_report();
    assert!(owner
        .request_conditional_source_history_retirement_with_source_pin(
            rig.presentation(),
            &original.source().query(),
            rig.identity,
            Some(rig.identity.initial_physical()),
        )
        .unwrap()
        .is_none());
    assert_eq!(
        conditional_probe_count(&trace),
        probes + 3,
        "two Pending probes precede request IO; one fresh probe confirms authorization"
    );
    assert_eq!(trace.history_queries().len(), 1);
    assert_eq!(
        trace.history_requests(),
        [(
            complete::OLD_EPOCH,
            rig.pin,
            rig.identity,
            rig.identity.initial_physical()
        )]
    );
    assert_eq!(format!("{:?}", rig.presentation()), before);
    assert_eq!(owner.render_report(), report);
    assert!(owner.current().is_none());
    assert!(owner.output_clock_suspended());
    assert_eq!(owner.last_issued_epoch(), complete::OLD_EPOCH);
    assert!(
        !trace.calls().contains(&"retire"),
        "authorization alone grants no native Stop or drain"
    );
}

#[test]
fn source_control_owner_history_request_recovery_unavailable_requires_confirmed_authorization() {
    for status in [
        SourceRetirementStatus::Pending,
        SourceRetirementStatus::Refused,
        SourceRetirementStatus::Cancelled,
    ] {
        let (rig, mut owner, mut trace, original) = narrow_history_request_setup();
        trace.conditional_statuses([
            SourceRetirementStatus::Pending,
            SourceRetirementStatus::Pending,
            status,
        ]);
        trace.history_request_faults([complete::HistoryFault::RecoveryUnavailable]);
        let probes = conditional_probe_count(&trace);
        let before = complete::owner_facts(owner.current().unwrap().source_state());
        let presentation = format!("{:?}", rig.presentation());
        let error = match owner.request_conditional_source_history_retirement_with_source_pin(
            rig.presentation(),
            &original.source().query(),
            rig.identity,
            Some(rig.identity.initial_physical()),
        ) {
            Err(error) => error,
            Ok(_) => panic!("{status:?} cannot hide the original request failure"),
        };
        let failure = error
            .downcast_ref::<ReplacementFailure<std::io::Error>>()
            .unwrap();
        assert!(
            matches!(&failure.cause, ReplacementCause::Backend { phase: ReplacementPhase::Retire, error }
            if error.get_ref().is_some_and(|native| native.is::<complete::RecoveryUnavailable>()))
        );
        assert_eq!(conditional_probe_count(&trace), probes + 3);
        assert_eq!(trace.history_queries().len(), 1);
        assert_eq!(trace.history_requests().len(), 1);
        assert_eq!(
            complete::owner_facts(owner.current().unwrap().source_state()),
            before
        );
        assert_eq!(format!("{:?}", rig.presentation()), presentation);
        assert!(!owner.output_clock_suspended());
    }
}

#[test]
fn source_control_owner_history_request_unrelated_native_and_typed_errors_never_consume_later_authorization(
) {
    for (fault, expected) in [
        (
            complete::HistoryFault::AuthorizeOtherNative,
            "original unrelated native failure",
        ),
        (
            complete::HistoryFault::AuthorizeTyped,
            "original typed packet refusal",
        ),
    ] {
        let (rig, mut owner, mut trace, original) = narrow_history_request_setup();
        trace.conditional_statuses([SourceRetirementStatus::Pending; 2]);
        trace.history_request_faults([fault]);
        let probes = conditional_probe_count(&trace);
        let before = complete::owner_facts(owner.current().unwrap().source_state());
        let presentation = format!("{:?}", rig.presentation());
        let error = match owner.request_conditional_source_history_retirement_with_source_pin(
            rig.presentation(),
            &original.source().query(),
            rig.identity,
            Some(rig.identity.initial_physical()),
        ) {
            Err(error) => error,
            Ok(_) => panic!(
                "an unclassified error must remain visible even after callback authorization"
            ),
        };
        let failure = error
            .downcast_ref::<ReplacementFailure<std::io::Error>>()
            .unwrap();
        assert!(
            matches!(&failure.cause, ReplacementCause::Backend { phase: ReplacementPhase::Retire, error }
            if error.to_string() == expected)
        );
        assert_eq!(
            conditional_probe_count(&trace),
            probes + 2,
            "unclassified errors never trigger the authorization re-probe"
        );
        assert_eq!(trace.history_requests().len(), 1);
        assert_eq!(
            complete::owner_facts(owner.current().unwrap().source_state()),
            before
        );
        assert_eq!(format!("{:?}", rig.presentation()), presentation);
        assert!(owner.current().is_some());
        assert!(!owner.output_clock_suspended());
    }
}
