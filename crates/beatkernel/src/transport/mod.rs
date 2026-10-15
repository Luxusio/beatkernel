//! Historical piecewise mapping from normalized host time to song time.
//!
//! Rate changes preserve integer song position. Real changes quantize a new
//! anchor to nanoseconds; fractional remainders are discarded. No-op commands
//! preserve the old anchor and therefore preserve its fractional progression.
//! The opt-in preroll constructor retains the full signed preroll phase across
//! exact acknowledged pause/resume controls, without changing legacy anchors.
//! [`PositiveRatePhase`] provides a separate exact positive-rate segment for
//! callers that retain rational phase across actual runtime rate cuts.

mod output_phase;
mod positive_rate_phase;
mod rate;
mod state;

pub use output_phase::{OutputPhaseBasis, OutputPhaseError, OutputPhasePoint};
pub use positive_rate_phase::{PositiveRatePhase, PositiveRatePhaseError};
pub use rate::{Rate, RateError};
pub use state::{
    LiveTransportControlError, LiveTransportSetupError, PrerollTransportSetup, Transport,
    TransportAnchor, TransportError,
};
