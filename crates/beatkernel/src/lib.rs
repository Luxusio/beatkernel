//! The OS-independent foundation of the BeatKernel rhythm game runtime.
//!
//! Integer nanosecond time, historical host-to-song transport mapping, and typed
//! canonical input, device-aware bindings and compiled charts form the core.
//! [`runtime`] connects those inputs to [`judge`] and bounded [`audio`] commands.
//! [`replay`] records the same admitted operations and restores complete logical
//! snapshots; [`visual`] projects independent render state, and [`telemetry`]
//! retains bounded software timings. [`runtime::restart`] prepares frame-selected
//! music and fresh output owners off-thread. Callers supply clock mappings;
//! native acquisition, presentation timestamps and device buffer resets belong
//! in `beatkernel-platform` and the final application.
//!
//! ```
//! use beatkernel::{time::Timestamp, transport::{Rate, Transport}};
//!
//! let mut transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
//! transport.pause(Timestamp::from_nanos(1_000))?;
//! assert_eq!(transport.position_at(Timestamp::from_nanos(2_000))?,
//!            Timestamp::from_nanos(1_000));
//! # Ok::<(), beatkernel::transport::TransportError>(())
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod audio;
pub mod chart;
pub mod input;
pub mod interaction;
pub mod judge;
pub mod replay;
pub mod runtime;
pub mod telemetry;
pub mod time;
pub mod transport;
pub mod visual;
