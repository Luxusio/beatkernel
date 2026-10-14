//! Real Source Owner services over the shared complete-owner/native-effects rig.
//! Scripted native packets exercise application policy, not Windows execution.
use super::*;
use crate::gameplay::output::{
    application::replacement::coordinate_source::test_support as support,
    ports::SourceRetirementStatus,
};
use beatkernel::audio::{PracticeRegion, SourcePlaybackState};
use beatkernel::judge::JudgeEngine;
use beatkernel::time::Timestamp;
use beatkernel_platform::audio::NativeOutputState;
use std::{cell::Cell, error::Error, io};
use support::*;

type Owner = GameplayOutputOwner<Effects, NativeOutputState, OutputFrameBasis, Ticket>;

fn setup(frequency: Option<u64>, partial: bool) -> (Rig, Owner, Effects) {
    let mut rig = Rig::with_frequency(partial, frequency);
    let (effects, output) = rig.take_live_native_owner();
    let mut trace = effects.clone();
    let mut owner = Owner::new_with_ticket(effects, output);
    trace.conditional_statuses([SourceRetirementStatus::Pending]);
    assert!(owner
        .queue(Request::normal(frequency.map(|_| 10_000_000)), 100_000_000)
        .is_ok());
    let identity = rig.source_identity();
    assert!(matches!(
        owner
            .publish_paused_coordinate_source_audio(rig.context(), now(), identity, false, |_| {
                Ok(())
            })
            .unwrap(),
        CoordinateSourceOwnerPublication::Pending
    ));
    assert_eq!(owner.state(), ReplacementState::ConditionalRetirement);
    assert!(owner.current().is_some());
    assert!(owner.has_work());
    assert!(owner.replacement_pending());
    assert!(!owner.output_clock_suspended());
    (rig, owner, trace)
}

fn count(trace: &Effects, name: &str) -> usize {
    trace.calls().iter().filter(|call| **call == name).count()
}

fn error<T>(result: Result<T, Box<dyn Error>>) -> Box<dyn Error> {
    match result {
        Err(error) => error,
        Ok(_) => panic!("the original operation failure must remain visible"),
    }
}

fn history_setup(
    frequency: Option<u64>,
) -> (Rig, Owner, Effects, NativePlaybackObservation<Evidence>) {
    let (mut rig, mut owner, mut trace) = setup(frequency, false);
    let identity = rig.source_identity();
    let original = owner
        .observe_native_playback(rig.presentation_mut(), identity)
        .unwrap()
        .unwrap();
    trace.enable_history_service();
    (rig, owner, trace, original)
}

fn settle<A>(
    rig: &mut Rig,
    owner: &mut Owner,
    mut stage: impl FnMut(&Output) -> Result<A, Box<dyn Error>>,
) -> Result<CoordinateSourceOwnerPublication<A>, Box<dyn Error>> {
    let identity = rig.source_identity();
    for _ in 0..12 {
        let outcome = owner.publish_paused_coordinate_source_audio(
            rig.context(),
            now(),
            identity,
            false,
            |output| stage(output),
        )?;
        if !matches!(outcome, CoordinateSourceOwnerPublication::Pending) {
            return Ok(outcome);
        }
    }
    panic!("actual progressing Source packets must settle the pending transaction")
}

#[test]
fn retained_source_observation_pending_uses_one_original_packet_for_both_coordinate_classes() {
    for frequency in [None, Some(OLD_FREQUENCY), Some(10_000_000)] {
        for control in [false, true] {
            let (mut rig, mut owner, mut trace) = setup(frequency, true);
            trace.conditional_statuses([SourceRetirementStatus::Pending]);
            let identity = rig.source_identity();
            let native_id = owner.current().unwrap().native_id;
            let merger = rig.inputs.preparation_state().unwrap();
            let source = source_history(owner.current().unwrap().source_state());
            let old = rig.presentation().latest_record().unwrap().pair();
            if control {
                let facts = owner
                    .observe_conditional_source_native_control(rig.presentation_mut(), identity)
                    .unwrap()
                    .unwrap();
                assert_eq!(facts.epoch(), OLD_EPOCH);
                assert_eq!(facts.identity(), identity);
                assert_eq!(facts.basis(), descriptor(rig.pin, frequency));
                assert_eq!(facts.source().render, owner.render_report());
                assert_eq!(
                    facts.pair(),
                    rig.presentation().latest_record().unwrap().pair()
                );
                facts.validate(rig.presentation(), identity).unwrap();
            } else {
                let packet = owner
                    .observe_conditional_source_native_playback(rig.presentation_mut(), identity)
                    .unwrap()
                    .unwrap();
                assert_eq!(packet.source().epoch(), OLD_EPOCH);
                assert_eq!(packet.source().identity(), identity);
                assert_eq!(packet.source().basis(), rig.pin);
                assert_eq!(
                    packet
                        .source()
                        .native_evidence()
                        .unwrap()
                        .original_native_words[0],
                    native_id
                );
                assert_eq!(packet.source().facts().render, owner.render_report());
                assert!(packet
                    .source()
                    .query()
                    .compare_interpolated_time(packet.coordinate().source_query().unwrap())
                    .is_eq());
                match packet.coordinate().coordinate() {
                    crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame {
                        frame,
                    } => {
                        assert_eq!(frequency, None);
                        assert_eq!(frame, 12);
                    }
                    crate::native_output_coordinate::NativeOutputCoordinate::Counter {
                        position,
                        frequency: actual,
                    } => {
                        assert_eq!(Some(actual), frequency);
                        assert_eq!(position, 12 * actual / u64::from(SOURCE_RATE));
                    }
                }
            }
            assert_eq!(count(&trace, "packet"), 1);
            assert_eq!(count(&trace, "conditional-poll"), 2);
            assert_eq!(owner.current().unwrap().native_id, native_id);
            assert_eq!(
                source_history(owner.current().unwrap().source_state()),
                source
            );
            assert!(
                rig.presentation()
                    .latest_record()
                    .unwrap()
                    .pair()
                    .source
                    .timestamp
                    > old.source.timestamp
            );
            assert!(rig.inputs.matches_preparation(&merger));
            assert!(!owner.output_clock_suspended());
            assert_eq!(count(&trace, "retire"), 0);
        }
    }
}

