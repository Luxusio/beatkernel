//! Selected-file and canvas bindings. Called serially by the browser Worker.

use std::sync::Arc;

use beatkernel::{
    audio::{AudioFormat, PcmLimits},
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

const LOOKAHEAD_NS: i64 = 2_000_000_000;

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
            chart,
            images,
            chart_seed: seed,
        })
    }
}

/// Owns resources once; moving into BrowserView does not clone PCM or indexes.
#[wasm_bindgen]
pub struct BrowserPrepared {
    pub(crate) prepared: PreparedBms,
    pub(crate) chart: PlayerChart,
    pub(crate) images: Arc<ImageAssets>,
    pub(crate) chart_seed: u64,
}

#[wasm_bindgen]
impl BrowserPrepared {
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

    pub fn lanes(&self) -> Vec<u8> {
        self.chart.lanes.clone()
    }
}

/// GPU and chart ownership in the dedicated Worker, separate from DOM controls.
#[wasm_bindgen]
pub struct BrowserView {
    canvas: BrowserCanvas,
    current: Option<BrowserPrepared>,
    song: Timestamp,
}

#[wasm_bindgen]
impl BrowserView {
    pub async fn create(canvas: web_sys::OffscreenCanvas) -> Result<BrowserView, JsValue> {
        Ok(Self {
            canvas: BrowserCanvas::create(canvas).await.map_err(js_error)?,
            current: None,
            song: Timestamp::ZERO,
        })
    }

    pub fn set_chart(&mut self, prepared: BrowserPrepared) {
        self.current = Some(prepared);
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
                .present_chart(&current.chart, &current.images, self.song, LOOKAHEAD_NS)
                .map_err(js_error)?;
        }
        Ok(())
    }

    pub fn draw_game(&mut self, game: &crate::browser_game::BrowserGame) -> Result<(), JsValue> {
        self.canvas
            .present_game(game, LOOKAHEAD_NS)
            .map_err(js_error)
    }

    pub fn needs_redraw(&self) -> bool {
        self.current.is_some() && self.canvas.needs_redraw()
    }
}
