//! One checked software song-start target for a prepared multi-host room.
//!
//! This owner performs no I/O. Complete-write receipts are supplied by the
//! actual per-stream owner; they are not remote application acknowledgements.
//! Commit delivery cannot be made atomic across arbitrary transport loss.

use crate::multiplayer_clock::OffsetEstimate;
use crate::multiplayer_group_rooms::{GroupRoomPhase, GroupRoomSnapshot};
use crate::multiplayer_protocol::MAX_IDENTITY;
use crate::multiplayer_room_wire::validate_snapshot;
use crate::multiplayer_rooms::ParticipantId;
use crate::multiplayer_start::{StartAgreement, StartError, StartMessage, StartPolicy, StartRole};
use std::fmt;

const MAX_PARTICIPANTS: usize = 64;

/// Refused transitions preserve all accepted peer state and the room clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomStartError {
    InvalidRoom,
    UnknownParticipant,
    Stopped,
    Allocation,
    Start(StartError),
}

impl fmt::Display for RoomStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRoom => f.write_str("start requires a valid prepared room"),
            Self::UnknownParticipant => f.write_str("unknown room start participant"),
            Self::Stopped => f.write_str("room start coordinator is stopped"),
            Self::Allocation => f.write_str("room start allocation failed"),
            Self::Start(error) => write!(f, "room start: {error}"),
        }
    }
}

