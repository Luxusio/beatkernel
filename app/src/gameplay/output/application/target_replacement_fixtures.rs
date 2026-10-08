//! Real target/source owners; effect scripts never implement clock authority.
use super::*;
use crate::gameplay::output::application::owner::GameplayOutputOwner;
use crate::{
    audio_authority::AudioAuthorityConfig,
    gameplay::output::ports::{
        OriginalTargetNativeOutputBackend, OutputReplacementBackend, TargetOutputTelemetry,
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayPauseControl},
    local_input::InputMerger,
    local_runtime::SoloRuntime,
    native_audio_startup::new_target_audio_presentation,
    native_converted_test_support::{controlled_pair, point, rig as converted_rig},
    native_gameplay::NativeGameplayConfig,
};
use beatkernel::{
    audio::*,
    input::*,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    runtime::{RuntimeProcessingClock, SoundBinding},
    time::{ClockDomainId, ClockPair, Duration, ExtrapolationPolicy},
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

#[derive(Debug)]
struct Fault(Box<u64>);
impl Fault {
    fn new(id: u64) -> Self {
        Self(Box::new(id))
    }
    fn pointer(&self) -> *const u64 {
        self.0.as_ref()
    }
}
impl std::fmt::Display for Fault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "scripted target effect {}", self.0)
    }
}
impl std::error::Error for Fault {}
#[derive(Clone, Copy, Debug)]
enum Mode {
    Normal,
    OpenFail,
    StartFail,
    ObserveFail,
    ReportFail,
    ConvertedFail,
    FactsFail,
    HeldFail,
    RetireFail,
    Quiet,
    WrongDomain,
    Repeated,
    AuxiliaryUnavailable,
    MalformedFacts,
    MalformedConverted,
}
#[derive(Clone, Copy, Debug)]
struct Request {
    rate: u32,
    mode: Mode,
}
struct Output {
    owner: Option<ConvertedNativeOutputState>,
    basis: TargetFrameBasis,
    epoch: u64,
    retired: bool,
    held: bool,
    started: bool,
    mode: Mode,
    presented: u64,
    last: Option<ConvertedRenderReport>,
    first: Option<TargetNativeAudioSnapshot>,
    take_errors: VecDeque<Fault>,
    calls: Rc<RefCell<Vec<&'static str>>>,
    telemetry_override: Option<TargetOutputTelemetry>,
}
impl StoppedMixerSource<ConvertedNativeOutputState> for Output {
    type Error = Fault;
    fn take_stopped_mixer(&mut self) -> Result<Option<ConvertedNativeOutputState>, Fault> {
        self.calls.borrow_mut().push("take");
        if let Some(error) = self.take_errors.pop_front() {
            return Err(error);
        }
        if !self.retired {
            return Err(Fault::new(90));
        }
        Ok(self.owner.take())
    }
}
struct Effects {
    calls: Rc<RefCell<Vec<&'static str>>>,
    retire_errors: VecDeque<Fault>,
}
fn output(
    owner: ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    epoch: u64,
    mode: Mode,
    calls: &Rc<RefCell<Vec<&'static str>>>,
) -> Output {
    Output {
        owner: Some(owner),
        basis,
        epoch,
        retired: false,
        held: false,
        started: false,
        mode,
        presented: 0,
        last: None,
        first: None,
        take_errors: VecDeque::new(),
        calls: calls.clone(),
        telemetry_override: None,
    }
}
impl OutputReplacementBackend<ConvertedNativeOutputState, TargetFrameBasis> for Effects {
    type Presentation = beatkernel::time::presentation::PresentationEstimator;
    type Output = Output;
    type Request = Request;
    type Error = Fault;
    fn open(
        &mut self,
        request: Request,
        mut owner: ConvertedNativeOutputState,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<Fault, Output, ConvertedNativeOutputState>> {
        self.calls.borrow_mut().push("open");
        if matches!(request.mode, Mode::OpenFail) {
            return Err(OutputOpenFailure::recovered_state(
                Fault::new(11),
                Some(owner),
            ));
        }
        if owner
            .reconfigure(
                DeviceFormat::new(request.rate, 1, SampleEncoding::Float32, None).unwrap(),
                ChannelMatrix::default_mix(1, 1).unwrap(),
                128,
            )
            .is_err()
        {
            return Err(OutputOpenFailure::recovered_state(
                Fault::new(12),
                Some(owner),
            ));
        }
        let basis = owner.target_frame_basis();
        Ok(output(owner, basis, epoch, request.mode, &self.calls))
    }
    fn retire(&mut self, o: &mut Output) -> Result<(), Fault> {
        self.calls.borrow_mut().push("retire");
        if matches!(o.mode, Mode::RetireFail) {
            o.mode = Mode::Normal;
            return Err(Fault::new(46));
        }
        if let Some(error) = self.retire_errors.pop_front() {
            return Err(error);
        }
        o.retired = true;
        Ok(())
    }
    fn prepare_replacement_start(&mut self, o: &mut Output) -> Result<(), Fault> {
        self.set_held(o, true)
    }
    fn start(&mut self, o: &mut Output) -> Result<(), Fault> {
        self.calls.borrow_mut().push("start");
        assert!(o.held, "cold held mode precedes start");
        if matches!(o.mode, Mode::StartFail) {
            return Err(Fault::new(21));
        }
        o.started = true;
        Ok(())
    }
    fn epoch(&self, o: &Output) -> u64 {
        o.epoch
    }
    fn basis(&self, o: &Output) -> TargetFrameBasis {
        o.basis
    }
    fn observe(&mut self, _: &mut Output, _: &mut Self::Presentation) -> Result<(), Fault> {
        panic!("target path cannot invoke legacy clock estimator")
    }
    fn render_report(&self, o: &Output) -> Result<Option<RenderReport>, Fault> {
        if matches!(o.mode, Mode::ReportFail) {
            Err(Fault::new(42))
        } else {
            Ok(o.owner.as_ref().unwrap().last_real_source_report())
        }
    }
}
impl OriginalTargetNativeOutputBackend<ConvertedNativeOutputState> for Effects {
    fn output_telemetry(&self, o: &Output) -> Result<Option<TargetOutputTelemetry>, Fault> {
        if matches!(o.mode, Mode::AuxiliaryUnavailable) {
            return Ok(None);
        }
        if let Some(telemetry) = o.telemetry_override {
            return Ok(Some(telemetry));
        }
        let source = self.render_report(o)?;
        let mut converted = self.converted_report(o)?;
        let mut facts = self.boundary_facts(o)?;
        if matches!(o.mode, Mode::MalformedFacts) {
            facts.source_rate = 0;
        }
        if matches!(o.mode, Mode::MalformedConverted) {
            converted.as_mut().unwrap().target_rate += 1;
        }
        Ok(Some(TargetOutputTelemetry {
            source,
            converted,
            facts,
        }))
    }
    fn planned_target_basis(
        &self,
        request: &Request,
        owner: &ConvertedNativeOutputState,
    ) -> Result<TargetFrameBasis, Fault> {
        self.calls.borrow_mut().push("plan");
        owner
            .validate_reconfigure(
                DeviceFormat::new(request.rate, 1, SampleEncoding::Float32, None).unwrap(),
                &ChannelMatrix::default_mix(1, 1).unwrap(),
                128,
            )
            .map_err(|_| Fault::new(31))?;
        let old = owner.target_frame_basis();
        TargetFrameBasis::new(old.origin(), old.start_time(), request.rate)
            .map_err(|_| Fault::new(32))
    }
    fn observe_native_target(
        &mut self,
        o: &mut Output,
    ) -> Result<Option<TargetNativeAudioSnapshot>, Fault> {
        self.calls.borrow_mut().push("native");
        if matches!(o.mode, Mode::ObserveFail) {
            return Err(Fault::new(41));
        }
        if matches!(o.mode, Mode::Quiet) {
            return Ok(None);
        }
        if matches!(o.mode, Mode::Repeated) && o.first.is_some() {
            return Ok(o.first);
        }
        assert!(o.started && o.held);
        let owner = o.owner.as_mut().unwrap();
        let report = if owner.pending_frames() > 0 {
            let report = owner.pending_report().unwrap();
            let count = owner.pending_frames();
            owner.admit(count).unwrap();
            o.presented += count as u64;
            report
        } else {
            let report = owner.render_held_pending(2).unwrap();
            owner.admit(2).unwrap();
            o.presented += 2;
            report
        };
        o.last = Some(report);
        let mut pair = relation(o.basis, o.presented);
        if matches!(o.mode, Mode::WrongDomain) {
            pair.target.domain = ClockDomainId(99);
        }
        let snapshot = TargetNativeAudioSnapshot {
            epoch: o.epoch,
            basis: o.basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(pair),
        };
        o.first.get_or_insert(snapshot);
        Ok(Some(snapshot))
    }
    fn converted_report(&self, o: &Output) -> Result<Option<ConvertedRenderReport>, Fault> {
        if matches!(o.mode, Mode::ConvertedFail) {
            return Err(Fault::new(43));
        }
        Ok(o.last)
    }
    fn boundary_facts(&self, o: &Output) -> Result<ConvertedBoundaryFacts, Fault> {
        if matches!(o.mode, Mode::FactsFail) {
            return Err(Fault::new(44));
        }
        Ok(o.owner.as_ref().unwrap().boundaries())
    }
    fn set_held(&mut self, o: &mut Output, held: bool) -> Result<(), Fault> {
        self.calls
            .borrow_mut()
            .push(if held { "held" } else { "release" });
        if matches!(o.mode, Mode::HeldFail) {
            return Err(Fault::new(45));
        }
        o.held = held;
        Ok(())
    }
}
type Controller = OutputReplacement<Effects, ConvertedNativeOutputState, TargetFrameBasis>;
fn relation(basis: TargetFrameBasis, frame: u64) -> ClockPair {
    controlled_pair(
        basis,
        frame,
        1_000_000_000
            + basis
                .point_at_stream_frame(frame)
                .unwrap()
                .timestamp
                .as_nanos(),
    )
}
fn snap(epoch: u64, basis: TargetFrameBasis, frame: u64) -> TargetNativeAudioSnapshot {
    TargetNativeAudioSnapshot {
        epoch,
        basis,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(relation(basis, frame)),
    }
}
fn now() -> ClockPoint {
    point(1, 1_010_000_000)
}
struct Rig {
    current: NativeAudioPresentation,
    pause: NativePause,
    end: Option<NativeEnd>,
    merger: InputMerger,
    runtime: SoloRuntime,
    config: NativeGameplayConfig,
    output: Option<Output>,
    calls: Rc<RefCell<Vec<&'static str>>>,
}
impl Rig {
    fn new(partial: bool) -> Self {
        let rate = if partial { 32_000 } else { 48_000 };
        let (mut producer, mut owner) = converted_rig(24_000, rate, None, Some(100), 0);
        let basis = owner.target_frame_basis();
        let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 24_000)
            .unwrap()
            .with_playback_end_frame(100)
            .unwrap()
            .with_target_basis(7, basis)
            .unwrap();
        owner.render_pending(1).unwrap();
        owner.admit(1).unwrap();
        pause.request_in_epoch(7, true, relation(basis, 1)).unwrap();
        producer.request_pause(true);
        owner.render_pending(8).unwrap();
        let pause_frame = if partial { 3 } else { 4 };
        pause
            .observe_target(
                7,
                basis,
                owner.boundaries(),
                owner.last_real_source_report(),
                relation(basis, pause_frame),
            )
            .unwrap()
            .unwrap();
        owner.admit(8).unwrap();
        let mut current = new_target_audio_presentation(
            7,
            basis,
            ClockDomainId(1),
            point(33, 5_000_000_000),
            AudioAuthorityConfig {
                history_capacity: 8,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
        )
        .unwrap();
        current.admit_target(snap(7, basis, 1)).unwrap();
        current.admit_target(snap(7, basis, pause_frame)).unwrap();
        let mut end = NativeEnd::new(point(2, 0), ClockDomainId(1), 24_000, 100)
            .unwrap()
            .with_target_basis(7, basis)
            .unwrap();
        end.observe_target(
            7,
            basis,
            owner.boundaries(),
            owner.last_real_source_report(),
            relation(basis, 0),
        )
        .unwrap();
        let source =
            beatkernel_bms::parse("#BPM 60\n#WAV01 key.wav\n#00011:01", Default::default())
                .unwrap();
        let chart = source.compile().unwrap().chart;
        let object = chart.objects()[0].id;
        let judge = JudgeEngine::new(
            chart,
            source.rules(),
            JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(1),
                    early: Duration::from_nanos(10_000_000),
                    late: Duration::from_nanos(10_000_000),
                }],
                Duration::ZERO,
            )
            .unwrap(),
        )
        .unwrap();
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(7),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let mut runtime = SoloRuntime::new(
            ClockDomainId(33),
            ClockDomainId(2),
            Transport::new(
                Timestamp::from_nanos(5_000_000_000),
                Timestamp::ZERO,
                Rate::NORMAL,
            ),
            bindings,
            judge,
            producer,
            vec![SoundBinding {
                object,
                stage: JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(30),
                gain: 0.25,
            }],
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        runtime
            .transport_mut()
            .pause(Timestamp::from_nanos(5_000_000_000))
            .unwrap();
        if partial {
            owner.render_held_pending(3).unwrap();
            owner.admit(1).unwrap();
        }
        let calls = Rc::new(RefCell::new(Vec::new()));
        let native = output(owner, basis, 7, Mode::Normal, &calls);
        Self {
            current,
            pause,
            end: Some(end),
            merger: InputMerger::new(
                ClockDomainId(1),
                point(1, 1_000_000_000),
                vec![DeviceId(9)],
                8,
            )
            .unwrap(),
            runtime,
            config: NativeGameplayConfig {
                origin: point(1, 1_000_000_000),
                stream_origin: point(2, 0),
                playback_origin: point(2, 0),
                song_origin: Timestamp::from_nanos(5_000_000_000),
                sample_rate: 24_000,
                end_song: Some(Timestamp::from_nanos(100 * 1_000_000_000 / 24_000)),
                advance_lag: Duration::ZERO,
                seconds: None,
                pause_supported: true,
                logical_schedule: true,
            },
            output: Some(native),
            calls,
        }
    }
    fn controller(&mut self) -> Controller {
        let mut controller = Controller::new(Effects {
            calls: self.calls.clone(),
            retire_errors: VecDeque::new(),
        });
        assert!(controller.attach(self.output.take().unwrap()).is_ok());
        controller
    }
    fn begin(&mut self, c: &mut Controller, mode: Mode) -> Result<(), ReplacementFailure<Fault>> {
        c.begin_target_audio(
            Request { rate: 32_000, mode },
            &self.current,
            &self.pause,
            self.end.as_ref(),
            &self.merger,
            self.config.song_origin,
            100_000_000,
            || self.runtime.hold_audio_pause(),
        )
    }
    fn context(&mut self) -> GameplayAudioOutputContext<'_> {
        GameplayAudioOutputContext {
            control: GameplayPauseControl::solo(&mut self.runtime),
            presentation: &mut self.current,
            merger: &self.merger,
            pause: &mut self.pause,
            config: &mut self.config,
            end: &mut self.end,
        }
    }
    fn ready(&mut self) -> ReadyTargetAudioOutput<Output> {
        let mut c = self.controller();
        self.begin(&mut c, Mode::Normal).unwrap();
        for _ in 0..4 {
            if let Some(ready) = c
                .poll_target_audio(now(), &self.current, &self.merger)
                .unwrap()
            {
                return ready;
            }
        }
        panic!("two genuine progressing fresh observations must produce readiness")
    }
    fn state(&mut self) -> String {
        let transport = format!("{:?}", self.runtime.transport_mut());
        format!(
            "{:?}",
            (
                &self.current,
                &self.pause,
                &self.end,
                self.config,
                transport,
                self.runtime.judge().effective_song_time(),
                self.merger.pending()
            )
        )
    }
}

