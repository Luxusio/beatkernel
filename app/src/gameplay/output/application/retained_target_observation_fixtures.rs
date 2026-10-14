//! Old conditional native IO remains authoritative until actual retirement permission.
use super::*;
use crate::coordinate_target_replacement_test_support as support;
use crate::gameplay::output::{
    application::owner::CoordinateTargetOwnerPublication, ports::TargetRetirementStatus,
};
use beatkernel::audio::TargetTime;
use std::{error::Error, io};
use support::*;

type Owner = GameplayOutputOwner<Effects, ConvertedNativeOutputState, TargetFrameBasis, Ticket>;

fn setup() -> (Rig, Owner, Effects) {
    let mut rig = Rig::new(false);
    let (effects, output) = rig.take_live_native_owner();
    let mut trace = effects.clone();
    let mut owner = Owner::new_with_ticket(effects, output);
    trace.conditional_statuses([TargetRetirementStatus::Pending]);
    assert!(owner.queue(Request::normal(32_000), 100_000_000).is_ok());
    let identity = rig.source_identity();
    let result = owner
        .publish_paused_coordinate_target_audio(rig.context(), now(), identity, false, |_| Ok(()))
        .unwrap();
    assert!(matches!(result, CoordinateTargetOwnerPublication::Pending));
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
        Ok(_) => panic!("scripted failure must be returned, never hidden as unavailable audio"),
    }
}
fn history_setup() -> (
    Rig,
    Owner,
    Effects,
    NativeTargetPlaybackObservation<Evidence>,
) {
    let (mut rig, mut owner, mut trace) = setup();
    let identity = rig.source_identity();
    let original = owner
        .observe_target_native_playback(rig.presentation_mut(), identity)
        .unwrap()
        .unwrap();
    trace.enable_history_service();
    (rig, owner, trace, original)
}

#[test]
fn retained_target_history_authorization_during_query_or_request_defers_old_stream_failure() {
    for during_query in [false, true] {
        let (rig, mut owner, mut trace, original) = history_setup();
        let identity = rig.source_identity();
        let query = original.playback().query();
        let native_words = original
            .playback()
            .native_evidence()
            .unwrap()
            .original_native_words;
        trace.conditional_statuses([TargetRetirementStatus::Pending]);
        if during_query {
            trace.history_query_faults([HistoryFault::Authorize]);
        } else {
            trace.history_request_faults([HistoryFault::Authorize]);
        }
        let record = rig.presentation().latest_record().unwrap().pair();
        let result = owner.request_conditional_target_history_retirement_with_source_pin(
            rig.presentation(),
            &query,
            identity,
            Some(identity.initial_physical()),
        );
        assert!(
            matches!(result, Ok(None)),
            "confirmed native authorization must defer old-stream history IO: {result:?}"
        );
        assert_eq!(trace.history_queries(), vec![native_words]);
        assert_eq!(trace.history_requests().len(), usize::from(!during_query));
        assert_eq!(rig.presentation().latest_record().unwrap().pair(), record);
        assert!(owner.output_clock_suspended());
        assert!(owner.current().is_none());
        assert!(owner.has_work());
    }
}

#[test]
fn retained_target_history_success_preserves_original_packets_pins_revision_and_live_state() {
    for case in 0..4 {
        let (rig, mut owner, mut trace, original) = history_setup();
        let identity = rig.source_identity();
        let query = original.playback().query();
        let facts = original.playback().facts().playback;
        let cover = facts.playback.physical_start();
        assert!(cover > identity.initial_physical());
        let source_minimum = match case {
            0 => Some(identity.initial_physical()),
            1 => Some(cover),
            2 => Some(u64::MAX),
            _ => None,
        };
        let words = facts.phase.to_words();
        let target_pin = TargetTime::new(words[4], words[5], words[6]).unwrap();
        let source_pin = source_minimum.map_or(cover, |hint| hint.min(cover));
        let presentation = format!("{:?}", rig.presentation());
        let merger = rig.inputs.preparation_state().unwrap();
        let source = source_history(owner.current().unwrap().source_state());
        let phases = phase_words(owner.current().unwrap().source_state());
        let report = owner.render_report();
        let native_id = owner.current().unwrap().native_id;
        trace.conditional_statuses([TargetRetirementStatus::Pending; 2]);
        for revision in 1..=2 {
            assert_eq!(
                owner
                    .request_conditional_target_history_retirement_with_source_pin(
                        rig.presentation(),
                        &query,
                        identity,
                        source_minimum,
                    )
                    .unwrap(),
                Some(revision)
            );
        }
        assert_eq!(
            trace.history_requests(),
            vec![
                (
                    OLD_EPOCH,
                    original.playback().basis(),
                    identity,
                    source_pin,
                    target_pin
                );
                2
            ]
        );
        assert_eq!(
            trace.history_queries(),
            vec![
                [
                    native_id,
                    original
                        .playback()
                        .native_evidence()
                        .unwrap()
                        .original_native_words[1]
                ];
                2
            ]
        );
        assert_eq!(
            source_history(owner.current().unwrap().source_state()),
            source
        );
        assert_eq!(phase_words(owner.current().unwrap().source_state()), phases);
        assert_eq!(owner.render_report(), report);
        assert_eq!(format!("{:?}", rig.presentation()), presentation);
        assert!(rig.inputs.matches_preparation(&merger));
        assert_eq!(owner.state(), ReplacementState::ConditionalRetirement);
        assert!(!owner.output_clock_suspended());
        assert!(owner.has_work());
    }
}

