//! Complete Source ownership and the thin native conditional-retirement ticket.
use super::*;
use crate::audio::wasapi_source_worker::{
    validate_source_open_geometry, SourceRetirementError, SourceRetirementFacts,
    WasapiSourceFailure, WasapiSourceOps,
};
use beatkernel::audio::{AudioError, MixerOpenFailure, OutputFrameBasis, RenderReport};
use std::sync::{atomic::AtomicU64, Weak};

/// Original owner-thread operation which failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasapiCompletePhase {
    /// Previously uncertain native admission forbids further output.
    Quarantined,
    /// Native queued-frame query.
    Padding,
    /// Native buffer acquisition.
    Acquire,
    /// Qualified Source history retirement.
    History,
    /// Original software rendering.
    Render,
    /// Native PCM encoding.
    Encode,
    /// Matching native buffer release.
    Release,
    /// Committing the positively released software prefix.
    Admission,
    /// Original native audio-clock reading.
    Clock,
    /// Native stream start or its acknowledgment.
    Start,
    /// Native stream stop.
    Stop,
    /// Worker join.
    Join,
    /// Complete software ownership or worker-resource recovery.
    Recovery,
}
impl From<WasapiSourcePhase> for WasapiCompletePhase {
    fn from(phase: WasapiSourcePhase) -> Self {
        match phase {
            WasapiSourcePhase::Quarantined => Self::Quarantined,
            WasapiSourcePhase::Padding => Self::Padding,
            WasapiSourcePhase::Acquire => Self::Acquire,
            WasapiSourcePhase::History => Self::History,
            WasapiSourcePhase::Render => Self::Render,
            WasapiSourcePhase::Encode => Self::Encode,
            WasapiSourcePhase::Release => Self::Release,
            WasapiSourcePhase::Admission => Self::Admission,
            WasapiSourcePhase::Clock => Self::Clock,
            WasapiSourcePhase::Start => Self::Start,
            WasapiSourcePhase::Stop => Self::Stop,
            WasapiSourcePhase::Join => Self::Join,
            WasapiSourcePhase::Recovery => Self::Recovery,
        }
    }
}
/// Fixed-size original failure, retaining its raw HRESULT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasapiCompleteFailure {
    /// Original failing operation.
    pub phase: WasapiCompletePhase,
    /// Unmodified native HRESULT, or the fixed worker failure marker.
    pub code: i32,
}
/// Cleanup failures supplement the primary failure rather than replacing it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WasapiCompleteFailureReport {
    /// First failure; later cleanup never replaces it.
    pub primary: Option<WasapiCompleteFailure>,
    /// Matching buffer release failure, including after a local failure.
    pub release: Option<i32>,
    /// Native terminal stop failure.
    pub stop: Option<i32>,
    /// Native stop after a refused start.
    pub start_cleanup: Option<i32>,
    /// Worker panic or unavailable join result.
    pub join: Option<i32>,
    /// Unavailable software owner or owner-thread resource cleanup failure.
    pub recovery: Option<i32>,
}
impl From<WasapiSourceFailureReport> for WasapiCompleteFailureReport {
    fn from(report: WasapiSourceFailureReport) -> Self {
        Self {
            primary: report.primary.map(|failure| WasapiCompleteFailure {
                phase: failure.phase.into(),
                code: failure.code,
            }),
            release: report.release,
            stop: report.stop,
            start_cleanup: report.start_cleanup,
            join: report.join,
            recovery: report.recovery,
        }
    }
}
/// Cold public error; the complete recovered owner is carried by open/recovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasapiCompleteError {
    /// Original public operation error.
    pub error: AudioPlatformError,
    /// Original worker failure and all available cleanup evidence.
    pub report: WasapiCompleteFailureReport,
}
impl std::fmt::Display for WasapiCompleteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({:?})", self.error, self.report)
    }
}
impl std::error::Error for WasapiCompleteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
impl WasapiCompleteError {
    pub(super) fn new(error: AudioPlatformError, report: WasapiSourceFailureReport) -> Self {
        Self {
            error,
            report: report.into(),
        }
    }
}