#[test]
fn retained_target_tail_defers_candidate_anchors_then_two_fresh_native_pairs_return_ready_with_original_hold(
) {
    let mut rig = Rig::new(true);
    let phase = rig
        .output
        .as_ref()
        .unwrap()
        .owner
        .as_ref()
        .unwrap()
        .converter_owner()
        .source_position();
    let before = rig.state();
    let mut c = rig.controller();
    rig.begin(&mut c, Mode::Normal).unwrap();
    let calls = rig.calls.borrow();
    let held = calls.iter().position(|call| *call == "held").unwrap();
    let start = calls.iter().position(|call| *call == "start").unwrap();
    assert!(held < start);
    drop(calls);
    assert!(c
        .poll_target_audio(now(), &rig.current, &rig.merger)
        .unwrap()
        .is_none());
    match &c.slot {
        Slot::Waiting(waiting) => match &waiting.timing {
            WaitingTiming::Target(timing) => {
                assert!(timing.snapshots.iter().all(Option::is_none));
                assert!(timing.pairs.iter().all(Option::is_none));
            }
            _ => panic!("actual target timing"),
        },
        _ => panic!("retained tail must keep waiting"),
    }
    assert!(c
        .poll_target_audio(now(), &rig.current, &rig.merger)
        .unwrap()
        .is_none());
    assert_eq!(rig.state(), before);
    let ready = c
        .poll_target_audio(now(), &rig.current, &rig.merger)
        .unwrap()
        .unwrap();
    assert_eq!(c.state(), ReplacementState::Detached);
    assert_eq!(rig.state(), before);
    assert_eq!(ready.basis.sample_rate(), 32_000);
    assert_eq!(ready.facts.source_rate, 24_000);
    assert!(ready.end().is_some());
    let owner = ready.output.owner.as_ref().unwrap();
    assert_eq!(owner.converter_owner().source_position(), phase);
    assert_eq!(owner.mixer().playback_frame_cursor(), 2);
    assert!(rig.runtime.hold_audio_pause().is_err());
    let pairs = ready.snapshots.map(|snapshot| match snapshot.evidence {
        OriginalNativePresentationEvidence::SuppliedPair(pair) => pair,
        _ => panic!("validated actual target pair"),
    });
    assert!(pairs[0].source.timestamp < pairs[1].source.timestamp);
    assert!(pairs[0].target.timestamp < pairs[1].target.timestamp);
    assert_eq!(ready.prepared.latest_record().pair(), pairs[1]);
}

