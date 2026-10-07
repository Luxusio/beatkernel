//! Portable split-operation driver over the existing room protocol owners.
use crate::{
    local_players::PlayerId,
    multiplayer_group::{decode_words, GroupPrefix},
    multiplayer_protocol::OutboundFrame,
    multiplayer_room_client::RoomClientError,
    multiplayer_room_play::{RoomPlayClient, RoomPlayError},
    multiplayer_room_progress_client::RoomProgressClientError,
    multiplayer_room_wire::{RoomFrameDecoder, RoomMessage, RoomWireError},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::{StartPolicy, StartSchedule},
};

pub struct RoomClientDriver {
    session: Option<RoomPlayClient>,
    decoder: Option<RoomFrameDecoder>,
    failure: Option<RoomPlayError>,
    revision: u64,
    retained_snapshot: crate::room_network_model::RoomSnapshot,
    pending_peer: Option<ParticipantId>,
    publication: crate::competition_progress_cadence::ProgressCadence,
    final_admitted: bool,
    drain_admitted: bool,
    leave_requested: bool,
    drain_wait: Option<crate::room_final_wait::RoomFinalWaitState>,
    start_wait: Option<crate::room_start_wait::RoomStartWaitState>,
    pending_start: Option<StartSchedule>,
    setup_wait: Option<crate::room_setup_wait::RoomSetupWaitState>,
    frame_wait: Option<crate::room_frame_wait::RoomFrameWaitState>,
}

