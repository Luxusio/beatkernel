//! Real controller/Owner/Mixer with supplied original ASIO native effects only.
use super::{source_frame_observations::PendingSourceFrameObservations, test_support::*, *};
use crate::{
    gameplay::output::{
        application::{
            owner::{CoordinateSourceOwnerPublication, GameplayOutputOwner},
            replacement::{ReplacementCause, ReplacementPhase, ReplacementState},
        },
        ports::{
            NativeSourcePlaybackQuery, OriginalCoordinateNativeOutputBackend,
            SourcePlaybackNativeOutputBackend,
        },
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayPauseControl},
};
use beatkernel::{audio::*, time::ClockDomainId};
use beatkernel_platform::audio::{
    asio::{AsioPresentationObservation, MultimediaHostInterval},
    NativeOutputState,
};
use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
};

const FIRST_BEFORE: i64 = HOST_START + 20_000_000;
const WIDTH_NS: i64 = 200_000;
const CALLBACK_NS: i64 = 2_000_000;
const FIRST_AFTER: i64 = FIRST_BEFORE + WIDTH_NS;
const SECOND_AFTER: i64 = FIRST_AFTER + CALLBACK_NS;
#[derive(Default)]
struct Trace {
    reads: Vec<(CoordinateNativeAudioSnapshot, SourcePlaybackFacts, [u64; 2])>,
    ordinary: usize,
}
struct AsioFutureEffects {
    inner: Effects,
    identity: SourcePlaybackIdentity,
    pending: Option<PendingSourceFrameObservations<Evidence>>,
    next_before: i64,
    trace: Rc<RefCell<Trace>>,
    held_native: Option<(CoordinateNativeAudioSnapshot, SourcePlaybackFacts, [u64; 2])>,
    renewed_host: Option<MultimediaHostInterval>,
}
impl AsioFutureEffects {
    fn new(inner: Effects, identity: SourcePlaybackIdentity, trace: Rc<RefCell<Trace>>) -> Self {
        Self {
            inner,
            identity,
            pending: None,
            next_before: FIRST_BEFORE,
            trace,
            held_native: None,
            renewed_host: None,
        }
    }
    fn queue_publication(
        &mut self,
        coordinate: CoordinateNativeAudioSnapshot,
        identity: SourcePlaybackIdentity,
        facts: SourcePlaybackFacts,
        words: [u64; 2],
        now: ClockPoint,
    ) -> Result<Option<NativePlaybackObservation<Evidence>>, io::Error> {
        let snapshot = coordinate.snapshot();
        let source = NativeSourcePlaybackQuery::new(
            snapshot.epoch,
            snapshot.basis,
            identity,
            coordinate.source_query().unwrap(),
            facts,
            Some(Evidence {
                original_native_words: words,
            }),
        )
        .map_err(io::Error::other)?;
        let packet =
            NativePlaybackObservation::new(coordinate, source).map_err(io::Error::other)?;
        self.trace
            .borrow_mut()
            .reads
            .push((coordinate, facts, words));
        let pending = self.pending.as_mut().unwrap();
        pending.push(packet).map_err(io::Error::other)?;
        pending.take_covered(now).map_err(io::Error::other)
    }
}
impl OutputReplacementBackend<NativeOutputState> for AsioFutureEffects {
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
        self.pending = Some(
            PendingSourceFrameObservations::new_asio(
                epoch,
                output.basis,
                self.identity,
                ClockDomainId(1),
            )
            .expect("trusted Source expectations"),
        );
        self.next_before = FIRST_BEFORE;
        Ok(output)
    }
    fn retire(&mut self, output: &mut Output) -> Result<(), io::Error> {
        if let Some(cache) = &mut self.pending {
            cache.clear();
        }
        self.inner.retire(output)
    }
    fn start(&mut self, output: &mut Output) -> Result<(), io::Error> {
        let carried_suffix = output.source_state().pending_frames() != 0;
        // Actual complete ASIO primes B before Start. These fixtures use a
        // two-frame negotiated period and never create presentation for B.
        let state = output.state.as_mut().unwrap();
        let report = match state.pending_report() {
            Some(report) => report,
            None => state.render_pending(2).unwrap(),
        };
        assert_eq!(state.pending_frames(), 2);
        state.admit(2).unwrap();
        output.last = Some(report);
        output.presented += 2;
        self.inner.start(output)?;
        if carried_suffix {
            // A may execute synchronously inside native Start. The old B
            // suffix does not itself prove a newly generated frozen block.
            let state = output.state.as_mut().unwrap();
            output.last = Some(state.render_pending(2).unwrap());
            state.admit(2).unwrap();
            output.presented += 2;
        }
        Ok(())
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
impl OriginalNativeOutputBackend<NativeOutputState> for AsioFutureEffects {
    fn observe_native(
        &mut self,
        output: &mut Output,
    ) -> Result<Option<NativeAudioSnapshot>, io::Error> {
        self.inner.observe_native(output)
    }
}
impl OriginalCoordinateNativeOutputBackend<NativeOutputState> for AsioFutureEffects {
    fn observe_native_coordinate(
        &mut self,
        output: &mut Output,
    ) -> Result<Option<CoordinateNativeAudioSnapshot>, io::Error> {
        self.inner.observe_native_coordinate(output)
    }
}
impl SourcePlaybackNativeOutputBackend<NativeOutputState> for AsioFutureEffects {
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
        // Current native failures must remain visible even if a cached anchor is covered.
        let first = output
            .source_state()
            .output_frame_basis()
            .start_physical_frame();
        let Some(original) = self.inner.observe_native_playback(output, identity)? else {
            return Ok(None);
        };
        if let Some((coordinate, facts, words)) = self.held_native {
            // Script the same coherent native callback being read again. The
            // real Mixer still runs, and a current native failure above wins.
            // Only the read's HOST bracket is renewed; source revision/facts
            // and callback-delivered geometry remain the original values.
            let mut snapshot = coordinate.snapshot();
            let OriginalNativePresentationEvidence::Asio { observation, .. } =
                &mut snapshot.evidence
            else {
                unreachable!("only ASIO publications are held");
            };
            observation.host = self
                .renewed_host
                .expect("explicit supplied renewed bracket");
            let renewed =
                CoordinateNativeAudioSnapshot::new(snapshot, coordinate.coordinate()).unwrap();
            self.next_before += CALLBACK_NS;
            return self.queue_publication(renewed, self.identity, facts, words, now);
        }
        let facts = original.source().facts();
        let words = original
            .source()
            .native_evidence()
            .unwrap()
            .original_native_words;
        let last = output
            .source_state()
            .output_frame_basis()
            .start_physical_frame();
        let report = facts.render.unwrap_or_else(|| output.last.unwrap());
        let observation = AsioPresentationObservation::from_delivery(
            report,
            first,
            usize::try_from(last - first).unwrap(),
            SOURCE_RATE,
            MultimediaHostInterval {
                before: host(self.next_before),
                after: host(self.next_before + WIDTH_NS),
            },
            0,
            0,
            output.basis.origin(),
        )
        .unwrap();
        self.next_before += CALLBACK_NS;
        let coordinate = CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: original.coordinate().snapshot().epoch,
                basis: original.coordinate().snapshot().basis,
                evidence: OriginalNativePresentationEvidence::Asio {
                    observation,
                    basis: Some(output.basis),
                },
            },
            NativeOutputCoordinate::StreamFrame {
                frame: first - output.basis.start_physical_frame(),
            },
        )
        .unwrap();
        self.queue_publication(coordinate, original.source().identity(), facts, words, now)
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
impl ConditionalSourceRetirementNativeOutputBackend<NativeOutputState> for AsioFutureEffects {
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
type Controller = OutputReplacement<AsioFutureEffects, NativeOutputState, OutputFrameBasis, Ticket>;
fn setup(wait_ns: u64, partial: bool) -> (Rig, Controller, Rc<RefCell<Trace>>) {
    let mut rig = Rig::with_frequency(partial, None);
    if partial {
        // Leave an exact two-frame suffix of the real paused eight-frame
        // report so the new fixed native period can consume it unchanged.
        let output = rig.output.as_mut().unwrap();
        output.state.as_mut().unwrap().admit(3).unwrap();
        output.presented += 3;
        assert_eq!(output.source_state().pending_frames(), 2);
    }
    let (effects, output) = rig.take_native_owner();
    let trace = Rc::new(RefCell::new(Trace::default()));
    let mut controller =
        Controller::new(AsioFutureEffects::new(effects, rig.identity, trace.clone()));
    assert!(controller.attach(output).is_ok());
    begin(&mut rig, &mut controller, wait_ns);
    pending(poll(&rig, &mut controller, now()).unwrap());
    (rig, controller, trace)
}
fn begin(rig: &mut Rig, controller: &mut Controller, wait_ns: u64) {
    controller
        .begin_coordinate_source_audio(
            request(),
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
fn request() -> Request {
    let mut request = Request::normal(None);
    request.period = 2;
    request
}
fn poll(
    rig: &Rig,
    c: &mut Controller,
    at: ClockPoint,
) -> Result<CoordinateSourcePoll<Output, Evidence>, ReplacementFailure<io::Error>> {
    c.poll_coordinate_source_audio(at, &rig.current, &rig.pause, &rig.inputs)
}

#[test]
fn asio_future_controller_keeps_original_intervals_until_after_coverage_and_commits_same_source_history(
) {
    for partial in [false, true] {
        let (mut rig, mut c, trace) = setup(100_000_000, partial);
        let before = format!("{:?}", rig.current);
        let generation = rig.practice.generation();
        for _ in 0..8 {
            pending(poll(&rig, &mut c, now()).unwrap());
        }
        for at in [
            FIRST_BEFORE,
            FIRST_BEFORE + WIDTH_NS / 2,
            FIRST_AFTER - 1,
            FIRST_AFTER,
            SECOND_AFTER - 1,
        ] {
            pending(poll(&rig, &mut c, host(at)).unwrap());
            assert_eq!(format!("{:?}", rig.current), before);
        }
        let ready = match poll(&rig, &mut c, host(SECOND_AFTER)).unwrap() {
            CoordinateSourcePoll::Ready(ready) => ready,
            _ => panic!("original covered ASIO anchors must qualify despite subsequent callbacks"),
        };
        {
            let trace = trace.borrow();
            assert_eq!(trace.ordinary, 0);
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
                ready.output().source_state().mixer().frame_cursor()
                    > trace.reads[1].1.pulled_source_end
            );
        }
        assert_eq!(ready.practice_mixer_basis(), rig.pin);
        assert_eq!(ready.source_identity(), rig.identity);
        assert_eq!(rig.practice.generation(), generation);
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
            host(SECOND_AFTER),
            |_| Ok(()),
        )
        .unwrap_or_else(|f| panic!("original ASIO epoch: {}", f.error));
        assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH + 1);
        assert_eq!(rig.pause.phase(), PausePhase::Paused);
        assert_eq!(rig.current.practice_mixer_basis(), Some(rig.pin));
        assert_eq!(
            rig.output
                .as_ref()
                .unwrap()
                .source_state()
                .mixer()
                .playback_frame_cursor(),
            7
        );
        drop(hold);
    }
}

