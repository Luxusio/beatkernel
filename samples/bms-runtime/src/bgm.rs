//! Off-thread rolling admission of prepared BGM into one existing audio queue.
use beatkernel::{
    audio::{AudioCommand, AudioLimits, CommandPushError},
    time::{ClockPoint, Duration},
};
use std::{error::Error, fmt};

/// Explicit output mapping and bounded outstanding background work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BgmConfig {
    /// Frame zero and its declared audio domain.
    pub output_origin: ClockPoint,
    /// Applied output sample rate.
    pub sample_rate: u32,
    /// Song zero offset after output frame zero.
    pub preroll: Duration,
    /// Positive amount of output time admitted ahead of the render cursor.
    pub lookahead: Duration,
    /// Maximum admitted BGM commands whose target frames have not rendered.
    pub max_pending: usize,
}

/// Admission evidence, separate from mixer execution and acoustic output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BgmFeedReport {
    /// Commands admitted by this call (zero for a snapshot).
    pub admitted: usize,
    /// Complete successful admission prefix.
    pub total_admitted: usize,
    /// Commands not admitted yet, including those beyond the current horizon.
    pub remaining: usize,
    /// Admitted commands whose target frame is not before the render cursor.
    pub outstanding: usize,
    /// Eligible work remains because credit or admission budget ran out.
    pub deferred: bool,
}

/// Explicit errors; successfully admitted commands are never rolled back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BgmFeedError {
    /// Zero/invalid capacities, rate, lookahead, preroll or admission budget.
    InvalidConfiguration(&'static str),
    /// Non-Play, nonfinite gain or a mapped target before output frame zero.
    InvalidCommand(AudioCommand),
    /// Checked timestamp/frame/horizon arithmetic was unrepresentable.
    Overflow,
    /// Off-thread schedule reservation failed.
    AllocationFailed,
    /// A caller's completed-render cursor moved backward.
    CursorRegression {
        /// Previously accepted cursor.
        previous: u64,
        /// Rejected cursor.
        received: u64,
    },
    /// The next unadmitted cue is already in a rendered frame.
    Late {
        /// Exact mapped command, never retimestamped.
        command: AudioCommand,
        /// Command execution frame.
        target_frame: u64,
        /// Caller-supplied completed-render cursor.
        rendered_frames: u64,
    },
    /// The queue rejected this exact command; the prefix remains admitted.
    Admission(CommandPushError),
}
impl fmt::Display for BgmFeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BGM admission: {self:?}")
    }
}
impl Error for BgmFeedError {}

struct Cue {
    command: AudioCommand,
    frame: u64,
    ordinal: usize,
}

/// Prepared command schedule and bounded admission state, outside audio callbacks.
///
/// The supplied cursor must come from actual completed Mixer rendering on this
/// configuration's grid. Neither queue admission nor this helper proves native
/// delivery. No native query, PCM decoding or producer ownership occurs here.
pub struct BgmFeeder {
    config: BgmConfig,
    cues: Vec<Cue>,
    lookahead_frames: u64,
    next: usize,
    retired: usize,
    cursor: u64,
    deferred: bool,
}
impl BgmFeeder {
    /// Maps song-time Play commands once and preserves original equal-time order.
    pub fn new(commands: Vec<AudioCommand>, config: BgmConfig) -> Result<Self, BgmFeedError> {
        if config.sample_rate == 0
            || config.lookahead.as_nanos() <= 0
            || config.preroll.as_nanos() < 0
            || !(1..=AudioLimits::MAX_COMMANDS).contains(&config.max_pending)
        {
            return Err(BgmFeedError::InvalidConfiguration(
                "positive rate/lookahead, nonnegative preroll and bounded BGM credit required",
            ));
        }
        let lookahead_frames =
            ceil_frames(i128::from(config.lookahead.as_nanos()), config.sample_rate)?;
        let mut cues = Vec::new();
        cues.try_reserve_exact(commands.len())
            .map_err(|_| BgmFeedError::AllocationFailed)?;
        for (ordinal, original) in commands.into_iter().enumerate() {
            let AudioCommand::Play {
                voice,
                sample,
                at,
                gain,
            } = original
            else {
                return Err(BgmFeedError::InvalidCommand(original));
            };
            let elapsed = i128::from(at.as_nanos()) + i128::from(config.preroll.as_nanos());
            if !gain.is_finite() || elapsed < 0 {
                return Err(BgmFeedError::InvalidCommand(original));
            }
            let mapped = i128::from(config.output_origin.timestamp.as_nanos()) + elapsed;
            let mapped = i64::try_from(mapped).map_err(|_| BgmFeedError::Overflow)?;
            cues.push(Cue {
                command: AudioCommand::Play {
                    voice,
                    sample,
                    at: beatkernel::time::Timestamp::from_nanos(mapped),
                    gain,
                },
                frame: ceil_frames(elapsed, config.sample_rate)?,
                ordinal,
            });
        }
        // Explicit ordinal makes equal-time order stable without sort allocation.
        cues.sort_unstable_by_key(|cue| (cue.command.at(), cue.ordinal));
        Ok(Self {
            config,
            cues,
            lookahead_frames,
            next: 0,
            retired: 0,
            cursor: 0,
            deferred: false,
        })
    }

