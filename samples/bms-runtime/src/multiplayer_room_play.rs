//! Common room admission, measured clock exchange, start and progress receipts.
//! The caller owns the stream, original elapsed observations and full writes;
//! a returned software schedule is not a physical audio synchronization claim.

use crate::local_players::PlayerId;
use crate::multiplayer_group::{GroupPrefix, MemberProgress};
use crate::multiplayer_group_rooms::{GroupRoomPhase, GroupRoomSnapshot};
use crate::multiplayer_protocol::OutboundFrame;
use crate::multiplayer_room_client::{RoomClientError, RoomClientSession};
use crate::multiplayer_room_clock::{RoomClockError, RoomClockExchange};
use crate::multiplayer_room_progress_client::{RoomProgressClient, RoomProgressClientError};
use crate::multiplayer_room_wire::{encode_message, RoomMessage, RoomWireError};
use crate::multiplayer_rooms::ParticipantId;
use crate::multiplayer_start::{
    StartAgreement, StartError, StartMessage, StartPolicy, StartRole, StartSchedule,
};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomPlayError {
    InvalidState,
    Stopped,
    NegativeNow,
    TimeRegression,
    InvalidObservation,
    UnknownWrite,
    IdExhausted,
    Admission(RoomClientError),
    Clock(RoomClockError),
    Progress(RoomProgressClientError),
    Start(StartError),
    Wire(RoomWireError),
    FrameWait(crate::room_frame_wait::RoomFrameWaitError),
}

impl fmt::Display for RoomPlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState => f.write_str("invalid room play operation or response state"),
            Self::Stopped => f.write_str("room play client is stopped"),
            Self::NegativeNow => f.write_str("room play clock must be nonnegative"),
            Self::TimeRegression => f.write_str("room play clock regressed"),
            Self::InvalidObservation => f.write_str("invalid room play observation time"),
            Self::UnknownWrite => {
                f.write_str("room play receipt does not match the in-flight frame")
            }
            Self::IdExhausted => f.write_str("room play write identity space is exhausted"),
            Self::Admission(error) => write!(f, "{error}"),
            Self::Clock(error) => write!(f, "{error}"),
            Self::Progress(error) => write!(f, "{error}"),
            Self::Start(error) => write!(f, "{error}"),
            Self::Wire(error) => write!(f, "{error}"),
            Self::FrameWait(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RoomPlayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Admission(error) => Some(error),
            Self::Clock(error) => Some(error),
            Self::Progress(error) => Some(error),
            Self::Start(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::FrameWait(error) => Some(error),
            _ => None,
        }
    }
}

