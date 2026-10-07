//! Correlated UI requests drive the genuine held native-output publisher.
use super::*;
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    gameplay::output::{
        application::owner::fixtures as memory, ports::OriginalNativeOutputBackend,
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayPauseControl},
    local_input::InputMerger,
    local_runtime::SoloRuntime,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_end::NativeEnd,
    native_gameplay::NativeGameplayConfig,
};
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource},
    input::DeviceId,
    runtime::RuntimeProcessingClock,
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, Duration, ExtrapolationPolicy,
        Timestamp,
    },
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::presentation::validation::{
    NativePresentationValidator, OriginalNativePresentationEvidence,
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

fn host(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(1),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn raw(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(2),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn logical(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(3),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn pair(nanos: i64) -> ClockPair {
    ClockPair {
        source: raw(nanos),
        target: host(nanos),
    }
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
struct Output(memory::Output);
impl StoppedMixerSource for Output {
    type Error = io::Error;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, io::Error> {
        self.0.take_stopped_mixer().map_err(io::Error::other)
    }
}
struct Request {
    native: memory::Request,
    open_error: Option<String>,
}
#[derive(Default)]
struct Counts {
    opens: usize,
    starts: usize,
    native: usize,
}
struct Backend {
    inner: memory::Backend,
    counts: Rc<RefCell<Counts>>,
}
impl OutputReplacementBackend for Backend {
    type Presentation = beatkernel::time::presentation::PresentationEstimator;
    type Output = Output;
    type Request = Request;
    type Error = io::Error;
    fn open(
        &mut self,
        request: Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<io::Error, Output>> {
        self.counts.borrow_mut().opens += 1;
        if let Some(error) = request.open_error {
            return Err(OutputOpenFailure::recovered(
                io::Error::other(error),
                Some(mixer),
            ));
        }
        match self.inner.open(request.native, mixer, epoch) {
            Ok(output) => Ok(Output(output)),
            Err(failure) => {
                let (error, mixer, pending, cleanup) = failure.into_parts();
                let mut failure = match pending {
                    Some(output) => {
                        OutputOpenFailure::pending(io::Error::other(error), Output(output))
                    }
                    None => OutputOpenFailure::recovered(io::Error::other(error), mixer),
                };
                if let Some(error) = cleanup {
                    failure = failure.with_cleanup_error(io::Error::other(error));
                }
                Err(failure)
            }
        }
    }
    fn retire(&mut self, output: &mut Output) -> Result<(), io::Error> {
        self.inner.retire(&mut output.0).map_err(io::Error::other)
    }
    fn start(&mut self, output: &mut Output) -> Result<(), io::Error> {
        self.counts.borrow_mut().starts += 1;
        self.inner.start(&mut output.0).map_err(io::Error::other)
    }
    fn epoch(&self, output: &Output) -> u64 {
        output.0.epoch
    }
    fn basis(&self, output: &Output) -> OutputFrameBasis {
        output.0.basis
    }
    fn observe(&mut self, _: &mut Output, _: &mut Self::Presentation) -> Result<(), io::Error> {
        panic!("audio UI requests cannot route through legacy observation")
    }
    fn render_report(&self, output: &Output) -> Result<Option<RenderReport>, io::Error> {
        self.inner
            .render_report(&output.0)
            .map_err(io::Error::other)
    }
}
impl OriginalNativeOutputBackend for Backend {
    fn observe_native(
        &mut self,
        output: &mut Output,
    ) -> Result<Option<NativeAudioSnapshot>, io::Error> {
        self.counts.borrow_mut().native += 1;
        output.0.render(1);
        let report = output.0.report.unwrap();
        assert!(report.paused);
        assert_eq!(report.playback_start_frame, 2);
        assert_eq!(report.playback_frames, 0);
        let nanos = (report.start_frame + report.frames as u64) as i64 * 1_000_000;
        Ok(Some(NativeAudioSnapshot {
            epoch: output.0.epoch,
            basis: output.0.basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(nanos)),
        }))
    }
}
#[derive(Default)]
struct PortState {
    requests: VecDeque<OutputRequest>,
    replies: Vec<OutputReply>,
    attempts: Vec<OutputReply>,
    takes: usize,
    blocked: bool,
    advertised: Vec<Option<OutputCapability>>,
}
struct Port(Rc<RefCell<PortState>>);
impl OutputUiPort for Port {
    fn advertise(&mut self, capability: Option<OutputCapability>) -> io::Result<()> {
        self.0.borrow_mut().advertised.push(capability);
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
        state.replies.push(reply.clone());
        Ok(())
    }
    fn pending(&self) -> bool {
        !self.0.borrow().requests.is_empty()
    }
}
fn args() -> Vec<String> {
    [
        "--alsa",
        "memory",
        "--buffer-frames",
        "128",
        "--period-frames",
        "32",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
fn capability() -> OutputCapability {
    OutputCapability {
        host: crate::settings::SettingsHost::Linux,
        current_args: args(),
    }
}
fn ui_request(id: u64) -> OutputRequest {
    OutputRequest { id, args: args() }
}
fn mapped(request: &OutputRequest, _: &Output) -> Result<Request, String> {
    Ok(Request {
        native: memory::request(request.id, 0),
        open_error: None,
    })
}
fn applied(output: &Output) -> Result<OutputCapability, String> {
    assert_eq!(output.0.epoch, 1);
    assert_eq!(output.0.mixer.as_ref().unwrap().playback_frame_cursor(), 2);
    Ok(capability())
}
struct Rig {
    owner: GameplayOutputOwner<Backend>,
    presentation: NativeAudioPresentation,
    pause: crate::playback_pause::NativePause,
    merger: InputMerger,
    runtime: SoloRuntime,
    config: NativeGameplayConfig,
    end: Option<NativeEnd>,
    trace: Rc<RefCell<memory::Trace>>,
    counts: Rc<RefCell<Counts>>,
}
impl Rig {
    fn new() -> Self {
        let (mut output, mut producer, trace) = memory::initial(vec![]);
        output.render(2);
        let mut pause =
            crate::playback_pause::NativePause::new(raw(0), ClockDomainId(1), 1000).unwrap();
        pause.request(true, pair(2_000_000)).unwrap();
        producer.request_pause(true);
        output.render(1);
        pause
            .observe(output.report, pair(3_000_000))
            .unwrap()
            .unwrap();
        output.render(2);
        let authority = AudioAuthority::new(
            AudioAuthorityConfig {
                history_capacity: 8,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
            AudioAuthorityEpoch {
                id: 0,
                stream_origin: raw(0),
                logical_origin: logical(0),
                host_domain: ClockDomainId(1),
            },
        )
        .unwrap();
        let mut presentation = NativeAudioPresentation::new(
            authority,
            NativePresentationValidator::new(0, raw(0), ClockDomainId(1)),
        )
        .unwrap();
        for nanos in [1_000_000, 3_000_000] {
            presentation
                .admit(NativeAudioSnapshot {
                    epoch: 0,
                    basis: output.basis,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(nanos)),
                })
                .unwrap();
        }
        let mut runtime = SoloRuntime::new(
            ClockDomainId(3),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            memory::bindings(None),
            memory::judge(&memory::source()),
            producer,
            vec![],
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let mut merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
        presentation
            .authority_mut()
            .record_acquired_prefix(host(5_000_000))
            .unwrap();
        let cutoff = presentation
            .authority()
            .prepare_control_cutoff(0, raw(2_000_000), host(2_000_000), host(5_000_000), &merger)
            .unwrap()
            .unwrap();
        let report = runtime
            .advance_to(cutoff.output(), &Identity, raw(4_000_000))
            .unwrap();
        assert_eq!(report.song_time, Timestamp::from_nanos(2_000_000));
        presentation
            .authority_mut()
            .commit_control_cutoff(cutoff, &merger)
            .unwrap();
        runtime
            .transport_mut()
            .pause(cutoff.output().timestamp)
            .unwrap();
        let held = presentation
            .authority()
            .prepare_held_frontier(host(5_000_000), &merger)
            .unwrap()
            .unwrap();
        presentation
            .authority_mut()
            .commit_held_frontier(held, &mut merger)
            .unwrap();
        let counts = Rc::new(RefCell::new(Counts::default()));
        let owner = GameplayOutputOwner::new(
            Backend {
                inner: memory::Backend {
                    trace: trace.clone(),
                },
                counts: counts.clone(),
            },
            Output(output),
        );
        Self {
            owner,
            presentation,
            pause,
            merger,
            runtime,
            config: memory::config(false),
            end: None,
            trace,
            counts,
        }
    }
    fn service(
        &mut self,
        ui: &mut GameplayOutputUi<Port>,
        now: i64,
        map: &mut impl FnMut(&OutputRequest, &Output) -> Result<Request, String>,
        applied: &mut impl FnMut(&Output) -> Result<OutputCapability, String>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        ui.service_audio(
            &mut self.owner,
            GameplayAudioOutputContext {
                control: GameplayPauseControl::solo(&mut self.runtime),
                presentation: &mut self.presentation,
                merger: &self.merger,
                pause: &mut self.pause,
                config: &mut self.config,
                end: &mut self.end,
            },
            host(now),
            map,
            applied,
        )
    }
}

#[test]
fn correlated_audio_request_requires_two_native_anchors_then_replies_with_exact_published_capability(
) {
    let mut rig = Rig::new();
    let port = Rc::new(RefCell::new(PortState::default()));
    port.borrow_mut().requests.push_back(ui_request(u64::MAX));
    let mut ui = GameplayOutputUi::new(Port(port.clone()));
    ui.advertise(Some(capability())).unwrap();
    let before = format!("{:?}", rig.presentation);
    assert!(!rig
        .service(&mut ui, 20_000_000, &mut mapped, &mut applied)
        .unwrap());
    assert!(ui.pending());
    assert!(port.borrow().replies.is_empty());
    assert_eq!(port.borrow().takes, 1);
    assert_eq!(rig.counts.borrow().native, 1);
    assert_eq!(format!("{:?}", rig.presentation), before);
    assert!(rig.owner.current().is_none());
    assert!(rig
        .service(&mut ui, 21_000_000, &mut mapped, &mut applied)
        .unwrap());
    assert!(!ui.pending());
    assert_eq!(rig.owner.current().unwrap().0.epoch, 1);
    assert_eq!(rig.presentation.authority().epoch().id, 1);
    assert_eq!(rig.pause.epoch(), 1);
    assert_eq!(rig.counts.borrow().native, 2);
    assert_eq!(rig.counts.borrow().opens, 1);
    assert_eq!(rig.counts.borrow().starts, 1);
    assert_eq!(
        port.borrow().replies,
        [OutputReply {
            id: u64::MAX,
            result: Ok(capability())
        }]
    );
    assert_eq!(port.borrow().advertised, [Some(capability())]);
    assert_eq!(
        rig.presentation.authority().committed_operation(),
        Some(logical(2_000_000))
    );
    assert_eq!(
        rig.owner
            .current()
            .unwrap()
            .0
            .mixer
            .as_ref()
            .unwrap()
            .playback_frame_cursor(),
        2
    );
    rig.owner.stop().unwrap();
}

#[test]
fn typed_mapping_refusal_returns_correlated_error_before_any_retirement_or_native_io() {
    let mut rig = Rig::new();
    let port = Rc::new(RefCell::new(PortState::default()));
    port.borrow_mut().requests.push_back(ui_request(17));
    let mut ui = GameplayOutputUi::new(Port(port.clone()));
    let before = format!("{:?}", rig.presentation);
    let calls = rig.trace.borrow().calls.clone();
    let mut map = |request: &OutputRequest, output: &Output| {
        assert_eq!(request, &ui_request(17));
        assert_eq!(output.0.epoch, 0);
        Err("typed device selection refused".to_string())
    };
    let mut applied = |_: &Output| -> Result<OutputCapability, String> {
        panic!("mapping refusal cannot publish capability")
    };
    assert!(!rig
        .service(&mut ui, 20_000_000, &mut map, &mut applied)
        .unwrap());
    assert_eq!(
        port.borrow().replies,
        [OutputReply {
            id: 17,
            result: Err("typed device selection refused".into())
        }]
    );
    assert_eq!(format!("{:?}", rig.presentation), before);
    assert_eq!(rig.trace.borrow().calls, calls);
    assert_eq!(rig.counts.borrow().opens, 0);
    assert_eq!(rig.counts.borrow().native, 0);
    assert_eq!(rig.owner.current().unwrap().0.epoch, 0);
    assert!(!ui.pending());
    rig.owner.stop().unwrap();
}

#[test]
fn would_block_reply_retains_exact_success_without_consuming_another_request_or_republishing() {
    let mut rig = Rig::new();
    let port = Rc::new(RefCell::new(PortState::default()));
    port.borrow_mut().requests.push_back(ui_request(11));
    port.borrow_mut().blocked = true;
    let mut ui = GameplayOutputUi::new(Port(port.clone()));
    assert!(!rig
        .service(&mut ui, 20_000_000, &mut mapped, &mut applied)
        .unwrap());
    assert!(rig
        .service(&mut ui, 21_000_000, &mut mapped, &mut applied)
        .unwrap());
    let expected = OutputReply {
        id: 11,
        result: Ok(capability()),
    };
    assert_eq!(port.borrow().attempts, [expected.clone()]);
    assert!(port.borrow().replies.is_empty());
    assert!(ui.pending());
    port.borrow_mut().requests.push_back(ui_request(12));
    let epoch = rig.owner.current().unwrap().0.epoch;
    let before = format!("{:?}", rig.presentation);
    let mut forbidden_map = |_: &OutputRequest, _: &Output| -> Result<Request, String> {
        panic!("blocked reply must precede next admission")
    };
    let mut forbidden_applied = |_: &Output| -> Result<OutputCapability, String> {
        panic!("successful publication must not repeat")
    };
    for now in [22_000_000, 23_000_000] {
        assert!(!rig
            .service(&mut ui, now, &mut forbidden_map, &mut forbidden_applied)
            .unwrap());
    }
    assert_eq!(port.borrow().takes, 1);
    assert_eq!(port.borrow().requests.len(), 1);
    assert!(port
        .borrow()
        .attempts
        .iter()
        .all(|reply| reply == &expected));
    assert_eq!(rig.counts.borrow().opens, 1);
    assert_eq!(rig.counts.borrow().native, 2);
    assert_eq!(rig.owner.current().unwrap().0.epoch, epoch);
    assert_eq!(format!("{:?}", rig.presentation), before);
    port.borrow_mut().blocked = false;
    let second = port.borrow_mut().requests.pop_front().unwrap();
    assert_eq!(second.id, 12);
    assert!(!rig
        .service(
            &mut ui,
            24_000_000,
            &mut forbidden_map,
            &mut forbidden_applied
        )
        .unwrap());
    assert_eq!(port.borrow().replies, [expected]);
    assert!(!ui.pending());
    assert_eq!(rig.counts.borrow().opens, 1);
    rig.owner.stop().unwrap();
}

#[test]
fn raw_backend_error_is_preserved_for_caller_but_ui_reply_removes_controls_and_limits_512_characters(
) {
    let mut rig = Rig::new();
    let port = Rc::new(RefCell::new(PortState::default()));
    port.borrow_mut().requests.push_back(ui_request(33));
    let mut ui = GameplayOutputUi::new(Port(port.clone()));
    let raw_error = format!("native\n\t\u{7f}拒否{}", "界".repeat(700));
    let expected_raw = raw_error.clone();
    let mut map = move |request: &OutputRequest, _: &Output| {
        Ok(Request {
            native: memory::request(request.id, 0),
            open_error: Some(raw_error.clone()),
        })
    };
    let before = format!("{:?}", rig.presentation);
    let failure = rig
        .service(&mut ui, 20_000_000, &mut map, &mut applied)
        .unwrap_err();
    let original = failure.to_string();
    assert!(original.contains(&expected_raw));
    let expected: String = original
        .chars()
        .filter(|ch| !ch.is_control())
        .take(512)
        .collect();
    assert_eq!(
        port.borrow().replies,
        [OutputReply {
            id: 33,
            result: Err(expected.clone())
        }]
    );
    assert_eq!(expected.chars().count(), 512);
    assert!(!expected.chars().any(char::is_control));
    assert_eq!(format!("{:?}", rig.presentation), before);
    assert_eq!(rig.counts.borrow().opens, 1);
    assert_eq!(rig.counts.borrow().native, 0);
    assert_eq!(
        rig.owner.state(),
        crate::output_replacement::ReplacementState::RecoveredMixer
    );
    assert!(!ui.pending());
    assert!(rig.owner.take_recovered_mixer().unwrap().is_paused());
}