#[test]
fn open_start_observation_domain_cancel_timeout_and_pending_retirement_preserve_old_gameplay_and_complete_owner(
) {
    for mode in [
        Mode::OpenFail,
        Mode::StartFail,
        Mode::ObserveFail,
        Mode::ReportFail,
        Mode::WrongDomain,
    ] {
        let mut rig = Rig::new(false);
        let before = rig.state();
        let phase = rig
            .output
            .as_ref()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position();
        let mut c = rig.controller();
        match rig.begin(&mut c, mode) {
            Err(_) => {}
            Ok(()) => {
                assert!(c
                    .poll_target_audio(now(), &rig.current, &rig.merger)
                    .is_err());
            }
        }
        assert_eq!(rig.state(), before);
        assert_eq!(c.state(), ReplacementState::RecoveredMixer);
        let recovered = c.take_recovered_mixer().unwrap();
        assert_eq!(recovered.converter_owner().source_position(), phase);
        assert_eq!(recovered.mixer().playback_frame_cursor(), 2);
        assert!(recovered.mixer().pause_requested());
        assert!(rig.runtime.hold_audio_pause().is_ok());
    }
    for timeout in [false, true] {
        let mut rig = Rig::new(true);
        let before = rig.state();
        let actual = rig.output.as_ref().unwrap().owner.as_ref().unwrap();
        let retained = actual.pending_samples().to_vec();
        let basis = actual.target_frame_basis();
        let mut c = rig.controller();
        rig.begin(&mut c, Mode::Quiet).unwrap();
        assert!(c
            .poll_target_audio(now(), &rig.current, &rig.merger)
            .unwrap()
            .is_none());
        if timeout {
            assert!(c
                .poll_target_audio(
                    point(1, now().timestamp.as_nanos() + 100_000_000),
                    &rig.current,
                    &rig.merger
                )
                .is_err());
        } else {
            assert!(c.cancel().unwrap());
        }
        assert_eq!(rig.state(), before);
        let recovered = c.take_recovered_mixer().unwrap();
        assert_eq!(recovered.pending_samples(), retained);
        assert_eq!(recovered.target_frame_basis(), basis);
        assert!(recovered.pending_is_held());
    }
    let mut rig = Rig::new(true);
    let before = rig.state();
    let primary = Fault::new(51);
    let pointer = primary.pointer();
    let recovery = Fault::new(52);
    let recovery_pointer = recovery.pointer();
    rig.output.as_mut().unwrap().take_errors.push_back(recovery);
    let mut c = rig.controller();
    c.backend_mut().retire_errors.push_back(primary);
    let failure = rig.begin(&mut c, Mode::Normal).unwrap_err();
    assert!(
        matches!(&failure.cause,ReplacementCause::Backend{phase:ReplacementPhase::Retire,error} if error.pointer()==pointer)
    );
    assert_eq!(
        failure.recovery.as_ref().unwrap().pointer(),
        recovery_pointer
    );
    assert_eq!(c.state(), ReplacementState::PendingRetirement);
    assert_eq!(rig.state(), before);
    assert!(c.retry_retirement().unwrap());
    assert!(c.take_recovered_mixer().unwrap().pending_is_held());
}

#[test]
fn cold_finite_validation_and_occupied_hold_refuse_before_native_open_and_keep_recovery() {
    let mut rig = Rig::new(false);
    let old_basis = rig.current.target_basis().unwrap();
    rig.end = Some(
        NativeEnd::new(point(2, 0), ClockDomainId(1), 24_000, 101)
            .unwrap()
            .with_target_basis(7, old_basis)
            .unwrap(),
    );
    let before = rig.state();
    let mut c = rig.controller();
    assert!(rig.begin(&mut c, Mode::Normal).is_err());
    assert_eq!(rig.state(), before);
    assert!(!rig.calls.borrow().contains(&"open"));
    assert_eq!(c.state(), ReplacementState::RecoveredMixer);
    assert_eq!(
        c.take_recovered_mixer()
            .unwrap()
            .mixer()
            .config()
            .playback_end_frame(),
        Some(100)
    );
    let mut rig = Rig::new(false);
    let before = rig.state();
    let mut c = rig.controller();
    let hold = rig.runtime.hold_audio_pause().unwrap();
    assert!(rig.begin(&mut c, Mode::Normal).is_err());
    assert_eq!(rig.state(), before);
    assert!(rig.calls.borrow().is_empty());
    assert_eq!(c.state(), ReplacementState::Attached);
    drop(hold);
}

