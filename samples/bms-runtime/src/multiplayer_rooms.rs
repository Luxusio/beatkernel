//! Bounded ownership of waiting and paired streams, without transport or clock I/O.
//!
//! Tickets identify resources owned by the caller. Removing a ticket does not
//! claim that its stream has closed; the caller must close every returned ticket.
use std::{collections::BTreeMap, fmt};

/// Validated resource and waiting-time limits for one registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoomPolicy {
    max_rooms: usize,
    max_key_bytes: usize,
    waiting_ttl_ns: i64,
}

impl RoomPolicy {
    pub fn new(
        max_rooms: usize,
        max_key_bytes: usize,
        waiting_ttl_ns: i64,
    ) -> Result<Self, RoomError> {
        if !(1..=4096).contains(&max_rooms)
            || !(1..=1024).contains(&max_key_bytes)
            || waiting_ttl_ns <= 0
        {
            return Err(RoomError::InvalidPolicy);
        }
        Ok(Self {
            max_rooms,
            max_key_bytes,
            waiting_ttl_ns,
        })
    }
}

/// Nonzero when issued by a registry; never reused during that registry's lifetime.
/// This local resource identity is not an authentication credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParticipantId(pub u64);

/// Owned resource identity returned on admission, release or expiry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParticipantTicket {
    pub id: ParticipantId,
    pub room: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinOutcome {
    Waiting {
        ticket: ParticipantTicket,
        deadline_ns: i64,
    },
    Paired {
        waiting: ParticipantTicket,
        joined: ParticipantTicket,
    },
}

/// A read-only membership snapshot; a paired room has no expiry deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomSnapshot {
    Waiting {
        id: ParticipantId,
        deadline_ns: i64,
    },
    Paired {
        first: ParticipantId,
        second: ParticipantId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomError {
    InvalidPolicy,
    InvalidKey,
    InvalidTime,
    ClockRegressed { previous: i64, now: i64 },
    RoomFull,
    Capacity,
    ExpiryRequired { id: ParticipantId, deadline_ns: i64 },
    IdExhausted,
    DeadlineOverflow,
}

impl fmt::Display for RoomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy => f.write_str("invalid bounded room policy"),
            Self::InvalidKey => f.write_str("invalid bounded ASCII room key"),
            Self::InvalidTime => f.write_str("room clock must be nonnegative"),
            Self::ClockRegressed { previous, now } => {
                write!(f, "room clock regressed from {previous} to {now}")
            }
            Self::RoomFull => f.write_str("room already has two participants"),
            Self::Capacity => f.write_str("room registry is at capacity"),
            Self::ExpiryRequired { id, deadline_ns } => write!(
                f,
                "participant {} expired at {deadline_ns}; process expiry before admission",
                id.0
            ),
            Self::IdExhausted => f.write_str("participant identity space is exhausted"),
            Self::DeadlineOverflow => f.write_str("waiting deadline exceeds the room clock"),
        }
    }
}

impl std::error::Error for RoomError {}

/// Shared exact ASCII key policy for bilateral and explicit multi-host owners.
pub(crate) fn validate_room_key(key: &str, max_bytes: usize) -> Result<(), RoomError> {
    if !(1..=1024).contains(&max_bytes) {
        return Err(RoomError::InvalidPolicy);
    }
    if key.is_empty()
        || key.len() > max_bytes
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(RoomError::InvalidKey);
    }
    Ok(())
}

/// One bounded owner for bilateral stream membership.
///
/// All times are explicit caller-supplied nanoseconds. Only successful operations
/// advance the clock baseline, including successful releases of unknown IDs and
/// expiry calls that remove nothing. Errors never change membership or consume IDs.
#[derive(Debug)]
pub struct RoomRegistry {
    policy: RoomPolicy,
    rooms: BTreeMap<String, RoomSnapshot>,
    next_id: Option<u64>,
    last_time: Option<i64>,
}

