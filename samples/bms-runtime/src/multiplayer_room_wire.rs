//! Distinct BKMR v2 room-admission and control frames, without authorization.
//! Stream owners assign participant IDs; requests never choose an identity.
//! Frame validation does not establish a start, progress, or a final ACK.

use crate::local_players::{PlayerId, MAX_LOCAL_PLAYERS};
use crate::multiplayer_group::validate_roster;
use crate::multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase};
use crate::multiplayer_protocol::MAX_IDENTITY;
use crate::multiplayer_rooms::ParticipantId;
use crate::multiplayer_start::StartMessage;
use std::fmt;

const MAGIC: &[u8; 4] = b"BKMR";
const VERSION: u16 = 2;
const HEADER_BYTES: usize = 11;
const MAX_HOSTS: usize = 64;
const MAX_PAYLOAD: usize = 4 + MAX_IDENTITY + 1 + 4 * MAX_LOCAL_PLAYERS;
const MAX_FRAME_BYTES: usize = HEADER_BYTES + MAX_PAYLOAD;
const MAX_SNAPSHOT_PAYLOAD: usize = 10 + MAX_HOSTS * (10 + 4 * MAX_LOCAL_PLAYERS);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomMessage {
    Join {
        identity: Vec<u8>,
        players: Vec<PlayerId>,
    },
    Admitted {
        participant: ParticipantId,
    },
    Snapshot {
        members: Vec<GroupRoomMember>,
        phase: GroupRoomPhase,
        deadline_ns: Option<i64>,
    },
    Seal,
    Ready,
    Leave,
    ClockPing {
        sequence: u64,
        sent_ns: i64,
    },
    ClockPong {
        sequence: u64,
        sent_ns: i64,
        received_ns: i64,
        replied_ns: i64,
    },
    Start(StartMessage),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomWireError {
    InvalidFrame,
    InvalidMessage,
    Allocation,
}

impl fmt::Display for RoomWireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidFrame => "invalid bounded BKMR v2 frame",
            Self::InvalidMessage => "invalid room admission or control message",
            Self::Allocation => "room message allocation failed",
        })
    }
}
impl std::error::Error for RoomWireError {}

pub(crate) fn validate_snapshot(
    members: &[GroupRoomMember],
    phase: GroupRoomPhase,
    deadline_ns: Option<i64>,
) -> Result<usize, RoomWireError> {
    if members.is_empty() || members.len() > MAX_HOSTS {
        return Err(RoomWireError::InvalidMessage);
    }
    match phase {
        GroupRoomPhase::Collecting => {
            if !deadline_ns.is_some_and(|deadline| deadline >= 0)
                || members.iter().any(|member| member.prepared)
            {
                return Err(RoomWireError::InvalidMessage);
            }
        }
        GroupRoomPhase::Frozen => {
            if members.len() < 2
                || !deadline_ns.is_some_and(|deadline| deadline >= 0)
                || members.iter().all(|member| member.prepared)
            {
                return Err(RoomWireError::InvalidMessage);
            }
        }
        GroupRoomPhase::Prepared => {
            if members.len() < 2
                || deadline_ns.is_some()
                || members.iter().any(|member| !member.prepared)
            {
                return Err(RoomWireError::InvalidMessage);
            }
        }
    }
    let mut length = 10;
    for (index, member) in members.iter().enumerate() {
        if member.id.0 == 0
            || members
                .iter()
                .take(index)
                .any(|prior| prior.id == member.id)
        {
            return Err(RoomWireError::InvalidMessage);
        }
        validate_roster(&member.players).map_err(|_| RoomWireError::InvalidMessage)?;
        // Both host and local counts have been bounded before this arithmetic.
        length += 10 + 4 * member.players.len();
    }
    Ok(length)
}

fn start_fields(message: StartMessage) -> (u8, i64) {
    match message {
        StartMessage::ClockReady(time) => (9, time),
        StartMessage::Propose(time) => (10, time),
        StartMessage::Accept(time) => (11, time),
        StartMessage::Commit(time) => (12, time),
    }
}

