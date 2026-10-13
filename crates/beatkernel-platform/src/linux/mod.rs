//! Explicit Linux evdev/hidraw acquisition and ALSA output workers.
//!
//! Native device selection and clock relationships remain caller-owned.

mod alsa;
mod input;
mod presentation;
#[allow(unsafe_code)]
mod sys;

pub use alsa::ConvertedAlsaStream;
pub use alsa::{
    alsa_output_devices, AlsaAppliedConfig, AlsaCadenceError, AlsaDevice, AlsaNativeTimestamp,
    AlsaRenderCadence, AlsaRequest, AlsaSnapshot, AlsaSourcePlaybackSnapshot, AlsaStatus,
    AlsaStream, AlsaTimingSnapshot, AlsaConvertedPlaybackSnapshot, ConvertedPhaseSnapshot,
    ConvertedPlaybackPhaseSnapshot,
};
pub use alsa::{
    ConvertedRetirementPolicy, ConvertedRetirementStatus, ConvertedRetirementTicket,
};
pub use input::{
    evdev_keyboard_devices, EvdevDevice, EvdevItem, EvdevKeyboardDevice, EvdevSnapshot,
    HidrawDevice, LinuxInputCounters,
};
pub use presentation::{
    alsa_presentation_pair, alsa_presentation_pair_with_basis,
    alsa_presentation_pair_with_target_basis,
};
pub use sys::{LinuxError, MonotonicClock};

/// The native target represented by this module.
pub const TARGET_OS: &str = "linux";
