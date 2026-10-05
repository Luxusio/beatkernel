//! Native room compatibility defaults and physical-start adaptation.
use crate::{
    room_competition::RoomCompetition,
    room_network_model::RoomNetworkPort,
    room_ui_host::RoomUiHost,
    room_runtime_host::RoomRuntimeHost,
    native_room_network::NativeRoomNetwork,
    native_room_ui_bridge::NativeRoomUiHost,
    native_room_runtime_bridge::NativeRoomRuntimeHost,
    native_start::{NativeStartAgreement, NativeStartResult, SessionHostBracket},
    multiplayer_start::StartSchedule,
    local_players::PlayerId,
};
use beatkernel::time::ClockPoint;
use std::{io, time::Duration};

pub use crate::room_network_model::RoomNetworkPort as NativeRoomPort;
pub type NativeRoomCompetition<
    P = NativeRoomNetwork,
    H = NativeRoomUiHost,
    R = NativeRoomRuntimeHost,
> = RoomCompetition<P, H, R>;

impl<P: RoomNetworkPort, H: RoomUiHost> RoomCompetition<P, H, NativeRoomRuntimeHost> {
    pub fn new_with_host(
        port: P,
        players: Vec<PlayerId>,
        finish_timeout: Duration,
        host: H,
    ) -> io::Result<Self> {
        Self::new_with_ports(port, players, finish_timeout, host, NativeRoomRuntimeHost)
    }
}

impl<P: RoomNetworkPort, H: RoomUiHost, R: RoomRuntimeHost> NativeStartAgreement
    for RoomCompetition<P, H, R>
{
    fn await_commit(
        &mut self,
        service: &mut dyn FnMut() -> NativeStartResult<bool>,
    ) -> NativeStartResult<bool> {
        self.await_commit_with_service(service)
            .map_err(|error| -> Box<dyn std::error::Error> {
                use crate::room_start_wait::RoomStartWaitError;
                match error {
                    RoomStartWaitError::Port(error) | RoomStartWaitError::Control(error) => {
                        error.into()
                    }
                    RoomStartWaitError::Service(error) => error,
                    RoomStartWaitError::Closing => "native room startup is closing".into(),
                    RoomStartWaitError::LeavePending => {
                        "native room Leave is pending before output activation".into()
                    }
                    RoomStartWaitError::Terminal => {
                        "native room ended before output activation".into()
                    }
                }
            })
    }
    fn committed_schedule(&self) -> NativeStartResult<StartSchedule> {
        self.committed_schedule_value().map_err(Into::into)
    }
    fn host_bracket(
        &self,
        sample: &mut dyn FnMut() -> NativeStartResult<ClockPoint>,
    ) -> NativeStartResult<SessionHostBracket> {
        self.committed_schedule_value()?;
        let before = self.room_clock_ns()?;
        let host = sample()?;
        let after = self.room_clock_ns()?;
        Ok(SessionHostBracket::new(before, host, after)?)
    }
    fn clock_now_ns(&self) -> NativeStartResult<i64> {
        Ok(self.room_clock_ns()?)
    }
}
