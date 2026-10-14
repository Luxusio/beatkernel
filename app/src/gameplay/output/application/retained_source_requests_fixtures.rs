//! Typed Source UI transactions use the real owner, publisher and complete PCM state.
//! Only native effects are scripted; Windows settings are validated, not executed.
use super::*;
use crate::{
    gameplay::output::{
        application::replacement::{
            coordinate_source::test_support as support, ReplacementCause, ReplacementFailure,
            ReplacementPhase, ReplacementState,
        },
        domain::remix::matrix_text,
        ports::SourceRetirementStatus,
    },
    native_end::NativeEnd,
    settings::SettingsHost,
};
use beatkernel::{audio::ChannelMatrix, judge::JudgeEngine, time::ClockDomainId};
use beatkernel_platform::audio::{DeviceFormat, NativeOutputState, SampleEncoding};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    error::Error,
    io,
    rc::Rc,
};
use support::*;

type Owner = GameplayOutputOwner<Effects, NativeOutputState, OutputFrameBasis, Ticket>;
type Ui = GameplayOutputUi<Port>;
const REQUEST_ID: u64 = 193;

#[derive(Default)]
struct PortState {
    requests: VecDeque<OutputRequest>,
    replies: Vec<OutputReply>,
    attempts: Vec<OutputReply>,
    capability: Option<OutputCapability>,
    blocked: bool,
    takes: usize,
}
struct Port(Rc<RefCell<PortState>>);
impl OutputUiPort for Port {
    fn advertise(&mut self, capability: Option<OutputCapability>) -> io::Result<()> {
        self.0.borrow_mut().capability = capability;
        Ok(())
    }
    fn take_request(&mut self) -> io::Result<Option<OutputRequest>> {
        let mut state = self.0.borrow_mut();
        state.takes += 1;
        Ok(state.requests.pop_front())
    }
    fn reply(&mut self, reply: &OutputReply) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.attempts.push(reply.clone());
        if state.blocked {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        if let Ok(capability) = &reply.result {
            state.capability = Some(capability.clone());
        }
        state.replies.push(reply.clone());
        Ok(())
    }
    fn pending(&self) -> bool {
        !self.0.borrow().requests.is_empty()
    }
}

fn capability(request: &Request) -> OutputCapability {
    let capability = OutputCapability {
        host: SettingsHost::Windows,
        current_args: vec![
            "--backend".into(),
            "wasapi".into(),
            "--mode".into(),
            "shared".into(),
            "--shared-policy".into(),
            "engine".into(),
            "--buffer".into(),
            format!("frames:{}", request.buffer()),
            "--period".into(),
            format!("frames:{}", request.period()),
            "--output-matrix".into(),
            request
                .matrix()
                .map_or_else(|| "exact".into(), |matrix| matrix_text(matrix).unwrap()),
        ],
    };
    capability
        .validate()
        .expect("the fixture advertises the actual Windows WASAPI shared schema");
    capability
}

fn setup(
    frequency: Option<u64>,
    partial: bool,
    request: &Request,
) -> (Rig, Owner, Effects, Ui, Rc<RefCell<PortState>>) {
    let mut rig = Rig::with_frequency(partial, frequency);
    let (effects, output) = rig.take_live_native_owner();
    let trace = effects.clone();
    let owner = Owner::new_with_ticket(effects, output);
    let state = Rc::new(RefCell::new(PortState {
        requests: VecDeque::from([OutputRequest {
            id: REQUEST_ID,
            args: capability(request).current_args,
        }]),
        capability: Some(capability(&Request::normal(frequency))),
        ..Default::default()
    }));
    let ui = GameplayOutputUi::new(Port(state.clone()));
    (rig, owner, trace, ui, state)
}

fn tick(
    rig: &mut Rig,
    owner: &mut Owner,
    ui: &mut Ui,
    request: &Request,
    practice_pending: bool,
    applied: &mut impl FnMut(&Output) -> Result<OutputCapability, String>,
) -> Result<bool, Box<dyn Error>> {
    let identity = rig.source_identity();
    ui.service_coordinate_source_audio(
        owner,
        rig.context(),
        now(),
        identity,
        practice_pending,
        &mut |ui_request, _| {
            assert_eq!(ui_request.id, REQUEST_ID);
            assert_eq!(ui_request.args, capability(request).current_args);
            Ok(request.clone())
        },
        applied,
    )
}

