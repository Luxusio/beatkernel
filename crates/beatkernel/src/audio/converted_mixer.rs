//! Unique ownership of a source mixer and its retained conversion stream.

use super::{
    AudioError, AudioFormat, ChannelMatrix, FormatConverter, Mixer, MixerOpenFailure, RenderReport,
    ResampleQuality, SourcePosition, TargetFrameBasis, TargetTime,
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
    /// Exact physical start duration on the original output origin.
    pub target_start_time: TargetTime,
    /// Exact exclusive physical duration after this generated block.
    pub target_end_time: TargetTime,
    /// Actual target rate for this block; total target cursors are not duration.
    pub target_rate: u32,
    /// Immutable source sample rate used for source-to-target projection.
    pub source_rate: u32,
    /// Exact sampling position before this operation, excluding source lookahead.
    pub source_start_position: SourcePosition,
    /// Immutable source startup gate, when configured and armed.
    pub startup_source_frame: Option<u64>,
    /// Boundary from the actual most recent nonempty source pause adoption.
    /// Retained through cached empty pulls; absent before adoption and on resume.
    pub pause_source_frame: Option<u64>,
    /// First actual nonempty source resume coordinate, retained through cached output.
    pub resume_source_frame: Option<u64>,
}

/// A source boundary crossed by target consumption in this actual block.
/// This is generated target evidence, never native admission or presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetBoundary {
    /// Absolute source-grid boundary.
    pub source_frame: u64,
    /// First target sample at or after the source boundary; may equal block extent.
    pub target_frame_offset: usize,
    /// Exact physical duration at that target frame boundary.
    pub target_time: TargetTime,
}
impl ConvertedRenderReport {
    /// Projects an integer source boundary only when this active block crosses it.
    /// Cached output uses consumed positions, not the source callback pull frontier.
    pub fn project_source_boundary(
        &self,
        frame: u64,
    ) -> Result<Option<TargetBoundary>, AudioError> {
        if self.state == ConvertedOutputState::Held || self.target_frames == 0 {
            return Ok(None);
        }
        if self.source_rate == 0 || self.target_rate == 0 {
            return Err(AudioError::InvalidFormat);
        }
        let start = self.source_start_position;
        let end = self.source_position;
        if start.denominator == 0
            || start.numerator >= start.denominator
            || end.denominator == 0
            || end.numerator >= end.denominator
        {
            return Err(AudioError::InvalidFormat);
        }
        if frame < start.frame
            || (frame == start.frame && start.numerator != 0)
            || frame > end.frame
        {
            return Ok(None);
        }
        let distance = u128::from(frame - start.frame) * u128::from(start.denominator)
            - u128::from(start.numerator);
        let scaled = distance
            .checked_mul(u128::from(self.target_rate))
            .ok_or(AudioError::Overflow)?;
        let divisor = u128::from(start.denominator) * u128::from(self.source_rate);
        let offset = scaled / divisor + u128::from(scaled % divisor != 0);
        let offset = usize::try_from(offset).map_err(|_| AudioError::Overflow)?;
        if offset > self.target_frames {
            return Ok(None);
        }
        Ok(Some(TargetBoundary {
            source_frame: frame,
            target_frame_offset: offset,
            target_time: self
                .target_start_time
                .checked_add_frames(offset as u64, self.target_rate)?,
        }))
    }
    /// Mapped immutable startup gate, when target consumption crosses it.
    pub fn startup_boundary(&self) -> Result<Option<TargetBoundary>, AudioError> {
        self.startup_source_frame
            .map_or(Ok(None), |frame| self.project_source_boundary(frame))
    }
    /// Actual nonempty source pause adoption projected onto target consumption.
    /// A cached empty pull retains previously proven adoption; held output cannot ACK.
    pub fn pause_boundary(&self) -> Result<Option<TargetBoundary>, AudioError> {
        self.pause_source_frame
            .map_or(Ok(None), |frame| self.project_source_boundary(frame))
    }
    /// Actual paused-to-active source adoption projected onto target consumption.
    /// Cached and held output cannot invent a resume transition.
    pub fn resume_boundary(&self) -> Result<Option<TargetBoundary>, AudioError> {
        self.resume_source_frame
            .map_or(Ok(None), |frame| self.project_source_boundary(frame))
    }
    /// Immutable exclusive source endpoint projected onto actual target consumption.
    pub fn end_boundary(&self) -> Result<Option<TargetBoundary>, AudioError> {
        self.source
            .and_then(|source| source.playback_end_physical_frame)
            .map_or(Ok(None), |frame| self.project_source_boundary(frame))
    }
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
    target_time: TargetTime,
    pause_source_frame: Option<u64>,
    resume_source_frame: Option<u64>,
    source_was_paused: bool,
}

