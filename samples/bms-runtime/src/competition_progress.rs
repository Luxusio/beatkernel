//! Shared comparison-network progress policy with statically dispatched ports.
use crate::{competition_presentation::NetworkStatus, multiplayer_group::MemberProgress};

pub enum ProgressNotice<E> {
    Connected,
    Ready,
    Disconnected(E),
}
pub trait CompetitionProgressPort {
    type Error;
    type Notices: IntoIterator<Item = ProgressNotice<Self::Error>>;
    fn notices(&mut self) -> Self::Notices;
    fn ready(&self) -> bool;
    fn started(&self) -> bool;
    fn publish(&mut self, members: &[MemberProgress]) -> Result<(), Self::Error>;
    fn observe_room(&mut self, members: &[MemberProgress]) -> Result<(), Self::Error>;
    /// Request shutdown without joining an endpoint owner on the observation path.
    fn request_stop(&mut self);
}
/// Drain the entire batch; the first disconnect dominates readiness and stays unwrapped.
pub fn poll_progress<P: CompetitionProgressPort>(
    port: &mut P,
    status: &mut NetworkStatus,
    active: bool,
) -> Result<(), P::Error> {
    let active = active && matches!(*status, NetworkStatus::Waiting | NetworkStatus::Connected);
    let mut next = *status;
    let mut first_error = None;
    for notice in port.notices() {
        match notice {
            ProgressNotice::Connected if active && next != NetworkStatus::Connected => {
                next = NetworkStatus::Waiting
            }
            ProgressNotice::Ready if active => next = NetworkStatus::Connected,
            ProgressNotice::Disconnected(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            _ => {}
        }
    }
    if let Some(error) = first_error {
        if *status != NetworkStatus::Stopped {
            *status = NetworkStatus::Disconnected;
        }
        return Err(error);
    }
    *status = next;
    Ok(())
}
/// Caller cadence can advance only when the actual borrowed publication succeeds.
pub fn publish_progress<P: CompetitionProgressPort>(
    port: &mut P,
    members: &[MemberProgress],
    allowed: bool,
    require_start: bool,
    due: bool,
) -> Result<bool, P::Error> {
    if !allowed || !due || !port.ready() || (require_start && !port.started()) {
        return Ok(false);
    }
    port.publish(members)?;
    Ok(true)
}
/// Room observation keeps its separate controller cadence and prefix ownership.
pub fn observe_room_progress<P: CompetitionProgressPort>(
    port: &mut P,
    members: &[MemberProgress],
) -> Result<(), P::Error> {
    port.observe_room(members)
}
#[cfg(test)]
#[path = "competition_progress_port_fixtures.rs"]
mod fixtures;
