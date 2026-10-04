//! Transport-independent BKMR admission and a bounded caller-driven I/O owner.
//! Room preparation is not a shared playback start or application final ACK.

use crate::local_players::PlayerId;
use crate::multiplayer_group::validate_roster;
use crate::multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase, GroupRoomSnapshot};
use crate::multiplayer_protocol::{MAX_IDENTITY, OutboundFrame};
use crate::multiplayer_room_wire::{
    RoomFrameDecoder, RoomMessage, RoomWireError, encode_message, validate_snapshot,
};
use crate::multiplayer_rooms::ParticipantId;
use std::{
    fmt,
    io::{self, Read, Write},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomClientError {
    InvalidState,
    InvalidSnapshot,
    UnknownWrite,
    IdExhausted,
    Allocation,
    Wire(RoomWireError),
}
impl fmt::Display for RoomClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState => f.write_str("invalid room client operation or response state"),
            Self::InvalidSnapshot => {
                f.write_str("room snapshot changed accepted identity or chronology")
            }
            Self::UnknownWrite => {
                f.write_str("room write receipt does not match the in-flight frame")
            }
            Self::IdExhausted => f.write_str("room write identity space is exhausted"),
            Self::Allocation => f.write_str("room client allocation failed"),
            Self::Wire(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for RoomClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wire(error) => Some(error),
            _ => None,
        }
    }
}
impl From<RoomWireError> for RoomClientError {
    fn from(error: RoomWireError) -> Self {
        Self::Wire(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    Join,
    Seal,
    Ready,
    Leave,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AcceptedRoom {
    members: Vec<GroupRoomMember>,
    phase: GroupRoomPhase,
    deadline_ns: Option<i64>,
}

fn copy_slice<T: Copy>(values: &[T]) -> Result<Vec<T>, RoomClientError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(values.len())
        .map_err(|_| RoomClientError::Allocation)?;
    copy.extend_from_slice(values);
    Ok(copy)
}

/// One local admission identity, one queued or in-flight request, and the latest
/// accepted snapshot. Rejected calls preserve accepted state and write IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomClientSession {
    identity: Vec<u8>,
    players: Vec<PlayerId>,
    participant: Option<ParticipantId>,
    room: Option<AcceptedRoom>,
    queued: Option<Request>,
    in_flight: Option<(u64, Request)>,
    next_id: Option<u64>,
    join_written: bool,
    seal_written: bool,
    ready_written: bool,
    leave_requested: bool,
    leave_written: bool,
}

impl RoomClientSession {
    pub fn new(identity: &[u8], players: &[PlayerId]) -> Result<Self, RoomClientError> {
        if identity.is_empty() || identity.len() > MAX_IDENTITY {
            return Err(RoomWireError::InvalidMessage.into());
        }
        validate_roster(players)
            .map_err(|_| RoomClientError::Wire(RoomWireError::InvalidMessage))?;
        Ok(Self {
            identity: copy_slice(identity)?,
            players: copy_slice(players)?,
            participant: None,
            room: None,
            queued: Some(Request::Join),
            in_flight: None,
            next_id: Some(1),
            join_written: false,
            seal_written: false,
            ready_written: false,
            leave_requested: false,
            leave_written: false,
        })
    }

    pub fn participant(&self) -> Option<ParticipantId> {
        self.participant
    }

    pub fn room(&self) -> Option<GroupRoomSnapshot<'_>> {
        self.room.as_ref().map(|room| GroupRoomSnapshot {
            identity: &self.identity,
            members: &room.members,
            phase: room.phase,
            deadline_ns: room.deadline_ns,
        })
    }

    pub fn leave_written(&self) -> bool {
        self.leave_written
    }

    fn request_available(&self) -> Result<(), RoomClientError> {
        if self.queued.is_some() || self.in_flight.is_some() || self.leave_requested {
            return Err(RoomClientError::InvalidState);
        }
        Ok(())
    }

    /// Only the observed first participant can seal a collecting roster.
    pub fn request_seal(&mut self) -> Result<(), RoomClientError> {
        self.request_available()?;
        let participant = self.participant.ok_or(RoomClientError::InvalidState)?;
        let room = self.room.as_ref().ok_or(RoomClientError::InvalidState)?;
        if self.seal_written
            || room.phase != GroupRoomPhase::Collecting
            || room.members.len() < 2
            || !room
                .members
                .first()
                .is_some_and(|member| member.id == participant)
        {
            return Err(RoomClientError::InvalidState);
        }
        self.queued = Some(Request::Seal);
        Ok(())
    }

