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

#[derive(Debug, PartialEq, Eq)]
pub enum RoomFinalStep {
    Completed,
    Wait(u64),
}

#[derive(Clone, Copy)]
enum TerminalState {
    Pending,
    Completed,
    Failed,
}

/// Fixed-deadline state for callers that schedule each pending iteration.
pub struct RoomFinalWaitState {
    network_deadline_ns: i64,
    control_deadline_ns: u64,
    final_queued: bool,
    drain_queued: bool,
    previous_network: Option<i64>,
    previous_control: Option<u64>,
    terminal: TerminalState,
}

impl RoomFinalWaitState {
    pub const fn new(network_deadline_ns: i64, control_deadline_ns: u64) -> Self {
        Self {
            network_deadline_ns,
            control_deadline_ns,
            final_queued: false,
            drain_queued: false,
            previous_network: None,
            previous_control: None,
            terminal: TerminalState::Pending,
        }
    }

    pub fn step<P: RoomFinalPort, C: FinalWaitControl>(
        &mut self,
        port: &mut P,
        control: &mut C,
    ) -> Result<RoomFinalStep, RoomFinalWaitError<P::Error, C::Error>> {
        match self.terminal {
            TerminalState::Completed => return Ok(RoomFinalStep::Completed),
            TerminalState::Failed => return Err(RoomFinalWaitError::InvalidTerminal),
            TerminalState::Pending => {}
        }
        let result = self.pending_step(port, control);
        match &result {
            Ok(RoomFinalStep::Completed) => self.terminal = TerminalState::Completed,
            Err(_) => self.terminal = TerminalState::Failed,
            Ok(RoomFinalStep::Wait(_)) => {}
        }
        result
    }

    fn pending_step<P: RoomFinalPort, C: FinalWaitControl>(
        &mut self,
        port: &mut P,
        control: &mut C,
    ) -> Result<RoomFinalStep, RoomFinalWaitError<P::Error, C::Error>> {
        let observation = port.poll().map_err(RoomFinalWaitError::Port)?;
        let network = port.clock_now_ns().map_err(RoomFinalWaitError::Port)?;
        if network < 0 {
            return Err(RoomFinalWaitError::InvalidClock);
        }
        if self
            .previous_network
            .is_some_and(|previous| network < previous)
        {
            return Err(RoomFinalWaitError::ClockRegressed);
        }
        self.previous_network = Some(network);
        let now = control.now_ns().map_err(RoomFinalWaitError::Control)?;
        if self.previous_control.is_some_and(|previous| now < previous) {
            return Err(RoomFinalWaitError::ClockRegressed);
        }
        if network >= self.network_deadline_ns || now >= self.control_deadline_ns {
            return Err(RoomFinalWaitError::TimedOut);
        }
        if let Some(terminal) = observation.terminal {
            let receipts = terminal.receipts;
            if !terminal.cancelled
                && !terminal.failed
                && self.final_queued
                && self.drain_queued
                && observation.final_accepted
                && observation.drain_accepted
                && receipts.local_final_written
                && receipts.local_final_acknowledged
                && receipts.progress_complete
                && receipts.drain_complete
            {
                return Ok(RoomFinalStep::Completed);
            }
            return Err(RoomFinalWaitError::InvalidTerminal);
        }
        if !self.final_queued && !observation.progress_pending {
            match port.queue_final().map_err(RoomFinalWaitError::Port)? {
                RoomFinalAdmission::Accepted => self.final_queued = true,
                RoomFinalAdmission::QueueFull => {}
            }
        }
        if observation.final_accepted && !self.drain_queued {
            match port.queue_drain().map_err(RoomFinalWaitError::Port)? {
                RoomFinalAdmission::Accepted => self.drain_queued = true,
                RoomFinalAdmission::QueueFull => {}
            }
        }
        let fresh = control.now_ns().map_err(RoomFinalWaitError::Control)?;
        if fresh < now {
            return Err(RoomFinalWaitError::ClockRegressed);
        }
        self.previous_control = Some(fresh);
        Ok(RoomFinalStep::Wait(
            1_000_000.min(self.control_deadline_ns.saturating_sub(fresh)),
        ))
    }
}

pub fn wait_for_room_final<P: RoomFinalPort, C: FinalWaitControl>(
    port: &mut P,
    control: &mut C,
    network_deadline_ns: i64,
    control_deadline_ns: u64,
) -> Result<(), RoomFinalWaitError<P::Error, C::Error>> {
    let mut state = RoomFinalWaitState::new(network_deadline_ns, control_deadline_ns);
    loop {
        match state.step(port, control)? {
            RoomFinalStep::Completed => return Ok(()),
            RoomFinalStep::Wait(duration_ns) => control
                .park_ns(duration_ns)
                .map_err(RoomFinalWaitError::Control)?,
        }
    }
}

#[cfg(test)]
#[path = "room_final_wait_fixtures.rs"]
mod fixtures;

#[cfg(test)]
#[path = "room_final_step_fixtures.rs"]
mod step_fixtures;
