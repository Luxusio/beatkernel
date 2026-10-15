use super::Rate;
use crate::time::{Duration, Timestamp};
use std::fmt;

/// Exact positive-rate song phase at one piecewise playback segment.
///
/// The original song start stays separate from the rational phase: position is
/// `start + trunc(phase)`, including during negative preroll. A rate change
/// retains the complete phase rather than starting again from its integer song
/// position. Elapsed values are cumulative active output nanoseconds; callers
/// retain older segments when they need to map historical input.
///
/// This value performs no allocation and does not establish audio presentation
/// or clock-domain authority. A finite end is only an upper position cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositiveRatePhase {
    original_start: Timestamp,
    original_end: Option<Timestamp>,
    rate: Rate,
    base_elapsed: i128,
    base: PhaseValue,
    common_denominator: u64,
}

/// An invalid exact positive-rate setup, query, or segment change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositiveRatePhaseError {
    /// Output preroll must be nonnegative.
    NegativePreroll,
    /// The playback rate must be positive.
    NonPositiveRate,
    /// The finite original end preceded the original start.
    InvalidEnd,
    /// Active output elapsed must be nonnegative.
    NegativeElapsed,
    /// Active output elapsed preceded this segment's base.
    RegressingElapsed,
    /// The required phase, song position, or elapsed value did not fit.
    Overflow,
    /// The exact fractional denominator exceeded the bounded representation.
    DenominatorCapacity,
}

impl fmt::Display for PositiveRatePhaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NegativePreroll => "output preroll must be nonnegative",
            Self::NonPositiveRate => "playback rate must be positive",
            Self::InvalidEnd => "original end precedes original start",
            Self::NegativeElapsed => "active output elapsed must be nonnegative",
            Self::RegressingElapsed => "active output elapsed precedes the phase segment",
            Self::Overflow => "exact phase, song position, or active elapsed overflow",
            Self::DenominatorCapacity => "exact phase denominator exceeds bounded storage",
        })
    }
}

impl std::error::Error for PositiveRatePhaseError {}

// Floor-normalized: value = whole + numerator / denominator. The stored base
// has a reduced proper fraction, with zero represented as 0/1, even for
// negative values. Query intermediates retain the common denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhaseValue {
    whole: i128,
    numerator: u64,
    denominator: u64,
}

impl PositiveRatePhase {
    /// Creates `start + trunc((E - preroll) * rate)` at active elapsed zero.
    ///
    /// Setup errors are checked in preroll, rate, end, then initial-position
    /// order. A finite end cannot rescue an overflowing negative preroll.
    pub fn new(
        original_start: Timestamp,
        output_preroll: Duration,
        rate: Rate,
        original_end: Option<Timestamp>,
    ) -> Result<Self, PositiveRatePhaseError> {
        if output_preroll.as_nanos() < 0 {
            return Err(PositiveRatePhaseError::NegativePreroll);
        }
        if rate.numerator() <= 0 {
            return Err(PositiveRatePhaseError::NonPositiveRate);
        }
        if original_end.is_some_and(|end| end < original_start) {
            return Err(PositiveRatePhaseError::InvalidEnd);
        }

        // Both factors fit positive i64, so this product fits i128.
        let product = i128::from(output_preroll.as_nanos()) * i128::from(rate.numerator());
        let denominator = i128::from(rate.denominator());
        let remainder = product % denominator;
        let whole = -(product / denominator) - i128::from(remainder != 0);
        let (numerator, denominator) = reduced_fraction(
            if remainder == 0 {
                0
            } else {
                (denominator - remainder) as u128
            },
            denominator as u128,
        )?;
        let phase = Self {
            original_start,
            original_end,
            rate,
            base_elapsed: 0,
            base: PhaseValue {
                whole,
                numerator,
                denominator,
            },
            common_denominator: rate.denominator(),
        };
        phase.song_from_value(phase.base)?;
        Ok(phase)
    }

    /// Returns this segment's normalized positive playback rate.
    pub const fn rate(self) -> Rate {
        self.rate
    }

    /// Returns the cumulative active output elapsed at this segment's base.
    pub const fn base_elapsed(self) -> i128 {
        self.base_elapsed
    }

    /// Maps an elapsed point in this segment to its capped original song time.
    ///
    /// An overflowing positive future phase can safely reach a finite end.
    /// Queries never advance segment chronology.
    pub fn song_at_elapsed(self, elapsed: i128) -> Result<Timestamp, PositiveRatePhaseError> {
        match (self.value_at_elapsed(elapsed), self.original_end) {
            (Ok(value), _) => self.song_from_value(value),
            (Err(PositiveRatePhaseError::Overflow), Some(end)) => {
                // Phase only increases from a validated representable base.
                Ok(end)
            }
            (Err(error), _) => Err(error),
        }
    }

