//! Real retained Mixer/converter histories and original input/pause adoption.
//! Scripted native effects exercise policy; they are not physical device evidence.
use super::*;
use crate as runtime;
use crate::{
    audio_authority::AudioAuthorityCoordinateBasis,
    gameplay::output::ports::{SourcePlaybackIdentity, TargetRetirementStatus},
    gameplay_presentation::{GameplayAudioOutputContext, GameplayPauseControl},
    native_audio_startup::new_coordinate_audio_presentation,
    native_end::NativeEnd,
    native_output_coordinate::NativeOutputCoordinate,
};
use beatkernel::{audio::*, input::*, judge::JudgeEngine, time::ClockDomainId};
use beatkernel_platform::audio::{
    DeviceFormat, SampleEncoding, presentation::validation::OriginalNativePresentationEvidence,
};
#[path = "coordinate_target_replacement_test_support.rs"]
mod support;
use support::*;
pub(crate) use support::{Effects, Evidence, Fault, Output, Request, Rig, Ticket};

#[test]
fn coordinate_target_replacement_conserves_original_pcm_histories_pin_and_runtime_for_supported_changes()
 {
    let mut requests = vec![
        Request::normal(TARGET_RATE),
        Request::normal(32_000),
        Request::normal(96_000),
    ];
    let mut channels = Request::normal(TARGET_RATE);
    channels.format = DeviceFormat::new(TARGET_RATE, 2, SampleEncoding::Float32, Some(3)).unwrap();
    channels.matrix = ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap();
    requests.push(channels);
    let mut matrix = Request::normal(TARGET_RATE);
    matrix.matrix = ChannelMatrix::new(1, 1, &[0.5]).unwrap();
    requests.push(matrix);
    let mut encoding = Request::normal(TARGET_RATE);
    encoding.format = DeviceFormat::new(
        TARGET_RATE,
        1,
        SampleEncoding::Pcm {
            container_bits: 24,
            valid_bits: 20,
        },
        None,
    )
    .unwrap();
    requests.push(encoding);
    let mut period = Request::normal(TARGET_RATE);
    period.period = 16;
    requests.push(period);
    let mut buffer = Request::normal(TARGET_RATE);
    buffer.buffer = 64;
    requests.push(buffer);
    for request in requests {
        let mut r = Rig::new(false);
        let before = r.output.as_ref().unwrap().state.as_ref().unwrap();
        let original_position = before.converter_owner().source_position();
        let original_source = source_history(before);
        let original_phases = phase_words(before);
        let frozen = before.mixer().playback_frame_cursor();
        let source_cursor = before.mixer().frame_cursor();
        let frontiers = preserved_frontiers(&r.current);
        let merger = r.inputs.preparation_state().unwrap();
        let transport = format!("{:?}", r.runtime.transport_mut());
        let telemetry = format!("{:?}", r.runtime.telemetry());
        let judge_address = r.runtime.judge() as *const JudgeEngine;
        let commands = r.runtime.admitted_audio_commands();
        let generation = r.practice.generation();
        let rate = request.format.sample_rate();
        let expected_applied = (
            request.format,
            request.period,
            request.buffer,
            request.matrix.coefficients().to_vec(),
        );
        let (_controller, ready) = r.ready(request);
        assert_eq!(ready.practice_mixer_basis(), r.pin);
        assert_eq!(ready.prepared().practice_mixer_basis(), r.pin);
        assert_eq!(
            ready
                .output()
                .state
                .as_ref()
                .unwrap()
                .converter_owner()
                .source_position(),
            original_position
        );
        assert_eq!(
            ready
                .output()
                .state
                .as_ref()
                .unwrap()
                .mixer()
                .frame_cursor(),
            source_cursor
        );
        assert_eq!(
            ready
                .output()
                .state
                .as_ref()
                .unwrap()
                .mixer()
                .playback_frame_cursor(),
            frozen
        );
        assert_eq!(
            source_history(ready.output().state.as_ref().unwrap()),
            original_source
        );
        let phases = phase_words(ready.output().state.as_ref().unwrap());
        assert_eq!(&phases[..original_phases.len()], &original_phases);
        assert_eq!(
            ready.basis().start_time(),
            TargetTime::from_frames(40, TARGET_RATE).unwrap()
        );
        assert_ne!(
            ready.basis().point_at_stream_frame(0).unwrap(),
            r.pin.point_at_stream_frame(0).unwrap()
        );
        let anchors = ready.snapshots();
        let [
            NativeOutputCoordinate::StreamFrame { frame: first },
            NativeOutputCoordinate::StreamFrame { frame: second },
        ] = [anchors[0].coordinate(), anchors[1].coordinate()]
        else {
            panic!("original frame anchors");
        };
        assert!(second > first);
        assert_eq!(
            anchors[1],
            ready.observation().coordinate(),
            "ready publication retains the exact accepted original compound packet"
        );
        let native_id = ready.output().native_id;
        assert_eq!(
            ready
                .observation()
                .playback()
                .native_evidence()
                .unwrap()
                .original_native_words,
            [native_id, second]
        );
        let packet_facts = ready.observation().playback().facts();
        assert_eq!(ready.telemetry().source, packet_facts.source);
        assert_eq!(ready.telemetry().converted, packet_facts.converted);
        assert_eq!(ready.telemetry().facts, packet_facts.boundaries);
        assert_eq!(
            packet_facts.revision,
            ready.output().observations as u64 * 2
        );
        assert_eq!(
            packet_facts.playback.phase.to_words()[7],
            1,
            "original packet describes Held generation"
        );
        assert_eq!(
            packet_facts.playback.playback.state(),
            SourcePlaybackState::Frozen
        );
        let result = r.publish(ready, |ready| {
            let request = &ready.output().request;
            Ok((
                request.format,
                request.period,
                request.buffer,
                request.matrix.coefficients().to_vec(),
            ))
        });
        let (hold, applied) = match result {
            Ok(value) => value,
            Err(error) => panic!("{}", error.error),
        };
        assert_eq!(applied, expected_applied);
        assert_eq!(r.current.authority().epoch().id, OLD_EPOCH + 1);
        assert_eq!(r.pause.epoch(), OLD_EPOCH + 1);
        assert_eq!(r.pause.phase(), PausePhase::Paused);
        assert_eq!(r.current.practice_mixer_basis(), Some(r.pin));
        assert_eq!(preserved_frontiers(&r.current), frontiers);
        assert!(r.inputs.matches_preparation(&merger));
        assert_eq!(r.runtime.judge() as *const JudgeEngine, judge_address);
        assert_eq!(format!("{:?}", r.runtime.transport_mut()), transport);
        assert_eq!(format!("{:?}", r.runtime.telemetry()), telemetry);
        assert_eq!(r.runtime.admitted_audio_commands(), commands);
        assert_eq!(r.practice.generation(), generation);
        assert_eq!(
            r.config.sample_rate, SOURCE_RATE,
            "target rate does not relabel the source grid"
        );
        assert_eq!(
            r.output.as_ref().unwrap().request.format.sample_rate(),
            rate
        );
        assert!(
            r.runtime.hold_audio_pause().is_err(),
            "publication retains the original exclusive hold"
        );
        drop(hold);
        let second_hold = r.runtime.hold_audio_pause().unwrap();
        drop(second_hold);
        // The retained original producer and sample bank still drive the same owner.
        assert!(
            r.pause
                .request_coordinate(false, r.current.authority())
                .unwrap()
        );
        r.runtime.request_audio_pause(false);
        let state = r.output.as_mut().unwrap().state.as_mut().unwrap();
        let resumed = state.render_pending(16).unwrap();
        assert!(!resumed.source.unwrap().paused);
        assert!(state.mixer().playback_frame_cursor() > frozen);
        assert!(
            state.pending_samples().iter().any(|sample| *sample > 0.0),
            "original prepared PCM resumes after replacing the output"
        );
        assert_eq!(r.practice.generation(), generation);
        assert_eq!(
            r.script
                .borrow()
                .calls
                .iter()
                .filter(|call| **call == "open")
                .count(),
            1
        );
    }
}