    /// The caller explicitly declares its local preparation; a later full write
    /// receipt is required before a snapshot can claim this host is prepared.
    pub fn request_ready(&mut self) -> Result<(), RoomClientError> {
        self.request_available()?;
        let participant = self.participant.ok_or(RoomClientError::InvalidState)?;
        let room = self.room.as_ref().ok_or(RoomClientError::InvalidState)?;
        if self.ready_written
            || room.phase != GroupRoomPhase::Frozen
            || !room
                .members
                .iter()
                .any(|member| member.id == participant && !member.prepared)
        {
            return Err(RoomClientError::InvalidState);
        }
        self.queued = Some(Request::Ready);
        Ok(())
    }

    pub fn request_leave(&mut self) -> Result<(), RoomClientError> {
        self.request_available()?;
        if self.participant.is_none() {
            return Err(RoomClientError::InvalidState);
        }
        self.queued = Some(Request::Leave);
        self.leave_requested = true;
        Ok(())
    }

    /// Transfer one immutable frame to its transport owner. Polling while that
    /// frame is outstanding yields None; it never resends or credits the frame.
    pub fn poll_write(&mut self) -> Result<Option<OutboundFrame>, RoomClientError> {
        if self.in_flight.is_some() {
            return Ok(None);
        }
        let Some(request) = self.queued else {
            return Ok(None);
        };
        let id = self.next_id.ok_or(RoomClientError::IdExhausted)?;
        let next_id = id.checked_add(1);
        let message = match request {
            Request::Join => RoomMessage::Join {
                identity: copy_slice(&self.identity)?,
                players: copy_slice(&self.players)?,
            },
            Request::Seal => RoomMessage::Seal,
            Request::Ready => RoomMessage::Ready,
            Request::Leave => RoomMessage::Leave,
        };
        let bytes = encode_message(&message)?;
        self.queued = None;
        self.in_flight = Some((id, request));
        self.next_id = next_id;
        Ok(Some(OutboundFrame { id, bytes }))
    }

    pub fn written(&mut self, id: u64) -> Result<(), RoomClientError> {
        let (expected, request) = self.in_flight.ok_or(RoomClientError::UnknownWrite)?;
        if id != expected {
            return Err(RoomClientError::UnknownWrite);
        }
        match request {
            Request::Join => self.join_written = true,
            Request::Seal => self.seal_written = true,
            Request::Ready => self.ready_written = true,
            Request::Leave => self.leave_written = true,
        }
        self.in_flight = None;
        Ok(())
    }

    /// Validate a complete response before replacing any accepted observation.
    pub fn receive(&mut self, message: RoomMessage) -> Result<(), RoomClientError> {
        match message {
            RoomMessage::Admitted { participant } => {
                if !self.join_written || self.participant.is_some() || participant.0 == 0 {
                    return Err(RoomClientError::InvalidState);
                }
                self.participant = Some(participant);
            }
            RoomMessage::Snapshot {
                members,
                phase,
                deadline_ns,
            } => {
                let participant = self.participant.ok_or(RoomClientError::InvalidState)?;
                validate_snapshot(&members, phase, deadline_ns)
                    .map_err(|_| RoomClientError::InvalidSnapshot)?;
                let own = members
                    .iter()
                    .find(|member| member.id == participant)
                    .ok_or(RoomClientError::InvalidSnapshot)?;
                if own.players != self.players || (own.prepared && !self.ready_written) {
                    return Err(RoomClientError::InvalidSnapshot);
                }
                if phase != GroupRoomPhase::Collecting
                    && members
                        .first()
                        .is_some_and(|member| member.id == participant)
                    && !self.seal_written
                {
                    return Err(RoomClientError::InvalidSnapshot);
                }
                if let Some(previous) = &self.room {
                    let append_only = previous.phase == GroupRoomPhase::Collecting
                        && phase == GroupRoomPhase::Collecting;
                    let phase_valid = match previous.phase {
                        GroupRoomPhase::Collecting => {
                            matches!(phase, GroupRoomPhase::Collecting | GroupRoomPhase::Frozen)
                        }
                        GroupRoomPhase::Frozen => {
                            matches!(phase, GroupRoomPhase::Frozen | GroupRoomPhase::Prepared)
                        }
                        GroupRoomPhase::Prepared => phase == GroupRoomPhase::Prepared,
                    };
                    if !phase_valid
                        || members.len() < previous.members.len()
                        || (!append_only && members.len() != previous.members.len())
                        || (phase != GroupRoomPhase::Prepared
                            && deadline_ns != previous.deadline_ns)
                    {
                        return Err(RoomClientError::InvalidSnapshot);
                    }
                    for (before, after) in previous.members.iter().zip(&members) {
                        if before.id != after.id
                            || before.players != after.players
                            || (before.prepared && !after.prepared)
                        {
                            return Err(RoomClientError::InvalidSnapshot);
                        }
                    }
                } else if phase != GroupRoomPhase::Collecting {
                    return Err(RoomClientError::InvalidSnapshot);
                }
                self.room = Some(AcceptedRoom {
                    members,
                    phase,
                    deadline_ns,
                });
            }
            _ => return Err(RoomClientError::InvalidState),
        }
        Ok(())
    }
}

