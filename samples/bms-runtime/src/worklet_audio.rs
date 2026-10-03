//! Preallocated actual Mixer ownership on an absolute browser context frame grid.
#[cfg(test)]
#[path = "browser_output_fixtures.rs"]
mod output_fixtures;

#[cfg(any(test, all(target_arch = "wasm32", feature = "browser")))]
use beatkernel::audio::AudioCounters;
use beatkernel::{
    audio::{
        AudioCommand, AudioError, AudioFormat, AudioLimits, CommandProducer, CommandPushError,
        Mixer, MixerConfig, PcmLimits, PcmSample, QueuePushError, RenderReport, SampleBank,
        SampleId, command_queue,
    },
    time::{ClockDomainId, Timestamp},
};
#[derive(Clone, Copy, Debug)]
pub struct WorkletAudioConfig {
    pub format: AudioFormat,
    pub pcm_limits: PcmLimits,
    pub audio_limits: AudioLimits,
}
pub struct WorkletAudioBuilder {
    config: WorkletAudioConfig,
    bank: SampleBank,
}
impl WorkletAudioBuilder {
    pub fn new(config: WorkletAudioConfig) -> Result<Self, AudioError> {
        Ok(Self {
            bank: SampleBank::new(config.format, config.pcm_limits)?,
            config,
        })
    }
    pub fn insert_sample(
        &mut self,
        id: SampleId,
        format: AudioFormat,
        samples: Vec<f32>,
    ) -> Result<(), AudioError> {
        self.bank
            .insert(id, PcmSample::new(format, samples, self.config.pcm_limits)?)
    }
    pub fn finish(self) -> Result<WorkletAudio, AudioError> {
        self.finish_with_end(None)
    }
    /// Fence playback at an immutable relative Mixer frame after prestart silence.
    pub fn finish_at(self, end: u64) -> Result<WorkletAudio, AudioError> {
        self.finish_with_end(Some(end))
    }
    fn finish_with_end(self, playback_end_frame: Option<u64>) -> Result<WorkletAudio, AudioError> {
        let length = self
            .config
            .audio_limits
            .max_render_frames()
            .checked_mul(usize::from(self.config.format.channels()))
            .ok_or(AudioError::Overflow)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|_| AudioError::AllocationFailed)?;
        output.resize(length, 0.0);
        let (producer, consumer) = command_queue(self.config.audio_limits.queue_capacity())?;
        let mixer_config = MixerConfig::new(
            self.config.format,
            ClockDomainId(1),
            Timestamp::ZERO,
            self.config.audio_limits,
        );
        let mixer_config = match playback_end_frame {
            Some(end) => mixer_config.with_playback_end_frame(end),
            None => mixer_config,
        };
        let mixer = Mixer::new(mixer_config, self.bank, consumer)?;
        Ok(WorkletAudio {
            producer,
            mixer,
            output,
            config: self.config,
            playback_end_frame,
            start: None,
            armed_current: None,
            expected: None,
            failed: false,
            report: None,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkletAudioError {
    AlreadyArmed,
    StaleStart,
    Chronology,
    Overflow,
    InvalidExtent,
    Render(AudioError),
    Failed,
}
impl std::fmt::Display for WorkletAudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for WorkletAudioError {}

pub struct WorkletAudio {
    producer: CommandProducer,
    mixer: Mixer,
    output: Vec<f32>,
    config: WorkletAudioConfig,
    playback_end_frame: Option<u64>,
    start: Option<u64>,
    armed_current: Option<u64>,
    expected: Option<u64>,
    failed: bool,
    report: Option<RenderReport>,
}
impl WorkletAudio {
    /// One immutable absolute start; failed admission does not alter the owner.
    pub fn arm(&mut self, start: u64, current: u64) -> Result<(), WorkletAudioError> {
        if self.failed {
            return Err(WorkletAudioError::Failed);
        }
        if self.start.is_some() {
            return Err(WorkletAudioError::AlreadyArmed);
        }
        if self.expected.is_some_and(|expected| expected != current) {
            return Err(WorkletAudioError::Chronology);
        }
        if start < current {
            return Err(WorkletAudioError::StaleStart);
        }
        if self
            .playback_end_frame
            .is_some_and(|end| start.checked_add(end).is_none())
        {
            return Err(WorkletAudioError::Overflow);
        }
        self.start = Some(start);
        self.armed_current = Some(current);
        Ok(())
    }
    pub fn enqueue(&mut self, command: AudioCommand) -> Result<(), CommandPushError> {
        if self.failed {
            return Err(CommandPushError {
                command,
                reason: QueuePushError::Disconnected,
            });
        }
        self.producer.try_push(command)
    }
    fn fail(&mut self, error: WorkletAudioError) -> WorkletAudioError {
        self.output.fill(0.0);
        self.failed = true;
        error
    }
    /// Valid nonempty context blocks must be contiguous. Failure permanently
    /// fences this owner and clears its entire fixed output, without dropping PCM.
    pub fn render(&mut self, current: u64, frames: usize) -> Result<(), WorkletAudioError> {
        if self.failed {
            return Err(self.fail(WorkletAudioError::Failed));
        }
        if frames == 0 {
            return Ok(());
        }
        if frames > self.max_frames() {
            return Err(self.fail(WorkletAudioError::InvalidExtent));
        }
        let extent = u64::try_from(frames).map_err(|_| self.fail(WorkletAudioError::Overflow))?;
        let end = current
            .checked_add(extent)
            .ok_or_else(|| self.fail(WorkletAudioError::Overflow))?;
        if self.expected.is_some_and(|expected| expected != current)
            || self.armed_current.is_some_and(|minimum| current < minimum)
        {
            return Err(self.fail(WorkletAudioError::Chronology));
        }
        let prefix = match self.start {
            None => frames,
            Some(start) if self.report.is_none() && current > start => {
                return Err(self.fail(WorkletAudioError::StaleStart));
            }
            Some(start) => usize::try_from(start.saturating_sub(current).min(extent))
                .map_err(|_| self.fail(WorkletAudioError::Overflow))?,
        };
        let channels = usize::from(self.channels());
        // Only this block is output. The Mixer clears its active suffix, so a
        // large configured capacity does not add a full-capacity fill per call.
        self.output[..prefix * channels].fill(0.0);
        if prefix < frames {
            let first = prefix * channels;
            let last = frames * channels;
            match self.mixer.render(&mut self.output[first..last]) {
                Ok(report) => self.report = Some(report),
                Err(error) => return Err(self.fail(WorkletAudioError::Render(error))),
            }
        }
        self.expected = Some(end);
        Ok(())
    }
    /// Fixed backing storage. Only the latest successful block's extent is
    /// valid output; unused capacity is not a newly rendered silent block.
    pub fn output(&self) -> &[f32] {
        &self.output
    }
    pub fn output_ptr(&self) -> usize {
        self.output.as_ptr() as usize
    }
    pub fn output_len(&self) -> usize {
        self.output.len()
    }
    pub fn channels(&self) -> u16 {
        self.config.format.channels()
    }
    pub fn max_frames(&self) -> usize {
        self.config.audio_limits.max_render_frames()
    }
    pub fn report(&self) -> Option<RenderReport> {
        self.report
    }
    pub fn context_frame(&self) -> Option<u64> {
        self.expected
    }
    pub fn start_frame(&self) -> Option<u64> {
        self.start
    }
    /// Immutable endpoint on the Mixer grid, excluding Worklet prestart silence.
    pub fn playback_end_frame(&self) -> Option<u64> {
        self.playback_end_frame
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
}

#[cfg(any(test, all(target_arch = "wasm32", feature = "browser")))]
pub(crate) struct OutputEvidence {
    pub(crate) report: Option<RenderReport>,
    pub(crate) start: u64,
    pub(crate) context: Option<u64>,
}

/// Decode the actual BrowserAudio::report_word ABI without Number conversion.
/// These capacities are the browser player's explicit Worklet configuration;
/// the first active suffix and later variable nonempty buffer lengths are valid.
#[cfg(any(test, all(target_arch = "wasm32", feature = "browser")))]
pub(crate) fn decode_output(words: &[u32]) -> Result<OutputEvidence, &'static str> {
    if words.len() != 56 {
        return Err("browser output report requires exactly 56 words");
    }
    let mut values = [0u64; 28];
    for (value, pair) in values.iter_mut().zip(words.chunks_exact(2)) {
        *value = u64::from(pair[0]) | (u64::from(pair[1]) << 32);
    }
    if [0, 5, 6, 11, 23, 25, 27]
        .into_iter()
        .any(|index| values[index] > 1)
    {
        return Err("browser output report contains an invalid boolean flag");
    }
    if values[27] != 0 || values[25] != 1 {
        return Err("browser output is terminal or has no armed start");
    }
    if values[23] == 0 && values[24] != 0 {
        return Err("browser output absent context cursor is nonzero");
    }
    let start = values[26];
    let context = (values[23] == 1).then_some(values[24]);
    if values[0] == 0 {
        if values[1..23].iter().any(|&value| value != 0)
            || context.is_some_and(|cursor| cursor > start)
        {
            return Err("browser unavailable output report contains render evidence");
        }
        return Ok(OutputEvidence {
            report: None,
            start,
            context,
        });
    }
    if !(1..=4096).contains(&values[2]) || values[8] > 4096 || values[9] > 4096 {
        return Err("browser output report exceeds configured render or mixer capacity");
    }
    if values[1] != values[3]
        || values[2] != values[4]
        || values[5] != 0
        || values[6] != 0
        || values[7] != 0
        || values[11] != 0
    {
        return Err(
            "browser output requires connected unlimited unpaused playback on its original grid",
        );
    }
    let end = values[1]
        .checked_add(values[2])
        .ok_or("browser output render cursor overflow")?;
    let absolute_end = start
        .checked_add(end)
        .ok_or("browser output context cursor overflow")?;
    if context != Some(absolute_end) {
        return Err("browser output context cursor differs from its relative mixer grid");
    }
    let report = RenderReport {
        start_frame: values[1],
        frames: values[2] as usize,
        playback_start_frame: values[3],
        playback_frames: values[4] as usize,
        paused: false,
        playback_end_physical_frame: None,
        active_voices: values[8] as usize,
        pending_commands: values[9] as usize,
        song_position: Timestamp::from_nanos(values[10] as i64),
        producer_disconnected: false,
        counters: AudioCounters {
            rendered_frames: values[12],
            commands_consumed: values[13],
            commands_applied: values[14],
            late_commands: values[15],
            pending_full: values[16],
            voice_full: values[17],
            unknown_samples: values[18],
            unknown_stops: values[19],
            invalid_gains: values[20],
            invalid_rates: values[21],
            invalid_times: values[22],
        },
    };
    Ok(OutputEvidence {
        report: Some(report),
        start,
        context,
    })
}
