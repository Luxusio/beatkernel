//! Browser bindings for the actual shared session; no transport or clock access.
use crate::{
    local_players::PlayerId,
    multiplayer_group::{decode_words, encode_words},
    multiplayer_protocol::{
        FrameDecoder, GroupEvent, MultiplayerError, MultiplayerEvent, Progress, Session, WriteStep,
    },
    multiplayer_start::{StartPolicy, StartRole},
};
use wasm_bindgen::prelude::*;

fn error(value: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&value.to_string()).into()
}

fn field(object: &js_sys::Object, name: &str, value: JsValue) -> Result<(), JsValue> {
    js_sys::Reflect::set(object, &JsValue::from_str(name), &value)?;
    Ok(())
}

/// One write admission. `kind` is waiting=0, frame=1, application slot=2.
#[wasm_bindgen]
pub struct BrowserMultiplayerWrite {
    kind: u8,
    frame_id: u64,
    bytes: Option<Vec<u8>>,
}

impl From<WriteStep> for BrowserMultiplayerWrite {
    fn from(step: WriteStep) -> Self {
        match step {
            WriteStep::Frame(frame) => Self {
                kind: 1,
                frame_id: frame.id,
                bytes: Some(frame.bytes),
            },
            WriteStep::Waiting => Self {
                kind: 0,
                frame_id: 0,
                bytes: None,
            },
            WriteStep::ApplicationSlot => Self {
                kind: 2,
                frame_id: 0,
                bytes: None,
            },
        }
    }
}

#[wasm_bindgen]
impl BrowserMultiplayerWrite {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> u8 {
        self.kind
    }

    #[wasm_bindgen(getter)]
    pub fn frame_id(&self) -> u64 {
        self.frame_id
    }

    /// Transfers the admitted frame allocation once; this does not credit a write.
    pub fn take_bytes(&mut self) -> Result<Vec<u8>, JsValue> {
        self.bytes
            .take()
            .ok_or_else(|| error("no remaining admitted frame bytes"))
    }
}

#[wasm_bindgen]
pub struct BrowserMultiplayer {
    session: Option<Session>,
    decoder: Option<FrameDecoder>,
    failure: Option<MultiplayerError>,
}

impl BrowserMultiplayer {
    fn operate<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, MultiplayerError>,
    ) -> Result<T, JsValue> {
        if let Some(failure) = &self.failure {
            return Err(error(failure));
        }
        let result = operation(self);
        if let Err(failure) = &result {
            self.failure = Some(failure.clone());
        }
        result.map_err(error)
    }

    fn session(&mut self) -> Result<&mut Session, MultiplayerError> {
        self.session.as_mut().ok_or(MultiplayerError::Closed)
    }

    fn decoder(&mut self) -> Result<&mut FrameDecoder, MultiplayerError> {
        self.decoder.as_mut().ok_or(MultiplayerError::Closed)
    }
}

#[wasm_bindgen]
impl BrowserMultiplayer {
    /// The caller supplies the actual canonical gameplay setup identity and
    /// bounds it before the generated binding copies bytes into WASM memory.
    #[wasm_bindgen(constructor)]
    pub fn new(identity: Vec<u8>, host: bool, preroll_ns: i64) -> Result<Self, JsValue> {
        let policy = StartPolicy::default();
        Self::with_policy(
            identity,
            host,
            preroll_ns,
            policy.lead_ns,
            policy.min_remaining_ns,
            policy.max_age_ns,
            policy.max_uncertainty_ns,
            policy.max_release_lateness_ns,
        )
    }

    pub fn with_policy(
        identity: Vec<u8>,
        host: bool,
        preroll_ns: i64,
        lead_ns: u64,
        min_remaining_ns: u64,
        max_age_ns: u64,
        max_uncertainty_ns: u64,
        max_release_lateness_ns: u64,
    ) -> Result<Self, JsValue> {
        let role = if host {
            StartRole::Host
        } else {
            StartRole::Join
        };
        let policy = StartPolicy {
            lead_ns,
            min_remaining_ns,
            max_age_ns,
            max_uncertainty_ns,
            max_release_lateness_ns,
        };
        let session = Session::new(identity, role, policy, preroll_ns).map_err(error)?;
        Ok(Self {
            session: Some(session),
            decoder: Some(FrameDecoder::new()),
            failure: None,
        })
    }

