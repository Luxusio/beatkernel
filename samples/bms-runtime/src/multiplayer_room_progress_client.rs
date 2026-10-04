//! One prepared participant's bounded progress publication and receipt history.
//! Local completion does not authorize closing the whole immutable room.

use crate::local_players::PlayerId;
use crate::multiplayer_group::{GroupPrefix, MemberProgress, validate_members};
use crate::multiplayer_group_rooms::{GroupRoomPhase, GroupRoomSnapshot};
use crate::multiplayer_protocol::{MAX_IDENTITY, OutboundFrame};
use crate::multiplayer_room_wire::{RoomMessage, RoomWireError, encode_message, validate_snapshot};
use crate::multiplayer_rooms::ParticipantId;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomProgressClientError {
    InvalidRoom,
    UnknownParticipant,
    InvalidState,
    InvalidProgress,
    InvalidAck,
    InvalidDrain,
    InvalidObservation,
    TimeRegression,
    UnknownWrite,
    IdExhausted,
    Stopped,
    Allocation,
    Wire(RoomWireError),
}

impl fmt::Display for RoomProgressClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRoom => f.write_str("progress client requires valid prepared membership"),
            Self::UnknownParticipant => f.write_str("unknown room progress participant"),
            Self::InvalidState => f.write_str("invalid room progress client operation"),
            Self::InvalidProgress => {
                f.write_str("room progress changed its sequence, roster or counters")
            }
            Self::InvalidAck => {
                f.write_str("room aggregate ACK does not match the actual final upload")
            }
            Self::InvalidDrain => {
                f.write_str("room drain notice lacks matching readiness evidence")
            }
            Self::InvalidObservation => f.write_str("invalid room progress observation time"),
            Self::TimeRegression => f.write_str("room progress observation regressed"),
            Self::UnknownWrite => {
                f.write_str("room progress receipt does not match its in-flight frame")
            }
            Self::IdExhausted => f.write_str("room progress identity space is exhausted"),
            Self::Stopped => f.write_str("room progress client is stopped"),
            Self::Allocation => f.write_str("room progress client allocation failed"),
            Self::Wire(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for RoomProgressClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wire(error) => Some(error),
            _ => None,
        }
    }
}
impl From<RoomWireError> for RoomProgressClientError {
    fn from(error: RoomWireError) -> Self {
        Self::Wire(error)
    }
}

fn copy_slice<T: Copy>(values: &[T]) -> Result<Vec<T>, RoomProgressClientError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(values.len())
        .map_err(|_| RoomProgressClientError::Allocation)?;
    copy.extend_from_slice(values);
    Ok(copy)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Peer {
    id: ParticipantId,
    players: Vec<PlayerId>,
    latest: Option<GroupPrefix>,
    ack_written: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Receipt {
    Upload { final_prefix: bool },
    Ack(usize),
    DrainReady,
}

/// One latest accepted local snapshot, one pending upload and one actual write
/// slot. Coalescing does not consume a wire sequence or erase a queued final.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomProgressClient {
    peers: Vec<Peer>,
    own: usize,
    latest_local: Option<Vec<MemberProgress>>,
    upload_pending: bool,
    final_queued: bool,
    next_sequence: Option<u64>,
    next_id: Option<u64>,
    in_flight: Option<(u64, Receipt)>,
    pending_acks: u64,
    ack_cursor: usize,
    final_upload: Option<(u64, i64)>,
    final_written: bool,
    final_acknowledged: bool,
    early_final_ack: bool,
    drain_requested: bool,
    drain_admitted: Option<i64>,
    drain_written: bool,
    drain_notice: bool,
    last_poll: Option<i64>,
    last_received: Option<i64>,
    active: bool,
    stopped: bool,
}

