//! Nonblocking playback of an actual recorded prefix and its original PCM.
//! No platform clock, synthetic input, alternate judge or wall-clock cutoff.
use crate::{
    PreparedBms,
    bgm::{BgmConfig, BgmFeedError, BgmFeedReport, BgmFeeder},
    competition::{CompetitionError, ScoreSummary},
    completion::{CompletionError, ReplayCompletion},
    gauge::BmsGauge,
    mine_damage::MineDamageSummary,
    offline::OwnedStopEvidence,
    replay_audio::{
        ReplayAudioError, completed_render_cursor_with_stops, plan_section_audio, section_end_frame,
    },
    replay_visual::ReplayVisual,
    step_gameplay::{
        StepAudioBatch, StepGameplayError, acknowledge_batch,
        validate_section_output_evidence_with_stops,
    },
};
use beatkernel::{
    audio::{AudioLimits, RenderReport, SampleBank},
    judge::JudgeEvent,
    replay::codec::{ReplayCodecLimits, ReplayFile},
    time::{ClockPoint, Duration, Timestamp},
};
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepReplayConfig {
    pub output_origin: ClockPoint,
    pub preroll: Duration,
    pub lookahead: Duration,
    /// Maximum commands admitted ahead of actual rendering, including a batch
    /// whose remote ACK is still pending. Also bounds each outbound batch.
    pub max_pending: usize,
}

#[derive(Debug)]
pub enum StepReplayError {
    InvalidConfiguration(&'static str),
    Setup(Box<dyn Error>),
    Failed,
    OutstandingBatch {
        sequence: u64,
    },
    AllocationFailed,
    SequenceOverflow,
    Bgm {
        error: BgmFeedError,
        report: BgmFeedReport,
    },
    /// Shared live/replay validation retains the exact original batch and ACK.
    Acknowledgement(StepGameplayError),
    Output {
        error: CompletionError,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    },
    Audio {
        error: ReplayAudioError,
        rendered: RenderReport,
        presented: Option<ClockPoint>,
    },
    Visual(Box<dyn Error>),
    /// These actual judge events remain committed even if score aggregation or
    /// retaining the event handoff failed. Earlier buffered events stay readable.
    Progress {
        error: CompetitionError,
        events: Vec<JudgeEvent>,
    },
}
impl fmt::Display for StepReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(reason) => write!(f, "step replay configuration: {reason}"),
            Self::Setup(error) => write!(f, "step replay setup: {error}"),
            Self::Failed => f.write_str("step replay is fenced"),
            Self::OutstandingBatch { sequence } => {
                write!(f, "audio batch {sequence} awaits acknowledgement")
            }
            Self::AllocationFailed => {
                f.write_str("replay batch allocation failed before admission")
            }
            Self::SequenceOverflow => f.write_str("replay audio batch sequence exhausted"),
            Self::Bgm { error, report } => write!(
                f,
                "{error}; {} replay audio admissions remain committed",
                report.total_admitted
            ),
            Self::Acknowledgement(error) => write!(f, "{error}"),
            Self::Output { error, .. } => write!(f, "step replay output: {error}"),
            Self::Audio { error, .. } => write!(f, "{error}"),
            Self::Visual(error) => write!(f, "step replay visual: {error}"),
            Self::Progress { error, events } => write!(
                f,
                "step replay progress: {error}; {} judge events remain committed",
                events.len()
            ),
        }
    }
}
impl Error for StepReplayError {}