#[test]
fn retained_source_authorization_is_fresh_before_io_and_sticky_afterward() {
    for frequency in [None, Some(OLD_FREQUENCY)] {
        let (mut rig, mut owner, mut trace, original) = history_setup(frequency);
        trace.conditional_statuses([
            SourceRetirementStatus::Authorized,
            SourceRetirementStatus::Pending,
        ]);
        trace.observation_faults([Fault::Observe]);
        trace.history_query_faults([HistoryFault::Io]);
        trace.history_request_faults([HistoryFault::Io]);
        let identity = rig.source_identity();
        let before = format!("{:?}", rig.presentation());
        let report = owner.render_report();
        let packets = count(&trace, "packet");
        let probes = count(&trace, "conditional-poll");
        assert!(owner
            .observe_conditional_source_native_playback(rig.presentation_mut(), identity)
            .unwrap()
            .is_none());
        assert!(owner
            .observe_conditional_source_native_control(rig.presentation_mut(), identity)
            .unwrap()
            .is_none());
        assert!(owner
            .conditional_source_playback_at(
                rig.presentation(),
                &original.source().query(),
                identity
            )
            .unwrap()
            .is_none());
        assert!(owner
            .request_conditional_source_history_retirement_with_source_pin(
                rig.presentation(),
                &original.source().query(),
                identity,
                Some(identity.initial_physical()),
            )
            .unwrap()
            .is_none());
        assert_eq!(count(&trace, "conditional-poll"), probes + 1);
        assert_eq!(count(&trace, "packet"), packets);
        assert!(trace.history_queries().is_empty());
        assert!(trace.history_requests().is_empty());
        assert_eq!(count(&trace, "retire"), 0);
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert_eq!(owner.render_report(), report);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        assert!(owner.has_work());
        assert!(rig.runtime.hold_audio_pause().is_err());
    }
}

#[test]
fn retained_source_observation_error_reprobes_actual_permission_instead_of_cached_pending() {
    for frequency in [None, Some(OLD_FREQUENCY)] {
        for control in [false, true] {
            let (mut rig, mut owner, mut trace) = setup(frequency, true);
            trace.conditional_statuses([
                SourceRetirementStatus::Pending,
                SourceRetirementStatus::Authorized,
            ]);
            trace.observation_faults([Fault::Observe]);
            let identity = rig.source_identity();
            let before = format!("{:?}", rig.presentation());
            if control {
                assert!(owner
                    .observe_conditional_source_native_control(rig.presentation_mut(), identity)
                    .unwrap()
                    .is_none());
            } else {
                assert!(owner
                    .observe_conditional_source_native_playback(rig.presentation_mut(), identity)
                    .unwrap()
                    .is_none());
            }
            assert_eq!(count(&trace, "packet"), 1);
            assert_eq!(count(&trace, "conditional-poll"), 3);
            assert_eq!(count(&trace, "retire"), 0);
            assert_eq!(format!("{:?}", rig.presentation()), before);
            assert!(owner.current().is_none());
            assert!(owner.output_clock_suspended());
        }
    }
}

#[test]
fn retained_source_genuine_observation_error_keeps_its_phase_and_live_complete_owner() {
    for status in [
        SourceRetirementStatus::Pending,
        SourceRetirementStatus::Refused,
        SourceRetirementStatus::Cancelled,
    ] {
        for control in [false, true] {
            let (mut rig, mut owner, mut trace) = setup(Some(OLD_FREQUENCY), true);
            trace.conditional_statuses([SourceRetirementStatus::Pending, status]);
            trace.observation_faults([Fault::Observe]);
            let identity = rig.source_identity();
            let native_id = owner.current().unwrap().native_id;
            let state = owner_facts(owner.current().unwrap().source_state());
            let before = format!("{:?}", rig.presentation());
            let failure =
                if control {
                    error(owner.observe_conditional_source_native_control(
                        rig.presentation_mut(),
                        identity,
                    ))
                } else {
                    error(owner.observe_conditional_source_native_playback(
                        rig.presentation_mut(),
                        identity,
                    ))
                };
            let original = failure
                .downcast_ref::<ReplacementFailure<io::Error>>()
                .unwrap();
            assert!(matches!(&original.cause,
                ReplacementCause::Backend { phase: ReplacementPhase::Observe, error }
                if error.to_string() == "original native observation failure"));
            assert_eq!(owner.current().unwrap().native_id, native_id);
            assert_eq!(owner_facts(owner.current().unwrap().source_state()), state);
            assert_eq!(format!("{:?}", rig.presentation()), before);
            assert_eq!(count(&trace, "packet"), 1);
            assert_eq!(count(&trace, "retire"), 0);
            assert!(!owner.output_clock_suspended());
        }
    }
}

