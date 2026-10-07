//! Bounded symmetric clock exchange for one actual prepared-room stream lease.
//! Original software timestamps and full-write receipts remain caller evidence;
//! this component owns no timer, transport, clock origin or playback authority.

use crate::multiplayer_clock::OffsetEstimate;
use crate::multiplayer_group_rooms::{GroupRoomPhase, GroupRoomSnapshot};
use crate::multiplayer_protocol::{
    ClockProbes, MultiplayerError, OutboundFrame, CLOCK_PROBES, MAX_IDENTITY,
};
use crate::multiplayer_room_wire::{encode_message, validate_snapshot, RoomMessage, RoomWireError};
use crate::multiplayer_rooms::ParticipantId;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomClockError {
    InvalidRoom,
    InvalidState,
    Stopped,
    TimeRegression,
    NegativeNow,
    InvalidObservation,
    UnknownWrite,
    IdExhausted,
    Wire(RoomWireError),
    Probe(MultiplayerError),
}

impl fmt::Display for RoomClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRoom => {
                f.write_str("clock exchange requires an admitted prepared-room participant")
            }
            Self::InvalidState => f.write_str("unexpected room clock message or state"),
            Self::Stopped => f.write_str("room clock exchange is stopped"),
            Self::TimeRegression => f.write_str("room clock regressed"),
            Self::NegativeNow => f.write_str("room clock must be nonnegative"),
            Self::InvalidObservation => f.write_str("invalid room clock observation time"),
            Self::UnknownWrite => {
                f.write_str("room clock receipt does not match the in-flight frame")
            }
            Self::IdExhausted => f.write_str("room clock write identity space exhausted"),
            Self::Wire(error) => write!(f, "{error}"),
            Self::Probe(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RoomClockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wire(error) => Some(error),
            Self::Probe(error) => Some(error),
            _ => None,
        }
    }
}