/// A bounded remote-audio producer plus the existing validated replay judge.
/// Preparation must already select the recorded section through
/// `section_start::prepare_section_replay`; this owner never selects a PCM suffix twice.
pub struct StepReplay {
    config: StepReplayConfig,
    sample_rate: u32,
    visual: ReplayVisual,
    feeder: BgmFeeder,
    completion: ReplayCompletion,
    score: ScoreSummary,
    song_origin: Timestamp,
    song: Timestamp,
    end: Option<Timestamp>,
    playback_end_frame: Option<u64>,
    events: Vec<JudgeEvent>,
    rendered_cursor: u64,
    last_render: Option<RenderReport>,
    last_presented: Option<Timestamp>,
    pending: Option<StepAudioBatch>,
    acknowledged_stops: OwnedStopEvidence,
    sequence: u64,
    failed: bool,
}
impl fmt::Debug for StepReplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepReplay")
            .field("config", &self.config)
            .field("song", &self.song)
            .field("score", &self.score)
            .field("rendered_cursor", &self.rendered_cursor)
            .field(
                "pending_sequence",
                &self.pending.as_ref().map(|batch| batch.sequence),
            )
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}
impl StepReplay {
    pub fn new(
        prepared: PreparedBms,
        file: ReplayFile,
        limits: ReplayCodecLimits,
        config: StepReplayConfig,
    ) -> Result<(Self, SampleBank), StepReplayError> {
        if config.preroll.as_nanos() < 0
            || config.lookahead.as_nanos() <= 0
            || !(1..=AudioLimits::MAX_COMMANDS).contains(&config.max_pending)
        {
            return Err(StepReplayError::InvalidConfiguration(
                "nonnegative preroll, positive lookahead and bounded pending capacity required",
            ));
        }
        let visual = ReplayVisual::new_section(&prepared.source, &file, limits)
            .map_err(StepReplayError::Setup)?;
        let song_origin = visual.start().checked_sub(config.preroll).ok_or(
            StepReplayError::InvalidConfiguration("replay song origin overflow"),
        )?;
        let sample_rate = prepared.bank.format().sample_rate();
        let end = visual.end();
        let playback_end_frame = end
            .map(|end| section_end_frame(visual.start(), end, config.preroll, sample_rate))
            .transpose()
            .map_err(|error| StepReplayError::Setup(error.into()))?;
        let acknowledged_stops = OwnedStopEvidence::default();
        validate_section_output_evidence_with_stops(
            config.output_origin,
            sample_rate,
            None,
            None,
            None,
            None,
            playback_end_frame,
            &acknowledged_stops,
        )
        .map_err(|error| StepReplayError::InvalidConfiguration(error.0))?;
        let plan = plan_section_audio(
            &prepared,
            file,
            limits,
            config.output_origin,
            config.preroll,
        )
        .map_err(StepReplayError::Setup)?;
        let remaining = plan.commands.len();
        let feeder = BgmFeeder::from_output_commands(
            plan.commands,
            BgmConfig {
                output_origin: config.output_origin,
                sample_rate,
                preroll: Duration::ZERO,
                lookahead: config.lookahead,
                max_pending: config.max_pending,
            },
        )
        .map_err(|error| StepReplayError::Bgm {
            error,
            report: BgmFeedReport {
                remaining,
                ..BgmFeedReport::default()
            },
        })?;
        Ok((
            Self {
                config,
                sample_rate,
                visual,
                feeder,
                completion: ReplayCompletion::new(config.output_origin.domain, sample_rate),
                score: ScoreSummary::default(),
                song_origin,
                song: song_origin,
                end,
                playback_end_frame,
                events: Vec::new(),
                rendered_cursor: 0,
                last_render: None,
                last_presented: None,
                pending: None,
                acknowledged_stops,
                sequence: 0,
                failed: false,
            },
            prepared.bank,
        ))
    }

    fn ensure_usable(&self) -> Result<(), StepReplayError> {
        if self.failed {
            Err(StepReplayError::Failed)
        } else {
            Ok(())
        }
    }

