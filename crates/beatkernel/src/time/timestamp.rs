use super::Duration;

/// An exact signed nanosecond timestamp in a caller-defined clock domain.
///
/// Negative values support song preroll and reverse playback. Arithmetic is
/// checked explicitly; overflow returns `None` in debug and release builds.
///
/// ```
/// use beatkernel::time::{Duration, Timestamp};
/// let start = Timestamp::from_nanos(-10);
/// assert_eq!(start.checked_add(Duration::from_nanos(15)),
///            Some(Timestamp::from_nanos(5)));
/// assert_eq!(Timestamp::MAX.checked_add(Duration::from_nanos(1)), None);
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The zero timestamp.
    pub const ZERO: Self = Self(0);
    /// The smallest representable timestamp.
    pub const MIN: Self = Self(i64::MIN);
    /// The largest representable timestamp.
    pub const MAX: Self = Self(i64::MAX);

    /// Constructs an exact timestamp from signed nanoseconds.
    pub const fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    /// Returns the exact signed nanosecond representation.
    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    /// Adds a duration, returning `None` if the result is unrepresentable.
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        self.0.checked_add(duration.as_nanos()).map(Self)
    }

    /// Subtracts a duration, returning `None` if the result is unrepresentable.
    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        self.0.checked_sub(duration.as_nanos()).map(Self)
    }

    /// Computes `self - earlier`, allowing negative differences.
    ///
    /// Returns `None` if the difference cannot fit a signed duration.
    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_nanos)
    }
}