impl From<RoomWireError> for RoomClockError {
    fn from(error: RoomWireError) -> Self {
        Self::Wire(error)
    }
}
impl From<MultiplayerError> for RoomClockError {
    fn from(error: MultiplayerError) -> Self {
        Self::Probe(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameKind {
    Ping,
    Pong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Control {
    probes: ClockProbes,
    next_id: Option<u64>,
    in_flight: Option<(u64, FrameKind, i64)>,
    ping_written: u64,
    pong_written: u64,
    last_now: Option<i64>,
    last_received: Option<i64>,
    stopped: bool,
}

/// Reuses the exact shared eight-probe algorithm. Rejected operations preserve
/// all samples, pending timestamps, write IDs and the successful clock baseline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomClockExchange {
    participant: ParticipantId,
    control: Control,
}

impl RoomClockExchange {
    pub fn new(
        snapshot: GroupRoomSnapshot<'_>,
        participant: ParticipantId,
    ) -> Result<Self, RoomClockError> {
        if snapshot.phase != GroupRoomPhase::Prepared
            || snapshot.identity.is_empty()
            || snapshot.identity.len() > MAX_IDENTITY
        {
            return Err(RoomClockError::InvalidRoom);
        }
        validate_snapshot(snapshot.members, snapshot.phase, snapshot.deadline_ns)
            .map_err(|_| RoomClockError::InvalidRoom)?;
        if !snapshot
            .members
            .iter()
            .any(|member| member.id == participant)
        {
            return Err(RoomClockError::InvalidRoom);
        }
        Ok(Self {
            participant,
            control: Control {
                probes: ClockProbes::default(),
                next_id: Some(1),
                in_flight: None,
                ping_written: 0,
                pong_written: 0,
                last_now: None,
                last_received: None,
                stopped: false,
            },
        })
    }

    pub fn participant(&self) -> ParticipantId {
        self.participant
    }

    fn candidate(&self, now: i64) -> Result<Control, RoomClockError> {
        if self.control.stopped {
            return Err(RoomClockError::Stopped);
        }
        if now < 0 {
            return Err(RoomClockError::NegativeNow);
        }
        if self.control.last_now.is_some_and(|previous| now < previous) {
            return Err(RoomClockError::TimeRegression);
        }
        let mut candidate = self.control;
        candidate.last_now = Some(now);
        Ok(candidate)
    }

    /// Admit one complete BKMR frame, giving a pending reply priority. Neither
    /// admitting the frame nor an early peer reply credits its actual write.
    pub fn next(&mut self, now: i64) -> Result<Option<OutboundFrame>, RoomClockError> {
        let mut candidate = self.candidate(now)?;
        if candidate.in_flight.is_some() {
            self.control = candidate;
            return Ok(None);
        }
        let (message, kind) = if let Some((sequence, sent_ns, received_ns, replied_ns)) =
            candidate.probes.next_pong_fields(now)?
        {
            (
                RoomMessage::ClockPong {
                    sequence: sequence + 1,
                    sent_ns,
                    received_ns,
                    replied_ns,
                },
                FrameKind::Pong,
            )
        } else if let Some((sequence, sent_ns)) = candidate.probes.next_ping_fields(now)? {
            (
                RoomMessage::ClockPing {
                    sequence: sequence + 1,
                    sent_ns,
                },
                FrameKind::Ping,
            )
        } else {
            self.control = candidate;
            return Ok(None);
        };
        // Shared probe sequences are bounded 0..7. Write IDs instead retain
        // the full nonreused u64 range, including its final value exactly once.
        let id = candidate.next_id.ok_or(RoomClockError::IdExhausted)?;
        let next_id = id.checked_add(1);
        let bytes = encode_message(&message)?;
        candidate.next_id = next_id;
        candidate.in_flight = Some((id, kind, now));
        self.control = candidate;
        Ok(Some(OutboundFrame { id, bytes }))
    }

    /// A transport may credit only the exact frame after its final byte writes.
    pub fn written(&mut self, id: u64, now: i64) -> Result<(), RoomClockError> {
        self.written_at(id, now, now)
    }

    /// A delayed completion notification retains its original observation;
    /// processing time advances independently of read/write capture order.
    pub fn written_at(
        &mut self,
        id: u64,
        completed_ns: i64,
        now: i64,
    ) -> Result<(), RoomClockError> {
        let mut candidate = self.candidate(now)?;
        let (expected, kind, admitted_ns) =
            candidate.in_flight.ok_or(RoomClockError::UnknownWrite)?;
        if id != expected {
            return Err(RoomClockError::UnknownWrite);
        }
        if completed_ns < admitted_ns || completed_ns > now {
            return Err(RoomClockError::InvalidObservation);
        }
        match kind {
            FrameKind::Ping => candidate.ping_written += 1,
            FrameKind::Pong => candidate.pong_written += 1,
        }
        candidate.in_flight = None;
        self.control = candidate;
        Ok(())
    }

    /// Retain actual receipt time even when the corresponding local Ping write
    /// is still in flight. For queued observations use receive_at instead.
    pub fn receive(&mut self, message: &RoomMessage, now: i64) -> Result<(), RoomClockError> {
        self.receive_at(message, now, now)
    }

    /// Keep original t1/t3 values while admitting against current processing
    /// time. One stream's reads remain ordered independently of write callbacks.
    pub fn receive_at(
        &mut self,
        message: &RoomMessage,
        captured_ns: i64,
        now: i64,
    ) -> Result<(), RoomClockError> {
        let mut candidate = self.candidate(now)?;
        if captured_ns < 0
            || captured_ns > now
            || candidate
                .last_received
                .is_some_and(|previous| captured_ns < previous)
        {
            return Err(RoomClockError::InvalidObservation);
        }
        match message {
            RoomMessage::ClockPing { sequence, sent_ns } => {
                let sequence = sequence
                    .checked_sub(1)
                    .ok_or(RoomWireError::InvalidMessage)?;
                candidate
                    .probes
                    .receive_ping_fields(sequence, *sent_ns, captured_ns)?;
            }
            RoomMessage::ClockPong {
                sequence,
                sent_ns,
                received_ns,
                replied_ns,
            } => {
                let sequence = sequence
                    .checked_sub(1)
                    .ok_or(RoomWireError::InvalidMessage)?;
                candidate.probes.receive_pong_fields(
                    sequence,
                    *sent_ns,
                    *received_ns,
                    *replied_ns,
                    captured_ns,
                )?;
            }
            _ => return Err(RoomClockError::InvalidState),
        }
        candidate.last_received = Some(captured_ns);
        self.control = candidate;
        Ok(())
    }

    /// Expose the actual selected estimate only after all symmetric samples and
    /// both sets of eight complete writes, with no pending or in-flight work.
    pub fn estimate(&self) -> Option<OffsetEstimate> {
        let state = &self.control;
        if state.stopped
            || state.probes.completed != CLOCK_PROBES
            || state.ping_written != CLOCK_PROBES
            || state.pong_written != CLOCK_PROBES
            || state.in_flight.is_some()
            || state.probes.pending_ping.is_some()
            || state.probes.pending_pong.is_some()
        {
            return None;
        }
        state.probes.filter.estimate()
    }

    /// Permanently refuse future operations and withhold readiness evidence.
    pub fn stop(&mut self) {
        self.control.stopped = true;
    }
}
