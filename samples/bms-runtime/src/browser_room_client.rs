//! WASM value conversion for the portable split-operation room driver.
use crate::{
    browser_multiplayer::BrowserMultiplayerWrite,
    local_players::{PlayerId, MAX_LOCAL_PLAYERS},
    multiplayer_group::encode_words,
    multiplayer_group_rooms::GroupRoomPhase,
    multiplayer_protocol::WriteStep,
    multiplayer_room_play::{RoomPlayClient, RoomPlayError},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::StartPolicy,
    room_client_driver::{RoomClientDriver, RoomDrainError, RoomClientSetupError},
};
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
    driver: RoomClientDriver,
}
impl BrowserRoomClient {
    fn drain_error(&mut self, value: RoomDrainError) -> JsValue {
        let code = match &value {
            RoomDrainError::TimedOut => "timeout",
            RoomDrainError::InvalidState => "state",
            RoomDrainError::Protocol(failure)
                if !self.driver.failed()
                    && matches!(failure,
            crate::multiplayer_room_play::RoomPlayError::InvalidState
            | crate::multiplayer_room_play::RoomPlayError::Admission(
                crate::multiplayer_room_client::RoomClientError::InvalidState)
            | crate::multiplayer_room_play::RoomPlayError::Progress(
                crate::multiplayer_room_progress_client::RoomProgressClientError::InvalidState)
        ) =>
            {
                "state"
            }
            _ => "protocol",
        };
        let result = error(value);
        match js_sys::Reflect::set(
            &result,
            &JsValue::from_str("code"),
            &JsValue::from_str(code),
        ) {
            Ok(true) => result,
            Ok(false) => {
                self.close();
                error("room error property assignment refused")
            }
            Err(failure) => {
                self.close();
                failure
            }
        }
    }

    fn setup_error(&mut self, value: RoomClientSetupError) -> JsValue {
        let (code, operation) = match &value {
            RoomClientSetupError::Policy(crate::room_setup_wait::RoomSetupError::Deadline(
                phase,
                crate::room_setup_wait::RoomDeadlineError::Expired,
            )) => (
                "timeout",
                if *phase == crate::room_setup_wait::RoomSetupPhase::Prepared {
                    "prepared"
                } else {
                    "setup"
                },
            ),
            RoomClientSetupError::InvalidState => ("state", "setup"),
            _ => ("protocol", "setup"),
        };
        let result = error(value);
        let assigned = (|| -> Result<(), JsValue> {
            field(
                &js_sys::Object::from(result.clone()),
                "code",
                JsValue::from_str(code),
            )?;
            field(
                &js_sys::Object::from(result.clone()),
                "operation",
                JsValue::from_str(operation),
            )
        })();
        match assigned {
            Ok(()) => result,
            Err(failure) => {
                self.close();
                failure
            }
        }
    }

    fn frame_error(&mut self, value: RoomPlayError) -> JsValue {
        if !matches!(
            value,
            RoomPlayError::FrameWait(crate::room_frame_wait::RoomFrameWaitError::Deadline(
                crate::room_setup_wait::RoomDeadlineError::Expired
            ))
        ) {
            return error(value);
        }
        let result = error(value);
        let assigned = (|| -> Result<(), JsValue> {
            field(
                &js_sys::Object::from(result.clone()),
                "code",
                JsValue::from_str("timeout"),
            )?;
            field(
                &js_sys::Object::from(result.clone()),
                "operation",
                JsValue::from_str("frame"),
            )
        })();
        match assigned {
            Ok(()) => result,
            Err(failure) => {
                self.close();
                failure
            }
        }
    }

    fn local_result(&mut self, result: Result<(), JsValue>) -> Result<(), JsValue> {
        if let Err(value) = &result {
            if !self.driver.failed() {
                // The JS owner distinguishes a recoverable local phase refusal
                // from malformed protocol evidence that failed this facade.
                match js_sys::Reflect::set(
                    value,
                    &JsValue::from_str("code"),
                    &JsValue::from_str("state"),
                ) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.close();
                        return Err(error("room error property assignment refused"));
                    }
                    Err(failure) => {
                        self.close();
                        return Err(failure);
                    }
                }
            }
        }
        result
    }
}

