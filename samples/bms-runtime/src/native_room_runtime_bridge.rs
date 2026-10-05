//! Native room clock/wait acquisition and implicit cleanup reporting.
use crate::{room_runtime_host::RoomRuntimeHost, room_network_model::RoomOutcome};
pub use crate::native_room_final_wait_bridge::{NativeRoomStartWaitControl, NativeRoomFinalWaitControl};
use std::{
    io,
    time::{Duration, Instant},
};

pub struct NativeRoomRuntimeHost;
impl RoomRuntimeHost for NativeRoomRuntimeHost {
    type StartControl = NativeRoomStartWaitControl;
    type FinalControl = NativeRoomFinalWaitControl;
    fn start_wait(&mut self) -> Self::StartControl {
        NativeRoomStartWaitControl
    }
    fn final_wait(&mut self, timeout: Duration) -> io::Result<(Self::FinalControl, u64)> {
        let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "native room finish deadline overflow",
            )
        })?;
        NativeRoomFinalWaitControl::until(deadline)
    }
    fn dropped(&mut self, outcome: &RoomOutcome) {
        if let Some(error) = outcome.cleanup_error.as_ref().or(outcome.error.as_ref()) {
            eprintln!("native room competition dropped after failure: {error}");
        }
    }
}