impl RoomProgressClient {
    pub fn new(
        snapshot: GroupRoomSnapshot<'_>,
        participant: ParticipantId,
    ) -> Result<Self, RoomProgressClientError> {
        if snapshot.phase != GroupRoomPhase::Prepared
            || snapshot.identity.is_empty()
            || snapshot.identity.len() > MAX_IDENTITY
        {
            return Err(RoomProgressClientError::InvalidRoom);
        }
        validate_snapshot(snapshot.members, snapshot.phase, snapshot.deadline_ns)
            .map_err(|_| RoomProgressClientError::InvalidRoom)?;
        let own = snapshot
            .members
            .iter()
            .position(|member| member.id == participant)
            .ok_or(RoomProgressClientError::UnknownParticipant)?;
        let mut peers = Vec::new();
        peers
            .try_reserve_exact(snapshot.members.len())
            .map_err(|_| RoomProgressClientError::Allocation)?;
        for member in snapshot.members {
            peers.push(Peer {
                id: member.id,
                players: copy_slice(&member.players)?,
                latest: None,
                ack_written: false,
            });
        }
        Ok(Self {
            peers,
            own,
            latest_local: None,
            upload_pending: false,
            final_queued: false,
            next_sequence: Some(1),
            next_id: Some(1),
            in_flight: None,
            pending_acks: 0,
            ack_cursor: 0,
            final_upload: None,
            final_written: false,
            final_acknowledged: false,
            early_final_ack: false,
            drain_requested: false,
            drain_admitted: None,
            drain_written: false,
            drain_notice: false,
            last_poll: None,
            last_received: None,
            active: false,
            stopped: false,
        })
    }

    fn ensure_live(&self) -> Result<(), RoomProgressClientError> {
        if self.stopped {
            Err(RoomProgressClientError::Stopped)
        } else {
            Ok(())
        }
    }

    pub fn active(&self) -> bool {
        self.active && !self.stopped
    }

    pub fn activate(&mut self) -> Result<(), RoomProgressClientError> {
        self.ensure_live()?;
        if self.active {
            return Err(RoomProgressClientError::InvalidState);
        }
        self.active = true;
        Ok(())
    }

    pub fn publish(
        &mut self,
        members: &[MemberProgress],
        final_prefix: bool,
    ) -> Result<(), RoomProgressClientError> {
        self.ensure_live()?;
        if !self.active || self.final_queued {
            return Err(RoomProgressClientError::InvalidState);
        }
        let players = &self.peers[self.own].players;
        if members.len() != players.len()
            || members
                .iter()
                .zip(players)
                .any(|(member, player)| member.player != *player)
        {
            return Err(RoomProgressClientError::InvalidProgress);
        }
        validate_members(self.latest_local.as_deref(), members)
            .map_err(|_| RoomProgressClientError::InvalidProgress)?;
        let owned = copy_slice(members)?;
        self.latest_local = Some(owned);
        self.upload_pending = true;
        self.final_queued = final_prefix;
        Ok(())
    }

