//! Worker-owned gameplay bindings; audio lives in a separate Worklet instance.
use std::{
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

use crate::{
    browser::BrowserPrepared,
    browser_hid_input::BrowserHidSetup,
    browser_input::{PhysicalInputSetup, TouchInputSetup, decode_input},
    competition::OpponentKind,
    image_assets::ImageAssets,
    note_progress::NoteProgress,
    player_chart::PlayerChart,
    pressed_keys::PressedKeys,
    saved_opponents::SavedOpponents,
    saved_opponent_hud::SavedOpponentHud,
    step_gameplay::{StepAudioBatch, StepGameplay, StepGameplayConfig, StepGameplayError},
    worklet_audio::{decode_output, decode_section_output},
};
use beatkernel::{
    audio::{AudioCommand, PcmSample, SampleId},
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent, Position2, codec::CodecLimits,
    },
    judge::JudgeEvent,
    replay::codec::ReplayCodecLimits,
    runtime::RuntimeReport,
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_bms::BmsInputMode;
use beatkernel_platform::audio::presentation::discipline::DisciplineConfig;
use wasm_bindgen::prelude::*;

const HOST: ClockDomainId = ClockDomainId(0x57494e);
pub(crate) const OUTPUT: ClockDomainId = ClockDomainId(0x57415544);
struct Explicit;
impl ClockMapper for Explicit {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn point(domain: ClockDomainId, ns: i64) -> ClockPoint {
    ClockPoint {
        domain,
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn error(value: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&value.to_string()).into()
}
fn field(object: &js_sys::Object, name: &str, value: JsValue) -> Result<(), JsValue> {
    js_sys::Reflect::set(object, &JsValue::from_str(name), &value)?;
    Ok(())
}
fn unsigned(value: u64) -> JsValue {
    js_sys::BigInt::from(value).into()
}
fn signed(value: i64) -> JsValue {
    js_sys::BigInt::from(value).into()
}

/// One setup asset; taking PCM transfers its original Rust allocation once.
#[wasm_bindgen]
pub struct BrowserSample {
    id: u64,
    rate: u32,
    channels: u16,
    pcm: Option<Vec<f32>>,
}
impl BrowserSample {
    pub(crate) fn from_pcm(id: SampleId, sample: PcmSample) -> Self {
        let format = sample.format();
        Self {
            id: id.0,
            rate: format.sample_rate(),
            channels: format.channels(),
            pcm: Some(sample.into_samples()),
        }
    }
}
#[wasm_bindgen]
impl BrowserSample {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> u64 {
        self.id
    }
    #[wasm_bindgen(getter)]
    pub fn rate(&self) -> u32 {
        self.rate
    }
    #[wasm_bindgen(getter)]
    pub fn channels(&self) -> u16 {
        self.channels
    }
    pub fn take_pcm(&mut self) -> Result<Vec<f32>, JsValue> {
        self.pcm
            .take()
            .ok_or_else(|| error("sample PCM was already taken"))
    }
}

#[wasm_bindgen]
pub struct BrowserGame {
    pub(crate) game: StepGameplay,
    pub(crate) chart: Arc<PlayerChart>,
    pub(crate) images: Arc<ImageAssets>,
    pub(crate) progress: NoteProgress,
    pub(crate) recent: Vec<JudgeEvent>,
    pub(crate) pressed: u32,
    pressed_owners: PressedKeys,
    input_limits: CodecLimits,
    input_bindings: Vec<Binding>,
    hid_setup: Option<BrowserHidSetup>,
    hid_events: Vec<PhysicalInputEvent>,
    keys: Vec<(u8, u16)>,
    samples: VecDeque<(SampleId, PcmSample)>,
    output_start: Option<u64>,
    output_context: Option<u64>,
    chart_seed: u64,
    opponent_source: Option<beatkernel_bms::BmsChart>,
    opponents: Option<SavedOpponents>,
    pub(crate) saved_hud: SavedOpponentHud,
}
#[wasm_bindgen]
impl BrowserGame {
    #[wasm_bindgen(constructor)]
    pub fn new(
        prepared: BrowserPrepared,
        host_origin_ns: i64,
        preroll_ns: i64,
        early_ns: i64,
        late_ns: i64,
        offset_ns: i64,
        key_pairs: Vec<u32>,
    ) -> Result<Self, JsValue> {
        Self::construct(
            prepared,
            host_origin_ns,
            preroll_ns,
            early_ns,
            late_ns,
            offset_ns,
            key_pairs,
            None,
        )
    }
    /// Use the same live owner with an immutable original-song endpoint.
    pub fn new_section(
        prepared: BrowserPrepared,
        host_origin_ns: i64,
        preroll_ns: i64,
        early_ns: i64,
        late_ns: i64,
        offset_ns: i64,
        key_pairs: Vec<u32>,
        end_ns: i64,
    ) -> Result<Self, JsValue> {
        Self::construct(
            prepared,
            host_origin_ns,
            preroll_ns,
            early_ns,
            late_ns,
            offset_ns,
            key_pairs,
            Some(Timestamp::from_nanos(end_ns)),
        )
    }
    /// Prepare device-aware bindings for canonical physical event blobs.
    pub fn new_physical(
        prepared: BrowserPrepared,
        host_origin_ns: i64,
        preroll_ns: i64,
        early_ns: i64,
        late_ns: i64,
        offset_ns: i64,
        binding_words: Vec<u32>,
        end_ns: Option<i64>,
        max_encoded_input: u32,
        max_payload_input: u32,
    ) -> Result<Self, JsValue> {
        Self::construct_physical(
            prepared,
            host_origin_ns,
            preroll_ns,
            early_ns,
            late_ns,
            offset_ns,
            binding_words,
            end_ns,
            max_encoded_input,
            max_payload_input,
            BmsInputMode::ButtonOnly,
        )
    }

    /// Opt in to actual button/contact rules while retaining physical provenance.
    /// Acquisition, permissions and coordinate-to-lane routing belong to the host.
    pub fn new_physical_contact(
        prepared: BrowserPrepared,
        host_origin_ns: i64,
        preroll_ns: i64,
        early_ns: i64,
        late_ns: i64,
        offset_ns: i64,
        binding_words: Vec<u32>,
        end_ns: Option<i64>,
        max_encoded_input: u32,
        max_payload_input: u32,
    ) -> Result<Self, JsValue> {
        Self::construct_physical(
            prepared,
            host_origin_ns,
            preroll_ns,
            early_ns,
            late_ns,
            offset_ns,
            binding_words,
            end_ns,
            max_encoded_input,
            max_payload_input,
            BmsInputMode::ButtonOrContact,
        )
    }
}

impl BrowserGame {
    fn construct_physical(
        prepared: BrowserPrepared,
        host_origin_ns: i64,
        preroll_ns: i64,
        early_ns: i64,
        late_ns: i64,
        offset_ns: i64,
        binding_words: Vec<u32>,
        end_ns: Option<i64>,
        max_encoded_input: u32,
        max_payload_input: u32,
        input_mode: BmsInputMode,
    ) -> Result<Self, JsValue> {
        let input = PhysicalInputSetup::new(
            &binding_words,
            &prepared.chart.lanes,
            max_encoded_input,
            max_payload_input,
        )
        .map_err(error)?;
        Self::construct_bound(
            prepared,
            host_origin_ns,
            preroll_ns,
            early_ns,
            late_ns,
            offset_ns,
            Vec::new(),
            input,
            end_ns.map(Timestamp::from_nanos),
            input_mode,
        )
    }

    fn construct(
        prepared: BrowserPrepared,
        host_origin_ns: i64,
        preroll_ns: i64,
        early_ns: i64,
        late_ns: i64,
        offset_ns: i64,
        key_pairs: Vec<u32>,
        end: Option<Timestamp>,
    ) -> Result<Self, JsValue> {
        if prepared.replay.is_some() {
            return Err(error("replay resources require the replay owner"));
        }
        if key_pairs.len() > 36 || key_pairs.len() % 2 != 0 || host_origin_ns < 0 {
            return Err(error("invalid browser binding count or host origin"));
        }
        let mut lanes = BTreeSet::new();
        let mut physical = BTreeSet::new();
        let mut keys = Vec::new();
        for pair in key_pairs.chunks_exact(2) {
            let lane = u8::try_from(pair[0]).map_err(error)?;
            let key = u16::try_from(pair[1]).map_err(error)?;
            if !matches!(lane, 0x11..=0x19 | 0x21..=0x29)
                || key == 0
                || !lanes.insert(lane)
                || !physical.insert(key)
            {
                return Err(error("browser lane/key bindings must be valid and unique"));
            }
            keys.push((lane, key));
        }
        if prepared
            .chart
            .lanes
            .iter()
            .any(|lane| !lanes.contains(lane))
        {
            return Err(error("a prepared lane has no browser key binding"));
        }
        let bindings = BindingMap::from_bindings(keys.iter().map(|&(lane, key)| Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(key),
            game_control: GameControlId(u32::from(lane)),
        }))
        .map_err(error)?;
        Self::construct_bound(
            prepared,
            host_origin_ns,
            preroll_ns,
            early_ns,
            late_ns,
            offset_ns,
            keys,
            PhysicalInputSetup {
                bindings,
                limits: CodecLimits::new(4096, 1024).map_err(error)?,
            },
            end,
            BmsInputMode::ButtonOnly,
        )
    }

    fn construct_bound(
        prepared: BrowserPrepared,
        host_origin_ns: i64,
        preroll_ns: i64,
        early_ns: i64,
        late_ns: i64,
        offset_ns: i64,
        keys: Vec<(u8, u16)>,
        input: PhysicalInputSetup,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
    ) -> Result<Self, JsValue> {
        if prepared.replay.is_some() || host_origin_ns < 0 {
            return Err(error(
                "live resources and a nonnegative browser host origin are required",
            ));
        }
        if input.bindings.bindings().len() > 256 {
            return Err(error("browser constructor exceeds 256 physical bindings"));
        }
        let mut input_bindings = Vec::new();
        input_bindings
            .try_reserve_exact(input.bindings.bindings().len())
            .map_err(|_| error("browser binding snapshot allocation failed"))?;
        input_bindings.extend_from_slice(input.bindings.bindings());
        let chart = Arc::new(prepared.chart);
        let progress = NoteProgress::new(chart.clone()).map_err(error)?;
        let config = StepGameplayConfig {
            host_origin: point(HOST, host_origin_ns),
            output_origin: point(OUTPUT, 0),
            preroll: Duration::from_nanos(preroll_ns),
            early_ns,
            late_ns,
            offset_ns,
            command_capacity: 4096,
            bgm_pending: 3072,
            bgm_lookahead: Duration::from_nanos(500_000_000),
            telemetry_capacity: 256,
        };
        // Comparison admission reconstructs genuine records from this source.
        // Clone only during preparation and release it after activation.
        let opponent_source = prepared.prepared.source.clone();
        let (mut game, bank) = StepGameplay::new_section_with_input_mode(
            prepared.prepared,
            config,
            input.bindings,
            prepared.start,
            end,
            input_mode,
        )
        .map_err(error)?;
        game.configure_output_clock(DisciplineConfig {
            max_observation_age: Duration::from_nanos(1_000_000_000),
            ..DisciplineConfig::default()
        })
        .map_err(error)?;
        Ok(Self {
            game,
            chart,
            images: prepared.images,
            progress,
            recent: Vec::with_capacity(128),
            pressed: 0,
            pressed_owners: PressedKeys::default(),
            input_limits: input.limits,
            input_bindings,
            hid_setup: None,
            hid_events: Vec::new(),
            keys,
            samples: bank.into_samples().collect(),
            output_start: None,
            output_context: None,
            chart_seed: prepared.chart_seed,
            opponent_source: Some(opponent_source),
            opponents: None,
            saved_hud: SavedOpponentHud::default(),
        })
    }
}

#[wasm_bindgen]
impl BrowserGame {
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Actual pristine judge/profile/chart-branch identity for the shared native
    /// protocol. Capture remains optional and unchanged; bytes are bounded by
    /// the common 65536-byte identity limit before returning to JavaScript.
    pub fn competition_identity(&self) -> Result<Vec<u8>, JsValue> {
        let limits = crate::competition_live::replay_limits().map_err(error)?;
        self.game
            .competition_identity(limits, self.chart_seed)
            .map_err(error)
    }
    /// Setup-only saved prefix admission. Failure belongs to this comparison;
    /// it neither fails the local game nor changes its optional recording.
    pub fn add_saved_opponent(
        &mut self,
        encoded: Vec<u8>,
        own: bool,
        label: String,
    ) -> Result<usize, JsValue> {
        let source = self
            .opponent_source
            .as_ref()
            .ok_or_else(|| error("saved opponents must be admitted before activation"))?;
        let limits = crate::competition_live::replay_limits().map_err(error)?;
        let header = self
            .game
            .competition_header(limits, self.chart_seed)
            .map_err(error)?;
        let kind = if own {
            OpponentKind::Own
        } else {
            OpponentKind::Other
        };
        if let Some(opponents) = &mut self.opponents {
            opponents.add(source, &encoded, kind, &label).map_err(error)
        } else {
            let mut opponents =
                SavedOpponents::new(header, limits, 8, 64 * 1024 * 1024).map_err(error)?;
            let index = opponents
                .add(source, &encoded, kind, &label)
                .map_err(error)?;
            self.opponents = Some(opponents);
            Ok(index)
        }
    }
    /// Control-side snapshot at the actual local song frontier. Comparison
    /// errors are returned independently; input/audio paths never call this.
    pub fn saved_opponents(&mut self) -> Result<JsValue, JsValue> {
        if self.saved_hud.failed() {
            return Err(error("saved opponent presentation is disabled"));
        }
        let result = (|| {
            let array = js_sys::Array::new();
            let Some(opponents) = &mut self.opponents else {
                return Ok(array.into());
            };
            opponents.advance_to(self.game.song_time()).map_err(error)?;
            self.saved_hud.update(opponents).map_err(error)?;
            for opponent in opponents.opponents() {
                let object = js_sys::Object::new();
                field(
                    &object,
                    "kind",
                    JsValue::from_str(match opponent.kind() {
                        OpponentKind::Own => "own",
                        OpponentKind::Other => "other",
                    }),
                )?;
                field(&object, "label", JsValue::from_str(opponent.label()))?;
                field(
                    &object,
                    "songNs",
                    opponent
                        .song_time()
                        .map(|time| signed(time.as_nanos()))
                        .unwrap_or(JsValue::NULL),
                )?;
                field(
                    &object,
                    "recordedUntilNs",
                    opponent
                        .recorded_until()
                        .map(|time| signed(time.as_nanos()))
                        .unwrap_or(JsValue::NULL),
                )?;
                let score = opponent.score();
                for (name, value) in [
                    ("hits", score.hits),
                    ("misses", score.misses),
                    ("combo", score.combo),
                    ("maxCombo", score.max_combo),
                ] {
                    field(&object, name, unsigned(value))?;
                }
                array.push(&object);
            }
            Ok(array.into())
        })();
        if result.is_err() {
            self.saved_hud.mark_failed();
        }
        result
    }

    /// Invalid host-side comparison metadata must not remain visible on canvas.
    pub fn disable_saved_opponent_hud(&mut self) {
        self.saved_hud.mark_failed();
    }
    /// Explicit actual peer evidence, with no local clock or scoring authority.
    pub fn update_peer_hud(&mut self, status: u32, words: Vec<u32>) -> Result<(), JsValue> {
        self.saved_hud.update_peer(status, &words).map_err(error)
    }
    pub fn disable_peer_hud(&mut self) {
        self.saved_hud.mark_peer_failed();
    }
    /// Optional bounded canonical replay capture; must precede gameplay input
    /// or advancement. The seed comes from the actual prepared chart owner.
    pub fn configure_capture(&mut self, max_bytes: u32, max_records: u32) -> Result<(), JsValue> {
        let limits = ReplayCodecLimits::new(
            max_bytes as usize,
            max_records as usize,
            4096,
            CodecLimits::new(65_536, 32_768).map_err(error)?,
        )
        .map_err(error)?;
        self.game
            .configure_capture(limits, self.chart_seed)
            .map_err(error)
    }
    /// Configure fixed projected hit regions before activating a contact-mode game.
    /// The core router retains physical source/surface/contact ownership.
    pub fn configure_touch_regions(
        &mut self,
        words: Vec<u32>,
        bounds: Vec<f32>,
        max_contacts: u32,
    ) -> Result<(), JsValue> {
        let setup = TouchInputSetup::new(&words, &bounds, &self.chart.lanes, max_contacts)
            .map_err(error)?;
        self.game
            .configure_touch_router(setup.router)
            .map_err(error)
    }
    /// Configure complete HID profiles once before activation or gameplay input.
    /// Each physical field must match an existing constructor binding; all setup
    /// validation and event storage reservation precede configuration adoption.
    pub fn configure_hid_devices(
        &mut self,
        device_words: Vec<u32>,
        field_words: Vec<u32>,
        axis_params: Vec<f32>,
    ) -> Result<(), JsValue> {
        if !self.game.input_setup_available() || self.hid_setup.is_some() {
            return Err(error(
                "HID configuration requires a pristine, unconfigured gameplay owner",
            ));
        }
        let setup = BrowserHidSetup::new(
            &device_words,
            &field_words,
            &axis_params,
            &self.input_bindings,
        )
        .map_err(error)?;
        let mut events = Vec::new();
        events
            .try_reserve_exact(256)
            .map_err(|_| error("HID event scratch allocation failed"))?;
        self.hid_setup = Some(setup);
        self.hid_events = events;
        Ok(())
    }
    /// Actual full-size rendered lane slots in prepared chart order.
    #[wasm_bindgen(getter)]
    pub fn touch_bounds(&self) -> Result<Vec<f32>, JsValue> {
        crate::playfield_layout::default_touch_bounds(&self.chart.lanes).map_err(error)
    }
    /// Logical scene width used by rendering and projected touch hit points.
    #[wasm_bindgen(getter)]
    pub fn touch_width(&self) -> u32 {
        crate::playfield_layout::LOGICAL_EXTENT[0]
    }
    /// Logical scene height used by rendering and projected touch hit points.
    #[wasm_bindgen(getter)]
    pub fn touch_height(&self) -> u32 {
        crate::playfield_layout::LOGICAL_EXTENT[1]
    }
    /// Called after stop/failure and before free; yields owned encoded bytes
    /// once. The binding does not create a file or infer a complete-song label.
    pub fn take_replay(&mut self) -> Result<Option<Vec<u8>>, JsValue> {
        self.game.take_replay().map_err(error)
    }
    pub fn activate(&mut self, host_ns: i64) -> Result<(), JsValue> {
        self.game.activate(point(HOST, host_ns)).map_err(error)?;
        self.opponent_source = None;
        Ok(())
    }
    pub fn next_sample(&mut self) -> Option<BrowserSample> {
        self.samples
            .pop_front()
            .map(|(id, sample)| BrowserSample::from_pcm(id, sample))
    }
    pub fn input(
        &mut self,
        host_ns: i64,
        key: u16,
        down: bool,
        sequence: u64,
        audio_ns: i64,
    ) -> Result<(), JsValue> {
        if !self.keys.iter().any(|entry| entry.1 == key) {
            return Err(error("browser input key is not bound"));
        }
        let input = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(1), point(HOST, host_ns), sequence),
            control: PhysicalControlId::keyboard(key),
            state: if down {
                ButtonState::Down
            } else {
                ButtonState::Up
            },
        });
        self.process_physical(input, audio_ns)
    }
    /// Admit a canonical physical event with its original variant and provenance.
    /// Decoding refusal happens before runtime mutation or chronology adoption.
    pub fn input_blob(&mut self, bytes: Vec<u8>, audio_ns: i64) -> Result<(), JsValue> {
        let input = decode_input(&bytes, self.input_limits, HOST).map_err(error)?;
        self.process_physical(input, audio_ns)
    }
    /// Decode a genuine raw report completely, then process its typed fanout
    /// through ordinary runtime/capture/feedback paths with original metadata.
    /// Zero-emission reports still enter Runtime as their original unbound raw
    /// input. Gameplay failure preserves its committed prefix and prevents retry.
    pub fn input_hid_blob(&mut self, bytes: Vec<u8>, audio_ns: i64) -> Result<(), JsValue> {
        let input = decode_input(&bytes, self.input_limits, HOST).map_err(error)?;
        let PhysicalInputEvent::RawHidReport(report) = input else {
            return Err(error("HID profile input requires a genuine raw HID report"));
        };
        if self.game.failed() {
            return Err(error(StepGameplayError::Failed));
        }
        if self.hid_setup.is_none() {
            return Err(error("HID profile input requires configured device owners"));
        }
        let mut events = std::mem::take(&mut self.hid_events);
        events.clear();
        let result = (|| {
            self.hid_setup
                .as_mut()
                .expect("HID setup checked before taking scratch")
                .decode_report(&report, &mut events)
                .map_err(error)?;
            if events.is_empty() {
                if let Err(failure) =
                    self.process_physical(PhysicalInputEvent::RawHidReport(report), audio_ns)
                {
                    self.game.fail();
                    return Err(failure);
                }
            } else {
                for input in events.drain(..) {
                    if let Err(failure) = self.process_physical(input, audio_ns) {
                        self.game.fail();
                        return Err(failure);
                    }
                }
            }
            Ok(())
        })();
        events.clear();
        self.hid_events = events;
        result
    }
    /// Route a canonical touch using a separate projected point. Original payload
    /// and acquisition provenance remain in the actual runtime/capture report.
    pub fn input_blob_at(
        &mut self,
        bytes: Vec<u8>,
        x: f32,
        y: f32,
        audio_ns: i64,
    ) -> Result<(), JsValue> {
        let input = decode_input(&bytes, self.input_limits, HOST).map_err(error)?;
        if !matches!(&input, PhysicalInputEvent::Touch(_)) || !x.is_finite() || !y.is_finite() {
            return Err(error(
                "projected browser input requires a genuine touch and finite hit position",
            ));
        }
        let result = self.game.process_input_at(
            input,
            Position2 { x, y },
            &Explicit,
            point(OUTPUT, audio_ns),
        );
        self.accept_report(result)
    }
    pub fn advance(&mut self, host_ns: i64, audio_ns: i64) -> Result<(), JsValue> {
        let result = self
            .game
            .advance_to(point(HOST, host_ns), &Explicit, point(OUTPUT, audio_ns));
        self.accept_report(result)?;
        self.game
            .update_output_clock(point(HOST, host_ns))
            .map(|_| ())
            .map_err(error)
    }
    /// Actual paired browser presentation estimate, in the original Window
    /// host domain and armed-start-relative output domain. Admission alone never
    /// corrects transport; only a successful later watermark may apply a rate.
    pub fn observe_presentation(&mut self, output_ns: i64, host_ns: i64) -> Result<(), JsValue> {
        if output_ns < 0 || host_ns < 0 {
            self.game.fail();
            return Err(error("browser presentation points must be nonnegative"));
        }
        self.game
            .observe_output_clock(ClockPair {
                source: point(OUTPUT, output_ns),
                target: point(HOST, host_ns),
            })
            .map(|_| ())
            .map_err(error)
    }
    pub fn feed_audio(&mut self, rendered_frames: u64, budget: u32) -> Result<(), JsValue> {
        self.game
            .feed_audio(rendered_frames, budget as usize)
            .map(|_| ())
            .map_err(error)
    }
    /// Actual Worklet ABI words and optional browser output presentation, both
    /// relative to the immutable armed start. No timer substitutes for either.
    pub fn observe_output(
        &mut self,
        words: Vec<u32>,
        presented_ns: Option<i64>,
    ) -> Result<bool, JsValue> {
        if self.game.failed() {
            return Err(error(StepGameplayError::Failed));
        }
        let decoded = match self.game.playback_end_frame() {
            Some(end) => decode_section_output(&words, Some(end)),
            None => decode_output(&words),
        };
        let evidence = match decoded {
            Ok(evidence) => evidence,
            Err(reason) => {
                self.game.fail();
                return Err(error(reason));
            }
        };
        if self
            .output_start
            .is_some_and(|start| start != evidence.start)
            || self
                .output_context
                .is_some_and(|previous| evidence.context.is_none_or(|current| current < previous))
        {
            self.game.fail();
            return Err(error(
                "browser output start changed or context cursor regressed",
            ));
        }
        let presented = presented_ns.map(|ns| point(OUTPUT, ns));
        // Validate all clocks, counters and report chronology before BGM can
        // publish commands. Shared completion adopts evidence only afterwards.
        self.game
            .validate_completion_evidence(evidence.report, presented)
            .map_err(error)?;
        if let Some(report) = evidence.report {
            self.game
                .feed_audio(report.counters.rendered_frames, 256)
                .map_err(error)?;
        }
        let completed = self
            .game
            .observe_completion(evidence.report, presented)
            .map_err(error)?;
        self.output_start = Some(evidence.start);
        self.output_context = evidence.context;
        Ok(completed)
    }
    pub fn commands(&mut self, max: u32) -> Result<JsValue, JsValue> {
        let Some(batch) = self.game.take_commands(max as usize).map_err(error)? else {
            return Ok(JsValue::NULL);
        };
        let result = encode_batch(batch);
        if result.is_err() {
            self.game.fail();
        }
        result
    }
    pub fn acknowledge(
        &mut self,
        sequence: u64,
        admitted: u32,
        success: bool,
    ) -> Result<(), JsValue> {
        self.game
            .acknowledge(sequence, admitted as usize, success)
            .map_err(error)
    }
    pub fn stop(&mut self) {
        self.game.fail();
        self.opponent_source = None;
        self.samples.clear();
        self.pressed_owners.clear();
        self.pressed = 0;
    }
    #[wasm_bindgen(getter)]
    pub fn song_ns(&self) -> i64 {
        self.game.song_time().as_nanos()
    }
    #[wasm_bindgen(getter)]
    pub fn end_ns(&self) -> Option<i64> {
        self.game.end_ns()
    }
    #[wasm_bindgen(getter)]
    pub fn playback_end_frame(&self) -> Option<u64> {
        self.game.playback_end_frame()
    }
    #[wasm_bindgen(getter)]
    pub fn hits(&self) -> u64 {
        self.game.score().hits
    }
    #[wasm_bindgen(getter)]
    pub fn misses(&self) -> u64 {
        self.game.score().misses
    }
    #[wasm_bindgen(getter)]
    pub fn combo(&self) -> u64 {
        self.game.score().combo
    }
    #[wasm_bindgen(getter)]
    pub fn max_combo(&self) -> u64 {
        self.game.score().max_combo
    }
    #[wasm_bindgen(getter)]
    pub fn failed(&self) -> bool {
        self.game.failed()
    }
}