impl From<RoomClientError> for RoomPlayError {
    fn from(error: RoomClientError) -> Self {
        Self::Admission(error)
    }
}
impl From<RoomClockError> for RoomPlayError {
    fn from(error: RoomClockError) -> Self {
        Self::Clock(error)
    }
}
impl From<RoomProgressClientError> for RoomPlayError {
    fn from(error: RoomProgressClientError) -> Self {
        Self::Progress(error)
    }
}
impl From<StartError> for RoomPlayError {
    fn from(error: StartError) -> Self {
        Self::Start(error)
    }
}
impl From<RoomWireError> for RoomPlayError {
    fn from(error: RoomWireError) -> Self {
        Self::Wire(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Receipt {
    Admission(u64),
    Clock(u64),
    Start(StartMessage),
    Progress(u64),
}

/// One external write identity space across the actual admission, clock,
/// Join-role start and progress owners. Failed transitions retain accepted prefixes.
///
/// Clock/start candidates contain only bounded scalar state. Initial Prepared
/// adoption clones the bounded admission identity/rosters once to validate the
/// new clock owner before committing either; ordinary control polls do not
/// clone those allocations. Explicitly cloning this owner also clones them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomPlayClient {
    admission: RoomClientSession,
    admission_pending: bool,
    clock: Option<RoomClockExchange>,
    progress: Option<RoomProgressClient>,
    start: StartAgreement,
    estimate_installed: bool,
    next_id: Option<u64>,
    in_flight: Option<(u64, Receipt, i64)>,
    last_now: Option<i64>,
    last_received: Option<i64>,
    pending_commit: Option<StartMessage>,
    leaving: bool,
    stopped: bool,
}

impl RoomPlayClient {
    pub fn new(
        identity: &[u8],
        players: &[PlayerId],
        policy: StartPolicy,
        preroll_ns: i64,
    ) -> Result<Self, RoomPlayError> {
        let start = StartAgreement::new_at(StartRole::Join, policy, preroll_ns)?;
        let admission = RoomClientSession::new(identity, players)?;
        Ok(Self {
            admission,
            admission_pending: true,
            clock: None,
            progress: None,
            start,
            estimate_installed: false,
            next_id: Some(1),
            in_flight: None,
            last_now: None,
            last_received: None,
            pending_commit: None,
            leaving: false,
            stopped: false,
        })
    }

    pub fn participant(&self) -> Option<ParticipantId> {
        self.admission.participant()
    }

    pub fn room(&self) -> Option<GroupRoomSnapshot<'_>> {
        self.admission.room()
    }

    fn request_available(&self) -> Result<(), RoomPlayError> {
        if self.stopped {
            return Err(RoomPlayError::Stopped);
        }
        if self.leaving || self.admission_pending || self.in_flight.is_some() {
            return Err(RoomPlayError::InvalidState);
        }
        Ok(())
    }

    pub fn request_seal(&mut self) -> Result<(), RoomPlayError> {
        self.request_available()?;
        self.admission.request_seal()?;
        self.admission_pending = true;
        Ok(())
    }

    pub fn request_ready(&mut self) -> Result<(), RoomPlayError> {
        self.request_available()?;
        self.admission.request_ready()?;
        self.admission_pending = true;
        Ok(())
    }

    /// Queue the actual admission Leave and fence further control/progress work.
    /// An outstanding progress frame drains through its exact outer receipt
    /// without new progress credit; other outstanding control frames refuse.
    pub fn request_leave(&mut self) -> Result<(), RoomPlayError> {
        if self.stopped {
            return Err(RoomPlayError::Stopped);
        }
        if self.leaving
            || self.admission_pending
            || self
                .in_flight
                .is_some_and(|(_, receipt, _)| !matches!(receipt, Receipt::Progress(_)))
        {
            return Err(RoomPlayError::InvalidState);
        }
        self.admission.request_leave()?;
        self.admission_pending = true;
        self.leaving = true;
        if let Some(progress) = &mut self.progress {
            progress.stop();
        }
        Ok(())
    }

    /// Publish actual local member progress only after genuine start commitment.
    /// A current upload may remain in flight while an ordinary next snapshot is
    /// coalesced; no upload sequence is consumed by this operation.
    pub fn publish_progress(
        &mut self,
        members: &[MemberProgress],
        final_prefix: bool,
    ) -> Result<(), RoomPlayError> {
        if self.stopped {
            return Err(RoomPlayError::Stopped);
        }
        if self.leaving || !self.start.committed() {
            return Err(RoomPlayError::InvalidState);
        }
        self.progress
            .as_mut()
            .ok_or(RoomPlayError::InvalidState)?
            .publish(members, final_prefix)?;
        Ok(())
    }

    pub fn peer_progress(&self, participant: ParticipantId) -> Option<&GroupPrefix> {
        self.progress
            .as_ref()
            .and_then(|progress| progress.peer_progress(participant))
    }

    pub fn local_final_written(&self) -> bool {
        self.progress
            .as_ref()
            .is_some_and(RoomProgressClient::local_final_written)
    }

    pub fn local_final_acknowledged(&self) -> bool {
        self.progress
            .as_ref()
            .is_some_and(RoomProgressClient::local_final_acknowledged)
    }

    pub fn peer_final_ack_written(&self, participant: ParticipantId) -> bool {
        self.progress
            .as_ref()
            .is_some_and(|progress| progress.peer_final_ack_written(participant))
    }

    /// Local receipt completion alone does not authorize whole-room Leave.
    pub fn progress_complete(&self) -> bool {
        !self.stopped
            && !self.leaving
            && self
                .progress
                .as_ref()
                .is_some_and(RoomProgressClient::local_complete)
    }

    /// Request coordinated drain after genuine local receipt completion.
    /// Premature or repeated local requests leave this owner available; this
    /// does not queue Leave or close the caller's transport.
    pub fn request_drain(&mut self) -> Result<(), RoomPlayError> {
        if self.stopped {
            return Err(RoomPlayError::Stopped);
        }
        if self.leaving || !self.start.committed() {
            return Err(RoomPlayError::InvalidState);
        }
        self.progress
            .as_mut()
            .ok_or(RoomPlayError::InvalidState)?
            .request_drain()
            .map_err(|error| match error {
                RoomProgressClientError::InvalidDrain => RoomPlayError::InvalidState,
                error => RoomPlayError::Progress(error),
            })
    }

    /// Actual Ready full write and matching server Complete, until Stop/Leave.
    pub fn drain_complete(&self) -> bool {
        !self.stopped
            && !self.leaving
            && self.start.committed()
            && self
                .progress
                .as_ref()
                .is_some_and(RoomProgressClient::drain_complete)
    }

    // Called after the candidate's genuine start transition succeeds and before
    // its infallible adoption. A preflighted pending Commit does not call this.
    fn activate_progress(&mut self, start: &StartAgreement) -> Result<(), RoomPlayError> {
        if start.committed() && !self.start.committed() {
            self.progress
                .as_mut()
                .ok_or(RoomPlayError::InvalidState)?
                .activate()?;
        }
        Ok(())
    }

    fn validate_now(&self, now: i64) -> Result<(), RoomPlayError> {
        if self.stopped {
            return Err(RoomPlayError::Stopped);
        }
        if now < 0 {
            return Err(RoomPlayError::NegativeNow);
        }
        if self.last_now.is_some_and(|previous| now < previous) {
            return Err(RoomPlayError::TimeRegression);
        }
        Ok(())
    }

    fn start_candidate(
        &self,
        clock: &RoomClockExchange,
    ) -> Result<(StartAgreement, bool), RoomPlayError> {
        let mut start = self.start;
        let mut installed = self.estimate_installed;
        if !installed {
            if let Some(estimate) = clock.estimate() {
                start.prepare(estimate)?;
                installed = true;
            }
        }
        Ok((start, installed))
    }

    // Called only after all fallible child transitions, encoding and ID checks.
    fn adopt_frame(
        &mut self,
        id: u64,
        bytes: Vec<u8>,
        receipt: Receipt,
        now: i64,
    ) -> OutboundFrame {
        self.next_id = id.checked_add(1);
        self.in_flight = Some((id, receipt, now));
        self.last_now = Some(now);
        OutboundFrame { id, bytes }
    }

    /// Admit at most one immutable frame. Admission requests have priority,
    /// followed by clock/start work and then progress or recipient ACKs.
    pub fn poll_write(&mut self, now: i64) -> Result<Option<OutboundFrame>, RoomPlayError> {
        self.validate_now(now)?;
        if self.in_flight.is_some() {
            self.last_now = Some(now);
            return Ok(None);
        }
        if self.admission_pending {
            let id = self.next_id.ok_or(RoomPlayError::IdExhausted)?;
            let frame = self
                .admission
                .poll_write()?
                .ok_or(RoomPlayError::InvalidState)?;
            self.admission_pending = false;
            return Ok(Some(self.adopt_frame(
                id,
                frame.bytes,
                Receipt::Admission(frame.id),
                now,
            )));
        }
        if self.leaving || self.clock.is_none() {
            self.last_now = Some(now);
            return Ok(None);
        }
        let mut clock = self.clock.clone().ok_or(RoomPlayError::InvalidState)?;
        if let Some(frame) = clock.next(now)? {
            let id = self.next_id.ok_or(RoomPlayError::IdExhausted)?;
            self.clock = Some(clock);
            return Ok(Some(self.adopt_frame(
                id,
                frame.bytes,
                Receipt::Clock(frame.id),
                now,
            )));
        }
        let (mut start, installed) = self.start_candidate(&clock)?;
        let frame = if let Some(message) = start.next(now)? {
            let id = self.next_id.ok_or(RoomPlayError::IdExhausted)?;
            let bytes = encode_message(&RoomMessage::Start(message))?;
            Some((id, bytes, Receipt::Start(message)))
        } else if self
            .progress
            .as_ref()
            .is_some_and(RoomProgressClient::has_pending_write)
        {
            // Check the enclosing write identity before changing child queues.
            // The child admits atomically without cloning all peer histories.
            let id = self.next_id.ok_or(RoomPlayError::IdExhausted)?;
            let frame = self
                .progress
                .as_mut()
                .ok_or(RoomPlayError::InvalidState)?
                .poll_write(now)?
                .ok_or(RoomPlayError::InvalidState)?;
            Some((id, frame.bytes, Receipt::Progress(frame.id)))
        } else {
            None
        };
        self.clock = Some(clock);
        self.start = start;
        self.estimate_installed = installed;
        self.last_now = Some(now);
        Ok(frame.map(|(id, bytes, receipt)| self.adopt_frame(id, bytes, receipt, now)))
    }

    /// Credit only the complete frame that owns the single external write slot.
    /// Finished clock evidence is retained independently of later start-policy
    /// admission; installing an estimate never erases a real write receipt.
    pub fn written(&mut self, id: u64, now: i64) -> Result<(), RoomPlayError> {
        self.written_at(id, now, now)
    }

    /// Credit a delayed full-write observation without retimestamping it. Start
    /// freshness is still checked at the current processing time.
    pub fn written_at(
        &mut self,
        id: u64,
        completed_ns: i64,
        now: i64,
    ) -> Result<(), RoomPlayError> {
        self.validate_now(now)?;
        let (expected, receipt, admitted_ns) = self.in_flight.ok_or(RoomPlayError::UnknownWrite)?;
        if id != expected {
            return Err(RoomPlayError::UnknownWrite);
        }
        if completed_ns < admitted_ns || completed_ns > now {
            return Err(RoomPlayError::InvalidObservation);
        }
        match receipt {
            Receipt::Admission(inner) => self.admission.written(inner)?,
            Receipt::Clock(inner) => self
                .clock
                .as_mut()
                .ok_or(RoomPlayError::InvalidState)?
                .written_at(inner, completed_ns, now)?,
            Receipt::Start(message) => {
                let mut start = self.start;
                start.written(message, now)?;
                if let Some(commit) = self.pending_commit {
                    start.receive(commit, now)?;
                }
                self.activate_progress(&start)?;
                self.start = start;
                self.pending_commit = None;
            }
            Receipt::Progress(inner) => {
                // Leave stopped the child immediately, but the transport still
                // owns these bytes until this exact outer frame finishes.
                if !self.leaving {
                    self.progress
                        .as_mut()
                        .ok_or(RoomPlayError::InvalidState)?
                        .written(inner)?;
                }
            }
        }
        self.in_flight = None;
        self.last_now = Some(now);
        Ok(())
    }

    /// Receive an observation processed immediately at its capture timestamp.
    pub fn receive(&mut self, message: RoomMessage, now: i64) -> Result<(), RoomPlayError> {
        self.receive_at(message, now, now)
    }

    /// Preserve original ordered read captures independently of processing time
    /// and local write-completion callbacks. A matching early Commit is held
    /// until the exact Accept frame receives a real full-write receipt.
    pub fn receive_at(
        &mut self,
        message: RoomMessage,
        captured_ns: i64,
        now: i64,
    ) -> Result<(), RoomPlayError> {
        self.validate_now(now)?;
        if captured_ns < 0
            || captured_ns > now
            || self
                .last_received
                .is_some_and(|previous| captured_ns < previous)
        {
            return Err(RoomPlayError::InvalidObservation);
        }
        match message {
            RoomMessage::PeerProgress { .. } => {
                if self.leaving || (!self.start.committed() && self.pending_commit.is_none()) {
                    return Err(RoomPlayError::InvalidState);
                }
                self.progress
                    .as_mut()
                    .ok_or(RoomPlayError::InvalidState)?
                    .receive(&message, captured_ns)?;
            }
            RoomMessage::FinalAck { .. } | RoomMessage::DrainComplete { .. } => {
                if self.leaving || !self.start.committed() {
                    return Err(RoomPlayError::InvalidState);
                }
                self.progress
                    .as_mut()
                    .ok_or(RoomPlayError::InvalidState)?
                    .receive(&message, captured_ns)?;
            }
            RoomMessage::ClockPing { .. } | RoomMessage::ClockPong { .. } => {
                if self.leaving {
                    return Err(RoomPlayError::InvalidState);
                }
                self.clock
                    .as_mut()
                    .ok_or(RoomPlayError::InvalidState)?
                    .receive_at(&message, captured_ns, now)?;
            }
            RoomMessage::Start(message) => {
                if self.leaving {
                    return Err(RoomPlayError::InvalidState);
                }
                let clock = self.clock.as_ref().ok_or(RoomPlayError::InvalidState)?;
                let (mut start, installed) = if matches!(message, StartMessage::ClockReady(_)) {
                    (self.start, self.estimate_installed)
                } else {
                    self.start_candidate(clock)?
                };
                if let (
                    StartMessage::Commit(target),
                    Some((_, Receipt::Start(StartMessage::Accept(expected)), admitted_ns)),
                ) = (message, self.in_flight)
                {
                    if self.pending_commit.is_some() || target != expected {
                        return Err(RoomPlayError::InvalidState);
                    }
                    if captured_ns < admitted_ns {
                        return Err(RoomPlayError::InvalidObservation);
                    }
                    // Validate through the genuine transitions on a copy, but
                    // do not substitute peer response for local write evidence.
                    start.written(StartMessage::Accept(expected), now)?;
                    start.receive(message, now)?;
                    self.pending_commit = Some(message);
                } else {
                    if self.pending_commit.is_some() {
                        return Err(RoomPlayError::InvalidState);
                    }
                    start.receive(message, now)?;
                    self.activate_progress(&start)?;
                    self.start = start;
                    self.estimate_installed = installed;
                }
            }
            message => {
                let initial_prepared = !self.leaving
                    && self.clock.is_none()
                    && matches!(
                        &message,
                        RoomMessage::Snapshot {
                            phase: GroupRoomPhase::Prepared,
                            ..
                        }
                    );
                if initial_prepared {
                    let mut admission = self.admission.clone();
                    admission.receive(message)?;
                    let participant = admission.participant().ok_or(RoomPlayError::InvalidState)?;
                    let snapshot = admission.room().ok_or(RoomPlayError::InvalidState)?;
                    let clock = RoomClockExchange::new(snapshot, participant)?;
                    let progress = RoomProgressClient::new(snapshot, participant)?;
                    self.admission = admission;
                    self.clock = Some(clock);
                    self.progress = Some(progress);
                } else {
                    self.admission.receive(message)?;
                }
            }
        }
        self.last_received = Some(captured_ns);
        self.last_now = Some(now);
        Ok(())
    }

    /// Read-only genuine commitment observation; never consumes its schedule.
    pub(crate) fn start_committed(&self) -> bool {
        !self.stopped && !self.leaving && self.start.committed()
    }

    /// A matching actual Commit yields one measured local software schedule.
    pub fn take_schedule(&mut self) -> Option<StartSchedule> {
        if self.stopped || self.leaving {
            None
        } else {
            self.start.take_schedule()
        }
    }

    pub fn leave_written(&self) -> bool {
        self.admission.leave_written()
    }

    /// Permanently fence mutation and schedule extraction, retaining history.
    pub fn stop(&mut self) {
        self.stopped = true;
        if let Some(clock) = &mut self.clock {
            clock.stop();
        }
        if let Some(progress) = &mut self.progress {
            progress.stop();
        }
    }
}