#[test]
fn retained_source_initial_probe_failure_preserves_cleanup_recovery_and_exclusive_hold() {
    let (mut rig, mut owner, mut trace) = setup(Some(OLD_FREQUENCY), true);
    trace.conditional_probe_failures([true]);
    trace.retirement_failures([true]);
    trace.recovery_failures([true]);
    let identity = rig.source_identity();
    let failure =
        error(owner.observe_conditional_source_native_playback(rig.presentation_mut(), identity));
    let original = failure
        .downcast_ref::<ReplacementFailure<io::Error>>()
        .unwrap();
    assert!(matches!(&original.cause,
        ReplacementCause::Backend { phase: ReplacementPhase::Retire, error }
        if error.to_string() == "scripted native conditional probe failure"));
    assert_eq!(
        original.cleanup.as_ref().unwrap().to_string(),
        "scripted native retirement failure"
    );
    assert_eq!(
        original.recovery.as_ref().unwrap().to_string(),
        "scripted native recovery failure"
    );
    assert_eq!(count(&trace, "packet"), 0);
    assert_eq!(count(&trace, "retire"), 1);
    assert_eq!(count(&trace, "recover"), 1);
    assert_eq!(owner.state(), ReplacementState::PendingRetirement);
    assert!(owner.current().is_none());
    assert!(owner.output_clock_suspended());
    assert!(rig.runtime.hold_audio_pause().is_err());
    assert!(owner.retry_retirement().unwrap());
    assert!(owner.take_recovered_mixer().is_some());
    drop(rig.runtime.hold_audio_pause().unwrap());
}

#[test]
fn retained_source_observation_double_failure_preserves_all_four_owned_errors() {
    for control in [false, true] {
        let (mut rig, mut owner, mut trace) = setup(Some(OLD_FREQUENCY), true);
        trace.conditional_statuses([SourceRetirementStatus::Pending]);
        trace.conditional_probe_failures([false, true]);
        trace.observation_faults([Fault::Observe]);
        trace.retirement_failures([true]);
        trace.recovery_failures([true]);
        let identity = rig.source_identity();
        let failure = if control {
            error(owner.observe_conditional_source_native_control(rig.presentation_mut(), identity))
        } else {
            error(
                owner.observe_conditional_source_native_playback(rig.presentation_mut(), identity),
            )
        };
        let combined = failure.downcast_ref::<ConditionalTargetObservationFailure<io::Error, ReplacementFailure<io::Error>>>().unwrap();
        assert!(matches!(&combined.observation.cause,
            ReplacementCause::Backend { phase: ReplacementPhase::Observe, error }
            if error.to_string() == "original native observation failure"));
        assert!(matches!(&combined.probe.cause,
            ReplacementCause::Backend { phase: ReplacementPhase::Retire, error }
            if error.to_string() == "scripted native conditional probe failure"));
        assert_eq!(
            combined.probe.cleanup.as_ref().unwrap().to_string(),
            "scripted native retirement failure"
        );
        assert_eq!(
            combined.probe.recovery.as_ref().unwrap().to_string(),
            "scripted native recovery failure"
        );
        assert!(failure
            .source()
            .unwrap()
            .downcast_ref::<ReplacementFailure<io::Error>>()
            .is_some());
        for expected in [
            "original native observation failure",
            "scripted native conditional probe failure",
            "scripted native retirement failure",
            "scripted native recovery failure",
        ] {
            assert!(failure.to_string().contains(expected));
        }
        assert_eq!(count(&trace, "packet"), 1);
        assert_eq!(owner.state(), ReplacementState::PendingRetirement);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        assert!(rig.runtime.hold_audio_pause().is_err());
    }
}

#[test]
fn retained_source_valid_packet_identity_basis_epoch_and_frequency_failures_never_reprobe() {
    for fault in [
        Fault::WrongEpoch,
        Fault::WrongIdentity,
        Fault::ChangedBasis,
        Fault::ChangedAfterRead,
        Fault::WrongFrequency,
        Fault::WrongDomain,
    ] {
        let (mut rig, mut owner, mut trace) = setup(Some(OLD_FREQUENCY), false);
        trace.conditional_statuses([
            SourceRetirementStatus::Pending,
            SourceRetirementStatus::Authorized,
        ]);
        trace.observation_faults([fault]);
        let identity = rig.source_identity();
        let before = format!("{:?}", rig.presentation());
        let report = owner.render_report();
        let failure = error(
            owner.observe_conditional_source_native_playback(rig.presentation_mut(), identity),
        );
        let original = failure
            .downcast_ref::<ReplacementFailure<io::Error>>()
            .unwrap();
        assert!(
            matches!(
                original.cause,
                ReplacementCause::Policy(_) | ReplacementCause::Timing(_)
            ),
            "valid packet reaches application validation: {fault:?}: {failure}"
        );
        assert_eq!(count(&trace, "conditional-poll"), 2, "{fault:?}");
        assert_eq!(count(&trace, "packet"), 1);
        assert_eq!(count(&trace, "retire"), 0);
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert_eq!(owner.render_report(), report);
        assert!(!owner.output_clock_suspended());
    }
}

#[test]
fn retained_source_malformed_packet_is_reported_at_the_backend_constructor_boundary() {
    for fault in [Fault::TornRevision, Fault::Unsubmitted] {
        let (mut rig, mut owner, mut trace) = setup(Some(OLD_FREQUENCY), false);
        trace.conditional_statuses([
            SourceRetirementStatus::Pending,
            SourceRetirementStatus::Authorized,
        ]);
        trace.observation_faults([fault]);
        let identity = rig.source_identity();
        let before = format!("{:?}", rig.presentation());
        let failure = error(
            owner.observe_conditional_source_native_playback(rig.presentation_mut(), identity),
        );
        let original = failure
            .downcast_ref::<ReplacementFailure<io::Error>>()
            .unwrap();
        assert!(matches!(
            &original.cause,
            ReplacementCause::Backend {
                phase: ReplacementPhase::Observe,
                error,
            } if error.kind() == io::ErrorKind::InvalidData
        ));
        // Construction happens inside the backend, but its typed validation
        // failure is not native IO. Only setup and the initial live probe poll;
        // the later authorization must remain unconsumed and cannot hide it.
        assert_eq!(
            count(&trace, "conditional-poll"),
            2,
            "typed packet validation must not trigger an authorization re-probe"
        );
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert!(!owner.output_clock_suspended());
    }
}

