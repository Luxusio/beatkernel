//! WASM access to the common room start client, without transport or clock acquisition.

use crate::browser_multiplayer::BrowserMultiplayerWrite;
use crate::local_players::{PlayerId, MAX_LOCAL_PLAYERS};
use crate::multiplayer_group_rooms::GroupRoomPhase;
use crate::multiplayer_protocol::WriteStep;
use crate::multiplayer_room_client::RoomClientError;
use crate::multiplayer_room_play::{RoomPlayClient, RoomPlayError};
use crate::multiplayer_room_wire::{RoomFrameDecoder, RoomMessage, RoomWireError};
use crate::multiplayer_start::StartPolicy;
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

fn snapshot_value(session: &RoomPlayClient) -> Result<JsValue, JsValue> {
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
    session: Option<RoomPlayClient>,
    decoder: Option<RoomFrameDecoder>,
    failure: Option<RoomPlayError>,
    revision: u64,
}

impl BrowserRoomClient {
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
    ) -> Result<T, JsValue> {
        self.ensure_live().map_err(error)?;
        let result = operation(self);
        if let Err(failure) = &result {
            if fatal_state
                || !matches!(
                    failure,
                    RoomPlayError::InvalidState
                        | RoomPlayError::Admission(RoomClientError::InvalidState)
                )
            {
                self.failure = Some(failure.clone());
            }
        }
        result.map_err(error)
    }

    fn session(&mut self) -> Result<&mut RoomPlayClient, RoomPlayError> {
        self.session.as_mut().ok_or(RoomPlayError::InvalidState)
    }

    fn decoder(&mut self) -> Result<&mut RoomFrameDecoder, RoomPlayError> {
        self.decoder.as_mut().ok_or(RoomPlayError::InvalidState)
    }
}

#[wasm_bindgen]
impl BrowserRoomClient {
    /// The caller bounds both arrays before generated WASM glue copies them.
    /// The common session validates the exact identity and positive unique roster.
    #[wasm_bindgen(constructor)]
    pub fn new(identity: Vec<u8>, players: Vec<u32>) -> Result<Self, JsValue> {
        Self::new_with_start(identity, players, 0)
    }

    /// Supplies the actual local audio preroll to the common measured start owner.
    pub fn new_with_start(
        identity: Vec<u8>,
        players: Vec<u32>,
        preroll_ns: i64,
    ) -> Result<Self, JsValue> {
        let count = players.len();
        if !(1..=MAX_LOCAL_PLAYERS).contains(&count) {
            return Err(error("room client requires 1..64 local players"));
        }
        let mut roster = [PlayerId(0); MAX_LOCAL_PLAYERS];
        for (target, player) in roster.iter_mut().zip(players) {
            *target = PlayerId(player);
        }
        let session = RoomPlayClient::new(
            &identity,
            &roster[..count],
            StartPolicy::default(),
            preroll_ns,
        )
        .map_err(error)?;
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
    /// Only complete admitted/snapshot messages increment the metadata revision.
    pub fn receive_bytes(
        &mut self,
        bytes: Vec<u8>,
        captured_ns: i64,
        now_ns: i64,
    ) -> Result<u32, JsValue> {
        self.operate(true, |owner| {
            if captured_ns < 0 || captured_ns > now_ns {
                return Err(RoomPlayError::InvalidObservation);
            }
            if bytes.len() > 65_808 || bytes.len() > owner.decoder()?.needed()? {
                return Err(RoomWireError::InvalidFrame.into());
            }
            let consumed = owner.decoder()?.push(&bytes)?;
            if let Some(message) = owner.decoder()?.take()? {
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
            }
            Ok(consumed as u32)
        })
    }

    /// Only waiting or a genuine frame; room controls have no application slot.
    pub fn next_write(&mut self, now_ns: i64) -> Result<BrowserMultiplayerWrite, JsValue> {
        self.operate(true, |owner| {
            Ok(match owner.session()?.poll_write(now_ns)? {
                Some(frame) => WriteStep::Frame(frame).into(),
                None => WriteStep::Waiting.into(),
            })
        })
    }

    pub fn written(&mut self, id: u64, completed_ns: i64, now_ns: i64) -> Result<(), JsValue> {
        self.operate(true, |owner| {
            owner.session()?.written_at(id, completed_ns, now_ns)
        })
    }

    /// Consumes only a genuine committed common schedule. All values retain
    /// their exact integer domains across the JavaScript boundary.
    pub fn take_start(&mut self) -> Result<JsValue, JsValue> {
        let schedule = self.operate(true, |owner| Ok(owner.session()?.take_schedule()))?;
        let Some(schedule) = schedule else {
            return Ok(JsValue::NULL);
        };
        let result = (|| {
            let value = js_sys::Object::new();
            field(
                &value,
                "targetNs",
                js_sys::BigInt::from(schedule.target_ns).into(),
            )?;
            field(
                &value,
                "songTargetNs",
                js_sys::BigInt::from(schedule.song_target_ns).into(),
            )?;
            field(
                &value,
                "uncertaintyNs",
                js_sys::BigInt::from(schedule.uncertainty_ns).into(),
            )?;
            Ok(value.into())
        })();
        if result.is_err() {
            self.close();
        }
        result
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
        if let Some(session) = &mut self.session {
            session.stop();
        }
        self.session = None;
        self.decoder = None;
        if self.failure.is_none() {
            self.failure = Some(RoomPlayError::Stopped);
        }
    }
}
