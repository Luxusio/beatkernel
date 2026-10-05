//! Shared terminal effect ordering with opaque ports.

use crate::multiplayer_group::MemberProgress;

pub trait CompetitionTerminalPort {
    type Error;
    fn deliver(&mut self, members: &[MemberProgress]) -> Result<(), Self::Error>;
    fn cleanup(&mut self) -> Result<(), Self::Error>;
    fn drain(&mut self) -> Result<(), Self::Error>;
}

pub enum DeliveryIntent<'a, E> {
    Skip,
    Send(&'a [MemberProgress]),
    Refuse(E),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryStatus {
    Skipped,
    Accepted,
}

pub struct TerminalOutcome<E> {
    pub delivery: Result<DeliveryStatus, E>,
    pub cleanup: Result<(), E>,
    pub drain: Result<(), E>,
}

impl<E> TerminalOutcome<E> {
    pub fn has_failed(&self) -> bool {
        self.delivery.is_err() || self.cleanup.is_err() || self.drain.is_err()
    }

    pub fn into_result(self) -> Result<(), E> {
        self.delivery.map(|_| ()).and(self.cleanup).and(self.drain)
    }
}

/// Every intent attempts cleanup and drain once, retaining each original error.
pub fn finalize_terminal<P: CompetitionTerminalPort>(
    port: &mut P,
    intent: DeliveryIntent<'_, P::Error>,
) -> TerminalOutcome<P::Error> {
    let delivery = match intent {
        DeliveryIntent::Skip => Ok(DeliveryStatus::Skipped),
        DeliveryIntent::Send(members) => port.deliver(members).map(|()| DeliveryStatus::Accepted),
        DeliveryIntent::Refuse(error) => Err(error),
    };
    let cleanup = port.cleanup();
    let drain = port.drain();
    TerminalOutcome {
        delivery,
        cleanup,
        drain,
    }
}

#[cfg(test)]
#[path = "competition_terminal_port_fixtures.rs"]
mod fixtures;