    pub fn receive(
        &mut self,
        message: &RoomMessage,
        captured_ns: i64,
    ) -> Result<(), RoomProgressClientError> {
        self.ensure_live()?;
        if captured_ns < 0 {
            return Err(RoomProgressClientError::InvalidObservation);
        }
        if self
            .last_received
            .is_some_and(|previous| captured_ns < previous)
        {
            return Err(RoomProgressClientError::TimeRegression);
        }
        match message {
            RoomMessage::PeerProgress {
                participant,
                prefix,
            } => {
                let index = self
                    .peers
                    .iter()
                    .position(|peer| peer.id == *participant)
                    .ok_or(RoomProgressClientError::UnknownParticipant)?;
                if index == self.own {
                    return Err(RoomProgressClientError::InvalidProgress);
                }
                let peer = &self.peers[index];
                if prefix.sequence == 0
                    || peer.latest.as_ref().is_some_and(|previous| {
                        previous.final_prefix || prefix.sequence <= previous.sequence
                    })
                    || prefix.members.len() != peer.players.len()
                    || prefix
                        .members
                        .iter()
                        .zip(&peer.players)
                        .any(|(member, player)| member.player != *player)
                {
                    return Err(RoomProgressClientError::InvalidProgress);
                }
                validate_members(
                    peer.latest
                        .as_ref()
                        .map(|previous| previous.members.as_slice()),
                    &prefix.members,
                )
                .map_err(|_| RoomProgressClientError::InvalidProgress)?;
                let members = copy_slice(&prefix.members)?;
                self.peers[index].latest = Some(GroupPrefix {
                    sequence: prefix.sequence,
                    final_prefix: prefix.final_prefix,
                    members,
                });
                if prefix.final_prefix {
                    self.pending_acks |= 1u64 << index;
                }
            }
            RoomMessage::FinalAck {
                participant,
                sequence,
            } => {
                let (expected, admitted_ns) = self
                    .final_upload
                    .ok_or(RoomProgressClientError::InvalidAck)?;
                if !self.active
                    || *participant != self.peers[self.own].id
                    || *sequence != expected
                    || captured_ns < admitted_ns
                    || self.final_acknowledged
                    || self.early_final_ack
                {
                    return Err(RoomProgressClientError::InvalidAck);
                }
                if self.final_written {
                    self.final_acknowledged = true;
                } else if matches!(
                    self.in_flight,
                    Some((_, Receipt::Upload { final_prefix: true }))
                ) {
                    self.early_final_ack = true;
                } else {
                    return Err(RoomProgressClientError::InvalidAck);
                }
            }
            RoomMessage::DrainComplete {
                participant,
                sequence,
            } => {
                let admitted = self
                    .drain_admitted
                    .ok_or(RoomProgressClientError::InvalidDrain)?;
                let (expected, _) = self
                    .final_upload
                    .ok_or(RoomProgressClientError::InvalidDrain)?;
                if !self.active
                    || !self.drain_requested
                    || self.drain_notice
                    || *participant != self.peers[self.own].id
                    || *sequence != expected
                    || captured_ns < admitted
                    || (!self.drain_written
                        && !matches!(self.in_flight, Some((_, Receipt::DrainReady))))
                {
                    return Err(RoomProgressClientError::InvalidDrain);
                }
                // A matching early notice remains gated by the real Ready
                // full-write receipt; receiving bytes alone grants no drain.
                self.drain_notice = true;
            }
            _ => return Err(RoomProgressClientError::InvalidState),
        }
        self.last_received = Some(captured_ns);
        Ok(())
    }

    /// Lets the enclosing common owner preflight its external write ID without
    /// cloning peer histories or mutating this child's pending work.
    pub(crate) fn has_pending_write(&self) -> bool {
        self.active()
            && self.in_flight.is_none()
            && (self.pending_acks != 0
                || self.upload_pending
                || (self.drain_requested && self.drain_admitted.is_none()))
    }

