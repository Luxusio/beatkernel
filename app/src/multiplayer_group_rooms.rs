//! Bounded multi-host room ownership, independent of transports and start clocks.
//!
//! A prepared room has only collected each host's preparation declaration. It
//! does not prove a start, a written frame, or an application acknowledgement.
//! Returned tickets remain the caller's responsibility to close or dispose.

use crate::local_players::PlayerId;
use crate::multiplayer_group::validate_roster;
use crate::multiplayer_protocol::MAX_IDENTITY;
use crate::multiplayer_rooms::{ParticipantId, validate_room_key};
use std::{collections::BTreeMap, fmt};

/// Fixed bounds for independently owned rooms and their host rosters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupRoomPolicy {
    max_rooms: usize,
    max_hosts: usize,
    max_key_bytes: usize,
    waiting_ttl_ns: i64,
}

impl GroupRoomPolicy {
    pub fn new(
        max_rooms: usize,
        max_hosts: usize,
        max_key_bytes: usize,
        waiting_ttl_ns: i64,
    ) -> Result<Self, GroupRoomError> {
        if !(1..=4096).contains(&max_rooms)
            || !(2..=64).contains(&max_hosts)
            || !(1..=1024).contains(&max_key_bytes)
            || waiting_ttl_ns <= 0
        {
            return Err(GroupRoomError::InvalidPolicy);
        }
        Ok(Self {
            max_rooms,
            max_hosts,
            max_key_bytes,
            waiting_ttl_ns,
        })
    }
}

/// One caller-owned resource. IDs are never reused within a registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupParticipantTicket {
    pub id: ParticipantId,
    pub room: String,
}

/// Player IDs are scoped to this host ID, not globally unique across hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupRoomMember {
    pub id: ParticipantId,
    pub players: Vec<PlayerId>,
    pub prepared: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupRoomPhase {
    Collecting,
    Frozen,
    Prepared,
}

/// Immutable identity and host order. The first member owns roster sealing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupRoomSnapshot<'a> {
    pub identity: &'a [u8],
    pub members: &'a [GroupRoomMember],
    pub phase: GroupRoomPhase,
    pub deadline_ns: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupRoomError {
    InvalidPolicy,
    InvalidKey,
    InvalidIdentity,
    IdentityMismatch,
    InvalidRoster,
    InvalidTime,
    ClockRegressed {
        previous: i64,
        now: i64,
    },
    RoomFull,
    Capacity,
    ExpiryRequired {
        owner: ParticipantId,
        deadline_ns: i64,
    },
    IdExhausted,
    DeadlineOverflow,
    UnknownParticipant,
    NotOwner,
    RoomFrozen,
    TooFewHosts,
    NotFrozen,
    AlreadyPrepared,
    Stopped,
    Allocation,
}

impl fmt::Display for GroupRoomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy => f.write_str("invalid bounded group room policy"),
            Self::InvalidKey => f.write_str("invalid bounded ASCII room key"),
            Self::InvalidIdentity => f.write_str("invalid bounded canonical identity"),
            Self::IdentityMismatch => f.write_str("group room canonical identity differs"),
            Self::InvalidRoster => f.write_str("host requires 1..64 positive unique player IDs"),
            Self::InvalidTime => f.write_str("group room clock must be nonnegative"),
            Self::ClockRegressed { previous, now } => {
                write!(f, "group room clock regressed from {previous} to {now}")
            }
            Self::RoomFull => f.write_str("group room is at its host capacity"),
            Self::Capacity => f.write_str("group room registry is at capacity"),
            Self::ExpiryRequired { owner, deadline_ns } => write!(
                f,
                "room owned by participant {} expired at {deadline_ns}; process expiry first",
                owner.0
            ),
            Self::IdExhausted => f.write_str("participant identity space is exhausted"),
            Self::DeadlineOverflow => f.write_str("preparation deadline exceeds the room clock"),
            Self::UnknownParticipant => f.write_str("unknown group room participant"),
            Self::NotOwner => f.write_str("only the first host can seal its room"),
            Self::RoomFrozen => f.write_str("group room roster is already frozen"),
            Self::TooFewHosts => f.write_str("sealing requires at least two hosts"),
            Self::NotFrozen => f.write_str("preparation requires a sealed host roster"),
            Self::AlreadyPrepared => f.write_str("host preparation was already declared"),
            Self::Stopped => f.write_str("group room registry is stopped"),
            Self::Allocation => f.write_str("group room allocation failed"),
        }
    }
}

impl std::error::Error for GroupRoomError {}

#[derive(Debug)]
struct Room {
    identity: Vec<u8>,
    members: Vec<GroupRoomMember>,
    phase: GroupRoomPhase,
    deadline_ns: Option<i64>,
}

impl Room {
    fn expired(&self, now: i64) -> bool {
        self.deadline_ns.is_some_and(|deadline| now >= deadline)
    }