#[test]
fn retained_target_history_authorization_before_io_skips_both_old_native_operations() {
    let (rig, mut owner, mut trace, original) = history_setup();
    let identity = rig.source_identity();
    let presentation = format!("{:?}", rig.presentation());
    trace.conditional_statuses([TargetRetirementStatus::Authorized]);
    trace.history_query_faults([HistoryFault::Io]);
    trace.history_request_faults([HistoryFault::Io]);
    assert!(owner
        .request_conditional_target_history_retirement_with_source_pin(
            rig.presentation(),
            &original.playback().query(),
            identity,
            Some(identity.initial_physical()),
        )
        .unwrap()
        .is_none());
    assert!(trace.history_queries().is_empty());
    assert!(trace.history_requests().is_empty());
    assert_eq!(count(&trace, "retire"), 0);
    assert_eq!(format!("{:?}", rig.presentation()), presentation);
    assert!(owner.current().is_none());
    assert!(owner.output_clock_suspended());
    assert!(owner.has_work());
}

#[test]
fn retained_target_history_query_and_request_errors_remain_original_without_authorization() {
    for status in [
        TargetRetirementStatus::Pending,
        TargetRetirementStatus::Refused,
        TargetRetirementStatus::Cancelled,
    ] {
        for during_query in [false, true] {
            let (rig, mut owner, mut trace, original) = history_setup();
            let identity = rig.source_identity();
            let native_id = owner.current().unwrap().native_id;
            let presentation = format!("{:?}", rig.presentation());
            trace.conditional_statuses([TargetRetirementStatus::Pending, status]);
            if during_query {
                trace.history_query_faults([HistoryFault::Io]);
            } else {
                trace.history_request_faults([HistoryFault::Io]);
            }
            let failure = error(
                owner.request_conditional_target_history_retirement_with_source_pin(
                    rig.presentation(),
                    &original.playback().query(),
                    identity,
                    Some(identity.initial_physical()),
                ),
            );
            let failure = failure
                .downcast_ref::<ReplacementFailure<io::Error>>()
                .expect("raw native history error remains typed");
            match &failure.cause {
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
                _ => panic!("original backend error and phase must survive"),
            }
            assert_eq!(trace.history_queries().len(), 1);
            assert_eq!(trace.history_requests().len(), usize::from(!during_query));
            assert_eq!(count(&trace, "conditional-poll"), 3);
            assert_eq!(count(&trace, "retire"), 0);
            assert_eq!(owner.current().unwrap().native_id, native_id);
            assert!(!owner.output_clock_suspended());
            assert_eq!(format!("{:?}", rig.presentation()), presentation);
        }
    }
}

