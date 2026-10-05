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
    let initial = port.initial();
    if initial.cancelled {
        return Ok(false);
    }
    if initial.closing {
        return Err(RoomStartWaitError::Closing);
    }
    loop {
        if !service().map_err(RoomStartWaitError::Service)? {
            return Ok(false);
        }
        let observation = port.poll();
        if observation.cancelled {
            return Ok(false);
        }
        if let Some(error) = observation.failure {
            return Err(RoomStartWaitError::Port(error));
        }
        if observation.leaving {
            return Err(RoomStartWaitError::LeavePending);
        }
        if observation.terminal {
            return Err(RoomStartWaitError::Terminal);
        }
        if observation.committed {
            return Ok(true);
        }
        control
            .wait_ns(1_000_000)
            .map_err(RoomStartWaitError::Control)?;
    }
}

#[cfg(test)]
#[path = "room_start_wait_fixtures.rs"]
mod fixtures;