#[test]
fn asio_future_owner_publishes_once_after_two_original_upper_bounds() {
    let mut rig = Rig::with_frequency(false, None);
    let (effects, output) = rig.take_native_owner();
    let trace = Rc::new(RefCell::new(Trace::default()));
    let mut owner = GameplayOutputOwner::new_with_ticket(
        AsioFutureEffects::new(effects, rig.identity, trace.clone()),
        output,
    );
    assert!(owner.queue(request(), 100_000_000).is_ok());
    let initial = format!("{:?}", rig.current);
    let stages = Cell::new(0);
    let identity = rig.identity;
    for at in std::iter::repeat_n(now(), 9).chain([
        host(FIRST_BEFORE + WIDTH_NS / 2),
        host(FIRST_AFTER),
        host(SECOND_AFTER - 1),
    ]) {
        let outcome = owner
            .publish_paused_coordinate_source_audio(rig.context(), at, identity, false, |_| {
                stages.set(stages.get() + 1);
                Ok(())
            })
            .unwrap();
        assert!(matches!(outcome, CoordinateSourceOwnerPublication::Pending));
        assert_eq!(stages.get(), 0);
        assert_eq!(format!("{:?}", rig.current), initial);
    }
    let outcome = owner
        .publish_paused_coordinate_source_audio(
            rig.context(),
            host(SECOND_AFTER),
            identity,
            false,
            |output| {
                stages.set(stages.get() + 1);
                Ok(output.native_id)
            },
        )
        .unwrap();
    let CoordinateSourceOwnerPublication::Published(native_id) = outcome else {
        panic!("covered native evidence must publish")
    };
    assert_eq!(owner.current().unwrap().native_id, native_id);
    assert_eq!(stages.get(), 1);
    assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH + 1);
    assert_eq!(trace.borrow().ordinary, 0);
}

