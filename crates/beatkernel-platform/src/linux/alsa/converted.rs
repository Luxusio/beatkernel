//! Unequal-rate ALSA lifecycle using the shared production PCM admission loop.
use super::converted_telemetry::ConvertedTelemetry;
use super::*;
use crate::audio::{ConvertedBoundaryFacts, ConvertedNativeOutputState};
use beatkernel::audio::{ConvertedRenderReport, StoppedMixerSource, TargetFrameBasis};

pub(super) type ConvertedWorkerExit = (Result<(), LinuxError>, ConvertedNativeOutputState);
impl WorkerSpawner<ConvertedWorkerExit> for NativeWorkerSpawner {
    fn spawn<F>(self, work: F) -> io::Result<JoinHandle<ConvertedWorkerExit>>
    where
        F: FnOnce() -> ConvertedWorkerExit + Send + 'static,
    {
        thread::Builder::new()
            .name("beatkernel-alsa-converted".into())
            .spawn(work)
    }
}
struct ConvertedPump<'a> {
    state: &'a mut ConvertedNativeOutputState,
    telemetry: &'a ConvertedTelemetry,
}
impl PumpOutput for ConvertedPump<'_> {
    type Report = ConvertedRenderReport;
    fn render_pending(&mut self, frames: usize) -> Result<Self::Report, AudioError> {
        if self.telemetry.held() {
            self.state.render_held_pending(frames)
        } else {
            self.state.render_pending(frames)
        }
    }
    fn pending_frames(&self) -> usize {
        self.state.pending_frames()
    }
    fn pending_samples(&self) -> &[f32] {
        self.state.pending_samples()
    }
    fn admit(&mut self, frames: usize) -> Result<(), AudioError> {
        self.state.admit(frames)
    }
}
pub(super) fn run_converted_worker<P: PcmOperations, C: WorkerClock, E: PcmEncoder>(
    pcm: &mut P,
    state: &mut ConvertedNativeOutputState,
    config: &AlsaAppliedConfig,
    conversion: &mut [u8],
    shared: &Shared,
    clock: &C,
    encoder: &mut E,
    telemetry: &ConvertedTelemetry,
) -> Result<(), LinuxError> {
    let mut output = ConvertedPump { state, telemetry };
    run_output_worker(
        pcm,
        &mut output,
        config,
        conversion,
        shared,
        clock,
        encoder,
        |report, state, _| {
            telemetry.publish(
                report,
                state.state.boundaries(),
                state.state.last_real_source_report(),
            )
        },
    )
}

