//! Fixed-deadline final acknowledgement policy over opaque ports.

pub enum FinalAckFailure<E> {
    Closed(E),
    Other(E),
}
pub struct FinalAckObservation<E> {
    pub cancelled: bool,
    pub acknowledged: bool,
    pub closed: bool,
    pub failure: Option<FinalAckFailure<E>>,
}
pub enum FinalAdmission {
    Accepted,
    QueueFull,
}
pub trait FinalAckPort {
    type Error;
    fn poll(&mut self) -> Result<FinalAckObservation<Self::Error>, Self::Error>;
    fn admit(&mut self) -> Result<FinalAdmission, Self::Error>;
}
pub trait FinalWaitControl {
    type Error;
    fn now_ns(&mut self) -> Result<u64, Self::Error>;
    fn park_ns(&mut self, duration_ns: u64) -> Result<(), Self::Error>;
}
pub enum FinalAckWaitError<P, C> {
    Port(P),
    Control(C),
    Cancelled,
    Closed,
    TimedOut,
    ClockRegressed,
}

pub fn wait_for_final_ack<P: FinalAckPort, C: FinalWaitControl>(
    port: &mut P,
    control: &mut C,
    deadline_ns: u64,
    already_admitted: bool,
) -> Result<(), FinalAckWaitError<P::Error, C::Error>> {
    let mut admitted = already_admitted;
    let mut previous = None;
    loop {
        let observation = port.poll().map_err(FinalAckWaitError::Port)?;
        if observation.cancelled {
            return Err(FinalAckWaitError::Cancelled);
        }
        if let Some(failure) = observation.failure {
            match failure {
                FinalAckFailure::Other(error) => return Err(FinalAckWaitError::Port(error)),
                FinalAckFailure::Closed(error) if !observation.acknowledged => {
                    return Err(FinalAckWaitError::Port(error));
                }
                FinalAckFailure::Closed(_) => {}
            }
        }
        if observation.acknowledged {
            return Ok(());
        }
        if observation.closed {
            return Err(FinalAckWaitError::Closed);
        }
        let now = control.now_ns().map_err(FinalAckWaitError::Control)?;
        if previous.is_some_and(|previous| now < previous) {
            return Err(FinalAckWaitError::ClockRegressed);
        }
        if now >= deadline_ns {
            return Err(FinalAckWaitError::TimedOut);
        }
        if !admitted {
            match port.admit().map_err(FinalAckWaitError::Port)? {
                FinalAdmission::Accepted => admitted = true,
                FinalAdmission::QueueFull => {}
            }
        }
        let fresh = control.now_ns().map_err(FinalAckWaitError::Control)?;
        if fresh < now {
            return Err(FinalAckWaitError::ClockRegressed);
        }
        previous = Some(fresh);
        control
            .park_ns(5_000_000.min(deadline_ns.saturating_sub(fresh)))
            .map_err(FinalAckWaitError::Control)?;
    }
}

#[cfg(test)]
#[path = "final_ack_wait_fixtures.rs"]
mod fixtures;
