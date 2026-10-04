//! Bounded participant-scoped room progress and genuine recipient ACK ownership.
//! The caller owns start admission, stream writes, failure disposal and clocks.

use crate::local_players::PlayerId;
use crate::multiplayer_group::{GroupPrefix, validate_members};
use crate::multiplayer_group_rooms::{GroupRoomPhase, GroupRoomSnapshot};
use crate::multiplayer_protocol::MAX_IDENTITY;
use crate::multiplayer_room_wire::{RoomMessage, RoomWireError, encode_message, validate_snapshot};
use crate::multiplayer_rooms::ParticipantId;
use std::{fmt, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomProgressError {
    InvalidRoom,
    UnknownParticipant,
    InvalidState,
    InvalidProgress,
    InvalidAck,
    UnknownWrite,
    IdExhausted,
    Stopped,
    Allocation,
    Wire(RoomWireError),
}

impl fmt::Display for RoomProgressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRoom => f.write_str("progress requires a valid prepared room"),
            Self::UnknownParticipant => f.write_str("unknown room progress participant"),
            Self::InvalidState => f.write_str("invalid room progress operation"),
            Self::InvalidProgress => {
                f.write_str("room progress changed its sequence, roster or counters")
            }
            Self::InvalidAck => {
                f.write_str("room final acknowledgement lacks matching delivery evidence")
            }
            Self::UnknownWrite => {
                f.write_str("room relay receipt does not match its in-flight frame")
            }
            Self::IdExhausted => f.write_str("room relay write identity space is exhausted"),
            Self::Stopped => f.write_str("room progress relay is stopped"),
            Self::Allocation => f.write_str("room progress allocation failed"),
            Self::Wire(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for RoomProgressError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wire(error) => Some(error),
            _ => None,
        }
    }
}
impl From<RoomWireError> for RoomProgressError {
    fn from(error: RoomWireError) -> Self {
        Self::Wire(error)
    }
}

