//! Reverse historical traversal with independent, dedicated keysound output.

use super::SoundBinding;
use crate::{
    audio::{
        command_queue, AudioCommand, AudioError, CommandProducer, CommandPushError, Mixer,
        MixerConfig, SampleBank,
    },
    judge::{JudgeEvent, JudgeOutcome},
    replay::{ReplayError, ReplaySession},
    time::{ClockPoint, Timestamp},
    transport::Rate,
};

/// Choice of sample-head direction during reverse historical traversal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReverseSoundPolicy {
    /// Traverse song history backward but play each sample from its first frame.
    ReverseTimelineOnly,
    /// Traverse history backward and play each sample from its last frame.
    ReverseSamples,
    /// Traverse history without publishing keysound Play commands.
    Mute,
}

/// Explicit validation, reconstruction or output setup failure.
#[derive(Debug)]
pub enum ReversePlaybackError {
    /// Zero rate, over 65,536 bindings, result limit outside 1..=1,048,576, or invalid gain.
    InvalidConfiguration,
    /// Recorded result count exceeds the caller's finite bound.
    ResultCapacity,
    /// Traversal would publish more than 1,048,576 command attempts.
    CommandCapacity,
    /// A binding names an asset absent from the fresh keysound bank.
    MissingSample,
    /// Integer negation or scheduling arithmetic is unrepresentable.
    Overflow,
    /// Output domain differs or its timestamp regresses behind traversal/rendering.
    OutputChronology,
    /// A reverse step requested a later song position.
    ForwardStep,
    /// Forward replay reconstruction failed.
    Replay(ReplayError),
    /// Mixer or queue preparation/rendering failed.
    Audio(AudioError),
    /// Initial rate command failed admission; no replacement was installed.
    Command(CommandPushError),
}
impl From<ReplayError> for ReversePlaybackError {
    fn from(value: ReplayError) -> Self {
        Self::Replay(value)
    }
}
impl From<AudioError> for ReversePlaybackError {
    fn from(value: AudioError) -> Self {
        Self::Audio(value)
    }
}
impl std::fmt::Display for ReversePlaybackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "reverse playback: {self:?}")
    }
}
impl std::error::Error for ReversePlaybackError {}

/// Complete traversal and queue-admission evidence, independent of rendering.
#[derive(Debug)]
pub struct ReverseStepReport {
    /// Previous inspection song position.
    pub from: Timestamp,
    /// Reconstructed inspection song position.
    pub to: Timestamp,
    /// Historical results in reverse original emission order, including misses.
    pub crossed: Vec<JudgeEvent>,
    /// Exact admitted Play commands in increasing output chronology.
    pub admitted: Vec<AudioCommand>,
    /// Exact failed Play commands, never automatically retried.
    pub failed: Vec<CommandPushError>,
    /// End of this traversal on the distinct output timeline.
    pub output_end: ClockPoint,
}

