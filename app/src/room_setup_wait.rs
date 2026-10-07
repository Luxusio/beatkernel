//! Fixed room setup deadlines and caller-scheduled phase observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomDeadlineError {
    InvalidClock,
    InvalidTimeout,
    Overflow,
    Expired,
}
#[derive(Clone, Copy)]
pub struct RoomDeadline {
    deadline_ns: i64,
}
impl RoomDeadline {
    pub fn new(now: i64, timeout: u64) -> Result<Self, RoomDeadlineError> {
        if now < 0 {
            return Err(RoomDeadlineError::InvalidClock);
        }
        if !(1_000_000..=120_000_000_000).contains(&timeout) {
            return Err(RoomDeadlineError::InvalidTimeout);
        }
        let deadline_ns = now
            .checked_add(timeout as i64)
            .ok_or(RoomDeadlineError::Overflow)?;
        Ok(Self { deadline_ns })
    }
    pub fn remaining_ns(&self, now: i64) -> Result<u64, RoomDeadlineError> {
        if now < 0 {
            return Err(RoomDeadlineError::InvalidClock);
        }
        if now >= self.deadline_ns {
            return Err(RoomDeadlineError::Expired);
        }
        Ok((self.deadline_ns - now) as u64)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomSetupPhase {
    Admission,
    Lobby,
    Prepared,
    Complete,
}
#[derive(Debug, PartialEq, Eq)]
pub enum RoomSetupStep {
    Wait(u64),
    Idle,
    Complete,
}
#[derive(Debug, PartialEq, Eq)]
pub enum RoomSetupError {
    Deadline(RoomSetupPhase, RoomDeadlineError),
    ClockRegressed,
    InvalidObservation,
    Finished,
}
pub struct RoomSetupWaitState {
    phase: RoomSetupPhase,
    deadline: Option<RoomDeadline>,
    timeout_ns: u64,
    last_now: i64,
    failed: bool,
}
impl RoomSetupWaitState {
    pub fn new(now: i64, timeout: u64) -> Result<Self, RoomSetupError> {
        let deadline = RoomDeadline::new(now, timeout)
            .map_err(|error| RoomSetupError::Deadline(RoomSetupPhase::Admission, error))?;
        Ok(Self {
            phase: RoomSetupPhase::Admission,
            deadline: Some(deadline),
            timeout_ns: timeout,
            last_now: now,
            failed: false,
        })
    }
    pub fn step(
        &mut self,
        now: i64,
        admitted: bool,
        prepared: bool,
        committed: bool,
    ) -> Result<RoomSetupStep, RoomSetupError> {
        if self.failed {
            return Err(RoomSetupError::Finished);
        }
        if self.phase == RoomSetupPhase::Complete {
            return Ok(RoomSetupStep::Complete);
        }
        let result = self.advance(now, admitted, prepared, committed);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn advance(
        &mut self,
        now: i64,
        admitted: bool,
        prepared: bool,
        committed: bool,
    ) -> Result<RoomSetupStep, RoomSetupError> {
        if now < 0 {
            return Err(RoomSetupError::Deadline(
                self.phase,
                RoomDeadlineError::InvalidClock,
            ));
        }
        if now < self.last_now {
            return Err(RoomSetupError::ClockRegressed);
        }
        self.last_now = now;
        if let Some(deadline) = &self.deadline {
            deadline
                .remaining_ns(now)
                .map_err(|error| RoomSetupError::Deadline(self.phase, error))?;
        }
        if (prepared && !admitted)
            || (committed && !prepared)
            || (self.phase != RoomSetupPhase::Admission && !admitted)
            || (self.phase == RoomSetupPhase::Prepared && !prepared)
        {
            return Err(RoomSetupError::InvalidObservation);
        }
        if self.phase == RoomSetupPhase::Admission && admitted {
            self.phase = RoomSetupPhase::Lobby;
            self.deadline = None;
        }
        if self.phase == RoomSetupPhase::Lobby && prepared {
            let deadline = RoomDeadline::new(now, self.timeout_ns)
                .map_err(|error| RoomSetupError::Deadline(RoomSetupPhase::Prepared, error))?;
            self.phase = RoomSetupPhase::Prepared;
            self.deadline = Some(deadline);
        }
        if committed {
            self.phase = RoomSetupPhase::Complete;
            self.deadline = None;
            return Ok(RoomSetupStep::Complete);
        }
        match &self.deadline {
            Some(deadline) => {
                Ok(RoomSetupStep::Wait(deadline.remaining_ns(now).map_err(
                    |error| RoomSetupError::Deadline(self.phase, error),
                )?))
            }
            None => Ok(RoomSetupStep::Idle),
        }
    }
}
#[cfg(test)]
#[path = "room_setup_wait_fixtures.rs"]
mod fixtures;
