//! Exact original mixer-grid basis for fresh stream-relative native counters.
use crate::time::{ClockPoint, Timestamp};

/// Invalid rate/frequency or an unrepresentable frame/timestamp conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFrameBasisError {
    /// Mixer sample rate must be positive.
    InvalidRate,
    /// Native counter frequency must be positive.
    InvalidFrequency,
    /// Checked frame addition or timestamp narrowing failed.
    Overflow,
}
impl std::fmt::Display for OutputFrameBasisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "output frame basis: {self:?}")
    }
}
impl std::error::Error for OutputFrameBasisError {}
/// Original absolute mixer frame grid and the physical next frame at stream creation.
/// Playback frames can stop during pause and must not replace this physical offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFrameBasis {
    origin: ClockPoint,
    sample_rate: u32,
    start_physical_frame: u64,
}
impl OutputFrameBasis {
    /// Validates the positive rate without reading clocks or allocating storage.
    pub fn new(
        origin: ClockPoint,
        sample_rate: u32,
        start_physical_frame: u64,
    ) -> Result<Self, OutputFrameBasisError> {
        if sample_rate == 0 {
            return Err(OutputFrameBasisError::InvalidRate);
        }
        Ok(Self {
            origin,
            sample_rate,
            start_physical_frame,
        })
    }
    /// Original absolute frame-zero domain/timestamp.
    pub const fn origin(&self) -> ClockPoint {
        self.origin
    }
    /// Actual mixer sample rate.
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    /// Exact physical next-frame offset when the new stream was created.
    pub const fn start_physical_frame(&self) -> u64 {
        self.start_physical_frame
    }
    /// Adds stream-relative frames before applying one floor on the original grid.
    pub fn point_at_stream_frame(&self, frame: u64) -> Result<ClockPoint, OutputFrameBasisError> {
        let physical = self
            .start_physical_frame
            .checked_add(frame)
            .ok_or(OutputFrameBasisError::Overflow)?;
        self.point(u128::from(physical) * 1_000_000_000 / u128::from(self.sample_rate))
    }
    /// Adds frame/rate and native-counter/frequency rationals before final floor.
    /// Quotient/remainder carry avoids both double rounding and a huge numerator.
    pub fn point_at_native_counter(
        &self,
        ticks: u64,
        frequency: u64,
    ) -> Result<ClockPoint, OutputFrameBasisError> {
        if frequency == 0 {
            return Err(OutputFrameBasisError::InvalidFrequency);
        }
        let rate = u128::from(self.sample_rate);
        let frequency = u128::from(frequency);
        let frames = u128::from(self.start_physical_frame) * 1_000_000_000;
        let ticks = u128::from(ticks) * 1_000_000_000;
        let carry = u128::from(
            (frames % rate) * frequency + (ticks % frequency) * rate >= rate * frequency,
        );
        self.point(frames / rate + ticks / frequency + carry)
    }
    fn point(&self, offset: u128) -> Result<ClockPoint, OutputFrameBasisError> {
        let nanos = i128::try_from(offset)
            .map_err(|_| OutputFrameBasisError::Overflow)?
            .checked_add(i128::from(self.origin.timestamp.as_nanos()))
            .ok_or(OutputFrameBasisError::Overflow)?;
        Ok(ClockPoint {
            domain: self.origin.domain,
            timestamp: Timestamp::from_nanos(
                i64::try_from(nanos).map_err(|_| OutputFrameBasisError::Overflow)?,
            ),
        })
    }
}
