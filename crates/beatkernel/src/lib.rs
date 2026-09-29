//! The OS-independent foundation of the BeatKernel rhythm game runtime.
//!
//! Integer nanosecond time, historical host-to-song transport mapping, and typed
//! canonical physical input form the core. Callers supply explicit clock domains;
//! native clock and input acquisition belong in `beatkernel-platform`.
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

pub mod input;
pub mod time;
pub mod transport;
