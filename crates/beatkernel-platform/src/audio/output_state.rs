//! Complete same-rate software output ownership, independent of native handles.
use super::{
    asio::AsioPcmEncoding,
    channel_remix,
    source_history_retirement_mailbox::{
        apply_source_history_retirement_with_pending, SourceHistoryRetirementRequest,
    },
    DeviceFormat,
};
use beatkernel::audio::{
    AudioError, AudioFormat, ChannelMatrix, FormatConverter, Mixer, MixerOpenFailure,
    OutputFrameBasis, RenderReport, SoftwareOutputState,
};

/// Exact ordered native channels and heterogeneous encodings of an ASIO target.
/// Fixed storage keeps cold ownership snapshots and validation allocation-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsioPlanarLayout {
    pcm: AudioFormat,
    channels: [(u32, AsioPcmEncoding); 32],
}
impl AsioPlanarLayout {
    /// Checks one distinct native channel per target PCM channel, in output order.
    pub fn new(pcm: AudioFormat, channels: &[(u32, AsioPcmEncoding)]) -> Result<Self, AudioError> {
        if channels.len() != usize::from(pcm.channels())
            || channels.iter().enumerate().any(|(index, (channel, _))| {
                *channel > i32::MAX as u32
                    || channels[..index].iter().any(|(old, _)| old == channel)
            })
        {
            return Err(AudioError::InvalidFormat);
        }
        let mut layout = Self {
            pcm,
            channels: [(0, AsioPcmEncoding::Float32Lsb); 32],
        };
        layout.channels[..channels.len()].copy_from_slice(channels);
        Ok(layout)
    }
    /// Software PCM rate and channel count, without claiming native interleaving.
    pub const fn pcm(&self) -> AudioFormat {
        self.pcm
    }
    /// Actual native channel indices and encodings in software output order.
    pub fn channels(&self) -> &[(u32, AsioPcmEncoding)] {
        &self.channels[..usize::from(self.pcm.channels())]
    }
}

/// Applied native interpretation of the retained software PCM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeOutputLayout {
    /// Uniform interleaved native encoding and speaker ordering.
    Interleaved(DeviceFormat),
    /// Explicit ordered ASIO planes, each retaining its own native encoding.
    AsioPlanar(AsioPlanarLayout),
}
impl NativeOutputLayout {
    /// Software PCM rate and channels shared by every native interpretation.
    pub const fn pcm(&self) -> AudioFormat {
        match self {
            Self::Interleaved(format) => format.pcm(),
            Self::AsioPlanar(layout) => layout.pcm(),
        }
    }
}

