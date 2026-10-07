//! Selected-file and canvas bindings. Called serially by the browser Worker.

use std::sync::Arc;

use beatkernel::{
    audio::{AudioFormat, PcmLimits},
    replay::codec::{ReplayFile, decode_replay},
    time::Timestamp,
};
use wasm_bindgen::prelude::*;

use crate::{
    ChannelPolicy, DefaultAssetDecoder, PreparedBms,
    asset_paths::AssetPathPolicy,
    asset_source::{MemoryAssetLimits, MemoryFiles},
    browser_canvas::BrowserCanvas,
    image_assets::{ImageAssetLimits, ImageAssets},
    player_chart::PlayerChart,
    prepare_from_source,
};

#[path = "browser_render.rs"]
pub(crate) mod render;

const LOOKAHEAD_NS: i64 = 2_000_000_000;

/// Joined presentation only. It owns no game, samples, transport or clocks.
#[wasm_bindgen]
pub struct BrowserRoomResults {
    builder: crate::room_results_builder::RoomResultsBuilder,
}
#[wasm_bindgen]
impl BrowserRoomResults {
    #[wasm_bindgen(constructor)]
    pub fn new(own: u64, words: Vec<u32>) -> Result<Self, JsValue> {
        Ok(Self {
            builder: crate::room_results_builder::RoomResultsBuilder::new(
                crate::multiplayer_rooms::ParticipantId(own),
                &words,
            )
            .map_err(js_error)?,
        })
    }
    pub fn update(
        &mut self,
        participant: u64,
        sequence: u64,
        final_prefix: bool,
        words: Vec<u32>,
    ) -> Result<(), JsValue> {
        self.builder
            .update(
                crate::multiplayer_rooms::ParticipantId(participant),
                sequence,
                final_prefix,
                &words,
            )
            .map_err(js_error)
    }
    pub fn freeze(
        &mut self,
        page: u32,
        cancelled: bool,
        error: Option<String>,
        failed: bool,
    ) -> Result<(), JsValue> {
        self.builder
            .freeze(page as usize, cancelled, error, failed)
            .map_err(js_error)
    }
    pub fn set_page(&mut self, page: u32) -> Result<(), JsValue> {
        self.builder.set_page(page as usize).map_err(js_error)
    }
    #[wasm_bindgen(getter)]
    pub fn page(&self) -> u32 {
        self.builder
            .presentation()
            .map_or(0, |page| page.page as u32)
    }
    #[wasm_bindgen(getter)]
    pub fn pages(&self) -> u32 {
        self.builder.pages() as u32
    }
    #[wasm_bindgen(getter)]
    pub fn failed(&self) -> bool {
        self.builder.failed()
    }
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&error.to_string()).into()
}

/// Retained encoded files; no browser filesystem or network access in Rust.
#[wasm_bindgen]
pub struct BrowserLibrary {
    files: MemoryFiles,
}

#[wasm_bindgen]
impl BrowserLibrary {
    #[wasm_bindgen(constructor)]
    pub fn new(
        max_files: u32,
        max_file_bytes: u32,
        max_total_bytes: u32,
        max_path_bytes: u32,
    ) -> Result<BrowserLibrary, JsValue> {
        Ok(Self {
            files: MemoryFiles::new(MemoryAssetLimits {
                max_files: max_files as usize,
                max_file_bytes: max_file_bytes as usize,
                max_total_bytes: max_total_bytes as usize,
                max_path_bytes: max_path_bytes as usize,
            })
            .map_err(js_error)?,
        })
    }