#[test]
fn coordinate_target_replacement_compatible_pending_suffix_is_not_new_native_generation() {
    let mut r = Rig::new(true);
    let old = r.output.as_ref().unwrap().state.as_ref().unwrap();
    let suffix = old.pending_samples().to_vec();
    let position = old.converter_owner().source_position();
    let source = source_history(old);
    let mut controller = r.controller();
    r.begin(&mut controller, Request::normal(TARGET_RATE))
        .unwrap();
    // Authorization/open may happen in this poll. Its first actual submitted
    // packet is the complete old held suffix, which cannot prime a fresh epoch.
    pending(r.poll(&mut controller).unwrap());
    assert_eq!(r.current.authority().epoch().id, OLD_EPOCH);
    let ready = r.poll_ready(&mut controller);
    assert_eq!(suffix, vec![0.0, 0.0]);
    assert_eq!(
        ready
            .output()
            .state
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position(),
        position
    );
    assert_eq!(
        source_history(ready.output().state.as_ref().unwrap()),
        source
    );
    assert!(
        ready.snapshots().iter().all(|original| {
            let NativeOutputCoordinate::StreamFrame { frame } = original.coordinate() else {
                return false;
            };
            frame > suffix.len() as u64
        }),
        "replaying old held bytes cannot enter either fresh anchor"
    );
    assert_eq!(ready.output().state.as_ref().unwrap().pending_frames(), 0);
    let (output, hold) = ready.into_output_and_hold();
    controller.retire_owned_output_held(output, hold).unwrap();
    let recovered = controller.take_recovered_mixer().unwrap();
    assert_eq!(recovered.converter_owner().source_position(), position);
    assert_eq!(source_history(&recovered), source);
}

