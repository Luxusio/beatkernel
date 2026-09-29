//! Historical piecewise mapping from normalized host time to song time.
//!
//! Rate changes preserve integer song position. Real changes quantize a new
//! anchor to nanoseconds; fractional remainders are discarded. No-op commands
//! preserve the old anchor and therefore preserve its fractional progression.

mod rate;
mod state;

pub use rate::{Rate, RateError};
pub use state::{Transport, TransportAnchor, TransportError};
