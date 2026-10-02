//! Preallocated actual Mixer ownership on an absolute browser context frame grid.
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
        let mixer = Mixer::new(
            MixerConfig::new(
                self.config.format,
                ClockDomainId(1),
                Timestamp::ZERO,
                self.config.audio_limits,
            ),
            self.bank,
            consumer,
        )?;
        Ok(WorkletAudio {
            producer,
            mixer,
            output,
            config: self.config,
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
    pub fn failed(&self) -> bool {
        self.failed
    }
}
