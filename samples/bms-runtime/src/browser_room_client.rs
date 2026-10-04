//! WASM access to the actual common room client, without transport or clocks.

use crate::browser_multiplayer::BrowserMultiplayerWrite;
use crate::local_players::{PlayerId, MAX_LOCAL_PLAYERS};
use crate::multiplayer_group_rooms::GroupRoomPhase;
use crate::multiplayer_protocol::WriteStep;
use crate::multiplayer_room_client::{RoomClientError, RoomClientSession};
use crate::multiplayer_room_wire::{RoomFrameDecoder, RoomWireError};
use wasm_bindgen::prelude::*;

fn error(value: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&value.to_string()).into()
}

fn field(object: &js_sys::Object, name: &str, value: JsValue) -> Result<(), JsValue> {
    if !js_sys::Reflect::set(object, &JsValue::from_str(name), &value)? {
        return Err(error("room snapshot property assignment refused"));
    }
    Ok(())
}

fn snapshot_value(session: &RoomClientSession) -> Result<JsValue, JsValue> {
    let Some(room) = session.room() else {
        return Ok(JsValue::NULL);
    };
    let snapshot = js_sys::Object::new();
    let phase = match room.phase {
        GroupRoomPhase::Collecting => 0,
        GroupRoomPhase::Frozen => 1,
        GroupRoomPhase::Prepared => 2,
    };
    field(&snapshot, "phase", JsValue::from_f64(f64::from(phase)))?;
    field(
        &snapshot,
        "deadlineNs",
        match room.deadline_ns {
            Some(deadline) => js_sys::BigInt::from(deadline).into(),
            None => JsValue::NULL,
        },
    )?;
    let members = js_sys::Array::new();
    for member in room.members {
        let row = js_sys::Object::new();
        let players = js_sys::Uint32Array::new_with_length(member.players.len() as u32);
        for (index, player) in member.players.iter().enumerate() {
            players.set_index(index as u32, player.0);
        }
        field(
            &row,
            "participant",
            js_sys::BigInt::from(member.id.0).into(),
        )?;
        field(&row, "players", players.into())?;
        field(&row, "prepared", JsValue::from_bool(member.prepared))?;
        members.push(&row);
    }
    field(&snapshot, "members", members.into())?;
    Ok(snapshot.into())
}

#[wasm_bindgen]
pub struct BrowserRoomClient {
    session: Option<RoomClientSession>,
    decoder: Option<RoomFrameDecoder>,
    failure: Option<RoomClientError>,
    revision: u64,
}

impl BrowserRoomClient {
    fn ensure_live(&self) -> Result<(), RoomClientError> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        if self.session.is_none() || self.decoder.is_none() {
            return Err(RoomClientError::InvalidState);
        }
        Ok(())
    }

    fn operate<T>(
        &mut self,
        fatal_state: bool,
        operation: impl FnOnce(&mut Self) -> Result<T, RoomClientError>,
    ) -> Result<T, JsValue> {
        self.ensure_live().map_err(error)?;
        let result = operation(self);
        if let Err(failure) = &result {
            if fatal_state || *failure != RoomClientError::InvalidState {
                self.failure = Some(*failure);
            }
        }
        result.map_err(error)
    }

    fn session(&mut self) -> Result<&mut RoomClientSession, RoomClientError> {
        self.session.as_mut().ok_or(RoomClientError::InvalidState)
    }

    fn decoder(&mut self) -> Result<&mut RoomFrameDecoder, RoomClientError> {
        self.decoder.as_mut().ok_or(RoomClientError::InvalidState)
    }
}

#[wasm_bindgen]
impl BrowserRoomClient {
    /// The caller bounds both arrays before generated WASM glue copies them.
    /// The common session validates the exact identity and positive unique roster.
    #[wasm_bindgen(constructor)]
    pub fn new(identity: Vec<u8>, players: Vec<u32>) -> Result<Self, JsValue> {
        let count = players.len();
        if !(1..=MAX_LOCAL_PLAYERS).contains(&count) {
            return Err(error("room client requires 1..64 local players"));
        }
        let mut roster = [PlayerId(0); MAX_LOCAL_PLAYERS];
        for (target, player) in roster.iter_mut().zip(players) {
            *target = PlayerId(player);
        }
        let session = RoomClientSession::new(&identity, &roster[..count]).map_err(error)?;
        Ok(Self {
            session: Some(session),
            decoder: Some(RoomFrameDecoder::new()),
            failure: None,
            revision: 0,
        })
    }

    /// Local request-state refusals leave the live owner available for valid calls.
    pub fn request_seal(&mut self) -> Result<(), JsValue> {
        self.operate(false, |owner| owner.session()?.request_seal())
    }
    pub fn request_ready(&mut self) -> Result<(), JsValue> {
        self.operate(false, |owner| owner.session()?.request_ready())
    }
    pub fn request_leave(&mut self) -> Result<(), JsValue> {
        self.operate(false, |owner| owner.session()?.request_leave())
    }

    pub fn needed_bytes(&mut self) -> Result<u32, JsValue> {
        self.operate(true, |owner| Ok(owner.decoder()?.needed()? as u32))
    }

    pub fn frame_pending(&self) -> bool {
        self.decoder
            .as_ref()
            .is_some_and(|decoder| decoder.buffered_bytes() != 0)
    }

    /// Supply only the current decoder prefix, sliced before entering WASM.
    /// Partial bytes never increment the accepted-message revision.
    pub fn receive_bytes(&mut self, bytes: Vec<u8>) -> Result<u32, JsValue> {
        self.operate(true, |owner| {
            if bytes.len() > 65_808 || bytes.len() > owner.decoder()?.needed()? {
                return Err(RoomWireError::InvalidFrame.into());
            }
            let consumed = owner.decoder()?.push(&bytes)?;
            if let Some(message) = owner.decoder()?.take()? {
                let revision = owner
                    .revision
                    .checked_add(1)
                    .ok_or(RoomClientError::IdExhausted)?;
                owner.session()?.receive(message)?;
                owner.revision = revision;
            }
            Ok(consumed as u32)
        })
    }

    /// Only waiting or a genuine frame; room admission has no application slot.
    pub fn next_write(&mut self) -> Result<BrowserMultiplayerWrite, JsValue> {
        self.operate(true, |owner| {
            Ok(match owner.session()?.poll_write()? {
                Some(frame) => WriteStep::Frame(frame).into(),
                None => WriteStep::Waiting.into(),
            })
        })
    }

    pub fn written(&mut self, id: u64) -> Result<(), JsValue> {
        self.operate(true, |owner| owner.session()?.written(id))
    }

    pub fn participant_id(&self) -> u64 {
        self.session
            .as_ref()
            .and_then(RoomClientSession::participant)
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
            .is_some_and(RoomClientSession::leave_written)
    }

    /// Exact accepted metadata. Participant IDs and deadline values stay BigInt.
    pub fn snapshot(&mut self) -> Result<JsValue, JsValue> {
        self.ensure_live().map_err(error)?;
        let result = snapshot_value(
            self.session
                .as_ref()
                .ok_or_else(|| error("room client closed"))?,
        );
        if result.is_err() {
            self.close();
        }
        result
    }

    /// Idempotently release owned state. The caller separately frees this WASM handle.
    pub fn close(&mut self) {
        self.session = None;
        self.decoder = None;
        if self.failure.is_none() {
            self.failure = Some(RoomClientError::InvalidState);
        }
    }
}