#[test]
fn publication_refusals_return_ready_output_hold_and_every_old_owner_unchanged() {
    for failure in 0..9 {
        let mut rig = Rig::new(false);
        let mut ready = rig.ready();
        let basis = ready.basis;
        let snapshots = ready.snapshots;
        let source = ready
            .output
            .owner
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position();
        let duration = ready
            .output
            .owner
            .as_ref()
            .unwrap()
            .converter_owner()
            .target_time();
        let mut slot = None;
        match failure {
            0 => {
                rig.current
                    .admit_target(snap(7, rig.current.target_basis().unwrap(), 6))
                    .unwrap();
            }
            1 => {
                slot = Rig::new(false).output;
            }
            2 => {
                rig.merger
                    .admit(
                        PhysicalInputEvent::Button(ButtonEvent {
                            meta: EventMeta::new(DeviceId(9), point(1, 1_000_100_000), 1),
                            control: PhysicalControlId::keyboard(7),
                            state: ButtonState::Down,
                        }),
                        now(),
                    )
                    .unwrap();
            }
            3 => rig.config.sample_rate = 32_000,
            4 => {
                let old = rig.current.target_basis().unwrap();
                rig.end = Some(
                    NativeEnd::new(point(2, 0), ClockDomainId(1), 24_000, 101)
                        .unwrap()
                        .with_target_basis(7, old)
                        .unwrap(),
                );
            }
            5 => rig.config.logical_schedule = false,
            6 => ready.facts.source_rate = 32_000,
            8 => {
                rig.merger = InputMerger::new(
                    ClockDomainId(99),
                    point(99, 1_000_000_000),
                    vec![DeviceId(9)],
                    8,
                )
                .unwrap()
            }
            _ => {}
        }
        let before = rig.state();
        let occupied = slot.as_ref().map(|o| o.epoch);
        let at = if failure == 7 {
            point(1, 3_000_000_000)
        } else {
            now()
        };
        let refusal =
            match publish_ready_target_audio_output_held(ready, &mut slot, rig.context(), at) {
                Err(refusal) => refusal,
                Ok(_) => panic!("explicit incompatible publication must refuse"),
            };
        assert_eq!(rig.state(), before);
        assert_eq!(slot.as_ref().map(|o| o.epoch), occupied);
        assert_eq!(refusal.ready.basis, basis);
        assert_eq!(refusal.ready.snapshots, snapshots);
        let owner = refusal.ready.output.owner.as_ref().unwrap();
        assert_eq!(owner.converter_owner().source_position(), source);
        assert_eq!(owner.converter_owner().target_time(), duration);
        assert!(rig.runtime.hold_audio_pause().is_err());
    }
}

#[test]
fn repeated_native_pair_cannot_replace_two_fresh_observations_or_extend_readiness() {
    let mut rig = Rig::new(false);
    let before = rig.state();
    let mut c = rig.controller();
    rig.begin(&mut c, Mode::Repeated).unwrap();
    for _ in 0..4 {
        assert!(c
            .poll_target_audio(now(), &rig.current, &rig.merger)
            .unwrap()
            .is_none());
        assert_eq!(rig.state(), before);
    }
    assert_eq!(c.state(), ReplacementState::Waiting);
    assert!(rig.runtime.hold_audio_pause().is_err());
    assert!(c.cancel().unwrap());
    assert_eq!(c.state(), ReplacementState::RecoveredMixer);
    let owner = c.take_recovered_mixer().unwrap();
    assert_eq!(owner.mixer().playback_frame_cursor(), 2);
    assert!(owner.mixer().pause_requested());
}

#[test]
fn valid_atomic_target_publication_preserves_source_bgm_and_runtime_keysound_fifo_times_across_rate_and_held_gap(
) {
    let mut rig = Rig::new(false);
    let ready = rig.ready();
    let basis = ready.basis;
    let raw = ready.playback_origin;
    let source_phase = ready
        .output
        .owner
        .as_ref()
        .unwrap()
        .converter_owner()
        .source_position();
    let song = rig.config.song_origin;
    let transport = format!("{:?}", rig.runtime.transport_mut());
    let mut slot = None;
    let hold = match publish_ready_target_audio_output_held(ready, &mut slot, rig.context(), now())
    {
        Ok(hold) => hold,
        Err(_) => panic!("valid real target transaction"),
    };
    assert_eq!(rig.current.authority().epoch().id, 8);
    assert_eq!(rig.current.target_basis(), Some(basis));
    assert_eq!(rig.pause.epoch(), 8);
    assert_eq!(rig.config.sample_rate, 24_000);
    assert_eq!(rig.config.song_origin, song);
    assert_eq!(rig.config.stream_origin, raw);
    assert_eq!(rig.config.playback_origin, raw);
    assert_eq!(format!("{:?}", rig.runtime.transport_mut()), transport);
    assert!(rig.runtime.hold_audio_pause().is_err());
    assert_eq!(
        slot.as_ref()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position(),
        source_phase
    );
    assert_eq!(rig.current.authority().committed_operation(), None);
    assert_eq!(rig.current.authority().committed_presentation(), None);
    let resume_reference = rig.current.authority().latest_observation().unwrap();
    assert!(rig
        .pause
        .request_in_epoch(8, false, resume_reference)
        .unwrap());
    drop(hold);
    rig.runtime.request_audio_pause(false);
    let native = slot.as_mut().unwrap();
    native.held = false;
    let owner = native.owner.as_mut().unwrap();
    let actual = owner.render_pending(2).unwrap();
    owner.admit(2).unwrap();
    native.presented += 2;
    assert!(!actual.source.unwrap().paused);
    let first_resumed = snap(8, basis, native.presented);
    let first_pair = relation(basis, native.presented);
    let resumed = rig
        .pause
        .observe_target(
            8,
            basis,
            owner.boundaries(),
            owner.last_real_source_report(),
            first_pair,
        )
        .unwrap()
        .unwrap();
    assert!(!resumed.paused);
    assert_eq!(resumed.playback_frame, 2);
    rig.current.admit_target(first_resumed).unwrap();
    let actual = owner.render_pending(2).unwrap();
    owner.admit(2).unwrap();
    native.presented += 2;
    let second_pair = relation(basis, native.presented);
    rig.current
        .admit_target(snap(8, basis, native.presented))
        .unwrap();
    let observations = [first_pair, second_pair];
    let commands = vec![
        AudioCommand::Play {
            voice: VoiceId(10),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_000_000),
            gain: 0.5,
        },
        AudioCommand::Play {
            voice: VoiceId(20),
            sample: SampleId(1),
            at: Timestamp::from_nanos(2_000_000),
            gain: -0.5,
        },
    ];
    let mut bgm = crate::bgm::BgmFeeder::new(
        commands.clone(),
        crate::bgm::BgmConfig {
            output_origin: point(2, 0),
            sample_rate: rig.config.sample_rate,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(10_000_000),
            max_pending: 4,
        },
    )
    .unwrap();
    let mut fifo = Vec::new();
    crate::native_audio::feed_rendered(&mut bgm, actual.source, |command| {
        fifo.push(command);
        rig.runtime.enqueue_audio(command)
    })
    .unwrap();
    assert_eq!(fifo, commands);
    let acquired = point(
        1,
        (observations[0].target.timestamp.as_nanos() + observations[1].target.timestamp.as_nanos())
            / 2,
    );
    let mut meta = EventMeta::new(DeviceId(9), acquired, 1);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(7),
        code: Some(7),
        timestamp: Some(point(99, 123)),
    });
    let input = PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    });
    rig.merger.admit(input.clone(), now()).unwrap();
    rig.current
        .authority_mut()
        .record_acquired_prefix(now())
        .unwrap();
    let mapped = rig
        .current
        .authority()
        .prepare_input(acquired, now())
        .unwrap()
        .unwrap();
    assert_eq!(mapped.original(), acquired);
    let delivered = rig.merger.pop_ready(now()).unwrap().unwrap();
    assert_eq!(delivered, input);
    rig.runtime
        .transport_mut()
        .resume(mapped.output().timestamp)
        .unwrap();
    let report = rig
        .runtime
        .process_input(delivered, mapped.mapper(), point(2, 2_000_000))
        .unwrap();
    assert_eq!(
        report.input.as_ref().unwrap().meta().native,
        input.meta().native
    );
    assert_eq!(
        report.input.as_ref().unwrap().meta().original_clock_point,
        Some(acquired)
    );
    rig.current.authority_mut().commit_input(mapped).unwrap();
    assert_eq!(
        report.audio_commands,
        vec![AudioCommand::Play {
            voice: VoiceId(30),
            sample: SampleId(1),
            at: Timestamp::from_nanos(2_000_000),
            gain: 0.25
        }]
    );
    fifo.extend_from_slice(&report.audio_commands);
    assert_eq!(
        fifo.iter().map(|c| c.at()).collect::<Vec<_>>(),
        vec![
            Timestamp::from_nanos(1_000_000),
            Timestamp::from_nanos(2_000_000),
            Timestamp::from_nanos(2_000_000)
        ]
    );
    let rendered = owner.render_pending(128).unwrap();
    assert_eq!(rendered.source.unwrap().counters.commands_consumed, 4);
    assert_eq!(owner.mixer().config().format().sample_rate(), 24_000);
    assert_eq!(owner.format().sample_rate(), 32_000);
    assert!(owner
        .pending_samples()
        .iter()
        .any(|sample| (*sample - 0.3125).abs() < 1e-6));
}