    pub fn poll_write(
        &mut self,
        now: i64,
    ) -> Result<Option<OutboundFrame>, RoomProgressClientError> {
        self.ensure_live()?;
        if now < 0 {
            return Err(RoomProgressClientError::InvalidObservation);
        }
        if self.last_poll.is_some_and(|previous| now < previous) {
            return Err(RoomProgressClientError::TimeRegression);
        }
        if !self.has_pending_write() {
            self.last_poll = Some(now);
            return Ok(None);
        }
        let id = self.next_id.ok_or(RoomProgressClientError::IdExhausted)?;
        let ack = (0..self.peers.len())
            .map(|offset| (self.ack_cursor + offset) % self.peers.len())
            .find(|index| self.pending_acks & (1u64 << *index) != 0);
        let (message, receipt) = if let Some(index) = ack {
            let peer = &self.peers[index];
            let prefix = peer
                .latest
                .as_ref()
                .ok_or(RoomProgressClientError::InvalidState)?;
            (
                RoomMessage::FinalAck {
                    participant: peer.id,
                    sequence: prefix.sequence,
                },
                Receipt::Ack(index),
            )
        } else if self.upload_pending {
            let sequence = self
                .next_sequence
                .ok_or(RoomProgressClientError::IdExhausted)?;
            let members = copy_slice(
                self.latest_local
                    .as_deref()
                    .ok_or(RoomProgressClientError::InvalidState)?,
            )?;
            (
                RoomMessage::Progress(GroupPrefix {
                    sequence,
                    final_prefix: self.final_queued,
                    members,
                }),
                Receipt::Upload {
                    final_prefix: self.final_queued,
                },
            )
        } else {
            let (sequence, _) = self
                .final_upload
                .ok_or(RoomProgressClientError::InvalidDrain)?;
            (
                RoomMessage::DrainReady {
                    participant: self.peers[self.own].id,
                    sequence,
                },
                Receipt::DrainReady,
            )
        };
        let bytes = encode_message(&message)?;
        if let RoomMessage::Progress(prefix) = message {
            self.next_sequence = prefix.sequence.checked_add(1);
            self.upload_pending = false;
            if prefix.final_prefix {
                self.final_upload = Some((prefix.sequence, now));
            }
        } else if let Some(index) = ack {
            self.pending_acks &= !(1u64 << index);
            self.ack_cursor = (index + 1) % self.peers.len();
        } else {
            self.drain_admitted = Some(now);
        }
        self.next_id = id.checked_add(1);
        self.in_flight = Some((id, receipt));
        self.last_poll = Some(now);
        Ok(Some(OutboundFrame { id, bytes }))
    }

    pub fn written(&mut self, write_id: u64) -> Result<(), RoomProgressClientError> {
        self.ensure_live()?;
        let (expected, receipt) = self
            .in_flight
            .ok_or(RoomProgressClientError::UnknownWrite)?;
        if write_id != expected {
            return Err(RoomProgressClientError::UnknownWrite);
        }
        match receipt {
            Receipt::Upload { final_prefix: true } => {
                self.final_written = true;
                if self.early_final_ack {
                    self.final_acknowledged = true;
                    self.early_final_ack = false;
                }
            }
            Receipt::Upload { .. } => {}
            Receipt::Ack(index) => self.peers[index].ack_written = true,
            Receipt::DrainReady => self.drain_written = true,
        }
        self.in_flight = None;
        Ok(())
    }

    pub fn peer_progress(&self, participant: ParticipantId) -> Option<&GroupPrefix> {
        self.peers
            .iter()
            .find(|peer| peer.id == participant)
            .and_then(|peer| peer.latest.as_ref())
    }

    pub fn local_final_written(&self) -> bool {
        self.final_written
    }
    pub fn local_final_acknowledged(&self) -> bool {
        self.final_acknowledged
    }
    pub fn peer_final_ack_written(&self, participant: ParticipantId) -> bool {
        self.peers
            .iter()
            .any(|peer| peer.id == participant && peer.ack_written)
    }

    /// This participant's receipt boundary, not permission to close the room.
    pub fn local_complete(&self) -> bool {
        self.active()
            && self.final_written
            && self.final_acknowledged
            && self.peers.iter().enumerate().all(|(index, peer)| {
                index == self.own
                    || (peer
                        .latest
                        .as_ref()
                        .is_some_and(|prefix| prefix.final_prefix)
                        && peer.ack_written)
            })
    }

    /// Opt in to coordinated drain only after all genuine local receipts. The
    /// existing local-completion boundary never queues readiness implicitly.
    pub fn request_drain(&mut self) -> Result<(), RoomProgressClientError> {
        self.ensure_live()?;
        if !self.local_complete() || self.drain_requested {
            return Err(RoomProgressClientError::InvalidDrain);
        }
        self.drain_requested = true;
        Ok(())
    }

    pub fn drain_complete(&self) -> bool {
        self.active() && self.drain_written && self.drain_notice
    }

    pub fn stop(&mut self) {
        self.stopped = true;
        self.upload_pending = false;
        self.pending_acks = 0;
        self.in_flight = None;
        self.early_final_ack = false;
    }
}