/// A ticket binds one original complete control allocation and checked nonce.
#[derive(Debug)]
pub struct SourceRetirementTicket {
    control: Weak<SourceControl>,
    nonce: u64,
}
/// Permission is distinct from confirmed native teardown and owner recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRetirementStatus {
    /// Sole worker has not decided against actual retained state.
    Pending,
    /// Worker refused and retained the live output.
    Refused,
    /// Cancellation won the shared atomic decision.
    Cancelled,
    /// Sticky permission; Stop, join and recovery still need confirmation.
    Authorized,
}
impl From<crate::audio::wasapi_source_worker::SourceRetirementStatus> for SourceRetirementStatus {
    fn from(status: crate::audio::wasapi_source_worker::SourceRetirementStatus) -> Self {
        use crate::audio::wasapi_source_worker::SourceRetirementStatus as S;
        match status {
            S::Pending => Self::Pending,
            S::Refused => Self::Refused,
            S::Cancelled => Self::Cancelled,
            S::Authorized => Self::Authorized,
        }
    }
}

// Each field is an append-only atomic word. A primary phase/code shares one
// word; cleanup fields never overwrite it. Readers may see an earlier prefix.
pub(super) struct FailurePublication([AtomicU64; 6]);
impl FailurePublication {
    pub(super) fn new() -> Self {
        Self(std::array::from_fn(|_| AtomicU64::new(0)))
    }
    pub(super) fn publish(&self, report: WasapiSourceFailureReport) {
        let primary = report.primary.map_or(0, |failure| {
            ((failure.phase as u64 + 1) << 32) | u64::from(failure.code as u32)
        });
        let code =
            |value: Option<i32>| value.map_or(0, |value| (1 << 32) | u64::from(value as u32));
        for (slot, word) in self.0.iter().zip([
            primary,
            code(report.release),
            code(report.stop),
            code(report.start_cleanup),
            code(report.join),
            code(report.recovery),
        ]) {
            if word != 0 {
                let _ = slot.compare_exchange(0, word, Ordering::AcqRel, Ordering::Acquire);
            }
        }
    }
    pub(super) fn read(&self) -> WasapiSourceFailureReport {
        let words = self.0.each_ref().map(|word| word.load(Ordering::Acquire));
        let code = |word: u64| (word != 0).then_some(word as u32 as i32);
        let phases = [
            WasapiSourcePhase::Quarantined,
            WasapiSourcePhase::Padding,
            WasapiSourcePhase::Acquire,
            WasapiSourcePhase::History,
            WasapiSourcePhase::Render,
            WasapiSourcePhase::Encode,
            WasapiSourcePhase::Release,
            WasapiSourcePhase::Admission,
            WasapiSourcePhase::Clock,
            WasapiSourcePhase::Start,
            WasapiSourcePhase::Stop,
            WasapiSourcePhase::Join,
            WasapiSourcePhase::Recovery,
        ];
        WasapiSourceFailureReport {
            primary: if words[0] == 0 {
                None
            } else {
                Some(WasapiSourceFailure {
                    phase: phases[((words[0] >> 32) - 1) as usize],
                    code: words[0] as u32 as i32,
                })
            },
            release: code(words[1]),
            stop: code(words[2]),
            start_cleanup: code(words[3]),
            join: code(words[4]),
            recovery: code(words[5]),
        }
    }
}
pub(super) struct SourceControl {
    pub(super) common: Arc<Control>,
    pub(super) retirement: SourceRetirementSlot,
    pub(super) failures: FailurePublication,
    pub(super) nonce: AtomicU64,
}
pub(super) struct CompleteWorkerResult {
    pub(super) state: Option<NativeOutputState>,
    pub(super) report: WasapiSourceFailureReport,
}

/// Explicit complete retained mode; legacy Mixer recovery remains separate.
pub struct WasapiCompleteStream {
    pub(super) basis: OutputFrameBasis,
    pub(super) configuration: AppliedStreamConfig,
    pub(super) options: WasapiOptions,
    pub(super) control: Arc<SourceControl>,
    pub(super) wake: Event,
    pub(super) worker: Option<JoinHandle<CompleteWorkerResult>>,
    pub(super) recovered: Option<NativeOutputState>,
    pub(super) retired: bool,
    pub(super) started: mpsc::Receiver<Result<(), i32>>,
    pub(super) has_started: bool,
    pub(super) clock_frequency: u64,
    pub(super) failure: WasapiSourceFailureReport,
    pub(super) facts: SourceRetirementFacts,
}

