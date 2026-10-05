//! Immutable sound bindings for actual committed hazard outcomes.

use crate::{
    audio::{AudioCommand, SampleId, VoiceId},
    judge::{HazardEvent, HazardId, HazardOutcome},
    time::Timestamp,
};
use std::fmt;

/// Caller-selected sound for one exact hazard identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HazardSoundBinding {
    /// Exact committed hazard identity that selects this sound.
    pub hazard: HazardId,
    /// Caller-prepared PCM sample identity.
    pub sample: SampleId,
    /// Intentional voice reuse follows the ordinary sound-binding policy.
    pub voice: VoiceId,
    /// Finite signed gain, including polarity inversion.
    pub gain: f32,
}

/// Invalid hazard-sound construction or installation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HazardSoundError {
    /// The binding count exceeds the caller's supplied capacity.
    Capacity,
    /// Multiple bindings name the same hazard identity.
    DuplicateHazard {
        /// Identity repeated in the supplied bindings.
        hazard: HazardId,
    },
    /// A binding gain is NaN or infinite; finite signed gains are valid.
    InvalidGain {
        /// Identity whose binding has a nonfinite gain.
        hazard: HazardId,
    },
    /// A hazard-sound timeline is already installed on this runtime.
    AlreadyConfigured,
    /// Input or advancement was committed before timeline installation.
    AlreadyStarted,
}

impl fmt::Display for HazardSoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hazard sound: {self:?}")
    }
}
impl std::error::Error for HazardSoundError {}

/// Bounded immutable bindings, sorted by full-width hazard identity.
#[derive(Clone, Debug, PartialEq)]
pub struct HazardSoundTimeline {
    bindings: Vec<HazardSoundBinding>,
}

impl HazardSoundTimeline {
    /// Owns at most the caller's capacity. An empty zero-capacity timeline is
    /// valid; asset existence and voice replacement remain caller policy.
    pub fn new(
        mut bindings: Vec<HazardSoundBinding>,
        max_bindings: usize,
    ) -> Result<Self, HazardSoundError> {
        if bindings.len() > max_bindings {
            return Err(HazardSoundError::Capacity);
        }
        if let Some(binding) = bindings.iter().find(|binding| !binding.gain.is_finite()) {
            return Err(HazardSoundError::InvalidGain {
                hazard: binding.hazard,
            });
        }
        bindings.sort_unstable_by_key(|binding| binding.hazard);
        for pair in bindings.windows(2) {
            if pair[0].hazard == pair[1].hazard {
                return Err(HazardSoundError::DuplicateHazard {
                    hazard: pair[1].hazard,
                });
            }
        }
        Ok(Self { bindings })
    }

    /// Borrows validated bindings in ascending hazard-identity order.
    pub fn bindings(&self) -> &[HazardSoundBinding] {
        &self.bindings
    }

    /// Selects only an exact triggered identity, without interpreting its opaque
    /// value or changing marker time/provenance. Lookup allocates no storage.
    pub fn command_for(&self, event: &HazardEvent, audio_at: Timestamp) -> Option<AudioCommand> {
        if event.outcome != HazardOutcome::Triggered {
            return None;
        }
        let index = self
            .bindings
            .binary_search_by_key(&event.id, |binding| binding.hazard)
            .ok()?;
        let binding = self.bindings[index];
        Some(AudioCommand::Play {
            voice: binding.voice,
            sample: binding.sample,
            at: audio_at,
            gain: binding.gain,
        })
    }
}
