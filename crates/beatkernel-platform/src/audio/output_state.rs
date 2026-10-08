//! Complete same-rate software output ownership, independent of native handles.
use super::{channel_remix, DeviceFormat};
use beatkernel::audio::{
    AudioError, ChannelMatrix, FormatConverter, Mixer, MixerOpenFailure, OutputFrameBasis,
    RenderReport, SoftwareOutputState,
};

/// Unique Mixer, prepared remix and generated output awaiting positive admission.
/// Native counters and presentation observations are deliberately not stored here.
pub struct NativeOutputState {
    mixer: Mixer,
    format: Option<DeviceFormat>,
    remix: Option<FormatConverter>,
    samples: Vec<f32>,
    max_frames: usize,
    report: Option<RenderReport>,
    admitted: usize,
}
impl NativeOutputState {
    /// Wraps an ordinary Mixer without allocating or inventing a target format.
    pub fn from_mixer(mixer: Mixer) -> Self {
        Self {
            mixer,
            format: None,
            remix: None,
            samples: Vec::new(),
            max_frames: 0,
            report: None,
            admitted: 0,
        }
    }
    /// Cold preparation; every refusal returns the original Mixer untouched.
    pub fn new(
        mixer: Mixer,
        format: DeviceFormat,
        matrix: Option<ChannelMatrix>,
        max_frames: usize,
    ) -> Result<Self, MixerOpenFailure<AudioError>> {
        let mut owner = Self::from_mixer(mixer);
        match owner.reconfigure(format, matrix, max_frames) {
            Ok(()) => Ok(owner),
            Err(error) => Err(MixerOpenFailure::new(error, Some(owner.mixer))),
        }
    }
    /// Original Mixer facts, without destructive extraction or mutable access.
    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }
    /// Applied interpretation, absent before cold preparation.
    pub fn format(&self) -> Option<DeviceFormat> {
        self.format
    }
    /// Maximum size of a fresh block; retained tails may be larger after reopen.
    pub fn max_frames(&self) -> usize {
        self.max_frames
    }
    /// Actual report for the retained whole block; never fresh callback evidence.
    pub fn pending_report(&self) -> Option<RenderReport> {
        if self.pending_frames() == 0 {
            None
        } else {
            self.report
        }
    }
    /// Positively admitted prefix of the most recently generated block.
    pub fn admitted_frames(&self) -> usize {
        self.admitted
    }
    /// Unsubmitted complete frames remaining in the retained block.
    pub fn pending_frames(&self) -> usize {
        self.report
            .map_or(0, |report| report.frames - self.admitted)
    }
    /// Exact target suffix awaiting admission; no allocation or regeneration.
    pub fn pending_samples(&self) -> &[f32] {
        if self.pending_frames() == 0 {
            return &[];
        }
        let Some(format) = self.format else {
            return &[];
        };
        let Some(report) = self.report else {
            return &[];
        };
        let channels = usize::from(format.channels());
        &self.samples[self.admitted * channels..report.frames * channels]
    }
    /// Basis of the first unsent frame, which may precede the Mixer frontier.
    pub fn output_frame_basis(&self) -> OutputFrameBasis {
        let base = self.mixer.output_frame_basis();
        let frame = self
            .pending_report()
            .map_or(base.start_physical_frame(), |report| {
                report.start_frame + self.admitted as u64
            });
        OutputFrameBasis::new(base.origin(), base.sample_rate(), frame)
            .expect("validated Mixer rate")
    }
    /// Retained output is admissible under a hold only with actual paused-zero
    /// evidence on the unchanged frozen playback grid. No tail is vacuous.
    pub fn paused_tail_admissible(&self) -> bool {
        self.pending_report().is_none_or(|report| {
            report.paused
                && report.playback_frames == 0
                && report.playback_start_frame == self.mixer.playback_frame_cursor()
                && self.pending_samples().iter().all(|sample| *sample == 0.0)
        })
    }
    /// Cold validation and preparation commit only after every fallible step.
    /// An incompatible pending interpretation refuses without consuming state.
    pub fn reconfigure(
        &mut self,
        format: DeviceFormat,
        matrix: Option<ChannelMatrix>,
        max_frames: usize,
    ) -> Result<(), AudioError> {
        self.validate_reconfigure(format, matrix.as_ref(), max_frames)?;
        let config = self.mixer.config();
        let same = self.format == Some(format)
            && self.remix.as_ref().map(FormatConverter::matrix) == matrix.as_ref();
        // Equal-rate legacy remix uses no source lookahead. Preparing to the
        // immutable Mixer bound allows later period changes to retain this owner.
        let replacement = if same {
            None
        } else {
            Some(match matrix {
                Some(matrix) => Some(channel_remix::prepare(
                    config,
                    format.pcm(),
                    matrix,
                    config.limits().max_render_frames(),
                )?),
                None => None,
            })
        };
        let retained = self.pending_report().map_or(0, |report| report.frames);
        let samples = max_frames
            .max(retained)
            .checked_mul(usize::from(format.channels()))
            .ok_or(AudioError::Overflow)?;
        let mut grown = if samples > self.samples.len() {
            let mut storage = Vec::new();
            storage
                .try_reserve_exact(samples)
                .map_err(|_| AudioError::AllocationFailed)?;
            storage.resize(samples, 0.0);
            if retained != 0 {
                let count = retained * usize::from(format.channels());
                storage[..count].copy_from_slice(&self.samples[..count]);
            }
            Some(storage)
        } else {
            None
        };
        if let Some(storage) = grown.take() {
            self.samples = storage;
        }
        if let Some(remix) = replacement {
            self.remix = remix;
            self.report = None;
            self.admitted = 0;
        }
        self.format = Some(format);
        self.max_frames = max_frames;
        Ok(())
    }
    /// Checks a cold request without allocating or changing retained state.
    /// Native acquisition can refuse after this check without committing it.
    pub fn validate_reconfigure(
        &self,
        format: DeviceFormat,
        matrix: Option<&ChannelMatrix>,
        max_frames: usize,
    ) -> Result<(), AudioError> {
        let config = self.mixer.config();
        if max_frames == 0 {
            return Err(AudioError::InvalidCapacity);
        }
        if max_frames > config.limits().max_render_frames() {
            return Err(AudioError::RenderCapacity);
        }
        match matrix {
            Some(matrix) => channel_remix::validate(config.format(), format.pcm(), matrix)?,
            None if config.format() != format.pcm() => return Err(AudioError::InvalidFormat),
            None => {}
        }
        let same = self.format == Some(format)
            && self.remix.as_ref().map(FormatConverter::matrix) == matrix;
        if self.pending_frames() != 0 && !same {
            return Err(AudioError::InvalidFormat);
        }
        Ok(())
    }
    /// Renders exactly one new block into prepared storage. Pending output must
    /// first be admitted; PCM/report are retained before native encoding starts.
    pub fn render_pending(&mut self, frames: usize) -> Result<RenderReport, AudioError> {
        if frames == 0 || self.pending_frames() != 0 {
            return Err(AudioError::InvalidBuffer);
        }
        if frames > self.max_frames {
            return Err(AudioError::RenderCapacity);
        }
        let format = self.format.ok_or(AudioError::InvalidFormat)?;
        let count = frames * usize::from(format.channels());
        let report =
            channel_remix::render(&mut self.mixer, &mut self.remix, &mut self.samples[..count])?;
        self.report = Some(report);
        self.admitted = 0;
        Ok(report)
    }
    /// Commits a validated positive native write before later fallible telemetry.
    /// Excess counts refuse without moving the pending prefix.
    pub fn admit(&mut self, frames: usize) -> Result<(), AudioError> {
        if frames == 0 || frames > self.pending_frames() {
            return Err(AudioError::InvalidBuffer);
        }
        self.admitted += frames;
        Ok(())
    }
    /// Legacy extraction refuses if it would discard converter or unsent PCM.
    /// Refusal returns the entire owner for a later complete-state retry.
    pub fn into_mixer(self) -> Result<Mixer, Self> {
        if self.remix.is_some() || self.pending_frames() != 0 {
            Err(self)
        } else {
            Ok(self.mixer)
        }
    }
}
impl SoftwareOutputState for NativeOutputState {
    fn mixer(&self) -> &Mixer {
        self.mixer()
    }
    fn output_frame_basis(&self) -> OutputFrameBasis {
        self.output_frame_basis()
    }
    fn paused_tail_admissible(&self) -> bool {
        self.paused_tail_admissible()
    }
}
#[cfg(test)]
#[path = "output_state_fixtures.rs"]
pub(crate) mod output_state_fixtures;
