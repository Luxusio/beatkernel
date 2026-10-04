//! Immutable per-control sound selection at original song time.

use crate::{
    audio::{AudioCommand, SampleId, VoiceId},
    input::{ButtonState, GameControlId, GameInputEvent, PhysicalInputEvent, TouchPhase},
    judge::{JudgeEvent, JudgeOutcome, JudgeStage},
    time::Timestamp,
};
use std::{collections::BTreeMap, fmt, ops::Range};

/// A keysound selection starting at an exact signed song timestamp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputSoundMarker {
    /// Logical control whose fresh presses may select this sound.
    pub control: GameControlId,
    /// Inclusive start on the original, unoffset song timeline.
    pub at: Timestamp,
    /// Caller-prepared PCM asset identity.
    pub sample: SampleId,
    /// Caller-selected voice; intentional reuse is allowed.
    pub voice: VoiceId,
    /// Finite signed gain, including polarity inversion.
    pub gain: f32,
}

/// Invalid input-sound setup, before the owned index is constructed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSoundError {
    /// Capacity must be positive and at most 100000 markers.
    InvalidCapacity,
    /// The supplied marker count exceeds the caller's capacity.
    TooManyMarkers,
    /// A marker gain is not finite.
    InvalidGain,
    /// One logical control has multiple selections at the same song time.
    DuplicatePosition {
        /// Repeated logical identity.
        control: GameControlId,
        /// Repeated signed song timestamp.
        at: Timestamp,
    },
}

impl fmt::Display for InputSoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "input sound: {self:?}")
    }
}
impl std::error::Error for InputSoundError {}

/// Bounded immutable markers with indexed control and timestamp lookup.
/// Construction may allocate; lookup does not allocate or advance a cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct InputSoundTimeline {
    markers: Vec<InputSoundMarker>,
    controls: BTreeMap<u32, Range<usize>>,
}

impl InputSoundTimeline {
    /// Validates and owns at most `max_markers` selections. Empty timelines are
    /// valid; signed song times and deliberate voice reuse remain unchanged.
    pub fn new(
        mut markers: Vec<InputSoundMarker>,
        max_markers: usize,
    ) -> Result<Self, InputSoundError> {
        if max_markers == 0 || max_markers > 100_000 {
            return Err(InputSoundError::InvalidCapacity);
        }
        if markers.len() > max_markers {
            return Err(InputSoundError::TooManyMarkers);
        }
        if markers.iter().any(|marker| !marker.gain.is_finite()) {
            return Err(InputSoundError::InvalidGain);
        }
        markers.sort_unstable_by_key(|marker| (marker.control.0, marker.at));
        for pair in markers.windows(2) {
            if pair[0].control == pair[1].control && pair[0].at == pair[1].at {
                return Err(InputSoundError::DuplicatePosition {
                    control: pair[1].control,
                    at: pair[1].at,
                });
            }
        }
        let mut controls = BTreeMap::new();
        let mut begin = 0;
        while begin < markers.len() {
            let control = markers[begin].control;
            let mut end = begin + 1;
            while end < markers.len() && markers[end].control == control {
                end += 1;
            }
            controls.insert(control.0, begin..end);
            begin = end;
        }
        Ok(Self { markers, controls })
    }

    /// Selects the most recent marker at or before `song_at`, scheduling at the
    /// independently supplied output timestamp. No prior marker means silence.
    pub fn command_for(
        &self,
        control: GameControlId,
        song_at: Timestamp,
        audio_at: Timestamp,
    ) -> Option<AudioCommand> {
        let range = self.controls.get(&control.0)?;
        let markers = &self.markers[range.clone()];
        let end = markers.partition_point(|marker| marker.at <= song_at);
        let marker = markers.get(end.checked_sub(1)?)?;
        Some(AudioCommand::Play {
            voice: marker.voice,
            sample: marker.sample,
            at: audio_at,
            gain: marker.gain,
        })
    }

    /// Shared live/replay fallback policy for one successfully bound operation.
    /// Freshness comes from the judge before mutation. An actual Instant or
    /// HoldHead hit takes precedence over this otherwise unjudged press sound.
    pub fn command_for_press(
        &self,
        input: &GameInputEvent,
        fresh: bool,
        song_at: Timestamp,
        audio_at: Timestamp,
        results: &[JudgeEvent],
    ) -> Option<AudioCommand> {
        if !fresh {
            return None;
        }
        let down = match &input.physical {
            PhysicalInputEvent::Button(button) => button.state == ButtonState::Down,
            PhysicalInputEvent::Touch(touch) => touch.phase == TouchPhase::Down,
            _ => false,
        };
        if !down
            || results.iter().any(|event| {
                matches!(event.stage, JudgeStage::Instant | JudgeStage::HoldHead)
                    && matches!(event.outcome, JudgeOutcome::Hit { .. })
            })
        {
            return None;
        }
        self.command_for(input.game_control, song_at, audio_at)
    }
}
