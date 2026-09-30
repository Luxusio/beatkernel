//! Signed integer nanosecond time with explicit checked arithmetic.
//!
//! A timestamp has meaning only in its clock domain. These types never read
//! an OS clock and do not assume native, host, or audio clocks are equivalent.

mod calibration;
mod clock_domain;
mod duration;
mod timestamp;

pub use calibration::{
    AffineClockMapper, CalibrationError, CalibrationUncertainty, ClockInterval, ClockPair,
    ExtrapolationPolicy,
};
pub use clock_domain::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint};
pub use duration::Duration;
pub use timestamp::Timestamp;