impl RoomClientDriver {
    fn ensure_live(&self) -> Result<(), RoomPlayError> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        if self.session.is_none() || self.decoder.is_none() {
            return Err(RoomPlayError::InvalidState);
        }
        Ok(())
    }

    fn operate<T>(
        &mut self,
        fatal_state: bool,
        operation: impl FnOnce(&mut Self) -> Result<T, RoomPlayError>,
    ) -> Result<T, RoomPlayError> {
        self.ensure_live()?;
        let result = operation(self);
        if let Err(failure) = &result {
            if fatal_state
                || !matches!(
                    failure,
                    RoomPlayError::InvalidState
                        | RoomPlayError::Admission(RoomClientError::InvalidState)
                        | RoomPlayError::Progress(RoomProgressClientError::InvalidState)
                )
            {
                self.failure = Some(failure.clone());
            }
        }
        result
    }

    fn session(&mut self) -> Result<&mut RoomPlayClient, RoomPlayError> {
        self.session.as_mut().ok_or(RoomPlayError::InvalidState)
    }

    fn decoder(&mut self) -> Result<&mut RoomFrameDecoder, RoomPlayError> {
        self.decoder.as_mut().ok_or(RoomPlayError::InvalidState)
    }
}
impl RoomClientDriver {
    pub fn new(
        identity: &[u8],
        players: &[PlayerId],
        policy: StartPolicy,
        preroll_ns: i64,
    ) -> Result<Self, RoomPlayError> {
        let session = RoomPlayClient::new(identity, players, policy, preroll_ns)?;
        Ok(Self {
            session: Some(session),
            decoder: Some(RoomFrameDecoder::new()),
            failure: None,
            revision: 0,
            retained_snapshot: crate::room_network_model::RoomSnapshot::default(),
            pending_peer: None,
            publication: crate::competition_progress_cadence::ProgressCadence::new(),
            final_admitted: false,
            drain_admitted: false,
            leave_requested: false,
            drain_wait: None,
            start_wait: Some(crate::room_start_wait::RoomStartWaitState::new()),
            pending_start: None,
            setup_wait: None,
            frame_wait: None,
        })
    }
    pub fn request_seal(&mut self) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| owner.session()?.request_seal())
    }
    pub fn request_ready(&mut self) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| owner.session()?.request_ready())
    }
    pub fn request_leave(&mut self) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| {
            owner.session()?.request_leave()?;
            owner.leave_requested = true;
            owner.frame_wait = None;
            owner.pending_start = None;
            Ok(())
        })
    }
    pub fn request_drain(&mut self) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| {
            owner.session()?.request_drain()?;
            owner.drain_admitted = true;
            Ok(())
        })
    }
    pub fn publish_progress_words(
        &mut self,
        words: &[u32],
        final_prefix: bool,
    ) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| {
            let members = decode_words(words)
                .map_err(|_| RoomPlayError::Progress(RoomProgressClientError::InvalidProgress))?;
            owner.session()?.publish_progress(&members, final_prefix)?;
            if final_prefix {
                owner.final_admitted = true;
            }
            Ok(())
        })
    }
    /// Scalar timing hint only; actual publication always validates protocol state.
    pub fn publication_due(
        &mut self,
        now_ns: i64,
        final_prefix: bool,
    ) -> Result<bool, RoomPlayError> {
        self.operate(false, |owner| {
            if owner.leave_requested {
                return Err(RoomPlayError::InvalidState);
            }
            let now = u64::try_from(now_ns).map_err(|_| {
                RoomPlayError::Progress(RoomProgressClientError::InvalidObservation)
            })?;
            if !owner.publication.observe(now) {
                return Err(RoomPlayError::Progress(
                    RoomProgressClientError::InvalidObservation,
                ));
            }
            Ok(owner.publication.due(final_prefix))
        })
    }
    pub fn publish_progress_words_at(
        &mut self,
        words: &[u32],
        final_prefix: bool,
        now_ns: i64,
    ) -> Result<bool, RoomPlayError> {
        self.operate(false, |owner| {
            let members = decode_words(words)
                .map_err(|_| RoomPlayError::Progress(RoomProgressClientError::InvalidProgress))?;
            owner.session()?.preflight_progress(&members)?;
            let now = u64::try_from(now_ns).map_err(|_| {
                RoomPlayError::Progress(RoomProgressClientError::InvalidObservation)
            })?;
            if !owner.publication.observe(now) {
                return Err(RoomPlayError::Progress(
                    RoomProgressClientError::InvalidObservation,
                ));
            }
            if !owner.publication.due(final_prefix) {
                return Ok(false);
            }
            owner.session()?.publish_progress(&members, final_prefix)?;
            if final_prefix {
                owner.final_admitted = true;
            }
            owner.publication.admitted();
            Ok(true)
        })
    }
    pub fn needed_bytes(&mut self) -> Result<usize, RoomPlayError> {
        self.operate(true, |owner| Ok(owner.decoder()?.needed()?))
    }
    pub fn frame_pending(&self) -> bool {
        self.decoder
            .as_ref()
            .is_some_and(|decoder| decoder.buffered_bytes() != 0)
    }

    /// Supply only the current decoder prefix, sliced before entering WASM.
    /// Only complete admitted/snapshot messages increment the metadata revision.
    pub fn receive_bytes(
        &mut self,
        bytes: &[u8],
        captured_ns: i64,
        now_ns: i64,
    ) -> Result<usize, RoomPlayError> {
        self.ensure_live()?;
        if self.leave_requested {
            return Err(RoomPlayError::InvalidState);
        }
        self.operate(true, |owner| {
            if captured_ns < 0 || captured_ns > now_ns {
                return Err(RoomPlayError::InvalidObservation);
            }
            if bytes.len() > 65_808 || bytes.len() > owner.decoder()?.needed()? {
                return Err(RoomWireError::InvalidFrame.into());
            }
            let pending = owner.frame_pending();
            owner.observe_frame(now_ns, pending)?;
            let consumed = owner.decoder()?.push(bytes)?;
            let message = owner.decoder()?.take()?;
            owner.observe_frame(now_ns, owner.frame_pending())?;
            if let Some(message) = message {
                let peer = match &message {
                    RoomMessage::PeerProgress { participant, .. } => Some(*participant),
                    _ => None,
                };
                if peer.is_some() && owner.pending_peer.is_some() {
                    return Err(RoomPlayError::InvalidState);
                }
                let revision = if matches!(
                    &message,
                    RoomMessage::Admitted { .. } | RoomMessage::Snapshot { .. }
                ) {
                    owner
                        .revision
                        .checked_add(1)
                        .ok_or(RoomPlayError::IdExhausted)?
                } else {
                    owner.revision
                };
                owner.session()?.receive_at(message, captured_ns, now_ns)?;
                owner.revision = revision;
                if peer.is_some() {
                    owner.pending_peer = peer;
                }
            }
            Ok(consumed)
        })
    }

    /// Only waiting or a genuine frame; room controls have no application slot.
    pub fn next_write(&mut self, now_ns: i64) -> Result<Option<OutboundFrame>, RoomPlayError> {
        self.operate(true, |owner| owner.session()?.poll_write(now_ns))
    }

    pub fn written(
        &mut self,
        id: u64,
        completed_ns: i64,
        now_ns: i64,
    ) -> Result<(), RoomPlayError> {
        self.operate(true, |owner| {
            owner.session()?.written_at(id, completed_ns, now_ns)
        })
    }

    pub fn pending_peer(&self) -> Result<Option<ParticipantId>, RoomPlayError> {
        self.ensure_live()?;
        Ok(self.pending_peer)
    }
    pub fn peer_progress(&self, participant: ParticipantId) -> Option<&GroupPrefix> {
        self.session
            .as_ref()
            .and_then(|session| session.peer_progress(participant))
    }
    pub fn consume_peer_progress(&mut self) {
        self.pending_peer = None;
    }
    pub fn session_ref(&self) -> Result<&RoomPlayClient, RoomPlayError> {
        self.ensure_live()?;
        self.session.as_ref().ok_or(RoomPlayError::InvalidState)
    }
    pub fn failed(&self) -> bool {
        self.failure.is_some()
    }
    pub fn local_final_written(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(RoomPlayClient::local_final_written)
    }

    pub fn local_final_acknowledged(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(RoomPlayClient::local_final_acknowledged)
    }

    pub fn peer_final_ack_written(&self, participant: ParticipantId) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.peer_final_ack_written(participant))
    }

    pub fn progress_complete(&self) -> bool {
        self.failure.is_none()
            && self
                .session
                .as_ref()
                .is_some_and(RoomPlayClient::progress_complete)
    }

    pub fn drain_complete(&self) -> bool {
        self.failure.is_none()
            && self
                .session
                .as_ref()
                .is_some_and(RoomPlayClient::drain_complete)
    }

    pub fn take_start(&mut self) -> Result<Option<StartSchedule>, RoomPlayError> {
        self.operate(true, |owner| {
            let mut state = owner.start_wait.take().ok_or(RoomPlayError::InvalidState)?;
            let result = state.step(&mut DriverStartPort(owner), &mut || {
                Ok::<bool, std::convert::Infallible>(true)
            });
            owner.start_wait = Some(state);
            match result {
                Ok(crate::room_start_wait::RoomStartStep::Ready) => {
                    let schedule = owner.pending_start.take();
                    if let Some(schedule) = schedule {
                        owner.retained_snapshot.schedule = Some(schedule);
                    }
                    Ok(schedule)
                }
                Ok(crate::room_start_wait::RoomStartStep::Pending(_))
                | Ok(crate::room_start_wait::RoomStartStep::Cancelled) => Ok(None),
                Err(crate::room_start_wait::RoomStartStepError::Port(error)) => Err(error),
                Err(crate::room_start_wait::RoomStartStepError::Service(error)) => match error {},
                Err(crate::room_start_wait::RoomStartStepError::Closing)
                | Err(crate::room_start_wait::RoomStartStepError::LeavePending)
                | Err(crate::room_start_wait::RoomStartStepError::Terminal) => {
                    Err(RoomPlayError::InvalidState)
                }
            }
        })
    }
    pub fn participant_id(&self) -> u64 {
        self.session
            .as_ref()
            .and_then(RoomPlayClient::participant)
            .map_or(0, |participant| participant.0)
    }

    pub fn retained_snapshot(
        &mut self,
    ) -> Result<&crate::room_network_model::RoomSnapshot, RoomPlayError> {
        self.operate(true, |owner| {
            let session = owner.session.as_ref().ok_or(RoomPlayError::InvalidState)?;
            crate::room_snapshot_projection::refresh(&mut owner.retained_snapshot, session)
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::InvalidInput {
                        RoomPlayError::IdExhausted
                    } else {
                        RoomPlayError::Progress(RoomProgressClientError::Allocation)
                    }
                })?;
            Ok(())
        })?;
        Ok(&self.retained_snapshot)
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn has_snapshot(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.room().is_some())
    }

    pub fn leave_written(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(RoomPlayClient::leave_written)
    }

    /// Idempotently release owned state. The caller separately frees this WASM handle.
    pub fn close(&mut self) {
        if let Some(session) = &mut self.session {
            session.stop();
        }
        self.session = None;
        self.decoder = None;
        self.pending_peer = None;
        self.drain_wait = None;
        self.start_wait = None;
        self.setup_wait = None;
        self.frame_wait = None;
        self.pending_start = None;
        if self.failure.is_none() {
            self.failure = Some(RoomPlayError::Stopped);
        }
    }
}

