//! Real Source controller/Owner and NativeOutputState, with only native effects scripted.
use super::{source_frame_observations::PendingSourceFrameObservations, test_support::*, *};
use crate::gameplay::output::{
    application::{
        owner::{CoordinateSourceOwnerPublication, GameplayOutputOwner},
        replacement::{ReplacementCause, ReplacementPhase, ReplacementState},
    },
    ports::{
        NativeSourcePlaybackQuery, OriginalCoordinateNativeOutputBackend,
        SourcePlaybackNativeOutputBackend,
    },
};
use crate::gameplay_presentation::{GameplayAudioOutputContext, GameplayPauseControl};
use beatkernel::{audio::*, time::ClockDomainId};
use beatkernel_platform::audio::NativeOutputState;
use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
};

const FIRST_FUTURE: i64 = HOST_START + 20_000_000;
const CALLBACK_NS: i64 = 2_000_000;
#[derive(Default)]
struct Trace {
    reads: Vec<(CoordinateNativeAudioSnapshot, SourcePlaybackFacts, [u64; 2])>,
    polls: Vec<ClockPoint>,
    ordinary: usize,
}
struct FutureEffects {
    inner: Effects,
    identity: SourcePlaybackIdentity,
    pending: Option<PendingSourceFrameObservations<Evidence>>,
    next_host: i64,
    trace: Rc<RefCell<Trace>>,
}
impl FutureEffects {
    fn new(inner: Effects, identity: SourcePlaybackIdentity, trace: Rc<RefCell<Trace>>) -> Self {
        Self {
            inner,
            identity,
            pending: None,
            next_host: FIRST_FUTURE,
            trace,
        }
    }
}
impl OutputReplacementBackend<NativeOutputState> for FutureEffects {
    type Presentation = <Effects as OutputReplacementBackend<NativeOutputState>>::Presentation;
    type Output = Output;
    type Request = Request;
    type Error = io::Error;
    fn open(
        &mut self,
        request: Request,
        state: NativeOutputState,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<io::Error, Output, NativeOutputState>> {
        let output = self.inner.open(request, state, epoch)?;
        self.pending = Some(PendingSourceFrameObservations::new(
            epoch,
            output.basis,
            self.identity,
            ClockDomainId(1),
        ));
        self.next_host = FIRST_FUTURE;
        Ok(output)
    }
    fn retire(&mut self, output: &mut Output) -> Result<(), io::Error> {
        if let Some(cache) = &mut self.pending {
            cache.clear();
        }
        self.inner.retire(output)
    }
    fn start(&mut self, output: &mut Output) -> Result<(), io::Error> {
        self.inner.start(output)
    }
    fn epoch(&self, output: &Output) -> u64 {
        self.inner.epoch(output)
    }
    fn basis(&self, output: &Output) -> OutputFrameBasis {
        self.inner.basis(output)
    }
    fn observe(
        &mut self,
        output: &mut Output,
        p: &mut Self::Presentation,
    ) -> Result<(), io::Error> {
        self.inner.observe(output, p)
    }
    fn render_report(&self, output: &Output) -> Result<Option<RenderReport>, io::Error> {
        self.inner.render_report(output)
    }
}
impl OriginalNativeOutputBackend<NativeOutputState> for FutureEffects {
    fn observe_native(
        &mut self,
        output: &mut Output,
    ) -> Result<Option<NativeAudioSnapshot>, io::Error> {
        self.inner.observe_native(output)
    }
}
impl OriginalCoordinateNativeOutputBackend<NativeOutputState> for FutureEffects {
    fn observe_native_coordinate(
        &mut self,
        output: &mut Output,
    ) -> Result<Option<CoordinateNativeAudioSnapshot>, io::Error> {
        self.inner.observe_native_coordinate(output)
    }
}
impl SourcePlaybackNativeOutputBackend<NativeOutputState> for FutureEffects {
    type NativeEvidence = Evidence;
    fn observe_native_playback(
        &mut self,
        output: &mut Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Evidence>>, io::Error> {
        self.trace.borrow_mut().ordinary += 1;
        self.inner.observe_native_playback(output, identity)
    }
    fn observe_native_playback_replacement_at(
        &mut self,
        output: &mut Output,
        identity: SourcePlaybackIdentity,
        now: ClockPoint,
    ) -> Result<Option<NativePlaybackObservation<Evidence>>, io::Error> {
        self.trace.borrow_mut().polls.push(now);
        // Every current read, including errors and full-cache headers, precedes release.
        let Some(observation) = self.inner.observe_native_playback(output, identity)? else {
            return Ok(None);
        };
        let original = observation.coordinate();
        let mut raw = original.snapshot();
        let OriginalNativePresentationEvidence::SuppliedPair(pair) = &mut raw.evidence else {
            panic!("fixture is specifically the CoreAudio SourceFrame/SuppliedPair path")
        };
        pair.target = host(self.next_host);
        self.next_host += CALLBACK_NS;
        let coordinate = CoordinateNativeAudioSnapshot::new(raw, original.coordinate()).unwrap();
        let observation =
            NativePlaybackObservation::new(coordinate, observation.into_source()).unwrap();
        self.trace.borrow_mut().reads.push((
            coordinate,
            observation.source().facts(),
            observation
                .source()
                .native_evidence()
                .unwrap()
                .original_native_words,
        ));
        let pending = self
            .pending
            .as_mut()
            .expect("cold open binds trusted expectations");
        pending.push(observation).map_err(io::Error::other)?;
        pending.take_covered(now).map_err(io::Error::other)
    }
    fn source_playback_at(
        &self,
        output: &Output,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Evidence>>, io::Error> {
        self.inner.source_playback_at(output, query, identity)
    }
}
impl ConditionalSourceRetirementNativeOutputBackend<NativeOutputState> for FutureEffects {
    type Ticket = Ticket;
    fn is_source_native_io_error(&self, error: &io::Error) -> bool {
        self.inner.is_source_native_io_error(error)
    }
    fn created_source_coordinate_basis(
        &self,
        output: &Output,
    ) -> Result<AudioAuthorityCoordinateBasis, io::Error> {
        self.inner.created_source_coordinate_basis(output)
    }
    fn submit_source_conditional_retirement(
        &mut self,
        output: &mut Output,
        request: &Request,
        matrix: Option<&ChannelMatrix>,
    ) -> Result<Option<Ticket>, io::Error> {
        self.inner
            .submit_source_conditional_retirement(output, request, matrix)
    }
    fn poll_source_conditional_retirement(
        &mut self,
        output: &mut Output,
        ticket: &Ticket,
    ) -> Result<SourceRetirementStatus, io::Error> {
        self.inner
            .poll_source_conditional_retirement(output, ticket)
    }
    fn cancel_source_conditional_retirement(
        &mut self,
        output: &mut Output,
        ticket: &Ticket,
    ) -> Result<SourceRetirementStatus, io::Error> {
        self.inner
            .cancel_source_conditional_retirement(output, ticket)
    }
}
type FutureController =
    OutputReplacement<FutureEffects, NativeOutputState, OutputFrameBasis, Ticket>;
fn setup(wait_ns: u64, fault: Fault) -> (Rig, FutureController, Rc<RefCell<Trace>>) {
    let mut rig = Rig::with_frequency(false, None);
    let (effects, output) = rig.take_native_owner();
    let trace = Rc::new(RefCell::new(Trace::default()));
    let mut controller =
        FutureController::new(FutureEffects::new(effects, rig.identity, trace.clone()));
    assert!(controller.attach(output).is_ok());
    begin(&mut rig, &mut controller, wait_ns, fault);
    pending(poll(&rig, &mut controller, now()).unwrap()); // actual retirement/recovery/open/start
    (rig, controller, trace)
}
fn begin(rig: &mut Rig, controller: &mut FutureController, wait_ns: u64, fault: Fault) {
    controller
        .begin_coordinate_source_audio(
            Request::normal(None).with_fault(fault),
            &rig.current,
            &rig.pause,
            None,
            &rig.inputs,
            rig.identity,
            false,
            wait_ns,
            || rig.runtime.hold_audio_pause(),
        )
        .unwrap();
}
fn poll(
    rig: &Rig,
    c: &mut FutureController,
    at: ClockPoint,
) -> Result<CoordinateSourcePoll<Output, Evidence>, ReplacementFailure<io::Error>> {
    c.poll_coordinate_source_audio(at, &rig.current, &rig.pause, &rig.inputs)
}

#[test]
fn coordinate_source_future_controller_releases_original_two_after_host_coverage_despite_continuous_callbacks(
) {
    let (mut rig, mut controller, trace) = setup(100_000_000, Fault::None);
    let before = format!("{:?}", rig.current);
    let generation = rig.practice.generation();
    let pin = rig.pin;
    for _ in 0..8 {
        pending(poll(&rig, &mut controller, now()).unwrap());
        assert_eq!(format!("{:?}", rig.current), before);
        assert_eq!(controller.last_issued_epoch(), OLD_EPOCH + 1);
    }
    pending(poll(&rig, &mut controller, host(FIRST_FUTURE - 1)).unwrap());
    pending(poll(&rig, &mut controller, host(FIRST_FUTURE)).unwrap());
    let ready = match poll(&rig, &mut controller, host(FIRST_FUTURE + CALLBACK_NS)).unwrap() {
        CoordinateSourcePoll::Ready(ready) => ready,
        _ => panic!("the two original covered anchors must qualify without starvation"),
    };
    let trace = trace.borrow();
    assert_eq!(
        trace.ordinary, 0,
        "only replacement reads use the supplied poll clock"
    );
    assert_eq!(ready.snapshots(), [trace.reads[0].0, trace.reads[1].0]);
    assert_eq!(ready.observation().coordinate(), trace.reads[1].0);
    assert_eq!(ready.observation().source().facts(), trace.reads[1].1);
    assert_eq!(
        ready
            .observation()
            .source()
            .native_evidence()
            .unwrap()
            .original_native_words,
        trace.reads[1].2
    );
    assert!(
        ready.output().source_state().mixer().frame_cursor() > trace.reads[1].1.pulled_source_end
    );
    assert_eq!(
        ready
            .output()
            .source_state()
            .mixer()
            .playback_frame_cursor(),
        7
    );
    assert_eq!(ready.practice_mixer_basis(), pin);
    assert_eq!(ready.source_identity(), rig.identity);
    assert_eq!(rig.practice.generation(), generation);
    assert_eq!(format!("{:?}", rig.current), before);
    let (hold, ()) = publish_ready_coordinate_source_audio_output_held(
        ready,
        &mut rig.output,
        GameplayAudioOutputContext {
            control: GameplayPauseControl::solo(&mut rig.runtime),
            presentation: &mut rig.current,
            merger: &rig.inputs,
            pause: &mut rig.pause,
            config: &mut rig.config,
            end: &mut rig.end,
        },
        host(FIRST_FUTURE + CALLBACK_NS),
        |_| Ok(()),
    )
    .unwrap_or_else(|failure| panic!("covered original native epoch: {}", failure.error));
    assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH + 1);
    assert_eq!(rig.pause.phase(), PausePhase::Paused);
    drop(hold);
}

#[test]
fn coordinate_source_future_owner_publishes_once_only_when_both_original_host_targets_are_covered()
{
    let mut rig = Rig::with_frequency(false, None);
    let (effects, output) = rig.take_native_owner();
    let trace = Rc::new(RefCell::new(Trace::default()));
    let mut owner = GameplayOutputOwner::new_with_ticket(
        FutureEffects::new(effects, rig.identity, trace.clone()),
        output,
    );
    assert!(owner.queue(Request::normal(None), 100_000_000).is_ok());
    let initial = format!("{:?}", rig.current);
    let stages = Cell::new(0);
    let identity = rig.identity;
    for at in std::iter::repeat_n(now(), 9).chain([host(FIRST_FUTURE - 1), host(FIRST_FUTURE)]) {
        let outcome = owner
            .publish_paused_coordinate_source_audio(rig.context(), at, identity, false, |_| {
                stages.set(stages.get() + 1);
                Ok(())
            })
            .unwrap();
        assert!(matches!(outcome, CoordinateSourceOwnerPublication::Pending));
        assert_eq!(stages.get(), 0);
        assert_eq!(format!("{:?}", rig.current), initial);
        assert_eq!(owner.last_issued_epoch(), OLD_EPOCH + 1);
    }
    let outcome = owner
        .publish_paused_coordinate_source_audio(
            rig.context(),
            host(FIRST_FUTURE + CALLBACK_NS),
            identity,
            false,
            |output| {
                stages.set(stages.get() + 1);
                Ok(output.native_id)
            },
        )
        .unwrap();
    let CoordinateSourceOwnerPublication::Published(native_id) = outcome else {
        panic!("covered original evidence must publish")
    };
    assert_eq!(owner.current().unwrap().native_id, native_id);
    assert_eq!(stages.get(), 1);
    assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH + 1);
    assert_eq!(trace.borrow().ordinary, 0);
    assert_eq!(
        owner
            .current()
            .unwrap()
            .source_state()
            .mixer()
            .playback_frame_cursor(),
        7
    );
    assert_eq!(rig.current.practice_mixer_basis(), Some(rig.pin));
}

#[test]
fn coordinate_source_future_current_read_errors_remain_visible_when_cache_is_full_or_an_anchor_is_covered(
) {
    for at in [now(), host(FIRST_FUTURE)] {
        let (rig, mut c, _) = setup(100_000_000, Fault::None);
        for _ in 0..2 {
            pending(poll(&rig, &mut c, now()).unwrap());
        }
        c.backend_mut().inner.observation_faults([Fault::Observe]);
        let error = match poll(&rig, &mut c, at) {
            Err(error) => error,
            _ => panic!("cached evidence cannot hide a current native failure"),
        };
        assert!(
            matches!(error.cause, ReplacementCause::Backend { phase: ReplacementPhase::Observe, error }
            if error.to_string() == "original native observation failure")
        );
        assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH);
        assert_eq!(c.state(), ReplacementState::RecoveredMixer);
    }
}

