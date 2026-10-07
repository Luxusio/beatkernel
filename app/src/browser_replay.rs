//! Worker-owned canonical replay; audio remains in the separate Worklet owner.
use std::{collections::VecDeque, sync::Arc};

use crate::{
    browser::BrowserPrepared,
    browser_game::{BrowserSample, OUTPUT, encode_batch},
    image_assets::ImageAssets,
    note_progress::NoteProgress,
    player_chart::PlayerChart,
    step_replay::{StepReplay, StepReplayConfig, StepReplayError},
    worklet_audio::decode_section_output,
};
use beatkernel::{
    audio::{PcmSample, SampleId},
    judge::JudgeEvent,
    time::{ClockPoint, Duration, Timestamp},
};
use wasm_bindgen::prelude::*;

fn error(value: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&value.to_string()).into()
}
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: OUTPUT,
        timestamp: Timestamp::from_nanos(ns),
    }
}

/// Consumes a prepared canonical replay once. No live input or host-clock
/// projection is accepted; reported output presentation advances recorded work.
#[wasm_bindgen]
pub struct BrowserReplay {
    pub(crate) replay: StepReplay,
    pub(crate) chart: Arc<PlayerChart>,
    pub(crate) images: Arc<ImageAssets>,
    pub(crate) progress: NoteProgress,
    pub(crate) recent: Vec<JudgeEvent>,
    samples: VecDeque<(SampleId, PcmSample)>,
    output_start: Option<u64>,
    output_context: Option<u64>,
}
#[wasm_bindgen]
impl BrowserReplay {
    #[wasm_bindgen(constructor)]
    pub fn new(mut prepared: BrowserPrepared, preroll_ns: i64) -> Result<Self, JsValue> {
        let file = prepared
            .replay
            .take()
            .ok_or_else(|| error("prepare a canonical replay before replay construction"))?;
        let limits = crate::competition_live::replay_limits().map_err(error)?;
        let chart = Arc::new(prepared.chart);
        let progress = NoteProgress::new(chart.clone()).map_err(error)?;
        let (replay, bank) = StepReplay::new(
            prepared.prepared,
            file,
            limits,
            StepReplayConfig {
                output_origin: point(0),
                preroll: Duration::from_nanos(preroll_ns),
                lookahead: Duration::from_nanos(500_000_000),
                max_pending: 4096,
            },
        )
        .map_err(error)?;
        Ok(Self {
            replay,
            chart,
            images: prepared.images,
            progress,
            recent: Vec::with_capacity(128),
            samples: bank.into_samples().collect(),
            output_start: None,
            output_context: None,
        })
    }
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
    pub fn next_sample(&mut self) -> Option<BrowserSample> {
        self.samples
            .pop_front()
            .map(|(id, sample)| BrowserSample::from_pcm(id, sample))
    }
    pub fn commands(&mut self, max: u32) -> Result<JsValue, JsValue> {
        let Some(batch) = self.replay.take_commands(max as usize).map_err(error)? else {
            return Ok(JsValue::NULL);
        };
        let result = encode_batch(batch);
        if result.is_err() {
            self.replay.fail();
        }
        result
    }
    pub fn acknowledge(
        &mut self,
        sequence: u64,
        admitted: u32,
        success: bool,
    ) -> Result<(), JsValue> {
        self.replay
            .acknowledge(sequence, admitted as usize, success)
            .map_err(error)
    }
    /// Both report and presentation are genuine observations on one immutable
    /// armed grid. An unavailable presentation never becomes a UI-time cursor.
    pub fn observe_output(
        &mut self,
        words: Vec<u32>,
        presented_ns: Option<i64>,
    ) -> Result<bool, JsValue> {
        if self.replay.failed() {
            return Err(error("replay owner is fenced"));
        }
        let evidence =
            decode_section_output(&words, self.replay.playback_end_frame()).map_err(|reason| {
                self.replay.fail();
                error(reason)
            })?;
        if self
            .output_start
            .is_some_and(|start| start != evidence.start)
            || self
                .output_context
                .is_some_and(|previous| evidence.context.is_none_or(|current| current < previous))
        {
            self.replay.fail();
            return Err(error(
                "replay output start changed or context cursor regressed",
            ));
        }
        let result = self
            .replay
            .observe_output(evidence.report, presented_ns.map(point));
        let failed_events = match &result {
            Err(StepReplayError::Progress { events, .. }) => events.as_slice(),
            _ => &[],
        };
        for event in self
            .replay
            .drain_events()
            .into_iter()
            .chain(failed_events.iter().copied())
        {
            self.progress.apply(std::slice::from_ref(&event));
            if self.recent.len() == 128 {
                self.recent.remove(0);
            }
            self.recent.push(event);
        }
        let completed = result.map_err(error)?;
        self.output_start = Some(evidence.start);
        self.output_context = evidence.context;
        Ok(completed)
    }
    pub fn stop(&mut self) {
        self.replay.fail();
        self.samples.clear();
    }
    #[wasm_bindgen(getter)]
    pub fn song_ns(&self) -> i64 {
        self.replay.song_time().as_nanos()
    }
    #[wasm_bindgen(getter)]
    pub fn recorded_until_ns(&self) -> Option<i64> {
        self.replay.recorded_until().map(|time| time.as_nanos())
    }
    #[wasm_bindgen(getter)]
    pub fn end_ns(&self) -> Option<i64> {
        self.replay.end_ns()
    }
    #[wasm_bindgen(getter)]
    pub fn playback_end_frame(&self) -> Option<u64> {
        self.replay.playback_end_frame()
    }
    #[wasm_bindgen(getter)]
    pub fn hits(&self) -> u64 {
        self.replay.score().hits
    }
    #[wasm_bindgen(getter)]
    pub fn misses(&self) -> u64 {
        self.replay.score().misses
    }
    #[wasm_bindgen(getter)]
    pub fn combo(&self) -> u64 {
        self.replay.score().combo
    }
    #[wasm_bindgen(getter)]
    pub fn failed(&self) -> bool {
        self.replay.failed()
    }
}