impl ConvertedMixer {
    /// Creates a conversion stream checked against this actual mixer's limits.
    /// Refusal returns the original mixer, without rendering or consuming commands.
    #[allow(
        clippy::result_large_err,
        reason = "Return the original Mixer inline for recovery without additional error-path allocation"
    )]
    pub fn new(
        mixer: Mixer,
        target: AudioFormat,
        matrix: ChannelMatrix,
        quality: ResampleQuality,
        max_output_frames: usize,
    ) -> Result<Self, MixerOpenFailure<AudioError>> {
        let target_time = match TargetTime::from_frames(
            mixer.frame_cursor(),
            mixer.config().format().sample_rate(),
        )
        .and_then(|time| {
            time.rate_denominator(target.sample_rate())?;
            time.point(mixer.output_frame_basis().origin())?;
            Ok(time)
        }) {
            Ok(time) => time,
            Err(error) => return Err(MixerOpenFailure::new(error, Some(mixer))),
        };
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
        let source_was_paused = mixer.is_paused();
        Ok(Self {
            source_base: mixer.frame_cursor(),
            mixer,
            converter,
            target_frame_cursor: 0,
            target_time,
            pause_source_frame: None,
            resume_source_frame: None,
            source_was_paused,
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
        self.target_time.rate_denominator(target.sample_rate())?;
        self.target_time
            .point(self.mixer.output_frame_basis().origin())?;
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

    /// Exact physical duration including all generated active and held frames.
    pub const fn target_time(&self) -> TargetTime {
        self.target_time
    }

    /// New stream basis at the next generated target frame, before pending-native accounting.
    pub fn target_frame_basis(&self) -> TargetFrameBasis {
        TargetFrameBasis::new(
            self.mixer.output_frame_basis().origin(),
            self.target_time,
            self.converter.target_format().sample_rate(),
        )
        .expect("prevalidated target time")
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
        let source_start = self.source_position();
        let time_start = self.target_time;
        let time_end = self.next_target_time(frames)?;
        let mixer = &mut self.mixer;
        let source = self.converter.render(output, |block| mixer.render(block))?;
        if source.frames != 0 {
            if source.paused {
                if !self.source_was_paused || self.pause_source_frame.is_none() {
                    self.pause_source_frame =
                        Some(source.start_frame + source.playback_frames as u64);
                }
                self.resume_source_frame = None;
            } else {
                if self.source_was_paused && source.playback_frames != 0 {
                    self.resume_source_frame = Some(source.start_frame);
                }
                self.pause_source_frame = None;
            }
            self.source_was_paused = source.paused;
        }
        self.target_frame_cursor = end;
        self.target_time = time_end;
        let report = self.report(
            frames,
            Some(source),
            ConvertedOutputState::Active,
            source_start,
            time_start,
        );
        // Source lookahead can expose a gate or endpoint before target consumption.
        // Apply strict sample-position clipping on the generated target grid.
        let channels = usize::from(self.converter.target_format().channels());
        for frame in [
            report.startup_source_frame,
            report.resume_source_frame,
            source.playback_end_physical_frame,
        ]
        .into_iter()
        .flatten()
        {
            let offset = if frame < source_start.frame
                || (frame == source_start.frame && source_start.numerator != 0)
            {
                0
            } else if frame > report.source_position.frame {
                frames
            } else {
                report
                    .project_source_boundary(frame)?
                    .map_or(frames, |boundary| boundary.target_frame_offset)
            };
            if Some(frame) == report.startup_source_frame
                || Some(frame) == report.resume_source_frame
            {
                output[..offset * channels].fill(0.0);
            }
            if Some(frame) == source.playback_end_physical_frame {
                output[offset * channels..].fill(0.0);
            }
        }
        Ok(report)
    }

    /// Writes bounded target silence without calling the mixer, consuming
    /// commands or advancing converter phase/history. Buffered PCM and producer
    /// pause requests remain unchanged for later conversion.
    /// This is explicit component output, not mixer/native pause adoption.
    pub fn render_held(&mut self, output: &mut [f32]) -> Result<ConvertedRenderReport, AudioError> {
        let (frames, end) = self.target_extent(output)?;
        let source_start = self.source_position();
        let time_start = self.target_time;
        let time_end = self.next_target_time(frames)?;
        output.fill(0.0);
        self.target_frame_cursor = end;
        self.target_time = time_end;
        Ok(self.report(
            frames,
            None,
            ConvertedOutputState::Held,
            source_start,
            time_start,
        ))
    }

    fn next_target_time(&self, frames: usize) -> Result<TargetTime, AudioError> {
        let time = self
            .target_time
            .checked_add_frames(frames as u64, self.converter.target_format().sample_rate())?;
        time.point(self.mixer.output_frame_basis().origin())?;
        Ok(time)
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
        source_start_position: SourcePosition,
        target_start_time: TargetTime,
    ) -> ConvertedRenderReport {
        ConvertedRenderReport {
            source,
            target_frames: frames,
            target_frame_cursor: self.target_frame_cursor,
            source_position: self.source_position(),
            pulled_source_frame_cursor: self.pulled_source_frame_cursor(),
            state,
            target_start_time,
            target_end_time: self.target_time,
            target_rate: self.converter.target_format().sample_rate(),
            source_rate: self.converter.source_format().sample_rate(),
            source_start_position,
            startup_source_frame: self.mixer.start_gate_frame().flatten(),
            pause_source_frame: self.pause_source_frame,
            resume_source_frame: self.resume_source_frame,
        }
    }
}