#[test]
fn retained_target_history_initial_probe_failure_keeps_cleanup_and_recovery_without_io() {
    let (rig, mut owner, mut trace, original) = history_setup();
    let identity = rig.source_identity();
    trace.conditional_probe_failures([true]);
    trace.retirement_failures([true]);
    trace.recovery_failures([true]);
    let failure = error(
        owner.request_conditional_target_history_retirement_with_source_pin(
            rig.presentation(),
            &original.playback().query(),
            identity,
            Some(identity.initial_physical()),
        ),
    );
    let failure = failure
        .downcast_ref::<ReplacementFailure<io::Error>>()
        .expect("initial probe failure remains typed");
    assert!(
        matches!(&failure.cause, ReplacementCause::Backend { phase: ReplacementPhase::Retire, error } if error.to_string() == "scripted native conditional probe failure")
    );
    assert_eq!(
        failure.cleanup.as_ref().unwrap().to_string(),
        "scripted native retirement failure"
    );
    assert_eq!(
        failure.recovery.as_ref().unwrap().to_string(),
        "scripted native recovery failure"
    );
    assert!(trace.history_queries().is_empty());
    assert!(trace.history_requests().is_empty());
    assert_eq!(count(&trace, "retire"), 1);
    assert_eq!(count(&trace, "recover"), 1);
    assert_eq!(owner.state(), ReplacementState::PendingRetirement);
    assert!(owner.current().is_none());
    assert!(owner.output_clock_suspended());
}

#[test]
fn retained_target_history_double_failure_preserves_operation_probe_cleanup_and_recovery() {
    for during_query in [false, true] {
        let (rig, mut owner, mut trace, original) = history_setup();
        let identity = rig.source_identity();
        let presentation = format!("{:?}", rig.presentation());
        trace.conditional_statuses([TargetRetirementStatus::Pending]);
        trace.conditional_probe_failures([false, true]);
        trace.retirement_failures([true]);
        trace.recovery_failures([true]);
        if during_query {
            trace.history_query_faults([HistoryFault::Io]);
        } else {
            trace.history_request_faults([HistoryFault::Io]);
        }
        let failure = error(
            owner.request_conditional_target_history_retirement_with_source_pin(
                rig.presentation(),
                &original.playback().query(),
                identity,
                Some(identity.initial_physical()),
            ),
        );
        let combined = failure.downcast_ref::<ConditionalTargetObservationFailure<io::Error, ReplacementFailure<io::Error>>>().expect("both original typed failures must survive");
        let original_error = if during_query {
            "original native history query failure"
        } else {
            "original native history retirement failure"
        };
        match &combined.observation.cause {
            ReplacementCause::Backend { phase, error } => {
                assert_eq!(
                    *phase,
                    if during_query {
                        ReplacementPhase::Observe
                    } else {
                        ReplacementPhase::Retire
                    }
                );
                assert_eq!(error.to_string(), original_error);
            }
            _ => panic!("original operation failure must remain typed"),
        }
        assert!(
            matches!(&combined.probe.cause, ReplacementCause::Backend { phase: ReplacementPhase::Retire, error } if error.to_string() == "scripted native conditional probe failure")
        );
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
        let rendered = failure.to_string();
        for expected in [
            original_error,
            "scripted native conditional probe failure",
            "scripted native retirement failure",
            "scripted native recovery failure",
        ] {
            assert!(
                rendered.contains(expected),
                "all original errors survive Display: {rendered}"
            );
        }
        assert_eq!(trace.history_queries().len(), 1);
        assert_eq!(trace.history_requests().len(), usize::from(!during_query));
        assert_eq!(count(&trace, "retire"), 1);
        assert_eq!(count(&trace, "recover"), 1);
        assert_eq!(owner.state(), ReplacementState::PendingRetirement);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        assert_eq!(format!("{:?}", rig.presentation()), presentation);
    }
}

#[test]
fn retained_target_history_packet_and_source_policy_errors_never_reprobe_later_authorization() {
    for case in 0..3 {
        let (rig, mut owner, mut trace, original) = history_setup();
        let mut identity = rig.source_identity();
        let presentation = format!("{:?}", rig.presentation());
        trace.conditional_statuses([
            TargetRetirementStatus::Pending,
            TargetRetirementStatus::Authorized,
        ]);
        match case {
            0 => trace.history_query_faults([HistoryFault::PacketIdentity]),
            1 => trace.history_query_faults([HistoryFault::PacketQuery]),
            _ => {
                identity = SourcePlaybackIdentity::new(
                    identity.hz(),
                    identity.initial_physical() + 1,
                    identity.initial_playback(),
                )
                .unwrap()
            }
        }
        let failure = error(
            owner.request_conditional_target_history_retirement_with_source_pin(
                rig.presentation(),
                &original.playback().query(),
                identity,
                Some(rig.source_identity().initial_physical()),
            ),
        );
        let failure = failure
            .downcast_ref::<ReplacementFailure<io::Error>>()
            .expect("policy failure remains typed");
        assert!(matches!(failure.cause, ReplacementCause::Policy(_)));
        assert_eq!(
            count(&trace, "conditional-poll"),
            2,
            "only raw IO errors may re-probe"
        );
        assert_eq!(trace.history_queries().len(), usize::from(case < 2));
        assert!(trace.history_requests().is_empty());
        assert!(owner.current().is_some());
        assert!(!owner.output_clock_suspended());
        assert_eq!(format!("{:?}", rig.presentation()), presentation);
    }
}

