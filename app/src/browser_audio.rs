//! Numeric separate-instance AudioWorklet binding; setup owns all allocations.
use crate::browser_native_clock::{NativeClock, NativeClockError};
use crate::worklet_audio::{
    WorkletAudio, WorkletAudioBuilder, WorkletAudioConfig, WorkletAudioError, WorkletRetainedConfig,
};
use beatkernel::{
    audio::{
        AudioCommand, AudioError, AudioFormat, AudioLimits, PRACTICE_RATE_PREPARATION_WORDS,
        PcmLimits, PracticeCue, PracticeLimits, PracticeRatePreparation, PracticeRegion,
        QueuePushError, SampleId, SourcePlaybackState, VoiceId,
    },
    time::{Duration, Timestamp},
    transport::Rate,
};
use wasm_bindgen::prelude::*;

/// Numeric status: 0 success; 1 invalid arguments/config/PCM; 2 setup state;
/// 3 queue full; 4 disconnected; 5 terminal; 6 context chronology;
/// 7 overflow; 8 Mixer render error; 9 setup allocation failure.
#[wasm_bindgen]
pub struct BrowserAudio {
    builder: Option<WorkletAudioBuilder>,
    audio: Option<WorkletAudio>,
    native_clock: Option<NativeClock>,
    status: u32,
    channels: u16,
    max_frames: u32,
    retained_setup: Option<RetainedSetup>,
    preparation: [u64; PRACTICE_RATE_PREPARATION_WORDS],
    preparation_next: usize,
    event_words: Option<[u32; 80]>,
}
struct RetainedSetup {
    config: WorkletRetainedConfig,
    cues: Vec<PracticeCue>,
    expected_cues: usize,
    expected_samples: usize,
}
fn audio_status(error: AudioError) -> u32 {
    match error {
        AudioError::AllocationFailed => 9,
        AudioError::Overflow => 7,
        _ => 1,
    }
}
fn worklet_status(error: WorkletAudioError) -> u32 {
    match error {
        WorkletAudioError::AlreadyArmed => 2,
        WorkletAudioError::StaleStart | WorkletAudioError::Chronology => 6,
        WorkletAudioError::Overflow => 7,
        WorkletAudioError::InvalidExtent => 1,
        WorkletAudioError::Render(_) => 8,
        WorkletAudioError::Failed => 5,
        WorkletAudioError::Practice(error) => match error {
            beatkernel::audio::PracticeError::Full
            | beatkernel::audio::PracticeError::ReceiptFull
            | beatkernel::audio::PracticeError::MetadataBusy => 3,
            beatkernel::audio::PracticeError::Disconnected => 4,
            beatkernel::audio::PracticeError::Overflow => 7,
            _ => 1,
        },
        WorkletAudioError::PendingControl => 3,
        WorkletAudioError::RetainedRequired => 2,
        WorkletAudioError::InvalidEvent
        | WorkletAudioError::StaleEvent
        | WorkletAudioError::InvalidRetentionPin => 1,
    }
}
fn native_clock_status(error: NativeClockError) -> u32 {
    match error {
        NativeClockError::Invalid => 1,
        NativeClockError::State => 2,
        NativeClockError::Chronology => 6,
        NativeClockError::Overflow => 7,
    }
}
fn frame_words(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}
impl BrowserAudio {
    fn apply_retained_preparation(
        &mut self,
        id: u64,
        generation: u64,
        revision: u64,
        restart: bool,
    ) -> u32 {
        let complete = self.preparation_next == PRACTICE_RATE_PREPARATION_WORDS;
        self.preparation_next = 0;
        self.status = if !complete {
            1
        } else {
            PracticeRatePreparation::from_words(self.preparation)
                .map_err(WorkletAudioError::Practice)
                .and_then(|preparation| {
                    let audio = self
                        .audio
                        .as_mut()
                        .ok_or(WorkletAudioError::RetainedRequired)?;
                    if restart {
                        audio.try_restart_original_next(id, generation, revision, preparation)
                    } else {
                        audio.try_set_rate_next(id, generation, revision, preparation)
                    }
                })
                .map_or_else(worklet_status, |()| 0)
        };
        self.status
    }
    fn finish_with_end(&mut self, end: Option<u64>) -> u32 {
        if self.retained_setup.is_some() {
            self.status = 2;
            return 2;
        }
        self.status = if let Some(builder) = self.builder.take() {
            let result = match end {
                Some(end) => builder.finish_at(end),
                None => builder.finish(),
            };
            match result {
                Ok(audio) => {
                    self.audio = Some(audio);
                    0
                }
                Err(error) => audio_status(error),
            }
        } else {
            2
        };
        self.status
    }
}
#[wasm_bindgen]
impl BrowserAudio {
    #[wasm_bindgen(constructor)]
    pub fn new(
        sample_rate: u32,
        channels: u16,
        max_asset_bytes: u32,
        max_total_bytes: u32,
        max_samples: u32,
        queue_capacity: u32,
        max_voices: u32,
        pending_capacity: u32,
        max_frames: u32,
        max_commands: u32,
    ) -> Self {
        let builder = (|| {
            WorkletAudioBuilder::new(WorkletAudioConfig {
                format: AudioFormat::new(sample_rate, channels)?,
                pcm_limits: PcmLimits::new(
                    max_asset_bytes as usize,
                    max_total_bytes as usize,
                    max_samples as usize,
                )?,
                audio_limits: AudioLimits::new(
                    queue_capacity as usize,
                    max_voices as usize,
                    pending_capacity as usize,
                    max_frames as usize,
                    max_commands as usize,
                )?,
            })
        })();
        let (builder, status) = match builder {
            Ok(builder) => (Some(builder), 0),
            Err(error) => (None, audio_status(error)),
        };
        Self {
            builder,
            audio: None,
            native_clock: None,
            status,
            channels,
            max_frames,
            retained_setup: None,
            preparation: [0; PRACTICE_RATE_PREPARATION_WORDS],
            preparation_next: 0,
            event_words: None,
        }
    }
    pub fn status(&self) -> u32 {
        self.status
    }
    /// Configure one native origin during setup; control metadata is not readiness.
    pub fn native_clock_start(
        &mut self,
        origin_low: u32,
        origin_high: u32,
        h0_low: u32,
        h0_high: u32,
        h1_low: u32,
        h1_high: u32,
        h2_low: u32,
        h2_high: u32,
    ) -> u32 {
        self.status = if self.builder.is_none() || self.native_clock.is_some() {
            2
        } else {
            match NativeClock::new(
                frame_words(origin_low, origin_high),
                [
                    frame_words(h0_low, h0_high),
                    frame_words(h1_low, h1_high),
                    frame_words(h2_low, h2_high),
                ],
            ) {
                Ok(clock) => {
                    self.native_clock = Some(clock);
                    0
                }
                Err(error) => native_clock_status(error),
            }
        };
        self.status
    }
    /// Commit all future lane horizons without changing the certified origin.
    pub fn native_clock_extend(
        &mut self,
        h0_low: u32,
        h0_high: u32,
        h1_low: u32,
        h1_high: u32,
        h2_low: u32,
        h2_high: u32,
    ) -> u32 {
        self.status = if let Some(clock) = self.native_clock.as_mut() {
            match clock.extend([
                frame_words(h0_low, h0_high),
                frame_words(h1_low, h1_high),
                frame_words(h2_low, h2_high),
            ]) {
                Ok(()) => 0,
                Err(error) => native_clock_status(error),
            }
        } else {
            2
        };
        self.status
    }
    /// Scalar candidate only: the adapter must validate every original sample.
    pub fn native_frame(
        &mut self,
        first0: u32,
        first1: u32,
        first2: u32,
        last0: u32,
        last1: u32,
        last2: u32,
        first_index: u32,
        frames: u32,
    ) -> f64 {
        let Some(clock) = self.native_clock.as_mut() else {
            self.status = 2;
            return -1.0;
        };
        match clock.frame(
            [first0, first1, first2],
            [last0, last1, last2],
            first_index,
            frames,
        ) {
            Ok(frame) => {
                self.status = 0;
                frame as f64
            }
            Err(error) => {
                self.status = native_clock_status(error);
                -1.0
            }
        }
    }
    pub fn native_clock_ready(&self) -> bool {
        self.native_clock.as_ref().is_some_and(NativeClock::ready)
    }
    /// Owned Float32Array transfer/import is setup-only; each source rate survives.
    pub fn insert_sample(&mut self, id: u64, rate: u32, channels: u16, pcm: Vec<f32>) -> u32 {
        if let (Some(setup), Some(builder)) = (&self.retained_setup, &self.builder) {
            if builder.sample_count() >= setup.expected_samples {
                self.status = 1;
                return 1;
            }
        }
        self.status = if let Some(builder) = self.builder.as_mut() {
            match AudioFormat::new(rate, channels)
                .and_then(|format| builder.insert_sample(SampleId(id), format, pcm))
            {
                Ok(()) => 0,
                Err(error) => audio_status(error),
            }
        } else {
            2
        };
        self.status
    }
    /// Cold, versioned 16-u64 setup carried as exact low/high pairs.
    pub fn retained_setup(&mut self, words: Vec<u32>) -> u32 {
        let result = (|| -> Result<RetainedSetup, WorkletAudioError> {
            let builder = self
                .builder
                .as_ref()
                .ok_or(WorkletAudioError::AlreadyArmed)?;
            if self.retained_setup.is_some() || words.len() != 32 {
                return Err(WorkletAudioError::InvalidExtent);
            }
            let mut values = [0u64; 16];
            for (value, pair) in values.iter_mut().zip(words.chunks_exact(2)) {
                *value = frame_words(pair[0], pair[1]);
            }
            if values[0] != 1 || values[1] == 0 || values[4] > 1 || values[8] > 1 {
                return Err(WorkletAudioError::InvalidExtent);
            }
            let rate = Rate::new(values[5] as i64, values[6])
                .map_err(|_| WorkletAudioError::InvalidExtent)?;
            if rate.numerator() <= 0
                || rate.numerator() as u64 != values[5]
                || rate.denominator() != values[6]
                || (values[7] as i64) < 0
            {
                return Err(WorkletAudioError::InvalidExtent);
            }
            let extent = |index| {
                usize::try_from(values[index]).map_err(|_| WorkletAudioError::InvalidExtent)
            };
            let expected_cues = extent(9)?;
            let expected_samples = extent(10)?;
            if builder.sample_count() > expected_samples
                || expected_samples > builder.sample_limit()
            {
                return Err(WorkletAudioError::InvalidExtent);
            }
            let config = WorkletRetainedConfig {
                program_id: values[1],
                initial_region: PracticeRegion::new(
                    Timestamp::from_nanos(values[2] as i64),
                    Timestamp::from_nanos(values[3] as i64),
                    values[4] == 1,
                )
                .map_err(WorkletAudioError::Practice)?,
                initial_rate: rate,
                output_preroll: Duration::from_nanos(values[7] as i64),
                open_end: values[8] == 1,
                limits: PracticeLimits::new(
                    expected_cues.max(1),
                    extent(11)?,
                    extent(12)?,
                    extent(13)?,
                    extent(14)?,
                )
                .map_err(WorkletAudioError::Practice)?,
                source_history_capacity: extent(15)?,
            };
            if config.source_history_capacity == 0 {
                return Err(WorkletAudioError::InvalidExtent);
            }
            let mut cues = Vec::new();
            cues.try_reserve_exact(expected_cues)
                .map_err(|_| WorkletAudioError::Render(AudioError::AllocationFailed))?;
            Ok(RetainedSetup {
                config,
                cues,
                expected_cues,
                expected_samples,
            })
        })();
        self.status = match result {
            Ok(setup) => {
                self.retained_setup = Some(setup);
                0
            }
            Err(error) => worklet_status(error),
        };
        self.status
    }
    pub fn retained_cue(&mut self, index: u32, voice: u64, sample: u64, at: i64, gain: f32) -> u32 {
        self.status = match (&mut self.retained_setup, &self.builder) {
            (Some(setup), Some(builder))
                if index as usize == setup.cues.len()
                    && setup.cues.len() < setup.expected_cues
                    && gain.is_finite()
                    && builder.has_sample(SampleId(sample))
                    && setup
                        .cues
                        .last()
                        .is_none_or(|cue| cue.at <= Timestamp::from_nanos(at)) =>
            {
                setup.cues.push(PracticeCue {
                    voice: VoiceId(voice),
                    sample: SampleId(sample),
                    at: Timestamp::from_nanos(at),
                    gain,
                });
                0
            }
            _ => 1,
        };
        self.status
    }
    pub fn finish_retained(&mut self) -> u32 {
        self.status = match (&self.builder, &self.retained_setup) {
            (Some(builder), Some(setup))
                if setup.cues.len() == setup.expected_cues
                    && builder.sample_count() == setup.expected_samples =>
            {
                0
            }
            _ => 2,
        };
        if self.status != 0 {
            return self.status;
        }
        let setup = self
            .retained_setup
            .take()
            .expect("validated retained setup");
        let builder = self.builder.take().expect("validated retained builder");
        self.status = match builder.finish_retained(setup.cues, setup.config) {
            Ok(audio) => {
                self.audio = Some(audio);
                0
            }
            Err(error) => worklet_status(error),
        };
        self.status
    }
    /// Stages exactly ten scalar words; no active slice or vector allocation.
    pub fn retained_preparation_word(&mut self, index: u32, low: u32, high: u32) -> u32 {
        let index = index as usize;
        if index == 0 {
            self.preparation_next = 0;
        }
        self.status = if self.audio.is_none()
            || index != self.preparation_next
            || index >= PRACTICE_RATE_PREPARATION_WORDS
        {
            1
        } else {
            self.preparation[index] = frame_words(low, high);
            self.preparation_next += 1;
            0
        };
        self.status
    }
    pub fn retained_rate(&mut self, id: u64, generation: u64, revision: u64) -> u32 {
        self.apply_retained_preparation(id, generation, revision, false)
    }
    pub fn retained_restart_original(&mut self, id: u64, generation: u64, revision: u64) -> u32 {
        self.apply_retained_preparation(id, generation, revision, true)
    }
    pub fn retained_region(
        &mut self,
        id: u64,
        generation: u64,
        revision: u64,
        start: i64,
        end: i64,
        repeat: bool,
    ) -> u32 {
        let result = PracticeRegion::new(
            Timestamp::from_nanos(start),
            Timestamp::from_nanos(end),
            repeat,
        )
        .map_err(WorkletAudioError::Practice)
        .and_then(|region| {
            self.audio
                .as_mut()
                .ok_or(WorkletAudioError::RetainedRequired)?
                .try_region_next(id, generation, revision, region)
        });
        self.status = result.map_or_else(worklet_status, |()| 0);
        self.status
    }
    pub fn retained_disable_loop(&mut self, id: u64, generation: u64, revision: u64) -> u32 {
        self.status = self
            .audio
            .as_mut()
            .ok_or(WorkletAudioError::RetainedRequired)
            .and_then(|audio| audio.try_disable_loop_next(id, generation, revision))
            .map_or_else(worklet_status, |()| 0);
        self.status
    }
    pub fn retained_pause(&mut self, id: u64, generation: u64, paused: bool) -> u32 {
        self.status = self
            .audio
            .as_mut()
            .ok_or(WorkletAudioError::RetainedRequired)
            .and_then(|audio| audio.request_pause(id, generation, paused))
            .map_or_else(worklet_status, |()| 0);
        self.status
    }
    /// Returns 1 for a latched fact, 0 for empty, -1 for failure (see status).
    pub fn retained_event_poll(&mut self) -> i32 {
        match self
            .audio
            .as_mut()
            .ok_or(WorkletAudioError::RetainedRequired)
            .and_then(WorkletAudio::poll_retained_event)
        {
            Ok(event) => {
                self.status = 0;
                self.event_words = event.map(|event| event.to_words());
                i32::from(self.event_words.is_some())
            }
            Err(error) => {
                self.status = worklet_status(error);
                -1
            }
        }
    }
    pub fn retained_event_word(&self, index: u32) -> u32 {
        self.event_words
            .as_ref()
            .and_then(|words| words.get(index as usize))
            .copied()
            .unwrap_or(0)
    }
    pub fn retained_event_ack(&mut self, low: u32, high: u32) -> u32 {
        self.status = self
            .audio
            .as_mut()
            .ok_or(WorkletAudioError::RetainedRequired)
            .and_then(|audio| audio.acknowledge_retained_event(frame_words(low, high)))
            .map_or_else(worklet_status, |()| {
                self.event_words = None;
                0
            });
        self.status
    }
    pub fn retained_metadata_word(&mut self, index: u32, high: bool) -> u32 {
        match self
            .audio
            .as_ref()
            .ok_or(WorkletAudioError::RetainedRequired)
            .and_then(WorkletAudio::retained_metadata)
        {
            Ok(metadata) => {
                self.status = 0;
                let value = match index {
                    0 => metadata.generation,
                    1 => metadata.revision,
                    2 => metadata.rate.numerator() as u64,
                    3 => metadata.rate.denominator(),
                    4 => u64::from(metadata.started),
                    5 => u64::from(metadata.active),
                    _ => {
                        self.status = 1;
                        return 0;
                    }
                };
                if high {
                    (value >> 32) as u32
                } else {
                    value as u32
                }
            }
            Err(error) => {
                self.status = worklet_status(error);
                0
            }
        }
    }
    pub fn retained_history_len(&self) -> u32 {
        self.audio
            .as_ref()
            .and_then(WorkletAudio::source_playback_history)
            .map_or(0, |history| history.len() as u32)
    }
    /// Header fields rate/initialPhysical/initialPlayback/retainedStart/pulledEnd/revision/capacity.
    pub fn retained_history_word(&self, index: u32, high: bool) -> u32 {
        let Some(history) = self
            .audio
            .as_ref()
            .and_then(WorkletAudio::source_playback_history)
        else {
            return 0;
        };
        let value = match index {
            0 => u64::from(history.source_rate()),
            1 => history.initial_physical_frame(),
            2 => history.initial_playback_frame(),
            3 => history.retained_start_frame(),
            4 => history.pulled_end_frame(),
            5 => history.retirement_revision(),
            6 => history.capacity() as u64,
            _ => return 0,
        };
        if high {
            (value >> 32) as u32
        } else {
            value as u32
        }
    }
    /// Span fields physicalStart/endPresent/end/playbackStart/running.
    pub fn retained_history_span_word(&self, span: u32, field: u32, high: bool) -> u32 {
        let Some(span) = self
            .audio
            .as_ref()
            .and_then(WorkletAudio::source_playback_history)
            .and_then(|history| history.span(span as usize))
        else {
            return 0;
        };
        let value = match field {
            0 => span.physical_start(),
            1 => u64::from(span.physical_end().is_some()),
            2 => span.physical_end().unwrap_or(0),
            3 => span.playback_start(),
            4 => u64::from(span.state() == SourcePlaybackState::Running),
            _ => return 0,
        };
        if high {
            (value >> 32) as u32
        } else {
            value as u32
        }
    }
    pub fn retained_history_retire(&mut self, low: u32, high: u32) -> u32 {
        self.status = self
            .audio
            .as_mut()
            .ok_or(WorkletAudioError::RetainedRequired)
            .and_then(|audio| audio.retire_source_playback_before(frame_words(low, high)))
            .map_or_else(worklet_status, |_| 0);
        self.status
    }
    pub fn finish(&mut self) -> u32 {
        self.finish_with_end(None)
    }
    pub fn finish_at(&mut self, end: u64) -> u32 {
        self.finish_with_end(Some(end))
    }
    pub fn arm(&mut self, start: u64, current: u64) -> u32 {
        self.status = if let Some(audio) = self.audio.as_mut() {
            match audio.arm(start, current) {
                Ok(()) => 0,
                Err(error) => worklet_status(error),
            }
        } else {
            2
        };
        self.status
    }
    /// Callback arm uses exact words and the unchanged core admission rules.
    pub fn arm_words(
        &mut self,
        start_low: u32,
        start_high: u32,
        current_low: u32,
        current_high: u32,
    ) -> u32 {
        self.arm(
            frame_words(start_low, start_high),
            frame_words(current_low, current_high),
        )
    }
    /// Kinds 0 Play, 1 Stop, 2 SetRate(value/denominator), 3 Seek(value).
    /// Admission success is not evidence of command execution or presentation.
    pub fn enqueue(
        &mut self,
        kind: u32,
        voice: u64,
        sample: u64,
        at: i64,
        gain: f32,
        value: i64,
        denominator: u64,
    ) -> u32 {
        let at = Timestamp::from_nanos(at);
        let command = match kind {
            0 => AudioCommand::Play {
                voice: VoiceId(voice),
                sample: SampleId(sample),
                at,
                gain,
            },
            1 => AudioCommand::Stop {
                voice: VoiceId(voice),
                at,
            },
            2 => match Rate::new(value, denominator) {
                Ok(rate) => AudioCommand::SetRate { rate, at },
                Err(_) => {
                    self.status = 1;
                    return 1;
                }
            },
            3 => AudioCommand::Seek {
                song_time: Timestamp::from_nanos(value),
                at,
            },
            _ => {
                self.status = 1;
                return 1;
            }
        };
        self.status = if let Some(audio) = self.audio.as_mut() {
            if audio.failed() {
                5
            } else {
                match audio.enqueue(command) {
                    Ok(()) => 0,
                    Err(error) => match error.reason {
                        QueuePushError::Full => 3,
                        QueuePushError::Disconnected => 4,
                    },
                }
            }
        } else {
            2
        };
        self.status
    }
    /// Callback ABI uses scalar words instead of per-quantum BigInt or slice copies.
    pub fn render(&mut self, frame_low: u32, frame_high: u32, frames: u32) -> u32 {
        let frame = frame_words(frame_low, frame_high);
        self.status = if let Some(audio) = self.audio.as_mut() {
            match audio.render(frame, frames as usize) {
                Ok(()) => 0,
                Err(error) => worklet_status(error),
            }
        } else {
            2
        };
        self.status
    }
    pub fn output_ptr(&self) -> usize {
        self.audio.as_ref().map_or(0, WorkletAudio::output_ptr)
    }
    pub fn output_len(&self) -> usize {
        self.audio.as_ref().map_or(0, WorkletAudio::output_len)
    }
    pub fn channels(&self) -> u16 {
        self.channels
    }
    pub fn max_frames(&self) -> u32 {
        self.max_frames
    }
    /// Obtain/rebuild the JS memory view during setup, never return PCM arrays.
    pub fn memory() -> JsValue {
        wasm_bindgen::memory()
    }
    #[wasm_bindgen(getter)]
    pub fn report_fields(&self) -> u32 {
        29
    }
    /// Fields 0 available; 1 start; 2 frames; 3 playback start; 4 playback frames;
    /// 5 paused; 6 endpoint present; 7 endpoint; 8 voices; 9 pending; 10 song;
    /// 11 disconnected; 12..22 actual counters in their original wire order;
    /// 23 context next present; 24 context next; 25 start present; 26 absolute start;
    /// 27 terminal; 28 commands expired. Signed song time is exposed as its two's-complement u64 bits.
    pub fn report_word(&self, index: u32, high: bool) -> u32 {
        let Some(audio) = &self.audio else {
            return 0;
        };
        let report = audio.report();
        let value = match index {
            0 => u64::from(report.is_some()),
            23 => u64::from(audio.context_frame().is_some()),
            24 => audio.context_frame().unwrap_or(0),
            25 => u64::from(audio.start_frame().is_some()),
            26 => audio.start_frame().unwrap_or(0),
            27 => u64::from(audio.failed()),
            _ => match report {
                None => 0,
                Some(r) => match index {
                    1 => r.start_frame,
                    2 => r.frames as u64,
                    3 => r.playback_start_frame,
                    4 => r.playback_frames as u64,
                    5 => u64::from(r.paused),
                    6 => u64::from(r.playback_end_physical_frame.is_some()),
                    7 => r.playback_end_physical_frame.unwrap_or(0),
                    8 => r.active_voices as u64,
                    9 => r.pending_commands as u64,
                    10 => r.song_position.as_nanos() as u64,
                    11 => u64::from(r.producer_disconnected),
                    12 => r.counters.rendered_frames,
                    13 => r.counters.commands_consumed,
                    14 => r.counters.commands_applied,
                    15 => r.counters.late_commands,
                    16 => r.counters.pending_full,
                    17 => r.counters.voice_full,
                    18 => r.counters.unknown_samples,
                    19 => r.counters.unknown_stops,
                    20 => r.counters.invalid_gains,
                    21 => r.counters.invalid_rates,
                    22 => r.counters.invalid_times,
                    28 => r.counters.commands_expired,
                    _ => 0,
                },
            },
        };
        if high {
            (value >> 32) as u32
        } else {
            value as u32
        }
    }
}