#[test]
fn coordinate_target_replacement_conditional_pending_and_refusal_keep_the_same_live_owner_clock_and_epoch()
 {
    for (status, via_cancel) in [
        (TargetRetirementStatus::Refused, false),
        (TargetRetirementStatus::Cancelled, false),
        (TargetRetirementStatus::Refused, true),
        (TargetRetirementStatus::Cancelled, true),
    ] {
        let mut r = Rig::new(true);
        r.script
            .borrow_mut()
            .statuses
            .extend([TargetRetirementStatus::Pending, status]);
        let old = r.output.as_ref().unwrap();
        let id = old.native_id;
        let basis = old.basis;
        let suffix = old.state.as_ref().unwrap().pending_samples().to_vec();
        let position = old
            .state
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position();
        let frontiers = preserved_frontiers(&r.current);
        let mut controller = r.controller();
        r.begin(&mut controller, Request::normal(32_000)).unwrap();
        assert!(!controller.output_clock_suspended());
        assert_eq!(controller.last_issued_epoch(), OLD_EPOCH);
        assert_eq!(controller.conditional_current().unwrap().native_id, id);
        assert!(r.runtime.hold_audio_pause().is_err());
        pending(r.poll(&mut controller).unwrap());
        assert!(!controller.output_clock_suspended());
        let live = controller.conditional_current_mut().unwrap();
        assert!(!live.retired && live.state.is_some());
        assert_eq!(live.basis, basis);
        assert_eq!(live.state.as_ref().unwrap().pending_samples(), suffix);
        assert_eq!(
            controller.probe_coordinate_target_retirement().unwrap(),
            Some(status),
        );
        assert_eq!(r.script.borrow().consumed_tickets.len(), 1);
        let calls_after_probe = r.script.borrow().calls.clone();
        if via_cancel {
            assert!(matches!(
                controller.cancel_coordinate_target_audio().unwrap(),
                CoordinateTargetCancellation::LiveRestored
            ));
        } else {
            assert!(matches!(
                r.poll(&mut controller).unwrap(),
                CoordinateTargetPoll::LiveRefused(_)
            ));
        }
        assert_eq!(
            r.script.borrow().calls,
            calls_after_probe,
            "a consumed terminal probe is used once without polling the stale ticket again"
        );
        assert!(!controller.output_clock_suspended());
        assert_eq!(controller.last_issued_epoch(), OLD_EPOCH);
        let live = controller.take_attached_output().unwrap();
        assert_eq!(live.native_id, id);
        assert!(!live.retired);
        assert_eq!(live.state.as_ref().unwrap().pending_samples(), suffix);
        assert_eq!(
            live.state
                .as_ref()
                .unwrap()
                .converter_owner()
                .source_position(),
            position
        );
        assert_eq!(preserved_frontiers(&r.current), frontiers);
        assert_eq!(r.current.authority().epoch().id, OLD_EPOCH);
        assert_eq!(r.script.borrow().consumed_tickets.len(), 1);
        assert!(!r.script.borrow().calls.contains(&"retire"));
        assert!(!r.script.borrow().calls.contains(&"open"));
        let hold = r.runtime.hold_audio_pause().unwrap();
        drop(hold);
    }

    // Acquisition continues on the original input owner while the worker's
    // conditional decision is pending. HOST orders this payload; it does not
    // substitute for target playback progress.
    let mut r = Rig::new(false);
    r.script.borrow_mut().statuses.extend([
        TargetRetirementStatus::Pending,
        TargetRetirementStatus::Pending,
        TargetRetirementStatus::Authorized,
    ]);
    let original_id = r.output.as_ref().unwrap().native_id;
    let original_pin = r.pin;
    let mut controller = r.controller();
    r.begin(&mut controller, Request::normal(32_000)).unwrap();
    pending(r.poll(&mut controller).unwrap());
    let input = physical(2, HOST_START + 4_000);
    let acquired = host(HOST_START + 10_000);
    r.inputs.admit(input.clone(), acquired).unwrap();
    r.current
        .authority_mut()
        .record_acquired_prefix(acquired)
        .unwrap();
    pending(r.poll(&mut controller).unwrap());
    assert_eq!(r.inputs.pending(), 1);
    assert!(!controller.output_clock_suspended());
    assert_eq!(controller.last_issued_epoch(), OLD_EPOCH);
    assert_eq!(
        controller.conditional_current().unwrap().native_id,
        original_id
    );
    assert!(r.runtime.hold_audio_pause().is_err());
    assert!(!r.script.borrow().calls.contains(&"conditional-cancel"));
    assert!(!r.script.borrow().calls.contains(&"retire"));
    assert!(!r.script.borrow().calls.contains(&"open"));
    pending(r.poll(&mut controller).unwrap());
    assert!(controller.output_clock_suspended());
    assert_eq!(r.current.authority().epoch().id, OLD_EPOCH);
    assert_eq!(r.inputs.pending(), 1);
    pending(r.poll(&mut controller).unwrap());
    assert!(!r.script.borrow().calls.contains(&"conditional-cancel"));
    assert_eq!(r.current.practice_mixer_basis(), Some(original_pin));
    let mapped = r
        .current
        .authority()
        .prepare_coordinate_input(host(HOST_START + 4_000), acquired)
        .unwrap()
        .unwrap();
    assert_eq!(mapped.original(), host(HOST_START + 4_000));
    r.current
        .authority()
        .validate_coordinate_input(&mapped, acquired)
        .unwrap();
    assert_eq!(r.inputs.pop_ready(acquired).unwrap(), Some(input));
    // Paused physical level bookkeeping consumes the original payload without
    // fabricating a judged Runtime operation or relabeling it for the new epoch.
    r.inputs.commit(host(HOST_START + 4_000)).unwrap();
    assert_eq!(r.inputs.pending(), 0);
    let preserved = preserved_frontiers(&r.current);
    let ready = r.poll_ready(&mut controller);
    let (hold, ()) = match r.publish(ready, |_| Ok(())) {
        Ok(value) => value,
        Err(failure) => panic!("{}", failure.error),
    };
    assert_eq!(r.current.authority().epoch().id, OLD_EPOCH + 1);
    assert_eq!(r.current.practice_mixer_basis(), Some(original_pin));
    assert_eq!(preserved_frontiers(&r.current), preserved);
    assert_eq!(
        r.current.authority().committed_input_host(),
        Some(host(HOST_START + 1_000))
    );
    assert_eq!(r.practice.generation(), 1);
    assert_eq!(
        r.script
            .borrow()
            .calls
            .iter()
            .filter(|call| **call == "retire")
            .count(),
        1
    );
    assert_eq!(
        r.script
            .borrow()
            .calls
            .iter()
            .filter(|call| **call == "open")
            .count(),
        1
    );
    assert!(!r.script.borrow().calls.contains(&"conditional-cancel"));
    drop(hold);
}