    fn require_unexpired(&self, now: i64) -> Result<(), GroupRoomError> {
        if let Some(deadline_ns) = self.deadline_ns.filter(|deadline| now >= *deadline) {
            // Every admitted room has at least one member, retained until removal.
            return Err(GroupRoomError::ExpiryRequired {
                owner: self.members[0].id,
                deadline_ns,
            });
        }
        Ok(())
    }
}

fn copy_slice<T: Copy>(values: &[T]) -> Result<Vec<T>, GroupRoomError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(values.len())
        .map_err(|_| GroupRoomError::Allocation)?;
    result.extend_from_slice(values);
    Ok(result)
}

fn copy_key(key: &str) -> Result<String, GroupRoomError> {
    let mut result = String::new();
    result
        .try_reserve_exact(key.len())
        .map_err(|_| GroupRoomError::Allocation)?;
    result.push_str(key);
    Ok(result)
}

/// Caller-clocked owner for collecting, frozen, and prepared host rosters.
///
/// Errors do not consume identities or advance the accepted clock. Successful
/// empty release/expiry/stop operations do advance it. No operation evicts a
/// room to admit another, and no resource is silently removed on expiry errors.
#[derive(Debug)]
pub struct GroupRoomRegistry {
    policy: GroupRoomPolicy,
    rooms: BTreeMap<String, Room>,
    next_id: Option<u64>,
    last_time: Option<i64>,
    stopped: bool,
}

impl GroupRoomRegistry {
    pub fn new(policy: GroupRoomPolicy) -> Self {
        Self {
            policy,
            rooms: BTreeMap::new(),
            next_id: Some(1),
            last_time: None,
            stopped: false,
        }
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    pub fn participant_count(&self) -> usize {
        self.rooms.values().map(|room| room.members.len()).sum()
    }

    pub fn room(&self, key: &str) -> Option<GroupRoomSnapshot<'_>> {
        self.rooms.get(key).map(|room| GroupRoomSnapshot {
            identity: &room.identity,
            members: &room.members,
            phase: room.phase,
            deadline_ns: room.deadline_ns,
        })
    }

    fn validate_time(&self, now: i64) -> Result<(), GroupRoomError> {
        if now < 0 {
            return Err(GroupRoomError::InvalidTime);
        }
        if let Some(previous) = self.last_time {
            if now < previous {
                return Err(GroupRoomError::ClockRegressed { previous, now });
            }
        }
        Ok(())
    }

    fn validate_active(&self, now: i64) -> Result<(), GroupRoomError> {
        self.validate_time(now)?;
        if self.stopped {
            return Err(GroupRoomError::Stopped);
        }
        Ok(())
    }

    /// Join a collecting room without changing its first-admission deadline.
    pub fn join(
        &mut self,
        key: &str,
        identity: &[u8],
        players: &[PlayerId],
        now: i64,
    ) -> Result<GroupParticipantTicket, GroupRoomError> {
        self.validate_active(now)?;
        validate_room_key(key, self.policy.max_key_bytes)
            .map_err(|_| GroupRoomError::InvalidKey)?;
        if identity.is_empty() || identity.len() > MAX_IDENTITY {
            return Err(GroupRoomError::InvalidIdentity);
        }
        validate_roster(players).map_err(|_| GroupRoomError::InvalidRoster)?;
        let deadline = if let Some(room) = self.rooms.get(key) {
            room.require_unexpired(now)?;
            if room.phase != GroupRoomPhase::Collecting {
                return Err(GroupRoomError::RoomFrozen);
            }
            if room.identity != identity {
                return Err(GroupRoomError::IdentityMismatch);
            }
            if room.members.len() >= self.policy.max_hosts {
                return Err(GroupRoomError::RoomFull);
            }
            None
        } else {
            if self.rooms.len() >= self.policy.max_rooms {
                return Err(GroupRoomError::Capacity);
            }
            Some(
                now.checked_add(self.policy.waiting_ttl_ns)
                    .ok_or(GroupRoomError::DeadlineOverflow)?,
            )
        };
        let id = ParticipantId(self.next_id.ok_or(GroupRoomError::IdExhausted)?);
        let next_id = id.0.checked_add(1);
        let ticket = GroupParticipantTicket {
            id,
            room: copy_key(key)?,
        };
        let member = GroupRoomMember {
            id,
            players: copy_slice(players)?,
            prepared: false,
        };
        if let Some(room) = self.rooms.get_mut(key) {
            room.members
                .try_reserve_exact(1)
                .map_err(|_| GroupRoomError::Allocation)?;
            room.members.push(member);
        } else {
            let identity = copy_slice(identity)?;
            let key = copy_key(key)?;
            let mut members = Vec::new();
            members
                .try_reserve_exact(1)
                .map_err(|_| GroupRoomError::Allocation)?;
            members.push(member);
            self.rooms.insert(
                key,
                Room {
                    identity,
                    members,
                    phase: GroupRoomPhase::Collecting,
                    deadline_ns: deadline,
                },
            );
        }
        self.next_id = next_id;
        self.last_time = Some(now);
        Ok(ticket)
    }

