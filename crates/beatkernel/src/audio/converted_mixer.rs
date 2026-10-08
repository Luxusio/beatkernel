//! Unique ownership of a source mixer and its retained conversion stream.

use super::{
    AudioError, AudioFormat, ChannelMatrix, FormatConverter, Mixer, MixerOpenFailure, RenderReport,
    ResampleQuality, SourcePosition,
};

/// Operation performed on a target block, without native presentation evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertedOutputState {
    /// Converted source PCM; the source report does not prove target silence.
    Active,
    /// Explicit target silence without executing the source or consuming PCM.
    /// This establishes neither mixer pause adoption nor native pause ACK.
    Held,
}

/// Source-grid facts and target-grid progress kept explicitly separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConvertedRenderReport {
    /// Actual source callback report, including an empty callback on active
    /// zero-length or entirely cached output. Absent for held output.
    /// Its pause/end facts never establish target pause/end or presentation.
    pub source: Option<RenderReport>,
    /// Number of complete target frames written by this operation.
    pub target_frames: usize,
    /// Exclusive target frame cursor after this operation, including held frames.
    pub target_frame_cursor: u64,
    /// Exact absolute next sampling position on the original source grid.
    pub source_position: SourcePosition,
    /// Actual exclusive mixer pull frontier on the original source grid.
    pub pulled_source_frame_cursor: u64,
    /// Conversion or explicit held-output operation, not a native acknowledgment.
    pub state: ConvertedOutputState,
}

/// One uniquely owned mixer and continuity-capable conversion stream.
///
/// Move this complete value to preserve voices, commands, history and unread
/// PCM. It intentionally provides no operation taking only the mixer.
/// Construction begins a new conversion stream at the mixer's current cursor,
/// with the converter's normal initial padding. It cannot recover DSP history
/// from any earlier bare-mixer output. Native continuity therefore requires this
/// complete owner from the beginning of that conversion stream.
///
/// Construction and retargeting are cold operations. Rendering uses prepared
/// storage without allocation, destruction, locks or dynamic dispatch.
pub struct ConvertedMixer {
    mixer: Mixer,
    converter: FormatConverter,
    source_base: u64,
    target_frame_cursor: u64,
}

impl ConvertedMixer {
    /// Creates a conversion stream checked against this actual mixer's limits.
    /// Refusal returns the original mixer, without rendering or consuming commands.
    pub fn new(
        mixer: Mixer,
        target: AudioFormat,
        matrix: ChannelMatrix,
        quality: ResampleQuality,
        max_output_frames: usize,
    ) -> Result<Self, MixerOpenFailure<AudioError>> {
        let converter = match FormatConverter::for_mixer_continuous(
            mixer.config(),
            target,
            matrix,
            quality,
            max_output_frames,
        ) {
            Ok(converter) => converter,
            Err(error) => return Err(MixerOpenFailure::new(error, Some(mixer))),
        };
        Ok(Self {
            source_base: mixer.frame_cursor(),
            mixer,
            converter,
            target_frame_cursor: 0,
        })
    }

    /// Prepares new target configuration while preserving the complete stream.
    /// All capacity checks and fallible converter preparation precede mutation.
    pub fn retarget(
        &mut self,
        target: AudioFormat,
        matrix: ChannelMatrix,
        max_output_frames: usize,
    ) -> Result<(), AudioError> {
        let required = FormatConverter::required_source_frames_continuous(
            self.mixer.config().format(),
            target,
            self.converter.quality(),
            max_output_frames,
        )?;
        if required > self.mixer.config().limits().max_render_frames() {
            return Err(AudioError::RenderCapacity);
        }
        self.converter.retarget(target, matrix, max_output_frames)
    }

    /// Read-only original mixer facts; this cannot separate unique ownership.
    pub const fn mixer(&self) -> &Mixer {
        &self.mixer
    }

    /// Read-only retained converter facts. Its positions/counters are relative
    /// to construction; use this owner's position for the absolute source grid.
    pub const fn converter(&self) -> &FormatConverter {
        &self.converter
    }

    /// Exclusive target cursor counting both conversion and held target frames.
    pub const fn target_frame_cursor(&self) -> u64 {
        self.target_frame_cursor
    }

    /// Exact absolute next source sampling position, distinct from pulled PCM.
    pub fn source_position(&self) -> SourcePosition {
        let mut position = self.converter.source_position();
        // Construction establishes zero relative position; every active render
        // validates its next absolute position before invoking the source.
        position.frame += self.source_base;
        position
    }

    /// Actual exclusive source pull frontier, including unread converter PCM.
    pub const fn pulled_source_frame_cursor(&self) -> u64 {
        self.mixer.frame_cursor()
    }

    /// Converts target PCM and returns the actual source callback facts.
    /// An empty source report can accompany nonzero cached target PCM; even a
    /// paused source report cannot prove that this target block is silent.
    pub fn render(&mut self, output: &mut [f32]) -> Result<ConvertedRenderReport, AudioError> {
        let (frames, end) = self.target_extent(output)?;
        self.validate_source_position(frames)?;
        let mixer = &mut self.mixer;
        let source = self.converter.render(output, |block| mixer.render(block))?;
        self.target_frame_cursor = end;
        Ok(self.report(frames, Some(source), ConvertedOutputState::Active))
    }

    /// Writes bounded target silence without calling the mixer, consuming
    /// commands or advancing converter phase/history. Buffered PCM and producer
    /// pause requests remain unchanged for later conversion.
    /// This is explicit component output, not mixer/native pause adoption.
    pub fn render_held(&mut self, output: &mut [f32]) -> Result<ConvertedRenderReport, AudioError> {
        let (frames, end) = self.target_extent(output)?;
        output.fill(0.0);
        self.target_frame_cursor = end;
        Ok(self.report(frames, None, ConvertedOutputState::Held))
    }

    fn target_extent(&self, output: &[f32]) -> Result<(usize, u64), AudioError> {
        let channels = usize::from(self.converter.target_format().channels());
        if !output.len().is_multiple_of(channels) {
            return Err(AudioError::InvalidBuffer);
        }
        let frames = output.len() / channels;
        if frames > self.converter.max_output_frames() {
            return Err(AudioError::RenderCapacity);
        }
        let end = self
            .target_frame_cursor
            .checked_add(frames as u64)
            .ok_or(AudioError::Overflow)?;
        Ok((frames, end))
    }

    fn validate_source_position(&self, frames: usize) -> Result<(), AudioError> {
        let position = self.converter.source_position();
        let source_rate = u128::from(self.converter.source_format().sample_rate());
        let target_rate = u128::from(self.converter.target_format().sample_rate());
        let denominator = u128::from(position.denominator);
        // Callback/rate/denominator bounds fit these intermediates in u128.
        let numerator = u128::from(position.numerator) * target_rate
            + frames as u128 * source_rate * denominator;
        let advance = u64::try_from(numerator / (denominator * target_rate))
            .map_err(|_| AudioError::Overflow)?;
        position
            .frame
            .checked_add(advance)
            .and_then(|frame| frame.checked_add(self.source_base))
            .ok_or(AudioError::Overflow)?;
        Ok(())
    }

    fn report(
        &self,
        frames: usize,
        source: Option<RenderReport>,
        state: ConvertedOutputState,
    ) -> ConvertedRenderReport {
        ConvertedRenderReport {
            source,
            target_frames: frames,
            target_frame_cursor: self.target_frame_cursor,
            source_position: self.source_position(),
            pulled_source_frame_cursor: self.pulled_source_frame_cursor(),
            state,
        }
    }
}