#[test]
fn retained_source_native_io_classification_survives_static_remix_forwarding() {
    use crate::gameplay::output::{
        adapters::remix::RemixedOutputBackend,
        ports::ConditionalSourceRetirementNativeOutputBackend,
    };
    let mut rig = Rig::new(false);
    let (effects, _output) = rig.take_live_native_owner();
    let trace = effects.clone();
    let native = io::Error::other("original native IO error");
    let packet = io::Error::new(io::ErrorKind::InvalidData, "typed packet validation error");
    assert!(<Effects as ConditionalSourceRetirementNativeOutputBackend<
        NativeOutputState,
    >>::is_source_native_io_error(&effects, &native));
    assert!(!<Effects as ConditionalSourceRetirementNativeOutputBackend<
        NativeOutputState,
    >>::is_source_native_io_error(&effects, &packet));
    let remixed = RemixedOutputBackend::new(effects);
    assert!(
        <RemixedOutputBackend<Effects> as ConditionalSourceRetirementNativeOutputBackend<
            NativeOutputState,
        >>::is_source_native_io_error(&remixed, &native)
    );
    assert!(
        !<RemixedOutputBackend<Effects> as ConditionalSourceRetirementNativeOutputBackend<
            NativeOutputState,
        >>::is_source_native_io_error(&remixed, &packet)
    );
    assert!(
        trace.calls().is_empty(),
        "classification is a cold static decision"
    );
}

#[test]
fn retained_source_wrong_creation_identity_refuses_before_any_native_observation() {
    let (mut rig, mut owner, mut trace) = setup(None, false);
    trace.conditional_statuses([
        SourceRetirementStatus::Pending,
        SourceRetirementStatus::Authorized,
    ]);
    let identity =
        SourcePlaybackIdentity::new(SOURCE_RATE, rig.pin.start_physical_frame() + 1, 3).unwrap();
    let before = format!("{:?}", rig.presentation());
    let failure =
        error(owner.observe_conditional_source_native_playback(rig.presentation_mut(), identity));
    assert!(matches!(
        failure
            .downcast_ref::<ReplacementFailure<io::Error>>()
            .unwrap()
            .cause,
        ReplacementCause::Policy(_)
    ));
    assert_eq!(count(&trace, "conditional-poll"), 2);
    assert_eq!(count(&trace, "packet"), 0);
    assert_eq!(format!("{:?}", rig.presentation()), before);
    assert!(owner.current().is_some());
    assert!(!owner.output_clock_suspended());
}

#[test]
fn retained_source_history_success_keeps_original_evidence_pin_revision_and_complete_live_state() {
    for frequency in [None, Some(OLD_FREQUENCY)] {
        for source_minimum in [Some(5), Some(9), Some(u64::MAX), None] {
            let (rig, mut owner, mut trace, original) = history_setup(frequency);
            let identity = rig.source_identity();
            let query = original.source().query();
            let cover = original.source().facts().playback.physical_start();
            assert_eq!(cover, 9);
            assert_eq!(
                original.source().facts().playback.state(),
                SourcePlaybackState::Frozen
            );
            let pin = source_minimum.map_or(cover, |hint| hint.min(cover));
            let native_words = original
                .source()
                .native_evidence()
                .unwrap()
                .original_native_words;
            let state = owner_facts(owner.current().unwrap().source_state());
            let before = format!("{:?}", rig.presentation());
            let merger = rig.inputs.preparation_state().unwrap();
            let report = owner.render_report();
            trace.conditional_statuses([SourceRetirementStatus::Pending; 8]);
            for revision in 1..=2 {
                assert_eq!(
                    owner
                        .request_conditional_source_history_retirement_with_source_pin(
                            rig.presentation(),
                            &query,
                            identity,
                            source_minimum,
                        )
                        .unwrap(),
                    Some(revision)
                );
            }
            assert_eq!(trace.history_queries(), vec![native_words; 2]);
            assert_eq!(
                trace.history_requests(),
                vec![(OLD_EPOCH, rig.pin, identity, pin); 2]
            );
            assert_eq!(owner_facts(owner.current().unwrap().source_state()), state);
            assert_eq!(format!("{:?}", rig.presentation()), before);
            assert_eq!(owner.render_report(), report);
            assert!(rig.inputs.matches_preparation(&merger));
            assert!(!owner.output_clock_suspended());
            assert_eq!(count(&trace, "retire"), 0);
        }
    }
}

#[test]
fn retained_source_history_successful_query_reprobes_before_the_separate_native_request() {
    for frequency in [None, Some(OLD_FREQUENCY)] {
        let (rig, mut owner, mut trace, original) = history_setup(frequency);
        trace.conditional_statuses([
            SourceRetirementStatus::Pending,
            SourceRetirementStatus::Authorized,
        ]);
        let identity = rig.source_identity();
        let before = format!("{:?}", rig.presentation());
        assert!(owner
            .request_conditional_source_history_retirement_with_source_pin(
                rig.presentation(),
                &original.source().query(),
                identity,
                Some(identity.initial_physical()),
            )
            .unwrap()
            .is_none());
        assert_eq!(trace.history_queries().len(), 1);
        assert!(
            trace.history_requests().is_empty(),
            "query success cannot prove a later native request remains live"
        );
        assert_eq!(count(&trace, "conditional-poll"), 3);
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        assert_eq!(count(&trace, "retire"), 0);
    }
}

