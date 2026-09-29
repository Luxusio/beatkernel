/// An exact signed duration in integer nanoseconds.
///
/// All arithmetic returns `None` on overflow rather than wrapping or
/// saturating. A negative duration is valid.
///
/// ```
/// use beatkernel::time::Duration;
/// assert_eq!(Duration::from_nanos(-5).checked_mul(2),
///            Some(Duration::from_nanos(-10)));
/// assert_eq!(Duration::MIN.checked_neg(), None);
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(i64);

impl Duration {
    /// A duration of zero nanoseconds.
    pub const ZERO: Self = Self(0);
    /// The smallest representable signed duration.
    pub const MIN: Self = Self(i64::MIN);
    /// The largest representable signed duration.
    pub const MAX: Self = Self(i64::MAX);

    /// Constructs an exact signed duration from nanoseconds.
    pub const fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    /// Returns the exact signed nanosecond representation.
    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    /// Adds another duration, returning `None` on overflow.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Subtracts another duration, returning `None` on overflow.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Negates this duration, returning `None` for [`Self::MIN`].
    pub fn checked_neg(self) -> Option<Self> {
        self.0.checked_neg().map(Self)
    }

    /// Multiplies by a signed integer, returning `None` on overflow.
    pub fn checked_mul(self, factor: i64) -> Option<Self> {
        self.0.checked_mul(factor).map(Self)
    }
}
