//! Preallocated actual Mixer ownership on an absolute browser context frame grid.
#[cfg(test)]
#[path = "browser_output_fixtures.rs"]
mod output_fixtures;

#[cfg(any(test, all(target_arch = "wasm32", feature = "browser")))]
use beatkernel::audio::AudioCounters;
use beatkernel::{
    audio::{
        AudioCommand, AudioError, AudioFormat, AudioLimits, CommandProducer, CommandPushError,
        CommandScope, Mixer, MixerConfig, PcmLimits, PcmSample, PracticeBoundaryKind,
        PracticeController, PracticeCue, PracticeError, PracticeEvent, PracticeLimits,
        PracticeMetadata, PracticeRateEvent, PracticeRateOutcome, PracticeRatePreparation,
        PracticeRateRefusal, PracticeReceipt, PracticeRegion, PracticeRestartEvent,
        PreparedPracticeProgram, ProjectedPracticeEvent, QueuePushError, RenderReport, SampleBank,
        SampleId, SourcePlaybackHistory, TargetBoundary, TargetTime, command_queue, practice_queue,
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::{PositiveRatePhaseError, Rate},
};
#[derive(Clone, Copy, Debug)]
pub struct WorkletAudioConfig {
    pub format: AudioFormat,
    pub pcm_limits: PcmLimits,
    pub audio_limits: AudioLimits,
}
#[derive(Clone, Copy, Debug)]
pub struct WorkletRetainedConfig {
    pub program_id: u64,
    pub initial_region: PracticeRegion,
    pub initial_rate: Rate,
    pub output_preroll: Duration,
    pub open_end: bool,
    pub limits: PracticeLimits,
    pub source_history_capacity: usize,
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
    #[cfg(all(target_arch = "wasm32", feature = "browser-audio"))]
    pub(crate) fn sample_count(&self) -> usize {
        self.bank.len()
    }
    #[cfg(all(target_arch = "wasm32", feature = "browser-audio"))]
    pub(crate) fn sample_limit(&self) -> usize {
        self.config.pcm_limits.max_samples()
    }
    #[cfg(all(target_arch = "wasm32", feature = "browser-audio"))]
    pub(crate) fn has_sample(&self, id: SampleId) -> bool {
        self.bank.get(id).is_some()
    }
    pub fn finish(self) -> Result<WorkletAudio, AudioError> {
        self.finish_with_end(None)
    }
    /// Fence playback at an immutable relative Mixer frame after prestart silence.
    pub fn finish_at(self, end: u64) -> Result<WorkletAudio, AudioError> {
        self.finish_with_end(Some(end))
    }
    pub fn finish_retained(
        self,
        cues: Vec<PracticeCue>,
        config: WorkletRetainedConfig,
    ) -> Result<WorkletAudio, WorkletAudioError> {
        let program = PreparedPracticeProgram::new_with_runtime_rates(
            &self.bank,
            cues,
            config.limits,
            config.initial_rate,
            config.output_preroll,
            config.open_end,
            config.program_id,
        )
        .map_err(WorkletAudioError::Practice)?;
        let (controller, endpoint) =
            practice_queue(&program).map_err(WorkletAudioError::Practice)?;
        let mut audio = self
            .finish_with_end(None)
            .map_err(WorkletAudioError::Render)?;
        audio
            .mixer
            .install_practice(program, endpoint, config.initial_region)
            .map_err(WorkletAudioError::Practice)?;
        audio
            .mixer
            .enable_source_playback_history(config.source_history_capacity)
            .map_err(WorkletAudioError::Render)?;
        audio.retained = Some(RetainedAudio {
            config,
            controller,
            pending_control: None,
            pending_pause: None,
            pause_event: None,
            practice_event: None,
            latch: None,
            serial: 0,
            last_pause_id: 0,
        });
        Ok(audio)
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
            retained: None,
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
    Practice(PracticeError),
    RetainedRequired,
    PendingControl,
    InvalidEvent,
    StaleEvent,
    InvalidRetentionPin,
}
impl std::fmt::Display for WorkletAudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for WorkletAudioError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkletPauseEvent {
    pub request_id: u64,
    pub generation: u64,
    pub physical_frame: u64,
    pub playback_frame: u64,
    pub paused: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkletRetainedEvent {
    Practice(ProjectedPracticeEvent),
    Pause(WorkletPauseEvent),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkletRetainedEnvelope {
    pub program_id: u64,
    pub context_start: u64,
    pub serial: u64,
    pub event: WorkletRetainedEvent,
}
struct RetainedAudio {
    config: WorkletRetainedConfig,
    controller: PracticeController,
    pending_control: Option<u64>,
    pending_pause: Option<(u64, u64, bool)>,
    pause_event: Option<WorkletPauseEvent>,
    practice_event: Option<ProjectedPracticeEvent>,
    latch: Option<WorkletRetainedEnvelope>,
    serial: u64,
    last_pause_id: u64,
}
impl RetainedAudio {
    fn admit(&self, generation: u64, revision: u64) -> Result<(), WorkletAudioError> {
        if self.pending_control.is_some()
            || self.latch.is_some()
            || self.practice_event.is_some()
            || self.pause_event.is_some()
            || self.pending_pause.is_some()
            || self.controller.history_retention_pins().0.is_some()
        {
            return Err(WorkletAudioError::PendingControl);
        }
        let metadata = self
            .controller
            .metadata()
            .map_err(WorkletAudioError::Practice)?;
        if metadata.generation != generation {
            return Err(WorkletAudioError::Practice(PracticeError::StaleGeneration));
        }
        if metadata.revision != revision {
            return Err(WorkletAudioError::Practice(
                PracticeError::InvalidRatePreparation,
            ));
        }
        Ok(())
    }
}

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
    retained: Option<RetainedAudio>,
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
        if self.expected.is_some_and(|expected| current < expected) {
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
        self.expected = Some(current);
        Ok(())
    }
    pub fn enqueue(&mut self, command: AudioCommand) -> Result<(), CommandPushError> {
        if self.failed {
            return Err(CommandPushError {
                command,
                reason: QueuePushError::Disconnected,
            });
        }
        if let Some(retained) = &self.retained {
            match retained.controller.metadata() {
                Ok(metadata) => self.producer.set_scope(CommandScope(metadata.generation)),
                Err(_) => {
                    return Err(CommandPushError {
                        command,
                        reason: QueuePushError::Full,
                    });
                }
            }
        }
        self.producer.try_push(command)
    }
    fn fail(&mut self, error: WorkletAudioError) -> WorkletAudioError {
        self.output.fill(0.0);
        self.failed = true;
        error
    }
    /// Unarmed nonempty blocks must not overlap; armed blocks must be contiguous.
    /// Failure permanently fences this owner and clears its entire fixed output,
    /// without dropping PCM.
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
        if self.expected.is_some_and(|expected| {
            if self.start.is_some() {
                current != expected
            } else {
                current < expected
            }
        }) || self.armed_current.is_some_and(|minimum| current < minimum)
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
                Ok(report) => {
                    if let Some(retained) = &mut self.retained {
                        if let Some((request_id, generation, paused)) = retained.pending_pause {
                            if report.paused == paused {
                                retained.pause_event = Some(WorkletPauseEvent {
                                    request_id,
                                    generation,
                                    physical_frame: report.start_frame,
                                    playback_frame: report.playback_start_frame,
                                    paused,
                                });
                                retained.pending_pause = None;
                            }
                        }
                    }
                    self.report = Some(report);
                }
                Err(error)
                    if self.retained.is_some()
                        && matches!(
                            error,
                            AudioError::RenderCapacity
                                | AudioError::PracticeReceiptFull
                                | AudioError::PracticeEventBudget
                        ) =>
                {
                    self.output.fill(0.0);
                    return Err(WorkletAudioError::Render(error));
                }
                Err(error) => return Err(self.fail(WorkletAudioError::Render(error))),
            }
        }
        self.expected = Some(end);
        Ok(())
    }
    fn retained_mut(&mut self) -> Result<&mut RetainedAudio, WorkletAudioError> {
        if self.failed {
            return Err(WorkletAudioError::Failed);
        }
        self.retained
            .as_mut()
            .ok_or(WorkletAudioError::RetainedRequired)
    }
    pub fn retained_metadata(&self) -> Result<PracticeMetadata, WorkletAudioError> {
        self.retained
            .as_ref()
            .ok_or(WorkletAudioError::RetainedRequired)?
            .controller
            .metadata()
            .map_err(WorkletAudioError::Practice)
    }
    pub fn try_set_rate_next(
        &mut self,
        id: u64,
        generation: u64,
        revision: u64,
        preparation: PracticeRatePreparation,
    ) -> Result<(), WorkletAudioError> {
        let retained = self.retained_mut()?;
        retained.admit(generation, revision)?;
        let token = retained
            .controller
            .bind_rate_preparation(generation, revision, preparation)
            .map_err(WorkletAudioError::Practice)?;
        retained
            .controller
            .try_set_rate_next(id, generation, &token)
            .map_err(WorkletAudioError::Practice)?;
        retained.pending_control = Some(id);
        Ok(())
    }
    pub fn try_region_next(
        &mut self,
        id: u64,
        generation: u64,
        revision: u64,
        region: PracticeRegion,
    ) -> Result<(), WorkletAudioError> {
        let retained = self.retained_mut()?;
        retained.admit(generation, revision)?;
        retained
            .controller
            .try_request_next(id, generation, region)
            .map_err(WorkletAudioError::Practice)?;
        retained.pending_control = Some(id);
        Ok(())
    }
    pub fn try_disable_loop_next(
        &mut self,
        id: u64,
        generation: u64,
        revision: u64,
    ) -> Result<(), WorkletAudioError> {
        let retained = self.retained_mut()?;
        retained.admit(generation, revision)?;
        retained
            .controller
            .try_disable_loop_next(id, generation)
            .map_err(WorkletAudioError::Practice)?;
        retained.pending_control = Some(id);
        Ok(())
    }
    pub fn try_restart_original_next(
        &mut self,
        id: u64,
        generation: u64,
        revision: u64,
        preparation: PracticeRatePreparation,
    ) -> Result<(), WorkletAudioError> {
        let retained = self.retained_mut()?;
        retained.admit(generation, revision)?;
        let words = preparation.to_words();
        if words[4] != retained.config.initial_rate.numerator() as u64
            || words[5] != retained.config.initial_rate.denominator()
        {
            return Err(WorkletAudioError::Practice(
                PracticeError::InvalidRatePreparation,
            ));
        }
        let token = retained
            .controller
            .bind_rate_preparation(generation, revision, preparation)
            .map_err(WorkletAudioError::Practice)?;
        retained
            .controller
            .try_restart_next(id, generation, retained.config.initial_region, &token)
            .map_err(WorkletAudioError::Practice)?;
        retained.pending_control = Some(id);
        Ok(())
    }
    pub fn request_pause(
        &mut self,
        id: u64,
        generation: u64,
        paused: bool,
    ) -> Result<(), WorkletAudioError> {
        let retained = self.retained_mut()?;
        if id == 0 || id <= retained.last_pause_id {
            return Err(WorkletAudioError::StaleEvent);
        }
        if retained.pending_pause.is_some()
            || retained.pause_event.is_some()
            || matches!(
                retained.latch,
                Some(WorkletRetainedEnvelope {
                    event: WorkletRetainedEvent::Pause(_),
                    ..
                })
            )
        {
            return Err(WorkletAudioError::PendingControl);
        }
        if retained
            .controller
            .metadata()
            .map_err(WorkletAudioError::Practice)?
            .generation
            != generation
        {
            return Err(WorkletAudioError::Practice(PracticeError::StaleGeneration));
        }
        retained.pending_pause = Some((id, generation, paused));
        retained.last_pause_id = id;
        self.producer.request_pause(paused);
        Ok(())
    }
    pub fn poll_retained_event(
        &mut self,
    ) -> Result<Option<WorkletRetainedEnvelope>, WorkletAudioError> {
        let Some(context_start) = self.start else {
            self.retained_mut()?;
            return Ok(None);
        };
        let retained = self.retained_mut()?;
        if let Some(latch) = retained.latch {
            return Ok(Some(latch));
        }
        let serial = retained
            .serial
            .checked_add(1)
            .ok_or(WorkletAudioError::Overflow)?;
        if retained.practice_event.is_none() {
            match retained.controller.try_pop_projected_event() {
                Ok(event) => retained.practice_event = Some(event),
                Err(PracticeError::Empty) => {}
                Err(error) => return Err(WorkletAudioError::Practice(error)),
            }
        }
        let pause_first = retained.pause_event.is_some_and(|pause| {
            retained
                .practice_event
                .is_none_or(|event| pause.physical_frame <= event.boundary.source_frame)
        });
        let event = if pause_first {
            WorkletRetainedEvent::Pause(retained.pause_event.take().expect("checked pause event"))
        } else if let Some(event) = retained.practice_event.take() {
            WorkletRetainedEvent::Practice(event)
        } else {
            return Ok(None);
        };
        let envelope = WorkletRetainedEnvelope {
            program_id: retained.config.program_id,
            context_start,
            serial,
            event,
        };
        retained.serial = serial;
        retained.latch = Some(envelope);
        Ok(Some(envelope))
    }
    pub fn acknowledge_retained_event(&mut self, serial: u64) -> Result<(), WorkletAudioError> {
        let retained = self.retained_mut()?;
        let envelope = retained.latch.ok_or(WorkletAudioError::StaleEvent)?;
        if envelope.serial != serial {
            return Err(WorkletAudioError::StaleEvent);
        }
        if let WorkletRetainedEvent::Practice(projected) = envelope.event {
            let id = match projected.event {
                PracticeEvent::Boundary(value) => value.request_id,
                PracticeEvent::Rate(value) => value.request_id,
                PracticeEvent::Restart(value) => value.boundary.request_id,
            };
            if retained.pending_control == Some(id) {
                retained.pending_control = None;
            }
        }
        retained.latch = None;
        Ok(())
    }
    pub fn source_playback_history(&self) -> Option<&SourcePlaybackHistory> {
        self.mixer.source_playback_history()
    }
    /// `pin` is the caller-qualified minimum including every unread input.
    /// Sending/acknowledging an event does not qualify that input minimum.
    pub fn retire_source_playback_before(&mut self, pin: u64) -> Result<usize, WorkletAudioError> {
        let retained = self
            .retained
            .as_ref()
            .ok_or(WorkletAudioError::RetainedRequired)?;
        let mut minimum = retained.controller.history_retention_pins().0;
        for event in [
            retained.latch.map(|event| event.event),
            retained.practice_event.map(WorkletRetainedEvent::Practice),
            retained.pause_event.map(WorkletRetainedEvent::Pause),
        ]
        .into_iter()
        .flatten()
        {
            let frame = match event {
                WorkletRetainedEvent::Practice(event) => event.boundary.source_frame,
                WorkletRetainedEvent::Pause(event) => event.physical_frame,
            };
            minimum = Some(minimum.map_or(frame, |old| old.min(frame)));
        }
        if minimum.is_some_and(|minimum| pin > minimum) {
            return Err(WorkletAudioError::InvalidRetentionPin);
        }
        self.mixer
            .retire_source_playback_before(pin)
            .map_err(WorkletAudioError::Render)
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

impl WorkletRetainedEnvelope {
    /// Separate fixed event ABI; all exact values remain integer words.
    pub fn to_words(self) -> [u32; 80] {
        let mut words = [0u64; 40];
        words[0] = 1;
        words[2] = self.program_id;
        words[3] = self.serial;
        words[35] = self.context_start;
        match self.event {
            WorkletRetainedEvent::Pause(event) => {
                words[1] = 4;
                words[4] = event.request_id;
                words[5] = event.generation;
                words[7] = event.physical_frame;
                words[8] = event.playback_frame;
                words[36] = u64::from(event.paused);
            }
            WorkletRetainedEvent::Practice(projected) => {
                words[26] = projected.boundary.source_frame;
                words[27] = projected.boundary.target_frame_offset as u64;
                words[28] = projected.boundary.target_time.seconds();
                words[29] = projected.boundary.target_time.numerator();
                words[30] = projected.boundary.target_time.denominator();
                words[31] = projected.target_frame;
                words[32] = u64::from(projected.target_rate);
                words[33] = u64::from(projected.origin.domain.0);
                words[34] = projected.origin.timestamp.as_nanos() as u64;
                match projected.event {
                    PracticeEvent::Boundary(event) => {
                        words[1] = 1;
                        encode_boundary(&mut words, event);
                    }
                    PracticeEvent::Rate(event) => {
                        words[1] = 2;
                        words[4] = event.request_id;
                        words[5] = event.generation;
                        words[6] = event.iteration;
                        words[7] = event.physical_frame;
                        words[8] = event.playback_frame;
                        words[20] = event.active_elapsed_nanos as u64;
                        words[21] = ((event.active_elapsed_nanos as u128) >> 64) as u64;
                        encode_rate(
                            &mut words,
                            event.request_revision,
                            event.applied_revision,
                            event.previous_rate,
                            event.requested_rate,
                            event.applied_rate,
                            event.outcome,
                        );
                    }
                    PracticeEvent::Restart(event) => {
                        words[1] = 3;
                        encode_boundary(&mut words, event.boundary);
                        encode_rate(
                            &mut words,
                            event.request_revision,
                            event.applied_revision,
                            event.previous_rate,
                            event.requested_rate,
                            event.applied_rate,
                            event.outcome,
                        );
                    }
                }
            }
        }
        let mut output = [0u32; 80];
        for (word, pair) in words.into_iter().zip(output.chunks_exact_mut(2)) {
            pair[0] = word as u32;
            pair[1] = (word >> 32) as u32;
        }
        output
    }
    pub fn from_words(input: &[u32]) -> Result<Self, WorkletAudioError> {
        let invalid = WorkletAudioError::InvalidEvent;
        if input.len() != 80 {
            return Err(invalid);
        }
        let mut words = [0u64; 40];
        for (word, pair) in words.iter_mut().zip(input.chunks_exact(2)) {
            *word = u64::from(pair[0]) | (u64::from(pair[1]) << 32);
        }
        if words[0] != 1
            || words[2] == 0
            || words[3] == 0
            || words[5] == 0
            || words[8] > words[7]
            || words[35].checked_add(words[7]).is_none()
        {
            return Err(invalid);
        }
        let event = if words[1] == 4 {
            if words[4] == 0 || words[36] > 1 {
                return Err(invalid);
            }
            WorkletRetainedEvent::Pause(WorkletPauseEvent {
                request_id: words[4],
                generation: words[5],
                physical_frame: words[7],
                playback_frame: words[8],
                paused: words[36] != 0,
            })
        } else {
            let boundary = || -> Result<PracticeReceipt, WorkletAudioError> {
                let kind = match words[25] {
                    0 => PracticeBoundaryKind::Started,
                    1 => PracticeBoundaryKind::Requested,
                    2 => PracticeBoundaryKind::Looped,
                    3 => PracticeBoundaryKind::Ended,
                    4 => PracticeBoundaryKind::LoopDisabled,
                    5 => PracticeBoundaryKind::ControlRejectedRevision,
                    _ => return Err(invalid),
                };
                Ok(PracticeReceipt {
                    request_id: words[4],
                    generation: words[5],
                    iteration: words[6],
                    physical_frame: words[7],
                    playback_frame: words[8],
                    requested_song_time: Timestamp::from_nanos(words[22] as i64),
                    applied_song_time: Timestamp::from_nanos(words[23] as i64),
                    correction_nanos: words[24] as i64,
                    kind,
                })
            };
            let decode_rate = |at: usize| -> Result<Rate, WorkletAudioError> {
                let rate = Rate::new(words[at] as i64, words[at + 1]).map_err(|_| invalid)?;
                if rate.numerator() <= 0
                    || rate.numerator() as u64 != words[at]
                    || rate.denominator() != words[at + 1]
                {
                    return Err(invalid);
                }
                Ok(rate)
            };
            let event = match words[1] {
                1 => PracticeEvent::Boundary(boundary()?),
                2 | 3 => {
                    if words[4] == 0 {
                        return Err(invalid);
                    }
                    let previous_rate = decode_rate(11)?;
                    let requested_rate = decode_rate(13)?;
                    let applied_rate = decode_rate(15)?;
                    let outcome = decode_rate_outcome(words[17], words[18], words[19])?;
                    match outcome {
                        PracticeRateOutcome::Applied
                            if requested_rate == previous_rate && words[1] == 2 =>
                        {
                            return Err(invalid);
                        }
                        PracticeRateOutcome::Applied
                            if applied_rate != requested_rate
                                || words[9].checked_add(1) != Some(words[10]) =>
                        {
                            return Err(invalid);
                        }
                        PracticeRateOutcome::Noop
                            if previous_rate != requested_rate
                                || applied_rate != requested_rate
                                || words[9].checked_add(1) != Some(words[10]) =>
                        {
                            return Err(invalid);
                        }
                        PracticeRateOutcome::Refused(_) if applied_rate != previous_rate => {
                            return Err(invalid);
                        }
                        _ => {}
                    }
                    if words[1] == 2 {
                        let elapsed =
                            (u128::from(words[20]) | (u128::from(words[21]) << 64)) as i128;
                        if elapsed < 0 {
                            return Err(invalid);
                        }
                        PracticeEvent::Rate(PracticeRateEvent {
                            request_id: words[4],
                            generation: words[5],
                            iteration: words[6],
                            physical_frame: words[7],
                            playback_frame: words[8],
                            request_revision: words[9],
                            applied_revision: words[10],
                            active_elapsed_nanos: elapsed,
                            previous_rate,
                            requested_rate,
                            applied_rate,
                            outcome,
                        })
                    } else {
                        PracticeEvent::Restart(PracticeRestartEvent {
                            boundary: boundary()?,
                            request_revision: words[9],
                            applied_revision: words[10],
                            previous_rate,
                            requested_rate,
                            applied_rate,
                            outcome,
                        })
                    }
                }
                _ => return Err(invalid),
            };
            let target_time =
                TargetTime::new(words[28], words[29], words[30]).map_err(|_| invalid)?;
            let target_rate = u32::try_from(words[32]).map_err(|_| invalid)?;
            let origin = ClockPoint {
                domain: ClockDomainId(u32::try_from(words[33]).map_err(|_| invalid)?),
                timestamp: Timestamp::from_nanos(words[34] as i64),
            };
            if target_rate == 0 || words[26] != words[7] {
                return Err(invalid);
            }
            target_time.point(origin).map_err(|_| invalid)?;
            WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
                event,
                boundary: TargetBoundary {
                    source_frame: words[26],
                    target_frame_offset: usize::try_from(words[27]).map_err(|_| invalid)?,
                    target_time,
                },
                target_frame: words[31],
                target_rate,
                origin,
            })
        };
        let envelope = Self {
            program_id: words[2],
            serial: words[3],
            context_start: words[35],
            event,
        };
        // Canonical re-encoding also rejects every unused and reserved field.
        if envelope.to_words().as_slice() != input {
            return Err(invalid);
        }
        Ok(envelope)
    }
}
fn encode_boundary(words: &mut [u64; 40], event: PracticeReceipt) {
    words[4] = event.request_id;
    words[5] = event.generation;
    words[6] = event.iteration;
    words[7] = event.physical_frame;
    words[8] = event.playback_frame;
    words[22] = event.requested_song_time.as_nanos() as u64;
    words[23] = event.applied_song_time.as_nanos() as u64;
    words[24] = event.correction_nanos as u64;
    words[25] = match event.kind {
        PracticeBoundaryKind::Started => 0,
        PracticeBoundaryKind::Requested => 1,
        PracticeBoundaryKind::Looped => 2,
        PracticeBoundaryKind::Ended => 3,
        PracticeBoundaryKind::LoopDisabled => 4,
        PracticeBoundaryKind::ControlRejectedRevision => 5,
    };
}
fn encode_rate(
    words: &mut [u64; 40],
    request: u64,
    applied: u64,
    previous: Rate,
    requested: Rate,
    actual: Rate,
    outcome: PracticeRateOutcome,
) {
    words[9] = request;
    words[10] = applied;
    words[11] = previous.numerator() as u64;
    words[12] = previous.denominator();
    words[13] = requested.numerator() as u64;
    words[14] = requested.denominator();
    words[15] = actual.numerator() as u64;
    words[16] = actual.denominator();
    let (tag, refusal, phase) = match outcome {
        PracticeRateOutcome::Applied => (0, 0, 0),
        PracticeRateOutcome::Noop => (1, 0, 0),
        PracticeRateOutcome::Refused(reason) => {
            let (refusal, phase) = match reason {
                PracticeRateRefusal::StaleGeneration => (1, 0),
                PracticeRateRefusal::StaleRevision => (2, 0),
                PracticeRateRefusal::Ended => (3, 0),
                PracticeRateRefusal::Phase(error) => (
                    4,
                    match error {
                        PositiveRatePhaseError::NegativePreroll => 1,
                        PositiveRatePhaseError::NonPositiveRate => 2,
                        PositiveRatePhaseError::InvalidEnd => 3,
                        PositiveRatePhaseError::NegativeElapsed => 4,
                        PositiveRatePhaseError::RegressingElapsed => 5,
                        PositiveRatePhaseError::Overflow => 6,
                        PositiveRatePhaseError::DenominatorCapacity => 7,
                    },
                ),
                PracticeRateRefusal::VoiceDenominatorCapacity => (5, 0),
                PracticeRateRefusal::BgmCapacity => (6, 0),
                PracticeRateRefusal::EventBudget => (7, 0),
                PracticeRateRefusal::DeadlineOverflow => (8, 0),
            };
            (2, refusal, phase)
        }
    };
    words[17] = tag;
    words[18] = refusal;
    words[19] = phase;
}
fn decode_rate_outcome(
    tag: u64,
    refusal: u64,
    phase: u64,
) -> Result<PracticeRateOutcome, WorkletAudioError> {
    let invalid = WorkletAudioError::InvalidEvent;
    Ok(match (tag, refusal, phase) {
        (0, 0, 0) => PracticeRateOutcome::Applied,
        (1, 0, 0) => PracticeRateOutcome::Noop,
        (2, refusal, phase) => PracticeRateOutcome::Refused(match (refusal, phase) {
            (1, 0) => PracticeRateRefusal::StaleGeneration,
            (2, 0) => PracticeRateRefusal::StaleRevision,
            (3, 0) => PracticeRateRefusal::Ended,
            (4, phase) => PracticeRateRefusal::Phase(match phase {
                1 => PositiveRatePhaseError::NegativePreroll,
                2 => PositiveRatePhaseError::NonPositiveRate,
                3 => PositiveRatePhaseError::InvalidEnd,
                4 => PositiveRatePhaseError::NegativeElapsed,
                5 => PositiveRatePhaseError::RegressingElapsed,
                6 => PositiveRatePhaseError::Overflow,
                7 => PositiveRatePhaseError::DenominatorCapacity,
                _ => return Err(invalid),
            }),
            (5, 0) => PracticeRateRefusal::VoiceDenominatorCapacity,
            (6, 0) => PracticeRateRefusal::BgmCapacity,
            (7, 0) => PracticeRateRefusal::EventBudget,
            (8, 0) => PracticeRateRefusal::DeadlineOverflow,
            _ => return Err(invalid),
        }),
        _ => return Err(invalid),
    })
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
    decode_section_output(words, None)
}