#[test]
fn retained_source_history_authorization_during_query_or_request_defers_only_the_old_io_error() {
    for during_query in [false, true] {
        let (mut rig, mut owner, mut trace, original) = history_setup(Some(OLD_FREQUENCY));
        trace.conditional_statuses([SourceRetirementStatus::Pending; 4]);
        if during_query {
            trace.history_query_faults([HistoryFault::Authorize]);
        } else {
            trace.history_request_faults([HistoryFault::Authorize]);
        }
        let identity = rig.source_identity();
        let before = format!("{:?}", rig.presentation());
        assert!(owner
            .request_conditional_source_history_retirement_with_source_pin(
                rig.presentation(),
                &original.source().query(),
                identity,
                Some(identity.initial_physical()),
            )
            .unwrap()
            .is_none());
        assert_eq!(trace.history_queries().len(), 1);
        assert_eq!(trace.history_requests().len(), usize::from(!during_query));
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        assert_eq!(count(&trace, "retire"), 0);
        assert!(rig.runtime.hold_audio_pause().is_err());
    }
}

#[test]
fn retained_source_history_genuine_query_and_request_errors_keep_their_original_phase() {
    for status in [
        SourceRetirementStatus::Pending,
        SourceRetirementStatus::Refused,
        SourceRetirementStatus::Cancelled,
    ] {
        for during_query in [false, true] {
            let (rig, mut owner, mut trace, original) = history_setup(Some(OLD_FREQUENCY));
            trace.conditional_statuses(if during_query {
                vec![SourceRetirementStatus::Pending, status]
            } else {
                vec![
                    SourceRetirementStatus::Pending,
                    SourceRetirementStatus::Pending,
                    status,
                ]
            });
            if during_query {
                trace.history_query_faults([HistoryFault::Io]);
            } else {
                trace.history_request_faults([HistoryFault::Io]);
            }
            let identity = rig.source_identity();
            let state = owner_facts(owner.current().unwrap().source_state());
            let before = format!("{:?}", rig.presentation());
            let failure = error(
                owner.request_conditional_source_history_retirement_with_source_pin(
                    rig.presentation(),
                    &original.source().query(),
                    identity,
                    Some(identity.initial_physical()),
                ),
            );
            match &failure
                .downcast_ref::<ReplacementFailure<io::Error>>()
                .unwrap()
                .cause
            {
                ReplacementCause::Backend { phase, error } => {
                    assert_eq!(
                        *phase,
                        if during_query {
                            ReplacementPhase::Observe
                        } else {
                            ReplacementPhase::Retire
                        }
                    );
                    assert_eq!(
                        error.to_string(),
                        if during_query {
                            "original native history query failure"
                        } else {
                            "original native history retirement failure"
                        }
                    );
                }
                _ => panic!("the original typed backend failure must survive"),
            }
            assert_eq!(trace.history_queries().len(), 1);
            assert_eq!(trace.history_requests().len(), usize::from(!during_query));
            assert_eq!(owner_facts(owner.current().unwrap().source_state()), state);
            assert_eq!(format!("{:?}", rig.presentation()), before);
            assert!(!owner.output_clock_suspended());
            assert_eq!(count(&trace, "retire"), 0);
        }
    }
}

#[test]
fn retained_source_history_double_failure_preserves_operation_probe_cleanup_and_recovery() {
    for during_query in [false, true] {
        let (rig, mut owner, mut trace, original) = history_setup(Some(OLD_FREQUENCY));
        trace.conditional_statuses([SourceRetirementStatus::Pending; 4]);
        trace.conditional_probe_failures(if during_query {
            vec![false, true]
        } else {
            vec![false, false, true]
        });
        trace.retirement_failures([true]);
        trace.recovery_failures([true]);
        if during_query {
            trace.history_query_faults([HistoryFault::Io]);
        } else {
            trace.history_request_faults([HistoryFault::Io]);
        }
        let identity = rig.source_identity();
        let before = format!("{:?}", rig.presentation());
        let failure = error(
            owner.request_conditional_source_history_retirement_with_source_pin(
                rig.presentation(),
                &original.source().query(),
                identity,
                Some(identity.initial_physical()),
            ),
        );
        let combined = failure.downcast_ref::<ConditionalTargetObservationFailure<io::Error, ReplacementFailure<io::Error>>>().unwrap();
        let original_error = if during_query {
            "original native history query failure"
        } else {
            "original native history retirement failure"
        };
        assert!(matches!(&combined.observation.cause,
            ReplacementCause::Backend { phase, error }
            if *phase == if during_query { ReplacementPhase::Observe } else { ReplacementPhase::Retire }
            && error.to_string() == original_error));
        assert!(matches!(&combined.probe.cause,
            ReplacementCause::Backend { phase: ReplacementPhase::Retire, error }
            if error.to_string() == "scripted native conditional probe failure"));
        assert_eq!(
            combined.probe.cleanup.as_ref().unwrap().to_string(),
            "scripted native retirement failure"
        );
        assert_eq!(
            combined.probe.recovery.as_ref().unwrap().to_string(),
            "scripted native recovery failure"
        );
        assert!(failure
            .source()
            .unwrap()
            .downcast_ref::<ReplacementFailure<io::Error>>()
            .is_some());
        for expected in [
            original_error,
            "scripted native conditional probe failure",
            "scripted native retirement failure",
            "scripted native recovery failure",
        ] {
            assert!(failure.to_string().contains(expected));
        }
        assert_eq!(trace.history_queries().len(), 1);
        assert_eq!(trace.history_requests().len(), usize::from(!during_query));
        assert_eq!(count(&trace, "retire"), 1);
        assert_eq!(count(&trace, "recover"), 1);
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert_eq!(owner.state(), ReplacementState::PendingRetirement);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
    }
}

