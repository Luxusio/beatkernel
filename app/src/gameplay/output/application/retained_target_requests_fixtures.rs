//! Correlated UI requests traverse the real retained owner and coordinate publisher.
use super::*;
use crate::coordinate_target_replacement_test_support as support;
use crate::{
    gameplay::output::application::replacement::ReplacementState,
    gameplay::output::domain::remix::matrix_text, gameplay::output::ports::TargetRetirementStatus,
    settings::SettingsHost,
};
use beatkernel::{audio::ChannelMatrix, judge::JudgeEngine};
use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
use support::*;

type Owner = GameplayOutputOwner<Effects, ConvertedNativeOutputState, TargetFrameBasis, Ticket>;
type Ui = GameplayOutputUi<Port>;

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
    let encoding = match request.format().encoding() {
        SampleEncoding::Float32 => "float32",
        SampleEncoding::Pcm {
            container_bits: 16,
            valid_bits: 16,
        } => "pcm16",
        SampleEncoding::Pcm {
            container_bits: 24,
            valid_bits: 24,
        } => "pcm24",
        SampleEncoding::Pcm {
            container_bits: 32,
            valid_bits: 32,
        } => "pcm32",
        _ => panic!("test fixture advertises only actual settings encodings"),
    };
    let capability = OutputCapability {
        host: SettingsHost::Linux,
        current_args: vec![
            "--alsa".into(),
            "retained-memory".into(),
            "--rate".into(),
            request.format().sample_rate().to_string(),
            "--output-matrix".into(),
            matrix_text(request.matrix()).unwrap(),
            "--encoding".into(),
            encoding.into(),
            "--period-frames".into(),
            request.period().to_string(),
            "--buffer-frames".into(),
            request.buffer().to_string(),
        ],
    };
    capability
        .validate()
        .expect("fixture uses the actual Linux live output schema");
    capability
}
fn setup(partial: bool, request: &Request) -> (Rig, Owner, Effects, Ui, Rc<RefCell<PortState>>) {
    let mut rig = Rig::new(partial);
    let (effects, output) = rig.take_live_native_owner();
    let trace = effects.clone();
    let owner = Owner::new_with_ticket(effects, output);
    let state = Rc::new(RefCell::new(PortState {
        requests: VecDeque::from([OutputRequest {
            id: 93,
            args: capability(request).current_args,
        }]),
        capability: Some(capability(&Request::normal(TARGET_RATE))),
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
) -> Result<bool, Box<dyn std::error::Error>> {
    let identity = rig.source_identity();
    ui.service_coordinate_target_audio(
        owner,
        rig.context(),
        now(),
        identity,
        practice_pending,
        &mut |ui_request, _| {
            assert_eq!(ui_request.id, 93);
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
) -> Result<bool, Box<dyn std::error::Error>> {
    for _ in 0..12 {
        if tick(rig, owner, ui, request, practice_pending, applied)? {
            return Ok(true);
        }
        if !ui.pending() && !owner.has_work() {
            return Ok(false);
        }
    }
    panic!("the bounded scripted native transaction must settle");
}
fn count(trace: &Effects, name: &str) -> usize {
    trace.calls().iter().filter(|call| **call == name).count()
}

#[test]
fn retained_target_requests_same_and_changed_configuration_preserve_actual_source_and_input_identity(
) {
    let stereo = Request::configured(
        DeviceFormat::new(TARGET_RATE, 2, SampleEncoding::Float32, Some(3)).unwrap(),
        ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap(),
        16,
        64,
    );
    let pcm = Request::configured(
        DeviceFormat::new(
            TARGET_RATE,
            1,
            SampleEncoding::Pcm {
                container_bits: 16,
                valid_bits: 16,
            },
            None,
        )
        .unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        16,
        64,
    );
    for request in [
        Request::normal(TARGET_RATE),
        Request::normal(32_000),
        Request::normal(96_000),
        stereo,
        pcm,
    ] {
        let (mut rig, mut owner, trace, mut ui, state) = setup(false, &request);
        let old = owner.current().unwrap();
        let source_position = old.source_state().converter_owner().source_position();
        let before_source_history = source_history(old.source_state());
        let frozen = old.source_state().mixer().playback_frame_cursor();
        let pulled = old.source_state().mixer().frame_cursor();
        let generation = old.source_state().mixer().practice_generation();
        let merger = rig.inputs.preparation_state().unwrap();
        let frontiers = preserved_frontiers(rig.presentation());
        let pin = rig.source_pin();
        let identity = rig.source_identity();
        let judge = rig.runtime.judge() as *const JudgeEngine;
        let transport = format!("{:?}", rig.runtime.transport_mut());
        let controller_generation = rig.practice_generation();
        let staged_calls = Cell::new(0);
        let native_calls_at_stage = RefCell::new(Vec::new());
        let expected = capability(&request);
        let mut applied = |output: &Output| {
            staged_calls.set(staged_calls.get() + 1);
            *native_calls_at_stage.borrow_mut() = trace.calls();
            assert_eq!(output.creation_epoch(), OLD_EPOCH + 1);
            assert_eq!(
                output.source_state().mixer().practice_generation(),
                generation
            );
            Ok(capability(output.applied_request()))
        };
        assert!(drive(&mut rig, &mut owner, &mut ui, &request, false, &mut applied).unwrap());
        let output = owner.current().unwrap();
        assert_eq!(output.format(), request.format());
        assert_eq!(output.applied_request().matrix(), request.matrix());
        assert_eq!(output.applied_request().period(), request.period());
        assert_eq!(output.applied_request().buffer(), request.buffer());
        assert_eq!(
            output.source_state().converter_owner().source_position(),
            source_position
        );
        assert_eq!(source_history(output.source_state()), before_source_history);
        assert_eq!(
            output.source_state().mixer().playback_frame_cursor(),
            frozen
        );
        assert_eq!(output.source_state().mixer().frame_cursor(), pulled);
        assert_eq!(
            output.source_state().mixer().practice_generation(),
            generation
        );
        assert_eq!(rig.practice_generation(), controller_generation);
        assert_eq!(rig.source_identity(), identity);
        assert_eq!(rig.presentation().practice_mixer_basis(), Some(pin));
        assert_eq!(rig.presentation().authority().epoch().id, OLD_EPOCH + 1);
        assert_eq!(rig.paused().epoch(), OLD_EPOCH + 1);
        assert_eq!(rig.native_config().sample_rate, SOURCE_RATE);
        assert_eq!(preserved_frontiers(rig.presentation()), frontiers);
        assert!(rig.inputs.matches_preparation(&merger));
        assert_eq!(rig.runtime.judge() as *const JudgeEngine, judge);
        assert_eq!(format!("{:?}", rig.runtime.transport_mut()), transport);
        assert_eq!(staged_calls.get(), 1);
        assert_eq!(
            *native_calls_at_stage.borrow(),
            trace.calls(),
            "no native effect after capability staging and commit"
        );
        assert_eq!(
            state.borrow().replies,
            [OutputReply {
                id: 93,
                result: Ok(expected.clone())
            }]
        );
        assert_eq!(state.borrow().capability, Some(expected));
        assert_eq!(count(&trace, "submit"), 1);
        assert_eq!(count(&trace, "open"), 1);
        assert!(!owner.has_work());
        assert!(!owner.output_clock_suspended());
    }
}

#[test]
fn retained_target_requests_compatible_pending_pcm_keeps_the_original_suffix() {
    let request = Request::normal(TARGET_RATE);
    let (mut rig, mut owner, _, mut ui, state) = setup(true, &request);
    let old = owner.current().unwrap().source_state();
    let suffix = old.pending_samples().to_vec();
    let generation = old.mixer().practice_generation();
    assert!(!suffix.is_empty());
    assert!(
        drive(&mut rig, &mut owner, &mut ui, &request, false, &mut |o| Ok(
            capability(o.applied_request())
        ))
        .unwrap()
    );
    assert_eq!(
        owner
            .current()
            .unwrap()
            .source_state()
            .mixer()
            .practice_generation(),
        generation
    );
    assert_eq!(state.borrow().replies.len(), 1);
    assert!(
        owner
            .current()
            .unwrap()
            .source_state()
            .pending_samples()
            .is_empty(),
        "original held suffix was submitted before fresh native evidence qualified publication"
    );
}

#[test]
fn retained_target_requests_capability_failures_precede_commit_and_keep_the_exclusive_ready_hold() {
    for invalid_capability in [false, true] {
        let request = Request::normal(32_000);
        let (mut rig, mut owner, trace, mut ui, state) = setup(false, &request);
        let epoch = rig.presentation().authority().epoch();
        let frontiers = preserved_frontiers(rig.presentation());
        let merger = rig.inputs.preparation_state().unwrap();
        let pin = rig.source_pin();
        let generation = rig.practice_generation();
        let old_capability = state.borrow().capability.clone();
        let staged = Cell::new(0);
        let result = drive(&mut rig, &mut owner, &mut ui, &request, false, &mut |_| {
            staged.set(staged.get() + 1);
            if invalid_capability {
                let invalid = OutputCapability {
                    host: SettingsHost::Linux,
                    current_args: vec!["--rate".into()],
                };
                assert!(
                    invalid.validate().is_err(),
                    "a missing value is structurally invalid, unlike an editable numeric draft"
                );
                Ok(invalid)
            } else {
                Err("capability staging refused before commit".into())
            }
        });
        assert!(result.is_err());
        assert_eq!(staged.get(), 1);
        assert_eq!(rig.presentation().authority().epoch(), epoch);
        assert_eq!(preserved_frontiers(rig.presentation()), frontiers);
        assert_eq!(rig.presentation().practice_mixer_basis(), Some(pin));
        assert_eq!(rig.practice_generation(), generation);
        assert!(rig.inputs.matches_preparation(&merger));
        assert_eq!(state.borrow().capability, old_capability);
        assert_eq!(state.borrow().replies.len(), 1);
        assert_eq!(state.borrow().replies[0].id, 93);
        assert!(state.borrow().replies[0].result.is_err());
        assert!(owner.current().is_none());
        assert!(owner.replacement_pending());
        assert!(
            owner.output_clock_suspended(),
            "rejected ready output has no published audio clock"
        );
        assert!(
            rig.runtime.hold_audio_pause().is_err(),
            "failed publication retains its only producer pause lease"
        );
        assert_eq!(count(&trace, "open"), 1);
        assert!(owner.cancel().unwrap());
        assert!(owner.take_recovered_mixer().is_some());
        let hold = rig.runtime.hold_audio_pause().unwrap();
        drop(hold);
    }
}

#[test]
fn retained_target_requests_live_refusal_restores_the_same_owner_capability_and_program() {
    for status in [
        TargetRetirementStatus::Refused,
        TargetRetirementStatus::Cancelled,
    ] {
        let request = Request::normal(32_000);
        let (mut rig, mut owner, mut trace, mut ui, state) = setup(true, &request);
        trace.conditional_statuses([status]);
        let native_id = owner.current().unwrap().native_id;
        let suffix = owner
            .current()
            .unwrap()
            .source_state()
            .pending_samples()
            .to_vec();
        let epoch = rig.presentation().authority().epoch();
        let pin = rig.source_pin();
        let generation = rig.practice_generation();
        let accepted = state.borrow().capability.clone();
        let mut applied = |_: &Output| -> Result<OutputCapability, String> {
            panic!("live refusal must not stage a capability")
        };
        assert!(!drive(&mut rig, &mut owner, &mut ui, &request, false, &mut applied).unwrap());
        assert_eq!(owner.current().unwrap().native_id, native_id);
        assert!(!owner.current().unwrap().retired);
        assert_eq!(
            owner.current().unwrap().source_state().pending_samples(),
            suffix
        );
        assert_eq!(rig.presentation().authority().epoch(), epoch);
        assert_eq!(rig.presentation().practice_mixer_basis(), Some(pin));
        assert_eq!(rig.practice_generation(), generation);
        assert_eq!(state.borrow().capability, accepted);
        assert_eq!(state.borrow().replies.len(), 1);
        assert_eq!(state.borrow().replies[0].id, 93);
        assert!(state.borrow().replies[0].result.is_err());
        assert_eq!(count(&trace, "submit"), 1);
        assert_eq!(count(&trace, "retire"), 0);
        assert_eq!(count(&trace, "open"), 0);
        assert!(!owner.has_work());
        assert!(!owner.output_clock_suspended());
        let hold = rig.runtime.hold_audio_pause().unwrap();
        drop(hold);
    }
}

#[test]
fn retained_target_requests_cold_practice_refusal_keeps_original_owner_without_native_submission() {
    let request = Request::normal(32_000);
    let (mut rig, mut owner, trace, mut ui, state) = setup(false, &request);
    let original = owner.current().unwrap().native_id;
    let capability = state.borrow().capability.clone();
    let generation = rig.practice_generation();
    assert!(!drive(
        &mut rig,
        &mut owner,
        &mut ui,
        &request,
        true,
        &mut |_| panic!("cold refusal cannot stage")
    )
    .unwrap());
    assert_eq!(owner.current().unwrap().native_id, original);
    assert_eq!(rig.practice_generation(), generation);
    assert_eq!(state.borrow().capability, capability);
    assert_eq!(state.borrow().replies.len(), 1);
    assert_eq!(state.borrow().replies[0].id, 93);
    assert!(state.borrow().replies[0].result.is_err());
    assert_eq!(count(&trace, "submit"), 0);
    assert_eq!(count(&trace, "retire"), 0);
    assert!(!owner.has_work());
}

#[test]
fn retained_target_requests_mapper_refusal_is_correlated_and_never_retires_native_output() {
    let request = Request::normal(32_000);
    let (mut rig, mut owner, trace, mut ui, state) = setup(false, &request);
    let original = owner.current().unwrap().native_id;
    let identity = rig.source_identity();
    let result = ui.service_coordinate_target_audio(
        &mut owner,
        rig.context(),
        now(),
        identity,
        false,
        &mut |_, _| Err("unsupported requested endpoint".into()),
        &mut |_| panic!("mapper refusal cannot stage"),
    );
    assert!(!result.unwrap());
    assert_eq!(owner.current().unwrap().native_id, original);
    assert_eq!(
        state.borrow().replies,
        [OutputReply {
            id: 93,
            result: Err("unsupported requested endpoint".into())
        }]
    );
    assert_eq!(count(&trace, "submit"), 0);
    assert_eq!(count(&trace, "retire"), 0);
}

#[test]
fn retained_target_requests_would_block_preserves_the_staged_result_and_never_repeats_native_work()
{
    let request = Request::normal(32_000);
    let (mut rig, mut owner, trace, mut ui, state) = setup(false, &request);
    state.borrow_mut().blocked = true;
    let staged = Cell::new(0);
    let mut applied = |output: &Output| {
        staged.set(staged.get() + 1);
        Ok(capability(output.applied_request()))
    };
    assert!(drive(&mut rig, &mut owner, &mut ui, &request, false, &mut applied).unwrap());
    assert!(state.borrow().replies.is_empty());
    let expected_reply = state.borrow().attempts[0].clone();
    let native = trace.calls();
    let takes = state.borrow().takes;
    for _ in 0..3 {
        assert!(!tick(&mut rig, &mut owner, &mut ui, &request, true, &mut applied).unwrap());
        assert_eq!(trace.calls(), native);
        assert_eq!(state.borrow().takes, takes);
    }
    state.borrow_mut().blocked = false;
    assert!(!tick(&mut rig, &mut owner, &mut ui, &request, true, &mut applied).unwrap());
    assert_eq!(state.borrow().replies, [expected_reply.clone()]);
    assert!(state
        .borrow()
        .attempts
        .iter()
        .all(|reply| *reply == expected_reply));
    assert_eq!(staged.get(), 1);
    assert_eq!(count(&trace, "submit"), 1);
    assert_eq!(count(&trace, "open"), 1);
    assert_eq!(trace.calls(), native);
    assert!(!ui.pending());
}

#[test]
fn retained_target_requests_admitted_work_continues_when_a_practice_transition_becomes_pending() {
    let request = Request::normal(32_000);
    let (mut rig, mut owner, mut trace, mut ui, state) = setup(false, &request);
    trace.conditional_statuses([
        TargetRetirementStatus::Pending,
        TargetRetirementStatus::Authorized,
    ]);
    assert!(!tick(
        &mut rig,
        &mut owner,
        &mut ui,
        &request,
        false,
        &mut |output| Ok(capability(output.applied_request()))
    )
    .unwrap());
    assert!(owner.has_work());
    assert!(owner.replacement_pending());
    assert!(!owner.output_clock_suspended());
    assert!(owner.current().is_some());
    assert_eq!(owner.state(), ReplacementState::ConditionalRetirement);
    assert!(drive(
        &mut rig,
        &mut owner,
        &mut ui,
        &request,
        true,
        &mut |output| Ok(capability(output.applied_request()))
    )
    .unwrap());
    assert_eq!(state.borrow().replies.len(), 1);
    assert!(state.borrow().replies[0].result.is_ok());
    assert_eq!(count(&trace, "submit"), 1);
    assert_eq!(count(&trace, "open"), 1);
    assert!(!owner.has_work());
}
