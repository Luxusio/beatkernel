//! Complete converted software ownership and positively unsubmitted target PCM.
use super::DeviceFormat;
use beatkernel::audio::{
    AudioError, ChannelMatrix, ConvertedBoundaryFacts, ConvertedMixer, ConvertedOutputState,
    ConvertedPhaseHistory, ConvertedRenderReport, Mixer, MixerOpenFailure, RenderReport,
    ResampleQuality, TargetFrameBasis, TargetTime,
};

/// Unique conversion owner plus one bounded generated target block and prefix.
pub struct ConvertedNativeOutputState {
    owner: ConvertedMixer,
    format: DeviceFormat,
    samples: Vec<f32>,
    max_frames: usize,
    report: Option<ConvertedRenderReport>,
    admitted: usize,
    admitted_target_time: Option<TargetTime>,
    boundaries: ConvertedBoundaryFacts,
    last_real_source: Option<RenderReport>,
}
impl ConvertedNativeOutputState {
    /// Allocates target storage before acquiring the complete conversion owner.
    /// Every failure returns the original Mixer without rendering or consuming it.
    pub fn new(
        mixer: Mixer,
        format: DeviceFormat,
        matrix: ChannelMatrix,
        quality: ResampleQuality,
        max_frames: usize,
    ) -> Result<Self, MixerOpenFailure<AudioError>> {
        let samples = match storage(format, max_frames) {
            Ok(samples) => samples,
            Err(error) => return Err(MixerOpenFailure::new(error, Some(mixer))),
        };
        let owner = ConvertedMixer::new(mixer, format.pcm(), matrix, quality, max_frames)?;
        let boundaries = initial_boundaries(&owner);
        Ok(Self {
            owner,
            format,
            samples,
            max_frames,
            report: None,
            admitted: 0,
            admitted_target_time: None,
            boundaries,
            last_real_source: None,
        })
    }
    /// Wraps an existing complete converter, never flattening it on refusal.
    pub fn from_converted(
        owner: ConvertedMixer,
        format: DeviceFormat,
        max_frames: usize,
    ) -> Result<Self, MixerOpenFailure<AudioError, ConvertedMixer>> {
        let prepared = (|| {
            if owner.converter().target_format() != format.pcm() {
                return Err(AudioError::InvalidFormat);
            }
            if max_frames > owner.converter().max_output_frames() {
                return Err(AudioError::RenderCapacity);
            }
            storage(format, max_frames)
        })();
        match prepared {
            Ok(samples) => {
                let boundaries = initial_boundaries(&owner);
                Ok(Self {
                    owner,
                    format,
                    samples,
                    max_frames,
                    report: None,
                    admitted: 0,
                    admitted_target_time: None,
                    boundaries,
                    last_real_source: None,
                })
            }
            Err(error) => Err(MixerOpenFailure::new_state(error, Some(owner))),
        }
    }
    /// Original immutable source scheduling owner.
    pub fn mixer(&self) -> &Mixer {
        self.owner.mixer()
    }
    /// Complete conversion facts; mutable subset extraction is unavailable.
    pub fn converter_owner(&self) -> &ConvertedMixer {
        &self.owner
    }
    /// Cold opt-in to bounded generated history before any target generation.
    /// Generated correspondence proves neither native submission nor presentation.
    pub fn enable_phase_history(&mut self, capacity: usize) -> Result<(), AudioError> {
        self.owner.enable_phase_history(capacity)
    }
    /// Cold-enable bounded actual Mixer source playback adoption history.
    pub fn enable_source_playback_history(&mut self, capacity: usize) -> Result<(), AudioError> {
        self.owner.enable_source_playback_history(capacity)
    }

