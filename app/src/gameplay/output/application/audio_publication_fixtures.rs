//! Held publication through real Mixers, pause leases and original native records.
use super::*;
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    gameplay::output::{
        application::owner::{fixtures as memory, GameplayOutputOwner},
        ports::OriginalNativeOutputBackend,
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayPauseControl},
    local_input::InputMerger,
    local_runtime::SoloRuntime,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_end::NativeEnd,
    native_gameplay::NativeGameplayConfig,
};
use beatkernel::{
    audio::*,
    input::{ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent},
    runtime::RuntimeProcessingClock,
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, Duration, ExtrapolationPolicy,
    },
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{
    asio::{AsioPresentationObservation, MultimediaHostInterval},
    presentation::validation::{NativePresentationValidator, OriginalNativePresentationEvidence},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const LOGICAL_ORIGIN: i64 = 5_000_000_000;
fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn host(nanos: i64) -> ClockPoint {
    point(1, nanos)
}
fn raw(nanos: i64) -> ClockPoint {
    point(2, nanos)
}
fn logical(nanos: i64) -> ClockPoint {
    point(3, LOGICAL_ORIGIN + nanos)
}
fn pair(nanos: i64) -> ClockPair {
    ClockPair {
        source: raw(nanos),
        target: host(nanos),
    }
}
fn input(nanos: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), host(nanos), sequence),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
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
#[derive(Clone, Copy, Debug)]
enum Mode {
    Normal,
    Asio,
    FutureAsio,
    Quiet,
    Repeated,
    Future,
    WrongSnapshotEpoch,
    WrongSnapshotBasis,
    WrongDomain,
    OpenError,
    WrongOpenEpoch,
    WrongOpenBasis,
    OpenPending,
    OpenUnavailable,
    StartError,
    ObserveError,
    ReportError,
}
struct Request {
    mode: Mode,
    error: Option<memory::Fault>,
}
fn request(mode: Mode) -> Request {
    Request { mode, error: None }
}
#[derive(Default)]
struct Io {
    created: RefCell<Vec<u64>>,
    native_calls: Cell<usize>,
    rendered: RefCell<Vec<RenderReport>>,
    snapshots: RefCell<Vec<NativeAudioSnapshot>>,
}
struct Output {
    inner: memory::Output,
    mode: Mode,
    first: Option<NativeAudioSnapshot>,
    error: RefCell<Option<memory::Fault>>,
    recovery_failures: usize,
}
impl StoppedMixerSource for Output {
    type Error = memory::Fault;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        if self.recovery_failures != 0 {
            self.recovery_failures -= 1;
            return Err(memory::Fault::new(92));
        }
        self.inner.take_stopped_mixer()
    }
}
struct Backend {
    inner: memory::Backend,
    io: Rc<Io>,
}
impl OutputReplacementBackend for Backend {
    type Presentation = beatkernel::time::presentation::PresentationEstimator;
    type Output = Output;
    type Request = Request;
    type Error = memory::Fault;
    fn open(
        &mut self,
        mut request: Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<Self::Error, Output>> {
        self.io.created.borrow_mut().push(epoch);
        if matches!(request.mode, Mode::OpenError) {
            return Err(OutputOpenFailure::recovered(
                request
                    .error
                    .take()
                    .unwrap_or_else(|| memory::Fault::new(30)),
                Some(mixer),
            ));
        }
        if matches!(request.mode, Mode::OpenUnavailable) {
            drop(mixer);
            return Err(OutputOpenFailure::recovered(
                request
                    .error
                    .take()
                    .unwrap_or_else(|| memory::Fault::new(31)),
                None,
            ));
        }
        let mut inner = self
            .inner
            .open(memory::request(epoch, 0), mixer, epoch)
            .unwrap();
        if matches!(request.mode, Mode::WrongOpenEpoch) {
            inner.epoch += 1;
        }
        if matches!(request.mode, Mode::WrongOpenBasis) {
            inner.basis = OutputFrameBasis::new(
                inner.basis.origin(),
                inner.basis.sample_rate(),
                inner.basis.start_physical_frame() + 1,
            )
            .unwrap();
        }
        let output = Output {
            inner,
            mode: request.mode,
            first: None,
            error: RefCell::new(request.error),
            recovery_failures: 0,
        };
        if matches!(request.mode, Mode::OpenPending) {
            return Err(OutputOpenFailure::pending(memory::Fault::new(32), output)
                .with_cleanup_error(memory::Fault::new(43)));
        }
        Ok(output)
    }
    fn retire(&mut self, output: &mut Output) -> Result<(), Self::Error> {
        self.inner.retire(&mut output.inner)
    }
    fn start(&mut self, output: &mut Output) -> Result<(), Self::Error> {
        if matches!(output.mode, Mode::StartError) {
            self.inner.trace.borrow_mut().calls.push("start");
            return Err(output
                .error
                .get_mut()
                .take()
                .unwrap_or_else(|| memory::Fault::new(40)));
        }
        self.inner.start(&mut output.inner)
    }
    fn epoch(&self, output: &Output) -> u64 {
        output.inner.epoch
    }
    fn basis(&self, output: &Output) -> OutputFrameBasis {
        output.inner.basis
    }
    fn observe(&mut self, _: &mut Output, _: &mut Self::Presentation) -> Result<(), Self::Error> {
        panic!("held audio publication must not instantiate or update legacy timing")
    }
    fn render_report(&self, output: &Output) -> Result<Option<RenderReport>, Self::Error> {
        if matches!(output.mode, Mode::ReportError) {
            return Err(output
                .error
                .borrow_mut()
                .take()
                .unwrap_or_else(|| memory::Fault::new(42)));
        }
        self.inner.render_report(&output.inner)
    }
}
impl OriginalNativeOutputBackend for Backend {
    fn observe_native(
        &mut self,
        output: &mut Output,
    ) -> Result<Option<NativeAudioSnapshot>, Self::Error> {
        self.io.native_calls.set(self.io.native_calls.get() + 1);
        if matches!(output.mode, Mode::ObserveError) {
            return Err(output
                .error
                .get_mut()
                .take()
                .unwrap_or_else(|| memory::Fault::new(41)));
        }
        output.inner.render(1);
        let report = output.inner.report.unwrap();
        assert!(report.paused);
        assert_eq!(report.playback_frames, 0);
        self.io.rendered.borrow_mut().push(report);
        if matches!(output.mode, Mode::Quiet) {
            return Ok(None);
        }
        if matches!(output.mode, Mode::Repeated) {
            if let Some(first) = output.first {
                return Ok(Some(first));
            }
        }
        let ns = (report.start_frame + report.frames as u64) as i64 * 1_000_000;
        let basis = output.inner.basis;
        let evidence = if matches!(output.mode, Mode::Asio | Mode::FutureAsio) {
            let start = report.start_frame as i64 * 1_000_000;
            let ahead = if matches!(output.mode, Mode::FutureAsio) {
                20_000_000
            } else {
                0
            };
            OriginalNativePresentationEvidence::Asio {
                observation: asio(report, start + ahead - 10_000, start + ahead + 10_000),
                basis: Some(basis),
            }
        } else {
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: if matches!(output.mode, Mode::WrongDomain) {
                    point(7, ns)
                } else {
                    raw(ns)
                },
                target: host(
                    ns + if matches!(output.mode, Mode::Future) {
                        1_000_000_000
                    } else {
                        0
                    },
                ),
            })
        };
        let snapshot = NativeAudioSnapshot {
            epoch: output.inner.epoch + u64::from(matches!(output.mode, Mode::WrongSnapshotEpoch)),
            basis: if matches!(output.mode, Mode::WrongSnapshotBasis) {
                OutputFrameBasis::new(
                    basis.origin(),
                    basis.sample_rate() * 2,
                    basis.start_physical_frame(),
                )
                .unwrap()
            } else {
                basis
            },
            evidence,
        };
        output.first = Some(snapshot);
        self.io.snapshots.borrow_mut().push(snapshot);
        Ok(Some(snapshot))
    }
}
fn asio(render: RenderReport, before: i64, after: i64) -> AsioPresentationObservation {
    AsioPresentationObservation::from_render(
        render,
        1000,
        MultimediaHostInterval {
            before: host(before),
            after: host(after),
        },
        0,
        0,
        raw(0),
    )
    .unwrap()
}
struct Rig {
    presentation: NativeAudioPresentation,
    pause: NativePause,
    merger: InputMerger,
    runtime: SoloRuntime,
    config: NativeGameplayConfig,
    end: Option<NativeEnd>,
    output: Option<Output>,
    trace: Rc<RefCell<memory::Trace>>,
    io: Rc<Io>,
}
impl Rig {
    fn new() -> Self {
        let (mut output, mut producer, trace) = memory::initial(vec![]);
        output.render(2);
        let mut pause = NativePause::new(raw(0), ClockDomainId(1), 1000).unwrap();
        pause.request(true, pair(2_000_000)).unwrap();
        producer.request_pause(true);
        output.render(1);
        let boundary = pause
            .observe(output.report, pair(3_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(boundary.playback_frame, 2);
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
        for ns in [1_000_000, 3_000_000] {
            presentation
                .admit(NativeAudioSnapshot {
                    epoch: 0,
                    basis: output.basis,
                    evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(ns)),
                })
                .unwrap();
        }
        let mut runtime = SoloRuntime::new(
            ClockDomainId(3),
            ClockDomainId(2),
            Transport::new(logical(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
            memory::bindings(None),
            memory::judge(&memory::source()),
            producer,
            vec![],
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let mut merger =
            InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(u64::MAX)], 8).unwrap();
        merger.admit(input(1_000_000, 1), host(4_000_000)).unwrap();
        presentation
            .authority_mut()
            .record_acquired_prefix(host(4_000_000))
            .unwrap();
        let prepared = presentation
            .authority()
            .prepare_input(host(1_000_000), host(4_000_000))
            .unwrap()
            .unwrap();
        let event = merger.pop_ready(host(4_000_000)).unwrap().unwrap();
        let report = runtime
            .process_input(event, prepared.mapper(), raw(4_000_000))
            .unwrap();
        assert!(report.input.is_some());
        presentation.authority_mut().commit_input(prepared).unwrap();
        let cutoff = presentation
            .authority()
            .prepare_control_cutoff(0, raw(2_000_000), host(2_000_000), host(4_000_000), &merger)
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
            .prepare_held_frontier(host(4_000_000), &merger)
            .unwrap()
            .unwrap();
        presentation
            .authority_mut()
            .commit_held_frontier(held, &mut merger)
            .unwrap();
        let io = Rc::new(Io::default());
        Self {
            presentation,
            pause,
            merger,
            runtime,
            config: memory::config(false),
            end: None,
            output: Some(Output {
                inner: output,
                mode: Mode::Normal,
                first: None,
                error: RefCell::new(None),
                recovery_failures: 0,
            }),
            trace,
            io,
        }
    }
    fn controller(&mut self) -> OutputReplacement<Backend> {
        let mut controller = OutputReplacement::new(Backend {
            inner: memory::Backend {
                trace: self.trace.clone(),
            },
            io: self.io.clone(),
        });
        assert!(controller.attach(self.output.take().unwrap()).is_ok());
        controller
    }
    fn begin(
        &mut self,
        controller: &mut OutputReplacement<Backend>,
        request: Request,
    ) -> Result<(), ReplacementFailure<memory::Fault>> {
        controller.begin_audio(
            request,
            &self.presentation,
            &self.pause,
            &self.merger,
            self.config.song_origin,
            100_000_000,
            || self.runtime.hold_audio_pause(),
        )
    }
    fn context(&mut self) -> GameplayAudioOutputContext<'_> {
        GameplayAudioOutputContext {
            control: GameplayPauseControl::solo(&mut self.runtime),
            presentation: &mut self.presentation,
            merger: &self.merger,
            pause: &mut self.pause,
            config: &mut self.config,
            end: &mut self.end,
        }
    }
    fn ready(&mut self, mode: Mode) -> ReadyAudioOutput<Output> {
        let mut controller = self.controller();
        self.begin(&mut controller, request(mode)).unwrap();
        assert!(controller
            .poll_audio(&self.presentation, &self.merger, host(20_000_000))
            .unwrap()
            .is_none());
        controller
            .poll_audio(&self.presentation, &self.merger, host(21_000_000))
            .unwrap()
            .expect("two original anchors must become ready")
    }
}
fn marks(presentation: &NativeAudioPresentation) -> [Option<ClockPoint>; 5] {
    let authority = presentation.authority();
    [
        authority.acquired_prefix(),
        authority.closed_host_prefix(),
        authority.committed_input_host(),
        authority.committed_operation(),
        authority.committed_presentation(),
    ]
}
fn state(rig: &Rig) -> (String, String, String, String) {
    (
        format!("{:?}", rig.presentation),
        format!("{:?}", rig.pause),
        format!("{:?}", rig.config),
        format!("{:?}", rig.end),
    )
}
fn publish(
    rig: &mut Rig,
    ready: ReadyAudioOutput<Output>,
    output: &mut Option<Output>,
    now: ClockPoint,
) -> Result<(), ReadyAudioPublicationFailure<Output>> {
    publish_ready_audio_output(ready, output, rig.context(), now)
}

#[test]
fn two_original_anchors_publish_current_prefix_and_every_watermark_before_releasing_hold() {
    for mode in [Mode::Normal, Mode::Asio] {
        let mut rig = Rig::new();
        let mut controller = rig.controller();
        let before = state(&rig);
        rig.begin(&mut controller, request(mode)).unwrap();
        rig.runtime.request_audio_pause(false);
        assert!(controller
            .poll_audio(&rig.presentation, &rig.merger, host(20_000_000))
            .unwrap()
            .is_none());
        assert_eq!(state(&rig), before);
        rig.presentation
            .authority_mut()
            .record_acquired_prefix(host(19_000_000))
            .unwrap();
        let retained = marks(&rig.presentation);
        let ready = controller
            .poll_audio(&rig.presentation, &rig.merger, host(21_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(
            marks(&rig.presentation),
            retained,
            "readiness must not commit candidate clocks"
        );
        assert_eq!(ready.basis.start_physical_frame(), 5);
        assert_eq!(ready.pause.epoch(), 1);
        assert_eq!(
            ready
                .output
                .inner
                .mixer
                .as_ref()
                .unwrap()
                .playback_frame_cursor(),
            2
        );
        assert!(rig.io.rendered.borrow().iter().all(|report| report.paused
            && report.playback_start_frame == 2
            && report.playback_frames == 0));
        assert!(matches!(
            rig.runtime.hold_audio_pause(),
            Err(PauseHoldError::AlreadyHeld)
        ));
        let snapshots = ready.snapshots;
        let basis = ready.basis;
        let calls = rig.trace.borrow().calls.clone();
        let mut published = None;
        if let Err(failure) = publish(&mut rig, ready, &mut published, host(21_000_000)) {
            panic!("publication refused: {}", failure.error);
        }
        assert_eq!(marks(&rig.presentation), retained);
        assert_eq!(rig.presentation.authority().epoch().id, 1);
        assert_eq!(rig.presentation.basis(), Some(basis));
        assert_eq!(
            rig.presentation.latest_record().unwrap().evidence(),
            &snapshots[1].evidence
        );
        assert_eq!(rig.pause.epoch(), 1);
        assert_eq!(rig.pause.phase(), PausePhase::Paused);
        assert_eq!(
            rig.config.stream_origin,
            basis.point_at_stream_frame(0).unwrap()
        );
        assert_eq!(rig.config.playback_origin, rig.config.stream_origin);
        assert_eq!(
            rig.trace.borrow().calls,
            calls,
            "publication has no fallible backend effects after timing commit"
        );
        let hold = rig
            .runtime
            .hold_audio_pause()
            .expect("lease is available only after publication returns installed ownership");
        drop(hold);
        let mut output = published.unwrap();
        output.inner.render(1);
        assert_eq!(
            output.inner.mixer.as_ref().unwrap().playback_frame_cursor(),
            2
        );
        output.inner.retired = true;
    }
}

#[test]
fn absent_repeated_or_future_original_evidence_cannot_publish_one_anchor_or_advance_song() {
    for mode in [Mode::Quiet, Mode::Repeated, Mode::Future] {
        let mut rig = Rig::new();
        let mut controller = rig.controller();
        let before = state(&rig);
        rig.begin(&mut controller, request(mode)).unwrap();
        for now in [20_000_000, 21_000_000] {
            assert!(controller
                .poll_audio(&rig.presentation, &rig.merger, host(now))
                .unwrap()
                .is_none());
            assert_eq!(state(&rig), before);
            assert!(matches!(
                rig.runtime.hold_audio_pause(),
                Err(PauseHoldError::AlreadyHeld)
            ));
        }
        assert!(rig
            .io
            .rendered
            .borrow()
            .iter()
            .all(|report| report.paused && report.playback_start_frame == 2));
        assert!(controller
            .poll_audio(&rig.presentation, &rig.merger, host(120_000_000))
            .is_err());
        assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
        assert_eq!(controller.last_issued_epoch(), 1);
        assert_eq!(state(&rig), before);
        assert!(controller.take_recovered_mixer().unwrap().is_paused());
    }
}

#[test]
fn publication_refusal_preserves_ready_owner_pause_lease_and_current_timing_for_retry() {
    let mut rig = Rig::new();
    let ready = rig.ready(Mode::Normal);
    rig.config.sample_rate = 2000;
    let before = state(&rig);
    let calls = rig.trace.borrow().calls.clone();
    let mut output = None;
    let failure = match publish(&mut rig, ready, &mut output, host(21_000_000)) {
        Err(failure) => failure,
        Ok(()) => panic!("configuration mismatch must refuse"),
    };
    assert!(output.is_none());
    assert_eq!(state(&rig), before);
    assert_eq!(rig.trace.borrow().calls, calls);
    assert_eq!(failure.ready.output.inner.epoch, 1);
    assert_eq!(
        failure
            .ready
            .output
            .inner
            .mixer
            .as_ref()
            .unwrap()
            .playback_frame_cursor(),
        2
    );
    assert!(matches!(
        rig.runtime.hold_audio_pause(),
        Err(PauseHoldError::AlreadyHeld)
    ));
    rig.config.sample_rate = 1000;
    if let Err(failure) = publish(&mut rig, failure.ready, &mut output, host(22_000_000)) {
        panic!("valid retry refused: {}", failure.error);
    }
    assert!(output.is_some());
    assert_eq!(rig.presentation.authority().epoch().id, 1);
    drop(rig.runtime.hold_audio_pause().unwrap());
    output.as_mut().unwrap().inner.retired = true;
}

#[test]
fn ready_dto_identity_pending_freshness_and_native_or_authority_token_changes_refuse_atomically() {
    for case in 0..7 {
        let mut rig = Rig::new();
        let mut ready = rig.ready(Mode::Normal);
        let mut now = host(21_000_000);
        match case {
            0 => ready.basis = OutputFrameBasis::new(raw(0), 2000, 5).unwrap(),
            1 => ready.snapshots[1].epoch += 1,
            2 => {
                ready.snapshots[1].evidence =
                    OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                        source: point(7, 17_000_000),
                        target: host(17_000_000),
                    })
            }
            3 => {
                rig.merger
                    .admit(input(5_000_000, 2), host(21_000_000))
                    .unwrap();
            }
            4 => now = host(1_100_000_000),
            5 => {
                rig.presentation
                    .authority_mut()
                    .record_acquired_prefix(host(5_000_000))
                    .unwrap();
            }
            6 => {
                let basis = rig.presentation.basis().unwrap();
                rig.presentation
                    .admit(NativeAudioSnapshot {
                        epoch: 0,
                        basis,
                        evidence: OriginalNativePresentationEvidence::SuppliedPair(pair(4_000_000)),
                    })
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = state(&rig);
        let merger_before = format!("{:?}", rig.merger);
        let mut output = None;
        let failure = match publish(&mut rig, ready, &mut output, now) {
            Err(failure) => failure,
            Ok(()) => panic!("invalid publication case {case} accepted"),
        };
        assert!(output.is_none());
        assert_eq!(state(&rig), before);
        assert_eq!(format!("{:?}", rig.merger), merger_before);
        assert!(matches!(
            rig.runtime.hold_audio_pause(),
            Err(PauseHoldError::AlreadyHeld)
        ));
        assert!(!failure.ready.output.inner.retired);
        drop(failure.ready);
    }
}

#[test]
fn occupied_output_slot_refuses_without_replacing_active_software_owner_or_releasing_ready_hold() {
    let mut rig = Rig::new();
    let ready = rig.ready(Mode::Normal);
    let mut occupied = Rig::new();
    let mut slot = occupied.output.take();
    let active_trace = slot.as_ref().unwrap().inner.trace.clone();
    let frame = slot
        .as_ref()
        .unwrap()
        .inner
        .mixer
        .as_ref()
        .unwrap()
        .frame_cursor();
    let before = state(&rig);
    let failure = match publish(&mut rig, ready, &mut slot, host(21_000_000)) {
        Err(failure) => failure,
        Ok(()) => panic!("publication must not displace an occupied slot"),
    };
    assert_eq!(state(&rig), before);
    assert!(Rc::ptr_eq(
        &slot.as_ref().unwrap().inner.trace,
        &active_trace
    ));
    assert_eq!(
        slot.as_ref()
            .unwrap()
            .inner
            .mixer
            .as_ref()
            .unwrap()
            .frame_cursor(),
        frame
    );
    assert_eq!(slot.as_ref().unwrap().inner.epoch, 0);
    assert_eq!(failure.ready.output.inner.epoch, 1);
    assert!(matches!(
        rig.runtime.hold_audio_pause(),
        Err(PauseHoldError::AlreadyHeld)
    ));
    slot.as_mut().unwrap().inner.retired = true;
    drop(failure.ready);
}

#[test]
fn native_open_start_observation_report_and_snapshot_refusals_preserve_old_application_state() {
    for mode in [
        Mode::OpenError,
        Mode::WrongOpenEpoch,
        Mode::WrongOpenBasis,
        Mode::StartError,
        Mode::ObserveError,
        Mode::ReportError,
        Mode::WrongSnapshotEpoch,
        Mode::WrongSnapshotBasis,
        Mode::WrongDomain,
    ] {
        let mut rig = Rig::new();
        let mut controller = rig.controller();
        let before = state(&rig);
        let begun = rig.begin(&mut controller, request(mode));
        if matches!(
            mode,
            Mode::OpenError | Mode::WrongOpenEpoch | Mode::WrongOpenBasis | Mode::StartError
        ) {
            assert!(begun.is_err());
        } else {
            begun.unwrap();
            assert!(controller
                .poll_audio(&rig.presentation, &rig.merger, host(20_000_000))
                .is_err());
        }
        assert_eq!(state(&rig), before);
        assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
        assert_eq!(controller.last_issued_epoch(), 1);
        let mixer = controller.take_recovered_mixer().unwrap();
        assert!(mixer.is_paused());
        assert_eq!(mixer.playback_frame_cursor(), 2);
        drop(rig.runtime.hold_audio_pause().unwrap());
    }
}

#[test]
fn original_fault_identity_retirement_recovery_cancellation_and_issued_epochs_remain_explicit() {
    let mut retired = Rig::new();
    let mut retirement = retired.controller();
    let fault = memory::Fault::new(51);
    let original_pointer = fault.pointer();
    retired.trace.borrow_mut().retire_failure = Some(fault);
    let before = state(&retired);
    let failure = retired
        .begin(&mut retirement, request(Mode::Normal))
        .unwrap_err();
    assert!(
        matches!(failure.cause, ReplacementCause::Backend { phase: ReplacementPhase::Retire, error } if error.pointer() == original_pointer)
    );
    assert!(failure.recovery.is_some());
    assert_eq!(retirement.state(), ReplacementState::PendingRetirement);
    assert!(retired.io.created.borrow().is_empty());
    assert_eq!(state(&retired), before);
    assert!(retired
        .begin(&mut retirement, request(Mode::Normal))
        .is_err());
    assert!(retired.io.created.borrow().is_empty());
    assert!(retirement.retry_retirement().unwrap());
    assert!(retirement.take_recovered_mixer().unwrap().is_paused());
    let mut rig = Rig::new();
    let mut controller = rig.controller();
    for epoch in [1, 2] {
        let fault = memory::Fault::new(30 + epoch);
        let pointer = fault.pointer();
        let failure = rig
            .begin(
                &mut controller,
                Request {
                    mode: Mode::OpenError,
                    error: Some(fault),
                },
            )
            .unwrap_err();
        assert!(
            matches!(failure.cause, ReplacementCause::Backend { phase: ReplacementPhase::Open, error } if error.pointer() == pointer)
        );
        assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
        assert_eq!(controller.last_issued_epoch(), epoch);
    }
    assert_eq!(*rig.io.created.borrow(), [1, 2]);
    rig.begin(&mut controller, request(Mode::Quiet)).unwrap();
    assert_eq!(controller.last_issued_epoch(), 3);
    assert!(controller
        .poll_audio(&rig.presentation, &rig.merger, host(20_000_000))
        .unwrap()
        .is_none());
    assert!(controller.cancel().unwrap());
    assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
    assert!(controller.take_recovered_mixer().unwrap().is_paused());
    let mut rig = Rig::new();
    rig.output.as_mut().unwrap().recovery_failures = 1;
    let mut controller = rig.controller();
    assert!(rig.begin(&mut controller, request(Mode::Normal)).is_err());
    assert_eq!(controller.state(), ReplacementState::PendingRetirement);
    assert!(rig.io.created.borrow().is_empty());
    assert!(rig.begin(&mut controller, request(Mode::Normal)).is_err());
    assert!(rig.io.created.borrow().is_empty());
    assert!(controller.retry_retirement().unwrap());
    assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
    assert!(controller.take_recovered_mixer().unwrap().is_paused());
    for mode in [Mode::OpenPending, Mode::OpenUnavailable] {
        let mut rig = Rig::new();
        let mut controller = rig.controller();
        let failure = rig.begin(&mut controller, request(mode)).unwrap_err();
        if matches!(mode, Mode::OpenPending) {
            assert_eq!(controller.state(), ReplacementState::PendingRetirement);
            assert!(failure.cleanup.is_some());
            assert!(controller.retry_retirement().unwrap());
            assert!(controller.take_recovered_mixer().unwrap().is_paused());
        } else {
            assert_eq!(controller.state(), ReplacementState::Unavailable);
            assert!(!controller.retry_retirement().unwrap());
        }
    }
}

#[test]
fn actual_output_owner_uses_two_observations_and_keeps_refused_ready_until_explicit_cancel() {
    for refused in [false, true] {
        let mut rig = Rig::new();
        let output = rig.output.take().unwrap();
        let mut owner = GameplayOutputOwner::new(
            Backend {
                inner: memory::Backend {
                    trace: rig.trace.clone(),
                },
                io: rig.io.clone(),
            },
            output,
        );
        owner
            .queue(request(Mode::Normal), 100_000_000)
            .ok()
            .unwrap();
        assert!(!owner
            .publish_paused_audio(rig.context(), host(20_000_000))
            .unwrap());
        assert!(owner.current().is_none());
        assert!(owner.output_clock_suspended());
        let retained = marks(&rig.presentation);
        if refused {
            rig.config.sample_rate = 2000;
        }
        let before = state(&rig);
        let result = owner.publish_paused_audio(rig.context(), host(21_000_000));
        if refused {
            assert!(result.is_err());
            assert!(owner.rejected_audio_ready().is_some());
            assert!(owner.current().is_none());
            assert_eq!(state(&rig), before);
            assert!(matches!(
                rig.runtime.hold_audio_pause(),
                Err(PauseHoldError::AlreadyHeld)
            ));
            assert!(owner.cancel().unwrap());
            assert!(owner.rejected_audio_ready().is_none());
            assert!(owner.take_recovered_mixer().unwrap().is_paused());
        } else {
            assert!(result.unwrap());
            assert_eq!(owner.current().unwrap().inner.epoch, 1);
            assert_eq!(marks(&rig.presentation), retained);
            assert_eq!(rig.pause.epoch(), 1);
            drop(rig.runtime.hold_audio_pause().unwrap());
            owner.stop().unwrap();
        }
    }
}

#[test]
fn original_future_asio_pair_stays_selected_until_its_full_upper_bracket_is_covered() {
    let mut rig = Rig::new();
    let mut controller = rig.controller();
    rig.begin(&mut controller, request(Mode::FutureAsio))
        .unwrap();
    let before = state(&rig);
    assert!(controller
        .poll_audio(&rig.presentation, &rig.merger, host(20_000_000))
        .unwrap()
        .is_none());
    assert!(controller
        .poll_audio(&rig.presentation, &rig.merger, host(21_000_000))
        .unwrap()
        .is_none());
    assert_eq!(state(&rig), before);
    let originals = [rig.io.snapshots.borrow()[0], rig.io.snapshots.borrow()[1]];
    let OriginalNativePresentationEvidence::Asio {
        observation: second,
        ..
    } = originals[1].evidence
    else {
        panic!("ASIO fixture must preserve its original interval")
    };
    assert_eq!(second.host.after, host(36_010_000));
    // A further native read would report another newer, still-future association.
    // Keeping the bounded original pair prevents chasing that future indefinitely.
    let ready = controller
        .poll_audio(&rig.presentation, &rig.merger, host(36_020_000))
        .unwrap()
        .expect("original two-anchor interval is now fully covered");
    assert_eq!(ready.snapshots, originals);
    assert_eq!(state(&rig), before);
    let retained = marks(&rig.presentation);
    let mut output = None;
    if let Err(failure) = publish(&mut rig, ready, &mut output, host(36_020_000)) {
        panic!("covered ASIO publication refused: {}", failure.error);
    }
    assert_eq!(marks(&rig.presentation), retained);
    assert_eq!(
        rig.presentation.latest_record().unwrap().evidence(),
        &originals[1].evidence
    );
    output.as_mut().unwrap().inner.retired = true;
}

#[test]
fn asio_end_rebind_uses_original_upper_chronology_and_preserves_old_end_on_refusal() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(32, 128, 1).unwrap();
    let bank = SampleBank::new(format, limits).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        )
        .with_playback_end_frame(10),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    let old_render = mixer.render(&mut [0.; 1]).unwrap();
    assert!(old_render.paused);
    let mut end = NativeEnd::new(raw(0), ClockDomainId(1), 1000, 10).unwrap();
    let old = asio(old_render, 0, 4_000_000);
    assert!(end.observe_asio(old).unwrap().is_none());
    mixer.render(&mut [0.; 2]).unwrap();
    let basis = mixer.output_frame_basis();
    assert_eq!(basis.start_physical_frame(), 5);
    let candidate_render = mixer.render(&mut [0.; 1]).unwrap();
    let candidate = asio(candidate_render, 2_500_000, 4_500_000);
    let before = format!("{end:?}");
    // The candidate midpoint3.5ms regresses from old upper4ms, while its original
    // upper4.5ms progresses and is the correct end-history admission coordinate.
    let restarted = end.restart_for_output_asio(basis, candidate).unwrap();
    assert_eq!(format!("{end:?}"), before);
    assert!(end
        .restart_for_output(
            basis,
            candidate.render,
            ClockPair {
                source: candidate.output,
                target: host(3_500_000)
            }
        )
        .is_err());
    let mut regressing = candidate;
    regressing.host = MultimediaHostInterval {
        before: host(2_500_000),
        after: host(3_500_000),
    };
    let mut bad_rate = candidate;
    bad_rate.sample_rate = 2000;
    let mut bad_origin = candidate;
    bad_origin.output_origin = raw(1);
    let mut malformed = candidate;
    malformed.host.before = host(5_000_000);
    let mut reached = candidate;
    reached.render.playback_start_frame = 10;
    for rejected in [regressing, bad_rate, bad_origin, malformed, reached] {
        assert!(end.restart_for_output_asio(basis, rejected).is_err());
        assert_eq!(format!("{end:?}"), before);
    }
    assert!(end
        .restart_for_output_asio(OutputFrameBasis::new(raw(0), 2000, 5).unwrap(), candidate)
        .is_err());
    assert_eq!(format!("{end:?}"), before);
    let mut restarted = restarted;
    producer.request_pause(false);
    let crossing = mixer.render(&mut [0.; 8]).unwrap();
    assert_eq!(crossing.playback_end_physical_frame, Some(14));
    assert!(restarted
        .observe_asio(asio(crossing, 6_000_000, 10_000_000))
        .unwrap()
        .is_none());
    let presented = mixer.render(&mut [0.; 1]).unwrap();
    let boundary = restarted
        .observe_asio(asio(presented, 10_000_000, 15_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(boundary.host, host(15_000_000));
    assert_eq!(boundary.output, raw(14_000_000));
    assert_eq!(boundary.physical_frame, 14);
    assert_eq!(boundary.playback_frame, 10);
    assert_eq!(format!("{end:?}"), before);
}