/// Replay inspection and a producer dedicated to the separately returned Mixer.
///
/// Setup/traversal allocate off-thread. The returned Mixer can move to a native
/// backend. No producer escapes; its bank must contain only dedicated keysounds.
/// Native hardware reset and presentation mapping remain host concerns.
pub struct ReversePlayback {
    replay: ReplaySession,
    history: Vec<JudgeEvent>,
    bindings: Vec<SoundBinding>,
    song_time: Timestamp,
    policy: ReverseSoundPolicy,
    magnitude: Rate,
    producer: CommandProducer,
    output_end: ClockPoint,
    setup_command: AudioCommand,
}
impl ReversePlayback {
    /// Reconstructs the complete immutable recording to capture bounded history,
    /// then reconstructs at `initial_song_time` and prepares fresh dedicated audio.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mut replay: ReplaySession,
        initial_song_time: Timestamp,
        bindings: Vec<SoundBinding>,
        max_results: usize,
        policy: ReverseSoundPolicy,
        magnitude: Rate,
        config: MixerConfig,
        bank: SampleBank,
    ) -> Result<(Self, Mixer), ReversePlaybackError> {
        if max_results == 0
            || max_results > 1_048_576
            || bindings.len() > 65_536
            || bindings.iter().any(|b| !b.gain.is_finite())
        {
            return Err(ReversePlaybackError::InvalidConfiguration);
        }
        let magnitude = positive_magnitude(magnitude)?;
        replay.seek_cursor(replay.records().len())?;
        if replay.results().len() > max_results {
            return Err(ReversePlaybackError::ResultCapacity);
        }
        let history = replay.results().to_vec();
        replay.seek(initial_song_time)?;
        let (producer, mixer, setup_command) = prepare(&bindings, policy, magnitude, config, bank)?;
        Ok((
            Self {
                replay,
                history,
                bindings,
                song_time: initial_song_time,
                policy,
                magnitude,
                producer,
                output_end: ClockPoint {
                    domain: config.domain(),
                    timestamp: config.origin(),
                },
                setup_command,
            },
            mixer,
        ))
    }
    /// Shared logical state; no policy or audio mutation reaches this session.
    pub const fn replay(&self) -> &ReplaySession {
        &self.replay
    }
    /// Current unoffset inspection song time.
    pub const fn song_time(&self) -> Timestamp {
        self.song_time
    }
    /// Current sample-head policy.
    pub const fn policy(&self) -> ReverseSoundPolicy {
        self.policy
    }
    /// Exact SetRate admitted when the current fresh output owner was installed.
    pub const fn setup_command(&self) -> AudioCommand {
        self.setup_command
    }
    /// Silent logical inspection; queued or active audio remains until transition.
    pub fn seek(&mut self, target: Timestamp) -> Result<(), ReversePlaybackError> {
        self.replay.seek(target)?;
        self.song_time = target;
        Ok(())
    }
    /// Prepares a new Mixer and atomically swaps to its fresh command producer.
    ///
    /// Call even for an unchanged policy when a silent seek needs an audible flush.
    /// A failure leaves the producer and logical state unchanged. The host must
    /// stop/reset/drop the old output owner before installing the returned Mixer;
    /// disconnecting its producer alone does not erase old pending commands.
    pub fn transition(
        &mut self,
        policy: ReverseSoundPolicy,
        magnitude: Rate,
        config: MixerConfig,
        bank: SampleBank,
    ) -> Result<(AudioCommand, Mixer), ReversePlaybackError> {
        self.validate_output(ClockPoint {
            domain: config.domain(),
            timestamp: config.origin(),
        })?;
        let magnitude = positive_magnitude(magnitude)?;
        let (producer, mixer, command) = prepare(&self.bindings, policy, magnitude, config, bank)?;
        self.producer = producer;
        self.policy = policy;
        self.magnitude = magnitude;
        self.output_end.timestamp = config.origin();
        self.setup_command = command;
        Ok((command, mixer))
    }
    /// Crosses `target < historical_result.at <= previous_song_time` backward.
    ///
    /// Output distance is divided by magnitude, rounding upward to integer nanos.
    /// Validation precedes reconstruction; queue failure occurs afterward and is
    /// reported without rolling back judge state or retrying commands.
    pub fn step_back(
        &mut self,
        target: Timestamp,
        output_start: ClockPoint,
    ) -> Result<ReverseStepReport, ReversePlaybackError> {
        if target > self.song_time {
            return Err(ReversePlaybackError::ForwardStep);
        }
        self.validate_output(output_start)?;
        let from = self.song_time;
        let output_end = ClockPoint {
            domain: output_start.domain,
            timestamp: output_time(from, target, output_start.timestamp, self.magnitude)?,
        };
        let crossed: Vec<_> = self
            .history
            .iter()
            .rev()
            .filter(|r| target < r.at && r.at <= from)
            .copied()
            .collect();
        let mut commands = Vec::new();
        if self.policy != ReverseSoundPolicy::Mute {
            for result in &crossed {
                if !matches!(result.outcome, JudgeOutcome::Hit { .. }) {
                    continue;
                }
                let at = output_time(from, result.at, output_start.timestamp, self.magnitude)?;
                for binding in &self.bindings {
                    if binding.object == result.object && binding.stage == result.stage {
                        if commands.len() == 1_048_576 {
                            return Err(ReversePlaybackError::CommandCapacity);
                        }
                        commands.push(AudioCommand::Play {
                            voice: binding.voice,
                            sample: binding.sample,
                            at,
                            gain: binding.gain,
                        });
                    }
                }
            }
        }
        self.replay.seek(target)?;
        self.song_time = target;
        self.output_end = output_end;
        let mut admitted = Vec::new();
        let mut failed = Vec::new();
        for command in commands {
            match self.producer.try_push(command) {
                Ok(()) => admitted.push(command),
                Err(error) => failed.push(error),
            }
        }
        Ok(ReverseStepReport {
            from,
            to: target,
            crossed,
            admitted,
            failed,
            output_end,
        })
    }
    /// Advances the minimum scheduling point using explicitly supplied host telemetry.
    ///
    /// This is a submitted/rendered output boundary, not a claim about physical
    /// presentation. Hosts must call it before scheduling behind a running Mixer.
    pub fn observe_output_floor(&mut self, point: ClockPoint) -> Result<(), ReversePlaybackError> {
        self.validate_output(point)?;
        self.output_end = point;
        Ok(())
    }
    fn validate_output(&self, point: ClockPoint) -> Result<(), ReversePlaybackError> {
        if point.domain != self.output_end.domain || point.timestamp < self.output_end.timestamp {
            return Err(ReversePlaybackError::OutputChronology);
        }
        Ok(())
    }
}
fn positive_magnitude(rate: Rate) -> Result<Rate, ReversePlaybackError> {
    let numerator = rate
        .numerator()
        .checked_abs()
        .ok_or(ReversePlaybackError::Overflow)?;
    if numerator == 0 {
        return Err(ReversePlaybackError::InvalidConfiguration);
    }
    Rate::new(numerator, rate.denominator()).map_err(|_| ReversePlaybackError::InvalidConfiguration)
}
fn output_time(
    from: Timestamp,
    at: Timestamp,
    origin: Timestamp,
    magnitude: Rate,
) -> Result<Timestamp, ReversePlaybackError> {
    let delta = i128::from(from.as_nanos()) - i128::from(at.as_nanos());
    let scaled = delta
        .checked_mul(i128::from(magnitude.denominator()))
        .ok_or(ReversePlaybackError::Overflow)?;
    let divisor = i128::from(magnitude.numerator());
    let offset = scaled / divisor + i128::from(scaled % divisor != 0);
    let nanos = i128::from(origin.as_nanos())
        .checked_add(offset)
        .ok_or(ReversePlaybackError::Overflow)?;
    Ok(Timestamp::from_nanos(
        i64::try_from(nanos).map_err(|_| ReversePlaybackError::Overflow)?,
    ))
}
fn prepare(
    bindings: &[SoundBinding],
    policy: ReverseSoundPolicy,
    magnitude: Rate,
    config: MixerConfig,
    bank: SampleBank,
) -> Result<(CommandProducer, Mixer, AudioCommand), ReversePlaybackError> {
    if bindings.iter().any(|b| bank.get(b.sample).is_none()) {
        return Err(ReversePlaybackError::MissingSample);
    }
    let rate = if policy == ReverseSoundPolicy::ReverseSamples {
        Rate::new(-magnitude.numerator(), magnitude.denominator())
            .map_err(|_| ReversePlaybackError::InvalidConfiguration)?
    } else {
        magnitude
    };
    let (mut producer, consumer) = command_queue(config.limits().queue_capacity())?;
    let mixer = Mixer::new(config, bank, consumer)?;
    let command = AudioCommand::SetRate {
        rate,
        at: config.origin(),
    };
    producer
        .try_push(command)
        .map_err(ReversePlaybackError::Command)?;
    Ok((producer, mixer, command))
}
