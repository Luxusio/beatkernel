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

/// Original open refusal plus optional uniquely recovered software mixer state.
/// Absence means ownership could not be recovered; no replacement mixer is made.
pub struct MixerOpenFailure<E> {
    error: E,
    mixer: Option<Mixer>,
}
impl<E> MixerOpenFailure<E> {
    /// Retains the original error and available mixer without cloning either.
    pub fn new(error: E, mixer: Option<Mixer>) -> Self {
        Self { error, mixer }
    }
    /// Borrows the original backend error unchanged.
    pub fn error(&self) -> &E {
        &self.error
    }
    /// Borrows available software state without consuming command ownership.
    pub fn mixer(&self) -> Option<&Mixer> {
        self.mixer.as_ref()
    }
    /// Moves both the original error and optional unique mixer to the caller.
    pub fn into_parts(self) -> (E, Option<Mixer>) {
        (self.error, self.mixer)
    }
}
impl<E: std::fmt::Debug> std::fmt::Debug for MixerOpenFailure<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MixerOpenFailure")
            .field("error", &self.error)
            .field("mixer_available", &self.mixer.is_some())
            .finish()
    }
}
impl<E: std::fmt::Display> std::fmt::Display for MixerOpenFailure<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl<E: std::error::Error + 'static> std::error::Error for MixerOpenFailure<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
