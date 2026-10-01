//! Explicit Linux evdev/hidraw acquisition and ALSA output workers.
//!
//! Native device selection and clock relationships remain caller-owned.

mod alsa;
mod input;
mod presentation;
#[allow(unsafe_code)]
mod sys;

pub use alsa::{
    AlsaAppliedConfig, AlsaCadenceError, AlsaDevice, AlsaNativeTimestamp, AlsaRenderCadence,
    AlsaRequest, AlsaSnapshot, AlsaStatus, AlsaStream, AlsaTimingSnapshot, alsa_output_devices,
};
pub use input::{
    EvdevDevice, EvdevItem, EvdevKeyboardDevice, EvdevSnapshot, HidrawDevice, LinuxInputCounters,
    evdev_keyboard_devices,
};
pub use presentation::alsa_presentation_pair;
pub use sys::{LinuxError, MonotonicClock};

/// The native target represented by this module.
pub const TARGET_OS: &str = "linux";
