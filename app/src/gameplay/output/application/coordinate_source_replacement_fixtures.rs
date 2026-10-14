//! Independent Source replacement checks over actual retained software state.
use super::{test_support::*, *};
use crate::{
    gameplay::output::application::replacement::{ReplacementPhase, ReplacementState},
    gameplay_presentation::{GameplayAudioOutputContext, GameplayPauseControl},
    native_audio_startup::new_coordinate_audio_presentation,
};
use beatkernel::{audio::*, input::DeviceId, judge::JudgeEngine, time::ClockDomainId};
use beatkernel_platform::audio::{
    presentation::validation::OriginalNativePresentationEvidence, DeviceFormat, SampleEncoding,
};
use std::cell::Cell;

fn publish(r: &mut Rig, ready: Ready) -> PauseHold {
    match r.publish(ready, |_| Ok(())) {
        Ok((hold, ())) => hold,
        Err(f) => panic!("actual prepared publication refused: {}", f.error),
    }
}
fn assert_no_native_retirement(r: &Rig) {
    let script = r.script.borrow();
    for effect in ["retire", "recover", "open", "created", "start", "packet"] {
        assert!(!script.calls.contains(&effect), "unexpected {effect}");
    }
}

#[test]
fn coordinate_source_replacement_retains_original_runtime_pcm_history_startup_and_input_for_both_classes(
) {
    for frequency in [None, Some(OLD_FREQUENCY), Some(10_000_000)] {
        for partial in [false, true] {
            let old_frequency = frequency.map(|_| OLD_FREQUENCY);
            let mut r = Rig::with_frequency(partial, old_frequency);
            let old = r.output.as_ref().unwrap().source_state();
            let old_history = source_history(old);
            let first_unsent = old.output_frame_basis();
            let frozen = old.mixer().playback_frame_cursor();
            let startup = old.mixer().applied_start_frame();
            let pulled = old.mixer().frame_cursor();
            let frontiers = preserved_frontiers(&r.current);
            let merger = r.inputs.preparation_state().unwrap();
            let judge = r.runtime.judge() as *const JudgeEngine;
            let transport = format!("{:?}", r.runtime.transport_mut());
            let telemetry = format!("{:?}", r.runtime.telemetry());
            let commands = r.runtime.admitted_audio_commands();
            let generation = r.practice.generation();
            let pin = r.pin;
            assert_eq!(startup, Some(2));
            assert_eq!((pin.start_physical_frame(), frozen, pulled), (5, 7, 17));
            assert_eq!(
                first_unsent.start_physical_frame(),
                if partial { 12 } else { 17 }
            );
            assert!(old_history
                .spans
                .iter()
                .any(|s| s[0] == 9 && s[3] == 7 && s[4] == 1));
            let mut request = Request::normal(frequency);
            request.period = 4;
            request.buffer = 8;
            let (_controller, ready) = r.ready(request);
            assert_eq!(ready.basis(), first_unsent);
            assert_eq!(ready.practice_mixer_basis(), pin);
            assert_eq!(ready.source_identity(), r.identity);
            assert_eq!(
                ready.coordinate_basis(),
                test_support::descriptor(first_unsent, frequency)
            );
            assert_eq!(
                ready
                    .output()
                    .source_state()
                    .mixer()
                    .playback_frame_cursor(),
                frozen
            );
            assert_eq!(
                ready.output().source_state().mixer().applied_start_frame(),
                startup
            );
            let new_history = source_history(ready.output().source_state());
            assert_eq!(
                new_history.spans, old_history.spans,
                "paused rendering extends the original merged history"
            );
            assert_eq!(new_history.initial_physical, 5);
            assert_eq!(new_history.initial_playback, 3);
            assert!(
                new_history.pulled_end > pulled,
                "new evidence requires a genuinely fresh render"
            );
            let snapshots = ready.snapshots();
            assert_ne!(snapshots[0].coordinate(), snapshots[1].coordinate());
            assert!(snapshots[0]
                .source_query()
                .unwrap()
                .compare_interpolated_time(snapshots[1].source_query().unwrap())
                .is_lt());
            assert_eq!(
                ready
                    .observation()
                    .source()
                    .native_evidence()
                    .unwrap()
                    .original_native_words[0],
                ready.output().native_id
            );
            let candidate = ready.output().native_id;
            let calls = r.script.borrow().calls.clone();
            let hold = publish(&mut r, ready);
            assert_eq!(r.output.as_ref().unwrap().native_id, candidate);
            assert_eq!(r.current.basis(), Some(first_unsent));
            assert_eq!(r.current.practice_mixer_basis(), Some(pin));
            assert_eq!(r.current.authority().epoch().id, OLD_EPOCH + 1);
            assert_eq!(
                r.current.authority().coordinate_basis(),
                Some(test_support::descriptor(first_unsent, frequency))
            );
            assert_eq!(r.pause.phase(), PausePhase::Paused);
            assert_eq!(r.pause.epoch(), OLD_EPOCH + 1);
            assert_eq!(preserved_frontiers(&r.current), frontiers);
            assert_eq!(r.inputs.preparation_state().unwrap(), merger);
            assert_eq!(r.inputs.source_count(), 1);
            assert_eq!(r.inputs.pending(), 0);
            assert_eq!(r.runtime.judge() as *const JudgeEngine, judge);
            assert_eq!(format!("{:?}", r.runtime.transport_mut()), transport);
            assert_eq!(format!("{:?}", r.runtime.telemetry()), telemetry);
            assert_eq!(r.runtime.admitted_audio_commands(), commands);
            assert_eq!(r.practice.generation(), generation);
            assert_eq!(
                r.script.borrow().calls,
                calls,
                "publication adds no native effects"
            );
            assert!(r.runtime.hold_audio_pause().is_err());
            drop(hold);
            drop(r.runtime.hold_audio_pause().unwrap());
        }
    }
}