#[test]
fn asio_future_current_native_error_is_visible_before_full_or_covered_cache_release() {
    for at in [now(), host(FIRST_AFTER)] {
        let (rig, mut c, _) = setup(100_000_000, false);
        for _ in 0..2 {
            pending(poll(&rig, &mut c, now()).unwrap());
        }
        c.backend_mut().inner.observation_faults([Fault::Observe]);
        let error = match poll(&rig, &mut c, at) {
            Err(error) => error,
            _ => panic!("cache cannot hide current failure"),
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
fn asio_future_timeout_and_cancel_retain_frozen_mixer_history_and_new_epoch_floor() {
    for cancel in [false, true] {
        let (mut rig, mut c, _) = setup(100_000_000, true);
        let generation = rig.practice.generation();
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
        assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH);
        assert_eq!(rig.practice.generation(), generation);
        begin(&mut rig, &mut c, 100_000_000);
        assert_eq!(c.last_issued_epoch(), OLD_EPOCH + 2);
        pending(poll(&rig, &mut c, now()).unwrap());
        assert_eq!(
            c.cancel_coordinate_source_audio().unwrap(),
            CoordinateSourceCancellation::Retired
        );
        let state = c.take_recovered_mixer().unwrap();
        assert_eq!(state.mixer().playback_frame_cursor(), 7);
        assert_eq!(state.mixer().applied_start_frame(), Some(2));
        let history = source_history(&state);
        assert_eq!((history.initial_physical, history.initial_playback), (5, 3));
    }
}

#[test]
fn asio_future_original_age_still_refuses_stale_covered_anchors() {
    let (rig, mut c, _) = setup(1_000_000_000, false);
    for _ in 0..3 {
        pending(poll(&rig, &mut c, now()).unwrap());
    }
    let error = match poll(&rig, &mut c, host(FIRST_BEFORE + 101_000_000)) {
        Err(error) => error,
        _ => panic!("upper coverage cannot refresh original midpoint age"),
    };
    assert!(matches!(error.cause, ReplacementCause::Policy(message) if message.contains("stale")));
    assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH);
}

#[test]
fn asio_future_controller_same_revision_renewal_preserves_original_anchor_until_real_next_callback()
{
    let (mut rig, mut c, trace) = setup(100_000_000, false);
    pending(poll(&rig, &mut c, now()).unwrap());
    let first = trace.borrow().reads[0];
    assert_eq!(first.1.revision, 2);
    c.backend_mut().held_native = Some(first);
    for offset in [1_000_000, 2_000_000, 3_000_000] {
        c.backend_mut().renewed_host = Some(MultimediaHostInterval {
            before: host(FIRST_BEFORE + offset),
            after: host(FIRST_AFTER + offset),
        });
        pending(poll(&rig, &mut c, now()).unwrap());
    }
    // The original interval is covered even though every renewed upper bound
    // is still future. This supplies one original anchor, never two duplicates.
    pending(poll(&rig, &mut c, host(FIRST_AFTER)).unwrap());
    assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH);
    c.backend_mut().held_native = None;
    pending(poll(&rig, &mut c, host(FIRST_AFTER)).unwrap());
    let next = *trace.borrow().reads.last().unwrap();
    assert!(next.1.revision > first.1.revision);
    let OriginalNativePresentationEvidence::Asio { observation, .. } = next.0.snapshot().evidence
    else {
        unreachable!()
    };
    c.backend_mut().held_native = Some(next);
    c.backend_mut().renewed_host = Some(MultimediaHostInterval {
        before: host(observation.host.before.timestamp.as_nanos() + 50_000_000),
        after: host(observation.host.after.timestamp.as_nanos() + 50_000_000),
    });
    let ready = match poll(&rig, &mut c, observation.host.after).unwrap() {
        CoordinateSourcePoll::Ready(ready) => ready,
        _ => panic!("two real original callbacks must qualify without HOST renewal"),
    };
    assert_eq!(ready.snapshots(), [first.0, next.0]);
    assert_eq!(ready.observation().source().facts(), next.1);
    assert_eq!(
        ready
            .observation()
            .source()
            .native_evidence()
            .unwrap()
            .original_native_words,
        next.2
    );
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
        observation.host.after,
        |_| Ok(()),
    )
    .unwrap_or_else(|f| panic!("original renewed ASIO epoch: {}", f.error));
    assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH + 1);
    assert_eq!(rig.pause.phase(), PausePhase::Paused);
    drop(hold);
}

