//! Fixed positive replay speed and checked original-song presentation mapping.
use beatkernel::{
    time::{Duration, Timestamp},
    transport::Rate,
};
use std::{fmt, str::FromStr};

/// A normalized, strictly forward pitch-changing replay speed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReplaySpeed(Rate);

/// Invalid replay speed input or unrepresentable song presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaySpeedError {
    InvalidFraction,
    ComponentOverflow,
    NonPositiveRate,
    NegativePreroll,
    NegativeElapsed,
    Overflow,
}

impl fmt::Display for ReplaySpeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidFraction => {
                "speed must be NUM/DEN with positive unsigned ASCII decimal components"
            }
            Self::ComponentOverflow => "number too large to fit in target type",
            Self::NonPositiveRate => "speed numerator and denominator must be positive",
            Self::NegativePreroll => "replay preroll must be nonnegative",
            Self::NegativeElapsed => "replay output elapsed time must be nonnegative",
            Self::Overflow => "replay song presentation timestamp overflow",
        })
    }
}

impl std::error::Error for ReplaySpeedError {}

impl ReplaySpeed {
    /// Validates a core rate once at replay setup; zero and reverse reject.
    pub fn new(rate: Rate) -> Result<Self, ReplaySpeedError> {
        if rate.numerator() <= 0 {
            return Err(ReplaySpeedError::NonPositiveRate);
        }
        Ok(Self(rate))
    }

    pub const fn rate(self) -> Rate {
        self.0
    }

    /// Maps genuine output elapsed time, after removal of any output pause gap.
    ///
    /// Preroll is an output duration: scale `(elapsed - preroll)`, then add the
    /// original start. A validated original finite end caps before narrowing.
    pub fn song_at_elapsed(
        self,
        start: Timestamp,
        preroll: Duration,
        elapsed_ns: i128,
        end: Option<Timestamp>,
    ) -> Result<Timestamp, ReplaySpeedError> {
        if preroll.as_nanos() < 0 {
            return Err(ReplaySpeedError::NegativePreroll);
        }
        if elapsed_ns < 0 {
            return Err(ReplaySpeedError::NegativeElapsed);
        }
        let elapsed = elapsed_ns - i128::from(preroll.as_nanos());
        let numerator = i128::from(self.0.numerator());
        let denominator = i128::from(self.0.denominator());
        // Divide before multiplying the whole part. The bounded remainder's
        // product always fits i128, even for maximal rate components.
        let remainder = (elapsed % denominator) * numerator / denominator;
        let song = (elapsed / denominator)
            .checked_mul(numerator)
            .and_then(|whole| whole.checked_add(remainder))
            .and_then(|scaled| scaled.checked_add(i128::from(start.as_nanos())));
        let song = match (song, end) {
            (Some(song), end) => end.map_or(song, |end| song.min(i128::from(end.as_nanos()))),
            // Only positive elapsed can exceed i128: the negative preroll is
            // bounded by i64. Such a value certainly exceeds every finite end,
            // even after adding the smallest possible original start.
            (None, Some(end)) if elapsed >= 0 => return Ok(end),
            (None, _) => return Err(ReplaySpeedError::Overflow),
        };
        i64::try_from(song)
            .map(Timestamp::from_nanos)
            .map_err(|_| ReplaySpeedError::Overflow)
    }
}

impl FromStr for ReplaySpeed {
    type Err = ReplaySpeedError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (numerator, denominator) = value
            .split_once('/')
            .ok_or(ReplaySpeedError::InvalidFraction)?;
        if [numerator, denominator]
            .into_iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(ReplaySpeedError::InvalidFraction);
        }
        let numerator: i64 = numerator
            .parse()
            .map_err(|_| ReplaySpeedError::ComponentOverflow)?;
        let denominator: u64 = denominator
            .parse()
            .map_err(|_| ReplaySpeedError::ComponentOverflow)?;
        if numerator == 0 || denominator == 0 {
            return Err(ReplaySpeedError::NonPositiveRate);
        }
        let rate =
            Rate::new(numerator, denominator).map_err(|_| ReplaySpeedError::NonPositiveRate)?;
        Self::new(rate)
    }
}

impl fmt::Display for ReplaySpeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.0.numerator(), self.0.denominator())
    }
}
