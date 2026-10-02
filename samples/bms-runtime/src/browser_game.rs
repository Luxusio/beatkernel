//! Worker-owned gameplay bindings; audio lives in a separate Worklet instance.
use std::{
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

use crate::{
    browser::BrowserPrepared,
    image_assets::ImageAssets,
    note_progress::NoteProgress,
    player_chart::PlayerChart,
    step_gameplay::{StepAudioBatch, StepGameplay, StepGameplayConfig, StepGameplayError},
    worklet_audio::decode_output,
};
use beatkernel::{
    audio::{AudioCommand, PcmSample, SampleId},
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent, codec::CodecLimits,
    },
    judge::JudgeEvent,
    replay::codec::ReplayCodecLimits,
    runtime::RuntimeReport,
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
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
    keys: Vec<(u8, u16)>,
    samples: VecDeque<(SampleId, PcmSample)>,
    output_start: Option<u64>,
    output_context: Option<u64>,
    chart_seed: u64,
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
        let (mut game, bank) =
            StepGameplay::new(prepared.prepared, config, bindings).map_err(error)?;
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
            keys,
            samples: bank.into_samples().collect(),
            output_start: None,
            output_context: None,
            chart_seed: prepared.chart_seed,
        })
    }
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
    /// Called after stop/failure and before free; yields owned encoded bytes
    /// once. The binding does not create a file or infer a complete-song label.
    pub fn take_replay(&mut self) -> Result<Option<Vec<u8>>, JsValue> {
        self.game.take_replay().map_err(error)
    }
    pub fn activate(&mut self, host_ns: i64) -> Result<(), JsValue> {
        self.game.activate(point(HOST, host_ns)).map_err(error)
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
        let lane = self
            .keys
            .iter()
            .find(|entry| entry.1 == key)
            .map(|entry| entry.0)
            .ok_or_else(|| error("browser input key is not bound"))?;
        let input = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(1), point(HOST, host_ns), sequence),
            control: PhysicalControlId::keyboard(key),
            state: if down {
                ButtonState::Down
            } else {
                ButtonState::Up
            },
        });
        let result = self
            .game
            .process_input(input, &Explicit, point(OUTPUT, audio_ns));
        self.accept_report(result)?;
        if let Some(mask) = crate::pressed_keys::lane_bit(GameControlId(u32::from(lane))) {
            if down {
                self.pressed |= mask;
            } else {
                self.pressed &= !mask;
            }
        }
        Ok(())
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
        let evidence = match decode_output(&words) {
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
        self.samples.clear();
        self.pressed = 0;
    }
    #[wasm_bindgen(getter)]
    pub fn song_ns(&self) -> i64 {
        self.game.song_time().as_nanos()
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
    pub fn failed(&self) -> bool {
        self.game.failed()
    }
}

impl BrowserGame {
    fn accept_report(
        &mut self,
        result: Result<RuntimeReport, StepGameplayError>,
    ) -> Result<(), JsValue> {
        match result {
            Ok(report) => {
                self.observe(&report);
                Ok(())
            }
            Err(failure) => {
                match &failure {
                    StepGameplayError::Report { report, .. }
                    | StepGameplayError::Capture {
                        report: Some(report),
                        ..
                    } => self.observe(report),
                    _ => {}
                }
                Err(error(failure))
            }
        }
    }
    fn observe(&mut self, report: &RuntimeReport) {
        self.progress.apply(&report.judge_events);
        for event in &report.judge_events {
            if self.recent.len() == 128 {
                self.recent.remove(0);
            }
            self.recent.push(*event);
        }
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
