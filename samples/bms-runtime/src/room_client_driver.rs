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
    pending_peer: Option<ParticipantId>,
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
            pending_peer: None,
        })
    }
    pub fn request_seal(&mut self) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| owner.session()?.request_seal())
    }
    pub fn request_ready(&mut self) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| owner.session()?.request_ready())
    }
    pub fn request_leave(&mut self) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| owner.session()?.request_leave())
    }
    pub fn request_drain(&mut self) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| owner.session()?.request_drain())
    }
    pub fn publish_progress_words(
        &mut self,
        words: &[u32],
        final_prefix: bool,
    ) -> Result<(), RoomPlayError> {
        self.operate(false, |owner| {
            let members = decode_words(words)
                .map_err(|_| RoomPlayError::Progress(RoomProgressClientError::InvalidProgress))?;
            owner.session()?.publish_progress(&members, final_prefix)
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
        self.operate(true, |owner| {
            if captured_ns < 0 || captured_ns > now_ns {
                return Err(RoomPlayError::InvalidObservation);
            }
            if bytes.len() > 65_808 || bytes.len() > owner.decoder()?.needed()? {
                return Err(RoomWireError::InvalidFrame.into());
            }
            let consumed = owner.decoder()?.push(bytes)?;
            if let Some(message) = owner.decoder()?.take()? {
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
        self.operate(true, |owner| Ok(owner.session()?.take_schedule()))
    }
    pub fn participant_id(&self) -> u64 {
        self.session
            .as_ref()
            .and_then(RoomPlayClient::participant)
            .map_or(0, |participant| participant.0)
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
        if self.failure.is_none() {
            self.failure = Some(RoomPlayError::Stopped);
        }
    }
}

#[cfg(test)]
#[path = "room_client_driver_fixtures.rs"]
mod fixtures;
