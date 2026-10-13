//! Unequal-rate ALSA lifecycle using the shared production PCM admission loop.
use super::converted_telemetry::ConvertedTelemetry;
use super::*;
use crate::audio::{ConvertedBoundaryFacts, ConvertedNativeOutputState};
use beatkernel::audio::{ConvertedRenderReport, StoppedMixerSource, TargetFrameBasis, TargetTime};

/// Original converted generation, positive target submission and optional
/// native timing from one worker publication. No epoch or pause ACK is granted.
#[derive(Clone, Copy, Debug)]
pub struct AlsaConvertedPlaybackSnapshot {
    /// Source playback and converted generation from the same worker publication.
    pub phase: ConvertedPlaybackPhaseSnapshot,
    /// Original optional native timing; absence grants no observation or pause ACK.
    pub timing: Option<AlsaTimingSnapshot>,
    /// Positively submitted target frames in this stream, never source frames.
    pub submitted_target_frames: u64,
    /// Outer TimingShared revision, distinct from the inner phase revision.
    pub revision: u64,
}

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
    retirement_completed: u64,
}
impl PumpOutput for ConvertedPump<'_> {
    type Report = ConvertedRenderReport;
    fn conditional_retirement(
        &self,
        control: &super::converted_conditional_retirement::ConditionalRetirement,
    ) -> PumpAction {
        control.worker_decision(self.state)
    }
    fn before_pending(&mut self) -> Result<(), AudioError> {
        let Some(request) = self.telemetry.retirement_request() else {
            return Ok(());
        };
        if request.revision <= self.retirement_completed {
            return Ok(());
        }
        let mut source_pin = request
            .source_pin
            .min(self.state.converter_owner().source_position().frame);
        let mut target_pin = request.target_pin;
        if let Some(pending) = self.state.pending_report() {
            source_pin = source_pin.min(pending.source_start_position.frame);
            if compare_retirement_time(pending.target_start_time, target_pin).is_lt() {
                target_pin = pending.target_start_time;
            }
        }
        let (practice_source, practice_target) = self.state.practice_history_retention_pins();
        if let Some(pin) = practice_source {
            source_pin = source_pin.min(pin);
        }
        if let Some(pin) = practice_target {
            if compare_retirement_time(pin, target_pin).is_lt() {
                target_pin = pin;
            }
        }
        let (source_removed, phase_removed) =
            self.state.retire_histories_before(source_pin, target_pin)?;
        if source_removed != 0 || phase_removed != 0 {
            self.telemetry.publish_generated(None, self.state);
        }
        if source_pin == request.source_pin
            && compare_retirement_time(target_pin, request.target_pin).is_eq()
        {
            self.retirement_completed = request.revision;
        }
        Ok(())
    }
    fn render_pending(&mut self, frames: usize) -> Result<Self::Report, AudioError> {
        let result = if self.telemetry.held() {
            self.state.render_held_pending(frames)
        } else {
            self.state.render_pending(frames)
        };
        if self.telemetry.phase_enabled() {
            self.telemetry
                .publish_generated(result.as_ref().ok().copied(), self.state);
        }
        result
    }
    fn pending_frames(&self) -> usize {
        self.state.pending_frames()
    }
    fn pending_samples(&self) -> &[f32] {
        self.state.pending_samples()
    }
    fn admit(&mut self, frames: usize) -> Result<(), AudioError> {
        self.state.admit(frames)?;
        if let Some(time) = self.state.admitted_target_time() {
            self.telemetry.publish_submitted(time);
        }
        Ok(())
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
    let mut output = ConvertedPump {
        state,
        telemetry,
        retirement_completed: 0,
    };
    run_output_worker(
        pcm,
        &mut output,
        config,
        conversion,
        shared,
        clock,
        encoder,
        |report, state, _| {
            if let Some(report) = report.filter(|_| !telemetry.phase_enabled()) {
                telemetry.publish(
                    report,
                    state.state.boundaries(),
                    state.state.last_real_source_report(),
                )
            }
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
        let telemetry = match ConvertedTelemetry::with_phase_history(&state, basis) {
            Ok(telemetry) => Arc::new(telemetry),
            Err(error) => {
                return Err(MixerOpenFailure::new_state(
                    LinuxError::Mixer(error),
                    Some(state),
                ))
            }
        };
        let shared = Arc::new(Shared {
            converted_source: Some(
                super::converted_conditional_retirement::ConvertedSourceFacts::from_state(&state),
            ),
            ..Shared::with_converted_history(&telemetry)
        });
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
    /// Validates a replacement's immutable source and requested target facts cold.
    /// This does not prove that the live worker's pending suffix is compatible.
    pub fn validate_conditional_replacement(
        &self,
        request: &AlsaRequest,
        matrix: &ChannelMatrix,
    ) -> Result<(), LinuxError> {
        self.shared
            .converted_source
            .ok_or(LinuxError::InvalidLifecycle)?
            .validate(request, matrix)
    }
    fn require_conditional_live(&self) -> Result<(), LinuxError> {
        if self.shared.stop.load(Ordering::Acquire)
            || self.shared.status.load(Ordering::Acquire) != 1
            || self.worker.as_ref().is_none_or(JoinHandle::is_finished)
        {
            return Err(LinuxError::InvalidLifecycle);
        }
        Ok(())
    }
    fn require_conditional_response(&self, nonce: u64) -> Result<(), LinuxError> {
        if self.shared.conditional_retirement.status(nonce)?
            != ConvertedRetirementStatus::Authorized
        {
            if let Err(error) = self.require_conditional_live() {
                // Authorization can win between the first status read and stop.
                if self.shared.conditional_retirement.status(nonce)?
                    != ConvertedRetirementStatus::Authorized
                {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    /// Queues one policy for the sole worker's actual pending-state decision.
    /// Busy or unconsumed terminal responses return None without replacing them.
    pub fn submit_conditional_retirement(
        &mut self,
        policy: ConvertedRetirementPolicy,
    ) -> Result<Option<ConvertedRetirementTicket>, LinuxError> {
        self.require_conditional_live()?;
        self.shared
            .conditional_retirement
            .submit(policy)
            .map(|nonce| nonce.map(|nonce| ConvertedRetirementTicket::new(&self.shared, nonce)))
    }
    /// Consumes matching live refusal/cancellation; authorization stays terminal.
    /// Authorization grants no completed native drop/join or recovered owner.
    pub fn poll_conditional_retirement(
        &mut self,
        ticket: &ConvertedRetirementTicket,
    ) -> Result<ConvertedRetirementStatus, LinuxError> {
        let nonce = ticket.nonce_for(&self.shared)?;
        self.require_conditional_response(nonce)?;
        self.shared.conditional_retirement.poll(nonce)
    }
    /// Cancels only while this same request is still pending; never undoes authorization.
    /// The cancellation response remains queued until a matching poll consumes it.
    pub fn cancel_conditional_retirement(
        &mut self,
        ticket: &ConvertedRetirementTicket,
    ) -> Result<ConvertedRetirementStatus, LinuxError> {
        let nonce = ticket.nonce_for(&self.shared)?;
        self.require_conditional_response(nonce)?;
        self.shared.conditional_retirement.cancel(nonce)
    }
    /// Queues caller-qualified mathematical history pins on this live stream.
    /// A returned revision is not a native pause ACK or a retirement receipt.
    pub fn request_history_retirement(
        &mut self,
        expected_basis: TargetFrameBasis,
        source_identity: (u32, u64, u64),
        source_pin: u64,
        target_pin: TargetTime,
    ) -> Result<Option<u64>, LinuxError> {
        if self.shared.stop.load(Ordering::Acquire) || self.worker.is_none() {
            return Err(LinuxError::InvalidLifecycle);
        }
        self.telemetry
            .request_history_retirement(expected_basis, source_identity, source_pin, target_pin)
            .map_err(LinuxError::Mixer)
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
    /// Exact generated correspondence within this stream's actual positive
    /// submission prefix. This proves neither native presentation nor epoch
    /// authentication; callers must qualify those separately.
    pub fn phase_snapshot_at(
        &self,
        query: beatkernel::audio::TargetTime,
    ) -> Option<super::ConvertedPhaseSnapshot> {
        self.telemetry.phase_snapshot_at(query)
    }
    /// Exact interpolated generated correspondence within this stream's
    /// positive submission prefix. Native presentation and epoch
    /// authentication require separate qualification.
    pub fn phase_snapshot_at_interpolated_time(
        &self,
        query: &beatkernel::audio::InterpolatedTargetTime,
    ) -> Option<super::ConvertedPhaseSnapshot> {
        self.telemetry.phase_snapshot_at_interpolated_time(query)
    }
    /// Coherent source playback adoption qualified by generated correspondence
    /// and this stream's positive software submission prefix.
    /// This does not authenticate native presentation or a pause acknowledgment.
    pub fn playback_phase_snapshot_at_interpolated_time(
        &self,
        query: &beatkernel::audio::InterpolatedTargetTime,
    ) -> Option<super::ConvertedPlaybackPhaseSnapshot> {
        self.telemetry
            .playback_phase_snapshot_at_interpolated_time(query)
    }

    /// Bounded read of the original native publication and explicit cold
    /// source-history identity. Missing timing supplies no native observation.
    pub fn native_playback_at(
        &self,
        query: &InterpolatedTargetTime,
        source_rate: u32,
        initial_physical: u64,
        initial_playback: u64,
    ) -> Option<AlsaConvertedPlaybackSnapshot> {
        shared_converted_playback_with(
            &self.shared,
            &self.telemetry,
            self.basis,
            self.configuration.requested.monotonic_domain,
            query,
            source_rate,
            initial_physical,
            initial_playback,
            |_| {},
        )
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

pub(super) fn shared_converted_playback_with(
    shared: &Shared,
    telemetry: &ConvertedTelemetry,
    basis: TargetFrameBasis,
    domain: ClockDomainId,
    query: &InterpolatedTargetTime,
    source_rate: u32,
    initial_physical: u64,
    initial_playback: u64,
    mut before_recheck: impl FnMut(usize),
) -> Option<AlsaConvertedPlaybackSnapshot> {
    if !shared.converted_phase_enabled {
        return None;
    }
    for attempt in 0..3 {
        let revision = shared.timing.sequence();
        if revision & 1 != 0 {
            continue;
        }
        if shared.stop.load(Ordering::SeqCst) || shared.status.load(Ordering::SeqCst) != 1 {
            return None;
        }
        let submitted = shared.submitted.load(Ordering::SeqCst);
        let phase = telemetry.playback_phase_snapshot_at_interpolated_time_with_identity(
            query,
            source_rate,
            initial_physical,
            initial_playback,
        );
        let timing = shared.timing.read_unversioned(domain);
        let selected = (|| {
            let phase = phase?;
            if submitted == 0
                || phase.phase.stream_start != basis.start_time()
                || phase.phase.submitted_end != basis.time_at_stream_frame(submitted).ok()?
                || timing.is_some_and(|timing| timing.submitted_frames != submitted)
            {
                return None;
            }
            Some(AlsaConvertedPlaybackSnapshot {
                phase,
                timing,
                submitted_target_frames: submitted,
                revision,
            })
        })();
        before_recheck(attempt);
        if shared.timing.sequence() == revision {
            if shared.stop.load(Ordering::SeqCst) || shared.status.load(Ordering::SeqCst) != 1 {
                return None;
            }
            return selected;
        }
    }
    None
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

#[cfg(test)]
#[path = "converted_native_playback_fixtures.rs"]
mod native_playback_fixtures;

fn compare_retirement_time(left: TargetTime, right: TargetTime) -> std::cmp::Ordering {
    left.seconds().cmp(&right.seconds()).then_with(|| {
        (u128::from(left.numerator()) * u128::from(right.denominator()))
            .cmp(&(u128::from(right.numerator()) * u128::from(left.denominator())))
    })
}

#[cfg(test)]
#[path = "converted_retirement_worker_fixtures.rs"]
mod retirement_worker_fixtures;

#[cfg(test)]
#[path = "converted_practice_retirement_worker_fixtures.rs"]
mod practice_retirement_worker_fixtures;
