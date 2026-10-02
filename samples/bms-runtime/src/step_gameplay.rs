//! Nonblocking control-side ownership of the actual solo runtime and BGM queue.
//! Hosts supply clock relations and genuine completed-render cursors. This
//! module does not sample a platform clock or infer delivery from admission.
use crate::{
    PreparedBms,
    bgm::{BgmConfig, BgmFeedError, BgmFeedReport, BgmFeeder},
    competition::{CompetitionError, ScoreSummary},
    completion::{CompletionError, SongCompletion},
    local_runtime::{GroupError, SoloRuntime},
    native_judge::NativeJudgeConfig,
    replay_capture::{CaptureError, LiveReplayCapture},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioCounters, AudioLimits, CommandConsumer, QueuePopError, RenderReport,
        SampleBank, command_queue,
    },
    input::{BindingMap, PhysicalInputEvent},
    judge::JudgeEngine,
    replay::codec::ReplayCodecLimits,
    runtime::{RuntimeProcessingClock, RuntimeReport},
    time::{ClockDomainId, ClockMapper, ClockPair, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::presentation::discipline::{
    DisciplineConfig, DisciplineError, DisciplineUpdate, ObservationAdmission,
    PresentationDiscipline,
};
use std::{error::Error, fmt};

/// Explicit mapping and bounded control-side resources for a fresh solo run.
#[derive(Clone, Copy, Debug)]
pub struct StepGameplayConfig {
    /// Host instant at which the song position is exactly negative preroll.
    pub host_origin: ClockPoint,
    /// Frame zero of the separate output grid; browser mixers use timestamp zero.
    pub output_origin: ClockPoint,
    pub preroll: Duration,
    pub early_ns: i64,
    pub late_ns: i64,
    pub offset_ns: i64,
    pub command_capacity: usize,
    /// Positive BGM credit strictly below command_capacity, reserving input space.
    pub bgm_pending: usize,
    pub bgm_lookahead: Duration,
    pub telemetry_capacity: usize,
}

/// One immutable admission attempt. A failed prefix must never be retried.
#[derive(Clone, Debug, PartialEq)]
pub struct StepAudioBatch {
    pub sequence: u64,
    pub commands: Vec<AudioCommand>,
}

/// Actual failure evidence is retained independently from the owner's fence.
#[derive(Debug)]
pub enum StepGameplayError {
    /// Public bounds failed before gameplay or queue mutation.
    InvalidConfiguration(&'static str),
    Setup(String),
    Failed,
    /// Snapshot extraction alone is busy; judging and BGM admission may continue.
    OutstandingBatch {
        sequence: u64,
    },
    /// Batch buffers could not be reserved; no commands were consumed.
    AllocationFailed,
    SequenceOverflow,
    Runtime(GroupError),
    /// Original clock discipline failure; committed judgments remain retained.
    Clock(DisciplineError),
    /// The report's judgments remain committed even when audio admission failed.
    /// Score aggregation is atomic; its error preserves the previous score.
    Report {
        report: RuntimeReport,
        score_error: Option<CompetitionError>,
        capture_error: Option<CaptureError>,
    },
    /// Capture rejected a committed operation, or setup/export failed without
    /// an operation. Earlier recorded operations remain the accepted prefix.
    Capture {
        error: CaptureError,
        report: Option<RuntimeReport>,
    },
    Bgm {
        error: BgmFeedError,
        report: BgmFeedReport,
    },
    /// Original output evidence is retained; invalid evidence fences the owner.
    Completion {
        error: CompletionError,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    },
    Queue {
        error: QueuePopError,
        commands: Vec<AudioCommand>,
    },
    /// A valid remote rejection preserves the exact batch and admitted prefix.
    AudioRejected {
        batch: StepAudioBatch,
        admitted: usize,
    },
    /// Received fields are untrusted; admitted is not validated prefix evidence.
    InvalidAcknowledgement {
        sequence: u64,
        admitted: usize,
        success: bool,
        batch: Option<StepAudioBatch>,
    },
}

impl fmt::Display for StepGameplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(f, "step gameplay configuration: {message}")
            }
            Self::Setup(message) => write!(f, "step gameplay setup: {message}"),
            Self::Failed => f.write_str("step gameplay is fenced"),
            Self::OutstandingBatch { sequence } => {
                write!(f, "audio batch {sequence} awaits acknowledgement")
            }
            Self::AllocationFailed => {
                f.write_str("audio batch allocation failed before queue consumption")
            }
            Self::SequenceOverflow => f.write_str("audio batch sequence exhausted"),
            Self::Runtime(error) => write!(f, "step gameplay: {error}"),
            Self::Clock(error) => write!(f, "step gameplay: {error}"),
            Self::Report {
                report,
                score_error,
                capture_error,
            } => write!(
                f,
                "step gameplay report failed: judge {:?}, {} audio failures, score {:?}, capture {:?}",
                report.judge_error,
                report.audio_failures.len(),
                score_error,
                capture_error
            ),
            Self::Capture { error, .. } => write!(f, "step gameplay capture: {error}"),
            Self::Bgm { error, report } => write!(
                f,
                "{error}; {} BGM admissions remain committed",
                report.total_admitted
            ),
            Self::Completion { error, .. } => write!(f, "step gameplay completion: {error}"),
            Self::Queue { error, commands } => write!(
                f,
                "audio queue {error:?} after {} consumed commands",
                commands.len()
            ),
            Self::AudioRejected { batch, admitted } => write!(
                f,
                "audio batch {} rejected after {admitted}/{} commands",
                batch.sequence,
                batch.commands.len()
            ),
            Self::InvalidAcknowledgement {
                sequence,
                admitted,
                success,
                batch,
            } => write!(
                f,
                "invalid audio acknowledgement {sequence} admitted {admitted} success {success}, expected {:?}",
                batch.as_ref().map(|batch| batch.sequence)
            ),
        }
    }
}
impl Error for StepGameplayError {}

