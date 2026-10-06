//! Cross-backend switching through the actual owner, controller and memory Mixer.
use super::*;
use crate::{
    gameplay_output_owner::{GameplayOutputOwner, fixtures::*},
    gameplay_presentation::{GameplayOutputContext, GameplayPauseControl},
    local_runtime::SoloRuntime,
};
use std::{cell::RefCell, rc::Rc};

/// Distinct second adapter type whose original-evidence overrides must be forwarded.
struct Marked {
    inner: Backend,
    pending_open: Option<(Fault, Fault)>,
    marks: Rc<RefCell<Vec<&'static str>>>,
}
impl OutputReplacementBackend for Marked {
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
        let output = self.inner.open(request, mixer, epoch)?;
        match self.pending_open.take() {
            Some((error, cleanup)) => {
                Err(OutputOpenFailure::pending(error, output).with_cleanup_error(cleanup))
            }
            None => Ok(output),
        }
    }
    fn retire(&mut self, output: &mut Output) -> Result<(), Fault> {
        self.inner.retire(output)
    }
    fn start(&mut self, output: &mut Output) -> Result<(), Fault> {
        self.inner.start(output)
    }
    fn epoch(&self, output: &Output) -> u64 {
        self.inner.epoch(output)
    }
    fn basis(&self, output: &Output) -> OutputFrameBasis {
        self.inner.basis(output)
    }
    fn observe(&mut self, output: &mut Output, p: &mut PresentationEstimator) -> Result<(), Fault> {
        self.inner.observe(output, p)
    }
    fn observe_end(
        &self,
        output: &Output,
        end: &mut NativeEnd,
        pair: ClockPair,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.marks.borrow_mut().push("end");
        self.inner.observe_end(output, end, pair, report)
    }
    fn render_report(&self, output: &Output) -> Result<Option<RenderReport>, Fault> {
        self.inner.render_report(output)
    }
    fn pause_observation(
        &self,
        output: &Output,
        pair: ClockPair,
        now: ClockPoint,
    ) -> Result<LivePauseObservation, Fault> {
        self.marks.borrow_mut().push("pause");
        self.inner.pause_observation(output, pair, now)
    }
}
type Switch = OutputBackendSwitch<Backend, Marked>;
type SwitchError = Switched<Fault, Fault>;
struct Fixture {
    owner: GameplayOutputOwner<Switch>,
    presentation: PresentationEstimator,
    pause: NativePause,
    config: NativeGameplayConfig,
    end: Option<NativeEnd>,
    runtime: SoloRuntime,
    first: Rc<RefCell<Trace>>,
    second: Rc<RefCell<Trace>>,
    marks: Rc<RefCell<Vec<&'static str>>>,
}
impl Fixture {
    fn new(pending_open: Option<(Fault, Fault)>) -> Self {
        let (mut output, mut producer, first) = initial(vec![]);
        output.render(2);
        let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 1000).unwrap();
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
        let second = Rc::new(RefCell::new(Trace::default()));
        let marks = Rc::new(RefCell::new(Vec::new()));
        let switch = OutputBackendSwitch::new(
            Backend {
                trace: first.clone(),
            },
            Marked {
                inner: Backend {
                    trace: second.clone(),
                },
                pending_open,
                marks: marks.clone(),
            },
        );
        let mut owner = GameplayOutputOwner::new(switch, Switched::First(output));
        assert!(owner.observe(&mut presentation).is_ok());
        assert!(
            owner
                .pause_observation(presentation.latest_pair().unwrap(), point(1, 5_000_000))
                .is_ok()
        );
        assert!(marks.borrow().is_empty());
        Self {
            owner,
            presentation,
            pause,
            config: config(false),
            end: None,
            runtime,
            first,
            second,
            marks,
        }
    }
    fn publish(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        // Both sides render the one shared Mixer; its cursor is the later trace.
        let frames = self.first.borrow().frames.max(self.second.borrow().frames);
        self.owner.publish_paused(
            GameplayOutputContext {
                presentation: &mut self.presentation,
                pause: &mut self.pause,
                config: &mut self.config,
                end: &mut self.end,
                control: GameplayPauseControl::solo(&mut self.runtime),
            },
            point(1, frames as i64 * 1_000_000),
        )
    }
    fn publish_until_ready(&mut self) -> usize {
        for polls in 1..=4 {
            if self.publish().unwrap() {
                return polls;
            }
            assert!(self.owner.replacement_pending());
        }
        panic!("switched output never produced genuine ready evidence");
    }
}
fn backend_cause(failure: &ReplacementFailure<SwitchError>) -> (ReplacementPhase, &SwitchError) {
    match &failure.cause {
        ReplacementCause::Backend { phase, error } => (*phase, error),
        _ => panic!("original switched backend cause required"),
    }
}
#[test]
fn queued_switch_moves_one_paused_mixer_between_static_backends_and_back_with_monotonic_epochs() {
    let mut f = Fixture::new(None);
    assert!(
        f.owner
            .queue(Switched::Second(request(1, 2)), 100_000_000)
            .is_ok()
    );
    assert_eq!(f.publish_until_ready(), 3);
    assert_eq!(f.presentation.epoch(), 1);
    assert_eq!(f.pause.epoch(), 1);
    assert_eq!(f.owner.last_issued_epoch(), 1);
    assert_eq!(f.owner.current().unwrap().second().unwrap().epoch, 1);
    assert!(f.first.borrow().calls.ends_with(&["retire", "take"]));
    assert!(!f.first.borrow().calls.contains(&"open"));
    assert!(f.second.borrow().calls.starts_with(&["open", "start"]));
    assert_eq!(f.first.borrow().drops, [true]);
    // Ready polling and committed publication both used the second side's overrides.
    assert_eq!(*f.marks.borrow(), ["pause", "pause"]);
    assert!(f.owner.observe(&mut f.presentation).is_ok());
    let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 1000, 10).unwrap();
    assert!(f.owner.observe_end(&mut end, &f.presentation).is_ok());
    assert_eq!(f.marks.borrow().last(), Some(&"end"));

    assert!(
        f.owner
            .queue(Switched::First(request(2, 2)), 100_000_000)
            .is_ok()
    );
    assert_eq!(f.publish_until_ready(), 3);
    assert_eq!(f.presentation.epoch(), 2);
    assert_eq!(f.pause.epoch(), 2);
    assert_eq!(f.owner.last_issued_epoch(), 2);
    assert_eq!(f.owner.current().unwrap().first().unwrap().epoch, 2);
    assert!(f.second.borrow().calls.ends_with(&["retire", "take"]));
    assert_eq!(f.second.borrow().drops, [true]);
    assert_eq!(
        f.first
            .borrow()
            .calls
            .iter()
            .filter(|c| **c == "open")
            .count(),
        1
    );
    let marks = f.marks.borrow().len();
    assert!(f.owner.observe(&mut f.presentation).is_ok());
    assert!(
        f.owner
            .pause_observation(f.presentation.latest_pair().unwrap(), point(1, 40_000_000))
            .is_ok()
    );
    assert_eq!(f.marks.borrow().len(), marks);

    // The pause cursor survived both handoffs: resume renders the queued voice once.
    f.runtime.request_audio_pause(false);
    let output = f.owner.current_mut().unwrap().first_mut().unwrap();
    assert_eq!(output.mixer.as_ref().unwrap().playback_frame_cursor(), 2);
    output.render(2);
    let pcm = f.first.borrow().pcm.clone();
    assert_eq!(&pcm[pcm.len() - 2..], &[0.25, 0.5]);
    assert!(f.owner.stop().is_ok());
}
#[test]
fn second_side_open_refusal_keeps_original_payload_and_mixer_without_fallback_until_explicit_first_request()
 {
    let mut f = Fixture::new(None);
    let original = Fault::new(31);
    let pointer = original.pointer();
    let mut refused = request(1, 0);
    refused.open_error = Some(original);
    assert!(
        f.owner
            .queue(Switched::Second(refused), 100_000_000)
            .is_ok()
    );
    let error = f.publish().unwrap_err();
    let failure = error
        .downcast_ref::<ReplacementFailure<SwitchError>>()
        .unwrap();
    assert_eq!(
        failure.to_string(),
        "output replacement Open: memory output refusal 31"
    );
    match backend_cause(failure) {
        (ReplacementPhase::Open, Switched::Second(error)) => assert_eq!(error.pointer(), pointer),
        _ => panic!("second side open refusal required"),
    }
    assert!(failure.cleanup.is_none() && failure.recovery.is_none());
    assert_eq!(f.owner.state(), ReplacementState::RecoveredMixer);
    assert_eq!(f.owner.last_issued_epoch(), 1);
    assert_eq!(f.presentation.epoch(), 0);
    assert!(f.owner.current().is_none());
    assert!(!f.first.borrow().calls.contains(&"open"));
    assert!(f.marks.borrow().is_empty());

    assert!(
        f.owner
            .queue(Switched::First(request(2, 2)), 100_000_000)
            .is_ok()
    );
    assert_eq!(f.publish_until_ready(), 3);
    assert_eq!(f.presentation.epoch(), 2);
    assert_eq!(f.owner.current().unwrap().first().unwrap().epoch, 2);
    assert_eq!(
        f.owner
            .current()
            .unwrap()
            .first()
            .unwrap()
            .mixer
            .as_ref()
            .unwrap()
            .playback_frame_cursor(),
        2
    );
    assert!(f.owner.stop().is_ok());
}
#[test]
fn pending_second_side_open_retags_owner_and_cleanup_then_explicit_retry_recovers_shared_mixer() {
    let (error, cleanup) = (Fault::new(51), Fault::new(52));
    let pointers = (error.pointer(), cleanup.pointer());
    let mut f = Fixture::new(Some((error, cleanup)));
    assert!(
        f.owner
            .queue(Switched::Second(request(1, 0)), 100_000_000)
            .is_ok()
    );
    let error = f.publish().unwrap_err();
    let failure = error
        .downcast_ref::<ReplacementFailure<SwitchError>>()
        .unwrap();
    match backend_cause(failure) {
        (ReplacementPhase::Open, Switched::Second(error)) => {
            assert_eq!(error.pointer(), pointers.0)
        }
        _ => panic!("second side pending open refusal required"),
    }
    match &failure.cleanup {
        Some(Switched::Second(cleanup)) => assert_eq!(cleanup.pointer(), pointers.1),
        _ => panic!("second side cleanup diagnostic required"),
    }
    assert_eq!(f.owner.state(), ReplacementState::PendingRetirement);
    assert!(f.owner.take_recovered_mixer().is_none());
    assert!(f.second.borrow().drops.is_empty());
    assert!(matches!(f.owner.retry_retirement(), Ok(true)));
    assert_eq!(f.second.borrow().drops, [true]);
    assert!(!f.first.borrow().calls.contains(&"open"));
    let mixer = f.owner.take_recovered_mixer().unwrap();
    assert_eq!(mixer.playback_frame_cursor(), 2);
    assert!(mixer.pause_requested());
}
#[test]
fn current_side_retirement_refusal_keeps_first_owner_pending_and_never_opens_second_side() {
    let mut f = Fixture::new(None);
    let original = Fault::new(41);
    let pointer = original.pointer();
    f.first.borrow_mut().retire_failure = Some(original);
    assert!(
        f.owner
            .queue(Switched::Second(request(1, 0)), 100_000_000)
            .is_ok()
    );
    let error = f.publish().unwrap_err();
    let failure = error
        .downcast_ref::<ReplacementFailure<SwitchError>>()
        .unwrap();
    match backend_cause(failure) {
        (ReplacementPhase::Retire, Switched::First(error)) => assert_eq!(error.pointer(), pointer),
        _ => panic!("first side retirement refusal required"),
    }
    match &failure.recovery {
        Some(Switched::First(error)) => assert_eq!(*error.0, 90),
        _ => panic!("first side recovery diagnostic required"),
    }
    assert_eq!(f.owner.state(), ReplacementState::PendingRetirement);
    assert!(f.second.borrow().calls.is_empty());
    assert!(f.first.borrow().drops.is_empty());
    assert!(matches!(f.owner.retry_retirement(), Ok(true)));
    assert_eq!(f.first.borrow().drops, [true]);
    assert_eq!(
        f.owner
            .take_recovered_mixer()
            .unwrap()
            .playback_frame_cursor(),
        2
    );
}