    /// The supplied JS host preflights metadata before acquiring this buffer.
    pub fn add_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), JsValue> {
        self.files.insert(path, bytes).map_err(js_error)
    }

    pub fn chart_paths(&self) -> js_sys::Array {
        self.files
            .keys()
            .filter(|path| {
                path.rsplit_once('.').is_some_and(|(_, extension)| {
                    ["bms", "bme", "bml", "pms"]
                        .iter()
                        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
                })
            })
            .map(JsValue::from_str)
            .collect()
    }

    /// Prepare actual audio as well as visuals for the later gameplay owner.
    /// Rate conversion is deliberately not hidden in this host boundary.
    pub fn prepare_chart(
        &self,
        path: &str,
        sample_rate: u32,
        channels: u16,
        seed: u64,
        max_pcm_asset_bytes: u32,
        max_pcm_total_bytes: u32,
        max_samples: u32,
    ) -> Result<BrowserPrepared, JsValue> {
        let format = AudioFormat::new(sample_rate, channels).map_err(js_error)?;
        let limits = PcmLimits::new(
            max_pcm_asset_bytes as usize,
            max_pcm_total_bytes as usize,
            max_samples as usize,
        )
        .map_err(js_error)?;
        let bytes = self
            .files
            .read_file(path, beatkernel_bms::ParseOptions::default().max_bytes)
            .map_err(js_error)?;
        let source = self.files.scope(path).map_err(js_error)?;
        let prepared = prepare_from_source(
            bytes,
            &source,
            format,
            limits,
            ChannelPolicy::MonoToStereo,
            &DefaultAssetDecoder,
            AssetPathPolicy::AudioVariants,
            seed,
            None,
        )
        .map_err(js_error)?;
        let chart = PlayerChart::from_compiled(&prepared.source, &prepared.compiled.chart)
            .map_err(js_error)?;
        let images = Arc::new(
            ImageAssets::prepare_from_source(
                &source,
                &prepared.source,
                ImageAssetLimits::default(),
            )
            .map_err(js_error)?,
        );
        Ok(BrowserPrepared {
            prepared,
            visual_preview: None,
            chart,
            images,
            chart_seed: seed,
            start: Timestamp::ZERO,
            replay: None,
        })
    }

    /// Select from freshly decoded originals once. Gameplay and visual targets
    /// retain original song times; only overlapping BGM receives bounded suffixes.
    pub fn prepare_chart_at(
        &self,
        path: &str,
        sample_rate: u32,
        channels: u16,
        seed: u64,
        start_ns: i64,
        max_pcm_asset_bytes: u32,
        max_pcm_total_bytes: u32,
        max_samples: u32,
    ) -> Result<BrowserPrepared, JsValue> {
        if start_ns < 0 {
            return Err(js_error("live section start must be nonnegative"));
        }
        let start = Timestamp::from_nanos(start_ns);
        let pcm_limits = PcmLimits::new(
            max_pcm_asset_bytes as usize,
            max_pcm_total_bytes as usize,
            max_samples as usize,
        )
        .map_err(js_error)?;
        let original = self.prepare_chart(
            path,
            sample_rate,
            channels,
            seed,
            max_pcm_asset_bytes,
            max_pcm_total_bytes,
            max_samples,
        )?;
        let (prepared, _) = crate::section_start::prepare_at(original.prepared, start, pcm_limits)
            .map_err(js_error)?;
        let chart = PlayerChart::from_compiled(&prepared.source, &prepared.compiled.chart)
            .map_err(js_error)?;
        Ok(BrowserPrepared {
            prepared,
            visual_preview: None,
            chart,
            images: original.images,
            chart_seed: seed,
            start,
            replay: None,
        })
    }

    /// Decode the canonical bounded recording and prepare its actual seeded
    /// chart section. This resource is consumed by BrowserReplay, not live play.
    pub fn prepare_replay_chart(
        &self,
        path: &str,
        bytes: Vec<u8>,
        sample_rate: u32,
        channels: u16,
        max_pcm_asset_bytes: u32,
        max_pcm_total_bytes: u32,
        max_samples: u32,
    ) -> Result<BrowserPrepared, JsValue> {
        let limits = crate::competition_live::replay_limits().map_err(js_error)?;
        let file = decode_replay(&bytes, limits).map_err(js_error)?;
        let setup =
            crate::replay_playback::decode_section_setup(&file.header.options).map_err(js_error)?;
        let start = setup.start;
        let seed = setup.chart_seed;
        let original = self.prepare_chart(
            path,
            sample_rate,
            channels,
            seed,
            max_pcm_asset_bytes,
            max_pcm_total_bytes,
            max_samples,
        )?;
        let pcm_limits = PcmLimits::new(
            max_pcm_asset_bytes as usize,
            max_pcm_total_bytes as usize,
            max_samples as usize,
        )
        .map_err(js_error)?;
        let prepared = crate::section_start::prepare_section_replay(
            original.prepared,
            &file,
            limits,
            pcm_limits,
        )
        .map_err(js_error)?;
        let chart = PlayerChart::from_compiled(&prepared.source, &prepared.compiled.chart)
            .map_err(js_error)?;
        Ok(BrowserPrepared {
            prepared,
            visual_preview: None,
            chart,
            images: original.images,
            chart_seed: seed,
            start,
            replay: Some(file),
        })
    }
}

