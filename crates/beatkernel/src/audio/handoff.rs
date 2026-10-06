//! Cold transfer of software mixer ownership after confirmed output retirement.
use super::Mixer;

/// Statically injected ownership recovery; this is not a physical output fence.
/// Implementations must refuse before confirmed worker/callback retirement and
/// never implicitly stop, join, reopen or clone the unique mixer.
pub trait StoppedMixerSource {
    /// Backend lifecycle or retirement refusal, with no required error bounds.
    type Error;
    /// Moves the original mixer once after confirmed retirement. Repeated takes
    /// return None; unretired or uncertain owners refuse without consuming state.
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error>;
}