    /// Generated correspondence retained with the complete conversion owner.
    pub fn phase_history(&self) -> Option<&ConvertedPhaseHistory> {
        self.owner.phase_history()
    }
    /// Original queued practice references, without consuming or projecting them.
    pub(crate) fn practice_history_retention_pins(&self) -> (Option<u64>, Option<TargetTime>) {
        self.owner.mixer().practice_history_retention_pins()
    }
    /// Retires qualified generated history without discarding a pending block's
    /// original interpretation. All external live consumers must also be pinned
    /// by the caller; this operation grants no native admission or acknowledgment.
    pub(crate) fn retire_phase_history_before(
        &mut self,
        pin: TargetTime,
    ) -> Result<usize, AudioError> {
        self.validate_phase_retirement(pin)?;
        self.owner.retire_phase_history_before(pin)
    }
    fn validate_phase_retirement(&self, pin: TargetTime) -> Result<(), AudioError> {
        let history = self
            .owner
            .phase_history()
            .ok_or(AudioError::InvalidCapacity)?;
        if let Some(report) = self.pending_report() {
            let start = report.target_start_time;
            if pin
                .seconds()
                .cmp(&start.seconds())
                .then_with(|| {
                    (u128::from(pin.numerator()) * u128::from(start.denominator()))
                        .cmp(&(u128::from(start.numerator()) * u128::from(pin.denominator())))
                })
                .is_gt()
            {
                return Err(AudioError::InvalidFormat);
            }
        }
        if let Some(start) = self.practice_history_retention_pins().1 {
            if pin
                .seconds()
                .cmp(&start.seconds())
                .then_with(|| {
                    (u128::from(pin.numerator()) * u128::from(start.denominator()))
                        .cmp(&(u128::from(start.numerator()) * u128::from(pin.denominator())))
                })
                .is_gt()
            {
                return Err(AudioError::InvalidFormat);
            }
        }
        history.validate_retirement_before(pin)
    }
    /// Retires qualified physical source history on the same complete owner.
    /// The converter's next sampling floor remains protected even after full
    /// admission because cached output can still refer to earlier source frames.
    /// A partially admitted block protects its original source sampling floor
    /// until its entire PCM is admitted, including cached and held output.
    /// The caller remains responsible for every external live consumer's pin.
    pub(crate) fn retire_source_playback_before(&mut self, pin: u64) -> Result<usize, AudioError> {
        self.validate_source_retirement(pin)?;
        self.owner.retire_source_playback_before(pin)
    }
    fn validate_source_retirement(&self, pin: u64) -> Result<(), AudioError> {
        let history = self
            .owner
            .source_playback_history()
            .ok_or(AudioError::InvalidCapacity)?;
        if pin > self.owner.source_position().frame
            || self
                .pending_report()
                .is_some_and(|report| pin > report.source_start_position.frame)
            || self
                .practice_history_retention_pins()
                .0
                .is_some_and(|start| pin > start)
        {
            return Err(AudioError::InvalidFormat);
        }
        history.validate_retirement_before(pin)
    }
    /// Retires both histories only after their ranges, revision increments and
    /// internal PCM/cache references have been checked without mutation.
    /// Returns the source and phase removal counts in that order. The caller
    /// must also qualify both pins against every external live consumer.
    pub(crate) fn retire_histories_before(
        &mut self,
        source_pin: u64,
        target_pin: TargetTime,
    ) -> Result<(usize, usize), AudioError> {
        self.validate_source_retirement(source_pin)?;
        self.validate_phase_retirement(target_pin)?;
        // Exclusive ownership prevents intervening changes. Each retirement
        // modifies only its own history, so the other validation stays valid.
        let source_removed = self.owner.retire_source_playback_before(source_pin)?;
        let phase_removed = self.owner.retire_phase_history_before(target_pin)?;
        Ok((source_removed, phase_removed))
    }
    /// Actual target encoding, channels and rate.
    pub const fn format(&self) -> DeviceFormat {
        self.format
    }
    /// Maximum fresh target extent after cold preparation.
    pub const fn max_frames(&self) -> usize {
        self.max_frames
    }
    /// Retained generated target report, absent without pending output.
    pub fn pending_report(&self) -> Option<ConvertedRenderReport> {
        if self.pending_frames() == 0 {
            None
        } else {
            self.report
        }
    }
    /// True only while an explicitly held generation block has an unsubmitted suffix.
    /// Active output containing zeros is not held provenance or a native pause ACK.
    pub fn pending_is_held(&self) -> bool {
        self.pending_report()
            .is_some_and(|report| report.state == ConvertedOutputState::Held)
    }
    /// Remaining positively unsubmitted target frames.
    pub fn pending_frames(&self) -> usize {
        self.report
            .map_or(0, |report| report.target_frames - self.admitted)
    }
    /// Positively admitted prefix within the retained block.
    pub const fn admitted_frames(&self) -> usize {
        self.admitted
    }
    /// Exact endpoint of the last positively admitted target prefix.
    /// This survives software owner moves but does not authenticate a native
    /// stream epoch or actual presentation. Generation alone leaves it absent.
    pub const fn admitted_target_time(&self) -> Option<TargetTime> {
        self.admitted_target_time
    }
    /// Exact pending interleaved target suffix.
    pub fn pending_samples(&self) -> &[f32] {
        if self.pending_frames() == 0 {
            return &[];
        }
        let report = self.report.expect("pending report");
        let channels = usize::from(self.format.channels());
        &self.samples[self.admitted * channels..report.target_frames * channels]
    }
    /// Physical basis of the first unsubmitted target frame, not source lookahead.
    pub fn target_frame_basis(&self) -> TargetFrameBasis {
        match self.pending_report() {
            Some(report) => TargetFrameBasis::new(
                self.owner.target_frame_basis().origin(),
                report
                    .target_start_time
                    .checked_add_frames(self.admitted as u64, report.target_rate)
                    .expect("admitted prefix bounded by validated block"),
                report.target_rate,
            )
            .expect("retained target basis validated before rendering"),
            None => self.owner.target_frame_basis(),
        }
    }
    /// Persisted mapped boundaries, separate from the last source callback.
    pub const fn boundaries(&self) -> ConvertedBoundaryFacts {
        self.boundaries
    }
    /// Last actual nonempty source callback; held/cached output never replaces it.
    pub const fn last_real_source_report(&self) -> Option<RenderReport> {
        self.last_real_source
    }
    /// Cold preserving validation before native acquisition or allocation.
    pub fn validate_reconfigure(
        &self,
        format: DeviceFormat,
        matrix: &ChannelMatrix,
        max_frames: usize,
    ) -> Result<(), AudioError> {
        if max_frames == 0 {
            return Err(AudioError::InvalidCapacity);
        }
        if matrix.source_channels() != self.mixer().config().format().channels()
            || matrix.target_channels() != format.channels()
        {
            return Err(AudioError::InvalidFormat);
        }
        let required = beatkernel::audio::FormatConverter::required_source_frames_continuous(
            self.mixer().config().format(),
            format.pcm(),
            self.owner.converter().quality(),
            max_frames,
        )?;
        if required > self.mixer().config().limits().max_render_frames() {
            return Err(AudioError::RenderCapacity);
        }
        if self.pending_frames() != 0
            && (format != self.format || matrix != self.owner.converter().matrix())
        {
            return Err(AudioError::InvalidFormat);
        }
        Ok(())
    }
    /// Prepares capacity/rate/matrix before committing; pending reinterpretation refuses.
    pub fn reconfigure(
        &mut self,
        format: DeviceFormat,
        matrix: ChannelMatrix,
        max_frames: usize,
    ) -> Result<(), AudioError> {
        self.validate_reconfigure(format, &matrix, max_frames)?;
        let retained = self
            .pending_report()
            .map_or(0, |report| report.target_frames);
        let count = max_frames
            .max(retained)
            .checked_mul(usize::from(format.channels()))
            .ok_or(AudioError::Overflow)?;
        let grown = if count > self.samples.len() {
            let mut samples = storage(format, max_frames.max(retained))?;
            if retained != 0 {
                samples[..retained * usize::from(format.channels())]
                    .copy_from_slice(&self.samples[..retained * usize::from(format.channels())]);
            }
            Some(samples)
        } else {
            None
        };
        // Converter cold refusal leaves both PCM and interpretation unchanged.
        self.owner.retarget(format.pcm(), matrix, max_frames)?;
        if let Some(samples) = grown {
            self.samples = samples;
        }
        self.format = format;
        self.max_frames = max_frames;
        if retained == 0 {
            self.report = None;
            self.admitted = 0;
        }
        Ok(())
    }
    /// Generates one active converted target block before any fallible encoding.
    pub fn render_pending(&mut self, frames: usize) -> Result<ConvertedRenderReport, AudioError> {
        self.render(frames, false)
    }
    /// Generates explicit held silence while preserving source phase and commands.
    pub fn render_held_pending(
        &mut self,
        frames: usize,
    ) -> Result<ConvertedRenderReport, AudioError> {
        self.render(frames, true)
    }
    fn render(&mut self, frames: usize, held: bool) -> Result<ConvertedRenderReport, AudioError> {
        if frames == 0 || self.pending_frames() != 0 {
            return Err(AudioError::InvalidBuffer);
        }
        if frames > self.max_frames {
            return Err(AudioError::RenderCapacity);
        }
        let output = &mut self.samples[..frames * usize::from(self.format.channels())];
        let report = if held {
            self.owner.render_held(output)?
        } else {
            self.owner.render(output)?
        };
        if let Some(source) = report.source.filter(|source| source.frames != 0) {
            self.last_real_source = Some(source);
        }
        self.report = Some(report);
        self.admitted = 0;
        // Internal reports have already validated exact source/target arithmetic.
        if let Some(boundary) = report
            .startup_boundary()
            .expect("validated generated startup")
        {
            self.boundaries.startup = Some(boundary);
        }
        if report
            .source
            .is_some_and(|source| source.frames != 0 && !source.paused)
        {
            self.boundaries.pause = None;
        }
        if let Some(boundary) = report.pause_boundary().expect("validated generated pause") {
            self.boundaries.pause = Some(boundary);
        }
        if report
            .source
            .is_some_and(|source| source.frames != 0 && source.paused)
        {
            self.boundaries.resume = None;
        }
        if let Some(boundary) = report
            .resume_boundary()
            .expect("validated generated resume")
        {
            self.boundaries.resume = Some(boundary);
        }
        if let Some(boundary) = report.end_boundary().expect("validated generated end") {
            self.boundaries.end = Some(boundary);
        }
        Ok(report)
    }
    /// Advances a validated positive native prefix before later fallible observations.
    pub fn admit(&mut self, frames: usize) -> Result<(), AudioError> {
        if frames == 0 || frames > self.pending_frames() {
            return Err(AudioError::InvalidBuffer);
        }
        let admitted = self
            .admitted
            .checked_add(frames)
            .ok_or(AudioError::Overflow)?;
        let report = self.report.expect("pending report");
        let admitted_target_time = report.target_start_time.checked_add_frames(
            u64::try_from(admitted).map_err(|_| AudioError::Overflow)?,
            report.target_rate,
        )?;
        self.admitted = admitted;
        self.admitted_target_time = Some(admitted_target_time);
        Ok(())
    }
}
fn initial_boundaries(owner: &ConvertedMixer) -> ConvertedBoundaryFacts {
    ConvertedBoundaryFacts {
        origin: Some(owner.target_frame_basis().origin()),
        source_rate: owner.mixer().config().format().sample_rate(),
        ..ConvertedBoundaryFacts::default()
    }
}
fn storage(format: DeviceFormat, frames: usize) -> Result<Vec<f32>, AudioError> {
    if frames == 0 || frames > beatkernel::audio::AudioLimits::MAX_RENDER_FRAMES {
        return Err(AudioError::InvalidCapacity);
    }
    let count = frames
        .checked_mul(usize::from(format.channels()))
        .ok_or(AudioError::Overflow)?;
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(count)
        .map_err(|_| AudioError::AllocationFailed)?;
    samples.resize(count, 0.0);
    Ok(samples)
}
#[cfg(test)]
#[path = "converted_output_state_fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[path = "converted_paired_retirement_fixtures.rs"]
mod paired_retirement_fixtures;
#[cfg(test)]
#[path = "converted_output_retirement_fixtures.rs"]
mod retirement_fixtures;

#[cfg(test)]
#[path = "converted_practice_retention_fixtures.rs"]
mod practice_retention_fixtures;
