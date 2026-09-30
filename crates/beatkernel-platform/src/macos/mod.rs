//! Native IOHID/CoreAudio acquisition and output with explicit mach clocks.
//! Platform APIs acquire/render only; the application owns binding and judging.
#![allow(unsafe_code)]
pub mod audio;
pub mod clock;
mod ffi;
pub mod input;
/// The OS name, independent of permission and hardware capability.
pub const TARGET_OS: &str = "macos";
