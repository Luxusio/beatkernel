//! Exact physical output duration across target-rate epochs.
use super::AudioError;
use crate::time::{ClockPoint, Timestamp};

/// Nonnegative whole seconds and a reduced proper fractional second.
/// No segment is rounded until conversion to the final timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetTime {
    seconds: u64,
    numerator: u64,
    denominator: u64,
}
impl TargetTime {
    /// Validates a proper fraction and reduces it without allocation.
    pub fn new(seconds: u64, numerator: u64, denominator: u64) -> Result<Self, AudioError> {
        if denominator == 0 || numerator >= denominator {
            return Err(AudioError::InvalidFormat);
        }
        let divisor = gcd(numerator, denominator);
        Ok(Self {
            seconds,
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }
    /// Exact duration of a fixed-rate frame prefix.
    pub fn from_frames(frames: u64, rate: u32) -> Result<Self, AudioError> {
        if rate == 0 {
            return Err(AudioError::InvalidFormat);
        }
        Self::new(
            frames / u64::from(rate),
            frames % u64::from(rate),
            u64::from(rate),
        )
    }
    /// Whole seconds before the fractional second.
    pub const fn seconds(self) -> u64 {
        self.seconds
    }
    /// Reduced proper numerator.
    pub const fn numerator(self) -> u64 {
        self.numerator
    }
    /// Positive reduced denominator.
    pub const fn denominator(self) -> u64 {
        self.denominator
    }
    /// Adds target frames exactly; denominator/cursor overflow refuses.
    pub fn checked_add_frames(self, frames: u64, rate: u32) -> Result<Self, AudioError> {
        self.checked_add_units(frames, u64::from(rate))
    }
    fn checked_add_units(self, frames: u64, rate: u64) -> Result<Self, AudioError> {
        if rate == 0 {
            return Err(AudioError::InvalidFormat);
        }
        if frames == 0 {
            return Ok(self);
        }
        let denominator = self.units_denominator(rate)?;
        let fraction = u128::from(self.numerator) * u128::from(denominator / self.denominator)
            + u128::from(frames % rate) * u128::from(denominator / rate);
        let seconds = self
            .seconds
            .checked_add(frames / rate)
            .and_then(|seconds| seconds.checked_add((fraction / u128::from(denominator)) as u64))
            .ok_or(AudioError::Overflow)?;
        Self::new(
            seconds,
            (fraction % u128::from(denominator)) as u64,
            denominator,
        )
    }
    pub(crate) fn rate_denominator(self, rate: u32) -> Result<u64, AudioError> {
        self.units_denominator(u64::from(rate))
    }
    fn units_denominator(self, rate: u64) -> Result<u64, AudioError> {
        if rate == 0 {
            return Err(AudioError::InvalidFormat);
        }
        (self.denominator / gcd(self.denominator, rate))
            .checked_mul(rate)
            .ok_or(AudioError::Overflow)
    }
    /// Applies one final nanosecond floor on the original clock origin.
    pub fn point(self, origin: ClockPoint) -> Result<ClockPoint, AudioError> {
        let nanos = i128::from(self.seconds) * 1_000_000_000
            + (u128::from(self.numerator) * 1_000_000_000 / u128::from(self.denominator)) as i128;
        let timestamp = i128::from(origin.timestamp.as_nanos())
            .checked_add(nanos)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(AudioError::Overflow)?;
        Ok(ClockPoint {
            domain: origin.domain,
            timestamp: Timestamp::from_nanos(timestamp),
        })
    }
}
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let rest = a % b;
        a = b;
        b = rest;
    }
    a
}

/// Exact physical start plus one new native stream's immutable target rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetFrameBasis {
    origin: ClockPoint,
    start: TargetTime,
    rate: u32,
}
impl TargetFrameBasis {
    /// Cold validation of rate, denominator compatibility and timestamp range.
    pub fn new(origin: ClockPoint, start: TargetTime, rate: u32) -> Result<Self, AudioError> {
        start.rate_denominator(rate)?;
        start.point(origin)?;
        Ok(Self {
            origin,
            start,
            rate,
        })
    }
    /// Original clock domain and frame-zero origin.
    pub const fn origin(self) -> ClockPoint {
        self.origin
    }
    /// Exact physical duration before this stream's first target frame.
    pub const fn start_time(self) -> TargetTime {
        self.start
    }
    /// Immutable native target frames per second.
    pub const fn sample_rate(self) -> u32 {
        self.rate
    }
    /// Exact physical duration at a stream-relative frame boundary.
    pub fn time_at_stream_frame(self, frames: u64) -> Result<TargetTime, AudioError> {
        self.start.checked_add_frames(frames, self.rate)
    }
    /// Native tick duration is combined exactly before the final timestamp floor.
    pub fn point_at_native_counter(
        self,
        ticks: u64,
        frequency: u64,
    ) -> Result<ClockPoint, AudioError> {
        self.start
            .checked_add_units(ticks, frequency)?
            .point(self.origin)
    }
    /// Stream-relative frames are added before the single timestamp floor.
    pub fn point_at_stream_frame(self, frames: u64) -> Result<ClockPoint, AudioError> {
        self.time_at_stream_frame(frames)?.point(self.origin)
    }
}