impl std::error::Error for RoomStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StartError> for RoomStartError {
    fn from(error: StartError) -> Self {
        Self::Start(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Control {
    peers: [Option<StartAgreement>; MAX_PARTICIPANTS],
    song_target: Option<i64>,
    last_now: Option<i64>,
    stopped: bool,
}

impl Control {
    fn observe_now(&mut self, now: i64) -> Result<(), StartError> {
        if now < 0 {
            return Err(StartError::NegativeNow);
        }
        if self.last_now.is_some_and(|previous| now < previous) {
            return Err(StartError::TimeRegression);
        }
        self.last_now = Some(now);
        Ok(())
    }

    fn peer(&mut self, index: usize) -> &mut StartAgreement {
        // Only indices from the constructor's immutable participant list enter.
        self.peers[index]
            .as_mut()
            .expect("admitted room participant")
    }
}

/// Control-rate owner for a fixed ordered set of 2..64 actual participant leases.
/// Mutation copies only fixed-size control state; it allocates no event storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomStartCoordinator {
    participants: Vec<ParticipantId>,
    policy: StartPolicy,
    control: Control,
}

impl RoomStartCoordinator {
    /// Validates the actual prepared room before retaining its immutable leases.
    /// Local player IDs remain scoped to their host and need not match other hosts.
    pub fn new(
        snapshot: GroupRoomSnapshot<'_>,
        policy: StartPolicy,
    ) -> Result<Self, RoomStartError> {
        if snapshot.phase != GroupRoomPhase::Prepared
            || snapshot.identity.is_empty()
            || snapshot.identity.len() > MAX_IDENTITY
        {
            return Err(RoomStartError::InvalidRoom);
        }
        validate_snapshot(snapshot.members, snapshot.phase, snapshot.deadline_ns)
            .map_err(|_| RoomStartError::InvalidRoom)?;
        let agreement = StartAgreement::new_at(StartRole::Host, policy, 0)?;
        let mut participants = Vec::new();
        participants
            .try_reserve_exact(snapshot.members.len())
            .map_err(|_| RoomStartError::Allocation)?;
        let mut control = Control {
            peers: [None; MAX_PARTICIPANTS],
            song_target: None,
            last_now: None,
            stopped: false,
        };
        for (index, member) in snapshot.members.iter().enumerate() {
            participants.push(member.id);
            control.peers[index] = Some(agreement);
        }
        Ok(Self {
            participants,
            policy,
            control,
        })
    }

    pub fn participants(&self) -> &[ParticipantId] {
        &self.participants
    }

    fn candidate(
        &self,
        participant: ParticipantId,
        now: i64,
    ) -> Result<(Control, usize), RoomStartError> {
        if self.control.stopped {
            return Err(RoomStartError::Stopped);
        }
        let index = self
            .participants
            .iter()
            .position(|id| *id == participant)
            .ok_or(RoomStartError::UnknownParticipant)?;
        let mut candidate = self.control;
        candidate.observe_now(now)?;
        Ok((candidate, index))
    }

    /// Installs one actual server-local ClockFilter estimate and validates its
    /// freshness immediately. This does not admit or credit a ClockReady write.
    pub fn prepare(
        &mut self,
        participant: ParticipantId,
        estimate: OffsetEstimate,
        now: i64,
    ) -> Result<(), RoomStartError> {
        let (mut candidate, index) = self.candidate(participant, now)?;
        let peer = candidate.peer(index);
        peer.prepare(estimate)?;
        peer.readiness(now)?;
        self.control = candidate;
        Ok(())
    }

    /// Admits at most one message for this participant. Before the whole-room
    /// readiness barrier only ClockReady can be emitted; all proposals then
    /// carry one immutable server song target, regardless of later polling times.
    pub fn next(
        &mut self,
        participant: ParticipantId,
        now: i64,
    ) -> Result<Option<StartMessage>, RoomStartError> {
        let (mut candidate, index) = self.candidate(participant, now)?;
        if candidate.song_target.is_none() {
            if let Some(message) = candidate.peer(index).next_clock_ready(now)? {
                self.control = candidate;
                return Ok(Some(message));
            }
            let mut max_preroll = 0;
            let mut max_uncertainty = 0;
            for peer in candidate.peers.iter().flatten() {
                let Some((preroll, uncertainty)) = peer.readiness(now)? else {
                    self.control = candidate;
                    return Ok(None);
                };
                max_preroll = max_preroll.max(preroll);
                max_uncertainty = max_uncertainty.max(uncertainty);
            }
            let target = i64::try_from(
                i128::from(now)
                    + i128::from(self.policy.lead_ns)
                    + i128::from(max_preroll)
                    + i128::from(max_uncertainty),
            )
            .map_err(|_| StartError::Overflow)?;
            // Preflight every actual peer without admitting any unsent frame.
            // A refusal leaves both the selected target and every peer unchanged.
            for peer in candidate.peers.iter().flatten() {
                let mut proposal = *peer;
                if proposal.propose_song_target(target, now)?.is_none() {
                    return Err(StartError::UnexpectedMessage.into());
                }
            }
            candidate.song_target = Some(target);
        }
        let target = candidate.song_target.expect("selected room song target");
        let accepted = candidate
            .peers
            .iter()
            .flatten()
            .all(StartAgreement::accepted);
        let message = if accepted {
            // The same agreement validates deadline/estimate and exact full
            // Commit write receipts. Earlier Commit writes keep accepted true.
            candidate.peer(index).next(now)?
        } else {
            candidate.peer(index).propose_song_target(target, now)?
        };
        self.control = candidate;
        Ok(message)
    }

    /// Credits only the complete frame previously admitted for this exact peer.
    pub fn written(
        &mut self,
        participant: ParticipantId,
        message: StartMessage,
        now: i64,
    ) -> Result<(), RoomStartError> {
        let (mut candidate, index) = self.candidate(participant, now)?;
        candidate.peer(index).written(message, now)?;
        self.control = candidate;
        Ok(())
    }

    /// Receives actual ClockReady or Accept messages through the common host
    /// agreement. An Accept before the full proposal write is never credited.
    pub fn receive(
        &mut self,
        participant: ParticipantId,
        message: StartMessage,
        now: i64,
    ) -> Result<(), RoomStartError> {
        let (mut candidate, index) = self.candidate(participant, now)?;
        candidate.peer(index).receive(message, now)?;
        self.control = candidate;
        Ok(())
    }

    /// Historical selected server target; selection alone never authorizes play.
    pub fn song_target_ns(&self) -> Option<i64> {
        self.control.song_target
    }

    /// True only after every actual full Commit write, and never after stop.
    /// This is neither a remote application ACK nor physical output alignment.
    pub fn committed(&self) -> bool {
        !self.control.stopped
            && self
                .control
                .peers
                .iter()
                .flatten()
                .all(StartAgreement::committed)
    }

    /// Permanently fences operations while retaining accepted historical data.
    pub fn stop(&mut self) {
        self.control.stopped = true;
    }
}