#[derive(Debug)]
pub enum RoomClientSetupError {
    Policy(crate::room_setup_wait::RoomSetupError),
    Protocol(RoomPlayError),
    InvalidState,
}
impl std::fmt::Display for RoomClientSetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Policy(error) => write!(f, "room setup: {error:?}"),
            Self::Protocol(error) => std::fmt::Display::fmt(error, f),
            Self::InvalidState => f.write_str("room setup wait is unavailable"),
        }
    }
}
impl RoomClientDriver {
    pub fn begin_setup(&mut self, now: i64, timeout: u64) -> Result<(), RoomClientSetupError> {
        self.ensure_live().map_err(RoomClientSetupError::Protocol)?;
        if self.setup_wait.is_some() || self.leave_requested {
            return Err(RoomClientSetupError::InvalidState);
        }
        let state = crate::room_setup_wait::RoomSetupWaitState::new(now, timeout)
            .map_err(RoomClientSetupError::Policy)?;
        self.setup_wait = Some(state);
        Ok(())
    }
    pub fn setup_step(
        &mut self,
        now: i64,
    ) -> Result<crate::room_setup_wait::RoomSetupStep, RoomClientSetupError> {
        self.ensure_live().map_err(RoomClientSetupError::Protocol)?;
        if self.leave_requested {
            return Err(RoomClientSetupError::InvalidState);
        }
        let session = self
            .session
            .as_ref()
            .ok_or(RoomClientSetupError::InvalidState)?;
        let admitted = session.participant().is_some() && session.room().is_some();
        let prepared = session.room().is_some_and(|room| {
            room.phase == crate::multiplayer_group_rooms::GroupRoomPhase::Prepared
        });
        let committed = session.start_committed();
        self.setup_wait
            .as_mut()
            .ok_or(RoomClientSetupError::InvalidState)?
            .step(now, admitted, prepared, committed)
            .map_err(RoomClientSetupError::Policy)
    }
}