fn message_extent(message: &RoomMessage) -> Result<(u8, usize), RoomWireError> {
    Ok(match message {
        RoomMessage::Join { identity, players } => {
            if identity.is_empty() || identity.len() > MAX_IDENTITY {
                return Err(RoomWireError::InvalidMessage);
            }
            validate_roster(players).map_err(|_| RoomWireError::InvalidMessage)?;
            (1, 4 + identity.len() + 1 + 4 * players.len())
        }
        RoomMessage::Admitted { participant } => {
            if participant.0 == 0 {
                return Err(RoomWireError::InvalidMessage);
            }
            (2, 8)
        }
        RoomMessage::Snapshot {
            members,
            phase,
            deadline_ns,
        } => (3, validate_snapshot(members, *phase, *deadline_ns)?),
        RoomMessage::Seal => (4, 0),
        RoomMessage::Ready => (5, 0),
        RoomMessage::Leave => (6, 0),
        RoomMessage::ClockPing { sequence, sent_ns } => {
            if *sequence == 0 || *sent_ns < 0 {
                return Err(RoomWireError::InvalidMessage);
            }
            (7, 16)
        }
        RoomMessage::ClockPong {
            sequence,
            sent_ns,
            received_ns,
            replied_ns,
        } => {
            // Sent is on the probing host's clock; only the responder's two
            // timestamps can be ordered here. Probe ownership checks the echo.
            if *sequence == 0 || *sent_ns < 0 || *received_ns < 0 || *replied_ns < *received_ns {
                return Err(RoomWireError::InvalidMessage);
            }
            (8, 32)
        }
        RoomMessage::Start(message) => {
            let (tag, time) = start_fields(*message);
            if time < 0 {
                return Err(RoomWireError::InvalidMessage);
            }
            (tag, 8)
        }
    })
}

fn append_players(frame: &mut Vec<u8>, players: &[PlayerId]) {
    frame.push(players.len() as u8);
    for player in players {
        frame.extend_from_slice(&player.0.to_le_bytes());
    }
}

/// Validate the whole message before allocating its one exact bounded frame.
pub fn encode_message(message: &RoomMessage) -> Result<Vec<u8>, RoomWireError> {
    let (tag, length) = message_extent(message)?;
    let mut frame = Vec::new();
    frame
        .try_reserve_exact(HEADER_BYTES + length)
        .map_err(|_| RoomWireError::Allocation)?;
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&VERSION.to_le_bytes());
    frame.push(tag);
    frame.extend_from_slice(&(length as u32).to_le_bytes());
    match message {
        RoomMessage::Join { identity, players } => {
            frame.extend_from_slice(&(identity.len() as u32).to_le_bytes());
            frame.extend_from_slice(identity);
            append_players(&mut frame, players);
        }
        RoomMessage::Admitted { participant } => {
            frame.extend_from_slice(&participant.0.to_le_bytes())
        }
        RoomMessage::Snapshot {
            members,
            phase,
            deadline_ns,
        } => {
            frame.push(match phase {
                GroupRoomPhase::Collecting => 0,
                GroupRoomPhase::Frozen => 1,
                GroupRoomPhase::Prepared => 2,
            });
            frame.extend_from_slice(&deadline_ns.unwrap_or(-1).to_le_bytes());
            frame.push(members.len() as u8);
            for member in members {
                frame.extend_from_slice(&member.id.0.to_le_bytes());
                frame.push(u8::from(member.prepared));
                append_players(&mut frame, &member.players);
            }
        }
        RoomMessage::Seal | RoomMessage::Ready | RoomMessage::Leave => {}
        RoomMessage::ClockPing { sequence, sent_ns } => {
            frame.extend_from_slice(&sequence.to_le_bytes());
            frame.extend_from_slice(&sent_ns.to_le_bytes());
        }
        RoomMessage::ClockPong {
            sequence,
            sent_ns,
            received_ns,
            replied_ns,
        } => {
            frame.extend_from_slice(&sequence.to_le_bytes());
            frame.extend_from_slice(&sent_ns.to_le_bytes());
            frame.extend_from_slice(&received_ns.to_le_bytes());
            frame.extend_from_slice(&replied_ns.to_le_bytes());
        }
        RoomMessage::Start(message) => {
            frame.extend_from_slice(&start_fields(*message).1.to_le_bytes())
        }
    }
    Ok(frame)
}

fn read_bytes<'a>(input: &mut &'a [u8], count: usize) -> Result<&'a [u8], RoomWireError> {
    let current = *input;
    let bytes = current.get(..count).ok_or(RoomWireError::InvalidMessage)?;
    *input = current.get(count..).ok_or(RoomWireError::InvalidMessage)?;
    Ok(bytes)
}