/// Owns resources once; moving into BrowserView does not clone PCM or indexes.
#[wasm_bindgen]
pub struct BrowserPrepared {
    pub(crate) prepared: PreparedBms,
    visual_preview: Option<(u64, u64, u64, u32, u32, bool)>,
    pub(crate) chart: PlayerChart,
    pub(crate) images: Arc<ImageAssets>,
    pub(crate) chart_seed: u64,
    pub(crate) start: Timestamp,
    pub(crate) replay: Option<ReplayFile>,
}

#[wasm_bindgen]
impl BrowserPrepared {
    #[wasm_bindgen(getter)]
    pub fn start_ns(&self) -> i64 {
        self.start.as_nanos()
    }
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        self.chart.title.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn artist(&self) -> String {
        self.chart.artist.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn duration_ns(&self) -> i64 {
        self.chart.duration_ns
    }
    #[wasm_bindgen(getter)]
    pub fn note_count(&self) -> usize {
        self.chart.notes.len()
    }
    #[wasm_bindgen(getter)]
    pub fn sample_count(&self) -> usize {
        self.prepared.bank.len()
    }
    #[wasm_bindgen(getter)]
    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    #[wasm_bindgen(getter)]
    pub fn lanes(&self) -> Vec<u8> {
        self.chart.lanes.clone()
    }
}

/// GPU and chart ownership in the dedicated Worker, separate from DOM controls.
#[wasm_bindgen]
pub struct BrowserView {
    canvas: BrowserCanvas,
    current: Option<(Arc<PlayerChart>, Arc<ImageAssets>)>,
    song: Timestamp,
    visual: Option<render::VisualPresentation>,
    visual_identity: Option<(u64, u64, u64)>,
    visual_generation_floor: u64,
}

#[wasm_bindgen]
impl BrowserView {
    pub async fn create(canvas: web_sys::OffscreenCanvas) -> Result<BrowserView, JsValue> {
        Ok(Self {
            canvas: BrowserCanvas::create(canvas).await.map_err(js_error)?,
            current: None,
            song: Timestamp::ZERO,
            visual: None,
            visual_identity: None,
            visual_generation_floor: 0,
        })
    }

    pub fn set_chart(&mut self, prepared: BrowserPrepared) {
        self.current = Some((Arc::new(prepared.chart), prepared.images));
        self.song = Timestamp::ZERO;
    }

    /// Explicit original-song preview position, represented by JS BigInt.
    pub fn seek(&mut self, song_ns: i64) -> Result<(), JsValue> {
        if self.current.is_none() {
            return Err(js_error("select a chart before seeking"));
        }
        if song_ns < 0 || song_ns.checked_add(LOOKAHEAD_NS).is_none() {
            return Err(js_error(
                "preview time must be nonnegative and leave room for lookahead",
            ));
        }
        self.song = Timestamp::from_nanos(song_ns);
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), JsValue> {
        self.canvas.resize(width, height).map_err(js_error)
    }

    pub fn draw(&mut self) -> Result<(), JsValue> {
        if let Some(current) = &self.current {
            self.canvas
                .present_chart(&current.0, &current.1, self.song, LOOKAHEAD_NS)
                .map_err(js_error)?;
        }
        Ok(())
    }

    pub fn draw_game(&mut self, game: &crate::browser_game::BrowserGame) -> Result<(), JsValue> {
        self.canvas
            .present_game(game, LOOKAHEAD_NS)
            .map_err(js_error)
    }

    /// Draw a bounded page of actual local members on the Worker-owned canvas.
    /// The page changes presentation only; every member continues on the same owner.
    pub fn draw_local_game(
        &mut self,
        game: &crate::browser_local_game::BrowserLocalGame,
        page: u32,
    ) -> Result<(), JsValue> {
        self.canvas
            .present_local_game(game, LOOKAHEAD_NS, page as usize)
            .map_err(js_error)
    }

    pub fn draw_replay(
        &mut self,
        replay: &crate::browser_replay::BrowserReplay,
    ) -> Result<(), JsValue> {
        self.canvas
            .present_replay(replay, LOOKAHEAD_NS)
            .map_err(js_error)
    }

    pub fn draw_historical_record(
        &mut self,
        record: &crate::browser_historical_record::BrowserHistoricalRecord,
    ) -> Result<(), JsValue> {
        self.canvas
            .present_historical_record(record)
            .map_err(js_error)
    }