impl RoomClientDriver {
    pub fn configure_frame_wait(&mut self, timeout_ns: u64) -> Result<(), RoomPlayError> {
        self.ensure_live()?;
        if self.leave_requested || self.frame_wait.is_some() || self.frame_pending() {
            return Err(RoomPlayError::InvalidState);
        }
        let state = crate::room_frame_wait::RoomFrameWaitState::new(timeout_ns)
            .map_err(RoomPlayError::FrameWait)?;
        self.frame_wait = Some(state);
        Ok(())
    }
    fn observe_frame(
        &mut self,
        now_ns: i64,
        pending: bool,
    ) -> Result<crate::room_frame_wait::RoomFrameWaitStep, RoomPlayError> {
        match &mut self.frame_wait {
            Some(state) => state
                .observe(now_ns, pending)
                .map_err(RoomPlayError::FrameWait),
            None => Ok(crate::room_frame_wait::RoomFrameWaitStep::Idle),
        }
    }
    pub fn frame_wait_step(
        &mut self,
        now_ns: i64,
    ) -> Result<crate::room_frame_wait::RoomFrameWaitStep, RoomPlayError> {
        self.operate(true, |owner| {
            if owner.leave_requested {
                return Ok(crate::room_frame_wait::RoomFrameWaitStep::Idle);
            }
            if owner.frame_wait.is_none() {
                return Err(RoomPlayError::InvalidState);
            }
            owner.observe_frame(now_ns, owner.frame_pending())
        })
    }
}

