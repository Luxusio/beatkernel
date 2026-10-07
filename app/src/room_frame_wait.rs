//! Fixed incomplete-frame waiting over caller-provided observations.
use crate::room_setup_wait::{RoomDeadline, RoomDeadlineError};
#[derive(Debug, PartialEq, Eq)]
pub enum RoomFrameWaitStep {
    Idle,
    Wait(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomFrameWaitError {
    Deadline(RoomDeadlineError),
    ClockRegressed,
    Finished,
}
impl std::fmt::Display for RoomFrameWaitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deadline(RoomDeadlineError::Expired) => {
                f.write_str("incomplete room frame timed out")
            }
            Self::Deadline(error) => write!(f, "invalid room frame deadline: {error:?}"),
            Self::ClockRegressed => f.write_str("room frame clock regressed"),
            Self::Finished => f.write_str("room frame wait has failed"),
        }
    }
}
impl std::error::Error for RoomFrameWaitError {}
pub struct RoomFrameWaitState {
    timeout_ns: u64,
    deadline: Option<RoomDeadline>,
    last_now: Option<i64>,
    failed: bool,
}
impl RoomFrameWaitState {
    pub fn new(timeout_ns: u64) -> Result<Self, RoomFrameWaitError> {
        RoomDeadline::new(0, timeout_ns).map_err(RoomFrameWaitError::Deadline)?;
        Ok(Self {
            timeout_ns,
            deadline: None,
            last_now: None,
            failed: false,
        })
    }
    pub fn observe(
        &mut self,
        now_ns: i64,
        pending: bool,
    ) -> Result<RoomFrameWaitStep, RoomFrameWaitError> {
        if self.failed {
            return Err(RoomFrameWaitError::Finished);
        }
        let result = self.advance(now_ns, pending);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn advance(
        &mut self,
        now_ns: i64,
        pending: bool,
    ) -> Result<RoomFrameWaitStep, RoomFrameWaitError> {
        if now_ns < 0 {
            return Err(RoomFrameWaitError::Deadline(
                RoomDeadlineError::InvalidClock,
            ));
        }
        if self.last_now.is_some_and(|last| now_ns < last) {
            return Err(RoomFrameWaitError::ClockRegressed);
        }
        self.last_now = Some(now_ns);
        if let Some(deadline) = &self.deadline {
            deadline
                .remaining_ns(now_ns)
                .map_err(RoomFrameWaitError::Deadline)?;
        }
        if !pending {
            self.deadline = None;
            return Ok(RoomFrameWaitStep::Idle);
        }
        if self.deadline.is_none() {
            self.deadline = Some(
                RoomDeadline::new(now_ns, self.timeout_ns).map_err(RoomFrameWaitError::Deadline)?,
            );
        }
        Ok(RoomFrameWaitStep::Wait(
            self.deadline
                .as_ref()
                .unwrap()
                .remaining_ns(now_ns)
                .map_err(RoomFrameWaitError::Deadline)?,
        ))
    }
}
#[cfg(test)]
#[path = "room_frame_wait_fixtures.rs"]
mod fixtures;