type TargetOwner = GameplayOutputOwner<Effects, ConvertedNativeOutputState, TargetFrameBasis>;

fn gameplay_owner(rig: &mut Rig) -> TargetOwner {
    let mut native = rig.output.take().unwrap();
    native.started = true;
    native.held = true;
    // The real initial source block has already been admitted by Rig::new.
    native.presented = if native.owner.as_ref().unwrap().pending_frames() == 0 {
        9
    } else {
        10
    };
    GameplayOutputOwner::new(
        Effects {
            calls: rig.calls.clone(),
            retire_errors: VecDeque::new(),
        },
        native,
    )
}

fn owner_caches(owner: &TargetOwner) -> String {
    format!(
        "{:?}",
        (
            owner.render_report(),
            owner.converted_report(),
            owner.target_boundary_facts()
        )
    )
}

fn queue_target(owner: &mut TargetOwner, mode: Mode) {
    assert!(owner
        .queue(Request { rate: 32_000, mode }, 100_000_000)
        .is_ok());
}

#[test]
fn gameplay_target_clock_admission_survives_missing_auxiliary_tuple_and_recovers_all_caches() {
    for populated in [false, true] {
        let mut rig = Rig::new(false);
        let mut owner = gameplay_owner(&mut rig);
        if populated {
            owner.observe_target_native(&mut rig.current).unwrap();
        }
        let caches = owner_caches(&owner);
        let prior = rig.current.latest_record().unwrap().pair();
        owner.current_mut().unwrap().mode = Mode::AuxiliaryUnavailable;
        owner.observe_target_native(&mut rig.current).unwrap();
        let pair = rig.current.latest_record().unwrap().pair();
        let output = owner.current().unwrap();
        assert_eq!(pair, relation(output.basis, output.presented));
        assert!(pair.source.timestamp > prior.source.timestamp);
        assert!(pair.target.timestamp > prior.target.timestamp);
        assert_eq!(owner_caches(&owner), caches);
        assert_eq!(rig.current.authority().epoch().id, 7);
        owner.current_mut().unwrap().mode = Mode::Normal;
        owner.observe_target_native(&mut rig.current).unwrap();
        let output = owner.current().unwrap();
        assert_eq!(
            owner.render_report(),
            output.owner.as_ref().unwrap().last_real_source_report()
        );
        assert_eq!(owner.converted_report(), output.last);
        assert_eq!(
            owner.target_boundary_facts(),
            Some(output.owner.as_ref().unwrap().boundaries())
        );
        assert!(
            rig.current.latest_record().unwrap().pair().target.timestamp > pair.target.timestamp
        );
    }
}

#[test]
fn gameplay_target_available_malformed_auxiliary_tuple_refuses_before_clock_and_cache_commit() {
    for mode in [Mode::MalformedFacts, Mode::MalformedConverted] {
        let mut rig = Rig::new(false);
        let mut owner = gameplay_owner(&mut rig);
        owner.observe_target_native(&mut rig.current).unwrap();
        let before = rig.state();
        let caches = owner_caches(&owner);
        owner.current_mut().unwrap().mode = mode;
        assert!(owner.observe_target_native(&mut rig.current).is_err());
        assert_eq!(rig.state(), before);
        assert_eq!(owner_caches(&owner), caches);
        owner.current_mut().unwrap().mode = Mode::Normal;
        owner.observe_target_native(&mut rig.current).unwrap();
        assert_ne!(rig.state(), before);
    }
}

#[test]
fn gameplay_target_caches_use_one_coherent_generation_instead_of_separate_legacy_getters() {
    let mut rig = Rig::new(false);
    let mut owner = gameplay_owner(&mut rig);
    owner.observe_target_native(&mut rig.current).unwrap();
    // A distinct genuine owner supplies an internally consistent source/pause
    // generation. The latest native output has another generation entirely.
    let (mut producer, mut converted) = converted_rig(24_000, 48_000, None, Some(100), 0);
    converted.render_pending(5).unwrap();
    converted.admit(5).unwrap();
    producer.request_pause(true);
    let report = converted.render_pending(8).unwrap();
    let tuple = TargetOutputTelemetry {
        source: converted.last_real_source_report(),
        converted: Some(report),
        facts: converted.boundaries(),
    };
    assert!(tuple.source.unwrap().paused);
    assert!(tuple.facts.pause.is_some());
    assert_ne!(tuple.source, owner.render_report());
    assert_ne!(Some(tuple.facts), owner.target_boundary_facts());
    owner.current_mut().unwrap().telemetry_override = Some(tuple);
    owner.observe_target_native(&mut rig.current).unwrap();
    assert_eq!(owner.render_report(), tuple.source);
    assert_eq!(owner.converted_report(), tuple.converted);
    assert_eq!(owner.target_boundary_facts(), Some(tuple.facts));
    assert_ne!(owner.converted_report(), owner.current().unwrap().last);
    // An available seed is a complete generation even before the first report.
    // Its absent reports must clear old reports together with installing facts.
    owner.current_mut().unwrap().telemetry_override = Some(TargetOutputTelemetry {
        source: None,
        converted: None,
        facts: tuple.facts,
    });
    owner.observe_target_native(&mut rig.current).unwrap();
    assert_eq!(owner.render_report(), None);
    assert_eq!(owner.converted_report(), None);
    assert_eq!(owner.target_boundary_facts(), Some(tuple.facts));
    owner.current_mut().unwrap().telemetry_override = None;
    owner.observe_target_native(&mut rig.current).unwrap();
    assert_eq!(owner.converted_report(), owner.current().unwrap().last);
}

#[test]
fn target_replacement_missing_auxiliary_tuple_keeps_anchors_and_hold_until_genuine_recovery() {
    let mut rig = Rig::new(false);
    let before = rig.state();
    let mut controller = rig.controller();
    rig.begin(&mut controller, Mode::AuxiliaryUnavailable)
        .unwrap();
    for _ in 0..3 {
        assert!(controller
            .poll_target_audio(now(), &rig.current, &rig.merger)
            .unwrap()
            .is_none());
        match &controller.slot {
            Slot::Waiting(waiting) => match &waiting.timing {
                WaitingTiming::Target(timing) => {
                    assert!(timing.snapshots.iter().all(Option::is_none));
                    assert!(timing.pairs.iter().all(Option::is_none));
                    assert!(waiting.output.held && waiting.output.started);
                }
                _ => panic!("actual target timing"),
            },
            _ => panic!("auxiliary absence must defer replacement"),
        }
        assert_eq!(rig.state(), before);
        assert!(rig.runtime.hold_audio_pause().is_err());
    }
    match &mut controller.slot {
        Slot::Waiting(waiting) => waiting.output.mode = Mode::Normal,
        _ => unreachable!(),
    }
    assert!(controller
        .poll_target_audio(now(), &rig.current, &rig.merger)
        .unwrap()
        .is_none());
    let ready = controller
        .poll_target_audio(now(), &rig.current, &rig.merger)
        .unwrap()
        .unwrap();
    assert_eq!(rig.state(), before);
    assert!(rig.runtime.hold_audio_pause().is_err());
    assert!(ready.converted_report.state == ConvertedOutputState::Held);
    assert_eq!(ready.facts.source_rate, 24_000);
    drop(ready);
    assert!(rig.runtime.hold_audio_pause().is_ok());
}