fn drive(
    rig: &mut Rig,
    owner: &mut Owner,
    ui: &mut Ui,
    request: &Request,
    practice_pending: bool,
    applied: &mut impl FnMut(&Output) -> Result<OutputCapability, String>,
) -> Result<bool, Box<dyn Error>> {
    for _ in 0..12 {
        if tick(rig, owner, ui, request, practice_pending, applied)? {
            return Ok(true);
        }
        if !ui.pending() && !owner.has_work() {
            return Ok(false);
        }
    }
    panic!("the bounded scripted Source transaction must settle");
}

fn count(trace: &Effects, name: &str) -> usize {
    trace.calls().iter().filter(|call| **call == name).count()
}

#[test]
fn retained_source_requests_frame_counter_geometry_and_remix_preserve_original_play() {
    for (frequency, new_frequency, remixed) in [
        (None, None, false),
        (Some(OLD_FREQUENCY), Some(10_000_000), false),
        (Some(OLD_FREQUENCY), Some(10_000_000), true),
    ] {
        let request = Request::configured(
            DeviceFormat::new(
                SOURCE_RATE,
                if remixed { 2 } else { 1 },
                SampleEncoding::Float32,
                None,
            )
            .unwrap(),
            remixed.then(|| ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap()),
            4,
            32,
            new_frequency,
        );
        let (mut rig, mut owner, trace, mut ui, state) = setup(frequency, false, &request);
        let old = owner.current().unwrap().source_state();
        let history = source_history(old);
        let first_unsent = old.output_frame_basis();
        let frozen = old.mixer().playback_frame_cursor();
        let pin = rig.source_pin();
        let identity = rig.source_identity();
        let judge = rig.runtime.judge() as *const JudgeEngine;
        let transport = format!("{:?}", rig.runtime.transport_mut());
        let telemetry = format!("{:?}", rig.runtime.telemetry());
        let commands = rig.runtime.admitted_audio_commands();
        let generation = rig.practice_generation();
        let merger = rig.inputs.preparation_state().unwrap();
        let frontiers = preserved_frontiers(rig.presentation());
        let stages = Cell::new(0);
        let stage_calls = RefCell::new(Vec::new());
        assert!(drive(
            &mut rig,
            &mut owner,
            &mut ui,
            &request,
            false,
            &mut |output| {
                stages.set(stages.get() + 1);
                *stage_calls.borrow_mut() = trace.calls();
                assert_eq!(output.creation_epoch(), OLD_EPOCH + 1);
                assert_eq!(output.basis, first_unsent);
                assert_eq!(
                    output.source_state().mixer().playback_frame_cursor(),
                    frozen
                );
                assert_eq!(source_history(output.source_state()).spans, history.spans);
                Ok(capability(output.applied_request()))
            }
        )
        .unwrap());
        let output = owner.current().unwrap();
        assert_eq!(output.format(), request.format());
        assert_eq!(output.applied_request().matrix(), request.matrix());
        assert_eq!(output.applied_request().period(), 4);
        assert_eq!(output.applied_request().buffer(), 32);
        assert_eq!(output.source_state().mixer().applied_start_frame(), Some(2));
        assert_eq!(
            output.source_state().mixer().playback_frame_cursor(),
            frozen
        );
        let after = source_history(output.source_state());
        assert_eq!(after.source_rate, history.source_rate);
        assert_eq!(after.initial_physical, history.initial_physical);
        assert_eq!(after.initial_playback, history.initial_playback);
        assert_eq!(after.spans, history.spans);
        assert_eq!(rig.source_identity(), identity);
        assert_eq!(rig.presentation().practice_mixer_basis(), Some(pin));
        assert_eq!(rig.presentation().basis(), Some(first_unsent));
        assert_ne!(first_unsent, pin);
        assert_eq!(
            rig.presentation().authority().coordinate_basis(),
            Some(descriptor(first_unsent, new_frequency))
        );
        assert_eq!(rig.presentation().authority().epoch().id, OLD_EPOCH + 1);
        assert_eq!(rig.paused().epoch(), OLD_EPOCH + 1);
        assert_eq!(preserved_frontiers(rig.presentation()), frontiers);
        assert!(rig.inputs.matches_preparation(&merger));
        assert_eq!(rig.runtime.judge() as *const JudgeEngine, judge);
        assert_eq!(format!("{:?}", rig.runtime.transport_mut()), transport);
        assert_eq!(format!("{:?}", rig.runtime.telemetry()), telemetry);
        assert_eq!(rig.runtime.admitted_audio_commands(), commands);
        assert_eq!(rig.practice_generation(), generation);
        assert_eq!(stages.get(), 1);
        assert_eq!(*stage_calls.borrow(), trace.calls());
        assert_eq!(
            state.borrow().replies,
            [OutputReply {
                id: REQUEST_ID,
                result: Ok(capability(&request))
            }]
        );
        assert_eq!(state.borrow().capability, Some(capability(&request)));
        assert_eq!(count(&trace, "submit"), 1);
        assert_eq!(count(&trace, "open"), 1);
        assert!(!owner.has_work());
        assert!(!owner.output_clock_suspended());
        // Resume reaches the same command producer, Mixer program and PCM bank.
        rig.runtime.request_audio_pause(false);
        let complete = owner.current_mut().unwrap().state.as_mut().unwrap();
        let report = complete.render_pending(4).unwrap();
        assert!(!report.paused);
        assert_eq!(report.playback_frames, 4);
        let samples = complete.pending_samples();
        assert!(samples.iter().any(|sample| *sample != 0.0));
        if remixed {
            assert!(samples.chunks_exact(2).all(|pair| pair == [0.25, 0.125]));
        } else {
            assert!(samples.iter().all(|sample| *sample == 0.25));
        }
    }
}

