//! Finite caller-bounded relation for a wrapped Windows multimedia timer.
//!
//! Native acquisition and accuracy assessment are outside this module. The
//! caller must establish that both readings use the same timer and that native
//! readings are within the supplied age horizon. Unknown error bounds cannot
//! establish this relation. Raw timer nanoseconds are never treated as QPC.

use beatkernel::time::{ClockPoint, Timestamp};

const NS_PER_MS: i128 = 1_000_000;
const WRAP_NS: i128 = (1i128 << 32) * NS_PER_MS;
const HALF_WRAP_NS: i128 = WRAP_NS / 2;

/// Failure to establish or apply a finite wrapped timer relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultimediaClockError {
    /// Invalid bounds, acquisition chronology or noncanonical raw nanoseconds.
    Malformed,
    /// Observation or native timer displacement is outside the finite horizon.
    Expired,
    /// Supplied host points do not use the anchor's host domain.
    DomainMismatch,
    /// A resulting host timestamp cannot be represented.
    Overflow,
    /// Bounds or modular displacement cannot distinguish a unique wrap epoch.
    Ambiguous,
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatkernel::time::ClockDomainId;

    fn host(ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(ns),
        }
    }

    #[test]
    fn crosses_wrap_without_resetting_host_epoch() {
        let anchor =
            MultimediaClockAnchor::new(u32::MAX, host(10), host(20), 5_000_000, 3, 4).unwrap();
        let interval = anchor.map_wrapped_ns(0, host(1_000_020)).unwrap();
        assert_eq!(interval.before, host(1_000_003));
        assert_eq!(interval.after, host(1_000_027));
    }

    #[test]
    fn rejects_expired_observation_and_wrong_domain() {
        let anchor = MultimediaClockAnchor::new(0, host(0), host(10), 100, 0, 0).unwrap();
        assert_eq!(
            anchor.map_wrapped_ns(0, host(111)),
            Err(MultimediaClockError::Expired)
        );
        let wrong = ClockPoint {
            domain: ClockDomainId(8),
            timestamp: Timestamp::ZERO,
        };
        assert_eq!(
            anchor.map_wrapped_ns(0, wrong),
            Err(MultimediaClockError::DomainMismatch)
        );
    }

    #[test]
    fn rejects_half_period_and_noncanonical_native_time() {
        let anchor = MultimediaClockAnchor::new(0, host(0), host(0), 100, 0, 0).unwrap();
        assert_eq!(
            anchor.map_wrapped_ns(HALF_WRAP_NS as u64, host(0)),
            Err(MultimediaClockError::Ambiguous)
        );
        assert_eq!(
            anchor.map_wrapped_ns(WRAP_NS as u64, host(0)),
            Err(MultimediaClockError::Malformed)
        );
    }

    #[test]
    fn rejects_unrepresentable_host_interval() {
        let anchor =
            MultimediaClockAnchor::new(0, host(i64::MIN), host(i64::MIN), 100, 1, 0).unwrap();
        assert_eq!(
            anchor.map_wrapped_ns(0, host(i64::MIN)),
            Err(MultimediaClockError::Overflow)
        );
        assert_eq!(
            MultimediaClockAnchor::new(0, host(2), host(1), 100, 0, 0),
            Err(MultimediaClockError::Malformed)
        );
    }

    #[test]
    fn renewal_threshold_checks_domain_regression_and_integer_boundaries() {
        let anchor = MultimediaClockAnchor::new(0, host(0), host(10), 5, 0, 0).unwrap();
        assert_eq!(anchor.refresh_due(host(12)), Ok(false));
        assert_eq!(anchor.refresh_due(host(13)), Ok(true));
        assert_eq!(anchor.refresh_due(host(16)), Ok(true));
        assert_eq!(
            anchor.refresh_due(host(9)),
            Err(MultimediaClockError::Malformed)
        );
        let mut wrong = host(13);
        wrong.domain = ClockDomainId(8);
        assert_eq!(
            anchor.refresh_due(wrong),
            Err(MultimediaClockError::DomainMismatch)
        );
        let tiny = MultimediaClockAnchor::new(0, host(0), host(0), 1, 0, 0).unwrap();
        assert_eq!(tiny.refresh_due(host(0)), Ok(false));
        assert_eq!(tiny.refresh_due(host(1)), Ok(true));
        let extreme =
            MultimediaClockAnchor::new(0, host(i64::MIN), host(i64::MIN), 1, 0, 0).unwrap();
        assert_eq!(extreme.refresh_due(host(i64::MAX)), Ok(true));
    }

    #[test]
    fn renewal_rejects_invalid_receipts_without_modifying_original_anchor() {
        let anchor = MultimediaClockAnchor::new(42, host(10), host(20), 100, 3, 4).unwrap();
        let original = anchor;
        assert_eq!(
            anchor.refreshed(43, host(19), host(25)),
            Err(MultimediaClockError::Malformed)
        );
        assert_eq!(
            anchor.refreshed(43, host(25), host(24)),
            Err(MultimediaClockError::Malformed)
        );
        let mut wrong = host(25);
        wrong.domain = ClockDomainId(8);
        assert_eq!(
            anchor.refreshed(43, wrong, wrong),
            Err(MultimediaClockError::DomainMismatch)
        );
        assert_eq!(anchor, original);
        let renewed = anchor.refreshed(43, host(25), host(26)).unwrap();
        assert_eq!(renewed.raw_ms(), 43);
        assert_eq!(renewed.before(), host(25));
        assert_eq!(renewed.after(), host(26));
        assert_eq!(renewed.max_age_ns(), 100);
        assert_eq!(renewed.measurement_error_ns(), 3);
        assert_eq!(renewed.drift_error_ns(), 4);
    }

    #[test]
    fn repeated_renewal_keeps_one_week_host_timeline_across_native_timer_wrap() {
        const HOUR_NS: i64 = 3_600_000_000_000;
        let initial_ms = u32::MAX - 1_000;
        let mut anchor =
            MultimediaClockAnchor::new(initial_ms, host(0), host(0), 2 * HOUR_NS as u64, 3, 4)
                .unwrap();
        for hour in 1..=168 {
            let time = hour * HOUR_NS;
            let raw = initial_ms.wrapping_add((time / 1_000_000) as u32);
            assert_eq!(anchor.refresh_due(host(time)), Ok(true));
            anchor = anchor.refreshed(raw, host(time), host(time)).unwrap();
            let mapped = anchor
                .map_wrapped_ns(u64::from(raw) * 1_000_000, host(time))
                .unwrap();
            assert_eq!(mapped.before, host(time - 7));
            assert_eq!(mapped.after, host(time + 7));
            assert_eq!(anchor.refresh_due(host(time)), Ok(false));
        }
        assert_eq!(anchor.after(), host(7 * 24 * HOUR_NS));
    }
}