#[test]
fn coordinate_source_future_timeout_and_cancellation_preserve_frozen_owner_history_and_epoch_floor()
{
    for cancel in [false, true] {
        let (mut rig, mut c, _) = setup(100_000_000, Fault::None);
        for _ in 0..3 {
            pending(poll(&rig, &mut c, now()).unwrap());
        }
        if cancel {
            assert_eq!(
                c.cancel_coordinate_source_audio().unwrap(),
                CoordinateSourceCancellation::Retired
            );
        } else {
            assert!(poll(&rig, &mut c, host(now().timestamp.as_nanos() + 100_000_000)).is_err());
        }
        assert_eq!(c.state(), ReplacementState::RecoveredMixer);
        assert_eq!(c.last_issued_epoch(), OLD_EPOCH + 1);
        assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH);
        begin(&mut rig, &mut c, 100_000_000, Fault::None);
        assert_eq!(c.last_issued_epoch(), OLD_EPOCH + 2);
        // A new output starts with an empty cache, even though the old owner pulled ahead.
        pending(poll(&rig, &mut c, now()).unwrap());
        assert_eq!(
            c.backend()
                .trace
                .borrow()
                .reads
                .last()
                .unwrap()
                .0
                .snapshot()
                .epoch,
            OLD_EPOCH + 2
        );
        assert_eq!(
            c.cancel_coordinate_source_audio().unwrap(),
            CoordinateSourceCancellation::Retired
        );
        let state = c.take_recovered_mixer().unwrap();
        assert_eq!(state.mixer().playback_frame_cursor(), 7);
        let history = source_history(&state);
        assert_eq!((history.initial_physical, history.initial_playback), (5, 3));
        assert_eq!(state.mixer().applied_start_frame(), Some(2));
    }
}