fn read_array<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], RoomWireError> {
    read_bytes(input, N)?
        .try_into()
        .map_err(|_| RoomWireError::InvalidMessage)
}

fn read_header(frame: &[u8]) -> Result<(u8, usize), RoomWireError> {
    let mut header = frame
        .get(..HEADER_BYTES)
        .ok_or(RoomWireError::InvalidFrame)?;
    let magic = read_array::<4>(&mut header).map_err(|_| RoomWireError::InvalidFrame)?;
    let version =
        u16::from_le_bytes(read_array(&mut header).map_err(|_| RoomWireError::InvalidFrame)?);
    let tag = read_array::<1>(&mut header).map_err(|_| RoomWireError::InvalidFrame)?[0];
    let length =
        u32::from_le_bytes(read_array(&mut header).map_err(|_| RoomWireError::InvalidFrame)?)
            as usize;
    if &magic != MAGIC || version != VERSION {
        return Err(RoomWireError::InvalidFrame);
    }
    let valid_length = match tag {
        1 => (10..=MAX_PAYLOAD).contains(&length),
        2 => length == 8,
        3 => (24..=MAX_SNAPSHOT_PAYLOAD).contains(&length),
        4..=6 => length == 0,
        7 => length == 16,
        8 => length == 32,
        9..=12 => length == 8,
        _ => false,
    };
    if !valid_length {
        return Err(RoomWireError::InvalidFrame);
    }
    Ok((tag, length))
}

fn read_players(payload: &mut &[u8]) -> Result<Vec<PlayerId>, RoomWireError> {
    let count = usize::from(read_array::<1>(payload)?[0]);
    if count == 0 || count > MAX_LOCAL_PLAYERS || payload.len() < count * 4 {
        return Err(RoomWireError::InvalidMessage);
    }
    let mut players = Vec::new();
    players
        .try_reserve_exact(count)
        .map_err(|_| RoomWireError::Allocation)?;
    for _ in 0..count {
        players.push(PlayerId(u32::from_le_bytes(read_array(payload)?)));
    }
    validate_roster(&players).map_err(|_| RoomWireError::InvalidMessage)?;
    Ok(players)
}

