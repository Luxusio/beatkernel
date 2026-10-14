//! Real retained Mixer/converter histories and original input/pause adoption.
//! Scripted native effects exercise policy; they are not physical device evidence.
use super::runtime::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    gameplay::output::application::replacement::{
        OutputReplacement, ReplacementFailure,
        coordinate_target::{
            CoordinateTargetPoll, ReadyCoordinateTargetAudioOutput,
            ReadyCoordinateTargetAudioPublicationFailure,
            publish_ready_coordinate_target_audio_output_held,
        },
    },
    gameplay::output::ports::{
        ConditionalTargetRetirementNativeOutputBackend, NativeTargetPlaybackFacts,
        NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
        OriginalCoordinateTargetNativeOutputBackend, OriginalTargetNativeOutputBackend,
        OutputReplacementBackend, SourcePlaybackIdentity, TargetOutputTelemetry,
        TargetPlaybackNativeOutputBackend, TargetRetirementStatus,
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayPauseControl},
    local_input::InputMerger,
    local_runtime::SoloRuntime,
    native_audio_presentation::{NativeAudioPresentation, TargetNativeAudioSnapshot},
    native_audio_startup::new_coordinate_audio_presentation,
    native_end::NativeEnd,
    native_gameplay::NativeGameplayConfig,
    native_output_coordinate::{CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate},
    native_playback_phase::ConvertedPlaybackFacts,
    playback_pause::{NativePause, PausePhase},
};
use beatkernel::{
    audio::*,
    input::*,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    runtime::RuntimeProcessingClock,
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{
    ConvertedBoundaryFacts, ConvertedNativeOutputState, DeviceFormat, SampleEncoding,
    presentation::validation::OriginalNativePresentationEvidence,
};
use std::{cell::RefCell, collections::VecDeque, io, rc::Rc};

pub(super) const OLD_EPOCH: u64 = 7;
pub(super) const SOURCE_RATE: u32 = 24_000;
pub(super) const TARGET_RATE: u32 = 48_000;
pub(super) const HOST_START: i64 = 1_000_000_000;
pub(super) fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
pub(super) fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
pub(super) fn now() -> ClockPoint {
    host(HOST_START + 10_000_000)
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
pub(super) fn snapshot(
    epoch: u64,
    basis: TargetFrameBasis,
    frame: u64,
    ns: i64,
) -> CoordinateTargetNativeAudioSnapshot {
    CoordinateTargetNativeAudioSnapshot::new(
        TargetNativeAudioSnapshot {
            epoch,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(frame).unwrap(),
                target: host(ns),
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}
pub(super) fn physical(sequence: u64, ns: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(ns), sequence),
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    None,
    Submit,
    Retire,
    Recover,
    Plan,
    Open,
    PartialOpen,
    Held,
    Start,
    Observe,
    Missing,
    Duplicate,
    Future,
    WrongDomain,
    WrongEpoch,
    ChangedBasis,
    ChangedAfterRead,
    RunningSource,
    ChangedBoundary,
    ActivePhase,
    Unsubmitted,
    InvalidPacket,
}
#[derive(Clone)]
pub(crate) struct Request {
    pub(super) format: DeviceFormat,
    pub(super) matrix: ChannelMatrix,
    pub(super) period: u32,
    pub(super) buffer: u32,
    pub(super) fault: Fault,
}
impl Request {
    pub(crate) fn normal(rate: u32) -> Self {
        Self {
            format: DeviceFormat::new(rate, 1, SampleEncoding::Float32, None).unwrap(),
            matrix: ChannelMatrix::default_mix(1, 1).unwrap(),
            period: 32,
            buffer: 128,
            fault: Fault::None,
        }
    }
    pub(crate) fn configured(
        format: DeviceFormat,
        matrix: ChannelMatrix,
        period: u32,
        buffer: u32,
    ) -> Self {
        Self {
            format,
            matrix,
            period,
            buffer,
            fault: Fault::None,
        }
    }
    pub(crate) fn format(&self) -> DeviceFormat {
        self.format
    }
    pub(crate) fn matrix(&self) -> &ChannelMatrix {
        &self.matrix
    }
    pub(crate) fn period(&self) -> u32 {
        self.period
    }
    pub(crate) fn buffer(&self) -> u32 {
        self.buffer
    }
    pub(crate) fn with_fault(mut self, fault: Fault) -> Self {
        self.fault = fault;
        self
    }
}
#[derive(Default)]
pub(super) struct Script {
    pub(super) calls: Vec<&'static str>,
    pub(super) statuses: VecDeque<TargetRetirementStatus>,
    pub(super) cancel_status: Option<TargetRetirementStatus>,
    pub(super) render_frames: VecDeque<usize>,
    serial: u64,
    faults: VecDeque<Fault>,
    pub(super) dropped: Vec<u64>,
    pub(super) consumed_tickets: Vec<u64>,
}
pub(crate) struct Ticket {
    native_id: u64,
    serial: u64,
}
// Evidence is intentionally non-Clone and non-Copy; original packets stay owned.
pub(crate) struct Evidence {
    pub(super) original_native_words: [u64; 2],
}
pub(crate) struct Output {
    pub(super) state: Option<ConvertedNativeOutputState>,
    pub(super) basis: TargetFrameBasis,
    pub(super) epoch: u64,
    pub(super) native_id: u64,
    started: bool,
    pub(super) retired: bool,
    held: bool,
    pub(super) request: Request,
    presented: u64,
    pub(super) observations: usize,
    last: Option<ConvertedRenderReport>,
    script: Rc<RefCell<Script>>,
}
impl Drop for Output {
    fn drop(&mut self) {
        self.script.borrow_mut().dropped.push(self.native_id);
    }
}
impl StoppedMixerSource<ConvertedNativeOutputState> for Output {
    type Error = io::Error;
    fn take_stopped_mixer(&mut self) -> Result<Option<ConvertedNativeOutputState>, io::Error> {
        self.script.borrow_mut().calls.push("recover");
        if self.request.fault == Fault::Recover {
            self.request.fault = Fault::None;
            return Err(io::Error::other("original recovery failure"));
        }
        if !self.retired {
            return Err(io::Error::other("same native owner remains live"));
        }
        Ok(self.state.take())
    }
}
pub(crate) struct Effects {
    script: Rc<RefCell<Script>>,
}
impl Effects {
    pub(crate) fn calls(&self) -> Vec<&'static str> {
        self.script.borrow().calls.clone()
    }
    pub(crate) fn conditional_statuses(
        &mut self,
        statuses: impl IntoIterator<Item = TargetRetirementStatus>,
    ) {
        self.script.borrow_mut().statuses.extend(statuses);
    }
    pub(crate) fn observation_faults(&mut self, faults: impl IntoIterator<Item = Fault>) {
        self.script.borrow_mut().faults.extend(faults);
    }
}
impl Output {
    pub(crate) fn format(&self) -> DeviceFormat {
        self.state.as_ref().unwrap().format()
    }
    pub(crate) fn applied_request(&self) -> &Request {
        &self.request
    }
    pub(crate) fn creation_epoch(&self) -> u64 {
        self.epoch
    }
    pub(crate) fn source_state(&self) -> &ConvertedNativeOutputState {
        self.state.as_ref().unwrap()
    }
}
pub(super) fn native(
    state: ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    epoch: u64,
    request: Request,
    script: &Rc<RefCell<Script>>,
) -> Output {
    let native_id = {
        let mut s = script.borrow_mut();
        s.serial += 1;
        s.serial
    };
    Output {
        state: Some(state),
        basis,
        epoch,
        native_id,
        started: false,
        retired: false,
        held: false,
        request,
        presented: 0,
        observations: 0,
        last: None,
        script: script.clone(),
    }
}
impl OutputReplacementBackend<ConvertedNativeOutputState, TargetFrameBasis> for Effects {
    type Presentation = beatkernel::time::presentation::PresentationEstimator;
    type Output = Output;
    type Request = Request;
    type Error = io::Error;
    fn open(
        &mut self,
        request: Request,
        mut state: ConvertedNativeOutputState,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<io::Error, Output, ConvertedNativeOutputState>> {
        self.script.borrow_mut().calls.push("open");
        if request.fault == Fault::Open {
            return Err(OutputOpenFailure::recovered_state(
                io::Error::other("original open failure"),
                Some(state),
            ));
        }
        if request.fault == Fault::PartialOpen {
            let basis = state.target_frame_basis();
            return Err(OutputOpenFailure::pending_state(
                io::Error::other("original partial open failure"),
                native(state, basis, epoch, request, &self.script),
            )
            .with_cleanup_error(io::Error::other("original partial open cleanup")));
        }
        if let Err(error) = state.reconfigure(
            request.format,
            request.matrix.clone(),
            request.buffer as usize,
        ) {
            return Err(OutputOpenFailure::recovered_state(
                io::Error::other(error.to_string()),
                Some(state),
            ));
        }
        let basis = state.target_frame_basis();
        Ok(native(state, basis, epoch, request, &self.script))
    }
    fn retire(&mut self, output: &mut Output) -> Result<(), io::Error> {
        self.script.borrow_mut().calls.push("retire");
        if output.request.fault == Fault::Retire {
            output.request.fault = Fault::None;
            return Err(io::Error::other("original retirement failure"));
        }
        output.retired = true;
        Ok(())
    }
    fn prepare_replacement_start(&mut self, output: &mut Output) -> Result<(), io::Error> {
        self.set_held(output, true)
    }
    fn start(&mut self, output: &mut Output) -> Result<(), io::Error> {
        self.script.borrow_mut().calls.push("start");
        assert!(
            output.held,
            "held generation is configured before native start"
        );
        if output.request.fault == Fault::Start {
            return Err(io::Error::other("original start failure"));
        }
        output.started = true;
        Ok(())
    }
    fn epoch(&self, output: &Output) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Output) -> TargetFrameBasis {
        output.basis
    }
    fn observe(&mut self, _: &mut Output, _: &mut Self::Presentation) -> Result<(), io::Error> {
        panic!("retained original coordinates must never use scalar estimator")
    }
    fn render_report(&self, _: &Output) -> Result<Option<RenderReport>, io::Error> {
        panic!("independent source reads cannot replace the coherent packet")
    }
}
impl OriginalTargetNativeOutputBackend<ConvertedNativeOutputState> for Effects {
    fn planned_target_basis(
        &self,
        request: &Request,
        state: &ConvertedNativeOutputState,
    ) -> Result<TargetFrameBasis, io::Error> {
        self.script.borrow_mut().calls.push("plan");
        if request.fault == Fault::Plan {
            return Err(io::Error::other("original basis planning failure"));
        }
        state
            .validate_reconfigure(request.format, &request.matrix, request.buffer as usize)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let old = state.target_frame_basis();
        TargetFrameBasis::new(old.origin(), old.start_time(), request.format.sample_rate())
            .map_err(|e| io::Error::other(e.to_string()))
    }
    fn observe_native_target(
        &mut self,
        _: &mut Output,
    ) -> Result<Option<TargetNativeAudioSnapshot>, io::Error> {
        panic!("independent original target reads cannot replace the coherent packet")
    }
    fn converted_report(&self, _: &Output) -> Result<Option<ConvertedRenderReport>, io::Error> {
        panic!("independent converted reads cannot replace the coherent packet")
    }
    fn boundary_facts(&self, _: &Output) -> Result<ConvertedBoundaryFacts, io::Error> {
        panic!("independent boundary reads cannot replace the coherent packet")
    }
    fn output_telemetry(&self, _: &Output) -> Result<Option<TargetOutputTelemetry>, io::Error> {
        panic!("independent telemetry cannot certify an original native packet")
    }
    fn set_held(&mut self, output: &mut Output, held: bool) -> Result<(), io::Error> {
        self.script
            .borrow_mut()
            .calls
            .push(if held { "held" } else { "release" });
        if output.request.fault == Fault::Held {
            return Err(io::Error::other("original held configuration failure"));
        }
        output.held = held;
        Ok(())
    }
}
impl OriginalCoordinateTargetNativeOutputBackend<ConvertedNativeOutputState> for Effects {
    fn observe_native_target_coordinate(
        &mut self,
        _: &mut Output,
    ) -> Result<Option<CoordinateTargetNativeAudioSnapshot>, io::Error> {
        panic!("standalone coordinate reads cannot certify coherent submitted playback")
    }
}
fn packet(
    output: &Output,
    identity: SourcePlaybackIdentity,
    frame: u64,
    at: i64,
    report: ConvertedRenderReport,
    fault: Fault,
) -> Result<NativeTargetPlaybackObservation<Evidence>, io::Error> {
    let state = output.state.as_ref().unwrap();
    let history = state.phase_history().unwrap();
    let source_history = state.converter_owner().source_playback_history().unwrap();
    let mut source = state.last_real_source_report();
    let mut boundaries = state.boundaries();
    if fault == Fault::RunningSource {
        source.as_mut().unwrap().paused = false;
    }
    if fault == Fault::ChangedBoundary {
        boundaries.pause.as_mut().unwrap().source_frame += 1;
    }
    let phase_index = if fault == Fault::ActivePhase {
        0
    } else {
        history.len() - 1
    };
    let mut facts = NativeTargetPlaybackFacts {
        playback: ConvertedPlaybackFacts {
            phase: history.span(phase_index).unwrap(),
            segment_end: history.end_time(),
            generated_target_end: history.end_time(),
            submitted_target_end: state.admitted_target_time().unwrap(),
            playback: source_history.span(source_history.len() - 1).unwrap(),
            pulled_source_end: state.mixer().frame_cursor(),
            revision: history.retirement_revision(),
        },
        source,
        converted: Some(report),
        boundaries,
        revision: output.observations as u64 * 2,
    };
    if fault == Fault::Unsubmitted {
        facts.playback.submitted_target_end = output.basis.start_time();
    }
    if fault == Fault::InvalidPacket {
        facts.revision |= 1;
    }
    let mut coordinate = snapshot(output.epoch, output.basis, frame, at);
    if fault == Fault::WrongDomain {
        let mut raw = coordinate.snapshot();
        let OriginalNativePresentationEvidence::SuppliedPair(mut pair) = raw.evidence else {
            unreachable!()
        };
        pair.target.domain = ClockDomainId(77);
        raw.evidence = OriginalNativePresentationEvidence::SuppliedPair(pair);
        coordinate =
            CoordinateTargetNativeAudioSnapshot::new(raw, coordinate.coordinate()).unwrap();
    }
    if fault == Fault::WrongEpoch {
        coordinate = snapshot(output.epoch + 1, output.basis, frame, at);
    }
    let query = exact(
        output
            .basis
            .start_time()
            .checked_add_frames(frame, output.basis.sample_rate())
            .unwrap(),
    );
    let playback = NativeTargetPlaybackQuery::new(
        output.epoch,
        output.basis,
        identity,
        query,
        facts,
        Some(Evidence {
            original_native_words: [output.native_id, output.presented],
        }),
    )
    .map_err(|e| io::Error::other(e.to_string()))?;
    NativeTargetPlaybackObservation::new(coordinate, playback)
        .map_err(|e| io::Error::other(e.to_string()))
}
impl TargetPlaybackNativeOutputBackend<ConvertedNativeOutputState> for Effects {
    type NativeEvidence = Evidence;
    fn observe_native_target_playback(
        &mut self,
        output: &mut Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackObservation<Evidence>>, io::Error> {
        self.script.borrow_mut().calls.push("packet");
        let fault = self
            .script
            .borrow_mut()
            .faults
            .pop_front()
            .unwrap_or(output.request.fault);
        if fault == Fault::Observe {
            return Err(io::Error::other("original native observation failure"));
        }
        if fault == Fault::Missing {
            return Ok(None);
        }
        assert!(output.started && output.held && !output.retired);
        output.observations += 1;
        let state = output.state.as_mut().unwrap();
        let report = if state.pending_frames() != 0 {
            let report = state.pending_report().unwrap();
            let frames = state.pending_frames();
            state.admit(frames).unwrap();
            output.presented += frames as u64;
            report
        } else {
            let frames = self
                .script
                .borrow_mut()
                .render_frames
                .pop_front()
                .unwrap_or(2);
            let report = state.render_held_pending(frames).unwrap();
            if fault != Fault::Unsubmitted {
                state.admit(frames).unwrap();
            }
            output.presented += frames as u64;
            report
        };
        output.last = Some(report);
        let frame = if fault == Fault::Duplicate {
            2
        } else {
            output.presented
        };
        let at = if fault == Fault::Future {
            now().timestamp.as_nanos() + 1_000_000
        } else {
            HOST_START + 20_000 + output.observations as i64 * 1_000
        };
        if fault == Fault::ChangedBasis {
            output.basis =
                TargetFrameBasis::new(output.basis.origin(), output.basis.start_time(), 44_100)
                    .unwrap();
        }
        let result = packet(output, identity, frame, at, report, fault).map(Some);
        if fault == Fault::ChangedAfterRead {
            output.epoch += 1;
        }
        result
    }
    fn target_playback_at(
        &self,
        _: &Output,
        _: &InterpolatedTargetTime,
        _: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackQuery<Evidence>>, io::Error> {
        panic!("replacement must retain its exact accepted original packet")
    }
}
impl ConditionalTargetRetirementNativeOutputBackend<ConvertedNativeOutputState> for Effects {
    type Ticket = Ticket;
    fn submit_target_conditional_retirement(
        &mut self,
        output: &mut Output,
        request: &Request,
    ) -> Result<Option<Ticket>, io::Error> {
        self.script.borrow_mut().calls.push("submit");
        if request.fault == Fault::Submit
            || request.period == 0
            || request.buffer < request.period
            || request.matrix.target_channels() != request.format.channels()
        {
            return Err(io::Error::other("original cold request refusal"));
        }
        let serial = {
            let mut s = self.script.borrow_mut();
            s.serial += 1;
            s.serial
        };
        Ok(Some(Ticket {
            native_id: output.native_id,
            serial,
        }))
    }
    fn poll_target_conditional_retirement(
        &mut self,
        output: &mut Output,
        ticket: &Ticket,
    ) -> Result<TargetRetirementStatus, io::Error> {
        assert_eq!(
            output.native_id, ticket.native_id,
            "poll addresses the same original native owner"
        );
        let mut script = self.script.borrow_mut();
        script.calls.push("conditional-poll");
        let status = script
            .statuses
            .pop_front()
            .unwrap_or(TargetRetirementStatus::Authorized);
        if matches!(
            status,
            TargetRetirementStatus::Refused | TargetRetirementStatus::Cancelled
        ) {
            script.consumed_tickets.push(ticket.serial);
        }
        Ok(status)
    }
    fn cancel_target_conditional_retirement(
        &mut self,
        output: &mut Output,
        ticket: &Ticket,
    ) -> Result<TargetRetirementStatus, io::Error> {
        assert_eq!(
            output.native_id, ticket.native_id,
            "cancel addresses the same original native owner"
        );
        let mut script = self.script.borrow_mut();
        script.calls.push("conditional-cancel");
        let status = script
            .cancel_status
            .unwrap_or(TargetRetirementStatus::Cancelled);
        script.statuses.push_front(status);
        Ok(status)
    }
}
pub(super) type Controller =
    OutputReplacement<Effects, ConvertedNativeOutputState, TargetFrameBasis, Ticket>;
pub(super) type Ready = ReadyCoordinateTargetAudioOutput<Output, Evidence>;
pub(crate) struct Rig {
    pub(super) current: NativeAudioPresentation,
    pub(super) pause: NativePause,
    pub(super) inputs: InputMerger,
    pub(super) runtime: SoloRuntime,
    pub(super) practice: PracticeController,
    pub(super) pin: OutputFrameBasis,
    pub(super) identity: SourcePlaybackIdentity,
    pub(super) config: NativeGameplayConfig,
    pub(super) end: Option<NativeEnd>,
    pub(super) output: Option<Output>,
    pub(super) script: Rc<RefCell<Script>>,
}
impl Rig {
    pub(crate) fn new(partial: bool) -> Self {
        Self::new_at_epoch(partial, OLD_EPOCH)
    }
    pub(super) fn new_at_epoch(partial: bool, epoch: u64) -> Self {
        let format = AudioFormat::new(SOURCE_RATE, 1).unwrap();
        let pcm_limits = PcmLimits::new(4096, 4096, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25; 512], pcm_limits).unwrap(),
        )
        .unwrap();
        let program = PreparedPracticeProgram::new(
            &bank,
            vec![PracticeCue {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            }],
            PracticeLimits::new(1, 1, 64, 8, 64).unwrap(),
        )
        .unwrap();
        let (practice, endpoint) = practice_queue(&program).unwrap();
        let (mut producer, consumer) = command_queue(16).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(8_000_000), true)
                    .unwrap(),
            )
            .unwrap();
        let pin = mixer.output_frame_basis();
        let identity =
            SourcePlaybackIdentity::new(SOURCE_RATE, pin.start_physical_frame(), 0).unwrap();
        let mut state = ConvertedNativeOutputState::new(
            mixer,
            DeviceFormat::new(TARGET_RATE, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            128,
        )
        .unwrap_or_else(|_| panic!("actual retained converted owner"));
        state.enable_source_playback_history(64).unwrap();
        state.enable_phase_history(64).unwrap();
        let basis = state.target_frame_basis();
        let mut current = new_coordinate_audio_presentation(
            epoch,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            ClockDomainId(1),
            point(33, 5_000_000_000),
            AudioAuthorityConfig {
                history_capacity: 64,
                max_observation_age: Duration::from_nanos(100_000_000),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
        )
        .unwrap();
        current
            .admit_target_coordinate(snapshot(epoch, basis, 0, HOST_START))
            .unwrap();
        current.pin_practice_mixer_basis(pin).unwrap();
        let mut pause = NativePause::new(basis.origin(), ClockDomainId(1), SOURCE_RATE)
            .unwrap()
            .with_target_basis(epoch, basis)
            .unwrap()
            .with_coordinate_binding(
                epoch,
                AudioAuthorityCoordinateBasis::TargetFrame(basis),
                identity,
            )
            .unwrap();
        let active = state.render_pending(8).unwrap();
        assert!(active.source.unwrap().playback_frames > 0);
        assert!(state.pending_samples().iter().any(|sample| *sample == 0.25));
        state.admit(8).unwrap();
        current
            .admit_target_coordinate(snapshot(epoch, basis, 8, HOST_START + 1_000))
            .unwrap();
        assert!(pause.request_coordinate(true, current.authority()).unwrap());
        producer.request_pause(true);
        state.render_pending(32).unwrap();
        let adoption = pause
            .prepare_coordinate_adoption(state.last_real_source_report(), Some(state.boundaries()))
            .unwrap()
            .unwrap();
        current
            .admit_target_coordinate(snapshot(epoch, basis, 40, HOST_START + 5_000))
            .unwrap();
        let mut inputs =
            InputMerger::new_dynamic(ClockDomainId(1), host(HOST_START), 4, 16).unwrap();
        inputs.register_source(DeviceId(9)).unwrap();
        let original = physical(1, HOST_START + 1_000);
        inputs
            .admit(original.clone(), host(HOST_START + 5_000))
            .unwrap();
        current
            .authority_mut()
            .record_acquired_prefix(host(HOST_START + 5_000))
            .unwrap();
        let mapped = current
            .authority()
            .prepare_coordinate_input(host(HOST_START + 1_000), host(HOST_START + 5_000))
            .unwrap()
            .unwrap();
        assert_eq!(
            inputs.pop_ready(host(HOST_START + 5_000)).unwrap(),
            Some(original)
        );
        current
            .authority_mut()
            .commit_coordinate_input(mapped, host(HOST_START + 5_000))
            .unwrap();
        inputs.commit(host(HOST_START + 1_000)).unwrap();
        let cut_host = current
            .authority()
            .presented_coordinate_boundary_host(&adoption.exact_cut(), host(HOST_START + 5_000))
            .unwrap()
            .unwrap();
        let cutoff = current
            .authority()
            .prepare_coordinate_control_cutoff(
                epoch,
                adoption.exact_cut(),
                cut_host,
                host(HOST_START + 5_000),
                &inputs,
            )
            .unwrap()
            .unwrap();
        let acknowledged = pause
            .commit_coordinate_adoption(
                adoption,
                current.authority(),
                &cutoff,
                host(HOST_START + 5_000),
                &inputs,
            )
            .unwrap();
        assert!(acknowledged.paused);
        assert_eq!(acknowledged.source_identity, identity);
        current
            .authority_mut()
            .commit_coordinate_control_cutoff(cutoff, host(HOST_START + 5_000), &inputs)
            .unwrap();
        state.admit(32).unwrap();
        assert_eq!(pause.phase(), PausePhase::Paused);
        assert_ne!(
            state.mixer().output_frame_basis(),
            pin,
            "next render frame is not the original creation pin"
        );
        let parsed =
            beatkernel_bms::parse("#BPM 60\n#WAV01 key.wav\n#00011:01", Default::default())
                .unwrap();
        let judge = JudgeEngine::new(
            parsed.compile().unwrap().chart,
            parsed.rules(),
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
        let mut runtime = SoloRuntime::new(
            ClockDomainId(33),
            ClockDomainId(2),
            Transport::new(
                Timestamp::from_nanos(5_000_000_000),
                Timestamp::ZERO,
                Rate::NORMAL,
            ),
            BindingMap::from_bindings([Binding {
                device: DeviceSelector::Any,
                physical: PhysicalControlId::keyboard(7),
                game_control: GameControlId(0x11),
            }])
            .unwrap(),
            judge,
            producer,
            vec![],
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        runtime
            .transport_mut()
            .pause(Timestamp::from_nanos(5_000_000_000))
            .unwrap();
        if partial {
            state.render_held_pending(3).unwrap();
            state.admit(1).unwrap();
        }
        let script = Rc::new(RefCell::new(Script::default()));
        let output = Some(native(
            state,
            basis,
            epoch,
            Request::normal(TARGET_RATE),
            &script,
        ));
        Self {
            current,
            pause,
            inputs,
            runtime,
            practice,
            pin,
            identity,
            end: None,
            output,
            script,
            config: NativeGameplayConfig {
                origin: host(HOST_START),
                stream_origin: point(2, 0),
                playback_origin: point(2, 0),
                song_origin: Timestamp::from_nanos(5_000_000_000),
                sample_rate: SOURCE_RATE,
                end_song: None,
                advance_lag: Duration::ZERO,
                seconds: None,
                pause_supported: true,
                logical_schedule: true,
            },
        }
    }
    pub(super) fn controller(&mut self) -> Controller {
        let mut controller = Controller::new(Effects {
            script: self.script.clone(),
        });
        assert!(controller.attach(self.output.take().unwrap()).is_ok());
        controller
    }
    pub(crate) fn take_native_owner(&mut self) -> (Effects, Output) {
        (
            Effects {
                script: self.script.clone(),
            },
            self.output.take().unwrap(),
        )
    }
    pub(crate) fn context(&mut self) -> GameplayAudioOutputContext<'_> {
        GameplayAudioOutputContext {
            control: GameplayPauseControl::solo(&mut self.runtime),
            presentation: &mut self.current,
            merger: &self.inputs,
            pause: &mut self.pause,
            config: &mut self.config,
            end: &mut self.end,
        }
    }
    pub(crate) fn source_identity(&self) -> SourcePlaybackIdentity {
        self.identity
    }
    pub(crate) fn source_pin(&self) -> OutputFrameBasis {
        self.pin
    }
    pub(crate) fn practice_generation(&self) -> u64 {
        self.practice.generation()
    }
    pub(crate) fn presentation(&self) -> &NativeAudioPresentation {
        &self.current
    }
    pub(crate) fn paused(&self) -> &NativePause {
        &self.pause
    }
    pub(crate) fn native_config(&self) -> NativeGameplayConfig {
        self.config
    }
    pub(super) fn poll_failure(
        &self,
        controller: &mut Controller,
    ) -> ReplacementFailure<io::Error> {
        for _ in 0..8 {
            match self.poll(controller) {
                Err(failure) => return failure,
                Ok(CoordinateTargetPoll::Pending) => {}
                _ => panic!("scripted failure must precede Ready or live restoration"),
            }
        }
        panic!("scripted failure cannot remain pending indefinitely");
    }
    pub(super) fn poll_ready(&self, controller: &mut Controller) -> Ready {
        for _ in 0..8 {
            match self.poll(controller).unwrap() {
                CoordinateTargetPoll::Pending => {}
                CoordinateTargetPoll::Ready(ready) => return ready,
                CoordinateTargetPoll::LiveRefused(reason) => panic!("{reason}"),
            }
        }
        panic!("two genuine original observations cannot remain pending indefinitely");
    }
    pub(super) fn begin(
        &mut self,
        controller: &mut Controller,
        request: Request,
    ) -> Result<(), ReplacementFailure<io::Error>> {
        controller.begin_coordinate_target_audio(
            request,
            &self.current,
            &self.pause,
            self.end.as_ref(),
            &self.inputs,
            self.identity,
            false,
            100_000_000,
            || self.runtime.hold_audio_pause(),
        )
    }
    pub(super) fn poll(
        &self,
        controller: &mut Controller,
    ) -> Result<CoordinateTargetPoll<Output, Evidence>, ReplacementFailure<io::Error>> {
        controller.poll_coordinate_target_audio(now(), &self.current, &self.pause, &self.inputs)
    }
    pub(super) fn ready(&mut self, request: Request) -> (Controller, Ready) {
        let mut controller = self.controller();
        self.begin(&mut controller, request).unwrap();
        for _ in 0..8 {
            match self.poll(&mut controller).unwrap() {
                CoordinateTargetPoll::Pending => {}
                CoordinateTargetPoll::LiveRefused(reason) => {
                    panic!("unexpected live refusal: {reason}")
                }
                CoordinateTargetPoll::Ready(ready) => return (controller, ready),
            }
        }
        panic!("two original submitted held observations must make ready");
    }
    pub(super) fn publish<A>(
        &mut self,
        ready: Ready,
        stage: impl FnOnce(&Ready) -> Result<A, Box<dyn std::error::Error>>,
    ) -> Result<(PauseHold, A), ReadyCoordinateTargetAudioPublicationFailure<Output, Evidence>>
    {
        publish_ready_coordinate_target_audio_output_held(
            ready,
            &mut self.output,
            GameplayAudioOutputContext {
                control: GameplayPauseControl::solo(&mut self.runtime),
                presentation: &mut self.current,
                merger: &self.inputs,
                pause: &mut self.pause,
                config: &mut self.config,
                end: &mut self.end,
            },
            now(),
            stage,
        )
    }
}
pub(super) fn preserved_frontiers(p: &NativeAudioPresentation) -> String {
    let a = p.authority();
    format!(
        "{:?}",
        (
            a.acquired_prefix(),
            a.closed_host_prefix(),
            a.committed_input_host(),
            a.committed_operation(),
            a.committed_presentation(),
            a.committed_coordinate_operation(),
            a.committed_coordinate_presentation()
        )
    )
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SourceHistorySnapshot {
    source_rate: u32,
    initial_physical: u64,
    initial_playback: u64,
    pulled_end: u64,
    retained_start: u64,
    capacity: usize,
    retirement_revision: u64,
    spans: Vec<[u64; 5]>,
}
pub(super) fn source_history(state: &ConvertedNativeOutputState) -> SourceHistorySnapshot {
    let history = state.converter_owner().source_playback_history().unwrap();
    SourceHistorySnapshot {
        source_rate: history.source_rate(),
        initial_physical: history.initial_physical_frame(),
        initial_playback: history.initial_playback_frame(),
        pulled_end: history.pulled_end_frame(),
        retained_start: history.retained_start_frame(),
        capacity: history.capacity(),
        retirement_revision: history.retirement_revision(),
        spans: (0..history.len())
            .map(|index| {
                let span = history.span(index).unwrap();
                [
                    span.physical_start(),
                    u64::from(span.physical_end().is_some()),
                    span.physical_end().unwrap_or(0),
                    span.playback_start(),
                    match span.state() {
                        SourcePlaybackState::Running => 0,
                        SourcePlaybackState::Frozen => 1,
                    },
                ]
            })
            .collect(),
    }
}
pub(super) fn phase_words(state: &ConvertedNativeOutputState) -> Vec<[u64; 8]> {
    let h = state.phase_history().unwrap();
    (0..h.len())
        .map(|i| h.span(i).unwrap().to_words())
        .collect()
}
pub(super) fn pending(result: CoordinateTargetPoll<Output, Evidence>) {
    assert!(matches!(result, CoordinateTargetPoll::Pending));
}
