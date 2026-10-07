//! Injected room wait-control creation and implicit cleanup diagnostics.
use crate::{
    final_ack_wait::FinalWaitControl, room_start_wait::RoomStartWaitControl,
    room_network_model::RoomOutcome,
};
use std::{io, time::Duration};

pub trait RoomRuntimeHost {
    type StartControl: RoomStartWaitControl<Error = io::Error>;
    type FinalControl: FinalWaitControl<Error = io::Error>;
    fn start_wait(&mut self) -> Self::StartControl;
    fn final_wait(&mut self, timeout: Duration) -> io::Result<(Self::FinalControl, u64)>;
    fn dropped(&mut self, outcome: &RoomOutcome);
}
