//! Actual retained Source owner, history, startup, pause and drained input prefix.
//! Only native effects are scripted; these fixtures make no hardware timing claim.
use super::runtime::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    gameplay::output::{
        application::replacement::{
            coordinate_source::{
                publish_ready_coordinate_source_audio_output_held, CoordinateSourcePoll,
                ReadyCoordinateSourceAudioOutput, ReadyCoordinateSourceAudioPublicationFailure,
            },
            OutputReplacement, ReplacementFailure,
        },
        ports::{
            ConditionalSourceRetirementNativeOutputBackend, NativePlaybackObservation,
            NativeSourcePlaybackQuery, OriginalCoordinateNativeOutputBackend,
            OriginalNativeOutputBackend, OutputChannelRemixBackend, OutputReplacementBackend,
            SourceHistoryRetirementNativeOutputBackend, SourcePlaybackFacts,
            SourcePlaybackIdentity, SourcePlaybackNativeOutputBackend, SourceRetirementStatus,
        },
    },
    gameplay_presentation::{GameplayAudioOutputContext, GameplayPauseControl},
    local_input::InputMerger,
    local_runtime::SoloRuntime,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_audio_startup::new_coordinate_audio_presentation,
    native_end::NativeEnd,
    native_gameplay::NativeGameplayConfig,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
    playback_pause::{NativePause, PausePhase},
};
use beatkernel::{
    audio::*,
    input::*,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    runtime::RuntimeProcessingClock,
    time::{
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, ExtrapolationPolicy,
        Timestamp,
    },
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{
    presentation::validation::OriginalNativePresentationEvidence, AudioClockReadingQuality,
    AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus, DeviceFormat, NativeOutputState,
    SampleEncoding, StreamCounters,
};
use std::{cell::RefCell, collections::VecDeque, io, rc::Rc};

pub(crate) const OLD_EPOCH: u64 = 7;
pub(crate) const SOURCE_RATE: u32 = 1000;
pub(crate) const OLD_FREQUENCY: u64 = 1_000_000;
pub(crate) const HOST_START: i64 = 1_000_000_000;
pub(crate) fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
pub(crate) fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
pub(crate) fn now() -> ClockPoint {
    host(HOST_START + 10_000_000)
}
pub(crate) fn descriptor(
    basis: OutputFrameBasis,
    frequency: Option<u64>,
) -> AudioAuthorityCoordinateBasis {
    match frequency {
        Some(frequency) => AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency },
        None => AudioAuthorityCoordinateBasis::SourceFrame(basis),
    }
}
pub(crate) fn source_basis(descriptor: AudioAuthorityCoordinateBasis) -> OutputFrameBasis {
    match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(basis)
        | AudioAuthorityCoordinateBasis::SourceCounter { basis, .. } => basis,
        _ => panic!("Source fixture requires a Source descriptor"),
    }
}
pub(crate) fn snapshot(
    epoch: u64,
    descriptor: AudioAuthorityCoordinateBasis,
    units: u64,
    ns: i64,
) -> CoordinateNativeAudioSnapshot {
    let basis = source_basis(descriptor);
    let (evidence, coordinate) = match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(_) => (
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(units).unwrap(),
                target: host(ns),
            }),
            NativeOutputCoordinate::StreamFrame { frame: units },
        ),
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => (
            OriginalNativePresentationEvidence::Wasapi {
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters::default(),
                    render: None,
                    clock: Some(AudioClockSnapshot {
                        position: units,
                        frequency,
                        qpc_100ns: units + 1,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(host(ns)),
                        mapping_quality: ClockMappingQuality::Unknown,
                    }),
                },
                basis: Some(basis),
            },
            NativeOutputCoordinate::Counter {
                position: units,
                frequency,
            },
        ),
        _ => unreachable!(),
    };
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch,
            basis,
            evidence,
        },
        coordinate,
    )
    .unwrap()
}
pub(crate) fn units_for_frames(descriptor: AudioAuthorityCoordinateBasis, frames: u64) -> u64 {
    match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(_) => frames,
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => {
            let ticks = u128::from(frames) * u128::from(frequency);
            assert_eq!(ticks % u128::from(SOURCE_RATE), 0);
            u64::try_from(ticks / u128::from(SOURCE_RATE)).unwrap()
        }
        _ => unreachable!(),
    }
}
pub(crate) fn physical(sequence: u64, ns: i64) -> PhysicalInputEvent {
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
    Busy,
    Retire,
    Recover,
    Open,
    PartialOpen,
    Start,
    Created,
    Observe,
    Missing,
    Duplicate,
    Future,
    Stale,
    WrongDomain,
    WrongEpoch,
    WrongFrequency,
    WrongIdentity,
    ChangedBasis,
    ChangedAfterRead,
    RunningSource,
    WrongFrozen,
    MissingRender,
    ActiveRender,
    Unsubmitted,
    TornRevision,
    RepeatedRevision,
    OldHeldOnly,
}
#[derive(Clone)]
pub(crate) struct Request {
    pub(crate) format: DeviceFormat,
    pub(crate) matrix: Option<ChannelMatrix>,
    pub(crate) period: u32,
    pub(crate) buffer: u32,
    pub(crate) frequency: Option<u64>,
    pub(crate) fault: Fault,
}
impl Request {
    pub(crate) fn normal(frequency: Option<u64>) -> Self {
        Self {
            format: DeviceFormat::new(SOURCE_RATE, 1, SampleEncoding::Float32, None).unwrap(),
            matrix: None,
            period: 8,
            buffer: 128,
            frequency,
            fault: Fault::None,
        }
    }
    pub(crate) fn configured(
        format: DeviceFormat,
        matrix: Option<ChannelMatrix>,
        period: u32,
        buffer: u32,
        frequency: Option<u64>,
    ) -> Self {
        Self {
            format,
            matrix,
            period,
            buffer,
            frequency,
            fault: Fault::None,
        }
    }
    pub(crate) fn format(&self) -> DeviceFormat {
        self.format
    }
    pub(crate) fn matrix(&self) -> Option<&ChannelMatrix> {
        self.matrix.as_ref()
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
pub(crate) struct Script {
    pub(crate) calls: Vec<&'static str>,
    pub(crate) statuses: VecDeque<SourceRetirementStatus>,
    pub(crate) cancel_status: Option<SourceRetirementStatus>,
    pub(crate) render_frames: VecDeque<usize>,
    pub(crate) native_units: VecDeque<u64>,
    pub(crate) faults: VecDeque<Fault>,
    pub(crate) probe_failures: VecDeque<bool>,
    pub(crate) retirement_failures: VecDeque<bool>,
    pub(crate) recovery_failures: VecDeque<bool>,
    pub(crate) dropped: Vec<u64>,
    pub(crate) consumed_tickets: Vec<u64>,
    pub(crate) native_observations: Vec<CoordinateNativeAudioSnapshot>,
    pub(crate) last_host: i64,
    serial: u64,
    pump: Option<PumpScript>,
    history: Option<HistoryScript>,
}
struct PumpScript {
    host: i64,
    frames: usize,
    admit_limit: usize,
    pcm: Option<Vec<(u64, Vec<f32>)>>,
}
#[derive(Clone, Copy)]
pub(crate) enum HistoryFault {
    None,
    Io,
    Authorize,
    PacketIdentity,
    PacketQuery,
}
#[derive(Default)]
struct HistoryScript {
    query_faults: VecDeque<HistoryFault>,
    request_faults: VecDeque<HistoryFault>,
    queries: Vec<[u64; 2]>,
    requests: Vec<(u64, OutputFrameBasis, SourcePlaybackIdentity, u64)>,
    revision: u64,
}
pub(crate) struct Ticket {
    native_id: u64,
    serial: u64,
}
// Non-Clone evidence checks that controllers retain the original compound packet.
pub(crate) struct Evidence {
    pub(crate) original_native_words: [u64; 2],
}
pub(crate) struct Output {
    pub(crate) state: Option<NativeOutputState>,
    pub(crate) basis: OutputFrameBasis,
    pub(crate) epoch: u64,
    pub(crate) native_id: u64,
    pub(crate) started: bool,
    pub(crate) retired: bool,
    pub(crate) request: Request,
    pub(crate) presented: u64,
    pub(crate) observations: usize,
    pub(crate) last: Option<RenderReport>,
    script: Rc<RefCell<Script>>,
}
impl Drop for Output {
    fn drop(&mut self) {
        self.script.borrow_mut().dropped.push(self.native_id);
    }
}
impl StoppedMixerSource<NativeOutputState> for Output {
    type Error = io::Error;
    fn take_stopped_mixer(&mut self) -> Result<Option<NativeOutputState>, io::Error> {
        self.script.borrow_mut().calls.push("recover");
        if self
            .script
            .borrow_mut()
            .recovery_failures
            .pop_front()
            .unwrap_or(false)
        {
            return Err(io::Error::other("scripted native recovery failure"));
        }
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
impl Output {
    pub(crate) fn source_state(&self) -> &NativeOutputState {
        self.state.as_ref().unwrap()
    }
    pub(crate) fn applied_request(&self) -> &Request {
        &self.request
    }
    pub(crate) fn creation_epoch(&self) -> u64 {
        self.epoch
    }
    pub(crate) fn format(&self) -> DeviceFormat {
        self.source_state().format().unwrap()
    }
    pub(crate) fn pump_partial_held(&mut self, frames: usize, admitted: usize) {
        assert!(self.source_state().mixer().is_paused());
        let state = self.state.as_mut().unwrap();
        self.last = Some(state.render_pending(frames).unwrap());
        state.admit(admitted).unwrap();
        self.presented += admitted as u64;
    }
}
pub(crate) fn native(
    state: NativeOutputState,
    epoch: u64,
    request: Request,
    script: &Rc<RefCell<Script>>,
) -> Output {
    let basis = state.output_frame_basis();
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
        request,
        presented: 0,
        observations: 0,
        last: None,
        script: script.clone(),
    }
}
#[derive(Clone)]
pub(crate) struct Effects {
    script: Rc<RefCell<Script>>,
}
impl Effects {
    pub(crate) fn for_active_pump(
        state: NativeOutputState,
        epoch: u64,
        request: Request,
        frames: usize,
        trace_pcm: bool,
    ) -> (Self, Output) {
        let script = Rc::new(RefCell::new(Script {
            pump: Some(PumpScript {
                host: HOST_START,
                frames,
                admit_limit: usize::MAX,
                pcm: trace_pcm.then(Vec::new),
            }),
            ..Default::default()
        }));
        let mut output = native(state, epoch, request, &script);
        output.started = true;
        (Self { script }, output)
    }
    pub(crate) fn calls(&self) -> Vec<&'static str> {
        self.script.borrow().calls.clone()
    }
    pub(crate) fn conditional_statuses(
        &mut self,
        statuses: impl IntoIterator<Item = SourceRetirementStatus>,
    ) {
        self.script.borrow_mut().statuses.extend(statuses);
    }
    pub(crate) fn observation_faults(&mut self, faults: impl IntoIterator<Item = Fault>) {
        self.script.borrow_mut().faults.extend(faults);
    }
    pub(crate) fn conditional_probe_failures(&mut self, values: impl IntoIterator<Item = bool>) {
        self.script.borrow_mut().probe_failures.extend(values);
    }
    pub(crate) fn retirement_failures(&mut self, values: impl IntoIterator<Item = bool>) {
        self.script.borrow_mut().retirement_failures.extend(values);
    }
    pub(crate) fn recovery_failures(&mut self, values: impl IntoIterator<Item = bool>) {
        self.script.borrow_mut().recovery_failures.extend(values);
    }
    pub(crate) fn pump_now(&self) -> ClockPoint {
        host(self.script.borrow().pump.as_ref().unwrap().host + 1000)
    }
    pub(crate) fn pump_wait(&self) {
        self.script.borrow_mut().pump.as_mut().unwrap().host += 100_000;
    }
    pub(crate) fn pump_admit_limit(&self, frames: usize) {
        assert!(frames > 0);
        self.script.borrow_mut().pump.as_mut().unwrap().admit_limit = frames;
    }
    pub(crate) fn pump_pcm(&self) -> Vec<(u64, Vec<f32>)> {
        self.script
            .borrow()
            .pump
            .as_ref()
            .unwrap()
            .pcm
            .clone()
            .unwrap()
    }
    pub(crate) fn enable_history_service(&mut self) {
        self.script.borrow_mut().history = Some(HistoryScript::default());
    }
    pub(crate) fn history_query_faults(&mut self, faults: impl IntoIterator<Item = HistoryFault>) {
        self.script
            .borrow_mut()
            .history
            .as_mut()
            .unwrap()
            .query_faults
            .extend(faults);
    }
    pub(crate) fn history_request_faults(
        &mut self,
        faults: impl IntoIterator<Item = HistoryFault>,
    ) {
        self.script
            .borrow_mut()
            .history
            .as_mut()
            .unwrap()
            .request_faults
            .extend(faults);
    }
    pub(crate) fn history_queries(&self) -> Vec<[u64; 2]> {
        self.script
            .borrow()
            .history
            .as_ref()
            .unwrap()
            .queries
            .clone()
    }
    pub(crate) fn history_requests(
        &self,
    ) -> Vec<(u64, OutputFrameBasis, SourcePlaybackIdentity, u64)> {
        self.script
            .borrow()
            .history
            .as_ref()
            .unwrap()
            .requests
            .clone()
    }
}
impl OutputReplacementBackend<NativeOutputState> for Effects {
    type Presentation = beatkernel::time::presentation::PresentationEstimator;
    type Output = Output;
    type Request = Request;
    type Error = io::Error;
    fn open(
        &mut self,
        request: Request,
        mut state: NativeOutputState,
        epoch: u64,
    ) -> Result<Output, OutputOpenFailure<io::Error, Output, NativeOutputState>> {
        self.script.borrow_mut().calls.push("open");
        if request.fault == Fault::Open {
            return Err(OutputOpenFailure::recovered_state(
                io::Error::other("original open failure"),
                Some(state),
            ));
        }
        if request.fault == Fault::PartialOpen {
            return Err(OutputOpenFailure::pending_state(
                io::Error::other("original partial open failure"),
                native(state, epoch, request, &self.script),
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
        Ok(native(state, epoch, request, &self.script))
    }
    fn retire(&mut self, output: &mut Output) -> Result<(), io::Error> {
        self.script.borrow_mut().calls.push("retire");
        if self
            .script
            .borrow_mut()
            .retirement_failures
            .pop_front()
            .unwrap_or(false)
        {
            return Err(io::Error::other("scripted native retirement failure"));
        }
        if output.request.fault == Fault::Retire {
            output.request.fault = Fault::None;
            return Err(io::Error::other("original retirement failure"));
        }
        output.retired = true;
        Ok(())
    }
    fn start(&mut self, output: &mut Output) -> Result<(), io::Error> {
        self.script.borrow_mut().calls.push("start");
        assert!(output.source_state().mixer().pause_requested());
        if output.request.fault == Fault::Start {
            return Err(io::Error::other("original start failure"));
        }
        output.started = true;
        Ok(())
    }
    fn epoch(&self, output: &Output) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Output) -> OutputFrameBasis {
        output.basis
    }
    fn observe(&mut self, _: &mut Output, _: &mut Self::Presentation) -> Result<(), io::Error> {
        panic!("Source coordinates must not invoke scalar estimation")
    }
    fn render_report(&self, _: &Output) -> Result<Option<RenderReport>, io::Error> {
        panic!("independent report cannot certify a coherent Source packet")
    }
}
impl OutputChannelRemixBackend<NativeOutputState> for Effects {
    fn open_remixed(
        &mut self,
        mut request: Request,
        state: NativeOutputState,
        epoch: u64,
        matrix: ChannelMatrix,
    ) -> Result<Output, OutputOpenFailure<io::Error, Output, NativeOutputState>> {
        request.matrix = Some(matrix);
        self.open(request, state, epoch)
    }
}
impl OriginalNativeOutputBackend<NativeOutputState> for Effects {
    fn observe_native(&mut self, _: &mut Output) -> Result<Option<NativeAudioSnapshot>, io::Error> {
        panic!("independent native read cannot certify coherent Source playback")
    }
}
impl OriginalCoordinateNativeOutputBackend<NativeOutputState> for Effects {
    fn observe_native_coordinate(
        &mut self,
        _: &mut Output,
    ) -> Result<Option<CoordinateNativeAudioSnapshot>, io::Error> {
        panic!("standalone coordinate read cannot certify coherent Source playback")
    }
}
fn packet_at(
    output: &Output,
    identity: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    report: Option<RenderReport>,
    fault: Fault,
) -> Result<NativeSourcePlaybackQuery<Evidence>, io::Error> {
    let state = output.source_state();
    let history = state.mixer().source_playback_history().unwrap();
    let mut playback = history
        .lookup_at_source_time(query)
        .map_err(|error| io::Error::other(format!("{error:?}")))?;
    if fault == Fault::RunningSource {
        playback = history.span(0).unwrap();
    }
    let mut report = report;
    if fault == Fault::MissingRender {
        report = None;
    }
    if fault == Fault::ActiveRender {
        report.as_mut().unwrap().paused = false;
    }
    if fault == Fault::WrongFrozen {
        report.as_mut().unwrap().playback_start_frame += 1;
    }
    let identity = if fault == Fault::WrongIdentity {
        SourcePlaybackIdentity::new(
            identity.hz(),
            identity.initial_physical(),
            identity.initial_playback() + 1,
        )
        .unwrap()
    } else {
        identity
    };
    NativeSourcePlaybackQuery::new(
        output.epoch + u64::from(fault == Fault::WrongEpoch),
        output.basis,
        identity,
        query,
        SourcePlaybackFacts {
            playback,
            pulled_source_end: history.pulled_end_frame(),
            submitted_source_end: if fault == Fault::Unsubmitted {
                output.basis.start_physical_frame()
            } else {
                state.output_frame_basis().start_physical_frame()
            },
            render: report,
            revision: match fault {
                Fault::TornRevision => 3,
                Fault::RepeatedRevision => 2,
                _ => output.observations as u64 * 2,
            },
        },
        Some(Evidence {
            original_native_words: [output.native_id, output.presented],
        }),
    )
    .map_err(|error| io::Error::other(error.to_string()))
}
impl SourcePlaybackNativeOutputBackend<NativeOutputState> for Effects {
    type NativeEvidence = Evidence;
    fn observe_native_playback(
        &mut self,
        output: &mut Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Evidence>>, io::Error> {
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
        assert!(output.started && !output.retired);
        output.observations += 1;
        let pump = self.script.borrow().pump.is_some();
        let state = output.state.as_mut().unwrap();
        let report = if state.pending_frames() != 0 {
            let report = state.pending_report().unwrap();
            let frames = state.pending_frames().min(
                self.script
                    .borrow()
                    .pump
                    .as_ref()
                    .map_or(usize::MAX, |p| p.admit_limit),
            );
            if let Some(pcm) = self
                .script
                .borrow_mut()
                .pump
                .as_mut()
                .and_then(|p| p.pcm.as_mut())
            {
                pcm.push((
                    output.epoch,
                    state.pending_samples()
                        [..frames * usize::from(state.format().unwrap().channels())]
                        .to_vec(),
                ));
            }
            state.admit(frames).unwrap();
            output.presented += frames as u64;
            report
        } else if fault == Fault::OldHeldOnly {
            output
                .last
                .expect("old held mode follows actual retained suffix")
        } else {
            let scripted_frames = self.script.borrow_mut().render_frames.pop_front();
            let frames = scripted_frames
                .unwrap_or_else(|| self.script.borrow().pump.as_ref().map_or(2, |p| p.frames));
            let report = state.render_pending(frames).unwrap();
            if let Some(pcm) = self
                .script
                .borrow_mut()
                .pump
                .as_mut()
                .and_then(|p| p.pcm.as_mut())
            {
                pcm.push((output.epoch, state.pending_samples().to_vec()));
            }
            if fault != Fault::Unsubmitted {
                state.admit(frames).unwrap();
            }
            output.presented += frames as u64;
            report
        };
        output.last = Some(report);
        let mut descriptor = descriptor(output.basis, output.request.frequency);
        if fault == Fault::WrongFrequency {
            let AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } = descriptor else {
                panic!("frequency fault needs Counter")
            };
            descriptor = AudioAuthorityCoordinateBasis::SourceCounter {
                basis: output.basis,
                frequency: frequency + 1,
            };
        }
        let scripted_units = self.script.borrow_mut().native_units.pop_front();
        let units = scripted_units.unwrap_or_else(|| {
            units_for_frames(
                self::descriptor(output.basis, output.request.frequency),
                if fault == Fault::Duplicate {
                    2
                } else {
                    output.presented
                },
            )
        });
        let at = if fault == Fault::Future {
            now().timestamp.as_nanos() + 1_000_000
        } else if fault == Fault::Stale {
            HOST_START - 200_000_000
        } else if matches!(fault, Fault::Duplicate | Fault::OldHeldOnly) {
            // This is the original unchanged native read, including its QPC
            // receipt and HOST point. Software can render/admit more PCM while
            // that native position remains unchanged, but cannot refresh it.
            HOST_START + 21_000
        } else if pump {
            let mut s = self.script.borrow_mut();
            let p = s.pump.as_mut().unwrap();
            p.host += 1000;
            p.host
        } else {
            let mut script = self.script.borrow_mut();
            script.last_host = script.last_host.max(HOST_START + 20_000) + 1000;
            script.last_host
        };
        let mut coordinate = snapshot(
            output.epoch + u64::from(fault == Fault::WrongEpoch),
            descriptor,
            units,
            at,
        );
        if fault == Fault::WrongDomain {
            let mut raw = coordinate.snapshot();
            match &mut raw.evidence {
                OriginalNativePresentationEvidence::Wasapi { snapshot, .. } => {
                    snapshot
                        .clock
                        .as_mut()
                        .unwrap()
                        .host_point
                        .as_mut()
                        .unwrap()
                        .domain = ClockDomainId(77)
                }
                OriginalNativePresentationEvidence::SuppliedPair(pair) => {
                    pair.target.domain = ClockDomainId(77)
                }
                _ => unreachable!(),
            }
            coordinate = CoordinateNativeAudioSnapshot::new(raw, coordinate.coordinate()).unwrap();
        }
        let query = coordinate
            .source_query()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let packet = packet_at(output, identity, query, Some(report), fault)?;
        self.script
            .borrow_mut()
            .native_observations
            .push(coordinate);
        let result = NativePlaybackObservation::new(coordinate, packet)
            .map(Some)
            .map_err(|error| io::Error::other(error.to_string()));
        if fault == Fault::ChangedAfterRead {
            output.epoch += 1;
        }
        if fault == Fault::ChangedBasis {
            output.basis = OutputFrameBasis::new(
                output.basis.origin(),
                SOURCE_RATE,
                output.basis.start_physical_frame() + 1,
            )
            .unwrap();
        }
        result
    }
    fn source_playback_at(
        &self,
        output: &Output,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Evidence>>, io::Error> {
        assert!(
            self.script.borrow().pump.is_some() || self.script.borrow().history.is_some(),
            "replacement must retain its original accepted packet"
        );
        let Some(report) = output.last else {
            return Ok(None);
        };
        let packet = packet_at(output, identity, *query, Some(report), Fault::None)?;
        let mut script = self.script.borrow_mut();
        let Some(history) = script.history.as_mut() else {
            return Ok(Some(packet));
        };
        history
            .queries
            .push(packet.native_evidence().unwrap().original_native_words);
        let fault = history
            .query_faults
            .pop_front()
            .unwrap_or(HistoryFault::None);
        match fault {
            HistoryFault::None => Ok(Some(packet)),
            HistoryFault::Io | HistoryFault::Authorize => {
                if matches!(fault, HistoryFault::Authorize) {
                    script.statuses.clear();
                    script
                        .statuses
                        .push_front(SourceRetirementStatus::Authorized);
                }
                Err(io::Error::other("original native history query failure"))
            }
            HistoryFault::PacketIdentity | HistoryFault::PacketQuery => {
                let query = if matches!(fault, HistoryFault::PacketQuery) {
                    let changed = TargetTime::from_frames(
                        output.basis.start_physical_frame() + output.presented - 1,
                        SOURCE_RATE,
                    )
                    .unwrap();
                    InterpolatedTargetTime::new(changed, 0, 0, 1, 0, 1).unwrap()
                } else {
                    *query
                };
                NativeSourcePlaybackQuery::new(
                    packet.epoch() + u64::from(matches!(fault, HistoryFault::PacketIdentity)),
                    packet.basis(),
                    identity,
                    query,
                    packet.facts(),
                    Some(Evidence {
                        original_native_words: [output.native_id, output.presented],
                    }),
                )
                .map(Some)
                .map_err(|error| io::Error::other(error.to_string()))
            }
        }
    }
}
impl ConditionalSourceRetirementNativeOutputBackend<NativeOutputState> for Effects {
    type Ticket = Ticket;
    fn created_source_coordinate_basis(
        &self,
        output: &Output,
    ) -> Result<AudioAuthorityCoordinateBasis, io::Error> {
        self.script.borrow_mut().calls.push("created");
        if output.request.fault == Fault::Created {
            return Err(io::Error::other("original created descriptor failure"));
        }
        Ok(descriptor(output.basis, output.request.frequency))
    }
    fn submit_source_conditional_retirement(
        &mut self,
        output: &mut Output,
        request: &Request,
        matrix: Option<&ChannelMatrix>,
    ) -> Result<Option<Ticket>, io::Error> {
        self.script.borrow_mut().calls.push("submit");
        if request.fault == Fault::Submit || request.period == 0 || request.buffer < request.period
        {
            return Err(io::Error::other("original cold request refusal"));
        }
        output
            .source_state()
            .validate_reconfigure(
                request.format,
                matrix.or(request.matrix.as_ref()),
                request.buffer as usize,
            )
            .map_err(|error| io::Error::other(error.to_string()))?;
        if request.fault == Fault::Busy {
            return Ok(None);
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
    fn poll_source_conditional_retirement(
        &mut self,
        output: &mut Output,
        ticket: &Ticket,
    ) -> Result<SourceRetirementStatus, io::Error> {
        assert_eq!(
            output.native_id, ticket.native_id,
            "ticket stays bound to its original native allocation"
        );
        let mut script = self.script.borrow_mut();
        script.calls.push("conditional-poll");
        if script.probe_failures.pop_front().unwrap_or(false) {
            return Err(io::Error::other(
                "scripted native conditional probe failure",
            ));
        }
        let status = script
            .statuses
            .pop_front()
            .unwrap_or(SourceRetirementStatus::Authorized);
        if matches!(
            status,
            SourceRetirementStatus::Refused | SourceRetirementStatus::Cancelled
        ) {
            script.consumed_tickets.push(ticket.serial);
        }
        Ok(status)
    }
    fn cancel_source_conditional_retirement(
        &mut self,
        output: &mut Output,
        ticket: &Ticket,
    ) -> Result<SourceRetirementStatus, io::Error> {
        assert_eq!(output.native_id, ticket.native_id);
        let mut script = self.script.borrow_mut();
        script.calls.push("conditional-cancel");
        let status = script
            .cancel_status
            .unwrap_or(SourceRetirementStatus::Cancelled);
        script.statuses.push_front(status);
        Ok(status)
    }
}
impl SourceHistoryRetirementNativeOutputBackend<NativeOutputState> for Effects {
    fn request_source_history_retirement(
        &mut self,
        output: &mut Output,
        expected_epoch: u64,
        expected_basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        source_pin: u64,
    ) -> Result<Option<u64>, io::Error> {
        assert!(output.started && !output.retired);
        assert_eq!(expected_epoch, output.epoch);
        assert_eq!(expected_basis, output.basis);
        let history = output
            .source_state()
            .mixer()
            .source_playback_history()
            .unwrap();
        assert_eq!(identity.hz(), history.source_rate());
        assert_eq!(
            identity.initial_physical(),
            history.initial_physical_frame()
        );
        assert_eq!(
            identity.initial_playback(),
            history.initial_playback_frame()
        );
        history.validate_retirement_before(source_pin).unwrap();
        let mut script = self.script.borrow_mut();
        script.calls.push("history-request");
        let history = script.history.as_mut().expect("explicit history service");
        history
            .requests
            .push((expected_epoch, expected_basis, identity, source_pin));
        let fault = history
            .request_faults
            .pop_front()
            .unwrap_or(HistoryFault::None);
        if matches!(fault, HistoryFault::Io | HistoryFault::Authorize) {
            if matches!(fault, HistoryFault::Authorize) {
                script.statuses.clear();
                script
                    .statuses
                    .push_front(SourceRetirementStatus::Authorized);
            }
            return Err(io::Error::other(
                "original native history retirement failure",
            ));
        }
        assert!(matches!(fault, HistoryFault::None));
        history.revision += 1;
        Ok(Some(history.revision))
    }
}

pub(crate) type Controller =
    OutputReplacement<Effects, NativeOutputState, OutputFrameBasis, Ticket>;
pub(crate) type Ready = ReadyCoordinateSourceAudioOutput<Output, Evidence>;
pub(crate) struct Rig {
    pub(crate) current: NativeAudioPresentation,
    pub(crate) pause: NativePause,
    pub(crate) inputs: InputMerger,
    pub(crate) runtime: SoloRuntime,
    pub(crate) practice: PracticeController,
    pub(crate) pin: OutputFrameBasis,
    pub(crate) identity: SourcePlaybackIdentity,
    pub(crate) config: NativeGameplayConfig,
    pub(crate) end: Option<NativeEnd>,
    pub(crate) output: Option<Output>,
    pub(crate) script: Rc<RefCell<Script>>,
}
impl Rig {
    pub(crate) fn new(partial: bool) -> Self {
        Self::with_frequency(partial, Some(OLD_FREQUENCY))
    }
    pub(crate) fn with_frequency(partial: bool, frequency: Option<u64>) -> Self {
        Self::new_at_epoch(partial, frequency, OLD_EPOCH)
    }
    pub(crate) fn new_at_epoch(partial: bool, frequency: Option<u64>, epoch: u64) -> Self {
        let format = AudioFormat::new(SOURCE_RATE, 1).unwrap();
        let sample_frames = 512;
        let sample_bytes = sample_frames * std::mem::size_of::<f32>();
        let limits = PcmLimits::new(sample_bytes, sample_bytes, 1).unwrap();
        let mut bank = SampleBank::new(format, limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25; sample_frames], limits).unwrap(),
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
        let (mut producer, consumer) = command_queue_with_start_gate(16).unwrap();
        producer.schedule_start_at(2).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::from_nanos(100),
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
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), true)
                    .unwrap(),
            )
            .unwrap();
        let startup = mixer.render(&mut [0.0; 5]).unwrap();
        assert_eq!(startup.playback_frames, 3);
        assert_eq!(mixer.applied_start_frame(), Some(2));
        mixer.enable_source_playback_history(64).unwrap();
        let pin = mixer.output_frame_basis();
        let identity = SourcePlaybackIdentity::new(SOURCE_RATE, 5, 3).unwrap();
        let mut state = NativeOutputState::new(
            mixer,
            DeviceFormat::new(SOURCE_RATE, 1, SampleEncoding::Float32, None).unwrap(),
            None,
            128,
        )
        .unwrap_or_else(|_| panic!("complete Source owner"));
        let active = state.render_pending(4).unwrap();
        assert_eq!(active.playback_frames, 4);
        assert!(state.pending_samples().iter().any(|v| *v == 0.25));
        state.admit(4).unwrap();
        let old_descriptor = descriptor(pin, frequency);
        let mut current = new_coordinate_audio_presentation(
            epoch,
            old_descriptor,
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
        for (units, at) in [
            (u64::from(frequency.is_some()), HOST_START),
            (units_for_frames(old_descriptor, 3), HOST_START + 1000),
        ] {
            current
                .admit_coordinate(snapshot(epoch, old_descriptor, units, at))
                .unwrap();
        }
        current.pin_practice_mixer_basis(pin).unwrap();
        let mut pause = NativePause::new(pin.origin(), ClockDomainId(1), SOURCE_RATE)
            .unwrap()
            .with_start_frame(2)
            .unwrap()
            .with_coordinate_binding(epoch, old_descriptor, identity)
            .unwrap();
        assert!(pause.request_coordinate(true, current.authority()).unwrap());
        producer.request_pause(true);
        let report = state.render_pending(8).unwrap();
        assert_eq!((report.start_frame, report.playback_start_frame), (9, 7));
        assert!(report.paused && report.playback_frames == 0);
        let adoption = pause
            .prepare_coordinate_adoption(Some(report), None)
            .unwrap()
            .unwrap();
        current
            .admit_coordinate(snapshot(
                epoch,
                old_descriptor,
                units_for_frames(old_descriptor, 5),
                HOST_START + 5000,
            ))
            .unwrap();
        let mut inputs =
            InputMerger::new_dynamic(ClockDomainId(1), host(HOST_START), 4, 16).unwrap();
        inputs.register_source(DeviceId(9)).unwrap();
        let original = physical(1, HOST_START + 1000);
        inputs
            .admit(original.clone(), host(HOST_START + 5000))
            .unwrap();
        current
            .authority_mut()
            .record_acquired_prefix(host(HOST_START + 5000))
            .unwrap();
        let mapped = current
            .authority()
            .prepare_coordinate_input(host(HOST_START + 1000), host(HOST_START + 5000))
            .unwrap()
            .unwrap();
        assert_eq!(
            inputs.pop_ready(host(HOST_START + 5000)).unwrap(),
            Some(original)
        );
        current
            .authority_mut()
            .commit_coordinate_input(mapped, host(HOST_START + 5000))
            .unwrap();
        inputs.commit(host(HOST_START + 1000)).unwrap();
        let at = current
            .authority()
            .presented_coordinate_boundary_host(&adoption.exact_cut(), host(HOST_START + 5000))
            .unwrap()
            .unwrap();
        let cut = current
            .authority()
            .prepare_coordinate_control_cutoff(
                epoch,
                adoption.exact_cut(),
                at,
                host(HOST_START + 5000),
                &inputs,
            )
            .unwrap()
            .unwrap();
        let ack = pause
            .commit_coordinate_adoption(
                adoption,
                current.authority(),
                &cut,
                host(HOST_START + 5000),
                &inputs,
            )
            .unwrap();
        assert!(ack.paused);
        assert_eq!(ack.source_identity, identity);
        current
            .authority_mut()
            .commit_coordinate_control_cutoff(cut, host(HOST_START + 5000), &inputs)
            .unwrap();
        state.admit(if partial { 3 } else { 8 }).unwrap();
        assert_eq!(pause.phase(), PausePhase::Paused);
        assert_eq!(state.mixer().frame_cursor(), 17);
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
        let script = Rc::new(RefCell::new(Script::default()));
        let mut output = native(state, epoch, Request::normal(frequency), &script);
        // Native creation remains the original pin even when the owner has pulled ahead.
        output.basis = pin;
        output.started = true;
        output.last = Some(report);
        output.presented = if partial { 7 } else { 12 };
        Self {
            current,
            pause,
            inputs,
            runtime,
            practice,
            pin,
            identity,
            end: None,
            output: Some(output),
            script,
            config: NativeGameplayConfig {
                origin: host(HOST_START),
                stream_origin: pin.point_at_stream_frame(0).unwrap(),
                playback_origin: pin.point_at_stream_frame(0).unwrap(),
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
    pub(crate) fn controller(&mut self) -> Controller {
        let mut c = Controller::new(Effects {
            script: self.script.clone(),
        });
        assert!(c.attach(self.output.take().unwrap()).is_ok());
        c
    }
    pub(crate) fn take_native_owner(&mut self) -> (Effects, Output) {
        (
            Effects {
                script: self.script.clone(),
            },
            self.output.take().unwrap(),
        )
    }
    pub(crate) fn take_live_native_owner(&mut self) -> (Effects, Output) {
        self.take_native_owner()
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
    pub(crate) fn presentation_mut(&mut self) -> &mut NativeAudioPresentation {
        &mut self.current
    }
    pub(crate) fn paused(&self) -> &NativePause {
        &self.pause
    }
    pub(crate) fn native_config(&self) -> NativeGameplayConfig {
        self.config
    }
    pub(crate) fn begin(
        &mut self,
        c: &mut Controller,
        request: Request,
    ) -> Result<(), ReplacementFailure<io::Error>> {
        c.begin_coordinate_source_audio(
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
    pub(crate) fn poll(
        &self,
        c: &mut Controller,
    ) -> Result<CoordinateSourcePoll<Output, Evidence>, ReplacementFailure<io::Error>> {
        c.poll_coordinate_source_audio(now(), &self.current, &self.pause, &self.inputs)
    }
    pub(crate) fn poll_ready(&self, c: &mut Controller) -> Ready {
        for _ in 0..12 {
            match self.poll(c).unwrap() {
                CoordinateSourcePoll::Pending => {}
                CoordinateSourcePoll::Ready(r) => return r,
                CoordinateSourcePoll::LiveRefused(reason) => panic!("{reason}"),
            }
        }
        panic!("two actual progressing paused Source packets must qualify")
    }
    pub(crate) fn poll_failure(&self, c: &mut Controller) -> ReplacementFailure<io::Error> {
        for _ in 0..12 {
            match self.poll(c) {
                Err(f) => return f,
                Ok(CoordinateSourcePoll::Pending) => {}
                _ => panic!("failure must precede publication or restoration"),
            }
        }
        panic!("scripted failure cannot wait indefinitely")
    }
    pub(crate) fn ready(&mut self, request: Request) -> (Controller, Ready) {
        let mut c = self.controller();
        self.begin(&mut c, request).unwrap();
        let r = self.poll_ready(&mut c);
        (c, r)
    }
    pub(crate) fn publish<A>(
        &mut self,
        ready: Ready,
        stage: impl FnOnce(&Ready) -> Result<A, Box<dyn std::error::Error>>,
    ) -> Result<(PauseHold, A), ReadyCoordinateSourceAudioPublicationFailure<Output, Evidence>>
    {
        publish_ready_coordinate_source_audio_output_held(
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
pub(crate) fn preserved_frontiers(p: &NativeAudioPresentation) -> String {
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
pub(crate) struct SourceHistorySnapshot {
    pub(crate) source_rate: u32,
    pub(crate) initial_physical: u64,
    pub(crate) initial_playback: u64,
    pub(crate) pulled_end: u64,
    pub(crate) retained_start: u64,
    pub(crate) capacity: usize,
    pub(crate) retirement_revision: u64,
    pub(crate) spans: Vec<[u64; 5]>,
}
pub(crate) fn source_history(state: &NativeOutputState) -> SourceHistorySnapshot {
    let h = state.mixer().source_playback_history().unwrap();
    SourceHistorySnapshot {
        source_rate: h.source_rate(),
        initial_physical: h.initial_physical_frame(),
        initial_playback: h.initial_playback_frame(),
        pulled_end: h.pulled_end_frame(),
        retained_start: h.retained_start_frame(),
        capacity: h.capacity(),
        retirement_revision: h.retirement_revision(),
        spans: (0..h.len())
            .map(|i| {
                let s = h.span(i).unwrap();
                [
                    s.physical_start(),
                    u64::from(s.physical_end().is_some()),
                    s.physical_end().unwrap_or(0),
                    s.playback_start(),
                    u64::from(s.state() == SourcePlaybackState::Frozen),
                ]
            })
            .collect(),
    }
}
pub(crate) fn owner_facts(state: &NativeOutputState) -> String {
    format!(
        "{:?}",
        (
            (
                state.mixer().config(),
                state.mixer().frame_cursor(),
                state.mixer().playback_frame_cursor(),
                state.mixer().counters(),
                state.mixer().applied_start_frame(),
                state.output_frame_basis(),
            ),
            (
                state.format(),
                state.max_frames(),
                state.pending_report(),
                state.admitted_frames(),
                state.pending_samples(),
                source_history(state),
            ),
            state.admission_is_uncertain()
        )
    )
}
pub(crate) fn pending(value: CoordinateSourcePoll<Output, Evidence>) {
    assert!(matches!(value, CoordinateSourcePoll::Pending));
}