#[test]
fn retained_target_history_diverged_native_timing_remains_error_before_query_and_later_authorization(
) {
    let (mut rig, mut owner, mut trace, original) = history_setup();
    let identity = rig.source_identity();
    let crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame { frame } =
        original.coordinate().coordinate()
    else {
        panic!("actual original held packet has native frame coordinates");
    };
    // Advance only the authority to exercise the existing compound timing guard.
    // A valid native packet cannot be substituted for the now divergent histories.
    rig.presentation_mut()
        .authority_mut()
        .observe_coordinate(
            OLD_EPOCH,
            beatkernel::time::ClockPair {
                source: original
                    .playback()
                    .basis()
                    .point_at_stream_frame(frame + 1)
                    .unwrap(),
                target: host(HOST_START + 100_000),
            },
            crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame {
                frame: frame + 1,
            },
        )
        .unwrap();
    trace.conditional_statuses([
        TargetRetirementStatus::Pending,
        TargetRetirementStatus::Authorized,
    ]);
    let presentation = format!("{:?}", rig.presentation());
    let failure = error(
        owner.request_conditional_target_history_retirement_with_source_pin(
            rig.presentation(),
            &original.playback().query(),
            identity,
            Some(identity.initial_physical()),
        ),
    );
    let failure = failure
        .downcast_ref::<ReplacementFailure<io::Error>>()
        .expect("compound timing error remains typed");
    assert!(matches!(failure.cause, ReplacementCause::Timing(_)));
    assert_eq!(count(&trace, "conditional-poll"), 2);
    assert!(trace.history_queries().is_empty());
    assert!(trace.history_requests().is_empty());
    assert!(owner.current().is_some());
    assert!(!owner.output_clock_suspended());
    assert_eq!(format!("{:?}", rig.presentation()), presentation);
}

#[test]
fn retained_target_observation_pending_reads_and_admits_the_original_live_native_packet() {
    let (mut rig, mut owner, mut trace) = setup();
    trace.conditional_statuses([TargetRetirementStatus::Pending]);
    let native_id = owner.current().unwrap().native_id;
    let identity = rig.source_identity();
    let old_record = rig.presentation().latest_record().unwrap().pair();
    let merger = rig.inputs.preparation_state().unwrap();
    let observation = owner
        .observe_conditional_target_native_playback(rig.presentation_mut(), identity)
        .unwrap()
        .unwrap();
    assert_eq!(count(&trace, "packet"), 1);
    assert_eq!(count(&trace, "conditional-poll"), 2);
    assert_eq!(owner.current().unwrap().native_id, native_id);
    assert_eq!(
        observation
            .playback()
            .native_evidence()
            .unwrap()
            .original_native_words[0],
        native_id
    );
    assert_eq!(observation.playback().epoch(), OLD_EPOCH);
    assert_eq!(observation.playback().identity(), identity);
    assert_eq!(
        rig.presentation().latest_record().unwrap().pair(),
        match observation.coordinate().snapshot().evidence {
            OriginalNativePresentationEvidence::SuppliedPair(pair) => pair,
            _ => panic!("actual scripted native observation preserves its original pair"),
        }
    );
    assert!(
        rig.presentation()
            .latest_record()
            .unwrap()
            .pair()
            .source
            .timestamp
            > old_record.source.timestamp
    );
    assert_eq!(
        owner.render_report(),
        observation
            .playback()
            .facts()
            .source
            .filter(|report| report.frames != 0)
    );
    assert!(rig.inputs.matches_preparation(&merger));
    assert!(!owner.output_clock_suspended());
}