/// Sole runtime producer and queue consumer, outside every audio callback.
///
/// The caller owns the returned PCM bank and the remote output lifecycle. This
/// owner never treats a command ACK, a UI tick or failure as playback completion.
pub struct StepGameplay {
    runtime: SoloRuntime,
    consumer: CommandConsumer,
    bgm: BgmFeeder,
    completion: SongCompletion,
    output_origin: ClockPoint,
    sample_rate: u32,
    last_render: Option<RenderReport>,
    last_presented: Option<Timestamp>,
    output_clock: Option<PresentationDiscipline>,
    correction_watermark: Option<ClockPoint>,
    capture: Option<LiveReplayCapture>,
    score: ScoreSummary,
    song: Timestamp,
    host_domain: ClockDomainId,
    preroll: Duration,
    activated: bool,
    started: bool,
    pending: Option<StepAudioBatch>,
    sequence: u64,
    failed: bool,
}

impl fmt::Debug for StepGameplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepGameplay")
            .field("song", &self.song)
            .field("score", &self.score)
            .field(
                "pending_sequence",
                &self.pending.as_ref().map(|batch| batch.sequence),
            )
            .field("failed", &self.failed)
            .field("activated", &self.activated)
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

impl StepGameplay {
    pub fn new(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        if config.host_origin.domain == config.output_origin.domain {
            return Err(StepGameplayError::InvalidConfiguration(
                "host and output domains must be distinct",
            ));
        }
        if config.preroll.as_nanos() < 0 || config.bgm_lookahead.as_nanos() <= 0 {
            return Err(StepGameplayError::InvalidConfiguration(
                "nonnegative preroll and positive BGM lookahead required",
            ));
        }
        if !(1..=AudioLimits::MAX_COMMANDS).contains(&config.command_capacity)
            || config.bgm_pending == 0
            || config.bgm_pending >= config.command_capacity
            || config.telemetry_capacity > 65_536
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "invalid command, BGM or telemetry capacity",
            ));
        }
        let song = Timestamp::ZERO.checked_sub(config.preroll).ok_or(
            StepGameplayError::InvalidConfiguration("preroll cannot be represented"),
        )?;
        if config
            .host_origin
            .timestamp
            .checked_add(config.preroll)
            .is_none()
            || config
                .output_origin
                .timestamp
                .checked_add(config.preroll)
                .is_none()
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "song zero exceeds the host or output clock range",
            ));
        }
        let profile = NativeJudgeConfig {
            early: config.early_ns,
            late: config.late_ns,
            offset: config.offset_ns,
            preroll: config.preroll.as_nanos(),
            output: config.output_origin.domain,
            end: None,
        }
        .profile()
        .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
        let completion = SongCompletion::prepare(
            &prepared,
            config.late_ns,
            config.offset_ns,
            config.preroll.as_nanos(),
            config.output_origin.domain,
        )
        .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
        let sample_rate = prepared.bank.format().sample_rate();
        let rules = prepared.source.rules();
        let judge = JudgeEngine::new(prepared.compiled.chart, rules, profile)
            .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
        let bgm_count = prepared.bgm_commands.len();
        let bgm = BgmFeeder::new(
            prepared.bgm_commands,
            BgmConfig {
                output_origin: config.output_origin,
                sample_rate: prepared.bank.format().sample_rate(),
                preroll: config.preroll,
                lookahead: config.bgm_lookahead,
                max_pending: config.bgm_pending,
            },
        )
        .map_err(|error| StepGameplayError::Bgm {
            error,
            report: BgmFeedReport {
                remaining: bgm_count,
                ..BgmFeedReport::default()
            },
        })?;
        let (producer, consumer) = command_queue(config.command_capacity)
            .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
        let mut runtime = SoloRuntime::new(
            config.host_origin.domain,
            config.output_origin.domain,
            Transport::new(config.host_origin.timestamp, song, Rate::NORMAL),
            bindings,
            judge,
            producer,
            prepared.sounds,
            config.telemetry_capacity,
        )
        .map_err(StepGameplayError::Setup)?;
        // Profiling is not a clock source. In particular, no std::time::Instant
        // is sampled by core processing on browser or other nonblocking hosts.
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let mut owner = Self {
            runtime,
            consumer,
            bgm,
            completion,
            output_origin: config.output_origin,
            sample_rate,
            last_render: None,
            last_presented: None,
            output_clock: None,
            correction_watermark: None,
            capture: None,
            score: ScoreSummary::default(),
            song,
            host_domain: config.host_origin.domain,
            preroll: config.preroll,
            activated: false,
            started: false,
            pending: None,
            sequence: 0,
            failed: false,
        };
        owner.feed_audio(0, config.bgm_pending)?;
        Ok((owner, prepared.bank))
    }

    fn ensure_usable(&self) -> Result<(), StepGameplayError> {
        if self.failed {
            Err(StepGameplayError::Failed)
        } else {
            Ok(())
        }
    }

    /// Opt in while the original judge is pristine, using the resolved chart
    /// branch seed. Setup refusal is atomic and leaves this owner usable.
    /// Preroll reports retain their actual negative song times; the chart's
    /// original-song section start remains zero in the existing replay header.
    pub fn configure_capture(
        &mut self,
        limits: ReplayCodecLimits,
        chart_seed: u64,
    ) -> Result<(), StepGameplayError> {
        self.ensure_usable()?;
        if self.started || self.capture.is_some() {
            return Err(StepGameplayError::InvalidConfiguration(
                "capture configuration requires an unprocessed, unconfigured runtime",
            ));
        }
        let capture = LiveReplayCapture::new_at_with_chart_seed(
            self.runtime.judge(),
            self.host_domain,
            limits,
            Timestamp::ZERO,
            chart_seed,
        )
        .map_err(|error| StepGameplayError::Capture {
            error,
            report: None,
        })?;
        self.capture = Some(capture);
        Ok(())
    }

    /// Take the canonical accepted prefix once, only after stop/failure fenced
    /// further gameplay. Disabled or already consumed capture returns None.
    /// Encoding failure also consumes this export attempt; no operations retry.
    pub fn take_replay(&mut self) -> Result<Option<Vec<u8>>, StepGameplayError> {
        if !self.failed {
            return Err(StepGameplayError::InvalidConfiguration(
                "replay export requires a stopped or fenced runtime",
            ));
        }
        self.capture
            .take()
            .map(LiveReplayCapture::into_bytes)
            .transpose()
            .map_err(|error| StepGameplayError::Capture {
                error,
                report: None,
            })
    }

    /// Opt in before any processing. Configuration reserves the existing
    /// discipline's bounded storage; its accuracy remains Unknown. Atomic setup
    /// refusal leaves this owner usable and does not alter its nominal transport.
    pub fn configure_output_clock(
        &mut self,
        config: DisciplineConfig,
    ) -> Result<(), StepGameplayError> {
        self.ensure_usable()?;
        if self.started || self.output_clock.is_some() {
            return Err(StepGameplayError::InvalidConfiguration(
                "output clock configuration requires an unprocessed, unconfigured runtime",
            ));
        }
        let discipline =
            PresentationDiscipline::new(config, self.output_origin, self.host_domain, self.song)
                .map_err(StepGameplayError::Clock)?;
        self.output_clock = Some(discipline);
        Ok(())
    }

    /// Admit an actual absolute output/host relation after activation, without
    /// changing transport or input timestamps. A measured host point may be
    /// earlier than the estimated activation instant; native warmup/history
    /// checks remain authoritative. Unchanged output cannot refresh freshness.
    pub fn observe_output_clock(
        &mut self,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, StepGameplayError> {
        self.ensure_usable()?;
        if !self.activated || self.output_clock.is_none() {
            return Err(self.clock_failure(DisciplineError::InvalidConfig));
        }
        if pair.source.domain != self.output_origin.domain || pair.target.domain != self.host_domain
        {
            return Err(self.clock_failure(DisciplineError::DomainMismatch));
        }
        if pair.source.timestamp < self.output_origin.timestamp {
            return Err(self.clock_failure(DisciplineError::NonIncreasing));
        }
        let result = self
            .output_clock
            .as_mut()
            .expect("configured discipline was checked")
            .observe_clock_pair(pair);
        result.map_err(|error| self.clock_failure(error))
    }

    /// Apply continuous correction only at the latest successfully accepted
    /// original host watermark, after its complete input prefix. Any later input
    /// invalidates that permission until another successful advance. Missing or
    /// stale observations preserve the current transport history without a clock
    /// substitution; all other observation/update faults fence this owner.
    pub fn update_output_clock(
        &mut self,
        host: ClockPoint,
    ) -> Result<Option<DisciplineUpdate>, StepGameplayError> {
        self.ensure_usable()?;
        if self.output_clock.is_none() {
            return Ok(None);
        }
        if !self.activated {
            return Err(self.clock_failure(DisciplineError::InvalidConfig));
        }
        if host.domain != self.host_domain {
            return Err(self.clock_failure(DisciplineError::DomainMismatch));
        }
        if self.correction_watermark != Some(host) {
            return Err(self.clock_failure(DisciplineError::NonIncreasing));
        }
        let result = self
            .output_clock
            .as_mut()
            .expect("configured discipline was checked")
            .update(host, self.runtime.transport_mut());
        match result {
            Ok(update) => Ok(Some(update)),
            Err(DisciplineError::NoObservation | DisciplineError::Stale) => Ok(None),
            Err(error) => Err(self.clock_failure(error)),
        }
    }

    fn clock_failure(&mut self, error: DisciplineError) -> StepGameplayError {
        self.failed = true;
        StepGameplayError::Clock(error)
    }

    /// Optionally choose the actual host anchor once resource transfer is done.
    /// This only replaces a pristine Transport; it never resets a judge or
    /// changes the already prepared output-relative BGM and command prefix.
    pub fn activate(&mut self, host_origin: ClockPoint) -> Result<(), StepGameplayError> {
        self.ensure_usable()?;
        if self.activated || self.started {
            return Err(StepGameplayError::InvalidConfiguration(
                "activation requires an unactivated, unprocessed runtime",
            ));
        }
        if host_origin.domain != self.host_domain
            || host_origin.timestamp.checked_add(self.preroll).is_none()
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "activation requires the original host domain and representable song zero",
            ));
        }
        *self.runtime.transport_mut() =
            Transport::new(host_origin.timestamp, self.song, Rate::NORMAL);
        self.activated = true;
        Ok(())
    }

    /// Input provenance, source sequences and clock relations are validated by
    /// the same core path as native solo/cohort play; no retimestamping occurs.
    pub fn process_input(
        &mut self,
        event: PhysicalInputEvent,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, StepGameplayError> {
        self.ensure_usable()?;
        self.started = true;
        self.correction_watermark = None;
        let result = self.runtime.process_input(event, mapper, audio_at);
        self.observe(result)
    }

    pub fn advance_to(
        &mut self,
        host: ClockPoint,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, StepGameplayError> {
        self.ensure_usable()?;
        self.started = true;
        self.correction_watermark = None;
        let result = self.runtime.advance_to(host, mapper, audio_at);
        let report = self.observe(result)?;
        self.correction_watermark = Some(host);
        Ok(report)
    }

    fn observe(
        &mut self,
        result: Result<RuntimeReport, GroupError>,
    ) -> Result<RuntimeReport, StepGameplayError> {
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                // SoloRuntime returns committed partial reports as Ok. Its
                // pre-report core errors retain the original GroupError here.
                self.failed = true;
                return Err(StepGameplayError::Runtime(error));
            }
        };
        self.song = report.song_time;
        if !report.audio_commands.is_empty() {
            self.completion.reset_drain();
        }
        let score_error = self.score.observe(&report.judge_events).err();
        let capture_error = self
            .capture
            .as_mut()
            .and_then(|capture| capture.record_report(&report).err());
        if score_error.is_some()
            || report.judge_error.is_some()
            || !report.audio_failures.is_empty()
        {
            self.failed = true;
            return Err(StepGameplayError::Report {
                report,
                score_error,
                capture_error,
            });
        }
        if let Some(error) = capture_error {
            self.failed = true;
            return Err(StepGameplayError::Capture {
                error,
                report: Some(report),
            });
        }
        Ok(report)
    }

    /// `rendered_frames` is trusted caller-supplied evidence of completed Mixer
    /// rendering on output_origin's grid. Do not pass UI time, a requested arm
    /// frame, admitted commands or an estimated presentation cursor instead.
    pub fn feed_audio(
        &mut self,
        rendered_frames: u64,
        budget: usize,
    ) -> Result<BgmFeedReport, StepGameplayError> {
        self.ensure_usable()?;
        if !(1..=AudioLimits::MAX_COMMANDS).contains(&budget) {
            return Err(StepGameplayError::InvalidConfiguration(
                "BGM budget must be 1..=65536",
            ));
        }
        let previous = self.bgm.report().total_admitted;
        let runtime = &mut self.runtime;
        match self.bgm.feed(rendered_frames, budget, |command| {
            runtime.enqueue_audio(command)
        }) {
            Ok(report) => {
                if report.admitted != 0 {
                    self.completion.reset_drain();
                }
                Ok(report)
            }
            Err(error) => {
                self.failed = true;
                let mut report = self.bgm.report();
                report.admitted = report.total_admitted - previous;
                Err(StepGameplayError::Bgm { error, report })
            }
        }
    }

    /// Observe actual normal-rate Mixer rendering and output presentation.
    /// The caller's mixer must drain at least the entire command queue capacity
    /// in each nonempty render. Buffer length itself may vary. Presentation is
    /// on output_origin's domain; no host time or command ACK substitutes for it.
    pub fn observe_completion(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<bool, StepGameplayError> {
        let normalized = self.validate_completion_evidence(rendered, presented)?;
        if let Some(report) = rendered.filter(|report| report.frames != 0) {
            self.last_render = Some(report);
        }
        if let Some(point) = normalized {
            self.last_presented = Some(point.timestamp);
        }
        if self.pending.is_some() || self.consumer.available_up_to(1) != 0 {
            self.completion.reset_drain();
            return Ok(false);
        }
        match self.completion.observe(
            self.runtime.judge(),
            self.song,
            self.bgm.report(),
            rendered,
            normalized,
        ) {
            Ok(complete) => Ok(complete),
            Err(error) => {
                self.failed = true;
                Err(StepGameplayError::Completion {
                    error,
                    rendered,
                    presented,
                })
            }
        }
    }

    /// Binding preflight before a report is allowed to advance BGM admission.
    /// Successful validation does not adopt the observation or change readiness.
    pub(crate) fn validate_completion_evidence(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<Option<ClockPoint>, StepGameplayError> {
        self.ensure_usable()?;
        match validate_output_evidence(
            self.output_origin,
            self.sample_rate,
            self.last_render,
            self.last_presented,
            rendered,
            presented,
        ) {
            Ok(normalized) => Ok(normalized),
            Err(error) => {
                self.failed = true;
                Err(StepGameplayError::Completion {
                    error,
                    rendered,
                    presented,
                })
            }
        }
    }

    /// Drain one bounded batch while retaining its original scalar commands
    /// until ACK. An outstanding batch never pauses the actual judge or producer.
    pub fn take_commands(
        &mut self,
        max: usize,
    ) -> Result<Option<StepAudioBatch>, StepGameplayError> {
        self.ensure_usable()?;
        if !(1..=self.consumer.capacity()).contains(&max) {
            return Err(StepGameplayError::InvalidConfiguration(
                "batch bound must be 1..=command_capacity",
            ));
        }
        if let Some(batch) = &self.pending {
            return Err(StepGameplayError::OutstandingBatch {
                sequence: batch.sequence,
            });
        }
        let count = self.consumer.available_up_to(max);
        if count == 0 {
            return Ok(None);
        }
        self.completion.reset_drain();
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.failed = true;
            return Err(StepGameplayError::SequenceOverflow);
        };
        // Reserve both buffers before the first destructive pop. The returned
        // Vec belongs to the adapter; the retained copy records the exact ACK scope.
        let mut commands = Vec::new();
        let mut retained = Vec::new();
        commands
            .try_reserve_exact(count)
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        retained
            .try_reserve_exact(count)
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        for _ in 0..count {
            match self.consumer.try_pop() {
                Ok(command) => {
                    commands.push(command);
                    retained.push(command);
                }
                Err(error) => {
                    self.failed = true;
                    return Err(StepGameplayError::Queue { error, commands });
                }
            }
        }
        self.sequence = sequence;
        self.pending = Some(StepAudioBatch {
            sequence,
            commands: retained,
        });
        Ok(Some(StepAudioBatch { sequence, commands }))
    }

    pub fn acknowledge(
        &mut self,
        sequence: u64,
        admitted: usize,
        success: bool,
    ) -> Result<(), StepGameplayError> {
        self.ensure_usable()?;
        let batch = self.pending.take();
        acknowledge_batch(batch, sequence, admitted, success).map_err(|error| {
            self.failed = true;
            error
        })
    }

    /// Fence without inventing completion, a replay operation or a remote ACK.
    pub fn fail(&mut self) {
        self.failed = true;
    }
    pub fn song_time(&self) -> Timestamp {
        self.song
    }
    pub fn score(&self) -> &ScoreSummary {
        &self.score
    }
    pub fn bgm_report(&self) -> BgmFeedReport {
        self.bgm.report()
    }
    pub fn judge(&self) -> &JudgeEngine {
        self.runtime.judge()
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
}

/// Validate and consume exactly one original batch; owners apply their own fence.
pub(crate) fn acknowledge_batch(
    batch: Option<StepAudioBatch>,
    sequence: u64,
    admitted: usize,
    success: bool,
) -> Result<(), StepGameplayError> {
    let valid = batch.as_ref().is_some_and(|batch| {
        sequence == batch.sequence
            && admitted <= batch.commands.len()
            && (!success || admitted == batch.commands.len())
    });
    if !valid {
        return Err(StepGameplayError::InvalidAcknowledgement {
            sequence,
            admitted,
            success,
            batch,
        });
    }
    if !success {
        return Err(StepGameplayError::AudioRejected {
            batch: batch.expect("validated exact batch"),
            admitted,
        });
    }
    Ok(())
}

/// Shared validation for normal unlimited live/replay output; no state is adopted.
pub(crate) fn validate_output_evidence(
    output_origin: ClockPoint,
    sample_rate: u32,
    last_render: Option<RenderReport>,
    last_presented: Option<Timestamp>,
    rendered: Option<RenderReport>,
    presented: Option<ClockPoint>,
) -> Result<Option<ClockPoint>, CompletionError> {
    let normalized = presented
        .map(|point| {
            if point.domain != output_origin.domain {
                return Err(CompletionError(
                    "completion presentation clock domain differs",
                ));
            }
            let ns = point
                .timestamp
                .as_nanos()
                .checked_sub(output_origin.timestamp.as_nanos())
                .ok_or(CompletionError(
                    "completion presentation origin subtraction overflow",
                ))?;
            let timestamp = Timestamp::from_nanos(ns);
            if ns < 0 || last_presented.is_some_and(|last| timestamp < last) {
                return Err(CompletionError(
                    "completion presentation precedes its output frontier",
                ));
            }
            Ok(ClockPoint {
                domain: point.domain,
                timestamp,
            })
        })
        .transpose()?;
    let Some(report) = rendered else {
        return Ok(normalized);
    };
    if report.paused || report.playback_end_physical_frame.is_some() || report.producer_disconnected
    {
        return Err(CompletionError(
            "completion requires connected unlimited unpaused output",
        ));
    }
    if report.frames > AudioLimits::MAX_RENDER_FRAMES
        || report.active_voices > AudioLimits::MAX_VOICES
        || report.pending_commands > AudioLimits::MAX_COMMANDS
        || report.start_frame != report.playback_start_frame
        || report.frames != report.playback_frames
    {
        return Err(CompletionError(
            "completion render capacity or playback grid differs",
        ));
    }
    let frames = u64::try_from(report.frames)
        .map_err(|_| CompletionError("completion render extent overflow"))?;
    let end = report
        .start_frame
        .checked_add(frames)
        .ok_or(CompletionError("completion render cursor overflow"))?;
    let end_ns =
        (i128::from(end) * 1_000_000_000 + i128::from(sample_rate) - 1) / i128::from(sample_rate);
    let duration = Duration::from_nanos(
        i64::try_from(end_ns)
            .map_err(|_| CompletionError("completion render duration overflow"))?,
    );
    output_origin
        .timestamp
        .checked_add(duration)
        .ok_or(CompletionError("completion render clock overflow"))?;
    let counters = report.counters;
    if counters.rendered_frames != end
        || counters.commands_applied > counters.commands_consumed
        || counters.late_commands > counters.commands_consumed
        || counter_values(counters)[4..]
            .iter()
            .any(|&value| value != 0)
    {
        return Err(CompletionError(
            "completion mixer counters contain failure or invalid evidence",
        ));
    }
    if let Some(previous) = last_render {
        if report.start_frame == previous.start_frame {
            if report != previous {
                return Err(CompletionError(
                    "completion render block changed after observation",
                ));
            }
        } else if report.start_frame < previous.counters.rendered_frames {
            return Err(CompletionError(
                "completion render chronology regressed or overlapped",
            ));
        }
        if counter_values(counters)
            .into_iter()
            .zip(counter_values(previous.counters))
            .any(|(current, previous)| current < previous)
        {
            return Err(CompletionError("completion mixer counters regressed"));
        }
    }
    Ok(normalized)
}

fn counter_values(counters: AudioCounters) -> [u64; 11] {
    [
        counters.rendered_frames,
        counters.commands_consumed,
        counters.commands_applied,
        counters.late_commands,
        counters.pending_full,
        counters.voice_full,
        counters.unknown_samples,
        counters.unknown_stops,
        counters.invalid_gains,
        counters.invalid_rates,
        counters.invalid_times,
    ]
}
