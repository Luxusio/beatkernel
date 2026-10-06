//! Shared real-model memory backend; no native handles or foreign callbacks.
use super::*;
pub(crate) use crate::gameplay_presentation_port_fixtures::*;
pub(crate) use crate::output_replacement::{
    OutputReplacementBackend, ReplacementState, ReplacementFailure, ReplacementCause,
    ReplacementPhase,
};
use crate::gameplay_presentation::GameplayPauseControl;
use crate::local_runtime::SoloRuntime;
use std::{cell::RefCell, rc::Rc};
#[derive(Debug)]
pub(crate) struct Fault(pub Box<u64>);
impl Fault {
    pub fn new(code: u64) -> Self {
        Self(Box::new(code))
    }
    pub fn pointer(&self) -> *const u64 {
        self.0.as_ref()
    }
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "memory output refusal {}", self.0)
    }
}
impl std::error::Error for Fault {}
#[derive(Default)]
pub(crate) struct Trace {
    pub calls: Vec<&'static str>,
    pub frames: u64,
    pub host_shift: i64,
    pub pcm: Vec<f32>,
    pub retire_failure: Option<Fault>,
    pub observed_epochs: Vec<u64>,
    pub drops: Vec<bool>,
}
pub(crate) struct Output {
    pub mixer: Option<Mixer>,
    pub epoch: u64,
    pub basis: OutputFrameBasis,
    pub report: Option<RenderReport>,
    pub quiet: usize,
    pub retired: bool,
    pub trace: Rc<RefCell<Trace>>,
}
impl Output {
    pub fn render(&mut self, frames: usize) {
        let mut pcm = vec![0.; frames];
        let mixer = self.mixer.as_mut().unwrap();
        self.report = Some(mixer.render(&mut pcm).unwrap());
        let mut trace = self.trace.borrow_mut();
        trace.frames = mixer.frame_cursor();
        trace.pcm.extend(pcm);
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.trace.borrow_mut().drops.push(self.retired);
    }
}
impl StoppedMixerSource for Output {
    type Error = Fault;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Fault> {
        self.trace.borrow_mut().calls.push("take");
        if !self.retired {
            return Err(Fault::new(90));
        }
        Ok(self.mixer.take())
    }
}
pub(crate) struct Request {
    pub id: Box<u64>,
    pub quiet: usize,
    pub open_error: Option<Fault>,
}
pub(crate) fn request(id: u64, quiet: usize) -> Request {
    Request {
        id: Box::new(id),
        quiet,
        open_error: None,
    }
}
pub(crate) struct Backend {
    pub trace: Rc<RefCell<Trace>>,
}
pub(crate) fn pair_frame(frame: u64) -> ClockPair {
    pair(frame as i64 * 1_000_000, frame as i64 * 1_000_000)
}
impl OutputReplacementBackend for Backend {
    type Presentation = PresentationEstimator;
    type Output = Output;
    type Request = Request;
    type Error = Fault;
    fn open(
        &mut self,
        request: Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<Fault, Output>> {
        self.trace.borrow_mut().calls.push("open");
        if let Some(error) = request.open_error {
            return Err(OutputOpenFailure::recovered(error, Some(mixer)));
        }
        let basis = mixer.output_frame_basis();
        let mut output = Output {
            mixer: Some(mixer),
            epoch,
            basis,
            report: None,
            quiet: request.quiet,
            retired: false,
            trace: self.trace.clone(),
        };
        output.render(10);
        assert!(output.report.unwrap().paused);
        Ok(output)
    }
    fn retire(&mut self, output: &mut Output) -> Result<(), Fault> {
        self.trace.borrow_mut().calls.push("retire");
        if let Some(error) = self.trace.borrow_mut().retire_failure.take() {
            return Err(error);
        }
        output.retired = true;
        Ok(())
    }
    fn start(&mut self, _: &mut Output) -> Result<(), Fault> {
        self.trace.borrow_mut().calls.push("start");
        Ok(())
    }
    fn epoch(&self, output: &Output) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Output) -> OutputFrameBasis {
        output.basis
    }
    fn observe(&mut self, output: &mut Output, p: &mut PresentationEstimator) -> Result<(), Fault> {
        self.trace.borrow_mut().calls.push("observe");
        if output.quiet > 0 {
            output.quiet -= 1;
            return Ok(());
        }
        if let Some(report) = output.report {
            let end = report.start_frame + report.frames as u64;
            let mut pair = pair_frame(end);
            pair.target.timestamp = Timestamp::from_nanos(
                pair.target.timestamp.as_nanos() + self.trace.borrow().host_shift,
            );
            p.observe_clock_pair_in_epoch(output.epoch, pair)
                .map_err(|_| Fault::new(91))?;
            self.trace.borrow_mut().observed_epochs.push(output.epoch);
        }
        Ok(())
    }
    fn render_report(&self, output: &Output) -> Result<Option<RenderReport>, Fault> {
        self.trace.borrow_mut().calls.push("report");
        Ok(output.report)
    }
}
pub(crate) fn initial(devices: Vec<DeviceId>) -> (Output, CommandProducer, Rc<RefCell<Trace>>) {
    initial_with_finite(devices, false)
}
fn initial_with_finite(
    devices: Vec<DeviceId>,
    finite: bool,
) -> (Output, CommandProducer, Rc<RefCell<Trace>>) {
    let (memory, producer) = device(finite, devices);
    let trace = Rc::new(RefCell::new(Trace::default()));
    let mixer = memory.mixer;
    let basis = mixer.output_frame_basis();
    (
        Output {
            mixer: Some(mixer),
            epoch: 0,
            basis,
            report: None,
            quiet: 0,
            retired: false,
            trace: trace.clone(),
        },
        producer,
        trace,
    )
}
pub(crate) fn settings() -> DisciplineConfig {
    DisciplineConfig {
        capacity: 8,
        min_span: Duration::from_nanos(500_000_000),
        ..Default::default()
    }
}
struct Fixture {
    owner: GameplayOutputOwner<Backend>,
    presentation: PresentationEstimator,
    pause: NativePause,
    config: NativeGameplayConfig,
    end: Option<NativeEnd>,
    runtime: SoloRuntime,
    trace: Rc<RefCell<Trace>>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_finite(false)
    }
    fn with_finite(finite: bool) -> Self {
        let (mut output, mut producer, trace) = initial_with_finite(vec![], finite);
        output.render(2);
        let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 1000).unwrap();
        if finite {
            pause = pause.with_playback_end_frame(10).unwrap();
        }
        pause.request(true, pair_frame(2)).unwrap();
        producer.request_pause(true);
        output.render(1);
        let boundary = pause
            .observe(output.report, pair_frame(3))
            .unwrap()
            .unwrap();
        output.render(2);
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(7),
                sample: SampleId(1),
                at: Timestamp::from_nanos(2_000_000),
                gain: 1.,
            })
            .unwrap();
        let mut transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
        transport.pause(boundary.host.timestamp).unwrap();
        transport
            .seek(boundary.host.timestamp, Timestamp::from_nanos(2_000_000))
            .unwrap();
        let mut runtime = SoloRuntime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            transport,
            bindings(None),
            judge(&source()),
            producer,
            vec![],
            8,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let mut presentation =
            PresentationEstimator::new(settings(), point(2, 0), ClockDomainId(1), Timestamp::ZERO)
                .unwrap();
        let mut owner = GameplayOutputOwner::new(
            Backend {
                trace: trace.clone(),
            },
            output,
        );
        assert!(owner.observe(&mut presentation).is_ok());
        assert!(
            owner
                .pause_observation(presentation.latest_pair().unwrap(), point(1, 5_000_000))
                .is_ok()
        );
        let mut config = config(false);
        let mut end =
            finite.then(|| NativeEnd::new(point(2, 0), ClockDomainId(1), 1000, 10).unwrap());
        if finite {
            config.end_song = Some(Timestamp::from_nanos(10_000_000));
            end.as_mut().unwrap().observe(None, pair_frame(0)).unwrap();
        }
        Self {
            owner,
            presentation,
            pause,
            config,
            end,
            runtime,
            trace,
        }
    }
    fn publish(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let trace = self.trace.borrow();
        let now = point(1, trace.frames as i64 * 1_000_000 + trace.host_shift);
        drop(trace);
        self.owner.publish_paused(
            GameplayOutputContext {
                presentation: &mut self.presentation,
                pause: &mut self.pause,
                config: &mut self.config,
                end: &mut self.end,
                control: GameplayPauseControl::solo(&mut self.runtime),
            },
            now,
        )
    }
}
#[test]
fn request_rejection_returns_original_typed_request_and_multipoll_wait_preserves_old_observer_report_and_pause_evidence()
 {
    let mut f = Fixture::new();
    assert!(f.owner.queue(request(11, 2), 100_000_000).is_ok());
    let second = request(12, 0);
    let pointer = second.id.as_ref() as *const u64;
    let (second, wait) = match f.owner.queue(second, 17) {
        Err(value) => value,
        Ok(()) => panic!("second request must remain owned by caller"),
    };
    assert_eq!(second.id.as_ref() as *const u64, pointer);
    assert_eq!(wait, 17);
    let old = format!("{:?}", f.presentation);
    let report = f.owner.render_report();
    let evidence = f
        .owner
        .pause_observation(f.presentation.latest_pair().unwrap(), point(1, 5_000_000))
        .unwrap();
    for _ in 0..2 {
        assert!(!f.publish().unwrap());
        assert!(f.owner.replacement_pending());
        assert_eq!(f.owner.state(), ReplacementState::Waiting);
        assert_eq!(format!("{:?}", f.presentation), old);
        assert_eq!(f.owner.render_report(), report);
        assert!(f.owner.observe(&mut f.presentation).is_ok());
        assert_eq!(format!("{:?}", f.presentation), old);
        assert_eq!(
            f.owner
                .pause_observation(f.presentation.latest_pair().unwrap(), point(1, 15_000_000))
                .unwrap(),
            evidence
        );
        assert!(f.runtime.hold_audio_pause().is_err());
        f.runtime.request_audio_pause(false);
    }
    assert!(f.publish().unwrap());
    assert!(!f.owner.replacement_pending());
    assert_eq!(f.presentation.epoch(), 1);
    assert_eq!(f.pause.epoch(), 1);
    assert_eq!(f.config.playback_origin, point(2, 5_000_000));
    assert_eq!(
        f.owner
            .current()
            .unwrap()
            .mixer
            .as_ref()
            .unwrap()
            .playback_frame_cursor(),
        2
    );
    f.runtime.request_audio_pause(false);
    f.owner.current_mut().unwrap().render(2);
    assert_eq!(&f.trace.borrow().pcm[15..17], &[0.25, 0.5]);
    assert!(f.owner.stop().is_ok());
}
#[test]
fn ready_publication_refusal_keeps_original_output_hold_and_live_clocks_until_explicit_cancel_recovers_model()
 {
    let mut f = Fixture::new();
    assert!(f.owner.queue(request(1, 2), 100_000_000).is_ok());
    assert!(!f.publish().unwrap());
    assert!(!f.publish().unwrap());
    f.config.sample_rate = 999;
    let before = format!("{:?}", f.presentation);
    let pause = format!("{:?}", f.pause);
    assert!(f.publish().is_err());
    assert!(f.owner.rejected_ready().is_some());
    assert!(f.owner.replacement_pending());
    assert_eq!(format!("{:?}", f.presentation), before);
    assert_eq!(format!("{:?}", f.pause), pause);
    assert_eq!(f.config.sample_rate, 999);
    assert!(f.runtime.hold_audio_pause().is_err());
    f.runtime.request_audio_pause(false);
    assert!(
        f.owner
            .rejected_ready()
            .unwrap()
            .output
            .mixer
            .as_ref()
            .unwrap()
            .pause_requested()
    );
    assert!(matches!(f.owner.cancel(), Ok(true)));
    assert!(f.owner.rejected_ready().is_none());
    assert!(!f.owner.replacement_pending());
    let mixer = f.owner.take_recovered_mixer().unwrap();
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (15, 2)
    );
    assert!(mixer.pause_requested());
}
#[test]
fn failed_open_and_retirement_preserve_original_boxed_failure_and_recovery_diagnostics_without_implicit_fallback()
 {
    for retire in [false, true] {
        let mut f = Fixture::new();
        let original = Fault::new(31);
        let pointer = original.pointer();
        let mut req = request(1, 0);
        if retire {
            f.trace.borrow_mut().retire_failure = Some(original);
        } else {
            req.open_error = Some(original);
        }
        assert!(f.owner.queue(req, 100_000_000).is_ok());
        let error = f.publish().unwrap_err();
        let failure = error.downcast_ref::<ReplacementFailure<Fault>>().unwrap();
        match &failure.cause {
            ReplacementCause::Backend { phase, error } => {
                assert_eq!(error.pointer(), pointer);
                assert_eq!(
                    *phase,
                    if retire {
                        ReplacementPhase::Retire
                    } else {
                        ReplacementPhase::Open
                    }
                );
            }
            _ => panic!("original backend cause required"),
        }
        assert_eq!(f.presentation.epoch(), 0);
        assert_eq!(f.pause.epoch(), 0);
        if retire {
            assert!(failure.recovery.is_some());
            assert_eq!(f.owner.state(), ReplacementState::PendingRetirement);
            assert!(matches!(f.owner.retry_retirement(), Ok(true)));
        } else {
            assert_eq!(f.owner.last_issued_epoch(), 1);
            assert_eq!(f.owner.state(), ReplacementState::RecoveredMixer);
        }
        assert_eq!(
            f.owner
                .take_recovered_mixer()
                .unwrap()
                .playback_frame_cursor(),
            2
        );
    }
}
#[test]
fn explicit_stop_retains_current_owner_and_original_retirement_failure_then_can_retire_without_fabricated_recovery()
 {
    let mut f = Fixture::new();
    let error = Fault::new(41);
    let pointer = error.pointer();
    f.trace.borrow_mut().retire_failure = Some(error);
    let failure = match f.owner.stop() {
        Err(error) => error,
        Ok(()) => panic!("injected stop refusal required"),
    };
    match failure.cause {
        ReplacementCause::Backend { error, .. } => assert_eq!(error.pointer(), pointer),
        _ => panic!("original stop cause required"),
    }
    assert!(f.owner.current().is_some());
    assert!(!f.owner.current().unwrap().retired);
    assert!(f.owner.take_recovered_mixer().is_none());
    assert!(f.owner.stop().is_ok());
    assert!(f.owner.current().unwrap().retired);
    assert_eq!(
        f.owner
            .current()
            .unwrap()
            .mixer
            .as_ref()
            .unwrap()
            .playback_frame_cursor(),
        2
    );
}
#[test]
fn malformed_current_epoch_or_basis_refuses_before_backend_observation_and_preserves_genuine_cached_report()
 {
    let mut f = Fixture::new();
    let before = format!("{:?}", f.presentation);
    let report = f.owner.render_report();
    let calls = f.trace.borrow().calls.len();
    f.owner.current_mut().unwrap().epoch = 2;
    assert!(f.owner.observe(&mut f.presentation).is_err());
    assert_eq!(format!("{:?}", f.presentation), before);
    assert_eq!(f.owner.render_report(), report);
    assert_eq!(f.trace.borrow().calls.len(), calls);
    f.owner.current_mut().unwrap().epoch = 0;
    let basis = f.owner.current().unwrap().basis;
    f.owner.current_mut().unwrap().basis = OutputFrameBasis::new(point(2, 1), 1000, 0).unwrap();
    assert!(f.owner.observe(&mut f.presentation).is_err());
    assert_eq!(format!("{:?}", f.presentation), before);
    assert_eq!(f.owner.render_report(), report);
    assert_eq!(f.trace.borrow().calls.len(), calls);
    f.owner.current_mut().unwrap().basis = basis;
    assert!(f.owner.stop().is_ok());
}
#[test]
fn explicit_waiting_cancel_recovers_only_actual_primed_model_and_lease_release_still_requires_new_resume()
 {
    let mut f = Fixture::new();
    assert!(f.owner.queue(request(1, 2), 100_000_000).is_ok());
    let before = format!("{:?}", f.presentation);
    assert!(!f.publish().unwrap());
    assert!(f.owner.replacement_pending());
    assert!(f.runtime.hold_audio_pause().is_err());
    f.runtime.request_audio_pause(false);
    assert!(matches!(f.owner.cancel(), Ok(true)));
    assert!(!f.owner.replacement_pending());
    assert_eq!(format!("{:?}", f.presentation), before);
    assert_eq!(f.owner.last_issued_epoch(), 1);
    assert!(f.owner.current().is_none());
    let mut mixer = f.owner.take_recovered_mixer().unwrap();
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (15, 2)
    );
    assert!(mixer.pause_requested());
    let mut pcm = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.; 2]);
    f.runtime.request_audio_pause(false);
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.5]);
}
#[test]
fn finite_endpoint_passes_actual_controller_publication_and_owner_end_projection_using_only_new_output_host_bracket()
 {
    let mut f = Fixture::with_finite(true);
    assert!(f.owner.queue(request(1, 2), 2_000_000_000).is_ok());
    assert!(!f.publish().unwrap());
    // Supplied host-clock model advances during handoff; the original finite
    // lower bracket at frame0/host0 must never be mixed with the new relation.
    f.trace.borrow_mut().host_shift = 1_000_000_000;
    assert!(!f.publish().unwrap());
    assert!(f.publish().unwrap());
    assert_eq!(f.presentation.epoch(), 1);
    assert_eq!(f.config.end_song, Some(Timestamp::from_nanos(10_000_000)));
    f.runtime.request_audio_pause(false);
    f.owner.current_mut().unwrap().render(12);
    let report = f.owner.current().unwrap().report.unwrap();
    assert_eq!(report.playback_end_physical_frame, Some(23));
    assert_eq!(
        report.playback_start_frame + report.playback_frames as u64,
        10
    );
    assert!(f.owner.observe(&mut f.presentation).is_ok());
    let boundary = f
        .owner
        .observe_end(f.end.as_mut().unwrap(), &f.presentation)
        .unwrap()
        .unwrap();
    assert_eq!(boundary.physical_frame, 23);
    assert_eq!(boundary.playback_frame, 10);
    assert_eq!(boundary.output, point(2, 23_000_000));
    assert_eq!(boundary.host, point(1, 1_023_000_000));
    assert_eq!(
        f.owner
            .observe_end(f.end.as_mut().unwrap(), &f.presentation)
            .unwrap(),
        None
    );
    assert!(f.owner.stop().is_ok());
}
