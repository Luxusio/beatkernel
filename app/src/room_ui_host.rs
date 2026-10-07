//! UI observations and room publication at an injected business boundary.
use crate::room_presentation::{RoomUiRequest, RoomUiReply, RoomPresentation, RoomResults};
use std::{io, sync::Arc};

pub trait RoomUiHost {
    fn attached(&self) -> bool;
    fn cancelled(&self) -> bool;
    fn close_controls(&mut self);
    fn take_request(&mut self) -> io::Result<Option<RoomUiRequest>>;
    fn reply(&mut self, reply: RoomUiReply) -> io::Result<()>;
    fn publish(&mut self, presentation: Arc<RoomPresentation>) -> Result<(), String>;
    fn retry_publication(&mut self);
    fn publish_results(&mut self, results: Arc<RoomResults>) -> Result<(), String>;
}
