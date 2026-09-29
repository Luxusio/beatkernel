use crate::time::Duration;
use std::fmt;

/// A normalized rational playback rate with a signed numerator.
///
/// The denominator is positive and fractions are reduced, including the
/// canonical zero `0/1`. Scaling multiplies in `i128` before dividing, then
/// truncates toward zero. There is no floating point timing representation.
///
/// ```
/// use beatkernel::{time::Duration, transport::Rate};
/// let half_reverse = Rate::new(-2, 4)?;
/// assert_eq!(half_reverse, Rate::new(-1, 2)?);
/// assert_eq!(half_reverse.scale(Duration::from_nanos(3)),
///            Some(Duration::from_nanos(-1)));
/// # Ok::<(), beatkernel::transport::RateError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rate {
    numerator: i64,
    denominator: u64,
}

/// Invalid rational rate construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateError {
    /// The denominator was zero.
    ZeroDenominator,
}

impl fmt::Display for RateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDenominator => f.write_str("rate denominator must be nonzero"),
        }
    }
}

impl std::error::Error for RateError {}

impl Rate {
    /// Normal forward playback at one song nanosecond per host nanosecond.
    pub const NORMAL: Self = Self {
        numerator: 1,
        denominator: 1,
    };
    /// Paused playback.
    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    /// Reverse playback at normal speed.
    pub const REVERSE: Self = Self {
        numerator: -1,
        denominator: 1,
    };

    /// Constructs a reduced fraction, rejecting a zero denominator.
    ///
    /// Every signed numerator, including `i64::MIN`, is supported. Zero is
    /// normalized to `0/1` for every valid denominator.
    pub fn new(numerator: i64, denominator: u64) -> Result<Self, RateError> {
        if denominator == 0 {
            return Err(RateError::ZeroDenominator);
        }
        let divisor = gcd(numerator.unsigned_abs(), denominator);
        Ok(Self {
            numerator: (i128::from(numerator) / i128::from(divisor)) as i64,
            denominator: denominator / divisor,
        })
    }

    /// Returns the reduced signed numerator.
    pub const fn numerator(self) -> i64 {
        self.numerator
    }

    /// Returns the reduced positive denominator.
    pub const fn denominator(self) -> u64 {
        self.denominator
    }

    /// Scales a duration with truncation toward zero.
    ///
    /// Returns `None` when the final scaled duration is outside `i64` range.
    pub fn scale(self, duration: Duration) -> Option<Duration> {
        i64::try_from(self.scale_wide(i128::from(duration.as_nanos())))
            .ok()
            .map(Duration::from_nanos)
    }

    // Transport's full i64 timestamp span also fits this i128 product.
    pub(super) fn scale_wide(self, nanos: i128) -> i128 {
        nanos * i128::from(self.numerator) / i128::from(self.denominator)
    }
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}
