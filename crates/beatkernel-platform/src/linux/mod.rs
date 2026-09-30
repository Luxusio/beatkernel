//! Explicit Linux evdev/hidraw acquisition and ALSA output workers.
//!
//! Native device selection and clock relationships remain caller-owned.

mod alsa;
mod input;
#[allow(unsafe_code)]
mod sys;

pub use alsa::{
    AlsaAppliedConfig, AlsaNativeTimestamp, AlsaRequest, AlsaSnapshot, AlsaStatus, AlsaStream,
    AlsaTimingSnapshot,
};
pub use input::{EvdevDevice, EvdevItem, EvdevSnapshot, HidrawDevice, LinuxInputCounters};
pub use sys::{LinuxError, MonotonicClock};

/// The native target represented by this module.
pub const TARGET_OS: &str = "linux";
