//! Static remix forwarding keeps non-Copy native evidence and exact packets intact.
use super::*;
use crate::gameplay::output::{
    application::owner::source_playback_owner_fixtures as memory,
    ports::{
        source_conditional_retirement_packet_fixtures as contract,
        ConditionalSourceRetirementNativeOutputBackend, SourceHistoryRetirementNativeOutputBackend,
        SourcePlaybackNativeOutputBackend, SourceRetirementStatus,
    },
};
use beatkernel::audio::{ChannelMatrix, StoppedMixerSource};
use beatkernel_platform::audio::NativeOutputState;
use std::rc::Rc;

#[test]
fn remix_forwards_coherent_observation_without_copy_clone_or_separate_observer() {
    let (backend, output, io, _producer) = memory::remix_fixture();
    let basis = output.basis;
    let mut remixed = RemixedOutputBackend::new(backend);
    let mut output = output;
    let observation = memory::observation(basis, 2, 100, output.report);
    let pointer = observation
        .source()
        .native_evidence()
        .unwrap()
        .words
        .as_ptr();
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(observation)));
    let got = remixed
        .observe_native_playback(&mut output, memory::identity())
        .unwrap()
        .unwrap();
    assert_eq!(
        got.source().native_evidence().unwrap().words.as_ptr(),
        pointer
    );
    assert_eq!(got.source().native_evidence().unwrap().words, [19, 31]);
    assert_eq!(got.source().facts().render, output.report);
    assert_eq!(io.observed_calls.get(), 1);
    assert_eq!(io.report_calls.get(), 0);
    assert_eq!(io.legacy_calls.get(), 0);
}
#[test]
fn remix_forwards_readonly_exact_query_and_missing_packet_verbatim() {
    let (backend, output, io, _producer) = memory::remix_fixture();
    let basis = output.basis;
    let remixed = RemixedOutputBackend::new(backend);
    let q = memory::query(3, basis.sample_rate());
    let packet = memory::packet(basis, 7, memory::identity(), q, output.report);
    let pointer = packet.native_evidence().unwrap().words.as_ptr();
    io.queries.borrow_mut().push_back(Ok(Some(packet)));
    let got = remixed
        .source_playback_at(&output, &q, memory::identity())
        .unwrap()
        .unwrap();
    assert_eq!(
        got.query().compare_interpolated_time(q),
        std::cmp::Ordering::Equal
    );
    assert_eq!(got.native_evidence().unwrap().words.as_ptr(), pointer);
    assert!(remixed
        .source_playback_at(&output, &q, memory::identity())
        .unwrap()
        .is_none());
    assert_eq!(io.query_calls.get(), 2);
    assert_eq!(io.report_calls.get(), 0);
}