/// Admit finite output only against its independently configured relative end.
#[cfg(any(test, all(target_arch = "wasm32", feature = "browser")))]
pub(crate) fn decode_section_output(
    words: &[u32],
    expected_end: Option<u64>,
) -> Result<OutputEvidence, &'static str> {
    decode_report(words, expected_end, false)
}

/// Actual pause-excluding retained output; terminal state comes from typed Ended.
#[cfg(any(test, all(target_arch = "wasm32", feature = "browser")))]
pub(crate) fn decode_retained_output(words: &[u32]) -> Result<OutputEvidence, &'static str> {
    decode_report(words, None, true)
}

#[cfg(any(test, all(target_arch = "wasm32", feature = "browser")))]
fn decode_report(
    words: &[u32],
    expected_end: Option<u64>,
    retained: bool,
) -> Result<OutputEvidence, &'static str> {
    if words.len() != 58 {
        return Err("browser output report requires exactly 58 words");
    }
    let mut values = [0u64; 29];
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
    if let Some(end) = expected_end {
        start
            .checked_add(end)
            .ok_or("browser configured output endpoint overflows its context grid")?;
    }
    let context = (values[23] == 1).then_some(values[24]);
    if values[0] == 0 {
        if values[1..23].iter().any(|&value| value != 0)
            || values[28] != 0
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
    let end = values[1]
        .checked_add(values[2])
        .ok_or("browser output render cursor overflow")?;
    if retained && values[12] != end {
        return Err("browser retained rendered frames differ from its physical cursor");
    }
    let (playback_start, playback_frames, paused, marker) = if retained {
        if values[3] > values[1] {
            return Err("browser retained playback exceeds physical output");
        }
        (
            values[3],
            if values[5] == 1 { 0 } else { values[2] },
            values[5] == 1,
            None,
        )
    } else {
        match expected_end {
            Some(endpoint) => (
                values[1].min(endpoint),
                values[2].min(endpoint.saturating_sub(values[1])),
                end >= endpoint,
                (end >= endpoint).then_some(endpoint),
            ),
            None => (values[1], values[2], false, None),
        }
    };
    if values[3] != playback_start
        || values[4] != playback_frames
        || values[5] != u64::from(paused)
        || values[6] != u64::from(marker.is_some())
        || values[7] != marker.unwrap_or(0)
        || values[11] != 0
    {
        return Err(match expected_end {
            Some(_) => "browser output differs from its configured finite playback grid",
            None => {
                "browser output requires connected unlimited unpaused playback on its original grid"
            }
        });
    }
    let absolute_end = start
        .checked_add(end)
        .ok_or("browser output context cursor overflow")?;
    if context != Some(absolute_end) {
        return Err("browser output context cursor differs from its relative mixer grid");
    }
    if values[14]
        .checked_add(values[28])
        .is_none_or(|settled| settled > values[13])
    {
        return Err("browser output settled commands exceed consumption");
    }
    if values[28] != 0 && (!paused || marker.is_none()) {
        return Err("browser output expiry requires actual finite playback end");
    }
    let report = RenderReport {
        start_frame: values[1],
        frames: values[2] as usize,
        playback_start_frame: values[3],
        playback_frames: values[4] as usize,
        paused: values[5] != 0,
        playback_end_physical_frame: (values[6] == 1).then_some(values[7]),
        active_voices: values[8] as usize,
        pending_commands: values[9] as usize,
        song_position: Timestamp::from_nanos(values[10] as i64),
        producer_disconnected: values[11] != 0,
        counters: AudioCounters {
            rendered_frames: values[12],
            commands_consumed: values[13],
            commands_applied: values[14],
            commands_expired: values[28],
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