#[test]
fn coordinate_source_future_cached_release_still_checks_actual_stale_age() {
    let (rig, mut c, _) = setup(1_000_000_000, Fault::None);
    for _ in 0..3 {
        pending(poll(&rig, &mut c, now()).unwrap());
    }
    let error = match poll(&rig, &mut c, host(FIRST_FUTURE + 101_000_000)) {
        Err(error) => error,
        _ => panic!("covered evidence must still satisfy the original stale-age bound"),
    };
    assert!(matches!(error.cause, ReplacementCause::Policy(message) if message.contains("stale")));
    assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH);
    assert_eq!(c.last_issued_epoch(), OLD_EPOCH + 1);
}

#[test]
fn coordinate_source_future_coverage_never_substitutes_for_frozen_source_and_same_publication_report(
) {
    for fault in [
        Fault::RunningSource,
        Fault::WrongFrozen,
        Fault::MissingRender,
        Fault::ActiveRender,
    ] {
        let (rig, mut c, trace) = setup(100_000_000, fault);
        let before = format!("{:?}", rig.current);
        let mut failure = None;
        for at in [
            now().timestamp.as_nanos(),
            now().timestamp.as_nanos(),
            FIRST_FUTURE,
            FIRST_FUTURE + CALLBACK_NS,
            FIRST_FUTURE + 100_000_000,
        ] {
            match poll(&rig, &mut c, host(at)) {
                Err(error) => {
                    failure = Some(error);
                    break;
                }
                Ok(CoordinateSourcePoll::Pending) => {
                    assert_eq!(format!("{:?}", rig.current), before);
                }
                _ => panic!("future HOST coverage cannot waive frozen Source policy: {fault:?}"),
            }
        }
        let failure = failure
            .unwrap_or_else(|| panic!("invalid frozen packet must fail or time out: {fault:?}"));
        match fault {
            Fault::RunningSource => {
                assert!(matches!(&failure.cause,
                    ReplacementCause::Backend { phase: ReplacementPhase::Observe, error }
                    if error.kind() == io::ErrorKind::InvalidData
                        && error.to_string().contains("QueryOutsideSpan")));
                assert!(
                    trace.borrow().reads.is_empty(),
                    "the malformed running span refuses before entering the future cache"
                );
            }
            Fault::WrongFrozen | Fault::ActiveRender => {
                assert!(matches!(&failure.cause, ReplacementCause::Timing(error)
                    if error.to_string().contains("Source replacement changed frozen rendering")));
                assert!(!trace.borrow().reads.is_empty());
            }
            Fault::MissingRender => {
                assert!(matches!(&failure.cause, ReplacementCause::Policy(message)
                    if message.contains("timed out")));
                assert!(!trace.borrow().reads.is_empty());
            }
            _ => unreachable!(),
        }
        assert_eq!(format!("{:?}", rig.current), before);
        assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH);
        assert_eq!(c.last_issued_epoch(), OLD_EPOCH + 1);
        assert_eq!(c.state(), ReplacementState::RecoveredMixer);
    }
}