#[test]
fn target_replacement_auxiliary_absence_still_times_out_and_recovers_complete_owner_and_hold() {
    let mut rig = Rig::new(false);
    let before = rig.state();
    let mut controller = rig.controller();
    rig.begin(&mut controller, Mode::AuxiliaryUnavailable)
        .unwrap();
    assert!(controller
        .poll_target_audio(now(), &rig.current, &rig.merger)
        .unwrap()
        .is_none());
    let expired = point(1, now().timestamp.as_nanos() + 100_000_000);
    assert!(controller
        .poll_target_audio(expired, &rig.current, &rig.merger)
        .is_err());
    assert_eq!(controller.state(), ReplacementState::RecoveredMixer);
    assert_eq!(rig.state(), before);
    assert!(rig.runtime.hold_audio_pause().is_ok());
    let recovered = controller.take_recovered_mixer().unwrap();
    assert_eq!(recovered.mixer().config().format().sample_rate(), 24_000);
    assert!(recovered.mixer().pause_requested());
}

#[test]
fn gameplay_target_owner_collects_all_fallible_effects_before_authority_and_cache_commit() {
    for mode in [
        Mode::ObserveFail,
        Mode::ReportFail,
        Mode::ConvertedFail,
        Mode::FactsFail,
        Mode::WrongDomain,
    ] {
        let mut rig = Rig::new(false);
        let mut owner = gameplay_owner(&mut rig);
        owner.observe_target_native(&mut rig.current).unwrap();
        let before = rig.state();
        let caches = owner_caches(&owner);
        let native_phase = owner
            .current()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position();
        owner.current_mut().unwrap().mode = mode;
        let failure = owner.observe_target_native(&mut rig.current).unwrap_err();
        match mode {
            Mode::WrongDomain => assert!(matches!(failure.cause, ReplacementCause::Timing(_))),
            _ => assert!(matches!(
                failure.cause,
                ReplacementCause::Backend {
                    phase: ReplacementPhase::Observe,
                    ..
                }
            )),
        }
        assert_eq!(rig.state(), before);
        assert_eq!(owner_caches(&owner), caches);
        assert_eq!(
            owner
                .current()
                .unwrap()
                .owner
                .as_ref()
                .unwrap()
                .converter_owner()
                .source_position(),
            native_phase
        );
        owner.current_mut().unwrap().mode = Mode::Normal;
        owner.observe_target_native(&mut rig.current).unwrap();
        assert!(
            rig.current.latest_record().unwrap().pair().source.timestamp
                > relation(owner.current().unwrap().basis, 11)
                    .source
                    .timestamp
        );
        assert_eq!(
            owner
                .current()
                .unwrap()
                .owner
                .as_ref()
                .unwrap()
                .mixer()
                .config()
                .format()
                .sample_rate(),
            24_000
        );
        assert_eq!(owner.converted_report().unwrap().target_rate, 48_000);
        assert_eq!(owner.target_boundary_facts().unwrap().source_rate, 24_000);
    }
}

#[test]
fn gameplay_target_owner_identity_and_held_effect_refuse_without_clock_or_cache_changes() {
    let mut rig = Rig::new(false);
    let mut owner = gameplay_owner(&mut rig);
    owner.observe_target_native(&mut rig.current).unwrap();
    let basis = owner.current().unwrap().basis;
    let before = rig.state();
    let caches = owner_caches(&owner);
    let call_count = rig.calls.borrow().len();
    owner.current_mut().unwrap().epoch += 1;
    assert!(owner.observe_target_native(&mut rig.current).is_err());
    assert_eq!(rig.calls.borrow().len(), call_count);
    assert_eq!(rig.state(), before);
    assert_eq!(owner_caches(&owner), caches);
    owner.current_mut().unwrap().epoch -= 1;
    owner.current_mut().unwrap().mode = Mode::HeldFail;
    assert!(owner.set_target_held(false).is_err());
    assert!(owner.current().unwrap().held);
    assert_eq!(rig.state(), before);
    assert_eq!(owner_caches(&owner), caches);
    owner.current_mut().unwrap().mode = Mode::Normal;
    assert!(owner
        .audio_pause_observation_target(&rig.current, point(99, 0))
        .is_err());
    match owner
        .audio_pause_observation_target(&rig.current, now())
        .unwrap()
    {
        crate::live_pause::LivePauseObservation::Target {
            epoch,
            basis: actual,
            source,
            facts,
            pair,
        } => {
            assert_eq!(epoch, 7);
            assert_eq!(actual, basis);
            assert_eq!(source, owner.render_report());
            assert_eq!(facts, owner.target_boundary_facts().unwrap());
            assert_eq!(pair, rig.current.latest_record().unwrap().pair());
        }
        _ => panic!("genuine target pause observation"),
    }
    assert!(owner
        .observe_target_audio_end(rig.end.as_mut().unwrap(), &rig.current)
        .unwrap()
        .is_none());
}

#[test]
fn gameplay_target_owner_publication_waits_for_retained_tail_and_keeps_original_source_schedule() {
    let mut rig = Rig::new(true);
    let song = rig.config.song_origin;
    let mut owner = gameplay_owner(&mut rig);
    owner.observe_target_native(&mut rig.current).unwrap();
    let phase = owner
        .current()
        .unwrap()
        .owner
        .as_ref()
        .unwrap()
        .converter_owner()
        .source_position();
    let before = rig.state();
    queue_target(&mut owner, Mode::Normal);
    // Give the replacement a genuine retained held block to drain first.
    let current = owner.current_mut().unwrap().owner.as_mut().unwrap();
    current.render_held_pending(3).unwrap();
    current.admit(1).unwrap();
    assert!(!owner
        .publish_paused_target_audio(rig.context(), now())
        .unwrap());
    assert!(owner.output_clock_suspended());
    assert_eq!(rig.state(), before);
    owner.observe_target_native(&mut rig.current).unwrap();
    assert_eq!(rig.state(), before);
    assert!(!owner
        .publish_paused_target_audio(rig.context(), now())
        .unwrap());
    assert!(owner
        .publish_paused_target_audio(rig.context(), now())
        .unwrap());
    assert_eq!(owner.state(), ReplacementState::Attached);
    assert!(!owner.has_work());
    assert!(!owner.output_clock_suspended());
    assert_eq!(owner.last_issued_epoch(), 8);
    assert_eq!(rig.current.authority().epoch().id, 8);
    assert_eq!(rig.config.sample_rate, 24_000);
    assert_eq!(rig.config.song_origin, song);
    assert!(rig.config.logical_schedule);
    assert_eq!(owner.current().unwrap().basis.sample_rate(), 32_000);
    assert_eq!(
        owner
            .current()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position(),
        phase
    );
    assert_eq!(owner.target_boundary_facts().unwrap().source_rate, 24_000);
    assert_eq!(owner.converted_report().unwrap().target_rate, 32_000);
    assert!(rig.runtime.hold_audio_pause().is_ok());
    let source = owner.render_report().unwrap();
    assert_eq!(
        source,
        owner
            .current()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .last_real_source_report()
            .unwrap()
    );
    owner.observe_target_native(&mut rig.current).unwrap();
    assert_eq!(owner.render_report(), Some(source));
    assert_eq!(
        owner
            .current()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .mixer()
            .playback_frame_cursor(),
        2
    );
    rig.runtime.request_audio_pause(false);
    owner.set_target_held(false).unwrap();
    let native = owner.current_mut().unwrap();
    let actual = native.owner.as_mut().unwrap().render_pending(2).unwrap();
    native.owner.as_mut().unwrap().admit(2).unwrap();
    native.last = Some(actual);
    native.presented += 2;
    native.mode = Mode::Quiet;
    owner.observe_target_native(&mut rig.current).unwrap();
    assert!(!owner.render_report().unwrap().paused);
    assert_eq!(
        owner.render_report(),
        actual.source.filter(|report| report.frames != 0)
    );
    let commands = vec![
        AudioCommand::Play {
            voice: VoiceId(10),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_000_000),
            gain: 0.5,
        },
        AudioCommand::Play {
            voice: VoiceId(20),
            sample: SampleId(1),
            at: Timestamp::from_nanos(2_000_000),
            gain: -0.5,
        },
    ];
    let mut bgm = crate::bgm::BgmFeeder::new(
        commands.clone(),
        crate::bgm::BgmConfig {
            output_origin: point(2, 0),
            sample_rate: rig.config.sample_rate,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(10_000_000),
            max_pending: 4,
        },
    )
    .unwrap();
    let mut fifo = Vec::new();
    crate::native_audio::feed_rendered(&mut bgm, owner.render_report(), |command| {
        fifo.push(command);
        rig.runtime.enqueue_audio(command)
    })
    .unwrap();
    assert_eq!(fifo, commands);
}

