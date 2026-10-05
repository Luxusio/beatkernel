//! Native player UI access at the room-controller compatibility boundary.
use crate::{
    player,
    room_ui_host::RoomUiHost,
    room_presentation::{RoomUiRequest, RoomUiReply, RoomPresentation, RoomResults},
    native_room_competition::{NativeRoomCompetition, NativeRoomPort},
    local_players::PlayerId,
};
use std::{io, sync::Arc, time::Duration};

pub struct NativeRoomUiHost;
impl RoomUiHost for NativeRoomUiHost {
    fn attached(&self) -> bool {
        player::attached()
    }
    fn cancelled(&self) -> bool {
        player::cancelled()
    }
    fn close_controls(&mut self) {
        player::close_room_controls();
    }
    fn take_request(&mut self) -> io::Result<Option<RoomUiRequest>> {
        player::take_room_request()
    }
    fn reply(&mut self, reply: RoomUiReply) -> io::Result<()> {
        player::reply_room(reply)
    }
    fn publish(&mut self, presentation: Arc<RoomPresentation>) -> Result<(), String> {
        player::publish_room(presentation)
    }
    fn retry_publication(&mut self) {
        player::retry_room_publication();
    }
    fn publish_results(&mut self, results: Arc<RoomResults>) -> Result<(), String> {
        player::publish_room_results(results)
    }
}

impl<P: NativeRoomPort> NativeRoomCompetition<P, NativeRoomUiHost> {
    pub fn new(port: P, players: Vec<PlayerId>, finish_timeout: Duration) -> io::Result<Self> {
        Self::new_with_host(port, players, finish_timeout, NativeRoomUiHost)
    }
}