#[test]
fn retained_target_observation_control_uses_the_same_original_packet_and_creation_identity() {
    let (mut rig, mut owner, mut trace) = setup();
    trace.conditional_statuses([TargetRetirementStatus::Pending]);
    let identity = rig.source_identity();
    let control = owner
        .observe_conditional_target_native_control(rig.presentation_mut(), identity)
        .unwrap()
        .unwrap();
    assert_eq!(control.epoch(), OLD_EPOCH);
    assert_eq!(control.identity(), identity);
    assert_eq!(
        control.coordinate().snapshot().basis,
        owner.current().unwrap().basis
    );
    assert_eq!(
        control.pair(),
        rig.presentation().latest_record().unwrap().pair()
    );
    assert_eq!(count(&trace, "packet"), 1);
    assert!(!owner.output_clock_suspended());
}

#[test]
fn retained_target_observation_authorization_before_io_suspends_old_clock_without_native_read() {
    for control in [false, true] {
        let (mut rig, mut owner, mut trace) = setup();
        trace.conditional_statuses([TargetRetirementStatus::Authorized]);
        trace.observation_faults([Fault::Observe]);
        let identity = rig.source_identity();
        let record = rig.presentation().latest_record().unwrap().pair();
        if control {
            assert!(owner
                .observe_conditional_target_native_control(rig.presentation_mut(), identity)
                .unwrap()
                .is_none());
        } else {
            assert!(owner
                .observe_conditional_target_native_playback(rig.presentation_mut(), identity)
                .unwrap()
                .is_none());
        }
        assert_eq!(count(&trace, "packet"), 0);
        assert_eq!(count(&trace, "retire"), 0);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        assert!(owner.has_work());
        assert_eq!(rig.presentation().latest_record().unwrap().pair(), record);
        assert!(rig.runtime.hold_audio_pause().is_err());
    }
}

#[test]
fn retained_target_observation_authorization_during_failing_io_only_skips_the_expected_old_stream_error(
) {
    for control in [false, true] {
        let (mut rig, mut owner, mut trace) = setup();
        trace.conditional_statuses([
            TargetRetirementStatus::Pending,
            TargetRetirementStatus::Authorized,
        ]);
        trace.observation_faults([Fault::Observe]);
        let identity = rig.source_identity();
        let record = rig.presentation().latest_record().unwrap().pair();
        if control {
            assert!(owner
                .observe_conditional_target_native_control(rig.presentation_mut(), identity)
                .unwrap()
                .is_none());
        } else {
            assert!(owner
                .observe_conditional_target_native_playback(rig.presentation_mut(), identity)
                .unwrap()
                .is_none());
        }
        assert_eq!(count(&trace, "packet"), 1);
        assert_eq!(count(&trace, "conditional-poll"), 3);
        assert_eq!(count(&trace, "retire"), 0);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        assert_eq!(rig.presentation().latest_record().unwrap().pair(), record);
    }
}

#[test]
fn retained_target_observation_real_io_failures_remain_errors_without_authorization() {
    for status in [
        TargetRetirementStatus::Pending,
        TargetRetirementStatus::Refused,
        TargetRetirementStatus::Cancelled,
    ] {
        for control in [false, true] {
            let (mut rig, mut owner, mut trace) = setup();
            trace.conditional_statuses([status, TargetRetirementStatus::Pending]);
            trace.observation_faults([Fault::Observe]);
            let identity = rig.source_identity();
            let record = rig.presentation().latest_record().unwrap().pair();
            let failure =
                if control {
                    error(owner.observe_conditional_target_native_control(
                        rig.presentation_mut(),
                        identity,
                    ))
                } else {
                    error(owner.observe_conditional_target_native_playback(
                        rig.presentation_mut(),
                        identity,
                    ))
                };
            let failure = failure
                .downcast_ref::<ReplacementFailure<io::Error>>()
                .expect("original Observe failure retains its typed error");
            assert!(
                matches!(&failure.cause, ReplacementCause::Backend { phase: ReplacementPhase::Observe, error } if error.to_string() == "original native observation failure")
            );
            assert_eq!(count(&trace, "packet"), 1);
            assert_eq!(count(&trace, "retire"), 0);
            assert!(owner.current().is_some());
            assert!(!owner.output_clock_suspended());
            assert_eq!(rig.presentation().latest_record().unwrap().pair(), record);
        }
    }
}

