//! Caller-qualified physical-source intervals, without mixer or native ACK proof.

use super::{ConvertedPhaseError, InterpolatedTargetTime, SourcePosition, TargetTime};

/// Whether physical source progress advances the active playback coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourcePlaybackState {
    /// Physical source progress advances active playback by the same amount.
    Running,
    /// Active playback remains at the interval's adopted playback coordinate.
    Frozen,
}

/// Mathematical playback mapping on a half-open physical source interval.
/// The caller must qualify actual source adoption and retain its history;
/// constructing this value authenticates no pause, resume or presentation ACK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePlaybackSpan {
    physical_start: u64,
    physical_end: Option<u64>,
    playback_start: u64,
    state: SourcePlaybackState,
}

impl SourcePlaybackSpan {
    /// Validates a mathematical interval with `end > start`, when bounded,
    /// and an adopted playback coordinate no greater than physical start.
    pub fn new(
        physical_start: u64,
        physical_end: Option<u64>,
        playback_start: u64,
        state: SourcePlaybackState,
    ) -> Result<Self, ConvertedPhaseError> {
        if physical_end.is_some_and(|end| end <= physical_start) || playback_start > physical_start
        {
            return Err(ConvertedPhaseError::InvalidDescriptor);
        }
        Ok(Self {
            physical_start,
            physical_end,
            playback_start,
            state,
        })
    }

    /// Inclusive physical source frame at which this mapping takes effect.
    pub const fn physical_start(self) -> u64 {
        self.physical_start
    }
    /// Exclusive physical source frame at which the next mapping takes effect.
    pub const fn physical_end(self) -> Option<u64> {
        self.physical_end
    }
    /// Active playback frame adopted at the inclusive physical start.
    pub const fn playback_start(self) -> u64 {
        self.playback_start
    }
    /// Whether this interval advances or freezes active playback.
    pub const fn state(self) -> SourcePlaybackState {
        self.state
    }

    /// Maps an exact physical source duration through this half-open interval,
    /// then floors elapsed active playback relative to its cut once. The last
    /// unbounded descriptor still needs the caller's actual pull/native bounds;
    /// this mathematical mapping authenticates no submission or pause ACK.
    pub fn elapsed_nanos_at_source_time(
        self,
        query: InterpolatedTargetTime,
        source_hz: u32,
        active_cut: u64,
    ) -> Result<i128, ConvertedPhaseError> {
        if query
            .compare_source_time_frame(self.physical_start, source_hz)?
            .is_lt()
        {
            return Err(ConvertedPhaseError::BeforeRetainedRange);
        }
        if let Some(end) = self.physical_end {
            if !query.compare_source_time_frame(end, source_hz)?.is_lt() {
                return Err(ConvertedPhaseError::AfterGeneratedRange);
            }
        }
        match self.state {
            SourcePlaybackState::Running => {
                let cut = i128::from(self.physical_start) + i128::from(active_cut)
                    - i128::from(self.playback_start);
                query.elapsed_nanos_since_source_cut(cut, source_hz)
            }
            SourcePlaybackState::Frozen => query.elapsed_at_source_cut(
                SourcePosition {
                    frame: self.playback_start,
                    numerator: 0,
                    denominator: 1,
                },
                source_hz,
                TargetTime::from_frames(0, source_hz)
                    .map_err(|_| ConvertedPhaseError::InvalidDescriptor)?,
                false,
                i128::from(active_cut),
            ),
        }
    }
}