impl RoomRegistry {
    pub fn new(policy: RoomPolicy) -> Self {
        Self {
            policy,
            rooms: BTreeMap::new(),
            next_id: Some(1),
            last_time: None,
        }
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    pub fn participant_count(&self) -> usize {
        self.rooms
            .values()
            .map(|room| match room {
                RoomSnapshot::Waiting { .. } => 1,
                RoomSnapshot::Paired { .. } => 2,
            })
            .sum()
    }

    pub fn room(&self, key: &str) -> Option<RoomSnapshot> {
        self.rooms.get(key).copied()
    }

    fn validate_time(&self, now: i64) -> Result<(), RoomError> {
        if now < 0 {
            return Err(RoomError::InvalidTime);
        }
        if let Some(previous) = self.last_time {
            if now < previous {
                return Err(RoomError::ClockRegressed { previous, now });
            }
        }
        Ok(())
    }

    /// Admit a waiter or pair with an unexpired waiter. An expired waiter remains
    /// owned until the caller processes `expire` or explicitly releases it.
    pub fn join(&mut self, key: &str, now: i64) -> Result<JoinOutcome, RoomError> {
        self.validate_time(now)?;
        validate_room_key(key, self.policy.max_key_bytes)?;
        let previous = self.rooms.get(key).copied();
        let deadline = match previous {
            Some(RoomSnapshot::Paired { .. }) => return Err(RoomError::RoomFull),
            Some(RoomSnapshot::Waiting { id, deadline_ns }) => {
                if now >= deadline_ns {
                    return Err(RoomError::ExpiryRequired { id, deadline_ns });
                }
                // Pairing removes the old deadline; there is no new waiting TTL.
                None
            }
            None => {
                if self.rooms.len() >= self.policy.max_rooms {
                    return Err(RoomError::Capacity);
                }
                Some(
                    now.checked_add(self.policy.waiting_ttl_ns)
                        .ok_or(RoomError::DeadlineOverflow)?,
                )
            }
        };
        let id = ParticipantId(self.next_id.ok_or(RoomError::IdExhausted)?);
        let next_id = id.0.checked_add(1);
        let ticket = ParticipantTicket {
            id,
            room: key.to_owned(),
        };
        let (room, outcome) = match previous {
            Some(RoomSnapshot::Waiting { id: first, .. }) => (
                RoomSnapshot::Paired { first, second: id },
                JoinOutcome::Paired {
                    waiting: ParticipantTicket {
                        id: first,
                        room: key.to_owned(),
                    },
                    joined: ticket,
                },
            ),
            None => {
                // The new-room branch above always computes the checked deadline.
                let deadline_ns = deadline.expect("new room has a checked deadline");
                (
                    RoomSnapshot::Waiting { id, deadline_ns },
                    JoinOutcome::Waiting {
                        ticket,
                        deadline_ns,
                    },
                )
            }
            Some(RoomSnapshot::Paired { .. }) => {
                unreachable!("full rooms rejected before admission")
            }
        };
        self.rooms.insert(key.to_owned(), room);
        self.next_id = next_id;
        self.last_time = Some(now);
        Ok(outcome)
    }

    /// Release a waiter or both paired participants in admission order. A stale
    /// ID cannot remove a replacement room that happens to use the same key.
    pub fn release(
        &mut self,
        id: ParticipantId,
        now: i64,
    ) -> Result<Vec<ParticipantTicket>, RoomError> {
        self.validate_time(now)?;
        let owner = self.rooms.iter().find(|(_, room)| match room {
            RoomSnapshot::Waiting { id: waiting, .. } => id == *waiting,
            RoomSnapshot::Paired { first, second } => id == *first || id == *second,
        });
        let tickets = match owner {
            None => Vec::new(),
            Some((key, room)) => {
                let tickets = match *room {
                    RoomSnapshot::Waiting { id, .. } => vec![ParticipantTicket {
                        id,
                        room: key.clone(),
                    }],
                    RoomSnapshot::Paired { first, second } => vec![
                        ParticipantTicket {
                            id: first,
                            room: key.clone(),
                        },
                        ParticipantTicket {
                            id: second,
                            room: key.clone(),
                        },
                    ],
                };
                let key = key.clone();
                self.rooms.remove(&key);
                tickets
            }
        };
        self.last_time = Some(now);
        Ok(tickets)
    }

    /// Remove waiters whose deadline is at or before `now`, in room-key order.
    /// Paired rooms have no deadline and remain until explicit release.
    pub fn expire(&mut self, now: i64) -> Result<Vec<ParticipantTicket>, RoomError> {
        self.validate_time(now)?;
        let tickets = self
            .rooms
            .iter()
            .filter_map(|(key, room)| match *room {
                RoomSnapshot::Waiting { id, deadline_ns } if deadline_ns <= now => {
                    Some(ParticipantTicket {
                        id,
                        room: key.clone(),
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for ticket in &tickets {
            self.rooms.remove(&ticket.room);
        }
        self.last_time = Some(now);
        Ok(tickets)
    }

    /// Initializes a pristine registry near exhaustion for independent fixtures.
    #[cfg(test)]
    pub(crate) fn with_next_id(policy: RoomPolicy, next_id: std::num::NonZeroU64) -> Self {
        Self {
            next_id: Some(next_id.get()),
            ..Self::new(policy)
        }
    }
}
