//! Numeric separate-instance AudioWorklet binding; setup owns all allocations.
use crate::worklet_audio::{WorkletAudio, WorkletAudioBuilder, WorkletAudioConfig, WorkletAudioError};
use beatkernel::{
    audio::{
        AudioCommand, AudioError, AudioFormat, AudioLimits, PcmLimits, QueuePushError, SampleId,
        VoiceId,
    },
    time::Timestamp,
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
    status: u32,
    channels: u16,
    max_frames: u32,
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
    }
}
impl BrowserAudio {
    fn finish_with_end(&mut self, end: Option<u64>) -> u32 {
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
            status,
            channels,
            max_frames,
        }
    }
    pub fn status(&self) -> u32 {
        self.status
    }
    /// Owned Float32Array transfer/import is setup-only; each source rate survives.
    pub fn insert_sample(&mut self, id: u64, rate: u32, channels: u16, pcm: Vec<f32>) -> u32 {
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
        let frame = u64::from(frame_low) | (u64::from(frame_high) << 32);
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
    /// Fields 0 available; 1 start; 2 frames; 3 playback start; 4 playback frames;
    /// 5 paused; 6 endpoint present; 7 endpoint; 8 voices; 9 pending; 10 song;
    /// 11 disconnected; 12..22 actual counters in RenderReport declaration order;
    /// 23 context next present; 24 context next; 25 start present; 26 absolute start;
    /// 27 terminal. Signed song time is exposed as its two's-complement u64 bits.
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