#[test]
fn remix_created_source_descriptor_and_counter_packets_preserve_original_units() {
    for frequency in [None, Some(10_000_003), Some(u64::MAX)] {
        let (backend, mut output, effects, _producer) = contract::fixture(frequency);
        let mut remixed = RemixedOutputBackend::new(backend);
        let descriptor = output.descriptor;
        assert_eq!(
            remixed.created_source_coordinate_basis(&output).unwrap(),
            descriptor
        );
        assert!(remixed
            .observe_native_playback(&mut output, memory::identity())
            .unwrap()
            .is_none());
        let native = contract::native(&output, 1);
        // The smaller Counter frequency would be beyond this tiny real history.
        // Created metadata alone still succeeds and grants no playback packet.
        if frequency != Some(10_000_003) {
            let output_pointer = &output as *const contract::Output as *const ();
            let packet = contract::packet(&output, native);
            let query = packet.query();
            let pointer = packet.native_evidence().unwrap().words.as_ptr();
            effects.observations.borrow_mut().push_back(Ok(Some(
                crate::gameplay::output::ports::NativePlaybackObservation::new(native, packet)
                    .unwrap(),
            )));
            let got = remixed
                .observe_native_playback(&mut output, memory::identity())
                .unwrap()
                .unwrap();
            assert_eq!(got.coordinate(), native);
            assert_eq!(
                got.source().native_evidence().unwrap().words.as_ptr(),
                pointer
            );
            let packet = contract::packet(&output, native);
            let pointer = packet.native_evidence().unwrap().words.as_ptr();
            effects.queries.borrow_mut().push_back(Ok(Some(packet)));
            let got = remixed
                .source_playback_at(&output, &query, memory::identity())
                .unwrap()
                .unwrap();
            assert_eq!(got.native_evidence().unwrap().words.as_ptr(), pointer);
            assert!(got.query().compare_interpolated_time(query).is_eq());
            assert_eq!(
                &*effects.observed_inputs.borrow(),
                &[
                    (output_pointer, memory::identity()),
                    (output_pointer, memory::identity()),
                ]
            );
            let inputs = effects.queried_inputs.borrow();
            assert_eq!(inputs.len(), 1);
            assert_eq!(inputs[0].0, output_pointer);
            assert!(inputs[0].1.compare_interpolated_time(query).is_eq());
            assert_eq!(inputs[0].2, memory::identity());
        }
        let error = contract::Fault::new(61);
        let pointer = error.pointer();
        *effects.created_error.borrow_mut() = Some(error);
        assert_eq!(
            remixed
                .created_source_coordinate_basis(&output)
                .unwrap_err()
                .pointer(),
            pointer
        );
        assert_eq!(effects.created_calls.get(), 2);
        assert_eq!(effects.retire_calls.get(), 0);
    }
}

#[test]
fn remix_conditional_submission_preserves_native_request_matrix_and_opaque_ticket() {
    let (backend, mut output, effects, _producer) = contract::fixture(Some(u64::MAX));
    let output_pointer = &output as *const contract::Output as *const ();
    let mut remixed = RemixedOutputBackend::new(backend);
    let matrix = ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap();
    let override_matrix = ChannelMatrix::new(1, 2, &[0.25, -0.5]).unwrap();
    let request = crate::gameplay::output::domain::remix::RemixedOutputRequest::remixed(
        contract::Request(Box::new(91)),
        matrix,
    );
    let request_pointer: *const u64 = request.native.0.as_ref();
    let ticket = contract::Ticket(Box::new([19, 23]));
    let ticket_pointer = ticket.pointer();
    effects.submitted.borrow_mut().push_back(Ok(Some(ticket)));
    let ticket = remixed
        .submit_source_conditional_retirement(&mut output, &request, None)
        .unwrap()
        .unwrap();
    assert_eq!(ticket.pointer(), ticket_pointer);
    // A busy slot must remain a busy result, retaining the caller's first ticket.
    effects.submitted.borrow_mut().push_back(Ok(None));
    assert!(remixed
        .submit_source_conditional_retirement(&mut output, &request, Some(&override_matrix))
        .unwrap()
        .is_none());
    let unconsumed = contract::Fault::new(92);
    let error_pointer = unconsumed.pointer();
    effects.submitted.borrow_mut().push_back(Err(unconsumed));
    let strict = crate::gameplay::output::domain::remix::RemixedOutputRequest::strict(
        contract::Request(Box::new(93)),
    );
    let strict_pointer: *const u64 = strict.native.0.as_ref();
    let error = match remixed.submit_source_conditional_retirement(&mut output, &strict, None) {
        Err(error) => error,
        Ok(_) => panic!("unconsumed refusal must remain an error"),
    };
    assert_eq!(error.pointer(), error_pointer);
    assert_eq!(
        &*effects.submissions.borrow(),
        &[
            (request_pointer, Some(vec![1.0, 0.5])),
            (request_pointer, Some(vec![0.25, -0.5])),
            (strict_pointer, None),
        ]
    );
    effects.polled.borrow_mut().extend([
        Ok(SourceRetirementStatus::Pending),
        Ok(SourceRetirementStatus::Authorized),
    ]);
    assert_eq!(
        remixed
            .poll_source_conditional_retirement(&mut output, &ticket)
            .unwrap(),
        SourceRetirementStatus::Pending
    );
    assert_eq!(
        remixed
            .poll_source_conditional_retirement(&mut output, &ticket)
            .unwrap(),
        SourceRetirementStatus::Authorized
    );
    // Authorization survives a later cancellation request, but does not join or
    // recover the native owner. The actual atomic decision belongs to the worker.
    effects.cancelled.borrow_mut().extend([
        Ok(SourceRetirementStatus::Authorized),
        Ok(SourceRetirementStatus::Authorized),
    ]);
    for _ in 0..2 {
        assert_eq!(
            remixed
                .cancel_source_conditional_retirement(&mut output, &ticket)
                .unwrap(),
            SourceRetirementStatus::Authorized
        );
    }
    assert_eq!(
        &*effects.decisions.borrow(),
        &[
            ("poll", ticket_pointer),
            ("poll", ticket_pointer),
            ("cancel", ticket_pointer),
            ("cancel", ticket_pointer),
        ]
    );
    assert!(output.take_stopped_mixer().is_err());
    assert_eq!(effects.retire_calls.get(), 0);
    assert_eq!(effects.observe_calls.get(), 0);
    assert_eq!(effects.query_calls.get(), 0);
    assert_eq!(
        &*effects.conditional_outputs.borrow(),
        &vec![output_pointer; 7]
    );
}

