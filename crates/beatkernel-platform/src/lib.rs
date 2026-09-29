//! Platform boundaries for BeatKernel native input, audio, and clock backends.
//!
//! Pure keyboard normalizers and Raw Input decoding compile on every host for
//! deterministic fixtures. Native input/audio/clock boundaries are target-gated.
//! Backends depend on the OS-independent `beatkernel` types; the core never
//! depends on this crate. Only the target OS native module is compiled.

#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]

pub mod keyboard;
pub mod raw_input;

#[path = "windows/device_registry.rs"]
mod raw_device_registry;
#[path = "windows/hid.rs"]
mod raw_hid;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;
