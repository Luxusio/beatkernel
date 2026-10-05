//! Per-owner native clock acquisition for comparison progress, never a judge clock.
use crate::{competition_progress_cadence::CompetitionProgressClock, multiplayer::MultiplayerError};
use std::time::Instant;

#[derive(Clone, Copy)]
pub(crate) struct NativeProgressClock {
    origin: Instant,
}
impl NativeProgressClock {
    pub(crate) fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}
impl CompetitionProgressClock for NativeProgressClock {
    type Error = MultiplayerError;
    fn now_ns(&mut self) -> Result<u64, Self::Error> {
        let elapsed = Instant::now()
            .checked_duration_since(self.origin)
            .ok_or_else(|| {
                MultiplayerError::Protocol("competition progress native clock regressed".into())
            })?;
        u64::try_from(elapsed.as_nanos()).map_err(|_| {
            MultiplayerError::Protocol(
                "competition progress native clock exceeds u64 nanoseconds".into(),
            )
        })
    }
}
