//! Platform boundaries for BeatKernel native input, audio, and clock backends.
//!
//! Pure keyboard normalizers compile on every host for deterministic fixtures.
//! Target-gated native modules remain stubs with no native API or I/O. Future
//! backends depend on the OS-independent `beatkernel` types; the core never
//! depends on this crate. Only the target OS native module is compiled.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod keyboard;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;
