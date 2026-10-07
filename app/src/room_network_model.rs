//! Portable room network configuration, evidence values and ownership port.
use crate::{
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::{StartPolicy, StartSchedule},
};
use std::{fmt, io, sync::Arc, time::Duration};

const MAX_TIMEOUT: Duration = Duration::from_secs(120);
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

/// Fixed deadlines include connection through commitment, and explicit drain
/// admission through genuine Complete respectively. They are never renewed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoomNetworkOptions {
    pub setup_timeout: Duration,
    pub drain_timeout: Duration,
    pub finish_timeout: Duration,
    pub frame_timeout: Duration,
    pub queue_capacity: usize,
    pub start_policy: StartPolicy,
    pub preroll_ns: i64,
}
impl Default for RoomNetworkOptions {
    fn default() -> Self {
        Self {
            setup_timeout: Duration::from_secs(60),
            drain_timeout: Duration::from_secs(10),
            finish_timeout: Duration::from_secs(2),
            frame_timeout: Duration::from_secs(10),
            queue_capacity: 32,
            start_policy: StartPolicy::default(),
            preroll_ns: 0,
        }
    }
}
impl RoomNetworkOptions {
    pub fn validate(self) -> io::Result<()> {
        if !(1..=1024).contains(&self.queue_capacity)
            || [
                self.setup_timeout,
                self.drain_timeout,
                self.finish_timeout,
                self.frame_timeout,
            ]
            .iter()
            .any(|limit| *limit < Duration::from_millis(1) || *limit > MAX_TIMEOUT)
            || self.preroll_ns < 0
        {
            return Err(invalid("invalid bounded native room options"));
        }
        self.start_policy
            .validate()
            .map_err(|error| invalid(error.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomCommand {
    Seal,
    Ready,
    Leave,
    Publish {
        members: Vec<MemberProgress>,
        final_prefix: bool,
    },
    /// Admit one fixed deadline; queue common DrainReady only after actual local completion.
    Drain,
}

/// Retained errors remain readable after stream and thread ownership have joined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomFailure {
    pub kind: io::ErrorKind,
    pub message: String,
}
impl From<io::Error> for RoomFailure {
    fn from(error: io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}
impl fmt::Display for RoomFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RoomFailure {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomReply {
    pub id: u64,
    pub result: Result<(), RoomFailure>,
}
pub use crate::room_final_wait::RoomFinalReceipts as RoomReceipts;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomRoster {
    pub members: Vec<GroupRoomMember>,
    pub phase: GroupRoomPhase,
    pub deadline_ns: Option<i64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomOutcome {
    pub cancelled: bool,
    pub error: Option<RoomFailure>,
    pub cleanup_error: Option<RoomFailure>,
    pub receipts: RoomReceipts,
    pub leave_written: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoomSnapshot {
    /// Changes only when an actual accepted room snapshot changes.
    pub revision: u64,
    pub participant: Option<ParticipantId>,
    pub room: Option<Arc<RoomRoster>>,
    /// Retained genuine schedule on this owner's elapsed clock, never a readiness guess.
    pub schedule: Option<StartSchedule>,
    /// Exact host-qualified prefixes in frozen room order, copied only on sequence changes.
    pub peers: Vec<(ParticipantId, Arc<GroupPrefix>)>,
    pub receipts: RoomReceipts,
    pub terminal: Option<RoomOutcome>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomPoll {
    pub snapshot: RoomSnapshot,
    pub replies: Vec<RoomReply>,
}

pub trait RoomNetworkPort {
    fn try_command(&mut self, command: RoomCommand) -> io::Result<u64>;
    fn poll(&self) -> io::Result<RoomPoll>;
    fn clock_now_ns(&self) -> io::Result<i64>;
    fn request_stop(&self);
    fn stop(&mut self) -> RoomOutcome;
}

#[cfg(test)]
#[path = "room_network_model_fixtures.rs"]
mod fixtures;
