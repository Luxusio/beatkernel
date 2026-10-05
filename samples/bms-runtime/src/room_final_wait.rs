//! Room final/drain admission and receipt policy with separate fixed clocks.
use crate::final_ack_wait::FinalWaitControl;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoomFinalReceipts {
    pub local_final_written: bool,
    pub local_final_acknowledged: bool,
    pub progress_complete: bool,
    pub drain_complete: bool,
}
pub struct RoomFinalTerminal {
    pub cancelled: bool,
    pub failed: bool,
    pub receipts: RoomFinalReceipts,
}
pub struct RoomFinalObservation {
    pub progress_pending: bool,
    pub final_accepted: bool,
    pub drain_accepted: bool,
    pub terminal: Option<RoomFinalTerminal>,
}
pub enum RoomFinalAdmission {
    Accepted,
    QueueFull,
}
pub trait RoomFinalPort {
    type Error;
    fn poll(&mut self) -> Result<RoomFinalObservation, Self::Error>;
    fn clock_now_ns(&mut self) -> Result<i64, Self::Error>;
    fn queue_final(&mut self) -> Result<RoomFinalAdmission, Self::Error>;
    fn queue_drain(&mut self) -> Result<RoomFinalAdmission, Self::Error>;
}
pub enum RoomFinalWaitError<P, C> {
    Port(P),
    Control(C),
    TimedOut,
    InvalidTerminal,
    ClockRegressed,
    InvalidClock,
}

pub fn wait_for_room_final<P: RoomFinalPort, C: FinalWaitControl>(
    port: &mut P,
    control: &mut C,
    network_deadline_ns: i64,
    control_deadline_ns: u64,
) -> Result<(), RoomFinalWaitError<P::Error, C::Error>> {
    let mut final_queued = false;
    let mut drain_queued = false;
    let mut previous_network = None;
    let mut previous_control = None;
    loop {
        let observation = port.poll().map_err(RoomFinalWaitError::Port)?;
        let network = port.clock_now_ns().map_err(RoomFinalWaitError::Port)?;
        if network < 0 {
            return Err(RoomFinalWaitError::InvalidClock);
        }
        if previous_network.is_some_and(|previous| network < previous) {
            return Err(RoomFinalWaitError::ClockRegressed);
        }
        previous_network = Some(network);
        let now = control.now_ns().map_err(RoomFinalWaitError::Control)?;
        if previous_control.is_some_and(|previous| now < previous) {
            return Err(RoomFinalWaitError::ClockRegressed);
        }
        if network >= network_deadline_ns || now >= control_deadline_ns {
            return Err(RoomFinalWaitError::TimedOut);
        }
        if let Some(terminal) = observation.terminal {
            let receipts = terminal.receipts;
            if !terminal.cancelled
                && !terminal.failed
                && final_queued
                && drain_queued
                && observation.final_accepted
                && observation.drain_accepted
                && receipts.local_final_written
                && receipts.local_final_acknowledged
                && receipts.progress_complete
                && receipts.drain_complete
            {
                return Ok(());
            }
            return Err(RoomFinalWaitError::InvalidTerminal);
        }
        if !final_queued && !observation.progress_pending {
            match port.queue_final().map_err(RoomFinalWaitError::Port)? {
                RoomFinalAdmission::Accepted => final_queued = true,
                RoomFinalAdmission::QueueFull => {}
            }
        }
        if observation.final_accepted && !drain_queued {
            match port.queue_drain().map_err(RoomFinalWaitError::Port)? {
                RoomFinalAdmission::Accepted => drain_queued = true,
                RoomFinalAdmission::QueueFull => {}
            }
        }
        let fresh = control.now_ns().map_err(RoomFinalWaitError::Control)?;
        if fresh < now {
            return Err(RoomFinalWaitError::ClockRegressed);
        }
        previous_control = Some(fresh);
        control
            .park_ns(1_000_000.min(control_deadline_ns.saturating_sub(fresh)))
            .map_err(RoomFinalWaitError::Control)?;
    }
}

#[cfg(test)]
#[path = "room_final_wait_fixtures.rs"]
mod fixtures;