#[test]
fn retained_target_observation_initial_probe_failure_preserves_probe_cleanup_and_recovery_errors() {
    let (mut rig, mut owner, mut trace) = setup();
    trace.conditional_probe_failures([true]);
    trace.retirement_failures([true]);
    trace.recovery_failures([true]);
    let identity = rig.source_identity();
    let failure =
        error(owner.observe_conditional_target_native_playback(rig.presentation_mut(), identity));
    let failure = failure
        .downcast_ref::<ReplacementFailure<io::Error>>()
        .expect("initial probe error remains typed");
    assert!(
        matches!(&failure.cause, ReplacementCause::Backend { phase: ReplacementPhase::Retire, error } if error.to_string() == "scripted native conditional probe failure")
    );
    assert_eq!(
        failure.cleanup.as_ref().unwrap().to_string(),
        "scripted native retirement failure"
    );
    assert_eq!(
        failure.recovery.as_ref().unwrap().to_string(),
        "scripted native recovery failure"
    );
    assert_eq!(count(&trace, "packet"), 0);
    assert_eq!(count(&trace, "retire"), 1);
    assert_eq!(count(&trace, "recover"), 1);
    assert_eq!(owner.state(), ReplacementState::PendingRetirement);
    assert!(owner.current().is_none());
    assert!(owner.output_clock_suspended());
    assert!(rig.runtime.hold_audio_pause().is_err());
}

#[test]
fn retained_target_observation_double_failure_keeps_observation_probe_cleanup_and_recovery() {
    for control in [false, true] {
        let (mut rig, mut owner, mut trace) = setup();
        trace.conditional_statuses([TargetRetirementStatus::Pending]);
        trace.conditional_probe_failures([false, true]);
        trace.observation_faults([Fault::Observe]);
        trace.retirement_failures([true]);
        trace.recovery_failures([true]);
        let identity = rig.source_identity();
        let failure = if control {
            error(owner.observe_conditional_target_native_control(rig.presentation_mut(), identity))
        } else {
            error(
                owner.observe_conditional_target_native_playback(rig.presentation_mut(), identity),
            )
        };
        let combined = failure
            .downcast_ref::<ConditionalTargetObservationFailure<io::Error>>()
            .expect("double failure retains both owned typed errors");
        assert_eq!(
            combined.observation.to_string(),
            "original native observation failure"
        );
        assert!(
            matches!(&combined.probe.cause, ReplacementCause::Backend { phase: ReplacementPhase::Retire, error } if error.to_string() == "scripted native conditional probe failure")
        );
        assert_eq!(
            combined.probe.cleanup.as_ref().unwrap().to_string(),
            "scripted native retirement failure"
        );
        assert_eq!(
            combined.probe.recovery.as_ref().unwrap().to_string(),
            "scripted native recovery failure"
        );
        assert_eq!(
            failure.source().unwrap().to_string(),
            "original native observation failure"
        );
        let rendered = failure.to_string();
        for expected in [
            "original native observation failure",
            "scripted native conditional probe failure",
            "scripted native retirement failure",
            "scripted native recovery failure",
        ] {
            assert!(
                rendered.contains(expected),
                "all original errors survive Display: {rendered}"
            );
        }
        assert_eq!(count(&trace, "packet"), 1);
        assert_eq!(count(&trace, "retire"), 1);
        assert_eq!(count(&trace, "recover"), 1);
        assert_eq!(owner.state(), ReplacementState::PendingRetirement);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        assert!(rig.runtime.hold_audio_pause().is_err());
    }
}

#[test]
fn retained_target_observation_packet_identity_failure_is_never_masked_by_later_authorization() {
    let (mut rig, mut owner, mut trace) = setup();
    trace.conditional_statuses([
        TargetRetirementStatus::Pending,
        TargetRetirementStatus::Authorized,
    ]);
    trace.observation_faults([Fault::ChangedAfterRead]);
    let identity = rig.source_identity();
    let old_record = rig.presentation().latest_record().unwrap().pair();
    let failure =
        error(owner.observe_conditional_target_native_playback(rig.presentation_mut(), identity));
    let failure = failure
        .downcast_ref::<ReplacementFailure<io::Error>>()
        .expect("packet identity policy failure remains typed");
    assert!(matches!(failure.cause, ReplacementCause::Policy(_)));
    assert_eq!(
        count(&trace, "conditional-poll"),
        2,
        "only raw IO errors permit an authorization re-probe"
    );
    assert_eq!(count(&trace, "packet"), 1);
    assert_eq!(
        rig.presentation().latest_record().unwrap().pair(),
        old_record
    );
    assert!(!owner.output_clock_suspended());
}