#[test]
fn coordinate_target_replacement_cancellation_restores_only_a_consumed_matching_live_decision() {
    for status in [
        TargetRetirementStatus::Cancelled,
        TargetRetirementStatus::Authorized,
    ] {
        let mut r = Rig::new(false);
        r.script.borrow_mut().cancel_status = Some(status);
        let id = r.output.as_ref().unwrap().native_id;
        let original_source = source_history(r.output.as_ref().unwrap().state.as_ref().unwrap());
        let original_position = r
            .output
            .as_ref()
            .unwrap()
            .state
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position();
        let original_presentation = format!("{:?}", r.current);
        let mut controller = r.controller();
        r.begin(&mut controller, Request::normal(32_000)).unwrap();
        let result = controller.cancel_coordinate_target_audio().unwrap();
        if status == TargetRetirementStatus::Cancelled {
            assert_eq!(
                r.script.borrow().consumed_tickets.len(),
                1,
                "live cancellation is consumed through matching poll before restoration"
            );
            assert!(matches!(result, CoordinateTargetCancellation::LiveRestored));
            assert!(!controller.output_clock_suspended());
            assert_eq!(controller.take_attached_output().unwrap().native_id, id);
            assert!(!r.script.borrow().calls.contains(&"retire"));
            assert_eq!(controller.last_issued_epoch(), OLD_EPOCH);
        } else {
            assert!(
                r.script.borrow().consumed_tickets.is_empty(),
                "Authorized is a permanent native decision, not a consumed live cancellation"
            );
            assert!(matches!(result, CoordinateTargetCancellation::Retired));
            assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
            assert!(
                controller.take_attached_output().is_none(),
                "authorized native retirement cannot be relabeled as restored live output"
            );
            assert!(controller.conditional_current().is_none());
            assert!(controller.conditional_current_mut().is_none());
            let recovered = controller.take_recovered_mixer().unwrap();
            assert_eq!(source_history(&recovered), original_source);
            assert_eq!(
                recovered.converter_owner().source_position(),
                original_position
            );
            assert!(recovered.phase_history().is_some());
            assert!(
                recovered
                    .converter_owner()
                    .source_playback_history()
                    .is_some()
            );
            assert!(r.script.borrow().calls.contains(&"retire"));
            assert!(!r.script.borrow().calls.contains(&"open"));
            assert_eq!(controller.last_issued_epoch(), OLD_EPOCH);
        }
        assert_eq!(format!("{:?}", r.current), original_presentation);
        let hold = r.runtime.hold_audio_pause().unwrap();
        drop(hold);
    }

    for cancel_after_probe in [false, true] {
        let mut r = Rig::new(false);
        let original_source = source_history(r.output.as_ref().unwrap().state.as_ref().unwrap());
        r.script
            .borrow_mut()
            .statuses
            .push_back(TargetRetirementStatus::Authorized);
        let mut controller = r.controller();
        r.begin(&mut controller, Request::normal(32_000)).unwrap();
        assert!(!controller.output_clock_suspended());
        assert_eq!(
            controller.probe_coordinate_target_retirement().unwrap(),
            Some(TargetRetirementStatus::Authorized)
        );
        assert_eq!(controller.state(), ReplacementState::ConditionalRetirement);
        assert!(controller.output_clock_suspended());
        assert!(controller.conditional_current().is_none());
        assert!(controller.conditional_current_mut().is_none());
        assert_eq!(controller.last_issued_epoch(), OLD_EPOCH);
        assert_eq!(r.current.authority().epoch().id, OLD_EPOCH);
        assert!(
            r.script.borrow().consumed_tickets.is_empty(),
            "a probed Authorized decision stays permanent until native owner recovery"
        );
        assert!(r.runtime.hold_audio_pause().is_err());
        assert!(!r.script.borrow().calls.contains(&"retire"));
        assert!(!r.script.borrow().calls.contains(&"open"));
        if cancel_after_probe {
            assert!(matches!(
                controller.cancel_coordinate_target_audio().unwrap(),
                CoordinateTargetCancellation::Retired
            ));
            assert_eq!(
                controller.last_issued_epoch(),
                OLD_EPOCH,
                "cancelling cached authorization recovers without creating a new epoch"
            );
            assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
            assert!(controller.take_attached_output().is_none());
            assert!(controller.conditional_current().is_none());
            assert!(controller.conditional_current_mut().is_none());
            let recovered = controller.take_recovered_mixer().unwrap();
            assert_eq!(source_history(&recovered), original_source);
            assert!(controller.take_recovered_mixer().is_none());
            assert!(!r.script.borrow().calls.contains(&"open"));
        } else {
            let ready = r.poll_ready(&mut controller);
            assert_eq!(ready.pause().epoch(), OLD_EPOCH + 1);
            assert_eq!(
                source_history(ready.output().state.as_ref().unwrap()),
                original_source
            );
            assert_eq!(
                r.script
                    .borrow()
                    .calls
                    .iter()
                    .filter(|call| **call == "open")
                    .count(),
                1
            );
            let (output, hold) = ready.into_output_and_hold();
            controller.retire_owned_output_held(output, hold).unwrap();
            assert!(controller.take_recovered_mixer().is_some());
        }
        assert_eq!(
            r.script
                .borrow()
                .calls
                .iter()
                .filter(|call| **call == "conditional-poll")
                .count(),
            1
        );
        assert!(
            !r.script.borrow().calls.contains(&"conditional-cancel"),
            "cached authorization recovers without cancelling the permanent native decision"
        );
        assert!(r.script.borrow().consumed_tickets.is_empty());
        let hold = r.runtime.hold_audio_pause().unwrap();
        drop(hold);
    }
}

