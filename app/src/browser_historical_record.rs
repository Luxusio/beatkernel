//! Worker-owned historical geometry, with no live game or completion proof.
use wasm_bindgen::prelude::*;
use crate::historical_record_presentation::HistoricalRecordPresentation;

#[wasm_bindgen]
pub struct BrowserHistoricalRecord {
    pub(crate) presentation: Option<HistoricalRecordPresentation>,
    error: Option<String>,
}
#[wasm_bindgen]
impl BrowserHistoricalRecord {
    #[wasm_bindgen(constructor)]
    pub fn new(replay: &[u8], archive: Option<Vec<u8>>, player: Option<u32>) -> Self {
        match HistoricalRecordPresentation::new(
            replay,
            archive.as_deref(),
            player.map(crate::local_players::PlayerId),
        ) {
            Ok(presentation) => Self {
                presentation,
                error: None,
            },
            Err(error) => Self {
                presentation: None,
                error: Some(error),
            },
        }
    }
    #[wasm_bindgen(getter)]
    pub fn available(&self) -> bool {
        self.presentation.is_some()
    }
    #[wasm_bindgen(getter)]
    pub fn grade_page(&self) -> u32 {
        self.presentation
            .as_ref()
            .map_or(0, |view| view.grade_page() as u32)
    }
    #[wasm_bindgen(getter)]
    pub fn grade_pages(&self) -> u32 {
        self.presentation
            .as_ref()
            .map_or(0, |view| view.grade_page_count() as u32)
    }
    pub fn set_grade_page(&mut self, page: u32) -> Result<(), JsValue> {
        self.presentation
            .as_mut()
            .ok_or_else(|| JsValue::from_str("historical presentation unavailable"))?
            .set_grade_page(page as usize)
            .map(|_| ())
            .map_err(|error| JsValue::from_str(&error))
    }
    #[wasm_bindgen(getter)]
    pub fn error(&self) -> Option<String> {
        self.error.clone()
    }
}