/// Unique Mixer, prepared remix and generated output awaiting positive admission.
/// Native counters and presentation observations are deliberately not stored here.
pub struct NativeOutputState {
    mixer: Mixer,
    layout: Option<NativeOutputLayout>,
    remix: Option<FormatConverter>,
    samples: Vec<f32>,
    max_frames: usize,
    report: Option<RenderReport>,
    admitted: usize,
    admission_uncertain: bool,
}
impl NativeOutputState {
    /// Wraps an ordinary Mixer without allocating or inventing a target format.
    pub fn from_mixer(mixer: Mixer) -> Self {
        Self {
            mixer,
            layout: None,
            remix: None,
            samples: Vec::new(),
            max_frames: 0,
            report: None,
            admitted: 0,
            admission_uncertain: false,
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
    /// Applied uniform interleaved format; planar targets have no DeviceFormat.
    pub fn format(&self) -> Option<DeviceFormat> {
        match self.layout.as_ref() {
            Some(NativeOutputLayout::Interleaved(format)) => Some(*format),
            _ => None,
        }
    }
    /// Exact prepared native interpretation, absent before cold preparation.
    pub fn layout(&self) -> Option<&NativeOutputLayout> {
        self.layout.as_ref()
    }
    /// Prepared software target rate and channels, including planar native output.
    pub fn target_pcm(&self) -> Option<AudioFormat> {
        self.layout.as_ref().map(NativeOutputLayout::pcm)
    }
    /// Borrow the exact prepared channel interpretation without copying its gains.
    pub fn channel_matrix(&self) -> Option<&ChannelMatrix> {
        self.remix.as_ref().map(FormatConverter::matrix)
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
    /// Irreversibly quarantines an owner after a native release whose admission
    /// outcome is unknown. Retained PCM remains available for diagnosis only.
    pub fn mark_admission_uncertain(&mut self) {
        self.admission_uncertain = true;
    }
    /// Unknown native admission forbids replay, reopening and publication.
    pub const fn admission_is_uncertain(&self) -> bool {
        self.admission_uncertain
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
        let Some(format) = self.target_pcm() else {
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
        !self.admission_uncertain
            && self.pending_report().is_none_or(|report| {
                report.paused
                    && report.playback_frames == 0
                    && report.playback_start_frame == self.mixer.playback_frame_cursor()
                    && self.pending_samples().iter().all(|sample| *sample == 0.0)
            })
    }
    /// Services qualified history intent while preserving the whole pending
    /// block's original Source interpretation and queued practice references.
    pub(crate) fn apply_source_history_retirement(
        &mut self,
        request: SourceHistoryRetirementRequest,
        completed: &mut u64,
    ) -> Result<usize, AudioError> {
        if self.admission_uncertain {
            return Err(AudioError::InvalidBuffer);
        }
        let pending = self.pending_report().map(|report| report.start_frame);
        apply_source_history_retirement_with_pending(&mut self.mixer, request, completed, pending)
    }
    /// Cold native setup prepares history before its first fill, retaining the
    /// original history and pending PCM. Quarantine refuses before any change.
    pub(crate) fn prepare_native_source_history(
        &mut self,
        buffer_frames: u32,
        headroom: usize,
    ) -> Result<(), AudioError> {
        if self.admission_uncertain {
            return Err(AudioError::InvalidBuffer);
        }
        super::source_playback_publication::prepare_native_source_history(
            &mut self.mixer,
            buffer_frames,
            headroom,
        )
    }

    /// Opt-in cold preparation grows original history before native publication
    /// storage is allocated. Every refusal preserves retained PCM and history.
    pub(crate) fn prepare_growing_native_source_history(
        &mut self,
        buffer_frames: u32,
        headroom: usize,
    ) -> Result<(), AudioError> {
        if self.admission_uncertain {
            return Err(AudioError::InvalidBuffer);
        }
        let frames = usize::try_from(buffer_frames).map_err(|_| AudioError::Overflow)?;
        if frames == 0 {
            return Err(AudioError::InvalidCapacity);
        }
        if frames > self.mixer.configuration().limits().max_render_frames() {
            return Err(AudioError::RenderCapacity);
        }
        let capacity = frames.checked_add(headroom).ok_or(AudioError::Overflow)?;
        self.prepare_native_source_history(buffer_frames, headroom)?;
        self.mixer.reserve_source_playback_history(capacity)
    }

    /// Cold validation and preparation commit only after every fallible step.
    /// An incompatible pending interpretation refuses without consuming state.
    pub fn reconfigure(
        &mut self,
        format: DeviceFormat,
        matrix: Option<ChannelMatrix>,
        max_frames: usize,
    ) -> Result<(), AudioError> {
        self.reconfigure_with_remix_capacity(
            NativeOutputLayout::Interleaved(format),
            matrix,
            max_frames,
            self.mixer.config().limits().max_render_frames(),
        )
    }
    /// CoreAudio cold preparation sizes remix storage to the actual callback
    /// maximum while retaining the Mixer's immutable bound for later growth.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn reconfigure_for_coreaudio(
        &mut self,
        format: DeviceFormat,
        matrix: Option<ChannelMatrix>,
        max_frames: usize,
    ) -> Result<(), AudioError> {
        self.reconfigure_with_remix_capacity(
            NativeOutputLayout::Interleaved(format),
            matrix,
            max_frames,
            max_frames,
        )
    }
    /// Cold ASIO preparation sizes remix storage to the actual native buffer.
    /// Pending PCM keeps its exact ordered native channels and encodings.
    pub fn reconfigure_asio(
        &mut self,
        layout: AsioPlanarLayout,
        matrix: Option<ChannelMatrix>,
        max_frames: usize,
    ) -> Result<(), AudioError> {
        self.reconfigure_with_remix_capacity(
            NativeOutputLayout::AsioPlanar(layout),
            matrix,
            max_frames,
            max_frames,
        )
    }
    fn reconfigure_with_remix_capacity(
        &mut self,
        layout: NativeOutputLayout,
        matrix: Option<ChannelMatrix>,
        max_frames: usize,
        remix_max_frames: usize,
    ) -> Result<(), AudioError> {
        self.validate_reconfigure_layout(&layout, matrix.as_ref(), max_frames)?;
        let config = self.mixer.config();
        let format = layout.pcm();
        let same = self.layout.as_ref() == Some(&layout)
            && self.remix.as_ref().map(FormatConverter::matrix) == matrix.as_ref();
        let storage_changed = self
            .remix
            .as_ref()
            .is_some_and(|remix| remix.max_output_frames() != remix_max_frames);
        // Equal-rate Linear remix has no retained lookahead. Cold rebuilding
        // storage resets only its private relative diagnostics; Mixer/Source
        // counters and the original pending report/PCM remain authoritative.
        let replacement = if same && !storage_changed {
            None
        } else {
            Some(match matrix {
                Some(matrix) => Some(channel_remix::prepare(
                    config,
                    format,
                    matrix,
                    remix_max_frames,
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
            if !same {
                self.report = None;
                self.admitted = 0;
            }
        }
        self.layout = Some(layout);
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
        self.validate_reconfigure_layout(
            &NativeOutputLayout::Interleaved(format),
            matrix,
            max_frames,
        )
    }
    /// Checks an exact ASIO request without changing retained state or allocating.
    pub fn validate_reconfigure_asio(
        &self,
        layout: &AsioPlanarLayout,
        matrix: Option<&ChannelMatrix>,
        max_frames: usize,
    ) -> Result<(), AudioError> {
        self.validate_reconfigure_layout(
            &NativeOutputLayout::AsioPlanar(*layout),
            matrix,
            max_frames,
        )
    }
    fn validate_reconfigure_layout(
        &self,
        layout: &NativeOutputLayout,
        matrix: Option<&ChannelMatrix>,
        max_frames: usize,
    ) -> Result<(), AudioError> {
        if self.admission_uncertain {
            return Err(AudioError::InvalidBuffer);
        }
        let config = self.mixer.config();
        let format = layout.pcm();
        if max_frames == 0 {
            return Err(AudioError::InvalidCapacity);
        }
        if max_frames > config.limits().max_render_frames() {
            return Err(AudioError::RenderCapacity);
        }
        match matrix {
            Some(matrix) => channel_remix::validate(config.format(), format, matrix)?,
            None if config.format() != format => return Err(AudioError::InvalidFormat),
            None => {}
        }
        let same = self.layout.as_ref() == Some(layout)
            && self.remix.as_ref().map(FormatConverter::matrix) == matrix;
        if self.pending_frames() != 0 && !same {
            return Err(AudioError::InvalidFormat);
        }
        Ok(())
    }
    /// Renders exactly one new block into prepared storage. Pending output must
    /// first be admitted; PCM/report are retained before native encoding starts.
    pub fn render_pending(&mut self, frames: usize) -> Result<RenderReport, AudioError> {
        if self.admission_uncertain || frames == 0 || self.pending_frames() != 0 {
            return Err(AudioError::InvalidBuffer);
        }
        if frames > self.max_frames {
            return Err(AudioError::RenderCapacity);
        }
        let format = self.target_pcm().ok_or(AudioError::InvalidFormat)?;
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
        if self.admission_uncertain || frames == 0 || frames > self.pending_frames() {
            return Err(AudioError::InvalidBuffer);
        }
        self.admitted += frames;
        Ok(())
    }
    /// Legacy extraction refuses if it would discard converter or unsent PCM,
    /// or bypass quarantine. Refusal retains the entire owner for diagnosis.
    pub fn into_mixer(self) -> Result<Mixer, Self> {
        if self.admission_uncertain || self.remix.is_some() || self.pending_frames() != 0 {
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