#[test]
fn gameplay_target_owner_refused_ready_retains_hold_until_explicit_cancel_and_retirement_retry() {
    let mut rig = Rig::new(false);
    let mut owner = gameplay_owner(&mut rig);
    owner.observe_target_native(&mut rig.current).unwrap();
    let caches = owner_caches(&owner);
    queue_target(&mut owner, Mode::RetireFail);
    assert!(!owner
        .publish_paused_target_audio(rig.context(), now())
        .unwrap());
    rig.config.logical_schedule = false;
    let before = rig.state();
    assert!(owner
        .publish_paused_target_audio(rig.context(), now())
        .is_err());
    assert_eq!(rig.state(), before);
    assert_eq!(owner_caches(&owner), caches);
    assert!(owner.current().is_none());
    assert!(owner.replacement_pending());
    let ready = owner.rejected_target_audio_ready().unwrap();
    let phase = ready
        .output
        .owner
        .as_ref()
        .unwrap()
        .converter_owner()
        .source_position();
    let basis = ready.output.owner.as_ref().unwrap().target_frame_basis();
    assert_eq!(ready.facts.source_rate, 24_000);
    assert!(rig.runtime.hold_audio_pause().is_err());
    let call_count = rig.calls.borrow().len();
    assert!(owner
        .publish_paused_target_audio(rig.context(), now())
        .is_err());
    assert_eq!(rig.calls.borrow().len(), call_count);
    assert!(owner
        .queue(
            Request {
                rate: 48_000,
                mode: Mode::Normal
            },
            100_000_000
        )
        .is_err());
    assert!(owner.cancel().is_err());
    assert_eq!(owner.state(), ReplacementState::PendingRetirement);
    assert!(owner.rejected_target_audio_ready().is_none());
    // Failed native retirement still owns a live output and the original hold.
    assert!(owner.output_clock_suspended());
    assert!(owner.replacement_pending());
    assert!(rig.runtime.hold_audio_pause().is_err());
    assert!(owner.retry_retirement().unwrap());
    assert!(rig.runtime.hold_audio_pause().is_ok());
    assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
    let recovered = owner.take_recovered_mixer().unwrap();
    assert_eq!(recovered.target_frame_basis(), basis);
    assert_eq!(recovered.converter_owner().source_position(), phase);
    assert!(recovered.mixer().pause_requested());
    assert_eq!(rig.state(), before);
    assert_eq!(owner_caches(&owner), caches);
}

#[test]
fn gameplay_target_owner_failed_open_recovers_complete_owner_and_next_publication_uses_fresh_epoch()
{
    let mut rig = Rig::new(true);
    let mut owner = gameplay_owner(&mut rig);
    let retained = owner
        .current()
        .unwrap()
        .owner
        .as_ref()
        .unwrap()
        .pending_samples()
        .to_vec();
    let phase = owner
        .current()
        .unwrap()
        .owner
        .as_ref()
        .unwrap()
        .converter_owner()
        .source_position();
    let before = rig.state();
    queue_target(&mut owner, Mode::OpenFail);
    assert!(owner
        .publish_paused_target_audio(rig.context(), now())
        .is_err());
    assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
    assert_eq!(owner.last_issued_epoch(), 8);
    assert_eq!(rig.state(), before);
    assert!(rig.runtime.hold_audio_pause().is_ok());
    queue_target(&mut owner, Mode::Normal);
    let mut published = false;
    for _ in 0..4 {
        if owner
            .publish_paused_target_audio(rig.context(), now())
            .unwrap()
        {
            published = true;
            break;
        }
    }
    assert!(published);
    assert_eq!(owner.last_issued_epoch(), 9);
    assert_eq!(rig.current.authority().epoch().id, 9);
    assert_eq!(
        owner
            .current()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position(),
        phase
    );
    assert!(!retained.is_empty());
    assert_eq!(
        owner
            .current()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .mixer()
            .playback_frame_cursor(),
        2
    );
}

#[test]
fn gameplay_target_owner_waiting_cancel_reuses_only_committed_pause_and_recovers_exact_pending_pcm()
{
    let mut rig = Rig::new(false);
    let mut owner = gameplay_owner(&mut rig);
    owner.observe_target_native(&mut rig.current).unwrap();
    let evidence = owner
        .audio_pause_observation_target(&rig.current, now())
        .unwrap();
    let native = owner.current_mut().unwrap().owner.as_mut().unwrap();
    native.render_held_pending(3).unwrap();
    native.admit(1).unwrap();
    let retained = native.pending_samples().to_vec();
    let phase = native.converter_owner().source_position();
    let before = rig.state();
    let caches = owner_caches(&owner);
    // Retained PCM keeps the physical rate at which it was generated.
    assert!(owner
        .queue(
            Request {
                rate: 48_000,
                mode: Mode::Quiet,
            },
            100_000_000,
        )
        .is_ok());
    assert!(!owner
        .publish_paused_target_audio(rig.context(), now())
        .unwrap());
    assert!(owner.output_clock_suspended());
    assert!(owner.current().is_none());
    assert_eq!(
        format!(
            "{:?}",
            owner
                .audio_pause_observation_target(&rig.current, now())
                .unwrap()
        ),
        format!("{:?}", evidence)
    );
    owner.observe_target_native(&mut rig.current).unwrap();
    assert_eq!(rig.state(), before);
    assert_eq!(owner_caches(&owner), caches);
    assert!(rig.runtime.hold_audio_pause().is_err());
    assert!(owner.cancel().unwrap());
    assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
    let recovered = owner.take_recovered_mixer().unwrap();
    assert_eq!(recovered.pending_samples(), retained);
    assert!(recovered.pending_is_held());
    assert_eq!(recovered.converter_owner().source_position(), phase);
    assert_eq!(recovered.mixer().playback_frame_cursor(), 2);
    assert_eq!(rig.state(), before);
    assert!(rig.runtime.hold_audio_pause().is_ok());
}

// Bounded UI protocol joined to the real converted owner and target authority.
use crate::gameplay::output::{
    application::requests::GameplayOutputUi,
    domain::control::{OutputCapability, OutputControls, OutputReply, OutputRequest},
    ports::OutputUiPort,
};
use std::io;

struct TargetUiState {
    controls: OutputControls,
    blocked: bool,
    takes: usize,
    attempts: Vec<OutputReply>,
}
struct TargetUiPort(Rc<RefCell<TargetUiState>>);
impl OutputUiPort for TargetUiPort {
    fn advertise(&mut self, capability: Option<OutputCapability>) -> io::Result<()> {
        self.0
            .borrow_mut()
            .controls
            .advertise(capability)
            .map_err(io::Error::other)
    }
    fn take_request(&mut self) -> io::Result<Option<OutputRequest>> {
        let mut state = self.0.borrow_mut();
        state.takes += 1;
        Ok(state.controls.take_request())
    }
    fn reply(&mut self, reply: &OutputReply) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.attempts.push(reply.clone());
        if state.blocked {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        state.controls.reply(reply).map_err(io::Error::other)
    }
    fn pending(&self) -> bool {
        self.0.borrow().controls.pending()
    }
}
fn target_ui_capability() -> OutputCapability {
    OutputCapability {
        host: crate::settings::SettingsHost::Linux,
        current_args: vec![
            "--rate".into(),
            "32000".into(),
            "--period-frames".into(),
            "32".into(),
            "--buffer-frames".into(),
            "128".into(),
        ],
    }
}
fn target_ui_args() -> Vec<String> {
    vec![
        "--rate".into(),
        "32000".into(),
        "--period-frames".into(),
        "16".into(),
    ]
}
fn target_ui() -> (GameplayOutputUi<TargetUiPort>, Rc<RefCell<TargetUiState>>) {
    let state = Rc::new(RefCell::new(TargetUiState {
        controls: OutputControls::new(),
        blocked: false,
        takes: 0,
        attempts: Vec::new(),
    }));
    let mut ui = GameplayOutputUi::new(TargetUiPort(state.clone()));
    ui.advertise(Some(target_ui_capability())).unwrap();
    (ui, state)
}
fn target_ui_applied(output: &Output) -> Result<OutputCapability, String> {
    assert_eq!(output.basis.sample_rate(), 32_000);
    assert!(output.started && output.held);
    assert_eq!(
        output
            .owner
            .as_ref()
            .unwrap()
            .mixer()
            .config()
            .format()
            .sample_rate(),
        24_000
    );
    assert_eq!(
        output
            .owner
            .as_ref()
            .unwrap()
            .mixer()
            .playback_frame_cursor(),
        2
    );
    Ok(target_ui_capability())
}