    /// Explicit group mode retains one shared readiness/clock/start/write owner.
    pub fn new_group(
        identity: Vec<u8>,
        players: Vec<u32>,
        host: bool,
        preroll_ns: i64,
    ) -> Result<Self, JsValue> {
        if !(1..=crate::local_players::MAX_LOCAL_PLAYERS).contains(&players.len()) {
            return Err(error("group session requires 1..64 players"));
        }
        let mut roster = Vec::new();
        roster
            .try_reserve_exact(players.len())
            .map_err(|_| error("group browser roster allocation failed"))?;
        roster.extend(players.into_iter().map(PlayerId));
        let role = if host {
            StartRole::Host
        } else {
            StartRole::Join
        };
        let session =
            Session::new_group(identity, roster, role, StartPolicy::default(), preroll_ns)
                .map_err(error)?;
        Ok(Self {
            session: Some(session),
            decoder: Some(FrameDecoder::new()),
            failure: None,
        })
    }

    pub fn request_ready(&mut self) -> Result<(), JsValue> {
        self.operate(|owner| owner.session()?.request_ready())
    }

    pub fn needed_bytes(&mut self) -> Result<u32, JsValue> {
        self.operate(|owner| Ok(owner.decoder()?.needed()? as u32))
    }

    /// Supply at most `needed_bytes()` bytes, sliced BEFORE entering generated
    /// WASM glue. A complete frame uses `now_ns` as its actual receipt timestamp.
    /// No protocol operation or clock estimate is fabricated for partial bytes.
    pub fn receive_bytes(&mut self, bytes: Vec<u8>, now_ns: i64) -> Result<u32, JsValue> {
        self.operate(|owner| {
            if bytes.len() > owner.decoder()?.needed()? {
                return Err(MultiplayerError::Protocol(
                    "incoming prefix exceeds decoder need".into(),
                ));
            }
            let consumed = owner.decoder()?.push(&bytes)?;
            if let Some((tag, payload)) = owner.decoder()?.take()? {
                owner.session()?.receive(tag, &payload, now_ns)?;
            }
            Ok(consumed as u32)
        })
    }

    pub fn next_write(&mut self, now_ns: i64) -> Result<BrowserMultiplayerWrite, JsValue> {
        self.operate(|owner| owner.session()?.poll_write(now_ns).map(Into::into))
    }

    pub fn send_progress(
        &mut self,
        song_ns: i64,
        hits: u64,
        misses: u64,
        combo: u64,
        max_combo: u64,
        final_prefix: bool,
        now_ns: i64,
    ) -> Result<BrowserMultiplayerWrite, JsValue> {
        self.operate(|owner| {
            let frame = owner.session()?.send_progress(
                Progress {
                    song_ns,
                    hits,
                    misses,
                    combo,
                    max_combo,
                },
                final_prefix,
                now_ns,
            )?;
            Ok(WriteStep::Frame(frame).into())
        })
    }

    pub fn written(&mut self, frame_id: u64, now_ns: i64) -> Result<(), JsValue> {
        self.operate(|owner| owner.session()?.written(frame_id, now_ns))
    }

    /// Submit exact member words only through a fresh real application slot.
    pub fn send_group_progress(
        &mut self,
        words: Vec<u32>,
        final_prefix: bool,
        now_ns: i64,
    ) -> Result<BrowserMultiplayerWrite, JsValue> {
        self.operate(|owner| {
            let members = decode_words(&words)?;
            let frame = owner
                .session()?
                .send_group_progress(members, final_prefix, now_ns)?;
            Ok(WriteStep::Frame(frame).into())
        })
    }