impl BrowserGame {
    fn process_physical(
        &mut self,
        input: PhysicalInputEvent,
        audio_ns: i64,
    ) -> Result<(), JsValue> {
        let result = self
            .game
            .process_input(input, &Explicit, point(OUTPUT, audio_ns));
        self.accept_report(result)
    }

    fn accept_report(
        &mut self,
        result: Result<RuntimeReport, StepGameplayError>,
    ) -> Result<(), JsValue> {
        match result {
            Ok(report) => self.observe(&report).map_err(error),
            Err(failure) => {
                match &failure {
                    StepGameplayError::Report { report, .. }
                    | StepGameplayError::Capture {
                        report: Some(report),
                        ..
                    } => {
                        // Preserve the original committed runtime/capture failure
                        // even if its presentation ownership also refuses the batch.
                        let _ = self.observe(report);
                    }
                    _ => {}
                }
                Err(error(failure))
            }
        }
    }
    fn observe(&mut self, report: &RuntimeReport) -> Result<(), String> {
        self.progress.apply(&report.judge_events);
        for event in &report.judge_events {
            if self.recent.len() == 128 {
                self.recent.remove(0);
            }
            self.recent.push(*event);
        }
        if report.song_end_reached {
            self.pressed_owners.clear();
            self.pressed = 0;
            return Ok(());
        }
        if let Err(error) = self.pressed_owners.apply(&report.bound_inputs) {
            self.game.fail();
            return Err(error);
        }
        self.pressed = self.pressed_owners.mask();
        Ok(())
    }
}