#[test]
fn coordinate_target_replacement_cold_guards_refuse_before_native_retirement() {
    for variant in 0..10 {
        let mut r = Rig::new(variant >= 8);
        let mut request = Request::normal(32_000);
        let mut identity = r.identity;
        let mut practice_pending = false;
        match variant {
            0 => {
                r.end =
                    Some(NativeEnd::new(point(2, 0), ClockDomainId(1), SOURCE_RATE, 100).unwrap());
            }
            1 => {
                assert!(
                    r.pause
                        .request_coordinate(false, r.current.authority())
                        .unwrap()
                );
            }
            2 => {
                r.inputs
                    .admit(physical(2, HOST_START + 6_000), now())
                    .unwrap();
            }
            3 => {
                practice_pending = true;
            }
            4 => {
                identity = SourcePlaybackIdentity::new(44_100, 0, 0).unwrap();
            }
            5 => {
                r.output.as_mut().unwrap().basis = TargetFrameBasis::new(
                    point(2, 0),
                    TargetTime::from_frames(0, 1).unwrap(),
                    44_100,
                )
                .unwrap();
            }
            6 => {
                request.period = 0;
            }
            7 => {
                request.fault = Fault::Submit;
            }
            8 => {
                identity = SourcePlaybackIdentity::new(SOURCE_RATE, 1, 0).unwrap();
            }
            9 => {
                identity = SourcePlaybackIdentity::new(SOURCE_RATE, 1, 1).unwrap();
            }
            _ => unreachable!(),
        }
        let old_id = r.output.as_ref().unwrap().native_id;
        let old_state = r.output.as_ref().unwrap().state.as_ref().unwrap();
        let old_samples = old_state.pending_samples().to_vec();
        let old_position = old_state.converter_owner().source_position();
        let old_source_history = source_history(old_state);
        let before = format!("{:?}", r.current);
        let mut controller = r.controller();
        let result = controller.begin_coordinate_target_audio(
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
        assert!(result.is_err(), "cold guard {variant} must refuse");
        assert_eq!(format!("{:?}", r.current), before);
        assert_eq!(controller.last_issued_epoch(), OLD_EPOCH);
        let live = controller.take_attached_output().unwrap();
        assert_eq!(live.native_id, old_id);
        assert!(!live.retired);
        assert_eq!(live.state.as_ref().unwrap().pending_samples(), old_samples);
        assert_eq!(
            live.state
                .as_ref()
                .unwrap()
                .converter_owner()
                .source_position(),
            old_position
        );
        assert_eq!(
            source_history(live.state.as_ref().unwrap()),
            old_source_history
        );
        assert!(!r.script.borrow().calls.contains(&"retire"));
        assert!(!r.script.borrow().calls.contains(&"open"));
        if variant <= 5 || variant >= 8 {
            assert!(
                r.script.borrow().calls.is_empty(),
                "invalid original retained identity is cold"
            );
        }
        let hold = r.runtime.hold_audio_pause().unwrap();
        drop(hold);
    }
}

#[test]
fn coordinate_target_replacement_pending_cancellation_keeps_live_ownership_until_matching_terminal_acknowledgement()
 {
    let mut r = Rig::new(true);
    r.script.borrow_mut().cancel_status = Some(TargetRetirementStatus::Pending);
    let id = r.output.as_ref().unwrap().native_id;
    let mut controller = r.controller();
    r.begin(&mut controller, Request::normal(32_000)).unwrap();
    assert!(matches!(
        controller.cancel_coordinate_target_audio().unwrap(),
        CoordinateTargetCancellation::Pending
    ));
    assert!(!controller.output_clock_suspended());
    assert_eq!(controller.conditional_current().unwrap().native_id, id);
    assert_eq!(controller.last_issued_epoch(), OLD_EPOCH);
    assert!(r.script.borrow().consumed_tickets.is_empty());
    assert!(r.runtime.hold_audio_pause().is_err());
    r.script.borrow_mut().cancel_status = Some(TargetRetirementStatus::Cancelled);
    assert!(matches!(
        controller.cancel_coordinate_target_audio().unwrap(),
        CoordinateTargetCancellation::LiveRestored
    ));
    assert_eq!(controller.take_attached_output().unwrap().native_id, id);
    assert_eq!(r.script.borrow().consumed_tickets.len(), 1);
    assert!(!r.script.borrow().calls.contains(&"retire"));
    let hold = r.runtime.hold_audio_pause().unwrap();
    drop(hold);
}

#[test]
fn coordinate_target_replacement_epoch_exhaustion_is_cold_and_does_not_acquire_hold() {
    let mut r = Rig::new_at_epoch(false, u64::MAX);
    let mut controller = r.controller();
    let before = format!("{:?}", r.current);
    let mut acquired = false;
    let result = controller.begin_coordinate_target_audio(
        Request::normal(32_000),
        &r.current,
        &r.pause,
        None,
        &r.inputs,
        r.identity,
        false,
        100_000_000,
        || {
            acquired = true;
            r.runtime.hold_audio_pause()
        },
    );
    let failure = match result {
        Err(f) => f,
        Ok(()) => panic!("exhausted output epoch must refuse"),
    };
    assert!(failure.to_string().contains("exhaust"));
    assert!(!acquired);
    assert_eq!(format!("{:?}", r.current), before);
    assert_eq!(controller.last_issued_epoch(), u64::MAX);
    assert_eq!(controller.take_attached_output().unwrap().epoch, u64::MAX);
    assert!(r.script.borrow().calls.is_empty());
    let hold = r.runtime.hold_audio_pause().unwrap();
    drop(hold);
}

#[test]
fn coordinate_target_replacement_effect_failures_recover_the_unique_complete_owner() {
    for fault in [Fault::Plan, Fault::Open, Fault::Held, Fault::Start] {
        let mut r = Rig::new(false);
        let old = r.output.as_ref().unwrap().state.as_ref().unwrap();
        let position = old.converter_owner().source_position();
        let histories = source_history(old);
        let original_phases = phase_words(old);
        let frontiers = preserved_frontiers(&r.current);
        let mut request = Request::normal(32_000);
        request.fault = fault;
        let mut controller = r.controller();
        r.begin(&mut controller, request).unwrap();
        let failure = r.poll_failure(&mut controller);
        assert!(
            failure.to_string().contains("original"),
            "{fault:?}: {failure}"
        );
        assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
        assert!(controller.take_attached_output().is_none());
        assert!(controller.conditional_current().is_none());
        assert!(controller.conditional_current_mut().is_none());
        let state = controller.take_recovered_mixer().unwrap();
        assert_eq!(state.converter_owner().source_position(), position);
        assert_eq!(source_history(&state), histories);
        assert_eq!(phase_words(&state), original_phases);
        assert_eq!(preserved_frontiers(&r.current), frontiers);
        assert_eq!(r.current.authority().epoch().id, OLD_EPOCH);
        assert!(
            controller.take_recovered_mixer().is_none(),
            "owner cannot be recovered twice"
        );
        let hold = r.runtime.hold_audio_pause().unwrap();
        drop(hold);
        let dropped = &r.script.borrow().dropped;
        let mut unique = dropped.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            dropped.len(),
            "each native shell is destroyed once"
        );
    }
}