    /// Group roster/prefix events stay separate from the compatible scalar DTO.
    /// Already committed events remain drainable after session failure.
    pub fn poll_group_event(&mut self) -> Result<JsValue, JsValue> {
        let Some(event) = self.session.as_mut().and_then(Session::poll_group_event) else {
            return Ok(JsValue::NULL);
        };
        let object = js_sys::Object::new();
        let kind = match event {
            GroupEvent::Roster(players) => {
                let mut words = Vec::new();
                words
                    .try_reserve_exact(players.len())
                    .map_err(|_| error("group browser event allocation failed"))?;
                words.extend(players.into_iter().map(|player| player.0));
                field(
                    &object,
                    "players",
                    js_sys::Uint32Array::from(words.as_slice()).into(),
                )?;
                "roster"
            }
            GroupEvent::Progress(prefix) => {
                let words = encode_words(&prefix.members).map_err(error)?;
                field(
                    &object,
                    "sequence",
                    js_sys::BigInt::from(prefix.sequence).into(),
                )?;
                field(
                    &object,
                    "words",
                    js_sys::Uint32Array::from(words.as_slice()).into(),
                )?;
                if prefix.final_prefix {
                    "group-final-progress"
                } else {
                    "group-progress"
                }
            }
        };
        field(&object, "kind", JsValue::from_str(kind))?;
        Ok(object.into())
    }

    /// Already committed events remain drainable after a protocol/decoder fault.
    /// Null means no event; all times and scores use exact JavaScript BigInts.
    pub fn poll_event(&mut self) -> Result<JsValue, JsValue> {
        let Some(event) = self.session.as_mut().and_then(Session::poll_event) else {
            return Ok(JsValue::NULL);
        };
        let object = js_sys::Object::new();
        let final_prefix = matches!(&event, MultiplayerEvent::FinalProgress(_));
        let kind = match event {
            MultiplayerEvent::Connected => "connected",
            MultiplayerEvent::Ready => "ready",
            MultiplayerEvent::ClockEstimated(estimate) => {
                for (name, value) in [
                    ("lowerNs", js_sys::BigInt::from(estimate.lower_ns())),
                    ("upperNs", js_sys::BigInt::from(estimate.upper_ns())),
                    ("midpointNs", js_sys::BigInt::from(estimate.midpoint_ns())),
                    (
                        "roundTripNs",
                        js_sys::BigInt::from(estimate.round_trip_ns()),
                    ),
                    (
                        "observedLocalNs",
                        js_sys::BigInt::from(estimate.observed_local_ns()),
                    ),
                ] {
                    field(&object, name, value.into())?;
                }
                "clock"
            }
            MultiplayerEvent::StartScheduled(schedule) => {
                field(
                    &object,
                    "targetNs",
                    js_sys::BigInt::from(schedule.target_ns).into(),
                )?;
                field(
                    &object,
                    "songTargetNs",
                    js_sys::BigInt::from(schedule.song_target_ns).into(),
                )?;
                field(
                    &object,
                    "uncertaintyNs",
                    js_sys::BigInt::from(schedule.uncertainty_ns).into(),
                )?;
                "start"
            }
            MultiplayerEvent::Progress(progress) | MultiplayerEvent::FinalProgress(progress) => {
                field(
                    &object,
                    "songNs",
                    js_sys::BigInt::from(progress.song_ns).into(),
                )?;
                for (name, value) in [
                    ("hits", progress.hits),
                    ("misses", progress.misses),
                    ("combo", progress.combo),
                    ("maxCombo", progress.max_combo),
                ] {
                    field(&object, name, js_sys::BigInt::from(value).into())?;
                }
                if final_prefix {
                    "final-progress"
                } else {
                    "progress"
                }
            }
            MultiplayerEvent::FinalAcknowledged => "final-acknowledged",
            MultiplayerEvent::Disconnected(reason) => {
                field(&object, "error", JsValue::from_str(&reason.to_string()))?;
                "disconnected"
            }
        };
        field(&object, "kind", JsValue::from_str(kind))?;
        Ok(object.into())
    }

    pub fn setup_complete(&self) -> bool {
        self.session.as_ref().is_some_and(Session::setup_complete)
    }
    pub fn preparation_pending(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(Session::preparation_pending)
    }
    pub fn start_committed(&self) -> bool {
        self.session.as_ref().is_some_and(Session::start_committed)
    }

    pub fn close(&mut self) {
        self.session = None;
        self.decoder = None;
        self.failure.get_or_insert(MultiplayerError::Closed);
    }
}
