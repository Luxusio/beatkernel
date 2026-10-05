//! Native sleep clock for room cleanup waits, outside gameplay callbacks.
use crate::final_ack_wait::FinalWaitControl;
use std::{
    io, thread,
    time::{Duration, Instant},
};
pub struct NativeRoomFinalWaitControl {
    origin: Instant,
}
impl NativeRoomFinalWaitControl {
    pub fn until(deadline: Instant) -> io::Result<(Self, u64)> {
        let origin = Instant::now();
        let remaining = deadline
            .checked_duration_since(origin)
            .unwrap_or(Duration::ZERO);
        let deadline_ns = u64::try_from(remaining.as_nanos()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "native room wait deadline overflow",
            )
        })?;
        Ok((Self { origin }, deadline_ns))
    }
}
impl FinalWaitControl for NativeRoomFinalWaitControl {
    type Error = io::Error;
    fn now_ns(&mut self) -> io::Result<u64> {
        let elapsed = Instant::now()
            .checked_duration_since(self.origin)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "native room wait clock regressed",
                )
            })?;
        u64::try_from(elapsed.as_nanos()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "native room wait clock overflow",
            )
        })
    }
    fn park_ns(&mut self, duration_ns: u64) -> io::Result<()> {
        thread::sleep(Duration::from_nanos(duration_ns));
        Ok(())
    }
}

/// Startup uses the owner's existing setup deadline, with no replacement clock.
pub struct NativeRoomStartWaitControl;
impl crate::room_start_wait::RoomStartWaitControl for NativeRoomStartWaitControl {
    type Error = io::Error;
    fn wait_ns(&mut self, duration_ns: u64) -> io::Result<()> {
        thread::sleep(Duration::from_nanos(duration_ns));
        Ok(())
    }
}