struct DriverStartPort<'a>(&'a mut RoomClientDriver);
impl crate::room_start_wait::RoomStartPort for DriverStartPort<'_> {
    type Error = RoomPlayError;
    fn initial(&mut self) -> crate::room_start_wait::RoomStartInitial {
        crate::room_start_wait::RoomStartInitial {
            cancelled: self.0.leave_requested,
            closing: false,
        }
    }
    fn poll(&mut self) -> crate::room_start_wait::RoomStartObservation<Self::Error> {
        let cancelled = self.0.leave_requested;
        let failure = if cancelled {
            None
        } else {
            (|| -> Result<(), RoomPlayError> {
                self.0.ensure_live()?;
                if let Some(schedule) = self.0.session()?.take_schedule() {
                    self.0.pending_start = Some(schedule);
                }
                Ok(())
            })()
            .err()
        };
        crate::room_start_wait::RoomStartObservation {
            cancelled,
            failure,
            leaving: false,
            terminal: false,
            committed: self.0.pending_start.is_some(),
        }
    }
}

#[derive(Debug)]
pub enum RoomDrainError {
    Protocol(RoomPlayError),
    TimedOut,
    InvalidTerminal,
    ClockRegressed,
    InvalidClock,
    InvalidState,
}
impl std::fmt::Display for RoomDrainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Protocol(error) => std::fmt::Display::fmt(error, f),
            Self::TimedOut => f.write_str("room coordinated drain timed out"),
            Self::InvalidTerminal => f.write_str("room ended without successful coordinated drain"),
            Self::ClockRegressed => f.write_str("room drain clock regressed"),
            Self::InvalidClock => f.write_str("invalid room drain clock"),
            Self::InvalidState => f.write_str("room drain request is unavailable"),
        }
    }
}

impl RoomClientDriver {
    pub fn begin_drain(&mut self, now_ns: i64, timeout_ns: u64) -> Result<(), RoomDrainError> {
        if now_ns < 0 {
            return Err(RoomDrainError::InvalidClock);
        }
        if !(1_000_000..=120_000_000_000).contains(&timeout_ns) {
            return Err(RoomDrainError::InvalidState);
        }
        let network_deadline = now_ns
            .checked_add(timeout_ns as i64)
            .ok_or(RoomDrainError::InvalidClock)?;
        let control_deadline = (now_ns as u64)
            .checked_add(timeout_ns)
            .ok_or(RoomDrainError::InvalidClock)?;
        self.ensure_live().map_err(RoomDrainError::Protocol)?;
        if self.drain_wait.is_some() || self.leave_requested || self.leave_written() {
            return Err(RoomDrainError::InvalidState);
        }
        self.drain_wait = Some(crate::room_final_wait::RoomFinalWaitState::new(
            network_deadline,
            control_deadline,
        ));
        Ok(())
    }
    pub fn drain_requested(&self) -> bool {
        self.drain_admitted
    }
    pub fn drain_step(
        &mut self,
        now_ns: i64,
    ) -> Result<crate::room_final_wait::RoomFinalStep, RoomDrainError> {
        let mut state = self.drain_wait.take().ok_or(RoomDrainError::InvalidState)?;
        let result = state.step(
            &mut DriverDrainPort {
                driver: self,
                now_ns,
            },
            &mut DriverDrainControl { now_ns },
        );
        self.drain_wait = Some(state);
        result.map_err(|error| match error {
            crate::room_final_wait::RoomFinalWaitError::Port(error) => {
                RoomDrainError::Protocol(error)
            }
            crate::room_final_wait::RoomFinalWaitError::Control(error) => error,
            crate::room_final_wait::RoomFinalWaitError::TimedOut => RoomDrainError::TimedOut,
            crate::room_final_wait::RoomFinalWaitError::InvalidTerminal => {
                RoomDrainError::InvalidTerminal
            }
            crate::room_final_wait::RoomFinalWaitError::ClockRegressed => {
                RoomDrainError::ClockRegressed
            }
            crate::room_final_wait::RoomFinalWaitError::InvalidClock => {
                RoomDrainError::InvalidClock
            }
        })
    }
}