#[test]
fn remix_live_refusal_explicit_cancel_and_ticket_errors_do_not_retire_the_owner() {
    let (backend, mut output, effects, _producer) = contract::fixture(None);
    let mut remixed = RemixedOutputBackend::new(backend);
    let refused = contract::Ticket(Box::new([1, 2]));
    let cancelled = contract::Ticket(Box::new([1, 3]));
    effects
        .polled
        .borrow_mut()
        .push_back(Ok(SourceRetirementStatus::Refused));
    effects
        .cancelled
        .borrow_mut()
        .push_back(Ok(SourceRetirementStatus::Cancelled));
    assert_eq!(
        remixed
            .poll_source_conditional_retirement(&mut output, &refused)
            .unwrap(),
        SourceRetirementStatus::Refused
    );
    assert_eq!(
        remixed
            .cancel_source_conditional_retirement(&mut output, &cancelled)
            .unwrap(),
        SourceRetirementStatus::Cancelled
    );
    let wrong = contract::Ticket(Box::new([2, 2]));
    let poll_error = contract::Fault::new(71);
    let poll_pointer = poll_error.pointer();
    let cancel_error = contract::Fault::new(72);
    let cancel_pointer = cancel_error.pointer();
    effects.polled.borrow_mut().push_back(Err(poll_error));
    effects.cancelled.borrow_mut().push_back(Err(cancel_error));
    assert_eq!(
        remixed
            .poll_source_conditional_retirement(&mut output, &wrong)
            .unwrap_err()
            .pointer(),
        poll_pointer
    );
    assert_eq!(
        remixed
            .cancel_source_conditional_retirement(&mut output, &wrong)
            .unwrap_err()
            .pointer(),
        cancel_pointer
    );
    assert_eq!(
        &*effects.decisions.borrow(),
        &[
            ("poll", refused.pointer()),
            ("cancel", cancelled.pointer()),
            ("poll", wrong.pointer()),
            ("cancel", wrong.pointer()),
        ]
    );
    assert!(output.owner.is_some());
    assert!(output.take_stopped_mixer().is_err());
    assert_eq!(effects.retire_calls.get(), 0);
}

