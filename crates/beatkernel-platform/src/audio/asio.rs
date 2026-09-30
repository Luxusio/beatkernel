//! SDK-free validation of ASIO driver buffer reports and explicit rate requests.
//!
//! These types describe reported constraints. They never open a driver or prove
//! that native buffer creation succeeds, and never round an exact request.

use std::fmt;

mod pcm;
pub use pcm::{encode_asio_channel, AsioPcmEncoding, AsioPcmError};
mod render;
pub use render::{AsioBlockRenderer, AsioRenderError};
mod clocks;
pub use clocks::{validate_clock_sources, AsioClockSource, AsioClockSourceError};

/// Invalid driver metadata or an unsupported exact configuration request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioConfigurationError {
    /// Reported bounds, preferred size or granularity are inconsistent.
    InvalidDriverReport,
    /// The requested frame count does not satisfy the reported constraints.
    UnsupportedBuffer {
        /// Original frame count, without rounding or replacement.
        requested: u32,
    },
    /// A Hertz request must be finite and positive; external clock is explicit.
    InvalidSampleRate,
}
impl fmt::Display for AsioConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDriverReport => f.write_str("invalid ASIO driver buffer report"),
            Self::UnsupportedBuffer { requested } => {
                write!(
                    f,
                    "ASIO buffer size {requested} violates reported constraints"
                )
            }
            Self::InvalidSampleRate => {
                f.write_str("ASIO Hertz request must be finite and positive")
            }
        }
    }
}
impl std::error::Error for AsioConfigurationError {}

/// Explicit selection of the reported preferred size or an exact frame count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioBufferRequest {
    /// Use the driver's validated preferred size.
    DriverPreferred,
    /// Require this frame count; failure does not select a different count.
    Frames(u32),
}

/// Validated ASIO buffer bounds in sample frames, retaining native granularity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsioBufferConstraints {
    min: u32,
    max: u32,
    preferred: u32,
    granularity: i32,
}
impl AsioBufferConstraints {
    /// Validates a native signed 32-bit ASIO buffer report.
    ///
    /// Minus one permits minimum times successive powers of two. Positive
    /// granularity permits minimum plus integral steps. Zero adds no step
    /// restriction within distinct bounds; fixed bounds require zero.
    /// The preferred size must itself satisfy all constraints.
    pub fn from_raw(
        min: i32,
        max: i32,
        preferred: i32,
        granularity: i32,
    ) -> Result<Self, AsioConfigurationError> {
        if min <= 0
            || max < min
            || preferred < min
            || preferred > max
            || granularity < -1
            || (min == max && granularity != 0)
        {
            return Err(AsioConfigurationError::InvalidDriverReport);
        }
        let report = Self {
            min: min as u32,
            max: max as u32,
            preferred: preferred as u32,
            granularity,
        };
        if !report.supports(report.preferred) {
            return Err(AsioConfigurationError::InvalidDriverReport);
        }
        Ok(report)
    }

    /// Smallest reported frame count.
    pub const fn min_frames(self) -> u32 {
        self.min
    }
    /// Largest reported frame count.
    pub const fn max_frames(self) -> u32 {
        self.max
    }
    /// Validated preferred frame count.
    pub const fn preferred_frames(self) -> u32 {
        self.preferred
    }
    /// Original native granularity, including minus one and zero.
    pub const fn granularity(self) -> i32 {
        self.granularity
    }
    /// Whether a count satisfies this report, without asserting native support.
    pub fn supports(self, frames: u32) -> bool {
        if frames < self.min || frames > self.max {
            return false;
        }
        match self.granularity {
            -1 => frames % self.min == 0 && (frames / self.min).is_power_of_two(),
            0 => true,
            step => (frames - self.min) % step as u32 == 0,
        }
    }
    /// Resolves explicit preference or validates exact frames without fallback.
    pub fn resolve(self, request: AsioBufferRequest) -> Result<u32, AsioConfigurationError> {
        let frames = match request {
            AsioBufferRequest::DriverPreferred => self.preferred,
            AsioBufferRequest::Frames(frames) => frames,
        };
        if self.supports(frames) {
            Ok(frames)
        } else {
            Err(AsioConfigurationError::UnsupportedBuffer { requested: frames })
        }
    }
}

/// An explicit hardware rate change, keeping external synchronization distinct.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AsioSampleRateRequest {
    /// A positive finite hardware sample rate; driver support is probed separately.
    Hertz(f64),
    /// Select external synchronization through the native ASIO zero-rate request.
    ExternalClock,
}
impl AsioSampleRateRequest {
    /// Rejects invalid Hertz without contacting a driver.
    pub fn validate(self) -> Result<(), AsioConfigurationError> {
        match self {
            Self::Hertz(rate) if !rate.is_finite() || rate <= 0.0 => {
                Err(AsioConfigurationError::InvalidSampleRate)
            }
            _ => Ok(()),
        }
    }
    /// Returns a validated native rate, with zero reserved for external clock.
    pub fn native_value(self) -> Result<f64, AsioConfigurationError> {
        self.validate()?;
        Ok(match self {
            Self::Hertz(rate) => rate,
            Self::ExternalClock => 0.0,
        })
    }
}