    pub fn draw_completed_results(
        &mut self,
        results: &crate::browser_completed_results::BrowserCompletedResults,
    ) -> Result<(), JsValue> {
        self.canvas
            .present_completed_results(results, None)
            .map_err(js_error)
    }
    pub fn draw_completed_room_results(
        &mut self,
        results: &crate::browser_completed_results::BrowserCompletedResults,
        room: &BrowserRoomResults,
    ) -> Result<(), JsValue> {
        let page = room
            .builder
            .presentation()
            .ok_or_else(|| js_error("room Results are not frozen"))?;
        self.canvas
            .present_completed_results(results, Some(page))
            .map_err(js_error)
    }

    pub fn draw_room_results(&mut self, results: &BrowserRoomResults) -> Result<(), JsValue> {
        let page = results
            .builder
            .presentation()
            .ok_or_else(|| js_error("room Results are not frozen"))?;
        self.canvas.present_room_results(page).map_err(js_error)
    }

    pub fn needs_redraw(&self) -> bool {
        self.canvas.needs_redraw()
    }
}

#[wasm_bindgen]
impl BrowserPrepared {
    pub fn visual_registration(&mut self, generation: u64, content: u64, max_packet_bytes: u32, max_diagnostic_bytes: u32) -> Result<Vec<u8>, JsValue> {
        let bytes = render::registration(&self.chart, &self.images, &[], generation, content, max_packet_bytes, max_diagnostic_bytes).map_err(js_error)?;
        self.visual_preview = Some((generation, content, 0, max_packet_bytes, max_diagnostic_bytes, false));
        Ok(bytes)
    }
    pub fn preview_state(&mut self, sequence: u64, song_ns: i64) -> Result<Vec<u8>, JsValue> {
        let (generation, content, previous, packet, diagnostics, pending) = self.visual_preview.ok_or_else(|| js_error("register preview before exporting state"))?;
        if pending || sequence <= previous || song_ns < 0 || song_ns.checked_add(LOOKAHEAD_NS).is_none() { return Err(js_error("invalid or pending preview state")); }
        let bytes = render::wire::encode_packet(render::header(render::wire::PREVIEW, generation, content, sequence), &render::wire::WirePacket::Preview(render::wire::PreviewState { song_ns, lookahead_ns: LOOKAHEAD_NS }), render::limits(packet, diagnostics)).map_err(js_error)?;
        self.visual_preview = Some((generation, content, sequence, packet, diagnostics, true));
        Ok(bytes)
    }
    pub fn acknowledge_visual(&mut self, generation: u64, content: u64, sequence: u64) -> bool {
        if let Some((registered, resource, pending_sequence, _, _, pending)) = self.visual_preview.as_mut() {
            if *registered == generation && *resource == content && *pending && *pending_sequence == sequence { *pending = false; return true; }
        }
        false
    }
}
#[wasm_bindgen]
impl BrowserRoomResults {
    pub fn visual_snapshot(&self, generation: u64, content: u64, max_packet_bytes: u32, max_diagnostic_bytes: u32) -> Result<Vec<u8>, JsValue> {
        let model = self.builder.export_visual().map_err(js_error)?.ok_or_else(|| js_error("room Results unavailable"))?;
        render::wire::encode_packet(render::header(render::wire::ROOM, generation, content, 0), &render::wire::WirePacket::Room(model), render::limits(max_packet_bytes, max_diagnostic_bytes)).map_err(js_error)
    }
}

#[wasm_bindgen]
impl BrowserView {
    /// Only the fixed header is copied before trusted byte admission. Generated
    /// bindings therefore cannot allocate an attacker-sized Rust input first.
    pub fn import_visual_packet(&mut self, bytes: js_sys::Uint8Array, max_packet_bytes: u32, max_diagnostic_bytes: u32) -> Result<u64, JsValue> {
        self.import_visual_packet_with_mode(bytes, max_packet_bytes, max_diagnostic_bytes, None)
    }

    /// Explicit mode admission is part of the atomic registration, including a
    /// one-member local cohort whose stable identity is P1.
    pub fn import_visual_registration(&mut self, bytes: js_sys::Uint8Array, max_packet_bytes: u32, max_diagnostic_bytes: u32, mode: u32) -> Result<u64, JsValue> {
        if mode > 3 { return Err(js_error("invalid visual registration mode")); }
        self.import_visual_packet_with_mode(bytes, max_packet_bytes, max_diagnostic_bytes, Some(mode))
    }