#[test]
fn coordinate_source_replacement_empty_output_allows_channel_matrix_change_without_rebuilding_source_pcm(
) {
    let mut r = Rig::new(false);
    let pin = r.pin;
    let history = source_history(r.output.as_ref().unwrap().source_state());
    let format = DeviceFormat::new(SOURCE_RATE, 2, SampleEncoding::Float32, Some(3)).unwrap();
    let matrix = ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap();
    let request = Request::configured(format, Some(matrix), 4, 32, Some(10_000_000));
    let (_c, ready) = r.ready(request);
    assert_eq!(ready.output().source_state().format(), Some(format));
    assert_eq!(
        ready
            .output()
            .source_state()
            .mixer()
            .config()
            .format()
            .channels(),
        1
    );
    assert_eq!(
        source_history(ready.output().source_state()).spans,
        history.spans
    );
    assert_eq!(ready.practice_mixer_basis(), pin);
    drop(publish(&mut r, ready));
    assert!(r
        .pause
        .request_coordinate(false, r.current.authority())
        .unwrap());
    r.runtime.request_audio_pause(false);
    let state = r.output.as_mut().unwrap().state.as_mut().unwrap();
    assert_eq!(state.render_pending(2).unwrap().playback_frames, 2);
    assert_eq!(
        state.pending_samples(),
        [0.25, 0.125, 0.25, 0.125],
        "the original sample and real prepared remix resume together"
    );
}