#[test]
fn remix_history_and_coherent_packet_errors_preserve_native_error_identity() {
    let (backend, mut output, effects, _producer) = contract::fixture(None);
    let mut remixed = RemixedOutputBackend::new(backend);
    let basis = output.basis();
    let identity = memory::identity();
    let query = memory::query(1, basis.sample_rate());
    effects
        .history_replies
        .borrow_mut()
        .extend([Ok(Some(24)), Ok(None)]);
    assert_eq!(
        remixed
            .request_source_history_retirement(&mut output, 7, basis, identity, 3)
            .unwrap(),
        Some(24)
    );
    assert_eq!(
        remixed
            .request_source_history_retirement(&mut output, 7, basis, identity, 2)
            .unwrap(),
        None
    );
    let error = contract::Fault::new(81);
    let pointer = error.pointer();
    effects.history_replies.borrow_mut().push_back(Err(error));
    assert_eq!(
        remixed
            .request_source_history_retirement(&mut output, 9, basis, identity, 1)
            .unwrap_err()
            .pointer(),
        pointer
    );
    assert_eq!(
        &*effects.history.borrow(),
        &[
            (7, basis, identity, 3),
            (7, basis, identity, 2),
            (9, basis, identity, 1)
        ]
    );
    let error = contract::Fault::new(82);
    let pointer = error.pointer();
    effects.observations.borrow_mut().push_back(Err(error));
    let error = match remixed.observe_native_playback(&mut output, identity) {
        Err(error) => error,
        Ok(_) => panic!("native observation error"),
    };
    assert_eq!(error.pointer(), pointer);
    let error = contract::Fault::new(83);
    let pointer = error.pointer();
    effects.queries.borrow_mut().push_back(Err(error));
    let error = match remixed.source_playback_at(&output, &query, identity) {
        Err(error) => error,
        Ok(_) => panic!("source query error"),
    };
    assert_eq!(error.pointer(), pointer);
    assert_eq!(effects.observe_calls.get(), 1);
    assert_eq!(effects.query_calls.get(), 1);
    assert_eq!(effects.retire_calls.get(), 0);
}

#[test]
fn remix_complete_owner_open_failure_and_start_preparation_keep_state_and_error() {
    let (owner, _producer) = contract::complete_owner();
    let pcm_pointer = owner.pending_samples().as_ptr();
    let pcm = owner.pending_samples().to_vec();
    let report = owner.pending_report();
    let basis = owner.output_frame_basis();
    let effects = Rc::new(contract::Effects::default());
    let backend = contract::Backend::<NativeOutputState>::new(effects.clone(), Some(10_000_003));
    let mut remixed = RemixedOutputBackend::new(backend);
    let error = contract::Fault::new(101);
    let pointer = error.pointer();
    *effects.open_error.borrow_mut() = Some(error);
    let request = crate::gameplay::output::domain::remix::RemixedOutputRequest::remixed(
        contract::Request(Box::new(102)),
        ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap(),
    );
    let request_pointer: *const u64 = request.native.0.as_ref();
    let failure = match remixed.open(request, owner, 12) {
        Err(failure) => failure,
        Ok(_) => panic!("native open refusal"),
    };
    assert_eq!(failure.error().pointer(), pointer);
    assert!(failure.pending_owner().is_none());
    let (_, owner, _, _) = failure.into_parts();
    let owner = owner.unwrap();
    assert_eq!(owner.pending_samples().as_ptr(), pcm_pointer);
    assert_eq!(owner.pending_samples(), pcm);
    assert_eq!(owner.pending_report(), report);
    assert_eq!(owner.output_frame_basis(), basis);
    assert_eq!(
        &*effects.opens.borrow(),
        &[(request_pointer, Some(vec![1.0, 0.5]))]
    );
    let mut output = remixed
        .open(
            crate::gameplay::output::domain::remix::RemixedOutputRequest::strict(
                contract::Request(Box::new(103)),
            ),
            owner,
            13,
        )
        .unwrap();
    let error = contract::Fault::new(104);
    let pointer = error.pointer();
    *effects.prepare_error.borrow_mut() = Some(error);
    assert_eq!(
        remixed
            .prepare_replacement_start(&mut output)
            .unwrap_err()
            .pointer(),
        pointer
    );
    assert_eq!(effects.prepare_calls.get(), 1);
    assert_eq!(
        output.owner.as_ref().unwrap().pending_samples().as_ptr(),
        pcm_pointer
    );
    assert_eq!(
        remixed.created_source_coordinate_basis(&output).unwrap(),
        contract::descriptor(basis, Some(10_000_003))
    );
    assert_eq!(effects.retire_calls.get(), 0);
}
