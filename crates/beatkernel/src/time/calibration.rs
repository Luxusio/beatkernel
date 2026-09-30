//! Integer affine clock mapping from explicit observations and finite validity.
//! Caller-provided uncertainty remains an estimate; no OS clock or device is read.
use super::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp};

/// One caller-supplied simultaneous/model-related clock observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockPair {
    /// Observation in the source clock domain.
    pub source: ClockPoint,
    /// Corresponding observation in the target clock domain.
    pub target: ClockPoint,
}
/// Inclusive finite timestamp interval in the domain documented by its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockInterval {
    /// First valid timestamp, inclusive.
    pub start: Timestamp,
    /// Last valid timestamp, inclusive; must not precede start.
    pub end: Timestamp,
}
/// Explicit permission to extend validity beyond the observed anchor span.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExtrapolationPolicy {
    /// Validity must lie entirely within the two supplied observations.
    #[default]
    Forbid,
    /// Permit only these finite source-domain distances outside the anchor span.
    Bounded {
        /// Nonnegative maximum distance before the first source observation.
        before: Duration,
        /// Nonnegative maximum distance after the second source observation.
        after: Duration,
    },
}
/// Caller-estimated affine-model error, not a physical guarantee from the core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationUncertainty {
    /// Nonnegative target-domain observation/model error assigned by the caller.
    pub observation_error: Duration,
    /// Nonnegative target-domain residual/drift error over the entire validity
    /// interval, including extrapolation; None means its bound is unknown.
    pub residual_drift_error: Option<Duration>,
}
/// Precise calibration setup/query rejection, before producing a timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibrationError {
    /// Source and target IDs must describe distinct clocks/origins.
    IdenticalDomains,
    /// The second observation changed source or target clock identity.
    PairDomainMismatch,
    /// Both source and target anchors must strictly increase.
    NonIncreasingAnchors,
    /// Inclusive validity end precedes its start.
    InvalidInterval,
    /// Observation/drift estimate was negative.
    NegativeUncertainty,
    /// Extrapolation distance was negative.
    NegativeExtrapolation,
    /// Validity exceeded the explicitly permitted observation/extrapolation span.
    ValidityOutsideEnvelope,
    /// Requested relation is not one of this calibration's declared domains.
    DomainMismatch {
        /// Incoming clock domain.
        from: ClockDomainId,
        /// Requested destination clock domain.
        to: ClockDomainId,
    },
    /// Incoming timestamp lies outside the finite accepted interval.
    OutsideValidity {
        /// Domain in which the rejected timestamp was supplied.
        domain: ClockDomainId,
    },
    /// Checked arithmetic or the resulting signed timestamp overflowed.
    Overflow,
}
impl std::fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdenticalDomains => f.write_str("clock calibration requires distinct domain IDs"),
            Self::PairDomainMismatch => f.write_str("calibration observation domains changed"),
            Self::NonIncreasingAnchors => {
                f.write_str("source and target anchors must strictly increase")
            }
            Self::InvalidInterval => f.write_str("calibration validity interval is reversed"),
            Self::NegativeUncertainty => {
                f.write_str("clock uncertainty estimates must be nonnegative")
            }
            Self::NegativeExtrapolation => {
                f.write_str("clock extrapolation limits must be nonnegative")
            }
            Self::ValidityOutsideEnvelope => {
                f.write_str("calibration validity exceeds its permitted observation span")
            }
            Self::DomainMismatch { from, to } => {
                write!(f, "unsupported clock relation {} to {}", from.0, to.0)
            }
            Self::OutsideValidity { domain } => write!(
                f,
                "clock domain {} timestamp is outside calibration validity",
                domain.0
            ),
            Self::Overflow => f.write_str("clock calibration arithmetic/timestamp overflow"),
        }
    }
}
impl std::error::Error for CalibrationError {}