#[test]
fn converted_ui_correlates_one_flight_through_retained_tail_native_publication_and_blocked_reply() {
    let mut rig = Rig::new(true);
    let mut owner = gameplay_owner(&mut rig);
    let phase = owner
        .current()
        .unwrap()
        .owner
        .as_ref()
        .unwrap()
        .converter_owner()
        .source_position();
    let before = rig.state();
    let (mut ui, state) = target_ui();
    let id = state
        .borrow_mut()
        .controls
        .request(target_ui_args())
        .unwrap();
    let mut mappings = 0;
    let mut map = |_: &OutputRequest, _: &Output| {
        mappings += 1;
        Ok(Request {
            rate: 32_000,
            mode: Mode::Normal,
        })
    };
    let mut applied = target_ui_applied;
    assert!(!ui
        .service_target_audio(&mut owner, rig.context(), now(), &mut map, &mut applied)
        .unwrap());
    assert!(ui.pending());
    assert!(owner.output_clock_suspended());
    assert_eq!(rig.state(), before);
    assert!(rig.runtime.hold_audio_pause().is_err());
    assert!(state
        .borrow_mut()
        .controls
        .request(target_ui_args())
        .is_err());
    assert!(!ui
        .service_target_audio(&mut owner, rig.context(), now(), &mut map, &mut applied)
        .unwrap());
    assert_eq!(rig.state(), before);
    state.borrow_mut().blocked = true;
    assert!(ui
        .service_target_audio(&mut owner, rig.context(), now(), &mut map, &mut applied)
        .unwrap());
    assert_eq!(rig.current.authority().epoch().id, 8);
    assert_eq!(rig.current.target_basis().unwrap().sample_rate(), 32_000);
    assert_eq!(rig.config.sample_rate, 24_000);
    assert_eq!(
        owner
            .current()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position(),
        phase
    );
    assert_eq!(owner.target_boundary_facts().unwrap().source_rate, 24_000);
    assert!(rig.runtime.hold_audio_pause().is_ok());
    assert!(ui.pending());
    let calls = rig.calls.borrow().clone();
    assert!(!ui
        .service_target_audio(&mut owner, rig.context(), now(), &mut map, &mut applied)
        .unwrap());
    assert_eq!(*rig.calls.borrow(), calls);
    assert_eq!(state.borrow().takes, 1);
    state.borrow_mut().blocked = false;
    assert!(!ui
        .service_target_audio(&mut owner, rig.context(), now(), &mut map, &mut applied)
        .unwrap());
    drop(map);
    assert_eq!(mappings, 1);
    assert!(ui.pending()); // The bounded port still owns the unread terminal reply.
    let mut state = state.borrow_mut();
    assert!(state.attempts.len() >= 3);
    assert!(state
        .attempts
        .iter()
        .all(|reply| reply.id == id && reply.result.is_ok()));
    let reply = state.controls.take_reply().unwrap();
    assert_eq!(reply.id, id);
    assert!(reply.result.is_ok());
    drop(state);
    assert!(!ui.pending());
}

#[test]
fn converted_ui_mapping_refusal_is_atomic_and_reply_backpressure_prevents_second_admission() {
    let mut rig = Rig::new(true);
    let mut owner = gameplay_owner(&mut rig);
    let before = rig.state();
    let caches = owner_caches(&owner);
    let original = owner.current().unwrap() as *const Output;
    let tail = owner
        .current()
        .unwrap()
        .owner
        .as_ref()
        .unwrap()
        .pending_samples()
        .to_vec();
    let (mut ui, state) = target_ui();
    let id = state
        .borrow_mut()
        .controls
        .request(target_ui_args())
        .unwrap();
    state.borrow_mut().blocked = true;
    let mut map = |_: &OutputRequest, _: &Output| -> Result<Request, String> {
        Err("incompatible retained target PCM".into())
    };
    let mut applied = |_: &Output| -> Result<OutputCapability, String> {
        panic!("refused mapping cannot publish")
    };
    for _ in 0..2 {
        assert!(!ui
            .service_target_audio(&mut owner, rig.context(), now(), &mut map, &mut applied)
            .unwrap());
        assert_eq!(rig.state(), before);
        assert_eq!(owner_caches(&owner), caches);
        assert_eq!(owner.current().unwrap() as *const Output, original);
        assert_eq!(
            owner
                .current()
                .unwrap()
                .owner
                .as_ref()
                .unwrap()
                .pending_samples(),
            tail
        );
        assert!(rig.calls.borrow().is_empty());
        assert!(!owner.replacement_pending());
    }
    assert_eq!(state.borrow().takes, 1);
    assert!(state
        .borrow_mut()
        .controls
        .request(target_ui_args())
        .is_err());
    state.borrow_mut().blocked = false;
    assert!(!ui
        .service_target_audio(&mut owner, rig.context(), now(), &mut map, &mut applied)
        .unwrap());
    let reply = state.borrow_mut().controls.take_reply().unwrap();
    assert_eq!(reply.id, id);
    assert_eq!(
        reply.result.unwrap_err(),
        "incompatible retained target PCM"
    );
    assert!(!ui.pending());
}

#[test]
fn converted_ui_native_failure_keeps_complete_owner_and_correlated_error_until_consumed() {
    for mode in [Mode::OpenFail, Mode::ObserveFail] {
        let mut rig = Rig::new(true);
        let mut owner = gameplay_owner(&mut rig);
        let phase = owner
            .current()
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position();
        let before = rig.state();
        let (mut ui, state) = target_ui();
        let id = state
            .borrow_mut()
            .controls
            .request(target_ui_args())
            .unwrap();
        state.borrow_mut().blocked = true;
        let mut map = |_: &OutputRequest, _: &Output| Ok(Request { rate: 32_000, mode });
        let mut applied = |_: &Output| -> Result<OutputCapability, String> {
            panic!("native failure cannot publish")
        };
        assert!(ui
            .service_target_audio(&mut owner, rig.context(), now(), &mut map, &mut applied)
            .is_err());
        assert_eq!(rig.state(), before);
        assert_eq!(owner.state(), ReplacementState::RecoveredMixer);
        assert!(ui.pending());
        assert!(rig.runtime.hold_audio_pause().is_ok());
        let recovered = owner.take_recovered_mixer().unwrap();
        assert_eq!(recovered.converter_owner().source_position(), phase);
        assert_eq!(recovered.mixer().playback_frame_cursor(), 2);
        assert!(recovered.mixer().pause_requested());
        if matches!(mode, Mode::OpenFail) {
            assert!(recovered.pending_is_held());
        }
        state.borrow_mut().blocked = false;
        assert!(!ui
            .service_target_audio(&mut owner, rig.context(), now(), &mut map, &mut applied)
            .unwrap());
        let reply = state.borrow_mut().controls.take_reply().unwrap();
        assert_eq!(reply.id, id);
        assert!(reply.result.is_err());
        assert!(!ui.pending());
    }
}