#[test]
fn coordinate_target_replacement_retire_and_recovery_errors_retain_hold_until_explicit_cleanup() {
    for (fault, cached) in [
        (Fault::Retire, false),
        (Fault::Recover, false),
        (Fault::Retire, true),
        (Fault::Recover, true),
    ] {
        let mut r = Rig::new(false);
        r.output.as_mut().unwrap().request.fault = fault;
        let position = r
            .output
            .as_ref()
            .unwrap()
            .state
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position();
        let mut controller = r.controller();
        r.begin(&mut controller, Request::normal(32_000)).unwrap();
        if cached {
            assert_eq!(
                controller.probe_coordinate_target_retirement().unwrap(),
                Some(TargetRetirementStatus::Authorized)
            );
            assert!(controller.output_clock_suspended());
            assert!(controller.conditional_current().is_none());
        }
        let failure = match r.poll(&mut controller) {
            Err(f) => f,
            _ => panic!("original stop/recovery must fail"),
        };
        assert!(failure.to_string().contains("original"));
        assert_eq!(controller.state(), ReplacementState::PendingRetirement);
        assert!(controller.output_clock_suspended());
        assert!(controller.take_attached_output().is_none());
        assert!(controller.take_recovered_mixer().is_none());
        assert!(
            r.runtime.hold_audio_pause().is_err(),
            "pending technical retirement owns the hold"
        );
        assert!(controller.stop_owned().unwrap());
        let state = controller.take_recovered_mixer().unwrap();
        assert_eq!(state.converter_owner().source_position(), position);
        assert_eq!(
            r.script
                .borrow()
                .calls
                .iter()
                .filter(|call| **call == "conditional-poll")
                .count(),
            1
        );
        assert!(!r.script.borrow().calls.contains(&"conditional-cancel"));
        let hold = r.runtime.hold_audio_pause().unwrap();
        drop(hold);
    }
}

#[test]
fn coordinate_target_replacement_partial_open_returns_original_error_and_pending_owner_hold() {
    let mut r = Rig::new(false);
    let position = r
        .output
        .as_ref()
        .unwrap()
        .state
        .as_ref()
        .unwrap()
        .converter_owner()
        .source_position();
    let histories = source_history(r.output.as_ref().unwrap().state.as_ref().unwrap());
    let mut controller = r.controller();
    r.begin(
        &mut controller,
        Request::normal(32_000).with_fault(Fault::PartialOpen),
    )
    .unwrap();
    let failure = r.poll_failure(&mut controller);
    assert!(
        failure
            .to_string()
            .contains("original partial open failure")
    );
    assert!(
        failure
            .cleanup
            .as_ref()
            .unwrap()
            .to_string()
            .contains("original partial open cleanup")
    );
    assert_eq!(controller.state(), ReplacementState::PendingRetirement);
    assert!(controller.output_clock_suspended());
    assert!(controller.take_attached_output().is_none());
    assert!(controller.take_recovered_mixer().is_none());
    assert!(r.runtime.hold_audio_pause().is_err());
    assert!(controller.stop_owned().unwrap());
    let state = controller.take_recovered_mixer().unwrap();
    assert_eq!(state.converter_owner().source_position(), position);
    assert_eq!(source_history(&state), histories);
    assert!(controller.take_recovered_mixer().is_none());
    let hold = r.runtime.hold_audio_pause().unwrap();
    drop(hold);
}

#[test]
fn coordinate_target_replacement_observation_failures_never_publish_or_lose_full_recovery() {
    for fault in [
        Fault::Observe,
        Fault::WrongDomain,
        Fault::WrongEpoch,
        Fault::ChangedBasis,
        Fault::ChangedAfterRead,
        Fault::RunningSource,
        Fault::ChangedBoundary,
        Fault::ActivePhase,
        Fault::Unsubmitted,
        Fault::InvalidPacket,
    ] {
        let mut r = Rig::new(false);
        let original = preserved_frontiers(&r.current);
        let mut request = Request::normal(32_000);
        request.fault = fault;
        let mut controller = r.controller();
        r.begin(&mut controller, request).unwrap();
        let failure = r.poll_failure(&mut controller);
        assert!(!failure.to_string().is_empty());
        assert_eq!(preserved_frontiers(&r.current), original);
        assert_eq!(r.current.authority().epoch().id, OLD_EPOCH);
        assert!(r.output.is_none());
        let state = controller.take_recovered_mixer().unwrap();
        assert!(state.phase_history().is_some());
        assert!(state.converter_owner().source_playback_history().is_some());
        assert!(controller.take_recovered_mixer().is_none());
        let hold = r.runtime.hold_audio_pause().unwrap();
        drop(hold);
    }
}

#[test]
fn coordinate_target_replacement_missing_duplicate_and_future_packets_cannot_supply_two_anchors() {
    for fault in [Fault::Missing, Fault::Duplicate, Fault::Future] {
        let mut r = Rig::new(false);
        let original = format!("{:?}", r.current);
        let mut request = Request::normal(32_000);
        request.fault = fault;
        let mut controller = r.controller();
        r.begin(&mut controller, request).unwrap();
        for _ in 0..3 {
            match r.poll(&mut controller) {
                Ok(result) => pending(result),
                Err(_) if fault == Fault::Future => break,
                Err(error) => panic!("incomplete {fault:?} evidence should wait: {error}"),
            }
            assert_eq!(format!("{:?}", r.current), original);
        }
        assert_eq!(r.current.authority().epoch().id, OLD_EPOCH);
        assert!(r.output.is_none());
        let _ = controller.cancel_coordinate_target_audio();
    }
}