#[test]
fn retained_source_requests_compatible_partial_pcm_keeps_original_history_and_first_unsent_basis() {
    let request = Request::normal(Some(10_000_000));
    let (mut rig, mut owner, trace, mut ui, state) = setup(Some(OLD_FREQUENCY), true, &request);
    let original = owner.current().unwrap().source_state();
    let first_unsent = original.output_frame_basis();
    let pulled = original.mixer().frame_cursor();
    let history = source_history(original);
    assert_eq!(original.pending_frames(), 5);
    assert!(first_unsent.start_physical_frame() < pulled);
    assert!(
        drive(&mut rig, &mut owner, &mut ui, &request, false, &mut |o| {
            Ok(capability(o.applied_request()))
        })
        .unwrap()
    );
    let output = owner.current().unwrap();
    assert_eq!(output.basis, first_unsent);
    assert_eq!(source_history(output.source_state()).spans, history.spans);
    assert!(output.source_state().pending_samples().is_empty());
    assert_eq!(output.source_state().mixer().playback_frame_cursor(), 7);
    assert_eq!(count(&trace, "submit"), 1);
    assert_eq!(state.borrow().replies.len(), 1);
}

#[test]
fn retained_source_requests_capability_refusal_keeps_ready_owner_and_pause_hold_before_commit() {
    for structurally_invalid in [false, true] {
        let request = Request::normal(Some(10_000_000));
        let (mut rig, mut owner, trace, mut ui, state) = setup(Some(OLD_FREQUENCY), true, &request);
        let before = (
            format!("{:?}", rig.presentation()),
            format!("{:?}", rig.pause),
            format!("{:?}", rig.config),
        );
        let merger = rig.inputs.preparation_state().unwrap();
        let accepted = state.borrow().capability.clone();
        let stages = Cell::new(0);
        let staged_owner = RefCell::new(None);
        let staged_calls = RefCell::new(Vec::new());
        let result = drive(
            &mut rig,
            &mut owner,
            &mut ui,
            &request,
            false,
            &mut |output| {
                stages.set(stages.get() + 1);
                *staged_owner.borrow_mut() = Some(owner_facts(output.source_state()));
                *staged_calls.borrow_mut() = trace.calls();
                if structurally_invalid {
                    let invalid = OutputCapability {
                        host: SettingsHost::Windows,
                        current_args: vec!["--buffer".into()],
                    };
                    assert!(invalid.validate().is_err());
                    Ok(invalid)
                } else {
                    Err("actual Source capability could not be staged".into())
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(stages.get(), 1);
        assert_eq!(
            (
                format!("{:?}", rig.presentation()),
                format!("{:?}", rig.pause),
                format!("{:?}", rig.config)
            ),
            before
        );
        assert!(rig.inputs.matches_preparation(&merger));
        assert_eq!(state.borrow().capability, accepted);
        assert_eq!(state.borrow().replies.len(), 1);
        assert_eq!(state.borrow().replies[0].id, REQUEST_ID);
        assert!(state.borrow().replies[0].result.is_err());
        assert_eq!(trace.calls(), *staged_calls.borrow());
        assert!(owner.current().is_none());
        assert!(owner.replacement_pending());
        assert_eq!(owner.state(), ReplacementState::Waiting);
        assert!(owner.output_clock_suspended());
        assert!(rig.runtime.hold_audio_pause().is_err());
        assert!(owner.cancel().unwrap());
        assert_eq!(
            owner_facts(&owner.take_recovered_mixer().unwrap()),
            staged_owner.borrow().as_ref().unwrap().as_str()
        );
        drop(rig.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn retained_source_requests_pending_then_live_refusal_preserves_original_capability_and_pcm() {
    for status in [
        SourceRetirementStatus::Refused,
        SourceRetirementStatus::Cancelled,
    ] {
        let request = Request::normal(Some(10_000_000));
        let (mut rig, mut owner, mut trace, mut ui, state) =
            setup(Some(OLD_FREQUENCY), true, &request);
        trace.conditional_statuses([SourceRetirementStatus::Pending, status]);
        let native_id = owner.current().unwrap().native_id;
        let original = owner_facts(owner.current().unwrap().source_state());
        let before = format!("{:?}", rig.presentation());
        let accepted = state.borrow().capability.clone();
        let mut applied = |_: &Output| -> Result<OutputCapability, String> {
            panic!("neither Pending nor live refusal may stage a capability")
        };
        assert!(!tick(&mut rig, &mut owner, &mut ui, &request, false, &mut applied).unwrap());
        assert_eq!(owner.state(), ReplacementState::ConditionalRetirement);
        assert!(owner.has_work());
        assert!(!owner.output_clock_suspended());
        assert!(state.borrow().replies.is_empty());
        assert_eq!(state.borrow().capability, accepted);
        assert!(!drive(&mut rig, &mut owner, &mut ui, &request, false, &mut applied).unwrap());
        assert_eq!(owner.current().unwrap().native_id, native_id);
        assert_eq!(
            owner_facts(owner.current().unwrap().source_state()),
            original
        );
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert_eq!(state.borrow().capability, accepted);
        assert_eq!(state.borrow().replies.len(), 1);
        assert_eq!(state.borrow().replies[0].id, REQUEST_ID);
        assert!(state.borrow().replies[0].result.is_err());
        assert_eq!(count(&trace, "submit"), 1);
        assert_eq!(count(&trace, "retire"), 0);
        assert_eq!(count(&trace, "open"), 0);
        assert!(!owner.has_work());
        assert!(!owner.output_clock_suspended());
        drop(rig.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn retained_source_requests_cold_mapping_practice_and_native_end_refuse_without_native_effects() {
    for refusal in ["mapping", "unsupported", "practice", "native-end"] {
        let request = Request::normal(Some(10_000_000));
        let (mut rig, mut owner, trace, mut ui, state) = setup(Some(OLD_FREQUENCY), true, &request);
        let original_id = owner.current().unwrap().native_id;
        let original = owner_facts(owner.current().unwrap().source_state());
        let accepted = state.borrow().capability.clone();
        let before = format!("{:?}", rig.presentation());
        if refusal == "native-end" {
            rig.end = Some(
                NativeEnd::new(rig.pin.origin(), ClockDomainId(1), SOURCE_RATE, 100)
                    .unwrap()
                    .with_coordinates(
                        OLD_EPOCH,
                        descriptor(rig.pin, Some(OLD_FREQUENCY)),
                        rig.identity,
                    )
                    .unwrap(),
            );
        }
        let identity = rig.source_identity();
        assert!(!ui
            .service_coordinate_source_audio(
                &mut owner,
                rig.context(),
                now(),
                identity,
                refusal == "practice",
                &mut |ui_request, _| {
                    assert_eq!(ui_request.id, REQUEST_ID);
                    match refusal {
                        "mapping" => Err("malformed requested endpoint".into()),
                        "unsupported" => Err("retained Source does not support this mode".into()),
                        _ => Ok(request.clone()),
                    }
                },
                &mut |_| panic!("a cold refusal cannot stage an applied capability"),
            )
            .unwrap());
        assert_eq!(owner.current().unwrap().native_id, original_id);
        assert_eq!(
            owner_facts(owner.current().unwrap().source_state()),
            original
        );
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert_eq!(state.borrow().capability, accepted);
        assert_eq!(state.borrow().replies.len(), 1);
        assert_eq!(state.borrow().replies[0].id, REQUEST_ID);
        assert!(state.borrow().replies[0].result.is_err());
        assert_eq!(count(&trace, "submit"), 0);
        assert_eq!(count(&trace, "retire"), 0);
        assert_eq!(count(&trace, "open"), 0);
        assert_eq!(count(&trace, "packet"), 0);
        assert!(!owner.has_work());
        drop(rig.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn retained_source_requests_would_block_keeps_one_reply_and_does_not_take_the_next_request() {
    let request = Request::normal(Some(10_000_000));
    let (mut rig, mut owner, trace, mut ui, state) = setup(Some(OLD_FREQUENCY), false, &request);
    state.borrow_mut().blocked = true;
    let stages = Cell::new(0);
    let mut applied = |output: &Output| {
        stages.set(stages.get() + 1);
        Ok(capability(output.applied_request()))
    };
    assert!(drive(&mut rig, &mut owner, &mut ui, &request, false, &mut applied).unwrap());
    assert!(state.borrow().replies.is_empty());
    let expected = OutputReply {
        id: REQUEST_ID,
        result: Ok(capability(&request)),
    };
    assert_eq!(state.borrow().attempts[0], expected);
    state.borrow_mut().requests.push_back(OutputRequest {
        id: REQUEST_ID + 1,
        args: capability(&request).current_args,
    });
    let takes = state.borrow().takes;
    let native = trace.calls();
    for _ in 0..3 {
        assert!(!tick(&mut rig, &mut owner, &mut ui, &request, true, &mut applied).unwrap());
        assert_eq!(state.borrow().takes, takes);
        assert_eq!(state.borrow().requests.len(), 1);
        assert_eq!(trace.calls(), native);
    }
    // The queued next request was never consumed while the previous reply blocked.
    state.borrow_mut().requests.clear();
    state.borrow_mut().blocked = false;
    assert!(!tick(&mut rig, &mut owner, &mut ui, &request, true, &mut applied).unwrap());
    assert_eq!(state.borrow().replies, [expected.clone()]);
    assert!(state
        .borrow()
        .attempts
        .iter()
        .all(|reply| *reply == expected));
    assert_eq!(stages.get(), 1);
    assert_eq!(count(&trace, "submit"), 1);
    assert_eq!(count(&trace, "open"), 1);
    assert_eq!(trace.calls(), native);
    assert!(!ui.pending());
}

#[test]
fn retained_source_requests_admitted_transaction_continues_after_practice_becomes_pending() {
    let request = Request::normal(Some(10_000_000));
    let (mut rig, mut owner, mut trace, mut ui, state) = setup(Some(OLD_FREQUENCY), true, &request);
    trace.conditional_statuses([
        SourceRetirementStatus::Pending,
        SourceRetirementStatus::Authorized,
    ]);
    assert!(
        !tick(&mut rig, &mut owner, &mut ui, &request, false, &mut |o| {
            Ok(capability(o.applied_request()))
        })
        .unwrap()
    );
    assert!(owner.has_work());
    assert!(
        drive(&mut rig, &mut owner, &mut ui, &request, true, &mut |o| {
            Ok(capability(o.applied_request()))
        })
        .unwrap()
    );
    assert_eq!(state.borrow().replies.len(), 1);
    assert!(state.borrow().replies[0].result.is_ok());
    assert_eq!(count(&trace, "submit"), 1);
    assert_eq!(count(&trace, "open"), 1);
    assert!(!owner.has_work());
}

#[test]
fn retained_source_requests_native_failures_propagate_original_errors_and_one_blocked_reply() {
    for failure_kind in ["probe", "observation", "partial-open"] {
        let fault = match failure_kind {
            "observation" => Fault::Observe,
            "partial-open" => Fault::PartialOpen,
            _ => Fault::None,
        };
        let request = Request::normal(Some(10_000_000)).with_fault(fault);
        let (mut rig, mut owner, mut trace, mut ui, state) =
            setup(Some(OLD_FREQUENCY), true, &request);
        let accepted = state.borrow().capability.clone();
        let before = format!("{:?}", rig.presentation());
        if failure_kind == "probe" {
            trace.conditional_probe_failures([true]);
            trace.retirement_failures([true]);
            trace.recovery_failures([true]);
        } else {
            trace.retirement_failures([false, true]);
            trace.recovery_failures([false, true]);
        }
        state.borrow_mut().blocked = true;
        let result = drive(&mut rig, &mut owner, &mut ui, &request, false, &mut |_| {
            panic!("a failed native transaction cannot stage a capability")
        });
        let failure = match result {
            Err(failure) => failure,
            Ok(_) => panic!("the original native failure must propagate"),
        };
        let original = failure
            .downcast_ref::<ReplacementFailure<io::Error>>()
            .unwrap();
        let (phase, message) = match failure_kind {
            "probe" => (
                ReplacementPhase::Retire,
                "scripted native conditional probe failure",
            ),
            "observation" => (
                ReplacementPhase::Observe,
                "original native observation failure",
            ),
            _ => (ReplacementPhase::Open, "original partial open failure"),
        };
        assert!(
            matches!(&original.cause, ReplacementCause::Backend { phase: actual, error }
            if *actual == phase && error.to_string() == message)
        );
        assert_eq!(
            original.cleanup.as_ref().unwrap().to_string(),
            if failure_kind == "partial-open" {
                "original partial open cleanup"
            } else {
                "scripted native retirement failure"
            }
        );
        if failure_kind == "partial-open" {
            assert!(original.recovery.is_none());
        } else {
            assert_eq!(
                original.recovery.as_ref().unwrap().to_string(),
                "scripted native recovery failure"
            );
        }
        assert_eq!(format!("{:?}", rig.presentation()), before);
        assert_eq!(state.borrow().capability, accepted);
        assert!(state.borrow().replies.is_empty());
        let expected = OutputReply {
            id: REQUEST_ID,
            result: Err(failure.to_string()),
        };
        assert_eq!(state.borrow().attempts, [expected.clone()]);
        let native = trace.calls();
        let takes = state.borrow().takes;
        for _ in 0..2 {
            assert!(
                !tick(&mut rig, &mut owner, &mut ui, &request, false, &mut |_| {
                    panic!("retrying a blocked failure reply cannot stage capability")
                })
                .unwrap()
            );
            assert_eq!(trace.calls(), native);
            assert_eq!(state.borrow().takes, takes);
        }
        state.borrow_mut().blocked = false;
        assert!(
            !tick(&mut rig, &mut owner, &mut ui, &request, false, &mut |_| {
                panic!("flushing a failure reply cannot stage capability")
            })
            .unwrap()
        );
        assert_eq!(state.borrow().replies, [expected.clone()]);
        assert!(state
            .borrow()
            .attempts
            .iter()
            .all(|reply| *reply == expected));
        assert_eq!(count(&trace, "submit"), 1);
        assert_eq!(trace.calls(), native);
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        assert!(rig.runtime.hold_audio_pause().is_err());
    }
}
