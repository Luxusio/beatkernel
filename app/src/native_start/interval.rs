//! Conditional interval projection under an explicitly assessed source/host rate band.
//! Bounds preserve supplied uncertainty; they do not establish physical accuracy.
use super::{HostStartWindow, OutputStartPlan, StartProjectionError};
use beatkernel::time::{ClockPoint, Timestamp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartInterval {
    pub output: ClockPoint,
    pub before: ClockPoint,
    pub after: ClockPoint,
}
impl StartInterval {
    pub fn new(
        output: ClockPoint,
        before: ClockPoint,
        after: ClockPoint,
    ) -> Result<Self, StartProjectionError> {
        let value = Self {
            output,
            before,
            after,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(self) -> Result<(), StartProjectionError> {
        if self.before.domain != self.after.domain || self.output.domain == self.before.domain {
            return Err(StartProjectionError::Domains);
        }
        if self.before.timestamp > self.after.timestamp {
            return Err(StartProjectionError::Chronology);
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Ratio {
    numerator: i128,
    denominator: i128,
}
impl Ratio {
    fn new(numerator: i128, denominator: i128) -> Self {
        debug_assert!(numerator >= 0 && denominator > 0);
        let divisor = gcd(numerator, denominator);
        Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        }
    }
    fn less(self, other: Self) -> Result<bool, StartProjectionError> {
        Ok(mul(self.numerator, other.denominator)? < mul(other.numerator, self.denominator)?)
    }
    fn scaled(self, value: i128, ceil: bool) -> Result<i128, StartProjectionError> {
        debug_assert!(value >= 0);
        let divisor = gcd(value, self.denominator);
        let numerator = mul(value / divisor, self.numerator)?;
        let denominator = self.denominator / divisor;
        let quotient = numerator / denominator;
        add(quotient, i128::from(ceil && numerator % denominator != 0))
    }
    fn inverse(self) -> Self {
        Self {
            numerator: self.denominator,
            denominator: self.numerator,
        }
    }
}
fn gcd(mut left: i128, mut right: i128) -> i128 {
    while right != 0 {
        let next = left % right;
        left = right;
        right = next;
    }
    left
}
fn mul(left: i128, right: i128) -> Result<i128, StartProjectionError> {
    left.checked_mul(right)
        .ok_or(StartProjectionError::Overflow)
}
fn add(left: i128, right: i128) -> Result<i128, StartProjectionError> {
    left.checked_add(right)
        .ok_or(StartProjectionError::Overflow)
}
fn ns(point: ClockPoint) -> i128 {
    i128::from(point.timestamp.as_nanos())
}
fn band(ppm: u32) -> Result<(Ratio, Ratio), StartProjectionError> {
    if ppm >= 1_000_000 {
        return Err(StartProjectionError::Slope);
    }
    Ok((
        Ratio::new(1_000_000 - i128::from(ppm), 1_000_000),
        Ratio::new(1_000_000 + i128::from(ppm), 1_000_000),
    ))
}
fn pair(first: StartInterval, second: StartInterval) -> Result<i128, StartProjectionError> {
    first.validate()?;
    second.validate()?;
    if first.output.domain != second.output.domain || first.before.domain != second.before.domain {
        return Err(StartProjectionError::Domains);
    }
    let span = ns(second.output) - ns(first.output);
    if span <= 0
        || first.before.timestamp > second.before.timestamp
        || first.after.timestamp > second.after.timestamp
    {
        return Err(StartProjectionError::Chronology);
    }
    Ok(span)
}
/// Project the uncertainty of a future committed target, selecting its latest ceiling frame.
/// The ppm band is a caller assessment consistent with observations, not measured drift.
pub fn project(
    window: HostStartWindow,
    first: StartInterval,
    second: StartInterval,
    origin: ClockPoint,
    sample_rate: u32,
    rendered_end: u64,
    minimum_ahead_frames: u64,
    maximum_rate_error_ppm: u32,
) -> Result<OutputStartPlan, StartProjectionError> {
    let span = pair(first, second)?;
    if sample_rate == 0 || sample_rate > 1_000_000_000 {
        return Err(StartProjectionError::InvalidRate);
    }
    if origin.domain != second.output.domain
        || window.earliest.domain != second.before.domain
        || window.latest.domain != second.before.domain
    {
        return Err(StartProjectionError::Domains);
    }
    if window.earliest.timestamp > window.latest.timestamp {
        return Err(StartProjectionError::Chronology);
    }
    if window.earliest.timestamp <= second.after.timestamp {
        return Err(StartProjectionError::TooClose);
    }
    if origin.timestamp > first.output.timestamp {
        return Err(StartProjectionError::Chronology);
    }
    let (assessed_low, assessed_high) = band(maximum_rate_error_ppm)?;
    let (mut low, mut high) = (assessed_low, assessed_high);
    let host_high = ns(second.after) - ns(first.before);
    let host_low = ns(second.before) - ns(first.after);
    if host_high <= 0 {
        return Err(StartProjectionError::Slope);
    }
    // Each output timestamp floors its actual grid coordinate; relative error is <1ns.
    let observed_low = Ratio::new(span - 1, host_high);
    if low.less(observed_low)? {
        low = observed_low;
    }
    if host_low > 0 {
        let observed_high = Ratio::new(add(span, 1)?, host_low);
        if observed_high.less(high)? {
            high = observed_high;
        }
    }
    if high.less(low)? {
        return Err(StartProjectionError::Slope);
    }
    // The observed average only checks consistency: future rate may vary over the full assessed band.
    let (low, high) = (assessed_low, assessed_high);
    let base = ns(second.output) - ns(origin);
    let lower = add(
        add(base, -1)?,
        low.scaled(ns(window.earliest) - ns(second.after), false)?,
    )?;
    let upper = add(
        add(base, 1)?,
        high.scaled(ns(window.latest) - ns(second.before), true)?,
    )?;
    if lower < 0 || upper < lower {
        return Err(StartProjectionError::TooClose);
    }
    let frame = |value: i128| -> Result<u64, StartProjectionError> {
        let scaled = mul(value, i128::from(sample_rate))?;
        let frame = add(
            scaled / 1_000_000_000,
            i128::from(scaled % 1_000_000_000 != 0),
        )?;
        u64::try_from(frame).map_err(|_| StartProjectionError::Overflow)
    };
    let earliest_frame = frame(lower)?;
    let latest_frame = frame(upper)?;
    let minimum = rendered_end
        .checked_add(minimum_ahead_frames)
        .ok_or(StartProjectionError::Overflow)?;
    if earliest_frame < minimum {
        return Err(StartProjectionError::TooClose);
    }
    let selected_ns = add(
        ns(origin),
        mul(i128::from(latest_frame), 1_000_000_000)? / i128::from(sample_rate),
    )?;
    Ok(OutputStartPlan {
        earliest_frame,
        latest_frame,
        selected_output: ClockPoint {
            domain: origin.domain,
            timestamp: Timestamp::from_nanos(
                i64::try_from(selected_ns).map_err(|_| StartProjectionError::Overflow)?,
            ),
        },
    })
}
/// Intersect two genuine host interval anchors under the assessed reciprocal rate band.
/// Equal host endpoints are permitted when their intervals remain feasible.
pub fn crossing(
    output: ClockPoint,
    lower: StartInterval,
    upper: StartInterval,
    maximum_rate_error_ppm: u32,
) -> Result<HostStartWindow, StartProjectionError> {
    let span = pair(lower, upper)?;
    if output.domain != lower.output.domain {
        return Err(StartProjectionError::Domains);
    }
    let delta = ns(output) - ns(lower.output);
    if delta < 0 || delta > span {
        return Err(StartProjectionError::Chronology);
    }
    let (slow, fast) = band(maximum_rate_error_ppm)?;
    let bounds = |distance: i128| -> Result<(i128, i128), StartProjectionError> {
        Ok((
            fast.inverse().scaled((distance - 1).max(0), false)?,
            slow.inverse().scaled(add(distance, 1)?, true)?,
        ))
    };
    let (forward_low, forward_high) = bounds(delta)?;
    let (backward_low, backward_high) = bounds(span - delta)?;
    let mut earliest =
        add(ns(lower.before), forward_low)?.max(add(ns(upper.before), -backward_high)?);
    let mut latest = add(ns(lower.after), forward_high)?.min(add(ns(upper.after), -backward_low)?);
    // At an actual anchor its own original host interval also applies directly.
    if delta == 0 {
        earliest = earliest.max(ns(lower.before));
        latest = latest.min(ns(lower.after));
    }
    if delta == span {
        earliest = earliest.max(ns(upper.before));
        latest = latest.min(ns(upper.after));
    }
    if earliest > latest {
        return Err(StartProjectionError::Slope);
    }
    let point = |value| -> Result<ClockPoint, StartProjectionError> {
        Ok(ClockPoint {
            domain: lower.before.domain,
            timestamp: Timestamp::from_nanos(
                i64::try_from(value).map_err(|_| StartProjectionError::Overflow)?,
            ),
        })
    };
    Ok(HostStartWindow {
        earliest: point(earliest)?,
        latest: point(latest)?,
    })
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::time::ClockDomainId;
    fn point(domain: u32, value: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(value),
        }
    }
    fn interval(output: i64, before: i64, after: i64) -> StartInterval {
        StartInterval::new(point(2, output), point(1, before), point(1, after)).unwrap()
    }
    fn window(earliest: i64, latest: i64) -> HostStartWindow {
        HostStartWindow {
            earliest: point(1, earliest),
            latest: point(1, latest),
        }
    }
    #[test]
    fn coarse_timer_range_intersects_assessed_band_and_keeps_target_uncertainty() {
        let first = interval(0, 0, 2_000_000);
        let second = interval(100_000_000, 100_000_000, 102_000_000);
        let plan = project(
            window(200_000_000, 200_000_000),
            first,
            second,
            point(2, 0),
            1000,
            190,
            8,
            1000,
        )
        .unwrap();
        assert_eq!((plan.earliest_frame(), plan.latest_frame()), (198, 201));
        assert_eq!(plan.selected_output(), point(2, 201_000_000));
        // A degenerate committed target remains usable; its uncertainty is not a tolerance.
        let wider = project(
            window(200_000_000, 202_000_000),
            first,
            second,
            point(2, 0),
            1000,
            0,
            0,
            1000,
        )
        .unwrap();
        assert!(wider.latest_frame() > plan.latest_frame());
        assert_eq!(
            project(
                window(200_000_000, 200_000_000),
                first,
                second,
                point(2, 0),
                1000,
                191,
                8,
                1000
            ),
            Err(StartProjectionError::TooClose)
        );
    }
    #[test]
    fn exact_average_does_not_remove_assessed_future_rate_uncertainty() {
        let plan = project(
            window(200_000_000, 200_000_000),
            interval(0, 0, 0),
            interval(100_000_000, 100_000_000, 100_000_000),
            point(2, 0),
            1_000_000_000,
            0,
            0,
            1000,
        )
        .unwrap();
        assert_eq!(
            (plan.earliest_frame(), plan.latest_frame()),
            (199_899_999, 200_100_001)
        );
        assert_eq!(
            project(
                window(200_000_000, 200_000_000),
                interval(0, 0, 0),
                interval(100_000_000, 100_000_000, 100_000_000),
                point(2, 1),
                1_000_000_000,
                0,
                0,
                1000
            ),
            Err(StartProjectionError::Chronology)
        );
    }
    #[test]
    fn nonzero_origin_fractional_frame_and_maximum_rate_round_outward() {
        let first = interval(10_000_000, 0, 2_000_000);
        let second = interval(110_000_000, 100_000_000, 102_000_000);
        let plan = project(
            window(200_000_000, 200_000_000),
            first,
            second,
            point(2, 10_000_000),
            48000,
            0,
            0,
            0,
        )
        .unwrap();
        assert_eq!((plan.earliest_frame(), plan.latest_frame()), (9504, 9601));
        assert_eq!(plan.selected_output(), point(2, 210_020_833));
        let nanoseconds = project(
            window(200_000_000, 200_000_000),
            first,
            second,
            point(2, 10_000_000),
            1_000_000_000,
            0,
            0,
            0,
        )
        .unwrap();
        assert_eq!(
            (nanoseconds.earliest_frame(), nanoseconds.latest_frame()),
            (197_999_999, 200_000_001)
        );
    }
    #[test]
    fn invalid_domains_shapes_regressions_and_inconsistent_slope_reject() {
        assert_eq!(
            StartInterval::new(point(1, 0), point(1, 0), point(1, 1)),
            Err(StartProjectionError::Domains)
        );
        assert_eq!(
            StartInterval::new(point(2, 0), point(1, 1), point(1, 0)),
            Err(StartProjectionError::Chronology)
        );
        let first = interval(0, 0, 0);
        let second = interval(100, 100, 100);
        for rate in [0, 1_000_000_001] {
            assert_eq!(
                project(window(200, 200), first, second, point(2, 0), rate, 0, 0, 0),
                Err(StartProjectionError::InvalidRate)
            );
        }
        assert_eq!(
            project(window(100, 200), first, second, point(2, 0), 1000, 0, 0, 0),
            Err(StartProjectionError::TooClose)
        );
        assert_eq!(
            project(window(200, 200), first, second, point(3, 0), 1000, 0, 0, 0),
            Err(StartProjectionError::Domains)
        );
        assert_eq!(
            project(window(200, 200), second, first, point(2, 0), 1000, 0, 0, 0),
            Err(StartProjectionError::Chronology)
        );
        assert_eq!(
            project(
                window(300, 300),
                first,
                interval(200, 100, 100),
                point(2, 0),
                1000,
                0,
                0,
                1000
            ),
            Err(StartProjectionError::Slope)
        );
        assert_eq!(
            project(
                window(200, 200),
                first,
                second,
                point(2, 0),
                1000,
                0,
                0,
                1_000_000
            ),
            Err(StartProjectionError::Slope)
        );
    }
    #[test]
    fn crossing_retains_plateaus_signed_coordinates_and_exact_anchor_bounds() {
        let lower = interval(0, 0, 100);
        let upper = interval(50, 50, 100);
        let crossed = crossing(point(2, 25), lower, upper, 0).unwrap();
        assert_eq!(crossed.earliest(), point(1, 24));
        assert_eq!(crossed.latest(), point(1, 76));
        let signed = crossing(
            point(2, 50),
            interval(0, -102, -98),
            interval(100, -2, 2),
            0,
        )
        .unwrap();
        assert_eq!(signed.earliest(), point(1, -53));
        assert_eq!(signed.latest(), point(1, -47));
        for (output, before, after) in [(0, 0, 0), (100, 100, 100)] {
            let edge = crossing(
                point(2, output),
                interval(0, 0, 0),
                interval(100, 100, 100),
                0,
            )
            .unwrap();
            assert_eq!(edge.earliest(), point(1, before));
            assert_eq!(edge.latest(), point(1, after));
        }
    }
    #[test]
    fn crossing_band_is_explicit_and_impossible_intersections_reject() {
        let lower = interval(0, 0, 0);
        let upper = interval(1000, 500, 500);
        assert_eq!(
            crossing(point(2, 500), lower, upper, 1000),
            Err(StartProjectionError::Slope)
        );
        assert_eq!(
            crossing(point(2, 500), lower, upper, 1_000_000),
            Err(StartProjectionError::Slope)
        );
        assert_eq!(
            crossing(point(2, 1001), lower, upper, 0),
            Err(StartProjectionError::Chronology)
        );
        assert_eq!(
            crossing(point(3, 500), lower, upper, 0),
            Err(StartProjectionError::Domains)
        );
        assert_eq!(
            crossing(point(2, 500), upper, lower, 0),
            Err(StartProjectionError::Chronology)
        );
    }
    #[test]
    fn long_spans_remain_integer_and_overflow_never_clamps() {
        let week = 604_800_000_000_000;
        let crossed = crossing(
            point(2, week / 2),
            interval(0, 0, 0),
            interval(week, week, week),
            0,
        )
        .unwrap();
        assert_eq!(crossed.earliest().timestamp.as_nanos(), week / 2 - 1);
        assert_eq!(crossed.latest().timestamp.as_nanos(), week / 2 + 1);
        assert_eq!(
            project(
                window(300, 300),
                interval(0, 0, 0),
                interval(100, 100, 100),
                point(2, 0),
                1000,
                u64::MAX,
                1,
                0
            ),
            Err(StartProjectionError::Overflow)
        );
        // The selected physical grid timestamp cannot extend beyond i64, even if frame IDs fit.
        assert_eq!(
            project(
                window(i64::MAX, i64::MAX),
                interval(0, 0, 0),
                interval(100, 100, 100),
                point(2, 0),
                1000,
                0,
                0,
                0
            ),
            Err(StartProjectionError::Overflow)
        );
        let extreme = interval(i64::MIN, i64::MIN, i64::MIN);
        let later = interval(i64::MAX - 2, i64::MAX - 2, i64::MAX - 2);
        assert!(
            project(
                window(i64::MAX, i64::MAX),
                extreme,
                later,
                point(2, i64::MIN),
                1_000_000_000,
                0,
                0,
                999_999
            )
            .is_err()
        );
    }
}