/// Immutable allocation-free affine mapper with an explicitly bounded lifetime.
///
/// Signed rational deltas truncate toward zero. Inverse quantization is not
/// bijective: rounded boundary values clamp to the finite source interval.
/// Hardware observations always report Estimated or Unknown, never Exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AffineClockMapper {
    first: ClockPair,
    second: Option<ClockPair>,
    numerator: i128,
    denominator: i128,
    source_interval: ClockInterval,
    target_interval: ClockInterval,
    extrapolation: ExtrapolationPolicy,
    uncertainty: Option<CalibrationUncertainty>,
    quality: ClockMappingQuality,
}
impl AffineClockMapper {
    /// Constructs a positive rational relation from two ordered observations.
    /// Validity uses source-domain timestamps, with explicit extrapolation policy.
    pub fn from_pairs(
        first: ClockPair,
        second: ClockPair,
        validity: ClockInterval,
        extrapolation: ExtrapolationPolicy,
        uncertainty: CalibrationUncertainty,
    ) -> Result<Self, CalibrationError> {
        if first.source.domain == first.target.domain {
            return Err(CalibrationError::IdenticalDomains);
        }
        if second.source.domain != first.source.domain
            || second.target.domain != first.target.domain
        {
            return Err(CalibrationError::PairDomainMismatch);
        }
        let source_span = i128::from(second.source.timestamp.as_nanos())
            - i128::from(first.source.timestamp.as_nanos());
        let target_span = i128::from(second.target.timestamp.as_nanos())
            - i128::from(first.target.timestamp.as_nanos());
        if source_span <= 0 || target_span <= 0 {
            return Err(CalibrationError::NonIncreasingAnchors);
        }
        validate_interval(validity)?;
        if uncertainty.observation_error < Duration::ZERO
            || uncertainty
                .residual_drift_error
                .is_some_and(|error| error < Duration::ZERO)
        {
            return Err(CalibrationError::NegativeUncertainty);
        }
        let (before, after) = match extrapolation {
            ExtrapolationPolicy::Forbid => (0, 0),
            ExtrapolationPolicy::Bounded { before, after } => {
                if before < Duration::ZERO || after < Duration::ZERO {
                    return Err(CalibrationError::NegativeExtrapolation);
                }
                (i128::from(before.as_nanos()), i128::from(after.as_nanos()))
            }
        };
        if i128::from(validity.start.as_nanos())
            < i128::from(first.source.timestamp.as_nanos()) - before
            || i128::from(validity.end.as_nanos())
                > i128::from(second.source.timestamp.as_nanos()) + after
        {
            return Err(CalibrationError::ValidityOutsideEnvelope);
        }
        let divisor = gcd(source_span, target_span);
        let numerator = target_span / divisor;
        let denominator = source_span / divisor;
        Self::build(
            first,
            Some(second),
            numerator,
            denominator,
            validity,
            extrapolation,
            Some(uncertainty),
            estimated_quality(numerator, denominator, uncertainty),
        )
    }
    /// Declares a genuinely known, exact identical-rate relation from one pair.
    /// Do not use this constructor merely because hardware observations appear
    /// to have equal rates: that would incorrectly assert an exact relation.
    pub fn exact_offset(
        anchor: ClockPair,
        validity: ClockInterval,
    ) -> Result<Self, CalibrationError> {
        if anchor.source.domain == anchor.target.domain {
            return Err(CalibrationError::IdenticalDomains);
        }
        validate_interval(validity)?;
        Self::build(
            anchor,
            None,
            1,
            1,
            validity,
            ExtrapolationPolicy::Forbid,
            None,
            ClockMappingQuality::Exact,
        )
    }
    fn build(
        first: ClockPair,
        second: Option<ClockPair>,
        numerator: i128,
        denominator: i128,
        validity: ClockInterval,
        extrapolation: ExtrapolationPolicy,
        uncertainty: Option<CalibrationUncertainty>,
        quality: ClockMappingQuality,
    ) -> Result<Self, CalibrationError> {
        let start = affine(
            validity.start,
            first.source.timestamp,
            first.target.timestamp,
            numerator,
            denominator,
        )?;
        let end = affine(
            validity.end,
            first.source.timestamp,
            first.target.timestamp,
            numerator,
            denominator,
        )?;
        Ok(Self {
            first,
            second,
            numerator,
            denominator,
            source_interval: validity,
            target_interval: ClockInterval { start, end },
            extrapolation,
            uncertainty,
            quality,
        })
    }
    /// Returns unchanged supplied observations; exact-offset has one anchor.
    pub const fn anchors(&self) -> (ClockPair, Option<ClockPair>) {
        (self.first, self.second)
    }
    /// Source-domain inclusive finite lifetime.
    pub const fn source_interval(&self) -> ClockInterval {
        self.source_interval
    }
    /// Target-domain rounded image of the source interval's endpoints.
    pub const fn target_interval(&self) -> ClockInterval {
        self.target_interval
    }
    /// Reduced positive target/source rate, each span fits u64.
    pub const fn rate(&self) -> (u64, u64) {
        (self.numerator as u64, self.denominator as u64)
    }
    /// Explicit extrapolation permission retained from setup.
    pub const fn extrapolation(&self) -> ExtrapolationPolicy {
        self.extrapolation
    }
    /// Caller-provided estimates, or None for a declared exact-offset relation.
    pub const fn uncertainty(&self) -> Option<CalibrationUncertainty> {
        self.uncertainty
    }
    /// Supplies explicit errors beyond the ClockMapper trait's Option boundary.
    /// Identity queries also obey the recognized domain's validity interval.
    pub fn map_checked(
        &self,
        from: ClockPoint,
        to: ClockDomainId,
    ) -> Result<Timestamp, CalibrationError> {
        let source = self.first.source.domain;
        let target = self.first.target.domain;
        if (from.domain != source && from.domain != target) || (to != source && to != target) {
            return Err(CalibrationError::DomainMismatch {
                from: from.domain,
                to,
            });
        }
        let interval = if from.domain == source {
            self.source_interval
        } else {
            self.target_interval
        };
        if from.timestamp < interval.start || from.timestamp > interval.end {
            return Err(CalibrationError::OutsideValidity {
                domain: from.domain,
            });
        }
        if from.domain == to {
            return Ok(from.timestamp);
        }
        if from.domain == source {
            affine(
                from.timestamp,
                self.first.source.timestamp,
                self.first.target.timestamp,
                self.numerator,
                self.denominator,
            )
        } else {
            let mapped = affine(
                from.timestamp,
                self.first.target.timestamp,
                self.first.source.timestamp,
                self.denominator,
                self.numerator,
            )?;
            Ok(mapped.clamp(self.source_interval.start, self.source_interval.end))
        }
    }
}
impl ClockMapper for AffineClockMapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        self.map_checked(from, to).ok()
    }
    fn quality(&self) -> ClockMappingQuality {
        self.quality
    }
}
fn validate_interval(interval: ClockInterval) -> Result<(), CalibrationError> {
    if interval.end < interval.start {
        Err(CalibrationError::InvalidInterval)
    } else {
        Ok(())
    }
}
fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}
fn affine(
    value: Timestamp,
    source: Timestamp,
    target: Timestamp,
    numerator: i128,
    denominator: i128,
) -> Result<Timestamp, CalibrationError> {
    let delta = i128::from(value.as_nanos()) - i128::from(source.as_nanos());
    let mapped = i128::from(target.as_nanos())
        .checked_add(scale(delta, numerator, denominator)?)
        .ok_or(CalibrationError::Overflow)?;
    i64::try_from(mapped)
        .map(Timestamp::from_nanos)
        .map_err(|_| CalibrationError::Overflow)
}
fn scale(delta: i128, numerator: i128, denominator: i128) -> Result<i128, CalibrationError> {
    if let Some(product) = delta.checked_mul(numerator) {
        return Ok(product / denominator);
    }
    // Differences/spans fit u64 but their product can exceed signed i128 even
    // when the quotient fits. Split integer and fractional parts; the fraction
    // uses at most 64 doubling steps with every intermediate below 3*u64::MAX.
    let absolute = delta.checked_abs().ok_or(CalibrationError::Overflow)?;
    let whole = (absolute / denominator)
        .checked_mul(numerator)
        .ok_or(CalibrationError::Overflow)?;
    let remainder = absolute % denominator;
    let mut quotient = 0i128;
    let mut residual = 0i128;
    for bit in (0..64).rev() {
        quotient = quotient.checked_mul(2).ok_or(CalibrationError::Overflow)?;
        residual = residual
            .checked_mul(2)
            .and_then(|value| {
                value.checked_add(if (numerator >> bit) & 1 != 0 {
                    remainder
                } else {
                    0
                })
            })
            .ok_or(CalibrationError::Overflow)?;
        quotient = quotient
            .checked_add(residual / denominator)
            .ok_or(CalibrationError::Overflow)?;
        residual %= denominator;
    }
    let value = whole
        .checked_add(quotient)
        .ok_or(CalibrationError::Overflow)?;
    if delta < 0 {
        value.checked_neg().ok_or(CalibrationError::Overflow)
    } else {
        Ok(value)
    }
}
fn estimated_quality(
    numerator: i128,
    denominator: i128,
    uncertainty: CalibrationUncertainty,
) -> ClockMappingQuality {
    let Some(drift) = uncertainty.residual_drift_error else {
        return ClockMappingQuality::Unknown;
    };
    let Some(observed) = uncertainty.observation_error.checked_add(drift) else {
        return ClockMappingQuality::Unknown;
    };
    let rounding = if numerator == denominator { 0 } else { 1 };
    let Some(forward) = i128::from(observed.as_nanos()).checked_add(rounding) else {
        return ClockMappingQuality::Unknown;
    };
    let Ok(inverse_base) = scale(i128::from(observed.as_nanos()), denominator, numerator) else {
        return ClockMappingQuality::Unknown;
    };
    // Ceiling inverse error, plus conservative inverse quantization/clamping.
    // Integer division's remainder is zero iff the reduced numerator divides
    // the supplied target-domain error; avoid multiplying to inspect it.
    let Some(inverse_ceil) =
        inverse_base.checked_add(i128::from(i128::from(observed.as_nanos()) % numerator != 0))
    else {
        return ClockMappingQuality::Unknown;
    };
    let inverse_rounding = if numerator == denominator {
        0
    } else {
        denominator / numerator + i128::from(denominator % numerator != 0) + 1
    };
    let Some(inverse) = inverse_ceil.checked_add(inverse_rounding) else {
        return ClockMappingQuality::Unknown;
    };
    match i64::try_from(forward.max(inverse)) {
        Ok(bound) => ClockMappingQuality::Estimated {
            max_error: Duration::from_nanos(bound),
        },
        Err(_) => ClockMappingQuality::Unknown,
    }
}