impl std::fmt::Display for MultimediaClockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Malformed => "malformed multimedia clock relation or reading",
            Self::Expired => "multimedia clock relation horizon expired",
            Self::DomainMismatch => "multimedia clock host domain mismatch",
            Self::Overflow => "multimedia clock host timestamp overflow",
            Self::Ambiguous => "ambiguous multimedia clock wrap epoch",
        })
    }
}
impl std::error::Error for MultimediaClockError {}

/// Inclusive host interval from actual bracket points and caller-supplied errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultimediaHostInterval {
    /// Earliest possible host point under the explicitly supplied bounds.
    pub before: ClockPoint,
    /// Latest possible host point under the explicitly supplied bounds.
    pub after: ClockPoint,
}

/// One multimedia-timer reading bracketed by actual same-domain host samples.
///
/// This relation assumes equal nominal nanosecond rates with the caller's
/// supplied measurement and drift error bounds. These are assumptions, not
/// physical guarantees inferred from the bracket. No default precision exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultimediaClockAnchor {
    raw_ms: u32,
    before: ClockPoint,
    after: ClockPoint,
    max_age_ns: u64,
    measurement_error_ns: u64,
    drift_error_ns: u64,
}

impl MultimediaClockAnchor {
    /// Establishes a finite relation with explicit observation and drift bounds.
    ///
    /// The bracket must be chronological. Age must be positive and below half
    /// the 2^32-ms period. Twice the age and error bounds plus bracket width
    /// must remain strictly below a full period to exclude wrap ambiguity.
    pub fn new(
        raw_ms: u32,
        before: ClockPoint,
        after: ClockPoint,
        max_age_ns: u64,
        measurement_error_ns: u64,
        drift_error_ns: u64,
    ) -> Result<Self, MultimediaClockError> {
        if before.domain != after.domain {
            return Err(MultimediaClockError::DomainMismatch);
        }
        let width =
            i128::from(after.timestamp.as_nanos()) - i128::from(before.timestamp.as_nanos());
        if width < 0 || max_age_ns == 0 || i128::from(max_age_ns) >= HALF_WRAP_NS {
            return Err(MultimediaClockError::Malformed);
        }
        let errors = i128::from(measurement_error_ns) + i128::from(drift_error_ns);
        if 2 * (i128::from(max_age_ns) + errors) + width >= WRAP_NS {
            return Err(MultimediaClockError::Ambiguous);
        }
        Ok(Self {
            raw_ms,
            before,
            after,
            max_age_ns,
            measurement_error_ns,
            drift_error_ns,
        })
    }

    /// Original wrapped multimedia-timer milliseconds.
    pub const fn raw_ms(&self) -> u32 {
        self.raw_ms
    }