/// Dedicated native worker whose recovered software owner includes exact conversion time.
/// Native admitted-but-unheard frames are not reconstructed on retirement.
pub struct ConvertedAlsaStream {
    pub(super) configuration: AlsaAppliedConfig,
    pub(super) basis: TargetFrameBasis,
    pub(super) shared: Arc<Shared>,
    pub(super) telemetry: Arc<ConvertedTelemetry>,
    pub(super) worker: Option<JoinHandle<ConvertedWorkerExit>>,
    pub(super) recovered_output: Option<ConvertedNativeOutputState>,
    pub(super) retired: bool,
}
impl ConvertedAlsaStream {
    /// Opens actual ALSA at the configured target rate, retaining full state on refusal.
    pub fn open_recoverable(
        request: AlsaRequest,
        state: ConvertedNativeOutputState,
        matrix: ChannelMatrix,
    ) -> Result<Self, MixerOpenFailure<LinuxError, ConvertedNativeOutputState>> {
        let validation = (|| {
            super::super::sys::supported_abi()?;
            if request.device.is_empty()
                || request.device.contains('\0')
                || request.period_frames == 0
                || request.buffer_frames <= request.period_frames
                || request.format.channel_mask().is_some()
            {
                return Err(LinuxError::InvalidConfiguration(
                    "explicit device, unspecified channel mask, and 0 < period < buffer required",
                ));
            }
            state
                .validate_reconfigure(request.format, &matrix, request.period_frames as usize)
                .map_err(LinuxError::Mixer)
        })();
        if let Err(error) = validation {
            return Err(MixerOpenFailure::new_state(error, Some(state)));
        }
        // The requested interpretation may change only without pending old output.
        let basis = match TargetFrameBasis::new(
            state.target_frame_basis().origin(),
            state.target_frame_basis().start_time(),
            request.format.sample_rate(),
        ) {
            Ok(basis) => basis,
            Err(error) => {
                return Err(MixerOpenFailure::new_state(
                    LinuxError::Mixer(error),
                    Some(state),
                ))
            }
        };
        let shared = Arc::new(Shared::new());
        let telemetry = Arc::new(ConvertedTelemetry::new());
        telemetry.seed(state.boundaries(), state.last_real_source_report());
        let worker_shared = Arc::clone(&shared);
        let worker_telemetry = Arc::clone(&telemetry);
        let (sender, receiver) = mpsc::sync_channel(1);
        let launched = crate::audio::mixer_launch::launch_worker(
            NativeWorkerSpawner,
            state,
            move |mut state| {
                let opened = NativePcm::open(&request).and_then(|(pcm, buffer, period)| {
                    state
                        .validate_reconfigure(request.format, &matrix, period as usize)
                        .map_err(LinuxError::Mixer)?;
                    let bytes = (period as usize)
                        .checked_mul(usize::from(request.format.block_align()))
                        .ok_or(LinuxError::Overflow)?;
                    let mut conversion = Vec::new();
                    conversion
                        .try_reserve_exact(bytes)
                        .map_err(|_| LinuxError::Mixer(AudioError::AllocationFailed))?;
                    conversion.resize(bytes, 0);
                    state
                        .reconfigure(request.format, matrix, period as usize)
                        .map_err(LinuxError::Mixer)?;
                    let configuration = AlsaAppliedConfig {
                        format: request.format,
                        buffer_frames: buffer,
                        period_frames: period,
                        sizing_adjusted: period != request.period_frames
                            || buffer != request.buffer_frames,
                        output_domain: state.mixer().config().domain(),
                        output_origin: state.mixer().config().origin(),
                        requested: request.clone(),
                    };
                    Ok((pcm, configuration, conversion))
                });
                let (mut pcm, configuration, mut conversion) = match opened {
                    Ok(opened) => opened,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        return (Ok(()), state);
                    }
                };
                if sender.send(Ok(configuration.clone())).is_err() {
                    return (Ok(()), state);
                }
                drop(sender);
                while !worker_shared.start.load(Ordering::Acquire)
                    && !worker_shared.stop.load(Ordering::Acquire)
                {
                    thread::park();
                }
                if worker_shared.stop.load(Ordering::Acquire) {
                    return (Ok(()), state);
                }
                worker_shared.status.store(1, Ordering::Release);
                let _guard = TimingInvalidation(&worker_shared.timing);
                let result = run_converted_worker(
                    &mut pcm,
                    &mut state,
                    &configuration,
                    &mut conversion,
                    &worker_shared,
                    &MonotonicClock::new(configuration.requested.monotonic_domain),
                    &mut NativeEncoder,
                    &worker_telemetry,
                );
                match &result {
                    Ok(()) => worker_shared.status.store(2, Ordering::Release),
                    Err(error) => {
                        let code = match error {
                            LinuxError::Alsa { code, .. } => *code,
                            _ => 0,
                        };
                        if code == -32 {
                            increment(&worker_shared.xruns, 1);
                        }
                        if code == -86 {
                            increment(&worker_shared.suspends, 1);
                        }
                        increment(&worker_shared.failures, 1);
                        worker_shared.errno.store(code, Ordering::Relaxed);
                        worker_shared.status.store(3, Ordering::Release);
                    }
                }
                (result, state)
            },
        );
        let worker = match launched {
            Ok(worker) => worker,
            Err(failure) => {
                let (error, state) = failure.into_parts();
                return Err(MixerOpenFailure::new_state(error.into(), state));
            }
        };
        match receiver.recv() {
            Ok(Ok(configuration)) => Ok(Self {
                configuration,
                basis,
                shared,
                telemetry,
                worker: Some(worker),
                recovered_output: None,
                retired: false,
            }),
            Ok(Err(error)) => Err(crate::audio::mixer_launch::join_open_failure(
                worker,
                error,
                |(_, state)| Some(state),
            )),
            Err(_) => Err(crate::audio::mixer_launch::join_open_failure(
                worker,
                LinuxError::WorkerPanicked,
                |(_, state)| Some(state),
            )),
        }
    }
    /// Exact first-unsubmitted physical duration for new stream-relative counters.
    pub const fn frame_basis(&self) -> TargetFrameBasis {
        self.basis
    }
    /// Actual native target settings, separate from the source Mixer format.
    pub const fn configuration(&self) -> &AlsaAppliedConfig {
        &self.configuration
    }
    /// Starts the prepared worker once.
    pub fn start(&mut self) -> Result<(), LinuxError> {
        let worker = self.worker.as_ref().ok_or(LinuxError::InvalidLifecycle)?;
        if self.shared.stop.load(Ordering::Acquire)
            || self.shared.start.swap(true, Ordering::AcqRel)
        {
            return Err(LinuxError::InvalidLifecycle);
        }
        worker.thread().unpark();
        Ok(())
    }
    /// Selects explicit target-held silence for subsequent fresh blocks.
    /// This adopts no source pause and proves no native presentation acknowledgment.
    pub fn set_held(&self, held: bool) -> Result<(), LinuxError> {
        if self.shared.stop.load(Ordering::Acquire) || self.worker.is_none() {
            return Err(LinuxError::InvalidLifecycle);
        }
        self.telemetry.set_held(held);
        Ok(())
    }
    /// Joins before exposing software recovery, even if the worker returns an error.
    pub fn stop(&mut self) -> Result<(), LinuxError> {
        self.shared.stop.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker.thread().unpark();
        match worker.join() {
            Ok((result, state)) => {
                self.recovered_output = Some(state);
                self.retired = true;
                self.shared.timing.invalidate();
                if result.is_ok() {
                    self.shared.status.store(2, Ordering::Release);
                }
                result
            }
            Err(_) => {
                self.shared.timing.invalidate();
                self.shared.status.store(4, Ordering::Release);
                Err(LinuxError::WorkerPanicked)
            }
        }
    }
    /// Moves the complete owner at most once after confirmed retirement.
    pub fn take_stopped_output(
        &mut self,
    ) -> Result<Option<ConvertedNativeOutputState>, LinuxError> {
        if !self.retired || self.worker.is_some() {
            return Err(LinuxError::InvalidLifecycle);
        }
        Ok(self.recovered_output.take())
    }
    /// Coherent auxiliary source/target facts; absence preserves caller history.
    pub fn output_telemetry(
        &self,
    ) -> Option<(
        Option<RenderReport>,
        Option<ConvertedRenderReport>,
        ConvertedBoundaryFacts,
    )> {
        self.telemetry.output_telemetry()
    }
    /// Last fresh generated target report; source facts alone never acknowledge native pause/end.
    pub fn last_render_report(&self) -> Option<ConvertedRenderReport> {
        self.telemetry.read().map(|(report, _)| report)
    }
    /// Last real nonempty source callback, retained through empty and held target output.
    pub fn last_real_source_report(&self) -> Option<RenderReport> {
        self.telemetry.last_real_source_report()
    }
    /// Persisted mapped source boundaries on this stream's immutable origin association.
    pub fn boundary_facts(&self) -> ConvertedBoundaryFacts {
        self.telemetry.boundaries()
    }
    /// Actual native played-frame estimate, unavailable outside the running epoch.
    pub fn timing_snapshot(&self) -> Option<AlsaTimingSnapshot> {
        if self.shared.stop.load(Ordering::SeqCst) || self.shared.status.load(Ordering::SeqCst) != 1
        {
            return None;
        }
        let value = self
            .shared
            .timing
            .snapshot(self.configuration.requested.monotonic_domain);
        if self.shared.stop.load(Ordering::SeqCst) || self.shared.status.load(Ordering::SeqCst) != 1
        {
            return None;
        }
        value
    }
    /// Native target-frame counters and lifecycle status, never source frame counts.
    pub fn snapshot(&self) -> AlsaSnapshot {
        shared_snapshot(&self.shared, self.configuration.requested.monotonic_domain)
    }
}
impl StoppedMixerSource<ConvertedNativeOutputState> for ConvertedAlsaStream {
    type Error = LinuxError;
    fn take_stopped_mixer(&mut self) -> Result<Option<ConvertedNativeOutputState>, LinuxError> {
        self.take_stopped_output()
    }
}
impl Drop for ConvertedAlsaStream {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
