//! Nominal constant-offset intervals from four session-elapsed timestamps.
//! Bounds assume nonnegative transport delays, not bounded drift or hardware time.
use std::fmt;

/// Checked sample or deadline rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockError {
    /// Session elapsed timestamps cannot be negative.
    NegativeTimestamp,
    /// Local or remote timestamps went backwards.
    Chronology,
    /// Remote processing exceeded the local round trip; no delay clamping.
    NegativeRoundTrip,
    /// An observation precedes the latest accepted receive observation.
    ObservationRegression,
    /// The local present precedes the estimate's observation.
    FutureObservation,
    /// The estimate exceeds the caller's age policy.
    StaleEstimate,
    /// A converted endpoint cannot fit the nonnegative i64 timeline.
    Overflow,
    /// The earliest possible local deadline is not strictly in the future.
    DeadlineNotFuture,
}
impl fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for ClockError {}

/// Validated four-timestamp sample; arithmetic remains in i128.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockSample {
    estimate: OffsetEstimate,
}
impl ClockSample {
    /// t0/t3 are local send/receive; t1/t2 are remote receive/send.
    pub fn new(t0: i64, t1: i64, t2: i64, t3: i64) -> Result<Self, ClockError> {
        if [t0, t1, t2, t3].iter().any(|time| *time < 0) {
            return Err(ClockError::NegativeTimestamp);
        }
        if t3 < t0 || t2 < t1 {
            return Err(ClockError::Chronology);
        }
        let lower = i128::from(t2) - i128::from(t3);
        let upper = i128::from(t1) - i128::from(t0);
        let width = upper - lower;
        if width < 0 {
            return Err(ClockError::NegativeRoundTrip);
        }
        let round_trip = u64::try_from(width).map_err(|_| ClockError::Overflow)?;
        Ok(Self {
            estimate: OffsetEstimate {
                lower,
                upper,
                round_trip,
                observed: t3,
            },
        })
    }
}
/// Remote-minus-local offset interval, retaining its corrected round trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffsetEstimate {
    lower: i128,
    upper: i128,
    round_trip: u64,
    observed: i64,
}
impl OffsetEstimate {
    /// Minimum possible remote-minus-local offset.
    pub fn lower_ns(&self) -> i128 {
        self.lower
    }
    /// Maximum possible remote-minus-local offset.
    pub fn upper_ns(&self) -> i128 {
        self.upper
    }
    /// Local round trip minus remote processing, equal to interval width.
    pub fn round_trip_ns(&self) -> u64 {
        self.round_trip
    }
    /// Local receive timestamp at which the sample was observed.
    pub fn observed_local_ns(&self) -> i64 {
        self.observed
    }
    /// Offset midpoint; integer division truncates toward zero.
    pub fn midpoint_ns(&self) -> i128 {
        (self.lower + self.upper) / 2
    }
    /// Converts a remote deadline while retaining uncertainty and checking age.
    pub fn remote_deadline_to_local(
        &self,
        remote_deadline: i64,
        local_now: i64,
        max_age_ns: u64,
    ) -> Result<DeadlineWindow, ClockError> {
        if remote_deadline < 0 || local_now < 0 {
            return Err(ClockError::NegativeTimestamp);
        }
        if local_now < self.observed {
            return Err(ClockError::FutureObservation);
        }
        if i128::from(local_now) - i128::from(self.observed) > i128::from(max_age_ns) {
            return Err(ClockError::StaleEstimate);
        }
        let earliest = i64::try_from(i128::from(remote_deadline) - self.upper)
            .map_err(|_| ClockError::Overflow)?;
        let latest = i64::try_from(i128::from(remote_deadline) - self.lower)
            .map_err(|_| ClockError::Overflow)?;
        if earliest <= local_now {
            return Err(ClockError::DeadlineNotFuture);
        }
        Ok(DeadlineWindow { earliest, latest })
    }
}
/// Scalar minimum-corrected-round-trip filter; ties select the newest sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockFilter {
    best: Option<OffsetEstimate>,
    last_observed: Option<i64>,
}
impl ClockFilter {
    /// Starts without observations or an estimate.
    pub fn new() -> Self {
        Self::default()
    }
    /// Accepts ordered samples; returns true when the selected estimate changes.
    pub fn observe(&mut self, sample: ClockSample) -> Result<bool, ClockError> {
        let next = sample.estimate;
        if self.last_observed.is_some_and(|last| next.observed < last) {
            return Err(ClockError::ObservationRegression);
        }
        let selected = self
            .best
            .map_or(true, |best| next.round_trip <= best.round_trip);
        let changed = selected && self.best != Some(next);
        self.last_observed = Some(next.observed);
        if selected {
            self.best = Some(next);
        }
        Ok(changed)
    }
    /// Best accepted offset estimate, independent of subsequent poorer samples.
    pub fn estimate(&self) -> Option<OffsetEstimate> {
        self.best
    }
}
/// Inclusive local deadline interval, with both endpoints strictly after now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeadlineWindow {
    earliest: i64,
    latest: i64,
}
impl DeadlineWindow {
    /// Earliest possible local deadline.
    pub fn earliest_ns(&self) -> i64 {
        self.earliest
    }
    /// Latest possible local deadline.
    pub fn latest_ns(&self) -> i64 {
        self.latest
    }
    /// Midpoint computed without adding i64 endpoints.
    pub fn midpoint_ns(&self) -> i64 {
        ((i128::from(self.earliest) + i128::from(self.latest)) / 2) as i64
    }
    /// Width of the retained uncertainty interval.
    pub fn uncertainty_ns(&self) -> u64 {
        (i128::from(self.latest) - i128::from(self.earliest)) as u64
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn estimate(times: [i64; 4]) -> OffsetEstimate {
        let mut filter = ClockFilter::new();
        filter
            .observe(ClockSample::new(times[0], times[1], times[2], times[3]).unwrap())
            .unwrap();
        filter.estimate().unwrap()
    }
    #[test]
    fn literal_offset_sign_asymmetry_and_processing() {
        let positive = estimate([100, 160, 180, 140]);
        assert_eq!(
            (
                positive.lower_ns(),
                positive.upper_ns(),
                positive.round_trip_ns(),
                positive.midpoint_ns()
            ),
            (40, 60, 20, 50)
        );
        assert_eq!(positive.observed_local_ns(), 140);
        let negative = estimate([100, 60, 80, 140]);
        assert_eq!(
            (
                negative.lower_ns(),
                negative.upper_ns(),
                negative.midpoint_ns()
            ),
            (-60, -40, -50)
        );
        let asymmetric = estimate([0, 101, 111, 31]);
        assert_eq!(
            (
                asymmetric.lower_ns(),
                asymmetric.upper_ns(),
                asymmetric.round_trip_ns()
            ),
            (80, 101, 21)
        );
        assert_eq!(asymmetric.midpoint_ns(), 90);
        assert_eq!(estimate([10, 10, 20, 20]).round_trip_ns(), 0);
    }
    #[test]
    fn invalid_samples_and_filter_rollback_are_atomic() {
        assert_eq!(
            ClockSample::new(-1, 0, 0, 0),
            Err(ClockError::NegativeTimestamp)
        );
        assert_eq!(ClockSample::new(10, 0, 0, 9), Err(ClockError::Chronology));
        assert_eq!(ClockSample::new(0, 10, 9, 10), Err(ClockError::Chronology));
        assert_eq!(
            ClockSample::new(0, 0, 11, 10),
            Err(ClockError::NegativeRoundTrip)
        );
        let mut filter = ClockFilter::new();
        assert!(
            filter
                .observe(ClockSample::new(0, 5, 5, 10).unwrap())
                .unwrap()
        );
        assert!(
            !filter
                .observe(ClockSample::new(10, 20, 20, 30).unwrap())
                .unwrap()
        );
        assert_eq!(filter.estimate().unwrap().observed_local_ns(), 10);
        let before = filter;
        assert_eq!(
            filter.observe(ClockSample::new(0, 0, 0, 20).unwrap()),
            Err(ClockError::ObservationRegression)
        );
        assert_eq!(filter, before);
        assert!(
            filter
                .observe(ClockSample::new(30, 35, 35, 40).unwrap())
                .unwrap()
        );
        assert_eq!(filter.estimate().unwrap().observed_local_ns(), 40);
        assert!(
            !filter
                .observe(ClockSample::new(30, 35, 35, 40).unwrap())
                .unwrap()
        );
    }
    #[test]
    fn deadline_interval_age_future_and_exact_boundaries() {
        let estimate = estimate([100, 160, 180, 140]);
        let window = estimate.remote_deadline_to_local(300, 150, 10).unwrap();
        assert_eq!(
            (
                window.earliest_ns(),
                window.latest_ns(),
                window.midpoint_ns(),
                window.uncertainty_ns()
            ),
            (240, 260, 250, 20)
        );
        assert_eq!(
            estimate.remote_deadline_to_local(300, 151, 10),
            Err(ClockError::StaleEstimate)
        );
        assert_eq!(
            estimate.remote_deadline_to_local(300, 139, 10),
            Err(ClockError::FutureObservation)
        );
        assert_eq!(
            estimate.remote_deadline_to_local(200, 140, 0),
            Err(ClockError::DeadlineNotFuture)
        );
        assert_eq!(
            estimate.remote_deadline_to_local(-1, 140, 0),
            Err(ClockError::NegativeTimestamp)
        );
        assert_eq!(
            estimate.remote_deadline_to_local(300, -1, 0),
            Err(ClockError::NegativeTimestamp)
        );
    }
    #[test]
    fn long_spans_extremes_overflow_and_signed_odd_midpoint() {
        for span in [
            20 * 60 * 60 * 1_000_000_000i64,
            7 * 24 * 60 * 60 * 1_000_000_000,
        ] {
            let sample = estimate([span, span + 20, span + 30, span + 20]);
            assert_eq!(sample.midpoint_ns(), 15);
            assert_eq!(
                sample
                    .remote_deadline_to_local(span + 100, span + 20, 0)
                    .unwrap()
                    .earliest_ns(),
                span + 80
            );
        }
        let zero = estimate([i64::MAX, i64::MAX, i64::MAX, i64::MAX]);
        assert_eq!(zero.round_trip_ns(), 0);
        assert_eq!(
            zero.remote_deadline_to_local(i64::MAX, i64::MAX, 0),
            Err(ClockError::DeadlineNotFuture)
        );
        let negative = estimate([10, 0, 0, 11]);
        assert_eq!(negative.midpoint_ns(), -10); // -21/2 truncates toward zero
        assert_eq!(
            negative.remote_deadline_to_local(i64::MAX, 11, 0),
            Err(ClockError::Overflow)
        );
        let full = estimate([0, 0, 0, i64::MAX]);
        assert_eq!(full.round_trip_ns(), i64::MAX as u64);
        assert_eq!(full.lower_ns(), -i128::from(i64::MAX));
    }
}