#[test]
fn asio_future_controller_same_revision_renewal_never_refreshes_original_stale_age() {
    let (rig, mut c, trace) = setup(1_000_000_000, false);
    pending(poll(&rig, &mut c, now()).unwrap());
    let first = trace.borrow().reads[0];
    assert_eq!(first.1.revision, 2);
    c.backend_mut().held_native = Some(first);
    let at = FIRST_BEFORE + 101_000_000;
    c.backend_mut().renewed_host = Some(MultimediaHostInterval {
        before: host(at - WIDTH_NS),
        after: host(at),
    });
    let error = match poll(&rig, &mut c, host(at)) {
        Err(error) => error,
        _ => panic!("the original callback remains stale despite a freshly supplied HOST bracket"),
    };
    assert!(matches!(error.cause, ReplacementCause::Policy(message) if message.contains("stale")));
    assert_eq!(rig.current.authority().epoch().id, OLD_EPOCH);
    assert_eq!(c.state(), ReplacementState::RecoveredMixer);
}

#[test]
fn asio_future_covered_intervals_do_not_waive_frozen_source_or_original_report_policy() {
    for fault in [
        Fault::RunningSource,
        Fault::WrongFrozen,
        Fault::ActiveRender,
    ] {
        let (rig, mut c, _) = setup(100_000_000, false);
        c.backend_mut()
            .inner
            .observation_faults(std::iter::repeat_n(fault, 8));
        let before = format!("{:?}", rig.current);
        let mut failure = None;
        for at in [now(), now(), host(FIRST_AFTER), host(SECOND_AFTER)] {
            match poll(&rig, &mut c, at) {
                Err(error) => {
                    failure = Some(error);
                    break;
                }
                Ok(CoordinateSourcePoll::Pending) => {}
                _ => panic!("original HOST coverage cannot waive frozen Source policy: {fault:?}"),
            }
        }
        let failure =
            failure.unwrap_or_else(|| panic!("invalid frozen packet must refuse: {fault:?}"));
        match fault {
            Fault::RunningSource => assert!(matches!(failure.cause, ReplacementCause::Backend {
                phase: ReplacementPhase::Observe, error } if error.kind() == io::ErrorKind::InvalidData
                && error.to_string().contains("QueryOutsideSpan"))),
            _ => assert!(matches!(failure.cause, ReplacementCause::Timing(error)
                if error.to_string().contains("Source replacement changed frozen rendering"))),
        }
        assert_eq!(format!("{:?}", rig.current), before);
        assert_eq!(c.state(), ReplacementState::RecoveredMixer);
    }
}
