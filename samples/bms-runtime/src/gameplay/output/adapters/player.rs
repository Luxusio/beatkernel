//! Player channel synchronization adapter; no policy lives here.
use super::super::{
    domain::control::{OutputCapability, OutputRequest, OutputReply},
    ports::OutputUiPort,
};
use std::io;

pub struct PlayerOutputUi;
impl OutputUiPort for PlayerOutputUi {
    fn advertise(&mut self, cap: Option<OutputCapability>) -> io::Result<()> {
        crate::player::advertise_output(cap)
    }
    fn take_request(&mut self) -> io::Result<Option<OutputRequest>> {
        crate::player::take_output_request()
    }
    fn reply(&mut self, reply: &OutputReply) -> io::Result<()> {
        crate::player::reply_output(reply)
    }
    fn pending(&self) -> bool {
        crate::player::output_pending()
    }
}