struct DriverDrainPort<'a> {
    driver: &'a mut RoomClientDriver,
    now_ns: i64,
}
impl crate::room_final_wait::RoomFinalPort for DriverDrainPort<'_> {
    type Error = RoomPlayError;
    fn poll(&mut self) -> Result<crate::room_final_wait::RoomFinalObservation, Self::Error> {
        use crate::room_final_wait::{RoomFinalObservation, RoomFinalTerminal, RoomFinalReceipts};
        self.driver.ensure_live()?;
        let cancelled = self.driver.leave_requested || self.driver.leave_written();
        let complete = self.driver.drain_complete();
        Ok(RoomFinalObservation {
            progress_pending: !self.driver.final_admitted,
            final_accepted: self.driver.final_admitted,
            drain_accepted: self.driver.drain_admitted,
            terminal: if cancelled || complete {
                Some(RoomFinalTerminal {
                    cancelled,
                    failed: false,
                    receipts: RoomFinalReceipts {
                        local_final_written: self.driver.local_final_written(),
                        local_final_acknowledged: self.driver.local_final_acknowledged(),
                        progress_complete: self.driver.progress_complete(),
                        drain_complete: complete,
                    },
                })
            } else {
                None
            },
        })
    }
    fn clock_now_ns(&mut self) -> Result<i64, Self::Error> {
        Ok(self.now_ns)
    }
    fn queue_final(&mut self) -> Result<crate::room_final_wait::RoomFinalAdmission, Self::Error> {
        Ok(if self.driver.final_admitted {
            crate::room_final_wait::RoomFinalAdmission::Accepted
        } else {
            crate::room_final_wait::RoomFinalAdmission::QueueFull
        })
    }
    fn queue_drain(&mut self) -> Result<crate::room_final_wait::RoomFinalAdmission, Self::Error> {
        use crate::room_final_wait::RoomFinalAdmission;
        if self.driver.drain_admitted {
            return Ok(RoomFinalAdmission::Accepted);
        }
        if !self.driver.progress_complete() {
            return Ok(RoomFinalAdmission::QueueFull);
        }
        self.driver.request_drain()?;
        Ok(RoomFinalAdmission::Accepted)
    }
}
struct DriverDrainControl {
    now_ns: i64,
}
impl crate::final_ack_wait::FinalWaitControl for DriverDrainControl {
    type Error = RoomDrainError;
    fn now_ns(&mut self) -> Result<u64, Self::Error> {
        u64::try_from(self.now_ns).map_err(|_| RoomDrainError::InvalidClock)
    }
    fn park_ns(&mut self, _duration_ns: u64) -> Result<(), Self::Error> {
        Err(RoomDrainError::InvalidState)
    }
}

#[cfg(test)]
#[path = "room_client_driver_fixtures.rs"]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "room_driver_publication_fixtures.rs"]
mod room_driver_publication_fixtures;

#[cfg(test)]
#[path = "room_driver_snapshot_fixtures.rs"]
mod room_driver_snapshot_fixtures;