pub(super) fn complete_source_telemetry(
    state: &NativeOutputState,
) -> Result<Telemetry, AudioError> {
    match state.mixer().source_playback_history() {
        Some(history) => {
            Telemetry::with_source_history_and_retirement(history, state.output_frame_basis())
        }
        None => Ok(Telemetry::new()),
    }
}

impl WasapiBackend {
    /// Opens a complete same-rate Source owner without destructive extraction.
    /// History is prepared from actual native capacity before Ready prefill.
    pub fn open_complete_recoverable(
        &self,
        request: AudioStreamRequest,
        state: NativeOutputState,
        clock: QpcClock,
        options: WasapiOptions,
        matrix: Option<ChannelMatrix>,
        history_headroom: usize,
    ) -> Result<WasapiCompleteStream, MixerOpenFailure<WasapiCompleteError, NativeOutputState>>
    {
        let basis = state.output_frame_basis();
        let preflight = (|| {
            if state.admission_is_uncertain() {
                return Err(AudioPlatformError::InvalidRequest);
            }
            if state.pending_frames() != 0 && state.mixer().source_playback_history().is_none() {
                return Err(AudioPlatformError::InvalidRequest);
            }
            if request.backend() != AudioBackendKind::Wasapi {
                return Err(AudioPlatformError::InvalidRequest);
            }
            validate_open_with_matrix(&request, state.mixer(), options, matrix.as_ref())?;
            validate_source_open_geometry(&state, &request)?;
            if state
                .format()
                .is_some_and(|format| format.encoding() != request.format().encoding())
            {
                return Err(AudioPlatformError::InvalidFormat);
            }
            state
                .validate_reconfigure(
                    request.format(),
                    matrix.as_ref(),
                    state.mixer().config().limits().max_render_frames(),
                )
                .map_err(|_| AudioPlatformError::InvalidFormat)
        })();
        if let Err(error) = preflight {
            let mut report = WasapiSourceFailureReport::default();
            if state.admission_is_uncertain() {
                report.record_primary(WasapiSourcePhase::Quarantined, E_INVALIDARG_CODE);
            }
            return Err(MixerOpenFailure::new_state(
                WasapiCompleteError::new(error, report),
                Some(state),
            ));
        }
        let (opened_tx, opened_rx) = mpsc::sync_channel(1);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let worker =
            crate::audio::mixer_launch::launch_worker(NativeWorkerSpawner, state, move |state| {
                let mut owner = Some(WorkerOwner::Complete(state));
                let mut setup_report = WasapiSourceFailureReport::default();
                let prepared = Worker::new(
                    request,
                    &mut owner,
                    clock,
                    options,
                    matrix.clone(),
                    None,
                    Some(history_headroom),
                    &mut setup_report,
                );
                let (mut worker, telemetry) = match prepared {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        let _ = opened_tx.send(Err(WasapiCompleteError::new(error, setup_report)));
                        return CompleteWorkerResult {
                            state: owner.map(WorkerOwner::into_complete),
                            report: setup_report,
                        };
                    }
                };
                let facts = SourceRetirementFacts::from_state(
                    match worker.owner.as_ref().expect("prepared complete owner") {
                        WorkerOwner::Complete(state) => state,
                        WorkerOwner::Legacy(_) => unreachable!("complete launch"),
                    },
                    matrix,
                    worker.configuration.requested.mode(),
                );
                let facts = match facts {
                    Ok(facts) => facts,
                    Err(error) => {
                        worker.finish_setup_failure(&mut setup_report);
                        let state = worker.owner.take().map(WorkerOwner::into_complete);
                        drop(worker);
                        let _ = opened_tx.send(Err(WasapiCompleteError::new(error, setup_report)));
                        return CompleteWorkerResult {
                            state,
                            report: setup_report,
                        };
                    }
                };
                let control = Arc::new(SourceControl {
                    common: Arc::new(Control {
                        request: AtomicU8::new(0),
                        telemetry,
                        cadence: crate::audio::cadence::Capture::new(),
                    }),
                    retirement: SourceRetirementSlot::new(),
                    failures: FailurePublication::new(),
                    nonce: AtomicU64::new(0),
                });
                worker.source_control = Some(Arc::clone(&control));
                worker.publish(&control.common.telemetry);
                let wake = match worker.control_event.duplicate() {
                    Ok(wake) => wake,
                    Err(error) => {
                        if let AudioPlatformError::Native { code } = &error {
                            setup_report.record_primary(WasapiSourcePhase::Recovery, *code);
                        }
                        worker.finish_setup_failure(&mut setup_report);
                        let state = worker.owner.take().map(WorkerOwner::into_complete);
                        drop(worker);
                        let _ = opened_tx.send(Err(WasapiCompleteError::new(error, setup_report)));
                        return CompleteWorkerResult {
                            state,
                            report: setup_report,
                        };
                    }
                };
                if opened_tx
                    .send(Ok((
                        worker.configuration.clone(),
                        wake,
                        Arc::clone(&control),
                        worker.clock_frequency,
                        facts,
                    )))
                    .is_ok()
                {
                    worker.run(&control.common, Some(&control), &started_tx);
                } else {
                    setup_report.record_primary(WasapiSourcePhase::Recovery, E_FAIL_CODE);
                    worker.finish_setup_failure(&mut setup_report);
                }
                let result = CompleteWorkerResult {
                    state: worker.owner.take().map(WorkerOwner::into_complete),
                    report: worker.failure,
                };
                drop(worker);
                result
            })
            .map_err(|failure| {
                let (_, state) = failure.into_parts();
                MixerOpenFailure::new_state(
                    WasapiCompleteError::new(
                        AudioPlatformError::WorkerFailure,
                        WasapiSourceFailureReport::default(),
                    ),
                    state,
                )
            })?;
        match opened_rx.recv() {
            Ok(Ok((configuration, wake, control, clock_frequency, facts))) => {
                Ok(WasapiCompleteStream {
                    basis,
                    configuration,
                    options,
                    control,
                    wake,
                    clock_frequency,
                    facts,
                    worker: Some(worker),
                    recovered: None,
                    retired: false,
                    started: started_rx,
                    has_started: false,
                    failure: WasapiSourceFailureReport::default(),
                })
            }
            result => {
                let mut error = match result {
                    Ok(Err(error)) => error,
                    Err(_) => WasapiCompleteError::new(
                        AudioPlatformError::WorkerFailure,
                        WasapiSourceFailureReport::default(),
                    ),
                    Ok(Ok(_)) => unreachable!("successful startup handled above"),
                };
                match worker.join() {
                    Ok(joined) => {
                        // Setup's operation/report precedes owner-thread resource teardown.
                        let mut report = joined.report;
                        if joined.state.is_none() {
                            report.recovery.get_or_insert(E_FAIL_CODE);
                        }
                        error.report = report.into();
                        Err(MixerOpenFailure::new_state(error, joined.state))
                    }
                    Err(_) => {
                        error.report.join = Some(E_FAIL_CODE);
                        error.report.recovery = Some(E_FAIL_CODE);
                        Err(MixerOpenFailure::new_state(error, None))
                    }
                }
            }
        }
    }
}

