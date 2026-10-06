//! Deferred actual software-model controller traces; memory retirement is no native fence.
use super::*;
use beatkernel::{
    audio::*,
    time::{
        ClockDomainId, ClockPair, ClockPoint, Timestamp,
        presentation::{DisciplineConfig, PresentationEstimator},
    },
};
use crate::playback_pause::{NativePause, PausePhase};
use std::{cell::RefCell, rc::Rc, collections::VecDeque};
struct Fault(Box<u64>); // Deliberately no Clone/Debug/Display/Error implementation.
impl Fault {
    fn new(id: u64) -> Self {
        Self(Box::new(id))
    }
    fn pointer(&self) -> *const u64 {
        self.0.as_ref()
    }
}
#[derive(Clone, Copy)]
enum Mode {
    Normal,
    Interval,
    OpenError,
    OpenPending,
    OpenUnavailable,
    StartError,
    ObserveError,
    NoPair,
    NoReport,
    WrongEpoch,
    WrongBasis,
}
#[derive(Default)]
struct Trace {
    calls: Vec<&'static str>,
    epochs: Vec<u64>,
    drops: Vec<bool>,
    retire_refusals: VecDeque<Fault>,
}
struct MemoryOutput {
    mixer: Option<Mixer>,
    epoch: u64,
    basis: OutputFrameBasis,
    report: Option<RenderReport>,
    retired: bool,
    mode: Mode,
    fault: Option<Fault>,
    trace: Rc<RefCell<Trace>>,
}
impl Drop for MemoryOutput {
    fn drop(&mut self) {
        self.trace.borrow_mut().drops.push(self.retired);
    }
}
impl StoppedMixerSource for MemoryOutput {
    type Error = Fault;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Fault> {
        self.trace.borrow_mut().calls.push("take");
        if !self.retired {
            return Err(Fault::new(90));
        }
        Ok(self.mixer.take())
    }
}
struct Request {
    mode: Mode,
    fault: Option<Fault>,
}
fn normal(mode: Mode) -> Request {
    Request { mode, fault: None }
}
struct MemoryBackend {
    trace: Rc<RefCell<Trace>>,
}
impl OutputReplacementBackend for MemoryBackend {
    type Presentation = PresentationEstimator;
    type Output = MemoryOutput;
    type Request = Request;
    type Error = Fault;
    fn open(
        &mut self,
        request: Request,
        mut mixer: Mixer,
        epoch: u64,
    ) -> Result<MemoryOutput, OutputOpenFailure<Fault, MemoryOutput>> {
        self.trace.borrow_mut().calls.push("open");
        self.trace.borrow_mut().epochs.push(epoch);
        if matches!(request.mode, Mode::OpenError) {
            return Err(OutputOpenFailure::recovered(
                request.fault.unwrap(),
                Some(mixer),
            ));
        }
        if matches!(request.mode, Mode::OpenUnavailable) {
            drop(mixer);
            return Err(OutputOpenFailure::recovered(request.fault.unwrap(), None));
        }
        if matches!(request.mode, Mode::OpenPending) {
            let output = MemoryOutput {
                basis: mixer.output_frame_basis(),
                mixer: Some(mixer),
                epoch,
                report: None,
                retired: false,
                mode: Mode::Normal,
                fault: None,
                trace: self.trace.clone(),
            };
            return Err(OutputOpenFailure::pending(request.fault.unwrap(), output)
                .with_cleanup_error(Fault::new(43)));
        }
        let mut basis = mixer.output_frame_basis();
        let report = mixer.render(&mut [0.; 1]).unwrap();
        assert!(report.paused);
        assert!(mixer.pause_requested());
        if matches!(request.mode, Mode::WrongBasis) {
            basis = OutputFrameBasis::new(
                basis.origin(),
                basis.sample_rate(),
                basis.start_physical_frame() + 1,
            )
            .unwrap();
        }
        if matches!(request.mode, Mode::StartError) {
            self.trace
                .borrow_mut()
                .retire_refusals
                .push_back(Fault::new(42));
        }
        Ok(MemoryOutput {
            mixer: Some(mixer),
            epoch: if matches!(request.mode, Mode::WrongEpoch) {
                epoch + 1
            } else {
                epoch
            },
            basis,
            report: Some(report),
            retired: false,
            mode: request.mode,
            fault: request.fault,
            trace: self.trace.clone(),
        })
    }
    fn retire(&mut self, output: &mut MemoryOutput) -> Result<(), Fault> {
        self.trace.borrow_mut().calls.push("retire");
        if let Some(error) = self.trace.borrow_mut().retire_refusals.pop_front() {
            return Err(error);
        }
        output.retired = true;
        Ok(())
    }
    fn start(&mut self, output: &mut MemoryOutput) -> Result<(), Fault> {
        self.trace.borrow_mut().calls.push("start");
        if matches!(output.mode, Mode::StartError) {
            return Err(output.fault.take().unwrap());
        }
        Ok(())
    }
    fn epoch(&self, output: &MemoryOutput) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &MemoryOutput) -> OutputFrameBasis {
        output.basis
    }
    fn observe(
        &mut self,
        output: &mut MemoryOutput,
        presentation: &mut PresentationEstimator,
    ) -> Result<(), Fault> {
        self.trace.borrow_mut().calls.push("observe");
        if matches!(output.mode, Mode::ObserveError) {
            return Err(output.fault.take().unwrap());
        }
        let mixer = output.mixer.as_mut().unwrap();
        let report = mixer.render(&mut [0.; 1]).unwrap();
        assert!(report.paused);
        output.report = Some(report);
        if !matches!(output.mode, Mode::NoPair) {
            presentation
                .observe_clock_pair_in_epoch(output.epoch, pair(mixer.frame_cursor()))
                .map_err(|_| Fault::new(91))?;
        }
        Ok(())
    }
    fn render_report(&self, output: &MemoryOutput) -> Result<Option<RenderReport>, Fault> {
        self.trace.borrow_mut().calls.push("report");
        Ok(if matches!(output.mode, Mode::NoReport) {
            None
        } else {
            output.report
        })
    }
    fn pause_observation(
        &self,
        output: &MemoryOutput,
        reference: ClockPair,
        now: ClockPoint,
    ) -> Result<crate::live_pause::LivePauseObservation, Fault> {
        if matches!(output.mode, Mode::Interval) {
            let report = output.report.unwrap();
            let start = pair(report.start_frame);
            let observation = crate::playback_pause::PauseIntervalObservation {
                output_origin: output.basis.origin(),
                sample_rate: output.basis.sample_rate(),
                render: report,
                clock: crate::native_start::StartInterval::new(
                    start.source,
                    start.target,
                    point(start.target.timestamp.as_nanos() + 100),
                )
                .unwrap(),
            };
            Ok(crate::live_pause::LivePauseObservation::Interval {
                observation: Some(observation),
                now,
            })
        } else {
            Ok(crate::live_pause::LivePauseObservation::Point(reference))
        }
    }
}
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(2),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn pair(frame: u64) -> ClockPair {
    let ns = (frame * 1_000_000_000 / 3) as i64;
    ClockPair {
        source: ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(ns),
        },
        target: point(ns + 100),
    }
}
struct Rig {
    producer: CommandProducer,
    current: PresentationEstimator,
    pause: NativePause,
    output: MemoryOutput,
    trace: Rc<RefCell<Trace>>,
}
fn rig() -> Rig {
    let format = AudioFormat::new(3, 1).unwrap();
    let limits = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(1),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut pause = NativePause::new(pair(0).source, ClockDomainId(2), 3).unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    pause.request(true, pair(2)).unwrap();
    producer.request_pause(true);
    let report = mixer.render(&mut [0.; 1]).unwrap();
    pause.observe(Some(report), pair(3)).unwrap().unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(7),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_000_000_000),
            gain: 0.5,
        })
        .unwrap();
    let settings = DisciplineConfig {
        capacity: 8,
        min_span: beatkernel::time::Duration::from_nanos(500_000_000),
        ..Default::default()
    };
    let mut current =
        PresentationEstimator::new(settings, pair(0).source, ClockDomainId(2), Timestamp::ZERO)
            .unwrap();
    current.observe_clock_pair(pair(3)).unwrap();
    let trace = Rc::new(RefCell::new(Trace::default()));
    let output = MemoryOutput {
        mixer: Some(mixer),
        epoch: 0,
        basis: OutputFrameBasis::new(pair(0).source, 3, 0).unwrap(),
        report: Some(report),
        retired: false,
        mode: Mode::Normal,
        fault: None,
        trace: trace.clone(),
    };
    Rig {
        producer,
        current,
        pause,
        output,
        trace,
    }
}
fn controller(
    output: MemoryOutput,
    trace: &Rc<RefCell<Trace>>,
) -> OutputReplacement<MemoryBackend> {
    let mut owner = OutputReplacement::new(MemoryBackend {
        trace: trace.clone(),
    });
    match owner.attach(output) {
        Ok(()) => {}
        Err(_) => panic!("first attachment required"),
    }
    owner
}
fn begin(
    owner: &mut OutputReplacement<MemoryBackend>,
    producer: &mut CommandProducer,
    current: &PresentationEstimator,
    pause: &NativePause,
    request: Request,
) {
    match owner.begin(request, current, pause, Timestamp::ZERO, 10_000_000, || {
        producer.hold_pause()
    }) {
        Ok(()) => {}
        Err(_) => panic!("memory replacement must begin"),
    }
}
fn failure<T>(result: Result<T, ReplacementFailure<Fault>>) -> ReplacementFailure<Fault> {
    match result {
        Err(failure) => failure,
        Ok(_) => panic!("injected refusal required"),
    }
}
fn original(error: &ReplacementFailure<Fault>, pointer: *const u64) {
    match &error.cause {
        ReplacementCause::Backend { error, .. } => assert_eq!(error.pointer(), pointer),
        _ => panic!("original backend error required"),
    }
}
#[test]
fn actual_ready_requires_accepted_observation_and_paused_report_then_returns_hold_without_committing_old_clocks()
 {
    for mode in [Mode::Normal, Mode::Interval] {
        let Rig {
            mut producer,
            mut current,
            mut pause,
            output,
            trace,
        } = rig();
        let old_pause = format!("{pause:?}");
        let old_pair = current.latest_pair();
        let settings = current.config();
        let mut owner = controller(output, &trace);
        begin(&mut owner, &mut producer, &current, &pause, normal(mode));
        assert_eq!(owner.state(), ReplacementState::Waiting);
        assert_eq!(owner.last_issued_epoch(), 1);
        producer.request_pause(false);
        let mut ready = match owner.poll(point(3_000_000_000)) {
            Ok(Some(ready)) => ready,
            _ => panic!("real memory pair/report must become ready"),
        };
        assert_eq!(ready.timing.presentation.epoch(), 1);
        assert_eq!(ready.timing.presentation.config(), settings);
        assert!(ready.timing.presentation.latest_pair().is_some());
        assert_eq!(ready.timing.pause.phase(), PausePhase::Paused);
        assert_eq!(ready.timing.basis.start_physical_frame(), 5);
        assert_eq!(format!("{pause:?}"), old_pause);
        assert_eq!(current.latest_pair(), old_pair);
        let calls = trace.borrow().calls.clone();
        assert_eq!(&calls[..4], &["retire", "take", "open", "start"]);
        assert!(calls.contains(&"observe"));
        assert!(calls.contains(&"report"));
        assert_eq!(
            ready.output.mixer.as_ref().unwrap().playback_frame_cursor(),
            2
        );
        assert!(ready.output.mixer.as_ref().unwrap().pause_requested());
        pause = ready.timing.pause;
        current = ready.timing.presentation;
        assert_eq!(pause.epoch(), current.epoch());
        drop(ready.hold);
        let mut pcm = [0.; 2];
        ready
            .output
            .mixer
            .as_mut()
            .unwrap()
            .render(&mut pcm)
            .unwrap();
        assert_eq!(pcm, [0.; 2]);
        assert_eq!(
            ready.output.mixer.as_ref().unwrap().playback_frame_cursor(),
            2
        );
        producer.request_pause(false);
        ready
            .output
            .mixer
            .as_mut()
            .unwrap()
            .render(&mut pcm)
            .unwrap();
        assert_eq!(pcm, [0.75, 1.125]);
        ready.output.retired = true; // Only memory ownership is retired; no native claim.
    }
}
#[test]
fn explicit_failed_open_attempts_preserve_original_error_model_and_never_reuse_issued_epochs() {
    let Rig {
        mut producer,
        current,
        pause,
        output,
        trace,
    } = rig();
    let before = format!("{pause:?}");
    let mut owner = controller(output, &trace);
    for expected_epoch in [1, 2] {
        let error = Fault::new(10 + expected_epoch);
        let pointer = error.pointer();
        let result = owner.begin(
            Request {
                mode: Mode::OpenError,
                fault: Some(error),
            },
            &current,
            &pause,
            Timestamp::ZERO,
            10_000_000,
            || producer.hold_pause(),
        );
        let error = failure(result);
        original(&error, pointer);
        assert_eq!(owner.last_issued_epoch(), expected_epoch);
        assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
        assert_eq!(format!("{pause:?}"), before);
        assert_eq!(current.epoch(), 0);
    }
    assert_eq!(trace.borrow().epochs, [1, 2]);
    let mut mixer = owner.take_recovered_mixer().unwrap();
    producer.request_pause(false);
    let mut pcm = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.75, 1.125]);
}
#[test]
fn failed_open_pending_owner_or_unavailable_model_is_retained_exactly_without_implicit_retry_or_empty_replacement()
 {
    for mode in [Mode::OpenPending, Mode::OpenUnavailable] {
        let Rig {
            mut producer,
            current,
            pause,
            output,
            trace,
        } = rig();
        let mut owner = controller(output, &trace);
        let original_error = Fault::new(51);
        let pointer = original_error.pointer();
        let error = failure(owner.begin(
            Request {
                mode,
                fault: Some(original_error),
            },
            &current,
            &pause,
            Timestamp::ZERO,
            10_000_000,
            || producer.hold_pause(),
        ));
        original(&error, pointer);
        assert_eq!(owner.last_issued_epoch(), 1);
        assert_eq!(trace.borrow().epochs, [1]);
        let calls = trace.borrow().calls.clone();
        assert!(owner.take_recovered_mixer().is_none());
        assert_eq!(trace.borrow().calls, calls);
        if matches!(mode, Mode::OpenPending) {
            assert_eq!(owner.state(), ReplacementState::PendingRetirement);
            assert_eq!(*error.cleanup.unwrap().0, 43);
            assert_eq!(trace.borrow().drops, [true]);
            let _ = failure(owner.begin(
                normal(Mode::Normal),
                &current,
                &pause,
                Timestamp::ZERO,
                10_000_000,
                || panic!("pending owner must not implicitly retry"),
            ));
            assert_eq!(trace.borrow().calls, calls);
            match owner.retry_retirement() {
                Ok(true) => {}
                _ => panic!("explicit memory retirement required"),
            }
            assert_eq!(trace.borrow().drops, [true, true]);
            assert!(owner.take_recovered_mixer().is_some());
        } else {
            assert_eq!(owner.state(), ReplacementState::Unavailable);
            assert!(error.cleanup.is_none());
            assert!(matches!(owner.retry_retirement(), Ok(false)));
            assert_eq!(trace.borrow().calls, calls);
        }
    }
}
#[test]
fn rejected_second_attach_and_retirement_refusals_retain_owner_without_open_until_explicit_retry_succeeds()
 {
    let Rig {
        mut producer,
        current,
        pause,
        output,
        trace,
    } = rig();
    let mut owner = controller(output, &trace);
    let mut second = rig().output;
    let second_identity = second.trace.clone();
    second = match owner.attach(second) {
        Err(output) => output,
        Ok(()) => panic!("second attach must return original owner"),
    };
    assert!(!second.retired);
    assert!(Rc::ptr_eq(&second.trace, &second_identity));
    assert!(second_identity.borrow().drops.is_empty());
    second.retired = true;
    drop(second);
    let error = Fault::new(31);
    let pointer = error.pointer();
    trace.borrow_mut().retire_refusals.push_back(error);
    let error = failure(owner.begin(
        normal(Mode::Normal),
        &current,
        &pause,
        Timestamp::ZERO,
        10_000_000,
        || producer.hold_pause(),
    ));
    original(&error, pointer);
    assert_eq!(owner.state(), ReplacementState::PendingRetirement);
    assert_eq!(owner.last_issued_epoch(), 0);
    assert!(trace.borrow().epochs.is_empty());
    assert!(trace.borrow().drops.is_empty());
    trace.borrow_mut().retire_refusals.push_back(Fault::new(32));
    assert!(owner.retry_retirement().is_err());
    assert!(trace.borrow().drops.is_empty());
    match owner.retry_retirement() {
        Ok(true) => {}
        _ => panic!("explicit memory retirement required"),
    }
    assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
    assert_eq!(trace.borrow().drops, [true]);
    let mixer = owner.take_recovered_mixer().unwrap();
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (5, 2)
    );
    assert!(mixer.pause_requested());
}
#[test]
fn start_and_observation_failures_keep_original_errors_and_separate_cleanup_refusal_then_recover_by_retry()
 {
    for mode in [Mode::StartError, Mode::ObserveError] {
        let Rig {
            mut producer,
            current,
            pause,
            output,
            trace,
        } = rig();
        let mut owner = controller(output, &trace);
        let error = Fault::new(41);
        let pointer = error.pointer();
        let error = if matches!(mode, Mode::StartError) {
            failure(owner.begin(
                Request {
                    mode,
                    fault: Some(error),
                },
                &current,
                &pause,
                Timestamp::ZERO,
                10_000_000,
                || producer.hold_pause(),
            ))
        } else {
            begin(
                &mut owner,
                &mut producer,
                &current,
                &pause,
                Request {
                    mode,
                    fault: Some(error),
                },
            );
            trace.borrow_mut().retire_refusals.push_back(Fault::new(42));
            failure(owner.poll(point(3_000_000_000)))
        };
        original(&error, pointer);
        assert!(error.cleanup.is_some());
        assert_eq!(owner.state(), ReplacementState::PendingRetirement);
        match owner.retry_retirement() {
            Ok(true) => {}
            _ => panic!("cleanup retry must recover"),
        }
        assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
        assert_eq!(owner.last_issued_epoch(), 1);
    }
}
#[test]
fn wrong_created_epoch_or_basis_is_refused_before_start_and_retains_recoverable_model() {
    for mode in [Mode::WrongEpoch, Mode::WrongBasis] {
        let Rig {
            mut producer,
            current,
            pause,
            output,
            trace,
        } = rig();
        let mut owner = controller(output, &trace);
        let _error = failure(owner.begin(
            normal(mode),
            &current,
            &pause,
            Timestamp::ZERO,
            10_000_000,
            || producer.hold_pause(),
        ));
        assert_eq!(owner.last_issued_epoch(), 1);
        assert!(!trace.borrow().calls.contains(&"start"));
        assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
        assert!(owner.take_recovered_mixer().is_some());
    }
}
#[test]
fn absent_pair_or_report_never_becomes_ready_and_exact_deadline_clock_refusal_or_cancel_retires_candidate_once()
 {
    for mode in [Mode::NoPair, Mode::NoReport] {
        let Rig {
            mut producer,
            current,
            pause,
            output,
            trace,
        } = rig();
        let mut owner = controller(output, &trace);
        begin(&mut owner, &mut producer, &current, &pause, normal(mode));
        assert!(matches!(owner.poll(point(3_000_000_000)), Ok(None)));
        let _ = failure(owner.poll(point(3_010_000_000)));
        assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
        assert_eq!(owner.last_issued_epoch(), 1);
        let calls = trace.borrow().calls.len();
        assert!(owner.poll(point(3_011_000_000)).is_err());
        assert_eq!(trace.borrow().calls.len(), calls);
    }
    for first in [
        604_800_000_000_000,
        9_007_199_254_740_993,
        i64::MAX - 10_000_000,
    ] {
        let Rig {
            mut producer,
            current,
            pause,
            output,
            trace,
        } = rig();
        let mut owner = controller(output, &trace);
        begin(
            &mut owner,
            &mut producer,
            &current,
            &pause,
            normal(Mode::NoPair),
        );
        assert!(matches!(owner.poll(point(first)), Ok(None)));
        let error = failure(owner.poll(point(first + 10_000_000)));
        assert!(matches!(
            error.cause,
            ReplacementCause::Policy("output replacement observation timed out")
        ));
        assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
    }
    for cancel in [false, true] {
        let Rig {
            mut producer,
            current,
            pause,
            output,
            trace,
        } = rig();
        let mut owner = controller(output, &trace);
        begin(
            &mut owner,
            &mut producer,
            &current,
            &pause,
            normal(Mode::NoPair),
        );
        assert!(matches!(owner.poll(point(3_000_000_000)), Ok(None)));
        if cancel {
            match owner.cancel() {
                Ok(true) => {}
                _ => panic!("cancel must retire candidate"),
            }
        } else {
            let _ = failure(owner.poll(point(2_999_999_999)));
        }
        assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
        assert_eq!(trace.borrow().epochs, [1]);
    }
}
#[test]
fn invalid_policy_phase_epoch_and_existing_hold_refuse_without_retiring_or_constructing_output() {
    let Rig {
        mut producer,
        current,
        pause,
        output,
        trace,
    } = rig();
    let mut owner = controller(output, &trace);
    let _ = failure(owner.begin(
        normal(Mode::Normal),
        &current,
        &pause,
        Timestamp::ZERO,
        0,
        || panic!("invalid wait must not acquire hold"),
    ));
    assert!(trace.borrow().calls.is_empty());
    let running = NativePause::new(pair(0).source, ClockDomainId(2), 3).unwrap();
    let _ = failure(owner.begin(
        normal(Mode::Normal),
        &current,
        &running,
        Timestamp::ZERO,
        10_000_000,
        || panic!("invalid phase must not acquire hold"),
    ));
    assert!(trace.borrow().calls.is_empty());
    let mut mismatched = current.clone();
    mismatched
        .rebind_output(1, pair(0).source, pair(0).source, Timestamp::ZERO)
        .unwrap();
    let _ = failure(owner.begin(
        normal(Mode::Normal),
        &mismatched,
        &pause,
        Timestamp::ZERO,
        10_000_000,
        || panic!("mismatched epoch must not acquire hold"),
    ));
    assert!(trace.borrow().calls.is_empty());
    let hold = match producer.hold_pause() {
        Ok(hold) => hold,
        Err(_) => panic!("external hold required"),
    };
    let _ = failure(owner.begin(
        normal(Mode::Normal),
        &current,
        &pause,
        Timestamp::ZERO,
        10_000_000,
        || producer.hold_pause(),
    ));
    assert!(trace.borrow().calls.is_empty());
    drop(hold);
    assert_eq!(owner.state(), ReplacementState::Attached);
    assert_eq!(owner.last_issued_epoch(), 0);
    begin(
        &mut owner,
        &mut producer,
        &current,
        &pause,
        normal(Mode::Normal),
    );
    assert!(matches!(owner.cancel(), Ok(true)));
    let Rig {
        producer: _,
        mut current,
        mut pause,
        mut output,
        trace,
    } = rig();
    current
        .rebind_output(u64::MAX, pair(0).source, pair(0).source, Timestamp::ZERO)
        .unwrap();
    pause
        .rebind_output(u64::MAX, output.mixer.as_ref().unwrap())
        .unwrap();
    output.epoch = u64::MAX;
    let mut exhausted = controller(output, &trace);
    let _ = failure(exhausted.begin(
        normal(Mode::Normal),
        &current,
        &pause,
        Timestamp::ZERO,
        10_000_000,
        || panic!("epoch overflow must precede hold acquisition"),
    ));
    assert!(trace.borrow().calls.is_empty());
    assert_eq!(exhausted.last_issued_epoch(), u64::MAX);
}
