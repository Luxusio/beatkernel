//! Nonblocking control-side ownership of the actual solo runtime and BGM queue.
//! Hosts supply clock relations and genuine completed-render cursors. This
//! module does not sample a platform clock or infer delivery from admission.
use crate::{
    PreparedBms,
    bgm::{BgmConfig, BgmFeedError, BgmFeedReport, BgmFeeder},
    competition::{CompetitionError, ScoreSummary},
    local_runtime::{GroupError, SoloRuntime},
    native_judge::NativeJudgeConfig,
};
use beatkernel::{
    audio::{AudioCommand, AudioLimits, CommandConsumer, QueuePopError, SampleBank, command_queue},
    input::{BindingMap, PhysicalInputEvent},
    judge::JudgeEngine,
    runtime::{RuntimeProcessingClock, RuntimeReport},
    time::{ClockDomainId, ClockMapper, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
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
    /// The report's judgments remain committed even when audio admission failed.
    /// Score aggregation is atomic; its error preserves the previous score.
    Report {
        report: RuntimeReport,
        score_error: Option<CompetitionError>,
    },
    Bgm {
        error: BgmFeedError,
        report: BgmFeedReport,
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
            Self::Report {
                report,
                score_error,
            } => write!(
                f,
                "step gameplay report failed: judge {:?}, {} audio failures, score {:?}",
                report.judge_error,
                report.audio_failures.len(),
                score_error
            ),
            Self::Bgm { error, report } => write!(
                f,
                "{error}; {} BGM admissions remain committed",
                report.total_admitted
            ),
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
        let result = self.runtime.advance_to(host, mapper, audio_at);
        self.observe(result)
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
        let score_error = self.score.observe(&report.judge_events).err();
        if score_error.is_some()
            || report.judge_error.is_some()
            || !report.audio_failures.is_empty()
        {
            self.failed = true;
            return Err(StepGameplayError::Report {
                report,
                score_error,
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
            Ok(report) => Ok(report),
            Err(error) => {
                self.failed = true;
                let mut report = self.bgm.report();
                report.admitted = report.total_admitted - previous;
                Err(StepGameplayError::Bgm { error, report })
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
        let valid = batch.as_ref().is_some_and(|batch| {
            sequence == batch.sequence
                && admitted <= batch.commands.len()
                && (!success || admitted == batch.commands.len())
        });
        if !valid {
            self.failed = true;
            return Err(StepGameplayError::InvalidAcknowledgement {
                sequence,
                admitted,
                success,
                batch,
            });
        }
        if !success {
            self.failed = true;
            return Err(StepGameplayError::AudioRejected {
                batch: batch.expect("validated exact batch"),
                admitted,
            });
        }
        Ok(())
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
