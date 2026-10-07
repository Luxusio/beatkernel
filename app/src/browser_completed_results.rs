//! Worker-owned frozen completion presentation with bounded page metadata.
use crate::completed_results_presentation::CompletedResultsPresentation;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BrowserCompletedResults {
    pub(crate) presentation: CompletedResultsPresentation,
    page: u32,
    comparisons: bool,
}
impl BrowserCompletedResults {
    pub(crate) fn from_presentation(presentation: CompletedResultsPresentation) -> Option<Self> {
        presentation.results().is_some().then_some(Self {
            presentation,
            page: 0,
            comparisons: false,
        })
    }
}
#[wasm_bindgen]
impl BrowserCompletedResults {
    #[wasm_bindgen(getter)]
    pub fn page(&self) -> u32 {
        self.page
    }
    #[wasm_bindgen(getter)]
    pub fn comparisons(&self) -> bool {
        self.comparisons
    }
    #[wasm_bindgen(getter)]
    pub fn has_comparisons(&self) -> bool {
        self.presentation
            .view()
            .is_some_and(|view| view.has_comparisons())
    }
    #[wasm_bindgen(getter)]
    pub fn pages(&self) -> u32 {
        self.presentation
            .view()
            .map_or(0, |view| view.page_count_for(self.comparisons) as u32)
    }
    #[wasm_bindgen(getter)]
    pub fn detail_pages(&self) -> u32 {
        self.presentation
            .view()
            .map_or(0, |view| view.page_count_for(false) as u32)
    }
    #[wasm_bindgen(getter)]
    pub fn comparison_pages(&self) -> u32 {
        self.presentation
            .view()
            .filter(|view| view.has_comparisons())
            .map_or(0, |view| view.page_count_for(true) as u32)
    }
    #[wasm_bindgen(getter)]
    pub fn failed(&self) -> bool {
        self.presentation.view().is_none()
    }
    #[wasm_bindgen(getter)]
    pub fn error(&self) -> Option<String> {
        self.presentation.error().map(str::to_owned)
    }
    #[wasm_bindgen(getter)]
    pub fn players(&self) -> Vec<u32> {
        self.presentation
            .results()
            .unwrap_or(&[])
            .iter()
            .map(|(player, _)| player.0)
            .collect()
    }
    pub fn set_presentation(&mut self, page: u32, comparisons: bool) -> Result<(), JsValue> {
        let view = self
            .presentation
            .view()
            .ok_or_else(|| JsValue::from_str("completed Results display unavailable"))?;
        if (comparisons && !view.has_comparisons())
            || page as usize >= view.page_count_for(comparisons)
        {
            return Err(JsValue::from_str("invalid completed Results mode or page"));
        }
        self.page = page;
        self.comparisons = comparisons;
        Ok(())
    }
}
#[wasm_bindgen]
impl BrowserCompletedResults {
    pub fn visual_snapshot(&self, generation: u64, content: u64, max_packet_bytes: u32, max_diagnostic_bytes: u32) -> Result<Vec<u8>, JsValue> {
        let model = self.presentation.export_visual().map_err(|error| JsValue::from_str(&error))?.ok_or_else(|| JsValue::from_str("completed Results unavailable"))?;
        crate::browser::render::wire::encode_packet(crate::browser::render::header(crate::browser::render::wire::RESULTS, generation, content, 0), &crate::browser::render::wire::WirePacket::Results(model), crate::browser::render::limits(max_packet_bytes, max_diagnostic_bytes)).map_err(|error| JsValue::from_str(&error))
    }
}