    /// Returns the uncapped exact original song phase as a floor whole and a
    /// reduced proper fraction. Zero fractions use denominator one.
    ///
    /// This is `start + Q`, distinct from the displayed `start + trunc(Q)`
    /// during negative preroll. It preserves actual cut metadata without
    /// reconstructing a fractional phase from an integer song snapshot.
    /// Both the retained `Q` floor and the resulting original-song floor must
    /// fit i128, as required when retaining an actual rate-cut segment.
    pub fn original_song_phase_at_elapsed(
        self,
        elapsed: i128,
    ) -> Result<(i128, u64, u64), PositiveRatePhaseError> {
        let value = self.value_at_elapsed(elapsed)?;
        let whole = value
            .whole
            .checked_add(i128::from(self.original_start.as_nanos()))
            .ok_or(PositiveRatePhaseError::Overflow)?;
        let (numerator, denominator) =
            reduced_fraction(u128::from(value.numerator), u128::from(value.denominator))?;
        Ok((whole, numerator, denominator))
    }

    /// Prepares a new segment at an exact elapsed cut, without mutating `self`.
    ///
    /// Equal reduced rates validate the query and return exactly the old value.
    /// A real change preserves the full phase and proves that its fractional
    /// denominator will fit at every subsequent integer elapsed point.
    pub fn with_rate_at_elapsed(
        self,
        elapsed: i128,
        rate: Rate,
    ) -> Result<Self, PositiveRatePhaseError> {
        if rate.numerator() <= 0 {
            return Err(PositiveRatePhaseError::NonPositiveRate);
        }
        if rate == self.rate {
            self.song_at_elapsed(elapsed)?;
            return Ok(self);
        }
        let mut base = self.value_at_elapsed(elapsed)?;
        self.song_from_value(base)?;
        (base.numerator, base.denominator) =
            reduced_fraction(u128::from(base.numerator), u128::from(base.denominator))?;

        let denominator = u128::from(base.denominator);
        let rate_denominator = u128::from(rate.denominator());
        let common = (denominator / gcd(denominator, rate_denominator)) * rate_denominator;
        if common > u128::from(u64::MAX) {
            return Err(PositiveRatePhaseError::DenominatorCapacity);
        }
        Ok(Self {
            rate,
            base_elapsed: elapsed,
            base,
            common_denominator: common as u64,
            ..self
        })
    }

    /// Finds the first elapsed integer at or after this segment's base that
    /// reaches an original-song scheduling threshold, ignoring the finite cap.
    ///
    /// For `T = target - original_start`, nonnegative targets require `Q >= T`;
    /// negative targets require `Q > T - 1`. This preserves original cue
    /// scheduling through negative preroll rather than starting a cue early
    /// when integer truncation first displays the song start.
    pub fn elapsed_at_original_song(
        self,
        target: Timestamp,
    ) -> Result<i128, PositiveRatePhaseError> {
        let offset = i128::from(target.as_nanos()) - i128::from(self.original_start.as_nanos());
        let strict = offset < 0;
        let threshold = offset - i128::from(strict);
        let satisfied = if strict {
            self.base.whole > threshold
                || (self.base.whole == threshold && self.base.numerator != 0)
        } else {
            self.base.whole >= threshold
        };
        if satisfied {
            return Ok(self.base_elapsed);
        }
        if self.base.whole == threshold {
            // Only strict equality with a zero fraction remains. Every positive
            // rate exceeds that threshold at the next integer elapsed point.
            return self
                .base_elapsed
                .checked_add(1)
                .ok_or(PositiveRatePhaseError::Overflow);
        }

        let gap = positive_difference(threshold, self.base.whole);
        let n = self.rate.numerator() as u128;
        let d = u128::from(self.rate.denominator());
        let g = u128::from(self.base.denominator);
        let f = u128::from(self.base.numerator);

        // The required distance is (gap - f/g) * d/n. Decompose gap-1
        // before multiplying, leaving a remainder smaller than n. This avoids
        // a potentially overflowing gap*g*d product without a search loop.
        let quotient = (gap - 1) / n;
        let remainder = (gap - 1) % n;
        let a = (remainder + 1) * d;
        let b = a / n;
        let left = (a % n) * g;
        let right = f * d;
        let divisor = g * n;

        // After removing quotient*d+b, the remaining fraction is
        // (left-right)/(g*n). left is smaller than g*n, so the positive
        // residual needs at most one upward rounding. The negative residual
        // uses floor/ceil directly, avoiding signed wide products.
        let residual = if strict {
            let floor = if left >= right {
                b
            } else {
                let difference = right - left;
                b.checked_sub(
                    difference / divisor + u128::from(!difference.is_multiple_of(divisor)),
                )
                .ok_or(PositiveRatePhaseError::Overflow)?
            };
            floor.checked_add(1)
        } else if left >= right {
            b.checked_add(u128::from(left > right))
        } else {
            b.checked_sub((right - left) / divisor)
        }
        .ok_or(PositiveRatePhaseError::Overflow)?;
        let distance = quotient
            .checked_mul(d)
            .and_then(|whole| whole.checked_add(residual))
            .and_then(|value| i128::try_from(value).ok())
            .ok_or(PositiveRatePhaseError::Overflow)?;
        self.base_elapsed
            .checked_add(distance)
            .ok_or(PositiveRatePhaseError::Overflow)
    }