#[test]
fn retained_source_history_policy_errors_never_consume_later_authorization() {
    for case in 0..4 {
        let (rig, mut owner, mut trace, original) = history_setup(Some(OLD_FREQUENCY));
        let mut identity = rig.source_identity();
        let mut source_pin = Some(identity.initial_physical());
        trace.conditional_statuses([
            SourceRetirementStatus::Pending,
            SourceRetirementStatus::Authorized,
        ]);
        match case {
            0 => trace.history_query_faults([HistoryFault::PacketIdentity]),
            1 => trace.history_query_faults([HistoryFault::PacketQuery]),
            2 => {
                identity =
                    SourcePlaybackIdentity::new(SOURCE_RATE, rig.pin.start_physical_frame() + 1, 3)
                        .unwrap()
            }
            _ => source_pin = Some(identity.initial_physical() - 1),
        }
        let before = format!("{:?}", rig.presentation());
        let failure = error(
            owner.request_conditional_source_history_retirement_with_source_pin(
                rig.presentation(),
                &original.source().query(),
                identity,
                source_pin,
            ),
        );
        assert!(matches!(
            failure
                .downcast_ref::<ReplacementFailure<io::Error>>()
                .unwrap()
                .cause,
            ReplacementCause::Policy(_)
        ));
        assert_eq!(count(&trace, "conditional-poll"), 2, "policy case {case}");
        assert_eq!(trace.history_queries().len(), usize::from(case < 2));
        assert!(trace.history_requests().is_empty());
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert!(owner.current().is_some());
        assert!(!owner.output_clock_suspended());
    }
}

#[test]
fn retained_source_direct_conditional_query_keeps_original_packet_and_read_only_state() {
    for frequency in [None, Some(OLD_FREQUENCY)] {
        let (rig, mut owner, mut trace, original) = history_setup(frequency);
        trace.conditional_statuses([SourceRetirementStatus::Pending]);
        let identity = rig.source_identity();
        let state = owner_facts(owner.current().unwrap().source_state());
        let before = format!("{:?}", rig.presentation());
        let report = owner.render_report();
        let packet = owner
            .conditional_source_playback_at(
                rig.presentation(),
                &original.source().query(),
                identity,
            )
            .unwrap()
            .unwrap();
        assert_eq!(packet.epoch(), original.source().epoch());
        assert_eq!(packet.basis(), original.source().basis());
        assert_eq!(packet.identity(), identity);
        assert_eq!(packet.facts(), original.source().facts());
        assert_eq!(
            packet.native_evidence().unwrap().original_native_words,
            original
                .source()
                .native_evidence()
                .unwrap()
                .original_native_words
        );
        assert!(packet
            .query()
            .compare_interpolated_time(original.source().query())
            .is_eq());
        assert_eq!(trace.history_queries().len(), 1);
        assert!(trace.history_requests().is_empty());
        assert_eq!(owner_facts(owner.current().unwrap().source_state()), state);
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert_eq!(owner.render_report(), report);
    }
}

#[test]
fn retained_source_direct_query_error_only_defers_confirmed_authorization() {
    for authorized in [false, true] {
        let (rig, mut owner, mut trace, original) = history_setup(Some(OLD_FREQUENCY));
        trace.conditional_statuses([SourceRetirementStatus::Pending; 2]);
        trace.history_query_faults([if authorized {
            HistoryFault::Authorize
        } else {
            HistoryFault::Io
        }]);
        let identity = rig.source_identity();
        let before = format!("{:?}", rig.presentation());
        let result = owner.conditional_source_playback_at(
            rig.presentation(),
            &original.source().query(),
            identity,
        );
        if authorized {
            assert!(result.unwrap().is_none());
            assert!(owner.current().is_none());
        } else {
            let failure = error(result);
            assert!(
                matches!(&failure.downcast_ref::<ReplacementFailure<io::Error>>().unwrap().cause,
                ReplacementCause::Backend { phase: ReplacementPhase::Observe, error }
                if error.to_string() == "original native history query failure")
            );
            assert!(owner.current().is_some());
        }
        assert_eq!(owner.output_clock_suspended(), authorized);
        assert_eq!(trace.history_queries().len(), 1);
        assert!(trace.history_requests().is_empty());
        assert_eq!(format!("{:?}", rig.presentation()), before);
    }
}