#[test]
fn coordinate_target_replacement_missing_native_timeout_recovers_software_without_inventing_a_live_clock()
 {
    let mut r = Rig::new(false);
    let original = format!("{:?}", r.current);
    let old_state = r.output.as_ref().unwrap().state.as_ref().unwrap();
    let original_source = source_history(old_state);
    let original_phases = phase_words(old_state);
    let original_position = old_state.converter_owner().source_position();
    let mut controller = r.controller();
    r.begin(
        &mut controller,
        Request::normal(32_000).with_fault(Fault::Missing),
    )
    .unwrap();
    pending(r.poll(&mut controller).unwrap());
    assert_eq!(controller.state(), ReplacementState::Waiting);
    pending(r.poll(&mut controller).unwrap());
    assert_eq!(
        r.script
            .borrow()
            .calls
            .iter()
            .filter(|call| **call == "packet")
            .count(),
        1,
        "the candidate's actual Missing observation starts its bounded wait"
    );
    let result = controller.poll_coordinate_target_audio(
        host(now().timestamp.as_nanos() + 100_000_000),
        &r.current,
        &r.pause,
        &r.inputs,
    );
    let failure = match result {
        Err(f) => f,
        _ => panic!("bounded native observation wait must expire"),
    };
    assert!(failure.to_string().contains("timed out"));
    assert_eq!(format!("{:?}", r.current), original);
    assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
    assert!(controller.take_attached_output().is_none());
    assert!(controller.conditional_current().is_none());
    assert!(controller.conditional_current_mut().is_none());
    let recovered = controller.take_recovered_mixer().unwrap();
    assert_eq!(source_history(&recovered), original_source);
    assert_eq!(phase_words(&recovered), original_phases);
    assert_eq!(
        recovered.converter_owner().source_position(),
        original_position
    );
    assert!(controller.take_recovered_mixer().is_none());
    assert!(failure.cleanup.is_none() && failure.recovery.is_none());
    let hold = r.runtime.hold_audio_pause().unwrap();
    drop(hold);
}

#[test]
fn coordinate_target_replacement_exact_native_units_progress_even_with_equal_rounded_nanoseconds() {
    let rate = 1_073_741_823;
    let mut r = Rig::new(false);
    r.script.borrow_mut().render_frames.extend([4, 1]);
    let (_controller, ready) = r.ready(Request::normal(rate));
    let originals = ready.snapshots();
    assert_eq!(
        originals[0].coordinate(),
        NativeOutputCoordinate::StreamFrame { frame: 4 }
    );
    assert_eq!(
        originals[1].coordinate(),
        NativeOutputCoordinate::StreamFrame { frame: 5 }
    );
    let OriginalNativePresentationEvidence::SuppliedPair(first) = originals[0].snapshot().evidence
    else {
        unreachable!()
    };
    let OriginalNativePresentationEvidence::SuppliedPair(second) = originals[1].snapshot().evidence
    else {
        unreachable!()
    };
    assert_eq!(
        first.source.timestamp, second.source.timestamp,
        "actual supported held PCM has exact sub-ns progress"
    );
    assert!(first.target.timestamp < second.target.timestamp);
    assert_eq!(
        ready
            .output()
            .state
            .as_ref()
            .unwrap()
            .format()
            .sample_rate(),
        rate
    );
    assert_eq!(ready.output().state.as_ref().unwrap().pending_frames(), 0);
    let numerator = 40_u64 * u64::from(rate) + 5 * u64::from(TARGET_RATE);
    let denominator = u64::from(TARGET_RATE) * u64::from(rate);
    assert_eq!(
        ready
            .output()
            .state
            .as_ref()
            .unwrap()
            .converter_owner()
            .target_time(),
        TargetTime::new(0, numerator, denominator).unwrap()
    );
    let (hold, ()) = match r.publish(ready, |_| Ok(())) {
        Ok(v) => v,
        Err(f) => panic!("{}", f.error),
    };
    assert_eq!(r.current.authority().history_len(), 2);
    assert_eq!(
        r.current
            .authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 5 })
    );
    drop(hold);
}

#[test]
fn coordinate_target_replacement_ready_publication_revalidates_current_pause_config_and_exact_merger()
 {
    for variant in 0..8 {
        let mut r = Rig::new(false);
        let (mut controller, ready) = r.ready(Request::normal(32_000));
        let candidate_id = ready.output().native_id;
        match variant {
            0 => {
                r.current
                    .authority_mut()
                    .record_acquired_prefix(now())
                    .unwrap();
            }
            1 => {
                r.inputs
                    .admit(physical(2, HOST_START + 6_000), now())
                    .unwrap();
                assert!(r.inputs.pop_ready(now()).unwrap().is_some());
                assert_eq!(
                    r.inputs.pending(),
                    0,
                    "empty queue is still semantically changed"
                );
            }
            2 => {
                r.inputs.commit(host(HOST_START + 6_000)).unwrap();
            }
            3 => {
                r.inputs.register_source(DeviceId(10)).unwrap();
            }
            4 => {
                r.config.sample_rate += 1;
            }
            5 => {
                assert!(
                    r.pause
                        .request_coordinate(false, r.current.authority())
                        .unwrap()
                );
            }
            6 => {
                r.end =
                    Some(NativeEnd::new(point(2, 0), ClockDomainId(1), SOURCE_RATE, 100).unwrap());
            }
            7 => {
                let basis = r.current.target_basis().unwrap();
                let mut changed = new_coordinate_audio_presentation(
                    OLD_EPOCH,
                    AudioAuthorityCoordinateBasis::TargetFrame(basis),
                    ClockDomainId(1),
                    point(33, 5_000_000_000),
                    r.current.authority().config(),
                )
                .unwrap();
                changed
                    .admit_target_coordinate(snapshot(OLD_EPOCH, basis, 0, HOST_START))
                    .unwrap();
                changed
                    .pin_practice_mixer_basis(
                        OutputFrameBasis::new(point(2, 0), 44_100, 0).unwrap(),
                    )
                    .unwrap();
                r.current = changed;
            }
            _ => unreachable!(),
        }
        let current = format!("{:?}", r.current);
        let pause = format!("{:?}", r.pause);
        let config = format!("{:?}", r.config);
        let failure = match r.publish(ready, |_| Ok(())) {
            Err(f) => f,
            Ok(_) => panic!("stale publication variant {variant} must refuse"),
        };
        assert_eq!(failure.ready.output().native_id, candidate_id);
        assert_eq!(format!("{:?}", r.current), current);
        assert_eq!(format!("{:?}", r.pause), pause);
        assert_eq!(format!("{:?}", r.config), config);
        assert!(r.output.is_none());
        assert!(
            r.runtime.hold_audio_pause().is_err(),
            "refusal returns the same exclusive ready hold"
        );
        let (output, hold) = failure.ready.into_output_and_hold();
        controller.retire_owned_output_held(output, hold).unwrap();
        assert!(controller.take_recovered_mixer().is_some());
        let hold = r.runtime.hold_audio_pause().unwrap();
        drop(hold);
    }
}

