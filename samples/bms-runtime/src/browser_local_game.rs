//! Worker-owned local gameplay: one shared output, independent actual members.
use std::{collections::VecDeque, sync::Arc};

use crate::{
    browser::BrowserPrepared,
    browser_game::{BrowserSample, OUTPUT, encode_batch, encode_saved_opponents},
    browser_hid_input::BrowserHidSetup,
    browser_input::{LocalPhysicalInputSetup, TouchInputSetup, decode_input},
    competition::OpponentKind,
    image_assets::ImageAssets,
    local_players::PlayerId,
    local_runtime::{InputResult, PlayerReport},
    note_progress::NoteProgress,
    player_chart::PlayerChart,
    pressed_keys::PressedKeys,
    saved_opponent_hud::SavedOpponentHud,
    saved_opponents::SavedOpponents,
    step_gameplay::{
        StepGameplayConfig, StepGameplayError, StepLocalGameplay, StepLocalGameplayError,
    },
    worklet_audio::{decode_output, decode_section_output},
};
use beatkernel::{
    audio::{PcmSample, SampleId},
    input::{Binding, DeviceId, PhysicalInputEvent, Position2, codec::CodecLimits},
    judge::JudgeEvent,
    replay::codec::ReplayCodecLimits,
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_bms::BmsInputMode;
use beatkernel_platform::audio::presentation::discipline::DisciplineConfig;
use wasm_bindgen::prelude::*;

const HOST: ClockDomainId = ClockDomainId(0x57494e);
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

pub(crate) struct BrowserLocalMember {
    pub(crate) player: PlayerId,
    pub(crate) progress: NoteProgress,
    pub(crate) recent: Vec<JudgeEvent>,
    pub(crate) pressed: u32,
    pub(crate) saved_hud: SavedOpponentHud,
    opponents: Option<SavedOpponents>,
    opponent_error: Option<String>,
    source: Option<DeviceId>,
    pressed_owners: PressedKeys,
}

impl BrowserLocalMember {
    /// Admission is setup-only. Failed presentation keeps this reserved space
    /// so a configured touch field never moves during gameplay.
    pub(crate) fn comparison_height(&self) -> i64 {
        self.opponents
            .as_ref()
            .map_or(0, |opponents| opponents.count() as i64 * 14)
    }
}

/// Owns one original prepared bank and the actual shared local runtime. Input
/// source IDs come from the caller's admitted plan, never from member indices.
#[wasm_bindgen]
pub struct BrowserLocalGame {
    pub(crate) game: StepLocalGameplay,
    pub(crate) chart: Arc<PlayerChart>,
    pub(crate) images: Arc<ImageAssets>,
    pub(crate) members: Vec<BrowserLocalMember>,
    input_limits: CodecLimits,
    input_bindings: Vec<Binding>,
    hid_setup: Option<BrowserHidSetup>,
    hid_events: Vec<PhysicalInputEvent>,
    samples: VecDeque<(SampleId, PcmSample)>,
    output_start: Option<u64>,
    output_context: Option<u64>,
    chart_seed: u64,
    opponent_source: Option<beatkernel_bms::BmsChart>,
    opponent_count: usize,
    opponent_bytes: usize,
}

#[wasm_bindgen]
impl BrowserLocalGame {
    /// Consume live preparation once. Plan rows have four words; binding rows
    /// have a stable player ID followed by the seven physical identity words.
    pub fn new_physical(
        prepared: BrowserPrepared,
        host_origin_ns: i64,
        preroll_ns: i64,
        early_ns: i64,
        late_ns: i64,
        offset_ns: i64,
        plan_words: Vec<u32>,
        binding_words: Vec<u32>,
        end_ns: Option<i64>,
        contact: bool,
        max_encoded_input: u32,
        max_payload_input: u32,
    ) -> Result<Self, JsValue> {
        if prepared.replay.is_some() || host_origin_ns < 0 {
            return Err(error(
                "live resources and a nonnegative browser host origin are required",
            ));
        }
        let input = LocalPhysicalInputSetup::new(
            &plan_words,
            &binding_words,
            &prepared.chart.lanes,
            max_encoded_input,
            max_payload_input,
        )
        .map_err(error)?;
        let mut input_bindings = Vec::new();
        input_bindings
            .try_reserve_exact(binding_words.len() / 8)
            .map_err(|_| error("local browser binding snapshot allocation failed"))?;
        for bindings in &input.bindings {
            input_bindings.extend_from_slice(bindings.bindings());
        }
        let chart = Arc::new(prepared.chart);
        let mut members = Vec::new();
        members
            .try_reserve_exact(input.plan.members().len())
            .map_err(|_| error("local browser member allocation failed"))?;
        for &(player, source) in input.plan.members() {
            let mut recent = Vec::new();
            recent
                .try_reserve_exact(128)
                .map_err(|_| error("local recent judgment allocation failed"))?;
            members.push(BrowserLocalMember {
                player,
                source,
                progress: NoteProgress::new(chart.clone()).map_err(error)?,
                recent,
                pressed: 0,
                saved_hud: SavedOpponentHud::default(),
                opponents: None,
                opponent_error: None,
                pressed_owners: PressedKeys::default(),
            });
        }
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
        let mode = if contact {
            BmsInputMode::ButtonOrContact
        } else {
            BmsInputMode::ButtonOnly
        };
        let opponent_source = prepared.prepared.source.clone();
        let (mut game, bank) = StepLocalGameplay::new_section(
            prepared.prepared,
            config,
            input.plan,
            input.bindings,
            prepared.start,
            end_ns.map(Timestamp::from_nanos),
            mode,
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
            members,
            input_limits: input.limits,
            input_bindings,
            hid_setup: None,
            hid_events: Vec::new(),
            samples: bank.into_samples().collect(),
            output_start: None,
            output_context: None,
            chart_seed: prepared.chart_seed,
            opponent_source: Some(opponent_source),
            opponent_count: 0,
            opponent_bytes: 0,
        })
    }

    #[wasm_bindgen(getter)]
    pub fn players(&self) -> Vec<u32> {
        self.game.players().iter().map(|player| player.0).collect()
    }
    pub fn hits(&self, player: u32) -> Result<u64, JsValue> {
        Ok(self.score(player)?.hits)
    }
    pub fn misses(&self, player: u32) -> Result<u64, JsValue> {
        Ok(self.score(player)?.misses)
    }
    pub fn combo(&self, player: u32) -> Result<u64, JsValue> {
        Ok(self.score(player)?.combo)
    }
    pub fn max_combo(&self, player: u32) -> Result<u64, JsValue> {
        Ok(self.score(player)?.max_combo)
    }
    pub fn member_song_ns(&self, player: u32) -> Result<i64, JsValue> {
        self.game
            .member_song_time(PlayerId(player))
            .map(Timestamp::as_nanos)
            .ok_or_else(|| error("unknown local player"))
    }
    pub fn pressed(&self, player: u32) -> Result<u32, JsValue> {
        self.members
            .iter()
            .find(|member| member.player == PlayerId(player))
            .map(|member| member.pressed)
            .ok_or_else(|| error("unknown local player"))
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
    pub fn failed(&self) -> bool {
        self.game.failed()
    }
    pub fn input_setup_available(&self) -> bool {
        self.game.input_setup_available()
    }
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
    pub fn next_sample(&mut self) -> Option<BrowserSample> {
        self.samples
            .pop_front()
            .map(|(id, sample)| BrowserSample::from_pcm(id, sample))
    }

    pub fn configure_capture(
        &mut self,
        player: u32,
        max_bytes: u32,
        max_records: u32,
    ) -> Result<(), JsValue> {
        let limits = ReplayCodecLimits::new(
            max_bytes as usize,
            max_records as usize,
            4096,
            CodecLimits::new(65_536, 32_768).map_err(error)?,
        )
        .map_err(error)?;
        self.game
            .configure_capture(PlayerId(player), limits, self.chart_seed)
            .map_err(error)
    }
    pub fn take_replay(&mut self, player: u32) -> Result<Option<Vec<u8>>, JsValue> {
        self.game.take_replay(PlayerId(player)).map_err(error)
    }
    pub fn competition_identity(&self, player: u32) -> Result<Vec<u8>, JsValue> {
        let limits = crate::competition_live::replay_limits().map_err(error)?;
        self.game
            .competition_identity(PlayerId(player), limits, self.chart_seed)
            .map_err(error)
    }
    /// Adds one genuine recorded prefix for one member before activation.
    /// The eight-record and 64 MiB quotas belong to the entire local owner.
    pub fn add_saved_opponent(
        &mut self,
        player: u32,
        encoded: Vec<u8>,
        own: bool,
        label: String,
    ) -> Result<usize, JsValue> {
        let index = self
            .members
            .iter()
            .position(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local player"))?;
        let source = self
            .opponent_source
            .as_ref()
            .ok_or_else(|| error("saved opponents must be admitted before activation"))?;
        if self.opponent_count >= 8 {
            return Err(error("local saved opponent capacity reached"));
        }
        let charged = self
            .opponent_bytes
            .checked_add(encoded.len())
            .filter(|bytes| *bytes <= 64 * 1024 * 1024)
            .ok_or_else(|| error("local saved opponent encoded-byte quota exceeded"))?;
        if self.members[index].saved_hud.failed() {
            return Err(error("saved opponent presentation is disabled"));
        }
        let limits = crate::competition_live::replay_limits().map_err(error)?;
        let header = self
            .game
            .competition_header(PlayerId(player), limits, self.chart_seed)
            .map_err(error)?;
        let kind = if own {
            OpponentKind::Own
        } else {
            OpponentKind::Other
        };
        let member = &mut self.members[index];
        let admitted = if let Some(opponents) = &mut member.opponents {
            opponents
                .add(source, &encoded, kind, &label)
                .map_err(error)?
        } else {
            let mut opponents =
                SavedOpponents::new(header, limits, 8, 64 * 1024 * 1024).map_err(error)?;
            let admitted = opponents
                .add(source, &encoded, kind, &label)
                .map_err(error)?;
            member.opponents = Some(opponents);
            admitted
        };
        self.opponent_count += 1;
        self.opponent_bytes = charged;
        Ok(admitted)
    }

    /// Refresh each member at its own committed song frontier. A comparison
    /// failure only disables that member's retained display, never gameplay.
    pub fn saved_opponents(&mut self) -> Result<JsValue, JsValue> {
        let array = js_sys::Array::new();
        for member in &mut self.members {
            let result = (|| -> Result<JsValue, String> {
                if member.saved_hud.failed() {
                    return Err(member
                        .opponent_error
                        .clone()
                        .unwrap_or_else(|| "saved opponent presentation is disabled".into()));
                }
                let Some(opponents) = &mut member.opponents else {
                    return Ok(js_sys::Array::new().into());
                };
                let song = self
                    .game
                    .member_song_time(member.player)
                    .ok_or("local member has no song frontier")?;
                opponents
                    .advance_to(song)
                    .map_err(|failure| failure.to_string())?;
                member.saved_hud.update(opponents)?;
                encode_saved_opponents(opponents)
                    .map_err(|_| "saved opponent snapshot encoding failed".to_string())
            })();
            let (opponents, failure) = match result {
                Ok(opponents) => (opponents, JsValue::NULL),
                Err(failure) => {
                    member.saved_hud.mark_failed();
                    let retained: String = failure.chars().take(2048).collect();
                    member.opponent_error = Some(retained.clone());
                    (JsValue::NULL, JsValue::from_str(&retained))
                }
            };
            let row = js_sys::Object::new();
            for (name, value) in [
                ("player", JsValue::from_f64(f64::from(member.player.0))),
                ("opponents", opponents),
                ("error", failure),
            ] {
                js_sys::Reflect::set(&row, &JsValue::from_str(name), &value)?;
            }
            array.push(&row);
        }
        Ok(array.into())
    }

    pub fn disable_saved_opponent_hud(&mut self, player: u32) -> Result<(), JsValue> {
        let member = self
            .members
            .iter_mut()
            .find(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local player"))?;
        member.saved_hud.mark_failed();
        member
            .opponent_error
            .get_or_insert_with(|| "saved opponent presentation is disabled".into());
        Ok(())
    }
    pub fn configure_touch_regions(
        &mut self,
        player: u32,
        words: Vec<u32>,
        bounds: Vec<f32>,
        max_contacts: u32,
    ) -> Result<(), JsValue> {
        let member = self
            .members
            .iter()
            .find(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local player"))?;
        if let Some(source) = member.source {
            if words.chunks_exact(7).any(|row| {
                row[1] != 1 || (u64::from(row[2]) | (u64::from(row[3]) << 32)) != source.0
            }) {
                return Err(error(
                    "assigned player's touch regions must select its exact source",
                ));
            }
        }
        let setup = TouchInputSetup::new(&words, &bounds, &self.chart.lanes, max_contacts)
            .map_err(error)?;
        self.game
            .configure_touch_router(PlayerId(player), setup.router)
            .map_err(error)
    }

    /// Prepared lane regions for this member's actual visible field. Coordinates
    /// remain global scene coordinates, separately from original pointer payloads.
    pub fn touch_bounds(&self, player: u32, page: u32) -> Result<Vec<f32>, JsValue> {
        let count = self.members.len();
        let page_size = crate::ui::organisms::LOCAL_PLAYERS_PER_PAGE;
        let page = page as usize;
        if page >= count.div_ceil(page_size) {
            return Err(error("invalid local touch page"));
        }
        let index = self
            .members
            .iter()
            .position(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local touch player"))?;
        let first = page * page_size;
        let visible = (count - first).min(page_size);
        if index < first || index >= first + visible {
            return Err(error("touch player is not on the visible local page"));
        }
        crate::playfield_layout::local_touch_bounds_with_comparison_space(
            &self.chart.lanes,
            visible,
            index - first,
            self.members[index].comparison_height(),
        )
        .map_err(error)
    }

    #[wasm_bindgen(getter)]
    pub fn touch_width(&self) -> u32 {
        crate::playfield_layout::LOGICAL_EXTENT[0]
    }
    #[wasm_bindgen(getter)]
    pub fn touch_height(&self) -> u32 {
        crate::playfield_layout::LOGICAL_EXTENT[1]
    }
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
    pub fn activate(&mut self, host_ns: i64) -> Result<(), JsValue> {
        self.game.activate(point(HOST, host_ns)).map_err(error)?;
        self.opponent_source = None;
        Ok(())
    }
    pub fn input_blob(&mut self, bytes: Vec<u8>, audio_ns: i64) -> Result<(), JsValue> {
        let input = decode_input(&bytes, self.input_limits, HOST).map_err(error)?;
        self.process_physical(input, audio_ns)
    }
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
        self.accept_input(result)
    }
    /// Decode an entire genuine report before dispatch. Every typed event keeps
    /// the same original source/time/sequence, including its committed prefix on
    /// failure. Empty fanout submits the original raw report, never a fake key.
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
                .expect("HID setup checked")
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
    pub fn advance(&mut self, host_ns: i64, audio_ns: i64) -> Result<(), JsValue> {
        let result = self
            .game
            .advance_to(point(HOST, host_ns), &Explicit, point(OUTPUT, audio_ns));
        self.accept_reports(result)?;
        self.game
            .update_output_clock(point(HOST, host_ns))
            .map(|_| ())
            .map_err(error)
    }
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
    /// Original Worklet evidence is validated before it can admit more BGM.
    /// One shared completion barrier still waits for every actual member.
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
        self.hid_setup = None;
        self.hid_events.clear();
        for member in &mut self.members {
            member.pressed_owners.clear();
            member.pressed = 0;
        }
    }
}

impl BrowserLocalGame {
    fn score(&self, player: u32) -> Result<&crate::competition::ScoreSummary, JsValue> {
        self.game
            .score(PlayerId(player))
            .ok_or_else(|| error("unknown local player"))
    }
    fn process_physical(
        &mut self,
        input: PhysicalInputEvent,
        audio_ns: i64,
    ) -> Result<(), JsValue> {
        let result = self
            .game
            .process_input(input, &Explicit, point(OUTPUT, audio_ns));
        self.accept_input(result)
    }
    fn accept_input(
        &mut self,
        result: Result<InputResult, StepLocalGameplayError>,
    ) -> Result<(), JsValue> {
        match result {
            Ok(InputResult::Ignored { .. }) => Ok(()),
            Ok(InputResult::Processed(reports)) => self.accept_reports(Ok(reports)),
            Err(failure) => self.accept_reports(Err(failure)),
        }
    }
    fn accept_reports(
        &mut self,
        result: Result<Vec<PlayerReport>, StepLocalGameplayError>,
    ) -> Result<(), JsValue> {
        match result {
            Ok(reports) => self.observe_reports(&reports).map_err(error),
            Err(failure) => {
                if let StepLocalGameplayError::Operation { reports, .. } = &failure {
                    // Observe every already committed member, retaining the
                    // original operation failure even if feedback also fails.
                    let _ = self.observe_reports(reports);
                }
                Err(error(failure))
            }
        }
    }
    fn observe_reports(&mut self, reports: &[PlayerReport]) -> Result<(), String> {
        let mut failure = None;
        for PlayerReport { player, report } in reports {
            let member = self
                .members
                .iter_mut()
                .find(|member| member.player == *player)
                .expect("actual local reports reference prepared members");
            member.progress.apply(&report.judge_events);
            for event in &report.judge_events {
                if member.recent.len() == 128 {
                    member.recent.remove(0);
                }
                member.recent.push(*event);
            }
            if report.song_end_reached {
                member.pressed_owners.clear();
                member.pressed = 0;
            } else if let Err(reason) = member.pressed_owners.apply(&report.bound_inputs) {
                if failure.is_none() {
                    failure = Some(reason);
                }
            } else {
                member.pressed = member.pressed_owners.mask();
            }
        }
        match failure {
            Some(reason) => {
                self.game.fail();
                Err(reason)
            }
            None => Ok(()),
        }
    }
}