    /// Actual host point acquired before the multimedia reading.
    pub const fn before(&self) -> ClockPoint {
        self.before
    }

    /// Actual host point acquired after the multimedia reading.
    pub const fn after(&self) -> ClockPoint {
        self.after
    }

    /// Explicit finite age bound, in nanoseconds.
    pub const fn max_age_ns(&self) -> u64 {
        self.max_age_ns
    }

    /// Explicitly supplied observation error, in nanoseconds.
    pub const fn measurement_error_ns(&self) -> u64 {
        self.measurement_error_ns
    }

    /// Explicitly supplied drift error across the finite horizon, in nanoseconds.
    pub const fn drift_error_ns(&self) -> u64 {
        self.drift_error_ns
    }

    /// Requests renewal halfway through the finite horizon using an actual host
    /// reading. A different domain or regressed reading is an error, not a reason
    /// to hide a clock discontinuity by acquiring a new anchor.
    pub fn refresh_due(&self, now: ClockPoint) -> Result<bool, MultimediaClockError> {
        if now.domain != self.after.domain {
            return Err(MultimediaClockError::DomainMismatch);
        }
        let elapsed =
            i128::from(now.timestamp.as_nanos()) - i128::from(self.after.timestamp.as_nanos());
        if elapsed < 0 {
            return Err(MultimediaClockError::Malformed);
        }
        Ok(elapsed >= i128::from(self.max_age_ns.div_ceil(2)))
    }

    /// Renews with another actual timer receipt while preserving every supplied
    /// age/error bound. The new bracket must follow the original bracket.
    /// This does not establish a new accuracy claim or alter native timestamps.
    pub fn refreshed(
        &self,
        raw_ms: u32,
        before: ClockPoint,
        after: ClockPoint,
    ) -> Result<Self, MultimediaClockError> {
        if before.domain != self.after.domain || after.domain != self.after.domain {
            return Err(MultimediaClockError::DomainMismatch);
        }
        if before.timestamp < self.after.timestamp {
            return Err(MultimediaClockError::Malformed);
        }
        Self::new(
            raw_ms,
            before,
            after,
            self.max_age_ns,
            self.measurement_error_ns,
            self.drift_error_ns,
        )
    }

    /// Maps canonical wrapped native nanoseconds to a bounded host interval.
    ///
    /// The caller supplies a fresh actual host observation, at or after the
    /// anchor's after point and within the age bound. The nearest modular
    /// displacement must also lie within that horizon. A candidate wholly
    /// later than the observation, after error expansion, rejects. No source
    /// timestamp is clamped or replaced by the receipt time.
    pub fn map_wrapped_ns(
        &self,
        raw_ns: u64,
        observed_host: ClockPoint,
    ) -> Result<MultimediaHostInterval, MultimediaClockError> {
        if observed_host.domain != self.after.domain {
            return Err(MultimediaClockError::DomainMismatch);
        }
        if i128::from(raw_ns) >= WRAP_NS {
            return Err(MultimediaClockError::Malformed);
        }
        let age = i128::from(observed_host.timestamp.as_nanos())
            - i128::from(self.after.timestamp.as_nanos());
        if age < 0 || age > i128::from(self.max_age_ns) {
            return Err(MultimediaClockError::Expired);
        }
        let modular =
            (i128::from(raw_ns) - i128::from(self.raw_ms) * NS_PER_MS).rem_euclid(WRAP_NS);
        if modular == HALF_WRAP_NS {
            return Err(MultimediaClockError::Ambiguous);
        }
        let delta = if modular > HALF_WRAP_NS {
            modular - WRAP_NS
        } else {
            modular
        };
        if delta.abs() > i128::from(self.max_age_ns) {
            return Err(MultimediaClockError::Expired);
        }
        let errors = i128::from(self.measurement_error_ns) + i128::from(self.drift_error_ns);
        // Each operand originates in i64/u64 or the bounded timer period, so
        // these wide sums cannot overflow i128. Narrowing remains checked.
        let earliest = i128::from(self.before.timestamp.as_nanos()) + delta - errors;
        let latest = i128::from(self.after.timestamp.as_nanos()) + delta + errors;
        if earliest > i128::from(observed_host.timestamp.as_nanos()) {
            return Err(MultimediaClockError::Malformed);
        }
        let before = i64::try_from(earliest).map_err(|_| MultimediaClockError::Overflow)?;
        let after = i64::try_from(latest).map_err(|_| MultimediaClockError::Overflow)?;
        Ok(MultimediaHostInterval {
            before: ClockPoint {
                domain: self.after.domain,
                timestamp: Timestamp::from_nanos(before),
            },
            after: ClockPoint {
                domain: self.after.domain,
                timestamp: Timestamp::from_nanos(after),
            },
        })
    }
}