    /// Admit immutable planned commands at the latest actual completed render
    /// cursor (frame zero during setup). Call even when no command is expected:
    /// the feeder retires rendered credits before returning an empty batch.
    /// A retained batch forbids a second admission until its exact remote ACK.
    pub fn take_commands(&mut self, max: usize) -> Result<Option<StepAudioBatch>, StepReplayError> {
        self.ensure_usable()?;
        if !(1..=self.config.max_pending).contains(&max) {
            return Err(StepReplayError::InvalidConfiguration(
                "batch bound must be 1..=max_pending",
            ));
        }
        if let Some(batch) = &self.pending {
            return Err(StepReplayError::OutstandingBatch {
                sequence: batch.sequence,
            });
        }
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.failed = true;
            return Err(StepReplayError::SequenceOverflow);
        };
        let capacity = max.min(self.feeder.report().remaining);
        let mut commands = Vec::new();
        let mut retained = Vec::new();
        commands
            .try_reserve_exact(capacity)
            .map_err(|_| StepReplayError::AllocationFailed)?;
        retained
            .try_reserve_exact(capacity)
            .map_err(|_| StepReplayError::AllocationFailed)?;
        let previous = self.feeder.report().total_admitted;
        if let Err(error) = self.feeder.feed(self.rendered_cursor, max, |command| {
            commands.push(command);
            retained.push(command);
            Ok(())
        }) {
            self.failed = true;
            let mut report = self.feeder.report();
            report.admitted = report.total_admitted - previous;
            return Err(StepReplayError::Bgm { error, report });
        }
        if commands.is_empty() {
            return Ok(None);
        }
        self.completion.reset_drain();
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
    ) -> Result<(), StepReplayError> {
        self.ensure_usable()?;
        let mut candidate = self.acknowledged_stops;
        let counted = self.pending.as_ref().map_or(Ok(()), |batch| {
            candidate.record_admitted(&batch.commands[..admitted.min(batch.commands.len())])
        });
        match acknowledge_batch(self.pending.take(), sequence, admitted, success) {
            Ok(()) => {
                if let Err(error) = counted {
                    self.failed = true;
                    return Err(StepReplayError::InvalidConfiguration(error));
                }
                self.acknowledged_stops = candidate;
                Ok(())
            }
            Err(error) => {
                if matches!(&error, StepGameplayError::AudioRejected { .. }) && counted.is_ok() {
                    self.acknowledged_stops = candidate;
                }
                self.failed = true;
                Err(StepReplayError::Acknowledgement(error))
            }
        }
    }

    /// Only genuine presentation advances recorded operations. Render reports
    /// provide audio credits, never a substitute visual song clock. The caller
    /// must drain its full command queue capacity per nonempty Mixer callback.
    pub fn observe_output(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<bool, StepReplayError> {
        self.ensure_usable()?;
        let normalized = validate_section_output_evidence_with_stops(
            self.config.output_origin,
            self.sample_rate,
            self.last_render,
            self.last_presented,
            rendered,
            presented,
            self.playback_end_frame,
            &self.acknowledged_stops,
        )
        .map_err(|error| self.output_failure(error, rendered, presented))?;
        let cursor = rendered
            .map(|report| {
                completed_render_cursor_with_stops(&report, &self.acknowledged_stops).map_err(
                    |error| {
                        self.failed = true;
                        StepReplayError::Audio {
                            error,
                            rendered: report,
                            presented,
                        }
                    },
                )
            })
            .transpose()?;
        let song = normalized
            .map(|point| {
                let ns = i128::from(self.song_origin.as_nanos())
                    + i128::from(point.timestamp.as_nanos());
                let ns = self
                    .end
                    .map_or(ns, |end| ns.min(i128::from(end.as_nanos())));
                let ns = i64::try_from(ns).map_err(|_| {
                    self.output_failure(
                        CompletionError("replay presentation song mapping overflow"),
                        rendered,
                        presented,
                    )
                })?;
                Ok::<_, StepReplayError>(Timestamp::from_nanos(ns))
            })
            .transpose()?;
        let finite_audio_complete = if let Some(endpoint) = self.playback_end_frame {
            let feed = self.feeder.report();
            let endpoint_report = rendered
                .or(self.last_render)
                .filter(|report| report.playback_end_physical_frame == Some(endpoint));
            if self.pending.is_some() || feed.remaining != 0 || feed.outstanding != 0 {
                false
            } else if let Some(report) = endpoint_report {
                let admitted = u64::try_from(feed.total_admitted).map_err(|_| {
                    self.output_failure(
                        CompletionError("finite replay admission count overflow"),
                        Some(report),
                        presented,
                    )
                })?;
                if report.counters.commands_consumed != admitted
                    || report.counters.commands_applied != admitted
                {
                    return Err(self.output_failure(
                        CompletionError(
                            "finite replay endpoint omitted or added planned audio execution",
                        ),
                        Some(report),
                        presented,
                    ));
                }
                true
            } else {
                false
            }
        } else {
            false
        };
        // Every untrusted evidence check precedes any feeder or visual mutation.
        if let Some(cursor) = cursor {
            self.rendered_cursor = cursor;
        }
        if let Some(report) = rendered.filter(|report| report.frames != 0) {
            self.last_render = Some(report);
        }
        if let Some(point) = normalized {
            self.last_presented = Some(point.timestamp);
        }
        if let Some(song) = song {
            let events = self.visual.advance_to(song).map_err(|error| {
                self.failed = true;
                StepReplayError::Visual(error)
            })?;
            self.song = song;
            if let Err(error) = self.score.observe(&events) {
                self.failed = true;
                return Err(StepReplayError::Progress { error, events });
            }
            if self.events.try_reserve(events.len()).is_err() {
                self.failed = true;
                return Err(StepReplayError::Progress {
                    error: CompetitionError::AllocationFailed,
                    events,
                });
            }
            self.events.extend(events);
        }
        if self.pending.is_some() {
            self.completion.reset_drain();
            return Ok(false);
        }
        if let Some(endpoint) = self.playback_end_frame {
            let endpoint_ns = (i128::from(endpoint) * 1_000_000_000 + i128::from(self.sample_rate)
                - 1)
                / i128::from(self.sample_rate);
            return Ok(finite_audio_complete
                && self.visual.finished()
                && self
                    .last_presented
                    .is_some_and(|point| i128::from(point.as_nanos()) >= endpoint_ns));
        }
        self.completion
            .observe(
                self.visual.finished(),
                self.feeder.report(),
                rendered,
                normalized,
            )
            .map_err(|error| self.output_failure(error, rendered, presented))
    }

    fn output_failure(
        &mut self,
        error: CompletionError,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> StepReplayError {
        self.failed = true;
        StepReplayError::Output {
            error,
            rendered,
            presented,
        }
    }

    /// Transfer accumulated committed events, including after failure. This is
    /// state readout only: it never advances the recorded cursor or submits audio.
    pub fn drain_events(&mut self) -> Vec<JudgeEvent> {
        std::mem::take(&mut self.events)
    }
    pub fn song_time(&self) -> Timestamp {
        self.song
    }
    pub fn end_ns(&self) -> Option<i64> {
        self.end.map(|end| end.as_nanos())
    }
    pub fn playback_end_frame(&self) -> Option<u64> {
        self.playback_end_frame
    }
    pub fn score(&self) -> &ScoreSummary {
        &self.score
    }
    /// Borrows the visual owner's actual recorded hazard accumulation.
    pub fn mine_damage(&self) -> &MineDamageSummary {
        self.visual.mine_damage()
    }
    /// Borrows the visual owner's gauge without observing the prefix again.
    pub fn gauge(&self) -> &BmsGauge {
        self.visual.gauge()
    }
    pub fn pressed_lanes(&self) -> u32 {
        self.visual.pressed_lanes()
    }
    pub fn recorded_until(&self) -> Option<Timestamp> {
        self.visual.recorded_until()
    }
    pub fn bgm_report(&self) -> BgmFeedReport {
        self.feeder.report()
    }
    /// Actual Stop prefix accepted by valid remote ACKs, retained after failure.
    /// Feeder callbacks and immutable planned commands do not establish this count.
    pub fn acknowledged_stop_commands(&self) -> u64 {
        self.acknowledged_stops.admitted_stops()
    }
    pub fn fail(&mut self) {
        self.failed = true;
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
}
