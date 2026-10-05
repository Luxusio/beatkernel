//! Native wait-clock acquisition and thread parking at cleanup only.
use crate::{final_ack_wait::FinalWaitControl, multiplayer_protocol::MultiplayerError};
use std::{
    thread,
    time::{Duration, Instant},
};

pub(crate) struct NativeFinalWaitControl {
    origin: Instant,
}
impl NativeFinalWaitControl {
    pub(crate) fn until(deadline: Instant) -> Result<(Self, u64), MultiplayerError> {
        let origin = Instant::now();
        let remaining = deadline
            .checked_duration_since(origin)
            .unwrap_or(Duration::ZERO);
        let deadline_ns = u64::try_from(remaining.as_nanos()).map_err(|_| {
            MultiplayerError::Protocol("final wait deadline extent exceeded".into())
        })?;
        Ok((Self { origin }, deadline_ns))
    }
}
impl FinalWaitControl for NativeFinalWaitControl {
    type Error = MultiplayerError;
    fn now_ns(&mut self) -> Result<u64, Self::Error> {
        let elapsed = Instant::now()
            .checked_duration_since(self.origin)
            .ok_or_else(|| MultiplayerError::Protocol("final wait clock regressed".into()))?;
        u64::try_from(elapsed.as_nanos())
            .map_err(|_| MultiplayerError::Protocol("final wait clock extent exceeded".into()))
    }
    fn park_ns(&mut self, duration_ns: u64) -> Result<(), Self::Error> {
        thread::park_timeout(Duration::from_nanos(duration_ns));
        Ok(())
    }
}