#[test]
fn coordinate_source_replacement_pending_refusal_and_cancel_preserve_the_live_owner_and_clock() {
    for terminal in [
        SourceRetirementStatus::Refused,
        SourceRetirementStatus::Cancelled,
    ] {
        let mut r = Rig::new(true);
        let old_id = r.output.as_ref().unwrap().native_id;
        let old_state = owner_facts(r.output.as_ref().unwrap().source_state());
        let presentation = format!("{:?}", r.current);
        r.script
            .borrow_mut()
            .statuses
            .extend([SourceRetirementStatus::Pending, terminal]);
        let mut c = r.controller();
        r.begin(&mut c, Request::normal(Some(10_000_000))).unwrap();
        pending(r.poll(&mut c).unwrap());
        assert_eq!(c.state(), ReplacementState::ConditionalRetirement);
        assert!(!c.output_clock_suspended());
        assert_eq!(c.conditional_current().unwrap().native_id, old_id);
        assert_eq!(
            owner_facts(c.conditional_current().unwrap().source_state()),
            old_state
        );
        assert!(r.runtime.hold_audio_pause().is_err());
        assert_no_native_retirement(&r);
        assert!(matches!(
            r.poll(&mut c).unwrap(),
            CoordinateSourcePoll::LiveRefused(_)
        ));
        assert_eq!(c.last_issued_epoch(), OLD_EPOCH);
        let live = c.take_attached_output().unwrap();
        assert_eq!(live.native_id, old_id);
        assert_eq!(owner_facts(live.source_state()), old_state);
        assert!(!live.retired);
        assert_eq!(format!("{:?}", r.current), presentation);
        assert_eq!(r.script.borrow().consumed_tickets.len(), 1);
        assert_no_native_retirement(&r);
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn coordinate_source_replacement_cancellation_consumes_only_its_original_live_ticket() {
    for status in [
        SourceRetirementStatus::Pending,
        SourceRetirementStatus::Cancelled,
        SourceRetirementStatus::Authorized,
    ] {
        let mut r = Rig::new(true);
        let old_id = r.output.as_ref().unwrap().native_id;
        let old = owner_facts(r.output.as_ref().unwrap().source_state());
        let mut c = r.controller();
        r.begin(&mut c, Request::normal(Some(OLD_FREQUENCY)))
            .unwrap();
        r.script.borrow_mut().cancel_status = Some(status);
        let cancellation = c.cancel_coordinate_source_audio().unwrap();
        match status {
            SourceRetirementStatus::Pending => {
                assert_eq!(cancellation, CoordinateSourceCancellation::Pending);
                assert_eq!(c.conditional_current().unwrap().native_id, old_id);
                assert!(!c.output_clock_suspended());
                assert!(r.runtime.hold_audio_pause().is_err());
                r.script.borrow_mut().cancel_status = Some(SourceRetirementStatus::Cancelled);
                assert_eq!(
                    c.cancel_coordinate_source_audio().unwrap(),
                    CoordinateSourceCancellation::LiveRestored
                );
                assert_eq!(
                    owner_facts(c.take_attached_output().unwrap().source_state()),
                    old
                );
                assert_eq!(r.script.borrow().consumed_tickets.len(), 1);
                assert_no_native_retirement(&r);
            }
            SourceRetirementStatus::Cancelled => {
                assert_eq!(cancellation, CoordinateSourceCancellation::LiveRestored);
                assert_eq!(c.take_attached_output().unwrap().native_id, old_id);
                assert_eq!(r.script.borrow().consumed_tickets.len(), 1);
                assert_no_native_retirement(&r);
            }
            SourceRetirementStatus::Authorized => {
                assert_eq!(cancellation, CoordinateSourceCancellation::Retired);
                assert_eq!(c.state(), ReplacementState::RecoveredMixer);
                assert_eq!(owner_facts(&c.take_recovered_mixer().unwrap()), old);
                assert!(r.script.borrow().calls.contains(&"retire"));
                assert!(!r.script.borrow().calls.contains(&"open"));
            }
            _ => unreachable!(),
        }
        assert_eq!(c.last_issued_epoch(), OLD_EPOCH);
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn coordinate_source_replacement_authorization_is_sticky_and_suspends_old_io_before_retirement() {
    for cancel_after_probe in [false, true] {
        let mut r = Rig::new(true);
        let old = owner_facts(r.output.as_ref().unwrap().source_state());
        let mut c = r.controller();
        r.begin(&mut c, Request::normal(Some(10_000_000))).unwrap();
        assert_eq!(
            c.probe_coordinate_source_retirement().unwrap(),
            Some(SourceRetirementStatus::Authorized)
        );
        assert_eq!(
            c.probe_coordinate_source_retirement().unwrap(),
            Some(SourceRetirementStatus::Authorized)
        );
        assert!(c.output_clock_suspended());
        assert!(c.conditional_current().is_none());
        assert!(c.conditional_current_mut().is_none());
        assert!(r.runtime.hold_audio_pause().is_err());
        assert_no_native_retirement(&r);
        if cancel_after_probe {
            assert_eq!(
                c.cancel_coordinate_source_audio().unwrap(),
                CoordinateSourceCancellation::Retired
            );
            assert_eq!(owner_facts(&c.take_recovered_mixer().unwrap()), old);
            assert!(!r.script.borrow().calls.contains(&"open"));
        } else {
            let ready = r.poll_ready(&mut c);
            let (output, hold) = ready.into_output_and_hold();
            c.retire_owned_output_held(output, hold).unwrap();
            assert!(c.take_recovered_mixer().is_some());
        }
        assert_eq!(
            r.script
                .borrow()
                .calls
                .iter()
                .filter(|c| **c == "conditional-poll")
                .count(),
            1
        );
        assert!(!r.script.borrow().calls.contains(&"conditional-cancel"));
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn coordinate_source_replacement_cold_guards_and_quarantine_refuse_before_native_effects() {
    for variant in 0..12 {
        let mut r = Rig::new(true);
        let mut request = Request::normal(Some(10_000_000));
        let mut identity = r.identity;
        let mut practice_pending = false;
        match variant {
            0 => {
                r.end = Some(
                    NativeEnd::new(r.pin.origin(), ClockDomainId(1), SOURCE_RATE, 100).unwrap(),
                )
            }
            1 => {
                assert!(r
                    .pause
                    .request_coordinate(false, r.current.authority())
                    .unwrap());
            }
            2 => r
                .inputs
                .admit(physical(2, HOST_START + 6000), now())
                .unwrap(),
            3 => practice_pending = true,
            4 => identity = SourcePlaybackIdentity::new(SOURCE_RATE, 5, 4).unwrap(),
            5 => r.output.as_mut().unwrap().epoch += 1,
            6 => request.period = 0,
            7 => request.fault = Fault::Submit,
            8 => request.fault = Fault::Busy,
            9 => {
                request.format = DeviceFormat::new(2000, 1, SampleEncoding::Float32, None).unwrap()
            }
            10 => request.matrix = Some(ChannelMatrix::new(1, 1, &[0.5]).unwrap()),
            11 => r
                .output
                .as_mut()
                .unwrap()
                .state
                .as_mut()
                .unwrap()
                .mark_admission_uncertain(),
            _ => unreachable!(),
        }
        let old_id = r.output.as_ref().unwrap().native_id;
        let state = owner_facts(r.output.as_ref().unwrap().source_state());
        let presentation = format!("{:?}", r.current);
        let mut c = r.controller();
        let result = c.begin_coordinate_source_audio(
            request,
            &r.current,
            &r.pause,
            r.end.as_ref(),
            &r.inputs,
            identity,
            practice_pending,
            100_000_000,
            || r.runtime.hold_audio_pause(),
        );
        assert!(result.is_err(), "cold variant {variant}");
        assert_no_native_retirement(&r);
        let output = c.take_attached_output().unwrap();
        assert_eq!(output.native_id, old_id);
        assert_eq!(owner_facts(output.source_state()), state);
        assert_eq!(format!("{:?}", r.current), presentation);
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn coordinate_source_replacement_recovered_uncertain_owner_refuses_explicit_retry_without_open() {
    let mut r = Rig::new(true);
    r.output
        .as_mut()
        .unwrap()
        .state
        .as_mut()
        .unwrap()
        .mark_admission_uncertain();
    let original = owner_facts(r.output.as_ref().unwrap().source_state());
    let mut c = r.controller();
    assert!(c.stop_owned().unwrap());
    let calls = r.script.borrow().calls.clone();
    let failure = r
        .begin(&mut c, Request::normal(Some(OLD_FREQUENCY)))
        .unwrap_err();
    assert!(failure.to_string().contains("uncertain"));
    assert_eq!(
        r.script.borrow().calls,
        calls,
        "explicit Retry cannot acquire a new native owner"
    );
    assert_eq!(owner_facts(&c.take_recovered_mixer().unwrap()), original);
    assert_eq!(r.current.authority().epoch().id, OLD_EPOCH);
    drop(r.runtime.hold_audio_pause().unwrap());
}

#[test]
fn coordinate_source_replacement_exhausted_epoch_and_zero_wait_do_not_acquire_pause_hold() {
    for exhausted in [false, true] {
        let mut r = Rig::new_at_epoch(
            false,
            Some(OLD_FREQUENCY),
            if exhausted { u64::MAX } else { OLD_EPOCH },
        );
        let mut c = r.controller();
        let acquired = Cell::new(false);
        let result = c.begin_coordinate_source_audio(
            Request::normal(Some(OLD_FREQUENCY)),
            &r.current,
            &r.pause,
            None,
            &r.inputs,
            r.identity,
            false,
            if exhausted { 1 } else { 0 },
            || {
                acquired.set(true);
                r.runtime.hold_audio_pause()
            },
        );
        assert!(result.is_err());
        assert!(!acquired.get());
        assert!(r.script.borrow().calls.is_empty());
        assert!(c.take_attached_output().is_some());
    }
}

#[test]
fn coordinate_source_replacement_open_start_and_created_failures_recover_the_whole_original_owner()
{
    for fault in [Fault::Open, Fault::Start, Fault::Created] {
        let mut r = Rig::new(true);
        let original = owner_facts(r.output.as_ref().unwrap().source_state());
        let frontiers = preserved_frontiers(&r.current);
        let mut c = r.controller();
        r.begin(&mut c, Request::normal(Some(10_000_000)).with_fault(fault))
            .unwrap();
        let failure = r.poll_failure(&mut c);
        assert!(
            failure.to_string().contains("original"),
            "{fault:?}: {failure}"
        );
        assert_eq!(c.state(), ReplacementState::RecoveredMixer);
        assert_eq!(owner_facts(&c.take_recovered_mixer().unwrap()), original);
        assert!(c.take_recovered_mixer().is_none());
        assert_eq!(preserved_frontiers(&r.current), frontiers);
        assert_eq!(r.current.authority().epoch().id, OLD_EPOCH);
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn coordinate_source_replacement_original_retire_and_recover_failures_hold_unique_state_until_cleanup(
) {
    for fault in [Fault::Retire, Fault::Recover] {
        for cached in [false, true] {
            let mut r = Rig::new(true);
            r.output.as_mut().unwrap().request.fault = fault;
            let original = owner_facts(r.output.as_ref().unwrap().source_state());
            let mut c = r.controller();
            r.begin(&mut c, Request::normal(Some(OLD_FREQUENCY)))
                .unwrap();
            if cached {
                assert_eq!(
                    c.probe_coordinate_source_retirement().unwrap(),
                    Some(SourceRetirementStatus::Authorized)
                );
            }
            let failure = r.poll_failure(&mut c);
            assert!(failure.to_string().contains("original"));
            assert_eq!(c.state(), ReplacementState::PendingRetirement);
            assert!(c.output_clock_suspended());
            assert!(c.take_recovered_mixer().is_none());
            assert!(r.runtime.hold_audio_pause().is_err());
            assert!(c.retry_retirement().unwrap());
            assert_eq!(owner_facts(&c.take_recovered_mixer().unwrap()), original);
            assert!(!r.script.borrow().calls.contains(&"open"));
            drop(r.runtime.hold_audio_pause().unwrap());
        }
    }
}

#[test]
fn coordinate_source_replacement_probe_and_partial_open_errors_keep_primary_cleanup_and_recovery() {
    for partial_open in [false, true] {
        let mut r = Rig::new(true);
        let original = owner_facts(r.output.as_ref().unwrap().source_state());
        if partial_open {
            r.script
                .borrow_mut()
                .retirement_failures
                .extend([false, true]);
            r.script
                .borrow_mut()
                .recovery_failures
                .extend([false, true]);
        } else {
            r.script.borrow_mut().probe_failures.push_back(true);
            r.script.borrow_mut().retirement_failures.push_back(true);
            r.script.borrow_mut().recovery_failures.push_back(true);
        }
        let mut c = r.controller();
        r.begin(
            &mut c,
            Request::normal(Some(OLD_FREQUENCY)).with_fault(if partial_open {
                Fault::PartialOpen
            } else {
                Fault::None
            }),
        )
        .unwrap();
        let failure = if partial_open {
            r.poll_failure(&mut c)
        } else {
            c.probe_coordinate_source_retirement().unwrap_err()
        };
        let text = failure.to_string();
        assert!(
            text.contains(if partial_open {
                "original partial open failure"
            } else {
                "conditional probe failure"
            }),
            "{text}"
        );
        assert!(failure.cleanup.is_some());
        if partial_open {
            assert!(
                failure.recovery.is_none(),
                "open preserves pending ownership before any recovery attempt"
            );
            assert!(text.contains("original partial open cleanup"), "{text}");
        } else {
            assert!(failure.recovery.is_some());
        }
        assert_eq!(c.state(), ReplacementState::PendingRetirement);
        assert!(r.runtime.hold_audio_pause().is_err());
        if partial_open {
            let retry = c.retry_retirement().unwrap_err();
            assert!(retry
                .to_string()
                .contains("scripted native retirement failure"));
            assert!(retry
                .recovery
                .as_ref()
                .unwrap()
                .to_string()
                .contains("scripted native recovery failure"));
            assert_eq!(c.state(), ReplacementState::PendingRetirement);
            assert!(r.runtime.hold_audio_pause().is_err());
        }
        assert!(c.retry_retirement().unwrap());
        assert_eq!(owner_facts(&c.take_recovered_mixer().unwrap()), original);
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn coordinate_source_replacement_observation_failure_preserves_all_error_phases_and_complete_state()
{
    let mut r = Rig::new(true);
    r.script
        .borrow_mut()
        .retirement_failures
        .extend([false, true]);
    r.script
        .borrow_mut()
        .recovery_failures
        .extend([false, true]);
    let original = owner_facts(r.output.as_ref().unwrap().source_state());
    let mut c = r.controller();
    r.begin(
        &mut c,
        Request::normal(Some(OLD_FREQUENCY)).with_fault(Fault::Observe),
    )
    .unwrap();
    let failure = r.poll_failure(&mut c);
    assert!(matches!(
        failure.cause,
        ReplacementCause::Backend {
            phase: ReplacementPhase::Observe,
            ..
        }
    ));
    assert!(failure
        .to_string()
        .contains("original native observation failure"));
    assert!(failure.cleanup.is_some() && failure.recovery.is_some());
    assert_eq!(c.state(), ReplacementState::PendingRetirement);
    assert!(r.runtime.hold_audio_pause().is_err());
    assert!(c.retry_retirement().unwrap());
    assert_eq!(owner_facts(&c.take_recovered_mixer().unwrap()), original);
    drop(r.runtime.hold_audio_pause().unwrap());
}

#[test]
fn coordinate_source_replacement_rejects_incoherent_original_native_source_and_render_packets() {
    for fault in [
        Fault::Future,
        Fault::Stale,
        Fault::WrongDomain,
        Fault::WrongEpoch,
        Fault::WrongFrequency,
        Fault::WrongIdentity,
        Fault::ChangedBasis,
        Fault::ChangedAfterRead,
        Fault::RunningSource,
        Fault::WrongFrozen,
        Fault::ActiveRender,
        Fault::Unsubmitted,
        Fault::TornRevision,
        Fault::RepeatedRevision,
    ] {
        let mut r = Rig::new(false);
        let original = format!("{:?}", r.current);
        let pin = r.pin;
        let mut c = r.controller();
        r.begin(&mut c, Request::normal(Some(10_000_000)).with_fault(fault))
            .unwrap();
        let failure = r.poll_failure(&mut c);
        assert!(!failure.to_string().is_empty(), "{fault:?}");
        assert_eq!(format!("{:?}", r.current), original, "{fault:?}");
        assert_eq!(
            c.state(),
            ReplacementState::RecoveredMixer,
            "{fault:?}: {failure}"
        );
        let recovered = c.take_recovered_mixer().unwrap();
        assert_eq!(recovered.mixer().playback_frame_cursor(), 7);
        assert_eq!(recovered.mixer().applied_start_frame(), Some(2));
        assert_eq!(
            recovered
                .mixer()
                .source_playback_history()
                .unwrap()
                .initial_physical_frame(),
            pin.start_physical_frame()
        );
        assert!(c.take_recovered_mixer().is_none());
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn coordinate_source_replacement_created_metadata_missing_render_old_suffix_and_duplicate_do_not_qualify(
) {
    for fault in [
        Fault::Missing,
        Fault::MissingRender,
        Fault::OldHeldOnly,
        Fault::Duplicate,
    ] {
        let mut r = Rig::new(fault == Fault::OldHeldOnly);
        let original = format!("{:?}", r.current);
        let mut c = r.controller();
        r.begin(
            &mut c,
            Request::normal(Some(OLD_FREQUENCY)).with_fault(fault),
        )
        .unwrap();
        pending(r.poll(&mut c).unwrap());
        assert_eq!(c.state(), ReplacementState::Waiting);
        assert!(r.script.borrow().calls.contains(&"created"));
        assert!(!r.script.borrow().calls.contains(&"packet"));
        for _ in 0..3 {
            pending(r.poll(&mut c).unwrap());
        }
        if matches!(fault, Fault::Duplicate | Fault::OldHeldOnly) {
            let script = r.script.borrow();
            assert_eq!(script.native_observations.len(), 3);
            assert!(
                script
                    .native_observations
                    .windows(2)
                    .all(|pair| pair[0] == pair[1]),
                "the original native position, QPC receipt and HOST point remain exactly unchanged"
            );
        }
        assert_eq!(format!("{:?}", r.current), original);
        assert!(r.output.is_none());
        assert!(r.runtime.hold_audio_pause().is_err());
        let failure = match c.poll_coordinate_source_audio(
            host(now().timestamp.as_nanos() + 100_000_000),
            &r.current,
            &r.pause,
            &r.inputs,
        ) {
            Err(failure) => failure,
            _ => panic!("missing fresh anchors must expire"),
        };
        assert!(failure.to_string().contains("timed out"));
        assert_eq!(c.state(), ReplacementState::RecoveredMixer);
        assert!(c.take_recovered_mixer().is_some());
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn coordinate_source_replacement_partial_tail_needs_two_later_fresh_actual_packets() {
    let mut r = Rig::new(true);
    let mut c = r.controller();
    r.begin(&mut c, Request::normal(Some(10_000_000))).unwrap();
    pending(r.poll(&mut c).unwrap());
    pending(r.poll(&mut c).unwrap());
    assert_eq!(
        c.state(),
        ReplacementState::Waiting,
        "admitting old paused suffix cannot grant a native anchor"
    );
    pending(r.poll(&mut c).unwrap());
    let ready = match r.poll(&mut c).unwrap() {
        CoordinateSourcePoll::Ready(ready) => ready,
        _ => panic!("two fresh packets must qualify"),
    };
    assert_eq!(ready.basis().start_physical_frame(), 12);
    assert_eq!(
        ready
            .observation()
            .source()
            .facts()
            .playback
            .physical_start(),
        9
    );
    assert_eq!(ready.output().source_state().mixer().frame_cursor(), 21);
    assert_eq!(
        ready.observation().source().facts().submitted_source_end,
        21
    );
    let snapshots = ready.snapshots();
    assert_eq!(
        snapshots[0].coordinate(),
        NativeOutputCoordinate::Counter {
            position: 70_000,
            frequency: 10_000_000
        }
    );
    assert_eq!(
        snapshots[1].coordinate(),
        NativeOutputCoordinate::Counter {
            position: 90_000,
            frequency: 10_000_000
        }
    );
    drop(publish(&mut r, ready));
}

#[test]
fn coordinate_source_replacement_exact_counter_progress_survives_equal_rounded_nanoseconds() {
    let frequency = 4_000_000_000;
    let mut r = Rig::new(false);
    r.script.borrow_mut().native_units.extend([1, 2]);
    let (_c, ready) = r.ready(Request::normal(Some(frequency)));
    let snapshots = ready.snapshots();
    assert_eq!(
        snapshots[0].coordinate(),
        NativeOutputCoordinate::Counter {
            position: 1,
            frequency
        }
    );
    assert_eq!(
        snapshots[1].coordinate(),
        NativeOutputCoordinate::Counter {
            position: 2,
            frequency
        }
    );
    let pairs = snapshots.map(|s| {
        let OriginalNativePresentationEvidence::Wasapi { snapshot, .. } = s.snapshot().evidence
        else {
            panic!("actual WASAPI-shaped original evidence")
        };
        snapshot.clock.unwrap()
    });
    assert_eq!(
        ready
            .output()
            .source_state()
            .format()
            .unwrap()
            .sample_rate(),
        SOURCE_RATE
    );
    assert!(snapshots[0]
        .source_query()
        .unwrap()
        .compare_interpolated_time(snapshots[1].source_query().unwrap())
        .is_lt());
    assert!(pairs[0].position < pairs[1].position);
    let first_pair = snapshots[0]
        .snapshot()
        .basis
        .point_at_native_counter(pairs[0].position, frequency);
    let second_pair = snapshots[1]
        .snapshot()
        .basis
        .point_at_native_counter(pairs[1].position, frequency);
    assert_eq!(first_pair.unwrap(), second_pair.unwrap());
    drop(publish(&mut r, ready));
}

#[test]
fn coordinate_source_replacement_publication_revalidates_semantic_merger_pause_pin_and_current_atomically(
) {
    for variant in 0..9 {
        let mut r = Rig::new(false);
        let (mut c, ready) = r.ready(Request::normal(Some(10_000_000)));
        let candidate = ready.output().native_id;
        match variant {
            0 => {
                r.current
                    .authority_mut()
                    .record_acquired_prefix(now())
                    .unwrap();
            }
            1 => {
                r.inputs
                    .admit(physical(2, HOST_START + 6000), now())
                    .unwrap();
                assert!(r.inputs.pop_ready(now()).unwrap().is_some());
            }
            2 => r.inputs.commit(host(HOST_START + 6000)).unwrap(),
            3 => r.inputs.register_source(DeviceId(10)).unwrap(),
            4 => r.config.sample_rate += 1,
            5 => {
                assert!(r
                    .pause
                    .request_coordinate(false, r.current.authority())
                    .unwrap());
            }
            6 => {
                r.end = Some(
                    NativeEnd::new(r.pin.origin(), ClockDomainId(1), SOURCE_RATE, 100).unwrap(),
                )
            }
            7 => {
                let basis = OutputFrameBasis::new(r.pin.origin(), SOURCE_RATE, 6).unwrap();
                let mut changed = new_coordinate_audio_presentation(
                    OLD_EPOCH,
                    test_support::descriptor(basis, Some(OLD_FREQUENCY)),
                    ClockDomainId(1),
                    point(33, 5_000_000_000),
                    r.current.authority().config(),
                )
                .unwrap();
                changed
                    .admit_coordinate(snapshot(
                        OLD_EPOCH,
                        test_support::descriptor(basis, Some(OLD_FREQUENCY)),
                        1,
                        HOST_START,
                    ))
                    .unwrap();
                changed.pin_practice_mixer_basis(basis).unwrap();
                changed.validate_coordinate_seed().unwrap();
                r.current = changed;
            }
            8 => r.config.logical_schedule = false,
            _ => unreachable!(),
        }
        let current = format!("{:?}", r.current);
        let pause = format!("{:?}", r.pause);
        let config = format!("{:?}", r.config);
        let stages = Cell::new(0);
        let failure = match r.publish(ready, |_| {
            stages.set(stages.get() + 1);
            Ok(())
        }) {
            Err(f) => f,
            Ok(_) => panic!("stale variant {variant} must refuse"),
        };
        assert_eq!(
            stages.get(),
            0,
            "stale evidence refuses before capability staging"
        );
        assert_eq!(failure.ready.output().native_id, candidate);
        assert_eq!(format!("{:?}", r.current), current);
        assert_eq!(format!("{:?}", r.pause), pause);
        assert_eq!(format!("{:?}", r.config), config);
        assert!(r.output.is_none());
        assert!(r.runtime.hold_audio_pause().is_err());
        let (output, hold) = failure.ready.into_output_and_hold();
        c.retire_owned_output_held(output, hold).unwrap();
        assert!(c.take_recovered_mixer().is_some());
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn coordinate_source_replacement_capability_failure_returns_intact_ready_and_success_stages_once() {
    let mut r = Rig::new(true);
    let (_c, ready) = r.ready(Request::normal(Some(10_000_000)));
    let native_id = ready.output().native_id;
    let actual_state = owner_facts(ready.output().source_state());
    let before = (
        format!("{:?}", r.current),
        format!("{:?}", r.pause),
        format!("{:?}", r.config),
    );
    let calls = r.script.borrow().calls.clone();
    let stages = Cell::new(0);
    let failure = match r.publish::<()>(ready, |_| {
        stages.set(stages.get() + 1);
        Err("applied Source capability refused".into())
    }) {
        Err(f) => f,
        Ok(_) => panic!("capability refusal precedes authoritative publication"),
    };
    assert_eq!(stages.get(), 1);
    assert_eq!(
        failure.error.to_string(),
        "applied Source capability refused"
    );
    assert_eq!(failure.ready.output().native_id, native_id);
    assert_eq!(
        owner_facts(failure.ready.output().source_state()),
        actual_state
    );
    assert_eq!(
        (
            format!("{:?}", r.current),
            format!("{:?}", r.pause),
            format!("{:?}", r.config)
        ),
        before
    );
    assert!(r.output.is_none());
    assert!(r.runtime.hold_audio_pause().is_err());
    stages.set(0);
    let (hold, ()) = match r.publish(failure.ready, |_| {
        stages.set(stages.get() + 1);
        Ok(())
    }) {
        Ok(v) => v,
        Err(f) => panic!("{}", f.error),
    };
    assert_eq!(stages.get(), 1);
    assert_eq!(r.script.borrow().calls, calls);
    assert_eq!(r.output.as_ref().unwrap().native_id, native_id);
    drop(hold);
}

#[test]
fn coordinate_source_replacement_occupied_output_or_aged_native_evidence_preserves_ready() {
    for occupied in [false, true] {
        let mut r = Rig::new(false);
        let (mut c, ready) = r.ready(Request::normal(Some(10_000_000)));
        let candidate = ready.output().native_id;
        let current = format!("{:?}", r.current);
        let pause = format!("{:?}", r.pause);
        if occupied {
            r.output = Rig::new(false).output.take();
        }
        let at = if occupied {
            now()
        } else {
            host(now().timestamp.as_nanos() + 200_000_000)
        };
        let failure = match publish_ready_coordinate_source_audio_output_held(
            ready,
            &mut r.output,
            GameplayAudioOutputContext {
                control: GameplayPauseControl::solo(&mut r.runtime),
                presentation: &mut r.current,
                merger: &r.inputs,
                pause: &mut r.pause,
                config: &mut r.config,
                end: &mut r.end,
            },
            at,
            |_| Ok(()),
        ) {
            Err(f) => f,
            Ok(_) => panic!("occupied or aged publication must refuse"),
        };
        assert_eq!(failure.ready.output().native_id, candidate);
        assert_eq!(format!("{:?}", r.current), current);
        assert_eq!(format!("{:?}", r.pause), pause);
        assert_eq!(r.output.is_some(), occupied);
        assert!(r.runtime.hold_audio_pause().is_err());
        let (output, hold) = failure.ready.into_output_and_hold();
        c.retire_owned_output_held(output, hold).unwrap();
        assert!(c.take_recovered_mixer().is_some());
    }
}

#[test]
fn coordinate_source_replacement_failed_open_retries_same_owner_with_checked_epoch_then_replaces_again(
) {
    let mut r = Rig::new(true);
    let pin = r.pin;
    let generation = r.practice.generation();
    let mut c = r.controller();
    r.begin(
        &mut c,
        Request::normal(Some(10_000_000)).with_fault(Fault::Open),
    )
    .unwrap();
    assert!(r.poll(&mut c).is_err());
    let failed_epoch = c.last_issued_epoch();
    assert_eq!(c.state(), ReplacementState::RecoveredMixer);
    r.begin(&mut c, Request::normal(Some(10_000_000))).unwrap();
    let ready = r.poll_ready(&mut c);
    assert_eq!(ready.pause().epoch(), failed_epoch + 1);
    assert_eq!(ready.practice_mixer_basis(), pin);
    drop(publish(&mut r, ready));
    let old_epoch = r.current.authority().epoch().id;
    let first_unsent = r
        .output
        .as_ref()
        .unwrap()
        .source_state()
        .output_frame_basis();
    let mut next = r.controller();
    r.begin(&mut next, Request::normal(Some(2_000_000)))
        .unwrap();
    let ready = r.poll_ready(&mut next);
    assert_eq!(ready.basis(), first_unsent);
    assert_eq!(ready.practice_mixer_basis(), pin);
    assert_eq!(ready.pause().epoch(), old_epoch + 1);
    drop(publish(&mut r, ready));
    assert_eq!(r.practice.generation(), generation);
    assert_eq!(
        r.output
            .as_ref()
            .unwrap()
            .source_state()
            .mixer()
            .playback_frame_cursor(),
        7
    );
    assert_eq!(
        r.output
            .as_ref()
            .unwrap()
            .source_state()
            .mixer()
            .applied_start_frame(),
        Some(2)
    );
}

fn commit_actual_transition(
    r: &mut Rig,
    adoption: crate::playback_pause::PreparedCoordinatePauseAdoption,
    at: ClockPoint,
) -> crate::playback_pause::CoordinatePauseBoundary {
    let authority = r.current.authority_mut();
    authority.record_acquired_prefix(at).unwrap();
    let cut_host = authority
        .presented_coordinate_boundary_host(&adoption.exact_cut(), at)
        .unwrap()
        .unwrap();
    let cut = if adoption.paused() {
        authority.prepare_coordinate_control_cutoff(
            authority.epoch().id,
            adoption.exact_cut(),
            cut_host,
            at,
            &r.inputs,
        )
    } else {
        authority.prepare_coordinate_resume_control_cutoff(
            authority.epoch().id,
            adoption.exact_cut(),
            cut_host,
            at,
            &r.inputs,
        )
    }
    .unwrap()
    .unwrap();
    let boundary = r
        .pause
        .commit_coordinate_adoption(adoption, authority, &cut, at, &r.inputs)
        .unwrap();
    authority
        .commit_coordinate_control_cutoff(cut, at, &r.inputs)
        .unwrap();
    boundary
}

#[test]
fn coordinate_source_replacement_actual_resume_repause_and_practice_keep_original_source_owner() {
    for frequency in [None, Some(10_000_000)] {
        let mut r = Rig::with_frequency(true, frequency.map(|_| OLD_FREQUENCY));
        let generation = r.practice.generation();
        let pin = r.pin;
        let (_c, ready) = r.ready(Request::normal(frequency));
        let basis = ready.basis();
        drop(publish(&mut r, ready));
        let descriptor = test_support::descriptor(basis, frequency);
        // A genuine later paused pull creates a resume frontier beyond the
        // already observed native position. It cannot acknowledge itself.
        {
            let state = r.output.as_mut().unwrap().state.as_mut().unwrap();
            assert!(state.render_pending(2).unwrap().paused);
            state.admit(2).unwrap();
        }
        assert!(r
            .pause
            .request_coordinate(false, r.current.authority())
            .unwrap());
        r.runtime.request_audio_pause(false);
        let active = r
            .output
            .as_mut()
            .unwrap()
            .state
            .as_mut()
            .unwrap()
            .render_pending(4)
            .unwrap();
        assert_eq!(
            (
                active.start_frame,
                active.playback_start_frame,
                active.playback_frames
            ),
            (23, 7, 4)
        );
        assert!(r
            .output
            .as_ref()
            .unwrap()
            .source_state()
            .pending_samples()
            .iter()
            .all(|sample| *sample == 0.25));
        let adoption = r
            .pause
            .prepare_coordinate_adoption(Some(active), None)
            .unwrap()
            .unwrap();
        assert_eq!(r.pause.phase(), PausePhase::Resuming);
        assert!(
            r.current
                .authority()
                .presented_coordinate_boundary_host(&adoption.exact_cut(), now())
                .unwrap()
                .is_none(),
            "rendering cannot invent the later native crossing"
        );
        r.output
            .as_mut()
            .unwrap()
            .state
            .as_mut()
            .unwrap()
            .admit(4)
            .unwrap();
        let frame = 27 - basis.start_physical_frame();
        r.current
            .admit_coordinate(snapshot(
                r.pause.epoch(),
                descriptor,
                units_for_frames(descriptor, frame),
                HOST_START + 30_000,
            ))
            .unwrap();
        let resumed = commit_actual_transition(&mut r, adoption, host(HOST_START + 30_000));
        assert!(!resumed.paused);
        assert_eq!(resumed.playback_frame, 7);
        assert_eq!(resumed.source_identity, r.identity);
        assert_eq!(r.pause.phase(), PausePhase::Running);
        assert!(r
            .pause
            .request_coordinate(true, r.current.authority())
            .unwrap());
        r.runtime.request_audio_pause(true);
        let paused = r
            .output
            .as_mut()
            .unwrap()
            .state
            .as_mut()
            .unwrap()
            .render_pending(4)
            .unwrap();
        assert_eq!(
            (
                paused.start_frame,
                paused.playback_start_frame,
                paused.playback_frames
            ),
            (27, 11, 0)
        );
        assert!(paused.paused);
        let adoption = r
            .pause
            .prepare_coordinate_adoption(Some(paused), None)
            .unwrap()
            .unwrap();
        r.output
            .as_mut()
            .unwrap()
            .state
            .as_mut()
            .unwrap()
            .admit(1)
            .unwrap();
        r.current
            .admit_coordinate(snapshot(
                r.pause.epoch(),
                descriptor,
                units_for_frames(descriptor, 28 - basis.start_physical_frame()),
                HOST_START + 40_000,
            ))
            .unwrap();
        let repaused = commit_actual_transition(&mut r, adoption, host(HOST_START + 40_000));
        assert!(repaused.paused);
        assert_eq!(repaused.playback_frame, 11);
        assert_eq!(r.pause.phase(), PausePhase::Paused);
        r.script.borrow_mut().last_host = HOST_START + 40_000;
        let mut controller = r.controller();
        r.begin(
            &mut controller,
            Request::normal(frequency.map(|_| 2_000_000)),
        )
        .unwrap();
        let ready = r.poll_ready(&mut controller);
        assert_eq!(ready.basis().start_physical_frame(), 28);
        assert_eq!(ready.practice_mixer_basis(), pin);
        drop(publish(&mut r, ready));
        let state = r.output.as_ref().unwrap().source_state();
        assert_eq!(state.mixer().applied_start_frame(), Some(2));
        assert_eq!(state.mixer().playback_frame_cursor(), 11);
        assert_eq!(r.practice.generation(), generation);
        let history = source_history(state);
        assert_eq!((history.initial_physical, history.initial_playback), (5, 3));
        assert!(history
            .spans
            .iter()
            .any(|span| span[0] == 23 && span[3] == 7 && span[4] == 0));
        assert!(history
            .spans
            .iter()
            .any(|span| span[0] == 27 && span[3] == 11 && span[4] == 1));
        // The retained practice producer still accepts its original generation.
        r.practice
            .try_request_next(
                41,
                generation,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(80_000_000), true)
                    .unwrap(),
            )
            .unwrap();
    }
}