/// One recipient's immutable actual frame, credited only by its full-write ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomRelayWrite {
    pub id: u64,
    pub bytes: Arc<Vec<u8>>,
    /// Actual final peer-prefix source for transport capture-time fencing.
    /// Ordinary prefixes and aggregate ACK notices have no final relay source.
    pub final_source: Option<ParticipantId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Latest {
    prefix: GroupPrefix,
    bytes: Arc<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    id: ParticipantId,
    players: Vec<PlayerId>,
    latest: Option<Latest>,
    delivered: u64,
    acknowledged: u64,
    aggregate_written: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Receipt {
    Prefix {
        source: usize,
        sequence: u64,
        final_prefix: bool,
        early_ack: bool,
    },
    Aggregate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Recipient {
    dirty: u64,
    cursor: usize,
    next_id: Option<u64>,
    in_flight: Option<(u64, Receipt)>,
}

/// One latest encoded prefix per source and one bounded write slot per recipient.
/// Uploads may be staged before activation; the actual start owner alone decides
/// when every Commit has been fully written and calls `activate` once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomProgressRelay {
    sources: Vec<Source>,
    recipients: Vec<Recipient>,
    mask: u64,
    active: bool,
    stopped: bool,
}

impl RoomProgressRelay {
    pub fn new(snapshot: GroupRoomSnapshot<'_>) -> Result<Self, RoomProgressError> {
        if snapshot.phase != GroupRoomPhase::Prepared
            || snapshot.identity.is_empty()
            || snapshot.identity.len() > MAX_IDENTITY
        {
            return Err(RoomProgressError::InvalidRoom);
        }
        validate_snapshot(snapshot.members, snapshot.phase, snapshot.deadline_ns)
            .map_err(|_| RoomProgressError::InvalidRoom)?;
        let mut sources = Vec::new();
        let mut recipients = Vec::new();
        sources
            .try_reserve_exact(snapshot.members.len())
            .map_err(|_| RoomProgressError::Allocation)?;
        recipients
            .try_reserve_exact(snapshot.members.len())
            .map_err(|_| RoomProgressError::Allocation)?;
        for member in snapshot.members {
            let mut players = Vec::new();
            players
                .try_reserve_exact(member.players.len())
                .map_err(|_| RoomProgressError::Allocation)?;
            players.extend_from_slice(&member.players);
            sources.push(Source {
                id: member.id,
                players,
                latest: None,
                delivered: 0,
                acknowledged: 0,
                aggregate_written: false,
            });
            recipients.push(Recipient {
                dirty: 0,
                cursor: 0,
                next_id: Some(1),
                in_flight: None,
            });
        }
        let mask = if sources.len() == 64 {
            u64::MAX
        } else {
            (1u64 << sources.len()) - 1
        };
        Ok(Self {
            sources,
            recipients,
            mask,
            active: false,
            stopped: false,
        })
    }

    fn ensure_live(&self) -> Result<(), RoomProgressError> {
        if self.stopped {
            Err(RoomProgressError::Stopped)
        } else {
            Ok(())
        }
    }

    fn index(&self, participant: ParticipantId) -> Result<usize, RoomProgressError> {
        self.sources
            .iter()
            .position(|source| source.id == participant)
            .ok_or(RoomProgressError::UnknownParticipant)
    }

    fn required(&self, source: usize) -> u64 {
        self.mask & !(1u64 << source)
    }

    pub fn active(&self) -> bool {
        self.active && !self.stopped
    }

    pub fn activate(&mut self) -> Result<(), RoomProgressError> {
        self.ensure_live()?;
        if self.active {
            return Err(RoomProgressError::InvalidState);
        }
        self.active = true;
        Ok(())
    }

    /// The lease argument supplies the source/recipient identity. A client cannot
    /// submit a labelled PeerProgress or acknowledge its own final prefix.
    pub fn receive(
        &mut self,
        lease: ParticipantId,
        message: &RoomMessage,
    ) -> Result<(), RoomProgressError> {
        self.ensure_live()?;
        let index = self.index(lease)?;
        match message {
            RoomMessage::Progress(prefix) => self.receive_progress(index, prefix),
            RoomMessage::FinalAck {
                participant,
                sequence,
            } => self.receive_ack(index, *participant, *sequence),
            _ => Err(RoomProgressError::InvalidState),
        }
    }

    fn receive_progress(
        &mut self,
        index: usize,
        prefix: &GroupPrefix,
    ) -> Result<(), RoomProgressError> {
        let source = &self.sources[index];
        let previous = source.latest.as_ref().map(|latest| &latest.prefix);
        let expected = match previous {
            Some(previous) if !previous.final_prefix => previous.sequence.checked_add(1),
            Some(_) => None,
            None => Some(1),
        };
        if expected != Some(prefix.sequence)
            || prefix.members.len() != source.players.len()
            || prefix
                .members
                .iter()
                .zip(&source.players)
                .any(|(member, player)| member.player != *player)
        {
            return Err(RoomProgressError::InvalidProgress);
        }
        validate_members(
            previous.map(|prefix| prefix.members.as_slice()),
            &prefix.members,
        )
        .map_err(|_| RoomProgressError::InvalidProgress)?;
        let mut members = Vec::new();
        members
            .try_reserve_exact(prefix.members.len())
            .map_err(|_| RoomProgressError::Allocation)?;
        members.extend_from_slice(&prefix.members);
        let message = RoomMessage::PeerProgress {
            participant: source.id,
            prefix: GroupPrefix {
                sequence: prefix.sequence,
                final_prefix: prefix.final_prefix,
                members,
            },
        };
        let bytes = Arc::new(encode_message(&message)?);
        // Recover the owned member snapshot used by the actual encoder, rather
        // than allocating a second copy for retained validation state.
        let RoomMessage::PeerProgress { prefix, .. } = message else {
            return Err(RoomProgressError::InvalidState);
        };
        self.sources[index].latest = Some(Latest { prefix, bytes });
        for (recipient, state) in self.recipients.iter_mut().enumerate() {
            if recipient != index {
                state.dirty |= 1u64 << index;
            }
        }
        Ok(())
    }

    fn receive_ack(
        &mut self,
        recipient: usize,
        participant: ParticipantId,
        sequence: u64,
    ) -> Result<(), RoomProgressError> {
        if !self.active {
            return Err(RoomProgressError::InvalidState);
        }
        let source = self.index(participant)?;
        let accepted = &self.sources[source];
        let final_prefix = accepted
            .latest
            .as_ref()
            .filter(|latest| latest.prefix.final_prefix)
            .ok_or(RoomProgressError::InvalidAck)?;
        let bit = 1u64 << recipient;
        if recipient == source
            || sequence != final_prefix.prefix.sequence
            || accepted.acknowledged & bit != 0
        {
            return Err(RoomProgressError::InvalidAck);
        }
        if accepted.delivered & bit != 0 {
            self.sources[source].acknowledged |= bit;
            return Ok(());
        }
        let in_flight = &mut self.recipients[recipient].in_flight;
        match in_flight {
            Some((
                _,
                Receipt::Prefix {
                    source: expected,
                    sequence: expected_sequence,
                    final_prefix: true,
                    early_ack,
                },
            )) if *expected == source && *expected_sequence == sequence && !*early_ack => {
                *early_ack = true;
                Ok(())
            }
            _ => Err(RoomProgressError::InvalidAck),
        }
    }

    /// Priority is an eligible aggregate final ACK, then round-robin dirty
    /// sources. Repeated polling never resends or credits an outstanding frame.
    pub fn poll_write(
        &mut self,
        recipient: ParticipantId,
    ) -> Result<Option<RoomRelayWrite>, RoomProgressError> {
        self.ensure_live()?;
        let index = self.index(recipient)?;
        let state = self.recipients[index];
        if !self.active || state.in_flight.is_some() {
            return Ok(None);
        }
        let source = &self.sources[index];
        let aggregate = source.latest.as_ref().filter(|latest| {
            latest.prefix.final_prefix
                && source.acknowledged == self.required(index)
                && !source.aggregate_written
        });
        let next = if let Some(latest) = aggregate {
            let bytes = Arc::new(encode_message(&RoomMessage::FinalAck {
                participant: recipient,
                sequence: latest.prefix.sequence,
            })?);
            Some((bytes, Receipt::Aggregate, None))
        } else {
            let next_source = (0..self.sources.len())
                .map(|offset| (state.cursor + offset) % self.sources.len())
                .find(|source| state.dirty & (1u64 << *source) != 0);
            if let Some(source) = next_source {
                let latest = self.sources[source]
                    .latest
                    .as_ref()
                    .ok_or(RoomProgressError::InvalidState)?;
                Some((
                    Arc::clone(&latest.bytes),
                    Receipt::Prefix {
                        source,
                        sequence: latest.prefix.sequence,
                        final_prefix: latest.prefix.final_prefix,
                        early_ack: false,
                    },
                    Some(source),
                ))
            } else {
                None
            }
        };
        let Some((bytes, receipt, source)) = next else {
            return Ok(None);
        };
        let id = state.next_id.ok_or(RoomProgressError::IdExhausted)?;
        let state = &mut self.recipients[index];
        state.next_id = id.checked_add(1);
        state.in_flight = Some((id, receipt));
        if let Some(source) = source {
            state.dirty &= !(1u64 << source);
            state.cursor = (source + 1) % self.sources.len();
        }
        let final_source = match receipt {
            Receipt::Prefix {
                source,
                final_prefix: true,
                ..
            } => Some(self.sources[source].id),
            _ => None,
        };
        Ok(Some(RoomRelayWrite {
            id,
            bytes,
            final_source,
        }))
    }

    pub fn written(
        &mut self,
        recipient: ParticipantId,
        write_id: u64,
    ) -> Result<(), RoomProgressError> {
        self.ensure_live()?;
        let index = self.index(recipient)?;
        let (expected, receipt) = self.recipients[index]
            .in_flight
            .ok_or(RoomProgressError::UnknownWrite)?;
        if write_id != expected {
            return Err(RoomProgressError::UnknownWrite);
        }
        match receipt {
            Receipt::Prefix {
                source,
                final_prefix: true,
                early_ack,
                ..
            } => {
                self.sources[source].delivered |= 1u64 << index;
                if early_ack {
                    self.sources[source].acknowledged |= 1u64 << index;
                }
            }
            Receipt::Prefix { .. } => {}
            Receipt::Aggregate => self.sources[index].aggregate_written = true,
        }
        self.recipients[index].in_flight = None;
        Ok(())
    }

    /// Historical recipient ACK evidence remains inspectable after stop; an
    /// aggregate notice's own complete write is a separate completion barrier.
    pub fn final_acknowledged(&self, source: ParticipantId) -> Result<bool, RoomProgressError> {
        let index = self.index(source)?;
        let source = &self.sources[index];
        Ok(source
            .latest
            .as_ref()
            .is_some_and(|latest| latest.prefix.final_prefix)
            && source.acknowledged == self.required(index))
    }

    pub fn complete(&self) -> bool {
        self.active()
            && self.sources.iter().enumerate().all(|(index, source)| {
                source
                    .latest
                    .as_ref()
                    .is_some_and(|latest| latest.prefix.final_prefix)
                    && source.acknowledged == self.required(index)
                    && source.aggregate_written
            })
    }

    pub fn stop(&mut self) {
        self.stopped = true;
        for recipient in &mut self.recipients {
            recipient.dirty = 0;
            recipient.in_flight = None;
        }
    }
}
