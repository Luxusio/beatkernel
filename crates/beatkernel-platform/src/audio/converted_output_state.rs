//! Complete converted software ownership and positively unsubmitted target PCM.
use super::DeviceFormat;
use beatkernel::audio::{
    AudioError, ChannelMatrix, ConvertedMixer, ConvertedOutputState, ConvertedRenderReport, Mixer,
    MixerOpenFailure, RenderReport, ResampleQuality, TargetBoundary, TargetFrameBasis,
};

/// Persisted mapped boundaries; latest/held rendering cannot erase prior evidence.
/// These are generated-output facts, not admission or presentation acknowledgments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConvertedBoundaryFacts {
    /// Exact generated target onset of the immutable startup gate.
    pub startup: Option<TargetBoundary>,
    /// Target boundary of actual source pause adoption.
    pub pause: Option<TargetBoundary>,
    /// Actual source resume projected onto consumed target output.
    pub resume: Option<TargetBoundary>,
    /// Immutable exclusive target endpoint.
    pub end: Option<TargetBoundary>,
    /// Immutable original output clock origin; None only before observation.
    pub origin: Option<beatkernel::time::ClockPoint>,
    /// Immutable source grid rate associated with mapped coordinates.
    pub source_rate: u32,
}
/// Unique conversion owner plus one bounded generated target block and prefix.
pub struct ConvertedNativeOutputState {
    owner: ConvertedMixer,
    format: DeviceFormat,
    samples: Vec<f32>,
    max_frames: usize,
    report: Option<ConvertedRenderReport>,
    admitted: usize,
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
        self.admitted += frames;
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