/// Decode exactly one complete frame. Trailing bytes belong to another frame
/// only when the caller explicitly separates them with RoomFrameDecoder.
pub fn decode_message(frame: &[u8]) -> Result<RoomMessage, RoomWireError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(RoomWireError::InvalidFrame);
    }
    let (tag, length) = read_header(frame)?;
    if frame.len() != HEADER_BYTES + length {
        return Err(RoomWireError::InvalidFrame);
    }
    let mut payload = frame
        .get(HEADER_BYTES..)
        .ok_or(RoomWireError::InvalidFrame)?;
    let message = match tag {
        1 => {
            let length = u32::from_le_bytes(read_array(&mut payload)?) as usize;
            if length == 0 || length > MAX_IDENTITY {
                return Err(RoomWireError::InvalidMessage);
            }
            let identity_bytes = read_bytes(&mut payload, length)?;
            let players = read_players(&mut payload)?;
            let mut identity = Vec::new();
            identity
                .try_reserve_exact(length)
                .map_err(|_| RoomWireError::Allocation)?;
            identity.extend_from_slice(identity_bytes);
            RoomMessage::Join { identity, players }
        }
        2 => {
            let participant = ParticipantId(u64::from_le_bytes(read_array(&mut payload)?));
            if participant.0 == 0 {
                return Err(RoomWireError::InvalidMessage);
            }
            RoomMessage::Admitted { participant }
        }
        3 => {
            let phase = match read_array::<1>(&mut payload)?[0] {
                0 => GroupRoomPhase::Collecting,
                1 => GroupRoomPhase::Frozen,
                2 => GroupRoomPhase::Prepared,
                _ => return Err(RoomWireError::InvalidMessage),
            };
            let deadline_ns = match i64::from_le_bytes(read_array(&mut payload)?) {
                -1 => None,
                value if value >= 0 => Some(value),
                _ => return Err(RoomWireError::InvalidMessage),
            };
            let count = usize::from(read_array::<1>(&mut payload)?[0]);
            if count == 0 || count > MAX_HOSTS || payload.len() < count * 14 {
                return Err(RoomWireError::InvalidMessage);
            }
            let mut members = Vec::new();
            members
                .try_reserve_exact(count)
                .map_err(|_| RoomWireError::Allocation)?;
            for _ in 0..count {
                let id = ParticipantId(u64::from_le_bytes(read_array(&mut payload)?));
                let prepared = match read_array::<1>(&mut payload)?[0] {
                    0 => false,
                    1 => true,
                    _ => return Err(RoomWireError::InvalidMessage),
                };
                let players = read_players(&mut payload)?;
                members.push(GroupRoomMember {
                    id,
                    players,
                    prepared,
                });
            }
            validate_snapshot(&members, phase, deadline_ns)?;
            RoomMessage::Snapshot {
                members,
                phase,
                deadline_ns,
            }
        }
        4 => RoomMessage::Seal,
        5 => RoomMessage::Ready,
        6 => RoomMessage::Leave,
        7 => RoomMessage::ClockPing {
            sequence: u64::from_le_bytes(read_array(&mut payload)?),
            sent_ns: i64::from_le_bytes(read_array(&mut payload)?),
        },
        8 => RoomMessage::ClockPong {
            sequence: u64::from_le_bytes(read_array(&mut payload)?),
            sent_ns: i64::from_le_bytes(read_array(&mut payload)?),
            received_ns: i64::from_le_bytes(read_array(&mut payload)?),
            replied_ns: i64::from_le_bytes(read_array(&mut payload)?),
        },
        9..=12 => {
            let time = i64::from_le_bytes(read_array(&mut payload)?);
            RoomMessage::Start(match tag {
                9 => StartMessage::ClockReady(time),
                10 => StartMessage::Propose(time),
                11 => StartMessage::Accept(time),
                12 => StartMessage::Commit(time),
                _ => return Err(RoomWireError::InvalidFrame),
            })
        }
        _ => return Err(RoomWireError::InvalidFrame),
    };
    if !payload.is_empty() {
        return Err(RoomWireError::InvalidMessage);
    }
    if tag >= 7 {
        message_extent(&message)?;
    }
    Ok(message)
}

/// One bounded frame at a time. A failed header or message remains held;
/// callers must terminate its stream rather than retrying after a silent reset.
#[derive(Debug, Default)]
pub struct RoomFrameDecoder {
    bytes: Vec<u8>,
}

impl RoomFrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(all(target_arch = "wasm32", feature = "browser"))]
    pub(crate) fn buffered_bytes(&self) -> usize {
        self.bytes.len()
    }

    /// First request only the header; its validated tag/extent bounds the body.
    pub fn needed(&self) -> Result<usize, RoomWireError> {
        if self.bytes.len() < HEADER_BYTES {
            return Ok(HEADER_BYTES - self.bytes.len());
        }
        let (_, length) = read_header(&self.bytes)?;
        (HEADER_BYTES + length)
            .checked_sub(self.bytes.len())
            .ok_or(RoomWireError::InvalidFrame)
    }

    /// Consume at most the current needed prefix. A complete held frame consumes
    /// zero; the caller takes it before admitting any coalesced remainder.
    pub fn push(&mut self, chunk: &[u8]) -> Result<usize, RoomWireError> {
        let needed = self.needed()?;
        let count = needed.min(chunk.len());
        if count == 0 {
            return Ok(0);
        }
        // Reserve the validated extent once, rather than reallocating for
        // each small transport fragment. Before validation this is only
        // the 11-byte header; afterwards it is the bounded body remainder.
        self.bytes
            .try_reserve_exact(needed)
            .map_err(|_| RoomWireError::Allocation)?;
        self.bytes.extend_from_slice(&chunk[..count]);
        self.needed()?;
        Ok(count)
    }

    /// Return a fully validated message and clear only on success. Incomplete
    /// frames return None; failed semantic decoding retains the exact bytes.
    pub fn take(&mut self) -> Result<Option<RoomMessage>, RoomWireError> {
        if self.needed()? != 0 {
            return Ok(None);
        }
        let message = decode_message(&self.bytes)?;
        self.bytes.clear();
        Ok(Some(message))
    }
}

#[cfg(test)]
#[path = "multiplayer_room_wire_fixtures.rs"]
mod fixtures;