#[wasm_bindgen]
impl BrowserRoomClient {
    #[wasm_bindgen(constructor)]
    pub fn new(identity: Vec<u8>, players: Vec<u32>) -> Result<Self, JsValue> {
        Self::new_with_start(identity, players, 0)
    }
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
        Ok(Self {
            driver: RoomClientDriver::new(
                &identity,
                &roster[..count],
                StartPolicy::default(),
                preroll_ns,
            )
            .map_err(error)?,
        })
    }
    pub fn request_seal(&mut self) -> Result<(), JsValue> {
        self.driver.request_seal().map_err(error)
    }
    pub fn request_ready(&mut self) -> Result<(), JsValue> {
        self.driver.request_ready().map_err(error)
    }
    pub fn request_leave(&mut self) -> Result<(), JsValue> {
        self.driver.request_leave().map_err(error)
    }
    pub fn request_drain(&mut self) -> Result<(), JsValue> {
        let result = self.driver.request_drain().map_err(error);
        self.local_result(result)
    }
    pub fn publish_progress(&mut self, words: Vec<u32>, final_prefix: bool) -> Result<(), JsValue> {
        let result = self
            .driver
            .publish_progress_words(&words, final_prefix)
            .map_err(error);
        self.local_result(result)
    }
    pub fn needed_bytes(&mut self) -> Result<u32, JsValue> {
        self.driver
            .needed_bytes()
            .map(|size| size as u32)
            .map_err(error)
    }
    pub fn frame_pending(&self) -> bool {
        self.driver.frame_pending()
    }
    pub fn receive_bytes(
        &mut self,
        bytes: Vec<u8>,
        captured_ns: i64,
        now_ns: i64,
    ) -> Result<u32, JsValue> {
        self.driver
            .receive_bytes(&bytes, captured_ns, now_ns)
            .map(|size| size as u32)
            .map_err(|error| self.frame_error(error))
    }
    pub fn next_write(&mut self, now_ns: i64) -> Result<BrowserMultiplayerWrite, JsValue> {
        self.driver
            .next_write(now_ns)
            .map(|frame| match frame {
                Some(frame) => WriteStep::Frame(frame).into(),
                None => WriteStep::Waiting.into(),
            })
            .map_err(error)
    }
    pub fn written(&mut self, id: u64, completed_ns: i64, now_ns: i64) -> Result<(), JsValue> {
        self.driver.written(id, completed_ns, now_ns).map_err(error)
    }
    /// Consume one accepted participant token, materializing only its latest
    /// borrowed prefix. Metadata revision is independent of progress traffic.
    pub fn take_peer_progress(&mut self) -> Result<JsValue, JsValue> {
        let Some(participant) = self.driver.pending_peer().map_err(error)? else {
            return Ok(JsValue::NULL);
        };
        let result = (|| {
            let prefix = self
                .driver
                .peer_progress(participant)
                .ok_or_else(|| error("missing accepted room peer prefix"))?;
            let words = encode_words(&prefix.members).map_err(error)?;
            let value = js_sys::Object::new();
            field(
                &value,
                "participant",
                js_sys::BigInt::from(participant.0).into(),
            )?;
            field(
                &value,
                "sequence",
                js_sys::BigInt::from(prefix.sequence).into(),
            )?;
            field(
                &value,
                "finalPrefix",
                JsValue::from_bool(prefix.final_prefix),
            )?;
            field(
                &value,
                "words",
                js_sys::Uint32Array::from(words.as_slice()).into(),
            )?;
            Ok(value.into())
        })();
        if result.is_err() {
            self.close();
        } else {
            self.driver.consume_peer_progress();
        }
        result
    }

    pub fn configure_frame_wait(&mut self, timeout_ns: u64) -> Result<(), JsValue> {
        self.driver
            .configure_frame_wait(timeout_ns)
            .map_err(|error| self.frame_error(error))
    }
    pub fn frame_wait_step(&mut self, now_ns: i64) -> Result<i64, JsValue> {
        self.driver
            .frame_wait_step(now_ns)
            .map(|step| match step {
                crate::room_frame_wait::RoomFrameWaitStep::Idle => -1,
                crate::room_frame_wait::RoomFrameWaitStep::Wait(ns) => ns as i64,
            })
            .map_err(|error| self.frame_error(error))
    }
    pub fn begin_setup(&mut self, now_ns: i64, timeout_ns: u64) -> Result<(), JsValue> {
        self.driver
            .begin_setup(now_ns, timeout_ns)
            .map_err(|error| self.setup_error(error))
    }
    pub fn setup_wait_step(&mut self, now_ns: i64) -> Result<i64, JsValue> {
        self.driver
            .setup_step(now_ns)
            .map(|step| match step {
                crate::room_setup_wait::RoomSetupStep::Idle => -2,
                crate::room_setup_wait::RoomSetupStep::Complete => -1,
                crate::room_setup_wait::RoomSetupStep::Wait(ns) => ns as i64,
            })
            .map_err(|error| self.setup_error(error))
    }
    pub fn begin_drain(&mut self, now_ns: i64, timeout_ns: u64) -> Result<(), JsValue> {
        self.driver
            .begin_drain(now_ns, timeout_ns)
            .map_err(|value| self.drain_error(value))
    }
    pub fn drain_wait_step(&mut self, now_ns: i64) -> Result<i64, JsValue> {
        self.driver
            .drain_step(now_ns)
            .map(|step| match step {
                crate::room_final_wait::RoomFinalStep::Completed => -1,
                crate::room_final_wait::RoomFinalStep::Wait(delay) => delay as i64,
            })
            .map_err(|value| self.drain_error(value))
    }
    pub fn drain_requested(&self) -> bool {
        self.driver.drain_requested()
    }
    pub fn local_final_written(&self) -> bool {
        self.driver.local_final_written()
    }
    pub fn local_final_acknowledged(&self) -> bool {
        self.driver.local_final_acknowledged()
    }
    pub fn peer_final_ack_written(&self, participant: u64) -> bool {
        self.driver
            .peer_final_ack_written(ParticipantId(participant))
    }
    pub fn progress_complete(&self) -> bool {
        self.driver.progress_complete()
    }
    pub fn drain_complete(&self) -> bool {
        self.driver.drain_complete()
    }
    /// Consumes only a genuine committed common schedule. All values retain
    /// their exact integer domains across the JavaScript boundary.
    pub fn take_start(&mut self) -> Result<JsValue, JsValue> {
        let schedule = self.driver.take_start().map_err(error)?;
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
        self.driver.participant_id()
    }
    pub fn revision(&self) -> u64 {
        self.driver.revision()
    }
    pub fn has_snapshot(&self) -> bool {
        self.driver.has_snapshot()
    }
    pub fn leave_written(&self) -> bool {
        self.driver.leave_written()
    }
    pub fn snapshot(&mut self) -> Result<JsValue, JsValue> {
        let result = snapshot_value(self.driver.session_ref().map_err(error)?);
        if result.is_err() {
            self.close();
        }
        result
    }
    pub fn close(&mut self) {
        self.driver.close();
    }
}