#[test]
fn coordinate_target_replacement_ready_native_age_and_occupied_output_refuse_atomically() {
    for occupied in [false, true] {
        let mut r = Rig::new(false);
        let (mut controller, ready) = r.ready(Request::normal(32_000));
        let candidate_id = ready.output().native_id;
        let before = format!("{:?}", r.current);
        let pause = format!("{:?}", r.pause);
        let config = format!("{:?}", r.config);
        if occupied {
            let mut other = Rig::new(false);
            r.output = other.output.take();
        }
        let at = if occupied {
            now()
        } else {
            host(now().timestamp.as_nanos() + 200_000_000)
        };
        let result = publish_ready_coordinate_target_audio_output_held(
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
        );
        let failure = match result {
            Err(f) => f,
            Ok(_) => panic!("stale or occupied publication must refuse"),
        };
        assert_eq!(failure.ready.output().native_id, candidate_id);
        assert_eq!(format!("{:?}", r.current), before);
        assert_eq!(format!("{:?}", r.pause), pause);
        assert_eq!(format!("{:?}", r.config), config);
        assert_eq!(r.output.is_some(), occupied);
        assert!(r.runtime.hold_audio_pause().is_err());
        let (output, hold) = failure.ready.into_output_and_hold();
        controller.retire_owned_output_held(output, hold).unwrap();
        assert!(controller.take_recovered_mixer().is_some());
    }
}

#[test]
fn coordinate_target_replacement_capability_stage_failure_precedes_every_authoritative_commit_and_allows_same_ready_retry()
 {
    let mut r = Rig::new(false);
    let (_controller, ready) = r.ready(Request::normal(32_000));
    let candidate = ready.output().native_id;
    let original = format!("{:?}", r.current);
    let pause = format!("{:?}", r.pause);
    let config = format!("{:?}", r.config);
    let calls = r.script.borrow().calls.clone();
    let failure = match r.publish::<()>(ready, |ready| {
        assert_eq!(ready.output().request.format.sample_rate(), 32_000);
        Err("applied capability staging refused".into())
    }) {
        Err(f) => f,
        Ok(_) => panic!("precommit capability stage must refuse"),
    };
    assert_eq!(
        failure.error.to_string(),
        "applied capability staging refused"
    );
    assert_eq!(format!("{:?}", r.current), original);
    assert_eq!(format!("{:?}", r.pause), pause);
    assert_eq!(format!("{:?}", r.config), config);
    assert!(r.output.is_none());
    assert_eq!(failure.ready.output().native_id, candidate);
    assert_eq!(r.script.borrow().calls, calls);
    assert!(r.runtime.hold_audio_pause().is_err());
    let (hold, applied) = match r.publish(failure.ready, |ready| Ok(ready.output().request.format))
    {
        Ok(value) => value,
        Err(f) => panic!("{}", f.error),
    };
    assert_eq!(applied.sample_rate(), 32_000);
    assert_eq!(r.current.authority().epoch().id, OLD_EPOCH + 1);
    assert_eq!(r.output.as_ref().unwrap().native_id, candidate);
    assert_eq!(
        r.script.borrow().calls,
        calls,
        "capability retry never repeats native effects"
    );
    drop(hold);
}

#[test]
fn coordinate_target_replacement_failed_open_can_retry_with_same_full_source_owner_and_new_checked_epoch()
 {
    let mut r = Rig::new(false);
    let basis = r.output.as_ref().unwrap().basis;
    let position = r
        .output
        .as_ref()
        .unwrap()
        .state
        .as_ref()
        .unwrap()
        .converter_owner()
        .source_position();
    let mut controller = r.controller();
    let mut request = Request::normal(32_000);
    request.fault = Fault::Open;
    r.begin(&mut controller, request).unwrap();
    assert!(r.poll(&mut controller).is_err());
    let failed_epoch = controller.last_issued_epoch();
    let state = controller.take_recovered_mixer().unwrap();
    assert_eq!(state.converter_owner().source_position(), position);
    assert!(
        controller
            .attach(native(
                state,
                basis,
                OLD_EPOCH,
                Request::normal(TARGET_RATE),
                &r.script
            ))
            .is_ok()
    );
    r.begin(&mut controller, Request::normal(32_000)).unwrap();
    let ready = r.poll_ready(&mut controller);
    assert_eq!(ready.pause().epoch(), failed_epoch + 1);
    assert_eq!(
        ready
            .output()
            .state
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position(),
        position
    );
    assert_eq!(ready.practice_mixer_basis(), r.pin);
    assert_eq!(r.practice.generation(), 1);
    let (hold, ()) = match r.publish(ready, |_| Ok(())) {
        Ok(v) => v,
        Err(f) => panic!("{}", f.error),
    };
    assert_eq!(r.current.authority().epoch().id, failed_epoch + 1);
    drop(hold);
}