    fn value_at_elapsed(self, elapsed: i128) -> Result<PhaseValue, PositiveRatePhaseError> {
        if elapsed < 0 {
            return Err(PositiveRatePhaseError::NegativeElapsed);
        }
        if elapsed < self.base_elapsed {
            return Err(PositiveRatePhaseError::RegressingElapsed);
        }
        let elapsed = (elapsed - self.base_elapsed) as u128;
        let n = self.rate.numerator() as u128;
        let d = u128::from(self.rate.denominator());
        let remainder_product = (elapsed % d) * n;
        let increment = (elapsed / d)
            .checked_mul(n)
            .and_then(|whole| whole.checked_add(remainder_product / d))
            .ok_or(PositiveRatePhaseError::Overflow)?;
        // Both proper fractions fit the cold-validated common denominator.
        // Their sum fits u128 even at u64::MAX, avoiding gcd work per query.
        let common = u128::from(self.common_denominator);
        let fraction = u128::from(self.base.numerator)
            * (common / u128::from(self.base.denominator))
            + (remainder_product % d) * (common / d);
        let carry = fraction >= common;
        let increment = increment
            .checked_add(u128::from(carry))
            .ok_or(PositiveRatePhaseError::Overflow)?;
        Ok(PhaseValue {
            whole: add_nonnegative(self.base.whole, increment)?,
            numerator: (fraction % common) as u64,
            denominator: self.common_denominator,
        })
    }

    fn song_from_value(self, value: PhaseValue) -> Result<Timestamp, PositiveRatePhaseError> {
        let truncated = value.whole + i128::from(value.whole < 0 && value.numerator != 0);
        if let Some(end) = self.original_end {
            let limit = i128::from(end.as_nanos()) - i128::from(self.original_start.as_nanos());
            if truncated >= limit {
                return Ok(end);
            }
        }
        truncated
            .checked_add(i128::from(self.original_start.as_nanos()))
            .and_then(|song| i64::try_from(song).ok())
            .map(Timestamp::from_nanos)
            .ok_or(PositiveRatePhaseError::Overflow)
    }
}

// Reduce wide intermediates before narrowing the retained proper fraction.
fn reduced_fraction(
    numerator: u128,
    denominator: u128,
) -> Result<(u64, u64), PositiveRatePhaseError> {
    let divisor = gcd(numerator, denominator);
    let numerator = u64::try_from(numerator / divisor)
        .map_err(|_| PositiveRatePhaseError::DenominatorCapacity)?;
    let denominator = u64::try_from(denominator / divisor)
        .map_err(|_| PositiveRatePhaseError::DenominatorCapacity)?;
    Ok((numerator, denominator))
}

fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn positive_difference(upper: i128, lower: i128) -> u128 {
    if upper >= 0 && lower < 0 {
        // This full signed span fits u128, even when subtraction cannot fit i128.
        upper as u128 + lower.unsigned_abs()
    } else {
        (upper - lower) as u128
    }
}

fn add_nonnegative(whole: i128, increment: u128) -> Result<i128, PositiveRatePhaseError> {
    if whole < 0 {
        let magnitude = whole.unsigned_abs();
        if increment < magnitude {
            let difference = magnitude - increment;
            return if difference == i128::MIN.unsigned_abs() {
                Ok(i128::MIN)
            } else {
                Ok(-(difference as i128))
            };
        }
        i128::try_from(increment - magnitude).map_err(|_| PositiveRatePhaseError::Overflow)
    } else {
        increment
            .checked_add(whole as u128)
            .and_then(|value| i128::try_from(value).ok())
            .ok_or(PositiveRatePhaseError::Overflow)
    }
}
