//! Windows native clock and input/audio backend boundary.
//!
//! QPC samples are receipt time with an explicit origin. Raw Input acquisition
//! uses app-owned registration and a message pump; audio remains a later phase.

pub mod clock;
pub mod input;

/// The OS name for this module; this is not a device/input/audio capability claim.
pub const TARGET_OS: &str = "windows";
