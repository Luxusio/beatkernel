use super::{Duration, Timestamp};

/// Identifies a clock domain within one runtime instance.
///
/// The caller assigns distinct IDs to clocks whose timestamps cannot be
/// compared directly. An ID itself does not establish any clock relation.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClockDomainId(pub u32);

/// A timestamp paired with the domain in which it was measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClockPoint {
    /// The originating clock domain.
    pub domain: ClockDomainId,
    /// The timestamp in that domain.
    pub timestamp: Timestamp,
}

/// Describes the uncertainty of a clock mapper's supported relations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMappingQuality {
    /// A supported relation has no mapping error.
    Exact,
    /// A supported relation has a bounded estimated error.
    Estimated {
        /// The nonnegative maximum absolute error in nanoseconds.
        ///
        /// Implementations must report a value greater than or equal to zero.
        max_error: Duration,
    },
    /// The error is unknown or no relation is established.
    Unknown,
}

/// Explicitly maps timestamps between clock domains.
///
/// Implementations must not silently equate distinct clock domains. Native
/// calibration and clock acquisition are outside the Phase 0/1 core.
///
/// ```
/// use beatkernel::time::{ClockDomainId, ClockMapper, ClockMappingQuality,
///                        ClockPoint, Timestamp};
/// struct SameDomain;
/// impl ClockMapper for SameDomain {
///     fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
///         (from.domain == to).then_some(from.timestamp)
///     }
///     fn quality(&self) -> ClockMappingQuality { ClockMappingQuality::Exact }
/// }
/// let point = ClockPoint { domain: ClockDomainId(1), timestamp: Timestamp::ZERO };
/// assert_eq!(SameDomain.map(point, ClockDomainId(2)), None);
/// ```
pub trait ClockMapper {
    /// Returns the timestamp in `to`, or `None` for an unsupported relation
    /// or an unrepresentable mapped value.
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp>;

    /// Returns the quality of the supported clock relations.
    ///
    /// Quality does not imply that every requested domain pair is supported.
    fn quality(&self) -> ClockMappingQuality;
}