fn protocol_error(error: impl fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

/// At most one write call and one bounded prefix read per step. Callers provide
/// nonblocking/deadline-bounded streams and own cancellation and final disposal.
pub struct RoomClientIo<S: Read + Write> {
    session: RoomClientSession,
    stream: S,
    pending: Option<OutboundFrame>,
    offset: usize,
    decoder: RoomFrameDecoder,
    scratch: [u8; 4096],
    failure: Option<(io::ErrorKind, String)>,
}

impl<S: Read + Write> RoomClientIo<S> {
    pub fn new(session: RoomClientSession, stream: S) -> Self {
        Self {
            session,
            stream,
            pending: None,
            offset: 0,
            decoder: RoomFrameDecoder::new(),
            scratch: [0; 4096],
            failure: None,
        }
    }

    pub fn session(&self) -> &RoomClientSession {
        &self.session
    }

    pub fn into_stream(self) -> S {
        self.stream
    }

    fn ensure_live(&self) -> io::Result<()> {
        match &self.failure {
            Some((kind, text)) => Err(io::Error::new(*kind, text.clone())),
            None => Ok(()),
        }
    }

    pub fn request_seal(&mut self) -> io::Result<()> {
        self.ensure_live()?;
        self.session
            .request_seal()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    }
    pub fn request_ready(&mut self) -> io::Result<()> {
        self.ensure_live()?;
        self.session
            .request_ready()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    }
    pub fn request_leave(&mut self) -> io::Result<()> {
        self.ensure_live()?;
        self.session
            .request_leave()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    }

    /// True means actual bytes moved. WouldBlock and Interrupted retain the
    /// current prefix for the next step without an internal retry loop. Other
    /// stream/protocol failures permanently fence this driver.
    pub fn step(&mut self) -> io::Result<bool> {
        self.ensure_live()?;
        let result = self.step_live();
        if let Err(error) = &result {
            self.failure = Some((error.kind(), error.to_string()));
        }
        result
    }

    fn step_live(&mut self) -> io::Result<bool> {
        let mut progressed = false;
        if self.pending.is_none() {
            self.pending = self.session.poll_write().map_err(protocol_error)?;
        }
        if let Some(frame) = &self.pending {
            let remaining = frame
                .bytes
                .get(self.offset..)
                .ok_or_else(|| protocol_error("room write offset exceeded frame"))?;
            match self.stream.write(remaining) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "room frame write made no progress",
                    ));
                }
                Ok(count) => {
                    if count > remaining.len() {
                        return Err(protocol_error("room writer exceeded its supplied slice"));
                    }
                    self.offset += count;
                    progressed = true;
                    if self.offset == frame.bytes.len() {
                        self.session.written(frame.id).map_err(protocol_error)?;
                        self.pending = None;
                        self.offset = 0;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        let needed = self.decoder.needed().map_err(protocol_error)?;
        let limit = needed.min(self.scratch.len());
        match self.stream.read(&mut self.scratch[..limit]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "room stream ended",
                ));
            }
            Ok(count) => {
                if count > limit {
                    return Err(protocol_error("room reader exceeded its supplied slice"));
                }
                let admitted = self
                    .decoder
                    .push(&self.scratch[..count])
                    .map_err(protocol_error)?;
                if admitted != count {
                    return Err(protocol_error(
                        "room decoder did not admit the requested prefix",
                    ));
                }
                progressed = true;
                if let Some(message) = self.decoder.take().map_err(protocol_error)? {
                    self.session.receive(message).map_err(protocol_error)?;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
        Ok(progressed)
    }
}

#[cfg(test)]
#[path = "multiplayer_room_client_fixtures.rs"]
mod fixtures;
