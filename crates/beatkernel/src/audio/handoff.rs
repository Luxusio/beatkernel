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

/// Original output-open refusal, recovered mixer or an owner pending retirement.
/// Constructors keep pending ownership exclusive from recovered mixer storage.
/// The adapter, not this value, must prove callbacks/worker access have retired.
pub struct OutputOpenFailure<E, S> {
    error: E,
    mixer: Option<Mixer>,
    pending: Option<S>,
    cleanup: Option<E>,
}
impl<E, S> OutputOpenFailure<E, S> {
    /// Retains available unique software ownership without a pending native owner.
    pub fn recovered(error: E, mixer: Option<Mixer>) -> Self {
        Self {
            error,
            mixer,
            pending: None,
            cleanup: None,
        }
    }
    /// Retains the actual partial owner without claiming recovered mixer ownership.
    pub fn pending(error: E, owner: S) -> Self {
        Self {
            error,
            mixer: None,
            pending: Some(owner),
            cleanup: None,
        }
    }
    /// Attaches the latest cleanup diagnostic without replacing the original
    /// opening error or changing recovered/pending ownership.
    pub fn with_cleanup_error(mut self, error: E) -> Self {
        self.cleanup = Some(error);
        self
    }
    /// Original opening error, unchanged by retirement attempts.
    pub fn error(&self) -> &E {
        &self.error
    }
    /// Mixer made available only after proven retirement, if recovery was possible.
    pub fn mixer(&self) -> Option<&Mixer> {
        self.mixer.as_ref()
    }
    /// Actual partial owner still responsible for callback/worker storage.
    pub fn pending_owner(&self) -> Option<&S> {
        self.pending.as_ref()
    }
    /// Most recent cleanup refusal, separate from the original opening error.
    pub fn cleanup_error(&self) -> Option<&E> {
        self.cleanup.as_ref()
    }
    /// Moves original error, mixer, pending owner and cleanup diagnostic unchanged.
    pub fn into_parts(self) -> (E, Option<Mixer>, Option<S>, Option<E>) {
        (self.error, self.mixer, self.pending, self.cleanup)
    }
    /// Runs a cold static retirement operation without allocating or cloning.
    /// An Ok result must prove retirement. Refusal retains the owner and original
    /// error, recording the latest cleanup error. No owner is an explicit no-op.
    pub fn retry_retirement(
        &mut self,
        retire: impl FnOnce(&mut S) -> Result<Option<Mixer>, E>,
    ) -> Result<bool, &E> {
        let Some(owner) = self.pending.as_mut() else {
            return Ok(false);
        };
        match retire(owner) {
            Err(error) => {
                self.cleanup = Some(error);
                Err(self.cleanup.as_ref().expect("cleanup error was retained"))
            }
            Ok(mixer) => {
                self.mixer = mixer;
                self.pending = None;
                self.cleanup = None;
                Ok(true)
            }
        }
    }
}
impl<E: std::fmt::Debug, S> std::fmt::Debug for OutputOpenFailure<E, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputOpenFailure")
            .field("error", &self.error)
            .field("mixer_available", &self.mixer.is_some())
            .field("pending_owner", &self.pending.is_some())
            .field("cleanup_error", &self.cleanup)
            .finish()
    }
}
impl<E: std::fmt::Display, S> std::fmt::Display for OutputOpenFailure<E, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl<E: std::error::Error + 'static, S> std::error::Error for OutputOpenFailure<E, S> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
