//! Cold transfer of complete software ownership after confirmed output retirement.
use super::{Mixer, OutputFrameBasis};

/// Read-only software facts used by cold output replacement.
/// The next native basis may precede the Mixer pull frontier when PCM awaits admission.
pub trait SoftwareOutputState {
    /// Original scheduling owner; callers cannot extract a destructive subset.
    fn mixer(&self) -> &Mixer;
    /// First frame awaiting native admission on the supported output grid.
    fn output_frame_basis(&self) -> OutputFrameBasis;
    /// Whether every retained frame has actual paused-zero render evidence.
    fn paused_tail_admissible(&self) -> bool;
}
impl SoftwareOutputState for Mixer {
    fn mixer(&self) -> &Mixer {
        self
    }
    fn output_frame_basis(&self) -> OutputFrameBasis {
        Mixer::output_frame_basis(self)
    }
    fn paused_tail_admissible(&self) -> bool {
        true
    }
}

/// Statically injected ownership recovery; this is not a physical output fence.
/// Implementations must refuse before confirmed worker/callback retirement and
/// never implicitly stop, join, reopen or clone the unique mixer.
pub trait StoppedMixerSource<O = Mixer> {
    /// Backend lifecycle or retirement refusal, with no required error bounds.
    type Error;
    /// Moves the original mixer once after confirmed retirement. Repeated takes
    /// return None; unretired or uncertain owners refuse without consuming state.
    fn take_stopped_mixer(&mut self) -> Result<Option<O>, Self::Error>;
}

/// Original open refusal plus optional uniquely recovered software owner.
/// Mixer is the compatible default; absence never fabricates replacement ownership.
pub struct MixerOpenFailure<E, O = Mixer> {
    error: E,
    mixer: Option<O>,
}
impl<E> MixerOpenFailure<E> {
    /// Compatible ordinary-Mixer constructor, including an unavailable owner.
    pub fn new(error: E, mixer: Option<Mixer>) -> Self {
        Self::new_state(error, mixer)
    }
}
impl<E, O> MixerOpenFailure<E, O> {
    /// Retains the original error and available mixer without cloning either.
    pub fn new_state(error: E, mixer: Option<O>) -> Self {
        Self { error, mixer }
    }
    /// Borrows the original backend error unchanged.
    pub fn error(&self) -> &E {
        &self.error
    }
    /// Borrows available software state without consuming command ownership.
    pub fn mixer(&self) -> Option<&O> {
        self.mixer.as_ref()
    }
    /// Moves both the original error and optional unique mixer to the caller.
    pub fn into_parts(self) -> (E, Option<O>) {
        (self.error, self.mixer)
    }
}
impl<E: std::fmt::Debug, O> std::fmt::Debug for MixerOpenFailure<E, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MixerOpenFailure")
            .field("error", &self.error)
            .field("mixer_available", &self.mixer.is_some())
            .finish()
    }
}
impl<E: std::fmt::Display, O> std::fmt::Display for MixerOpenFailure<E, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl<E: std::error::Error + 'static, O> std::error::Error for MixerOpenFailure<E, O> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Original output-open refusal, recovered software state or pending native owner.
/// Constructors keep pending ownership exclusive from recovered mixer storage.
/// The adapter, not this value, must prove callbacks/worker access have retired.
pub struct OutputOpenFailure<E, S, O = Mixer> {
    error: E,
    mixer: Option<O>,
    pending: Option<S>,
    cleanup: Option<E>,
}
impl<E, S> OutputOpenFailure<E, S> {
    /// Compatible ordinary-Mixer recovery constructor.
    pub fn recovered(error: E, mixer: Option<Mixer>) -> Self {
        Self::recovered_state(error, mixer)
    }
    /// Compatible ordinary-Mixer pending-retirement constructor.
    pub fn pending(error: E, owner: S) -> Self {
        Self::pending_state(error, owner)
    }
}
impl<E, S, O> OutputOpenFailure<E, S, O> {
    /// Retains available unique software ownership without a pending native owner.
    pub fn recovered_state(error: E, mixer: Option<O>) -> Self {
        Self {
            error,
            mixer,
            pending: None,
            cleanup: None,
        }
    }
    /// Retains the actual partial owner without claiming recovered mixer ownership.
    pub fn pending_state(error: E, owner: S) -> Self {
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
    pub fn mixer(&self) -> Option<&O> {
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
    pub fn into_parts(self) -> (E, Option<O>, Option<S>, Option<E>) {
        (self.error, self.mixer, self.pending, self.cleanup)
    }
    /// Runs a cold static retirement operation without allocating or cloning.
    /// An Ok result must prove retirement. Refusal retains the owner and original
    /// error, recording the latest cleanup error. No owner is an explicit no-op.
    pub fn retry_retirement(
        &mut self,
        retire: impl FnOnce(&mut S) -> Result<Option<O>, E>,
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
impl<E: std::fmt::Debug, S, O> std::fmt::Debug for OutputOpenFailure<E, S, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputOpenFailure")
            .field("error", &self.error)
            .field("mixer_available", &self.mixer.is_some())
            .field("pending_owner", &self.pending.is_some())
            .field("cleanup_error", &self.cleanup)
            .finish()
    }
}
impl<E: std::fmt::Display, S, O> std::fmt::Display for OutputOpenFailure<E, S, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl<E: std::error::Error + 'static, S, O> std::error::Error for OutputOpenFailure<E, S, O> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