    /// Freeze the admitted order; only its first host may seal a room of two or more.
    pub fn seal(&mut self, owner: ParticipantId, now: i64) -> Result<(), GroupRoomError> {
        self.validate_active(now)?;
        let room = self
            .rooms
            .values_mut()
            .find(|room| room.members.iter().any(|member| member.id == owner))
            .ok_or(GroupRoomError::UnknownParticipant)?;
        room.require_unexpired(now)?;
        if room.members[0].id != owner {
            return Err(GroupRoomError::NotOwner);
        }
        if room.phase != GroupRoomPhase::Collecting {
            return Err(GroupRoomError::RoomFrozen);
        }
        if room.members.len() < 2 {
            return Err(GroupRoomError::TooFewHosts);
        }
        room.phase = GroupRoomPhase::Frozen;
        self.last_time = Some(now);
        Ok(())
    }

    /// Declare one host prepared exactly once. True means every frozen host has
    /// declared preparation, without making any transport/start/ACK assertion.
    pub fn ready(&mut self, id: ParticipantId, now: i64) -> Result<bool, GroupRoomError> {
        self.validate_active(now)?;
        let (room, index) = self
            .rooms
            .values_mut()
            .find_map(|room| {
                let index = room.members.iter().position(|member| member.id == id)?;
                Some((room, index))
            })
            .ok_or(GroupRoomError::UnknownParticipant)?;
        room.require_unexpired(now)?;
        if room.phase == GroupRoomPhase::Collecting {
            return Err(GroupRoomError::NotFrozen);
        }
        if room.members[index].prepared {
            return Err(GroupRoomError::AlreadyPrepared);
        }
        room.members[index].prepared = true;
        let complete = room.members.iter().all(|member| member.prepared);
        if complete {
            room.phase = GroupRoomPhase::Prepared;
            room.deadline_ns = None;
        }
        self.last_time = Some(now);
        Ok(complete)
    }

    /// A live host departure releases the entire room in admission order.
    /// A stale identity cannot close a replacement room under the same key.
    pub fn release(
        &mut self,
        id: ParticipantId,
        now: i64,
    ) -> Result<Vec<GroupParticipantTicket>, GroupRoomError> {
        self.validate_time(now)?;
        let tickets = if let Some((key, room)) = self
            .rooms
            .iter()
            .find(|(_, room)| room.members.iter().any(|member| member.id == id))
        {
            let tickets = tickets_for_room(key, room)?;
            // Ticket allocation is complete before any ownership is removed.
            self.rooms.remove(tickets[0].room.as_str());
            tickets
        } else {
            Vec::new()
        };
        self.last_time = Some(now);
        Ok(tickets)
    }

    /// Return every host from expired collecting or frozen rooms, in key order
    /// and then admission order. Equality with the original deadline expires.
    pub fn expire(&mut self, now: i64) -> Result<Vec<GroupParticipantTicket>, GroupRoomError> {
        self.validate_time(now)?;
        let tickets = self.collect_tickets(|room| room.expired(now))?;
        self.rooms.retain(|_, room| !room.expired(now));
        self.last_time = Some(now);
        Ok(tickets)
    }

    /// Permanently fence new activity and return all owned tickets. Repeated
    /// successful stop calls return no tickets; release and expiry remain safe.
    pub fn stop(&mut self, now: i64) -> Result<Vec<GroupParticipantTicket>, GroupRoomError> {
        self.validate_time(now)?;
        let tickets = self.collect_tickets(|_| true)?;
        self.rooms.clear();
        self.stopped = true;
        self.last_time = Some(now);
        Ok(tickets)
    }

    fn collect_tickets(
        &self,
        include: impl Fn(&Room) -> bool,
    ) -> Result<Vec<GroupParticipantTicket>, GroupRoomError> {
        let count = self
            .rooms
            .values()
            .filter(|room| include(room))
            .map(|room| room.members.len())
            .sum();
        let mut tickets = Vec::new();
        tickets
            .try_reserve_exact(count)
            .map_err(|_| GroupRoomError::Allocation)?;
        for (key, room) in &self.rooms {
            if include(room) {
                for member in &room.members {
                    tickets.push(GroupParticipantTicket {
                        id: member.id,
                        room: copy_key(key)?,
                    });
                }
            }
        }
        Ok(tickets)
    }
}

fn tickets_for_room(key: &str, room: &Room) -> Result<Vec<GroupParticipantTicket>, GroupRoomError> {
    let mut tickets = Vec::new();
    tickets
        .try_reserve_exact(room.members.len())
        .map_err(|_| GroupRoomError::Allocation)?;
    for member in &room.members {
        tickets.push(GroupParticipantTicket {
            id: member.id,
            room: copy_key(key)?,
        });
    }
    Ok(tickets)
}

#[cfg(test)]
#[path = "multiplayer_group_room_fixtures.rs"]
mod fixtures;