#[test]
fn retained_source_pending_refused_and_cancelled_preserve_full_live_owner_and_runtime() {
    for terminal in [
        SourceRetirementStatus::Refused,
        SourceRetirementStatus::Cancelled,
    ] {
        let (mut rig, mut owner, mut trace) = setup(Some(OLD_FREQUENCY), true);
        let state = owner_facts(owner.current().unwrap().source_state());
        let id = owner.current().unwrap().native_id;
        let before = format!("{:?}", rig.presentation());
        let pause = format!("{:?}", rig.pause);
        let merger = rig.inputs.preparation_state().unwrap();
        let judge = rig.runtime.judge() as *const JudgeEngine;
        let transport = format!("{:?}", rig.runtime.transport_mut());
        let telemetry = format!("{:?}", rig.runtime.telemetry());
        let commands = rig.runtime.admitted_audio_commands();
        trace.conditional_statuses([SourceRetirementStatus::Pending, terminal]);
        let identity = rig.source_identity();
        assert!(matches!(
            owner
                .publish_paused_coordinate_source_audio(
                    rig.context(),
                    now(),
                    identity,
                    false,
                    |_| Ok(()),
                )
                .unwrap(),
            CoordinateSourceOwnerPublication::Pending
        ));
        assert_eq!(owner_facts(owner.current().unwrap().source_state()), state);
        assert!(rig.runtime.hold_audio_pause().is_err());
        assert!(matches!(
            owner
                .publish_paused_coordinate_source_audio(
                    rig.context(),
                    now(),
                    identity,
                    false,
                    |_| Ok(()),
                )
                .unwrap(),
            CoordinateSourceOwnerPublication::LiveRefused(_)
        ));
        assert_eq!(owner.current().unwrap().native_id, id);
        assert_eq!(owner_facts(owner.current().unwrap().source_state()), state);
        assert_eq!(owner.last_issued_epoch(), OLD_EPOCH);
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert_eq!(format!("{:?}", rig.pause), pause);
        assert!(rig.inputs.matches_preparation(&merger));
        assert_eq!(rig.runtime.judge() as *const JudgeEngine, judge);
        assert_eq!(format!("{:?}", rig.runtime.transport_mut()), transport);
        assert_eq!(format!("{:?}", rig.runtime.telemetry()), telemetry);
        assert_eq!(rig.runtime.admitted_audio_commands(), commands);
        assert_eq!(count(&trace, "retire"), 0);
        assert_eq!(count(&trace, "open"), 0);
        assert_eq!(count(&trace, "packet"), 0);
        assert!(!owner.has_work());
        assert!(!owner.output_clock_suspended());
        drop(rig.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn retained_source_owner_publication_preserves_original_pin_runtime_history_and_producer() {
    for (old_frequency, new_frequency) in [
        (None, None),
        (Some(OLD_FREQUENCY), Some(10_000_000)),
        (Some(10_000_000), Some(OLD_FREQUENCY)),
    ] {
        let mut rig = Rig::with_frequency(true, old_frequency);
        let (effects, output) = rig.take_live_native_owner();
        let trace = effects.clone();
        let history = source_history(output.source_state());
        let first_unsent = output.source_state().output_frame_basis();
        let pin = rig.pin;
        let frozen = output.source_state().mixer().playback_frame_cursor();
        let judge = rig.runtime.judge() as *const JudgeEngine;
        let transport = format!("{:?}", rig.runtime.transport_mut());
        let telemetry = format!("{:?}", rig.runtime.telemetry());
        let commands = rig.runtime.admitted_audio_commands();
        let generation = rig.practice_generation();
        let merger = rig.inputs.preparation_state().unwrap();
        let frontiers = preserved_frontiers(rig.presentation());
        let identity = rig.source_identity();
        let mut owner = Owner::new_with_ticket(effects, output);
        let mut request = Request::normal(new_frequency);
        request.period = 4;
        request.buffer = 8;
        assert!(owner.queue(request, 100_000_000).is_ok());
        let stages = Cell::new(0);
        let staged = settle(&mut rig, &mut owner, |output| {
            stages.set(stages.get() + 1);
            assert_eq!(output.basis, first_unsent);
            assert_eq!(
                output.source_state().mixer().playback_frame_cursor(),
                frozen
            );
            assert_eq!(source_history(output.source_state()).spans, history.spans);
            Ok((output.native_id, trace.calls()))
        })
        .unwrap();
        let CoordinateSourceOwnerPublication::Published((native_id, staged_calls)) = staged else {
            panic!("supported Source output must publish");
        };
        assert_eq!(stages.get(), 1);
        assert_eq!(
            trace.calls(),
            staged_calls,
            "borrowed publication adds no native IO"
        );
        assert_eq!(owner.current().unwrap().native_id, native_id);
        assert_eq!(owner.current().unwrap().basis, first_unsent);
        assert_ne!(first_unsent, pin);
        assert_eq!(rig.presentation().practice_mixer_basis(), Some(pin));
        assert_eq!(rig.presentation().basis(), Some(first_unsent));
        assert_eq!(
            rig.presentation().authority().coordinate_basis(),
            Some(descriptor(first_unsent, new_frequency))
        );
        assert_eq!(rig.presentation().authority().epoch().id, OLD_EPOCH + 1);
        assert_eq!(rig.pause.coordinate_source_identity(), Some(identity));
        assert_eq!(rig.pause.phase(), crate::playback_pause::PausePhase::Paused);
        assert_eq!(rig.pause.epoch(), OLD_EPOCH + 1);
        assert_eq!(preserved_frontiers(rig.presentation()), frontiers);
        assert!(rig.inputs.matches_preparation(&merger));
        assert_eq!(rig.runtime.judge() as *const JudgeEngine, judge);
        assert_eq!(format!("{:?}", rig.runtime.transport_mut()), transport);
        assert_eq!(format!("{:?}", rig.runtime.telemetry()), telemetry);
        assert_eq!(rig.runtime.admitted_audio_commands(), commands);
        assert_eq!(rig.practice_generation(), generation);
        assert_eq!(
            owner
                .current()
                .unwrap()
                .source_state()
                .mixer()
                .applied_start_frame(),
            Some(2)
        );
        assert!(!owner.has_work());
        assert!(!owner.output_clock_suspended());
        drop(rig.runtime.hold_audio_pause().unwrap());
        // The original producer still reaches the retained Mixer and PCM bank.
        rig.runtime.request_audio_pause(false);
        let state = owner.current_mut().unwrap().state.as_mut().unwrap();
        let report = state.render_pending(4).unwrap();
        assert!(!report.paused);
        assert_eq!(report.playback_frames, 4);
        assert!(state.pending_samples().iter().all(|sample| *sample == 0.25));
        rig.practice
            .try_request_next(
                41,
                generation,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(80_000_000), true)
                    .unwrap(),
            )
            .unwrap();
    }
}

#[test]
fn retained_source_owner_capability_refusal_preserves_ready_owner_and_hold_atomically() {
    let (mut rig, mut owner, trace) = setup(Some(OLD_FREQUENCY), true);
    let before = (
        format!("{:?}", rig.presentation()),
        format!("{:?}", rig.pause),
        format!("{:?}", rig.config),
    );
    let merger = rig.inputs.preparation_state().unwrap();
    let stages = Cell::new(0);
    let mut staged = None;
    let failure = error(settle::<()>(&mut rig, &mut owner, |output| {
        stages.set(stages.get() + 1);
        staged = Some((
            output.native_id,
            owner_facts(output.source_state()),
            trace.calls(),
        ));
        Err("actual applied Source capability refused".into())
    }));
    assert_eq!(
        failure.to_string(),
        "actual applied Source capability refused"
    );
    assert_eq!(stages.get(), 1);
    let (id, state, calls) = staged.unwrap();
    let Some(RejectedReady::Coordinate { output, .. }) = owner.rejected.as_ref() else {
        panic!("failed publication must retain its complete Ready owner");
    };
    assert_eq!(output.native_id, id);
    assert_eq!(owner_facts(output.source_state()), state);
    assert_eq!(trace.calls(), calls);
    assert_eq!(
        (
            format!("{:?}", rig.presentation()),
            format!("{:?}", rig.pause),
            format!("{:?}", rig.config)
        ),
        before
    );
    assert!(rig.inputs.matches_preparation(&merger));
    assert!(owner.current().is_none());
    assert_eq!(owner.state(), ReplacementState::Waiting);
    assert!(owner.output_clock_suspended());
    assert!(rig.runtime.hold_audio_pause().is_err());
    assert!(owner.cancel().unwrap());
    assert_eq!(owner_facts(&owner.take_recovered_mixer().unwrap()), state);
    drop(rig.runtime.hold_audio_pause().unwrap());
}

#[test]
fn retained_source_owner_explicit_retry_cannot_reopen_quarantined_native_admission() {
    let (mut rig, mut owner, trace) = setup(Some(OLD_FREQUENCY), true);
    owner
        .current_mut()
        .unwrap()
        .state
        .as_mut()
        .unwrap()
        .mark_admission_uncertain();
    let original = owner_facts(owner.current().unwrap().source_state());
    assert!(owner.cancel().unwrap());
    assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
    let calls = trace.calls();
    assert!(owner
        .queue(Request::normal(Some(OLD_FREQUENCY)), 100_000_000)
        .is_ok());
    let identity = rig.source_identity();
    let failure = error(owner.publish_paused_coordinate_source_audio(
        rig.context(),
        now(),
        identity,
        false,
        |_| Ok(()),
    ));
    assert!(failure.to_string().contains("uncertain"));
    assert_eq!(
        trace.calls(),
        calls,
        "explicit Retry must have no native effects"
    );
    assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
    assert_eq!(
        owner_facts(&owner.take_recovered_mixer().unwrap()),
        original
    );
    assert_eq!(rig.presentation().authority().epoch().id, OLD_EPOCH);
    drop(rig.runtime.hold_audio_pause().unwrap());
}

#[test]
fn retained_source_default_and_unrelated_ticket_types_keep_original_source_services() {
    fn exercise<T>(
        mut rig: Rig,
        mut owner: GameplayOutputOwner<Effects, NativeOutputState, OutputFrameBasis, T>,
        mut trace: Effects,
    ) {
        let identity = rig.source_identity();
        trace.enable_history_service();
        let control = owner
            .observe_native_control(rig.presentation_mut(), identity)
            .unwrap()
            .unwrap();
        assert_eq!(control.identity(), identity);
        let before = format!("{:?}", rig.presentation());
        let report = owner.render_report();
        let query = owner
            .source_playback_at(rig.presentation(), &control.query(), identity)
            .unwrap()
            .unwrap();
        assert_eq!(query.identity(), identity);
        assert_eq!(query.facts().render, report);
        assert_eq!(
            owner
                .request_source_history_retirement(rig.presentation(), &control.query(), identity)
                .unwrap(),
            Some(1)
        );
        assert_eq!(trace.history_queries().len(), 2);
        assert_eq!(trace.history_requests().len(), 1);
        assert_eq!(count(&trace, "conditional-poll"), 0);
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert_eq!(owner.render_report(), report);
    }
    for frequency in [None, Some(OLD_FREQUENCY)] {
        let mut rig = Rig::with_frequency(false, frequency);
        let (effects, output) = rig.take_live_native_owner();
        let trace = effects.clone();
        let owner = GameplayOutputOwner::<Effects, NativeOutputState>::new(effects, output);
        exercise(rig, owner, trace);
        let mut rig = Rig::with_frequency(false, frequency);
        let (effects, output) = rig.take_live_native_owner();
        let trace = effects.clone();
        let owner = GameplayOutputOwner::<Effects, NativeOutputState, OutputFrameBasis, u8>::new_with_ticket(effects, output);
        exercise(rig, owner, trace);
    }
}

#[test]
fn retained_source_legacy_default_mixer_owner_still_uses_its_original_compound_source_port() {
    use super::source_playback_owner_fixtures as legacy;
    let (mut owner, mut presentation, trace, _producer) = legacy::fixture();
    let basis = owner.current().unwrap().basis;
    let report = owner.current().unwrap().report;
    trace
        .observations
        .borrow_mut()
        .push_back(Ok(Some(legacy::observation(basis, 2, 100, report))));
    let control = owner
        .observe_native_control(&mut presentation, legacy::identity())
        .unwrap()
        .unwrap();
    trace.queries.borrow_mut().push_back(Ok(Some(legacy::packet(
        basis,
        OLD_EPOCH,
        legacy::identity(),
        control.query(),
        report,
    ))));
    let packet = owner
        .source_playback_at(&presentation, &control.query(), legacy::identity())
        .unwrap()
        .unwrap();
    assert_eq!(packet.native_evidence().unwrap().words, [19, 31]);
    assert_eq!(packet.facts().render, report);
    assert_eq!(trace.observed_calls.get(), 1);
    assert_eq!(trace.query_calls.get(), 1);
    assert_eq!(trace.report_calls.get(), 0);
    assert_eq!(trace.legacy_calls.get(), 0);
}