/// Single scalar command ABI used by both live and replay owners.
pub(crate) fn encode_batch(batch: StepAudioBatch) -> Result<JsValue, JsValue> {
    (|| {
        let commands = js_sys::Array::new();
        for command in batch.commands {
            let object = js_sys::Object::new();
            let at = command.at().as_nanos();
            let (kind, voice, sample, gain, value, denominator) = match command {
                AudioCommand::Play {
                    voice,
                    sample,
                    gain,
                    ..
                } => (0, voice.0, sample.0, gain, 0, 0),
                AudioCommand::Stop { voice, .. } => (1, voice.0, 0, 0.0, 0, 0),
                AudioCommand::SetRate { rate, .. } => {
                    (2, 0, 0, 0.0, rate.numerator(), rate.denominator())
                }
                AudioCommand::Seek { song_time, .. } => (3, 0, 0, 0.0, song_time.as_nanos(), 0),
            };
            field(&object, "kind", JsValue::from_f64(f64::from(kind)))?;
            field(&object, "voice", unsigned(voice))?;
            field(&object, "sample", unsigned(sample))?;
            field(&object, "at", signed(at))?;
            field(&object, "gain", JsValue::from_f64(f64::from(gain)))?;
            field(&object, "value", signed(value))?;
            field(&object, "denominator", unsigned(denominator))?;
            commands.push(&object);
        }
        let result = js_sys::Object::new();
        field(&result, "sequence", unsigned(batch.sequence))?;
        field(&result, "commands", commands.into())?;
        Ok(result.into())
    })()
}
