//! Room startup observation ordering with injected service and waiting.

pub struct RoomStartInitial {
    pub cancelled: bool,
    pub closing: bool,
}
pub struct RoomStartObservation<E> {
    pub cancelled: bool,
    pub failure: Option<E>,
    pub leaving: bool,
    pub terminal: bool,
    pub committed: bool,
}
pub trait RoomStartPort {
    type Error;
    fn initial(&mut self) -> RoomStartInitial;
    fn poll(&mut self) -> RoomStartObservation<Self::Error>;
}
pub trait RoomStartWaitControl {
    type Error;
    fn wait_ns(&mut self, duration_ns: u64) -> Result<(), Self::Error>;
}
pub enum RoomStartWaitError<P, S, C> {
    Port(P),
    Service(S),
    Control(C),
    Closing,
    LeavePending,
    Terminal,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RoomStartStep {
    Pending(u64),
    Ready,
    Cancelled,
}

pub enum RoomStartStepError<P, S> {
    Port(P),
    Service(S),
    Closing,
    LeavePending,
    Terminal,
}

#[derive(Clone, Copy)]
enum StartTerminal {
    Pending,
    Ready,
    Cancelled,
    Failed,
}

/// One owner of startup observation state; callers schedule each pending step.
pub struct RoomStartWaitState {
    initial_observed: bool,
    terminal: StartTerminal,
}
impl RoomStartWaitState {
    pub const fn new() -> Self {
        Self {
            initial_observed: false,
            terminal: StartTerminal::Pending,
        }
    }
    pub fn step<P: RoomStartPort, F: FnMut() -> Result<bool, S> + ?Sized, S>(
        &mut self,
        port: &mut P,
        service: &mut F,
    ) -> Result<RoomStartStep, RoomStartStepError<P::Error, S>> {
        match self.terminal {
            StartTerminal::Ready => return Ok(RoomStartStep::Ready),
            StartTerminal::Cancelled => return Ok(RoomStartStep::Cancelled),
            StartTerminal::Failed => return Err(RoomStartStepError::Terminal),
            StartTerminal::Pending => {}
        }
        let result = self.pending_step(port, service);
        match &result {
            Ok(RoomStartStep::Ready) => self.terminal = StartTerminal::Ready,
            Ok(RoomStartStep::Cancelled) => self.terminal = StartTerminal::Cancelled,
            Err(_) => self.terminal = StartTerminal::Failed,
            Ok(RoomStartStep::Pending(_)) => {}
        }
        result
    }
    fn pending_step<P: RoomStartPort, F: FnMut() -> Result<bool, S> + ?Sized, S>(
        &mut self,
        port: &mut P,
        service: &mut F,
    ) -> Result<RoomStartStep, RoomStartStepError<P::Error, S>> {
        if !self.initial_observed {
            self.initial_observed = true;
            let initial = port.initial();
            if initial.cancelled {
                return Ok(RoomStartStep::Cancelled);
            }
            if initial.closing {
                return Err(RoomStartStepError::Closing);
            }
        }
        if !service().map_err(RoomStartStepError::Service)? {
            return Ok(RoomStartStep::Cancelled);
        }
        let observation = port.poll();
        if observation.cancelled {
            return Ok(RoomStartStep::Cancelled);
        }
        if let Some(error) = observation.failure {
            return Err(RoomStartStepError::Port(error));
        }
        if observation.leaving {
            return Err(RoomStartStepError::LeavePending);
        }
        if observation.terminal {
            return Err(RoomStartStepError::Terminal);
        }
        if observation.committed {
            return Ok(RoomStartStep::Ready);
        }
        Ok(RoomStartStep::Pending(1_000_000))
    }
}

pub fn await_room_start<
    P: RoomStartPort,
    C: RoomStartWaitControl,
    F: FnMut() -> Result<bool, S> + ?Sized,
    S,
>(
    port: &mut P,
    control: &mut C,
    service: &mut F,
) -> Result<bool, RoomStartWaitError<P::Error, S, C::Error>> {
    let mut state = RoomStartWaitState::new();
    loop {
        let step = state.step(port, service).map_err(|error| match error {
            RoomStartStepError::Port(error) => RoomStartWaitError::Port(error),
            RoomStartStepError::Service(error) => RoomStartWaitError::Service(error),
            RoomStartStepError::Closing => RoomStartWaitError::Closing,
            RoomStartStepError::LeavePending => RoomStartWaitError::LeavePending,
            RoomStartStepError::Terminal => RoomStartWaitError::Terminal,
        })?;
        match step {
            RoomStartStep::Ready => return Ok(true),
            RoomStartStep::Cancelled => return Ok(false),
            RoomStartStep::Pending(duration_ns) => control
                .wait_ns(duration_ns)
                .map_err(RoomStartWaitError::Control)?,
        }
    }
}

#[cfg(test)]
#[path = "room_start_wait_fixtures.rs"]
mod fixtures;

#[cfg(test)]
#[path = "room_start_step_fixtures.rs"]
mod step_fixtures;