    /// Declared frame grid and admission policy, without native inspection.
    pub const fn config(&self) -> BgmConfig {
        self.config
    }

    /// State snapshot; the per-call admitted count is zero.
    pub fn report(&self) -> BgmFeedReport {
        BgmFeedReport {
            admitted: 0,
            total_admitted: self.next,
            remaining: self.cues.len() - self.next,
            outstanding: self.next - self.retired,
            deferred: self.deferred,
        }
    }

    /// Admit eligible commands using the caller's sole producer or Runtime.
    ///
    /// Horizon is inclusive. Target equal to `rendered_frames` still needs an
    /// outstanding credit; only earlier targets retire. Errors before admission
    /// preserve state. Queue failure retains accepted cursor/credits and prefix,
    /// leaving the exact failed command unadmitted. The caller chooses recovery.
    pub fn feed(
        &mut self,
        rendered_frames: u64,
        budget: usize,
        mut admit: impl FnMut(AudioCommand) -> Result<(), CommandPushError>,
    ) -> Result<BgmFeedReport, BgmFeedError> {
        if !(1..=AudioLimits::MAX_COMMANDS).contains(&budget) {
            return Err(BgmFeedError::InvalidConfiguration(
                "admission budget must be 1..=65536",
            ));
        }
        if rendered_frames < self.cursor {
            return Err(BgmFeedError::CursorRegression {
                previous: self.cursor,
                received: rendered_frames,
            });
        }
        let horizon = rendered_frames
            .checked_add(self.lookahead_frames)
            .ok_or(BgmFeedError::Overflow)?;
        if let Some(cue) = self
            .cues
            .get(self.next)
            .filter(|cue| cue.frame < rendered_frames)
        {
            return Err(BgmFeedError::Late {
                command: cue.command,
                target_frame: cue.frame,
                rendered_frames,
            });
        }
        self.cursor = rendered_frames;
        while self.retired < self.next && self.cues[self.retired].frame < rendered_frames {
            self.retired += 1;
        }
        let mut admitted = 0;
        while admitted < budget && self.next - self.retired < self.config.max_pending {
            let Some(cue) = self.cues.get(self.next).filter(|cue| cue.frame <= horizon) else {
                break;
            };
            if let Err(error) = admit(cue.command) {
                self.deferred = true;
                return Err(BgmFeedError::Admission(error));
            }
            self.next += 1;
            admitted += 1;
        }
        self.deferred = self
            .cues
            .get(self.next)
            .is_some_and(|cue| cue.frame <= horizon);
        Ok(BgmFeedReport {
            admitted,
            ..self.report()
        })
    }
}

fn ceil_frames(nanos: i128, sample_rate: u32) -> Result<u64, BgmFeedError> {
    let scaled = nanos
        .checked_mul(i128::from(sample_rate))
        .ok_or(BgmFeedError::Overflow)?;
    let frames =
        scaled.div_euclid(1_000_000_000) + i128::from(scaled.rem_euclid(1_000_000_000) != 0);
    u64::try_from(frames).map_err(|_| BgmFeedError::Overflow)
}