    fn import_visual_packet_with_mode(&mut self, bytes: js_sys::Uint8Array, max_packet_bytes: u32, max_diagnostic_bytes: u32, mode: Option<u32>) -> Result<u64, JsValue> {
        use render::{wire, VisualPresentation};
        let packet_len = bytes.length() as usize;
        if packet_len < wire::HEADER_BYTES { return Err(js_error("truncated visual packet header")); }
        let mut fixed = [0u8; wire::HEADER_BYTES];
        for (index, destination) in fixed.iter_mut().enumerate() { *destination = bytes.get_index(index as u32); }
        let limits = render::limits(max_packet_bytes, max_diagnostic_bytes);
        let admitted = wire::preflight_header(&fixed, packet_len, limits).map_err(js_error)?;
        if mode.is_some() && admitted.kind != wire::REGISTRATION {
            return Err(js_error("mode admission requires a visual registration packet"));
        }
        let registration = matches!(admitted.kind, wire::REGISTRATION | wire::HISTORY | wire::RESULTS | wire::ROOM);
        let combined_room = admitted.kind == wire::ROOM && self.visual_identity.is_some_and(|(generation, content, _)| generation == admitted.generation && content == admitted.content) && matches!(self.visual, Some(VisualPresentation::Results { .. }));
        if registration {
            if admitted.generation <= self.visual_generation_floor && !combined_room { return Err(js_error("stale visual registration")); }
        } else if self.visual_identity.is_none_or(|(generation, content, sequence)| generation != admitted.generation || content != admitted.content || admitted.sequence <= sequence) {
            return Err(js_error("stale or foreign visual state"));
        }
        let (header, packet) = wire::decode_packet(&bytes.to_vec(), limits).map_err(js_error)?;
        if header.kind != admitted.kind || header.generation != admitted.generation || header.content != admitted.content || header.sequence != admitted.sequence || header.payload_len != admitted.payload_len {
            return Err(js_error("visual packet header changed during admission"));
        }
        match packet {
            wire::WirePacket::Registration(registration) => {
                let count = registration.roster.len();
                if mode.is_some_and(|mode| match mode {
                    0 => count != 0,
                    1 | 3 => count != 1,
                    2 => !(1..=crate::browser_render_state::MAX_RENDER_PLAYERS).contains(&count),
                    _ => true,
                }) {
                    return Err(js_error("visual roster does not match registration mode"));
                }
                let replacement = if registration.roster.is_empty() {
                    let chart = Arc::new(PlayerChart::import_visual(registration.chart).map_err(js_error)?);
                    let images = Arc::new(ImageAssets::import_visual(registration.images, limits.image).map_err(js_error)?);
                    VisualPresentation::Preview { chart, images, song: Timestamp::ZERO, lookahead: LOOKAHEAD_NS }
                } else {
                    let local = mode.map_or_else(
                        || registration.roster.len() > 1 || registration.roster[0].0 != 1,
                        |mode| mode == 2,
                    );
                    let state = crate::browser_render_state::BrowserRenderState::import_visual_with_budget(header.generation, header.content, registration.chart, registration.images, registration.roster, limits.image, limits.max_diagnostic_bytes).map_err(js_error)?;
                    VisualPresentation::Play { state, local }
                };
                self.visual = Some(replacement);
                self.current = None;
            }
            wire::WirePacket::Frame(frame) => {
                let Some(VisualPresentation::Play { state, .. }) = self.visual.as_mut() else { return Err(js_error("live state requires visual chart registration")); };
                state.apply_frame(&frame).map_err(js_error)?;
            }
            wire::WirePacket::Preview(preview) => {
                if preview.song_ns < 0 || preview.lookahead_ns <= 0 || preview.song_ns.checked_add(preview.lookahead_ns).is_none() { return Err(js_error("invalid preview song position or lookahead")); }
                let Some(VisualPresentation::Preview { song, lookahead, .. }) = self.visual.as_mut() else { return Err(js_error("preview state requires preview registration")); };
                *song = Timestamp::from_nanos(preview.song_ns);
                *lookahead = preview.lookahead_ns;
            }
            wire::WirePacket::History(model) => {
                let presentation = crate::historical_record_presentation::HistoricalRecordPresentation::import_visual(model).map_err(js_error)?;
                self.visual = Some(VisualPresentation::History(presentation));
                self.current = None;
            }
            wire::WirePacket::Results(model) => {
                let view = crate::ui::results::FrozenResultsView::from_model(model).map_err(js_error)?;
                self.visual = Some(VisualPresentation::Results { view, page: 0, comparisons: false, room: None });
                self.current = None;
            }
            wire::WirePacket::Room(model) => {
                model.validate().map_err(js_error)?;
                let page = model.initial_page;
                if combined_room {
                    let Some(VisualPresentation::Results { room, .. }) = self.visual.as_mut() else { unreachable!() };
                    *room = Some((model, page));
                } else { self.visual = Some(VisualPresentation::Room { model, page }); self.current = None; }
            }
        }
        self.visual_generation_floor = self.visual_generation_floor.max(header.generation);
        self.visual_identity = Some((header.generation, header.content, header.sequence));
        Ok(header.sequence)
    }