impl WasapiCompleteStream {
    /// First unsent Source frame captured before this native prefill.
    pub const fn frame_basis(&self) -> OutputFrameBasis {
        self.basis
    }
    /// Actual newly created IAudioClock frequency, without progress permission.
    pub const fn clock_frequency(&self) -> u64 {
        self.clock_frequency
    }
    /// Actual native format, geometry and latency.
    pub fn configuration(&self) -> &AppliedStreamConfig {
        &self.configuration
    }
    /// Explicit worker scheduling and wake policy.
    pub const fn options(&self) -> WasapiOptions {
        self.options
    }
    /// Original worker telemetry, retaining raw counter units.
    pub fn snapshot(&self) -> AudioStreamSnapshot {
        self.control.common.telemetry.read()
    }
    fn report(&self) -> WasapiSourceFailureReport {
        let mut report = self.failure;
        merge_report(&mut report, self.control.failures.read());
        report
    }
    /// Available immutable failure prefix; join completes cleanup evidence.
    pub fn failure_report(&self) -> WasapiCompleteFailureReport {
        self.report().into()
    }
    fn error(&self, error: AudioPlatformError) -> WasapiCompleteError {
        WasapiCompleteError::new(error, self.report())
    }
    fn ticket_nonce(&self, ticket: &SourceRetirementTicket) -> Result<u64, WasapiCompleteError> {
        if !ticket.control.ptr_eq(&Arc::downgrade(&self.control)) {
            return Err(self.error(AudioPlatformError::InvalidRequest));
        }
        Ok(ticket.nonce)
    }
    fn ticket_error(&self, error: SourceRetirementError) -> WasapiCompleteError {
        self.error(match error {
            SourceRetirementError::InvalidTicket => AudioPlatformError::InvalidRequest,
            SourceRetirementError::NoncesExhausted => AudioPlatformError::Capacity,
        })
    }
    /// Probes the sticky atomic permission even after a worker has exited.
    pub fn poll_source_conditional_retirement(
        &mut self,
        ticket: &SourceRetirementTicket,
    ) -> Result<SourceRetirementStatus, WasapiCompleteError> {
        let nonce = self.ticket_nonce(ticket)?;
        let status = self
            .control
            .retirement
            .status(nonce)
            .map_err(|e| self.ticket_error(e))?;
        if status != crate::audio::wasapi_source_worker::SourceRetirementStatus::Authorized {
            self.check_live()?;
        }
        self.control
            .retirement
            .poll(nonce)
            .map(Into::into)
            .map_err(|e| self.ticket_error(e))
    }
    /// Competes with authorization in the same checked atomic decision.
    pub fn cancel_source_conditional_retirement(
        &mut self,
        ticket: &SourceRetirementTicket,
    ) -> Result<SourceRetirementStatus, WasapiCompleteError> {
        let nonce = self.ticket_nonce(ticket)?;
        let status = self
            .control
            .retirement
            .cancel(nonce)
            .map_err(|e| self.ticket_error(e))?;
        if status != crate::audio::wasapi_source_worker::SourceRetirementStatus::Authorized {
            self.check_live()?;
        }
        Ok(status.into())
    }
    /// Validates immutable request facts before asking the sole worker.
    /// Busy and unconsumed responses return None without replacing the request.
    pub fn submit_source_conditional_retirement(
        &mut self,
        request: &AudioStreamRequest,
        matrix: Option<&ChannelMatrix>,
        max_frames: usize,
    ) -> Result<Option<SourceRetirementTicket>, WasapiCompleteError> {
        let policy = self
            .facts
            .validate(request, matrix, max_frames)
            .map_err(|e| self.error(e))?;
        let current = self.control.nonce.load(Ordering::Acquire);
        if matches!(
            self.control.retirement.status(current),
            Ok(crate::audio::wasapi_source_worker::SourceRetirementStatus::Authorized)
        ) {
            return Ok(None);
        }
        self.check_live()?;
        let nonce = self
            .control
            .retirement
            .submit(policy)
            .map_err(|e| self.ticket_error(e))?;
        // Bounded native wait also probes the slot if signaling fails. Keep the
        // submitted ticket so the caller can always observe/cancel its decision.
        if let Some(nonce) = nonce {
            self.control.nonce.store(nonce, Ordering::Release);
            let _ = self.wake.signal();
        }
        Ok(nonce.map(|nonce| SourceRetirementTicket {
            control: Arc::downgrade(&self.control),
            nonce,
        }))
    }
    fn check_live(&self) -> Result<(), WasapiCompleteError> {
        let report = self.report();
        if let Some(primary) = report.primary {
            return Err(WasapiCompleteError::new(native(primary.code), report));
        }
        if let AudioStreamStatus::Failed { hresult } = self.snapshot().status {
            return Err(self.error(native(hresult)));
        }
        let nonce = self.control.nonce.load(Ordering::Acquire);
        if matches!(
            self.control.retirement.status(nonce),
            Ok(crate::audio::wasapi_source_worker::SourceRetirementStatus::Authorized)
        ) {
            return Err(self.error(AudioPlatformError::WorkerFailure));
        }
        if self.retired
            || self.worker.is_none()
            || self.control.common.request.load(Ordering::Acquire) == 2
            || self.worker.as_ref().is_some_and(JoinHandle::is_finished)
        {
            return Err(self.error(AudioPlatformError::WorkerFailure));
        }
        match self.snapshot().status {
            AudioStreamStatus::Ready | AudioStreamStatus::Running => Ok(()),
            AudioStreamStatus::Failed { hresult } => Err(self.error(native(hresult))),
            _ => Err(self.error(AudioPlatformError::WorkerFailure)),
        }
    }
    /// Fresh atomic permission, terminal status and thread-liveness probe.
    /// A cached Pending ticket is never evidence that old IO is still live.
    pub fn probe_live(&self) -> Result<(), WasapiCompleteError> {
        self.check_live()
    }
    /// Reads a coherent original Source publication after the caller's live probe.
    pub fn source_playback_at(
        &self,
        query: &InterpolatedTargetTime,
        expected_source_rate: u32,
        expected_initial_physical: u64,
        expected_initial_playback: u64,
    ) -> Option<SourcePlaybackSnapshot> {
        self.control.common.telemetry.read_source_playback_at(
            query,
            expected_source_rate,
            expected_initial_physical,
            expected_initial_playback,
        )
    }
    /// Validates the stream basis and live worker before posting history intent.
    pub fn request_source_history_retirement(
        &self,
        expected_basis: OutputFrameBasis,
        source_identity: (u32, u64, u64),
        source_pin: u64,
    ) -> Result<Option<u64>, WasapiCompleteError> {
        if expected_basis != self.basis {
            return Err(self.error(AudioPlatformError::InvalidRequest));
        }
        self.check_live()?;
        self.control
            .common
            .telemetry
            .request_source_history_retirement(expected_basis, source_identity, source_pin)
            .map_err(|_| self.error(AudioPlatformError::InvalidRequest))
    }
    /// Starts on the owner thread and joins every failed start cleanup.
    pub fn start(&mut self) -> Result<(), WasapiCompleteError> {
        self.check_live()?;
        if self.has_started {
            return if self.snapshot().status == AudioStreamStatus::Running {
                Ok(())
            } else {
                Err(self.error(AudioPlatformError::WorkerFailure))
            };
        }
        self.control.common.request.store(1, Ordering::Release);
        let signal_error = self.wake.signal().err();
        let acknowledgment = self.started.recv_timeout(StdDuration::from_secs(5));
        match acknowledgment {
            Ok(Ok(())) if signal_error.is_none() => {
                self.has_started = true;
                Ok(())
            }
            outcome => {
                let original = match outcome {
                    Ok(Err(code)) => {
                        self.failure.record_primary(WasapiSourcePhase::Start, code);
                        native(code)
                    }
                    _ => signal_error.unwrap_or(AudioPlatformError::WorkerFailure),
                };
                self.failure
                    .record_primary(WasapiSourcePhase::Start, platform_code(&original));
                let cleanup = self.stop();
                if let Err(error) = cleanup {
                    merge_public_cleanup(&mut self.failure, &error.report);
                }
                Err(self.error(original))
            }
        }
    }
    /// Stops and joins the sole native owner, retaining complete recovery even
    /// when the original operation or subsequent cleanup failed.
    pub fn stop(&mut self) -> Result<(), WasapiCompleteError> {
        let mut signal_failure = None;
        if let Some(worker) = self.worker.take() {
            self.control.common.request.store(2, Ordering::Release);
            let signal_error = self.wake.signal().err();
            match worker.join() {
                Ok(joined) => {
                    self.recovered = joined.state;
                    merge_report(&mut self.failure, joined.report);
                    self.retired = true;
                    if self.recovered.is_none() {
                        self.failure
                            .record_primary(WasapiSourcePhase::Recovery, E_FAIL_CODE);
                        self.failure.recovery.get_or_insert(E_FAIL_CODE);
                    }
                }
                Err(_) => {
                    self.failure
                        .record_primary(WasapiSourcePhase::Join, E_FAIL_CODE);
                    self.failure.join.get_or_insert(E_FAIL_CODE);
                    self.failure.recovery.get_or_insert(E_FAIL_CODE);
                    self.control.common.telemetry.status.store(
                        status_code(AudioStreamStatus::WorkerPanicked),
                        Ordering::SeqCst,
                    );
                }
            }
            if let Some(error) = signal_error {
                self.failure.stop.get_or_insert(platform_code(&error));
                self.failure
                    .record_primary(WasapiSourcePhase::Stop, platform_code(&error));
                signal_failure = Some(error);
            }
        }
        let report = self.report();
        if let Some(error) = signal_failure {
            return Err(WasapiCompleteError::new(error, report));
        }
        if let Some(primary) = report.primary {
            return Err(WasapiCompleteError::new(native(primary.code), report));
        }
        Ok(())
    }
    /// Moves the unique complete owner once, only after confirmed worker join.
    /// An uncertain-admission owner remains quarantined when returned.
    pub fn take_stopped_output_state(
        &mut self,
    ) -> Result<Option<NativeOutputState>, WasapiCompleteError> {
        if !self.retired || self.worker.is_some() {
            return Err(self.error(AudioPlatformError::RecoveryUnavailable));
        }
        Ok(self.recovered.take())
    }
    /// Reads the drained render-entry cadence after the sole writer has joined.
    pub fn render_cadence(
        &self,
    ) -> Result<
        Option<crate::audio::cadence::RenderCadence>,
        crate::audio::cadence::RenderCadenceError,
    > {
        if self.worker.is_some() {
            Ok(None)
        } else {
            self.control
                .common
                .cadence
                .summary(self.configuration.format.sample_rate())
                .map(Some)
        }
    }
}
impl beatkernel::audio::StoppedMixerSource<NativeOutputState> for WasapiCompleteStream {
    type Error = WasapiCompleteError;
    fn take_stopped_mixer(&mut self) -> Result<Option<NativeOutputState>, Self::Error> {
        self.take_stopped_output_state()
    }
}
impl Drop for WasapiCompleteStream {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(super) fn merge_report(into: &mut WasapiSourceFailureReport, from: WasapiSourceFailureReport) {
    if into.primary.is_none() {
        into.primary = from.primary;
    }
    for (into, from) in [
        (&mut into.release, from.release),
        (&mut into.stop, from.stop),
        (&mut into.start_cleanup, from.start_cleanup),
        (&mut into.join, from.join),
        (&mut into.recovery, from.recovery),
    ] {
        if into.is_none() {
            *into = from;
        }
    }
}
fn merge_public_cleanup(into: &mut WasapiSourceFailureReport, from: &WasapiCompleteFailureReport) {
    if into.release.is_none() {
        into.release = from.release;
    }
    if into.stop.is_none() {
        into.stop = from.stop;
    }
    if into.start_cleanup.is_none() {
        into.start_cleanup = from.start_cleanup;
    }
    if into.join.is_none() {
        into.join = from.join;
    }
    if into.recovery.is_none() {
        into.recovery = from.recovery;
    }
}
fn platform_code(error: &AudioPlatformError) -> i32 {
    match error {
        AudioPlatformError::Native { code } => *code,
        _ => E_FAIL_CODE,
    }
}

pub(super) struct NativeSourceOps<'a> {
    pub(super) client: &'a IAudioClient,
    pub(super) renderer: &'a IAudioRenderClient,
    pub(super) audio_clock: &'a IAudioClock,
    pub(super) clock: &'a QpcClock,
    pub(super) clock_frequency: u64,
    pub(super) snapshot: &'a mut AudioStreamSnapshot,
    pub(super) last_qpc: &'a mut Option<u64>,
    pub(super) period_duration: Duration,
    pub(super) running: bool,
    pub(super) cadence: Option<&'a crate::audio::cadence::Capture>,
    pub(super) telemetry: &'a Telemetry,
}
pub(super) struct NativeSourceLease<'a> {
    lease: BufferLease<'a>,
    pointer: *mut u8,
}
impl<'a> WasapiSourceOps for NativeSourceOps<'a> {
    type Lease = NativeSourceLease<'a>;
    fn available_frames(&mut self, buffer_frames: u32) -> Result<u32, i32> {
        let mut padding = 0;
        // SAFETY: owner-thread scalar query; no projected COM error allocation.
        let result =
            unsafe { (self.client.vtable().GetCurrentPadding)(self.client.as_raw(), &mut padding) };
        if result.is_err() {
            return Err(result.0);
        }
        if padding > buffer_frames {
            return Err(E_INVALIDARG_CODE);
        }
        self.snapshot.counters.padding_frames = padding;
        if self.running && padding == 0 && self.snapshot.counters.buffer_fills != 0 {
            self.snapshot.counters.inferred_starvations = self
                .snapshot
                .counters
                .inferred_starvations
                .saturating_add(1);
        }
        Ok(buffer_frames - padding)
    }
    fn acquire(&mut self, frames: u32) -> Result<Self::Lease, i32> {
        let mut pointer = std::ptr::null_mut();
        // SAFETY: requested frames fit the live native capacity; lease pairs release.
        let result = unsafe {
            (self.renderer.vtable().GetBuffer)(self.renderer.as_raw(), frames, &mut pointer)
        };
        if result.is_err() {
            return Err(result.0);
        }
        Ok(NativeSourceLease {
            lease: BufferLease {
                renderer: self.renderer,
                frames,
                released: false,
            },
            pointer,
        })
    }
    fn encode(
        &mut self,
        lease: &mut Self::Lease,
        format: DeviceFormat,
        samples: &[f32],
    ) -> Result<(), i32> {
        if lease.pointer.is_null() {
            return Err(E_INVALIDARG_CODE);
        }
        let bytes = (lease.lease.frames as usize)
            .checked_mul(usize::from(format.block_align()))
            .ok_or(E_INVALIDARG_CODE)?;
        // SAFETY: GetBuffer provides this exact complete-frame extent until release.
        let target = unsafe { std::slice::from_raw_parts_mut(lease.pointer, bytes) };
        encode_pcm(format, samples, target).map_err(|_| E_INVALIDARG_CODE)
    }
    fn release(&mut self, mut lease: Self::Lease, silent: bool) -> Result<(), i32> {
        let result = lease.lease.release(if silent {
            AUDCLNT_BUFFERFLAGS_SILENT.0 as u32
        } else {
            0
        });
        if result.is_err() {
            Err(result.0)
        } else {
            Ok(())
        }
    }
    fn history_request(
        &self,
    ) -> Option<crate::audio::source_history_retirement_mailbox::SourceHistoryRetirementRequest>
    {
        self.telemetry.source_retirement_request()
    }
    fn read_clock(&mut self) -> Result<(), i32> {
        read_clock_parts(
            self.audio_clock,
            self.clock,
            self.clock_frequency,
            self.snapshot,
            self.last_qpc,
            self.period_duration,
            self.running,
        )
    }
    fn render_pending(
        &mut self,
        state: &mut NativeOutputState,
        frames: usize,
    ) -> Result<RenderReport, i32> {
        let started = self
            .cadence
            .and_then(|_| self.clock.sample_realtime())
            .map(|sample| sample.normalized.timestamp);
        let report = state
            .render_pending(frames)
            .map_err(|_| E_INVALIDARG_CODE)?;
        if let Some(cadence) = self.cadence {
            match started {
                Some(at) => {
                    cadence.record(at, report.start_frame, report.frames as u64);
                }
                None => cadence.mark_unavailable(),
            }
        }
        Ok(report)
    }
    fn admitted(&mut self, report: RenderReport, frames: u32) {
        self.snapshot.render = Some(report);
        self.snapshot.counters.submitted_frames = self
            .snapshot
            .counters
            .submitted_frames
            .saturating_add(u64::from(frames));
        self.snapshot.counters.buffer_fills = self.snapshot.counters.buffer_fills.saturating_add(1);
    }
}

#[cfg(test)]
#[path = "source_conditional_retirement_fixtures.rs"]
mod fixtures;
