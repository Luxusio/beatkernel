//! Deterministic source charts and immutable absolute song timelines.
//!
//! Beats use integer ticks; BPM and visual speed use exact rational values.
//! Format parsing and game-specific interpretation belong to callers.

mod compiler;
mod model;

pub use compiler::{compile, MAX_SOURCE_ITEMS};
pub use model::*;