    /// Local cohorts may contain one P1. Mode is admitted independently of ID.
    pub fn set_visual_local(&mut self, local: bool) -> Result<(), JsValue> {
        let Some(render::VisualPresentation::Play { state, local: mode }) = self.visual.as_mut() else { return Err(js_error("local mode requires live visual registration")); };
        if !local && state.roster().len() != 1 { return Err(js_error("solo visual mode requires one member")); }
        *mode = local;
        Ok(())
    }
    pub fn set_visual_page(&mut self, requested: u32, comparisons: bool) -> Result<(), JsValue> {
        use render::VisualPresentation;
        match self.visual.as_mut() {
            Some(VisualPresentation::History(history)) if !comparisons => { history.set_grade_page(requested as usize).map_err(js_error)?; }
            Some(VisualPresentation::Results { view, page, comparisons: mode, .. }) => {
                if (comparisons && !view.has_comparisons()) || requested as usize >= view.page_count_for(comparisons) { return Err(js_error("invalid visual Results page/mode")); }
                *page = requested as usize; *mode = comparisons;
            }
            Some(VisualPresentation::Room { model, page }) if !comparisons => { model.project(requested as usize).map_err(js_error)?; *page = requested as usize; }
            _ => return Err(js_error("visual presentation does not support this page")),
        }
        Ok(())
    }
    pub fn set_visual_room_page(&mut self, requested: u32) -> Result<(), JsValue> {
        let Some(render::VisualPresentation::Results { room: Some((model, page)), .. }) = self.visual.as_mut() else { return Err(js_error("combined room Results unavailable")); };
        model.project(requested as usize).map_err(js_error)?; *page = requested as usize;
        Ok(())
    }
    /// Applied presentation page, independent of any requested UI page.
    pub fn visual_page(&self) -> Result<u32, JsValue> {
        use render::VisualPresentation;
        match self.visual.as_ref().ok_or_else(|| js_error("visual presentation not registered"))? {
            VisualPresentation::Preview { .. } => Ok(0),
            VisualPresentation::Play { state, local } => Ok(if *local { state.page() } else { 0 }),
            VisualPresentation::History(history) => u32::try_from(history.grade_page()).map_err(js_error),
            VisualPresentation::Results { page, .. } | VisualPresentation::Room { page, .. } => u32::try_from(*page).map_err(js_error),
        }
    }
    pub fn draw_visual(&mut self) -> Result<(), JsValue> {
        use render::VisualPresentation;
        match self.visual.as_ref().ok_or_else(|| js_error("visual presentation not registered"))? {
            VisualPresentation::Preview { chart, images, song, lookahead } => self.canvas.present_chart(chart, images, *song, *lookahead),
            VisualPresentation::Play { state, local } => self.canvas.present_visual(state, *local),
            VisualPresentation::History(history) => self.canvas.present_frozen_history(history),
            VisualPresentation::Results { view, page, comparisons, room } => {
                let room = room.as_ref().map(|(model, page)| model.project(*page)).transpose().map_err(js_error)?;
                self.canvas.present_frozen_results(view, *page, *comparisons, room)
            }
            VisualPresentation::Room { model, page } => self.canvas.present_room_results(model.project(*page).map_err(js_error)?),
        }.map_err(js_error)
    }
    pub fn retire_visual(&mut self) {
        self.visual = None;
        self.visual_identity = None;
        self.current = None;
    }
}
