use super::{AudioError, AudioFormat, AudioLimits, MixerConfig};

const MAX_CHANNELS: usize = 32;
// Half-kernel table points per source frame; lookups interpolate between them.
const KERNEL_RESOLUTION: usize = 512;

/// Validated target-major channel gain matrix applied after rate conversion.
///
/// Coefficient `target * source_channels + source` scales that source channel
/// into that target channel. It carries no speaker-position semantics; callers
/// that know a native speaker layout supply an explicit matrix.
///
/// ```
/// use beatkernel::audio::ChannelMatrix;
/// let up = ChannelMatrix::default_mix(1, 2)?;
/// assert_eq!(up.coefficients(), &[1.0, 1.0]);
/// let down = ChannelMatrix::default_mix(2, 1)?;
/// assert_eq!(down.coefficients(), &[0.5, 0.5]);
/// assert!(ChannelMatrix::new(2, 1, &[1.0]).is_err());
/// # Ok::<(), beatkernel::audio::AudioError>(())
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelMatrix {
    source: u16,
    target: u16,
    coefficients: Vec<f32>,
}

impl ChannelMatrix {
    /// Validates 1–32 channels per side and exactly `source * target` finite gains.
    pub fn new(source: u16, target: u16, coefficients: &[f32]) -> Result<Self, AudioError> {
        if !(1..=MAX_CHANNELS).contains(&usize::from(source))
            || !(1..=MAX_CHANNELS).contains(&usize::from(target))
        {
            return Err(AudioError::InvalidFormat);
        }
        if coefficients.len() != usize::from(source) * usize::from(target) {
            return Err(AudioError::InvalidBuffer);
        }
        if coefficients.iter().any(|gain| !gain.is_finite()) {
            return Err(AudioError::NonFiniteSample);
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(coefficients.len())
            .map_err(|_| AudioError::AllocationFailed)?;
        owned.extend_from_slice(coefficients);
        Ok(Self {
            source,
            target,
            coefficients: owned,
        })
    }

    /// Layout-agnostic default: equal counts are identity, mono feeds the first
    /// two targets, a mono target averages every source channel, and otherwise
    /// the shared channel prefix is copied. Extra targets stay silent and extra
    /// sources are dropped rather than guessing speaker positions.
    pub fn default_mix(source: u16, target: u16) -> Result<Self, AudioError> {
        let (sources, targets) = (usize::from(source), usize::from(target));
        if !(1..=MAX_CHANNELS).contains(&sources) || !(1..=MAX_CHANNELS).contains(&targets) {
            return Err(AudioError::InvalidFormat);
        }
        let mut gains = [0.0f32; MAX_CHANNELS * MAX_CHANNELS];
        let gains = &mut gains[..sources * targets];
        if targets == 1 {
            gains.fill(1.0 / sources as f32);
        } else if sources == 1 {
            gains[..2].fill(1.0);
        } else {
            for channel in 0..sources.min(targets) {
                gains[channel * sources + channel] = 1.0;
            }
        }
        Self::new(source, target, gains)
    }

    /// Source (mixer-side) channel count.
    pub const fn source_channels(&self) -> u16 {
        self.source
    }
    /// Target (device-side) channel count.
    pub const fn target_channels(&self) -> u16 {
        self.target
    }
    /// Target-major gains.
    pub fn coefficients(&self) -> &[f32] {
        &self.coefficients
    }
    /// Whether this exactly copies every channel unchanged.
    pub fn is_identity(&self) -> bool {
        let channels = usize::from(self.source);
        self.source == self.target
            && self.coefficients.iter().enumerate().all(|(index, gain)| {
                *gain
                    == if index % channels == index / channels {
                        1.0
                    } else {
                        0.0
                    }
            })
    }
}

/// Fixed rate-conversion kernel; quality is bounded by its tap count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResampleQuality {
    /// Two-tap linear interpolation with one source frame of lookahead.
    Linear,
    /// Blackman-windowed sinc with `2 * half_taps` source taps and `half_taps`
    /// frames of lookahead. The cutoff follows the lower of both Nyquist rates.
    WindowedSinc {
        /// Taps on each side of the interpolation point, 2 through 32.
        half_taps: u8,
    },
}

impl ResampleQuality {
    /// Maximum `WindowedSinc` half-width.
    pub const MAX_HALF_TAPS: u8 = 32;

    fn half_taps(self) -> Result<usize, AudioError> {
        match self {
            Self::Linear => Ok(1),
            Self::WindowedSinc { half_taps } if (2..=Self::MAX_HALF_TAPS).contains(&half_taps) => {
                Ok(usize::from(half_taps))
            }
            Self::WindowedSinc { .. } => Err(AudioError::InvalidCapacity),
        }
    }
}

/// Exact absolute position of the next output sample on the immutable source grid.
/// `numerator / denominator` is a reduced proper fraction after `frame`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePosition {
    /// Whole source frames before the next output sampling position.
    pub frame: u64,
    /// Reduced fractional numerator in a reported source position.
    pub numerator: u64,
    /// Positive denominator of the reported reduced fraction.
    pub denominator: u64,
}

#[derive(Debug)]
struct ContinuousPosition {
    frame: u64,
    // Absolute first nonnegative source frame retained in window.
    window_start: u64,
}

/// Preallocated callback-safe conversion from a source render format, normally
/// a [`super::Mixer`], to a different device rate and/or channel count.
///
/// Each [`render`](Self::render) pulls exactly one contiguous source block, so
/// the source's own report, pause and endpoint evidence pass through unchanged
/// on the source frame grid. Output frame `k` samples source position
/// `k * source_rate / target_rate` using an exact reduced rational phase, so any
/// partition of the same output frames yields identical samples. Continuity-capable
/// construction also preserves exact absolute position across target changes.
/// Frames before
/// source frame zero read as silence. Converted results are clamped once to
/// [-1, 1]; a matching-format pass-through only replaces non-finite samples.
/// Device sample encoding (integer widths or float) stays a native concern.
///
/// ```
/// use beatkernel::audio::{AudioError, AudioFormat, ChannelMatrix, FormatConverter,
///     ResampleQuality};
/// let source = AudioFormat::new(24_000, 1)?;
/// let target = AudioFormat::new(48_000, 2)?;
/// let matrix = ChannelMatrix::default_mix(1, 2)?;
/// let mut converter =
///     FormatConverter::new(source, target, matrix, ResampleQuality::Linear, 4)?;
/// let mut next = 0.0;
/// let mut output = [0.0; 8];
/// converter.render(&mut output, |block: &mut [f32]| {
///     for sample in block.iter_mut() {
///         *sample = next;
///         next += 0.25;
///     }
///     Ok::<_, AudioError>(())
/// })?;
/// assert_eq!(output, [0.0, 0.0, 0.125, 0.125, 0.25, 0.25, 0.375, 0.375]);
/// # Ok::<(), AudioError>(())
/// ```
#[derive(Debug)]
pub struct FormatConverter {
    source: AudioFormat,
    target: AudioFormat,
    matrix: ChannelMatrix,
    identity_mix: bool,
    quality: ResampleQuality,
    // Legacy converters bypass the kernel at equal rates. Continuous converters
    // retain this half-width at every rate to preserve history for later changes.
    half_taps: usize,
    step: u64,
    denominator: u64,
    fraction: u64,
    // Source frames starting at the first tap of the next output frame.
    window: Vec<f32>,
    window_frames: usize,
    kernel: Vec<f32>,
    max_output_frames: usize,
    max_source_frames: usize,
    output_cursor: u64,
    source_cursor: u64,
    sanitized_samples: u64,
    continuous: Option<ContinuousPosition>,
}

impl FormatConverter {
    /// Allocates the source window and kernel table before output starts.
    ///
    /// `max_output_frames` bounds one device callback, 1 through
    /// [`AudioLimits::MAX_RENDER_FRAMES`]; the derived source block bound must
    /// fit the same ceiling. The matrix must match both channel counts.
    pub fn new(
        source: AudioFormat,
        target: AudioFormat,
        matrix: ChannelMatrix,
        quality: ResampleQuality,
        max_output_frames: usize,
    ) -> Result<Self, AudioError> {
        if matrix.source != source.channels() || matrix.target != target.channels() {
            return Err(AudioError::ChannelMismatch);
        }
        let max_source_frames =
            Self::required_source_frames(source, target, quality, max_output_frames)?;
        let half_taps = if source.sample_rate() == target.sample_rate() {
            0
        } else {
            quality.half_taps()?
        };
        let divisor = gcd(source.sample_rate(), target.sample_rate());
        let denominator = u64::from(target.sample_rate() / divisor);
        let step = u64::from(source.sample_rate() / divisor);
        let window_len = max_source_frames
            .checked_mul(usize::from(source.channels()))
            .ok_or(AudioError::Overflow)?;
        let mut window = Vec::new();
        window
            .try_reserve_exact(window_len)
            .map_err(|_| AudioError::AllocationFailed)?;
        window.resize(window_len, 0.0);
        let kernel = match quality {
            ResampleQuality::WindowedSinc { .. } if half_taps != 0 => {
                let cutoff =
                    (f64::from(target.sample_rate()) / f64::from(source.sample_rate())).min(1.0);
                sinc_table(half_taps, cutoff)?
            }
            _ => Vec::new(),
        };
        Ok(Self {
            source,
            target,
            identity_mix: matrix.is_identity(),
            matrix,
            quality,
            half_taps,
            step,
            denominator,
            fraction: 0,
            window,
            // History before source frame zero is explicit silence.
            window_frames: half_taps.saturating_sub(1),
            kernel,
            max_output_frames,
            max_source_frames,
            output_cursor: 0,
            source_cursor: 0,
            sanitized_samples: 0,
            continuous: None,
        })
    }

    /// Cold construction retaining the history needed for later target changes.
    /// Unlike [`new`](Self::new), this keeps kernel history even at equal rates.
    /// Source format and source callback ownership must remain unchanged.
    pub fn new_continuous(
        source: AudioFormat,
        target: AudioFormat,
        matrix: ChannelMatrix,
        quality: ResampleQuality,
        max_output_frames: usize,
    ) -> Result<Self, AudioError> {
        let max_source_frames =
            Self::required_source_frames_continuous(source, target, quality, max_output_frames)?;
        let half_taps = quality.half_taps()?;
        let storage_frames = max_source_frames
            .checked_add(2 * half_taps)
            .ok_or(AudioError::Overflow)?;
        let samples = storage_frames
            .checked_mul(usize::from(source.channels()))
            .ok_or(AudioError::Overflow)?;
        let mut window = Vec::new();
        window
            .try_reserve_exact(samples)
            .map_err(|_| AudioError::AllocationFailed)?;
        window.resize(samples, 0.0);
        let kernel = match quality {
            ResampleQuality::WindowedSinc { .. } => sinc_table(
                half_taps,
                (f64::from(target.sample_rate()) / f64::from(source.sample_rate())).min(1.0),
            )?,
            ResampleQuality::Linear => Vec::new(),
        };
        let mut converter = Self::new(source, target, matrix, quality, max_output_frames)?;
        converter.continuous = Some(ContinuousPosition {
            frame: 0,
            window_start: 0,
        });
        converter.window_frames = 0;
        converter.window = window;
        converter.kernel = kernel;
        converter.half_taps = half_taps;
        converter.max_source_frames = max_source_frames;
        Ok(converter)
    }

    /// Continuity-capable setup checked against the source mixer's render limit.
    pub fn for_mixer_continuous(
        config: MixerConfig,
        target: AudioFormat,
        matrix: ChannelMatrix,
        quality: ResampleQuality,
        max_output_frames: usize,
    ) -> Result<Self, AudioError> {
        let converter =
            Self::new_continuous(config.format(), target, matrix, quality, max_output_frames)?;
        if converter.max_source_frames > config.limits().max_render_frames() {
            return Err(AudioError::RenderCapacity);
        }
        Ok(converter)
    }

    /// Worst contiguous source pull for a continuity-capable callback. Retained
    /// history storage is additional and is prepared internally on the cold path.
    pub fn required_source_frames_continuous(
        source: AudioFormat,
        target: AudioFormat,
        quality: ResampleQuality,
        max_output_frames: usize,
    ) -> Result<usize, AudioError> {
        let half = quality.half_taps()? as u64;
        if max_output_frames == 0 || max_output_frames > AudioLimits::MAX_RENDER_FRAMES {
            return Err(AudioError::InvalidCapacity);
        }
        let frames = max_output_frames as u64;
        let step = u64::from(source.sample_rate());
        let denominator = u64::from(target.sample_rate());
        let required = ((frames - 1) * step)
            .div_ceil(denominator)
            .checked_add(2 * half)
            .ok_or(AudioError::Overflow)?
            .max((frames * step).div_ceil(denominator));
        if required > AudioLimits::MAX_RENDER_FRAMES as u64 {
            return Err(AudioError::RenderCapacity);
        }
        Ok(required as usize)
    }

    /// Prepare a new target on the cold path, preserving source position,
    /// pulled frontier, history, pending PCM and counters. A legacy converter
    /// constructed with `new` refuses this operation with `InvalidCapacity`.
    /// Every fallible preparation finishes before any state is changed.
    pub fn retarget(
        &mut self,
        target: AudioFormat,
        matrix: ChannelMatrix,
        max_output_frames: usize,
    ) -> Result<(), AudioError> {
        if self.continuous.is_none() {
            return Err(AudioError::InvalidCapacity);
        }
        if matrix.source != self.source.channels() || matrix.target != target.channels() {
            return Err(AudioError::ChannelMismatch);
        }
        let max_source_frames = Self::required_source_frames_continuous(
            self.source,
            target,
            self.quality,
            max_output_frames,
        )?;
        let position = self.source_position();
        let divisor = gcd(self.source.sample_rate(), target.sample_rate());
        let rate_denominator = u64::from(target.sample_rate() / divisor);
        let denominator = (position.denominator / gcd64(position.denominator, rate_denominator))
            .checked_mul(rate_denominator)
            .ok_or(AudioError::Overflow)?;
        let fraction = position
            .numerator
            .checked_mul(denominator / position.denominator)
            .ok_or(AudioError::Overflow)?;
        let step = u64::from(self.source.sample_rate() / divisor)
            .checked_mul(denominator / rate_denominator)
            .ok_or(AudioError::Overflow)?;
        let half_taps = self.quality.half_taps()?;
        // Existing unread PCM may exceed a newly smaller callback. Keep all of
        // it, plus enough space for the largest new pull without dropping history.
        let storage_frames = self
            .window_frames
            .checked_add(max_source_frames)
            .and_then(|n| n.checked_add(2 * half_taps))
            .ok_or(AudioError::Overflow)?;
        let samples = storage_frames
            .checked_mul(usize::from(self.source.channels()))
            .ok_or(AudioError::Overflow)?;
        let mut window = Vec::new();
        window
            .try_reserve_exact(samples)
            .map_err(|_| AudioError::AllocationFailed)?;
        window.resize(samples, 0.0);
        let retained_samples = self.window_frames * usize::from(self.source.channels());
        window[..retained_samples].copy_from_slice(&self.window[..retained_samples]);
        let kernel = match self.quality {
            ResampleQuality::WindowedSinc { .. } => sinc_table(
                half_taps,
                (f64::from(target.sample_rate()) / f64::from(self.source.sample_rate())).min(1.0),
            )?,
            ResampleQuality::Linear => Vec::new(),
        };
        self.target = target;
        self.identity_mix = matrix.is_identity();
        self.matrix = matrix;
        self.half_taps = half_taps;
        self.denominator = denominator;
        self.fraction = fraction;
        self.step = step;
        self.window = window;
        self.kernel = kernel;
        self.max_output_frames = max_output_frames;
        self.max_source_frames = max_source_frames;
        Ok(())
    }

    /// Exact next source sample position, distinct from the pulled frontier.
    pub fn source_position(&self) -> SourcePosition {
        let frame = match &self.continuous {
            Some(position) => position.frame,
            None => {
                ((u128::from(self.output_cursor) * u128::from(self.source.sample_rate()))
                    / u128::from(self.target.sample_rate())) as u64
            }
        };
        let divisor = gcd64(self.fraction, self.denominator);
        SourcePosition {
            frame,
            numerator: self.fraction / divisor,
            denominator: self.denominator / divisor,
        }
    }

    /// Like [`new`](Self::new) with the mixer's format, also requiring that its
    /// configured render limit accepts every source block this converter pulls.
    pub fn for_mixer(
        config: MixerConfig,
        target: AudioFormat,
        matrix: ChannelMatrix,
        quality: ResampleQuality,
        max_output_frames: usize,
    ) -> Result<Self, AudioError> {
        let converter = Self::new(config.format(), target, matrix, quality, max_output_frames)?;
        if converter.max_source_frames > config.limits().max_render_frames() {
            return Err(AudioError::RenderCapacity);
        }
        Ok(converter)
    }

    /// Largest source block one render can request, for sizing
    /// [`AudioLimits::max_render_frames`] before constructing the mixer.
    pub fn required_source_frames(
        source: AudioFormat,
        target: AudioFormat,
        quality: ResampleQuality,
        max_output_frames: usize,
    ) -> Result<usize, AudioError> {
        let half_taps = quality.half_taps()? as u64;
        if max_output_frames == 0 || max_output_frames > AudioLimits::MAX_RENDER_FRAMES {
            return Err(AudioError::InvalidCapacity);
        }
        if source.sample_rate() == target.sample_rate() {
            return Ok(max_output_frames);
        }
        let divisor = gcd(source.sample_rate(), target.sample_rate());
        let (step, denominator) = (
            u64::from(source.sample_rate() / divisor),
            u64::from(target.sample_rate() / divisor),
        );
        // Worst initial phase is denominator - 1. The window spans the last
        // output frame's taps and every frame skipped before the next phase.
        let frames = max_output_frames as u64;
        let last = ((frames - 1) * step).div_ceil(denominator) + 2 * half_taps;
        let next = (frames * step).div_ceil(denominator);
        let required = last.max(next);
        if required > AudioLimits::MAX_RENDER_FRAMES as u64 {
            return Err(AudioError::RenderCapacity);
        }
        Ok(required as usize)
    }

    /// Source (mixer) format.
    pub const fn source_format(&self) -> AudioFormat {
        self.source
    }
    /// Device-side rate and channel count.
    pub const fn target_format(&self) -> AudioFormat {
        self.target
    }
    /// Configured kernel. Continuous converters use it for inherited fractional
    /// positions even while rates match.
    pub const fn quality(&self) -> ResampleQuality {
        self.quality
    }
    /// Channel matrix applied after rate conversion.
    pub const fn matrix(&self) -> &ChannelMatrix {
        &self.matrix
    }
    /// Largest output block accepted by one render.
    pub const fn max_output_frames(&self) -> usize {
        self.max_output_frames
    }
    /// Largest source block one render can request.
    pub const fn max_source_frames(&self) -> usize {
        self.max_source_frames
    }
    /// Kernel half-width in source frames, zero for legacy equal-rate paths.
    /// Continuity-capable paths retain history even at equal rates. This is not
    /// the current pulled-frame lead or a measured native presentation delay.
    /// Native mapping must also use original frame basis and output evidence.
    pub const fn source_lookahead_frames(&self) -> usize {
        self.half_taps
    }
    /// Total output frames produced by successful renders.
    pub const fn output_frame_cursor(&self) -> u64 {
        self.output_cursor
    }
    /// Total source frames successfully pulled.
    pub const fn source_frame_cursor(&self) -> u64 {
        self.source_cursor
    }
    /// Non-finite source samples replaced with silence; a correct Mixer never
    /// produces them.
    pub const fn sanitized_samples(&self) -> u64 {
        self.sanitized_samples
    }

    /// Fills `output` in the target format from one contiguous source block.
    ///
    /// Buffer alignment and extent are checked before calling `source`. The
    /// callback receives a slice of complete source frames, possibly empty, and
    /// its error is returned with converter phase and retained history unchanged.
    /// Source side effects and output writes cannot be rolled back. Cursor
    /// overflow rejects before source invocation. No allocation, lock or
    /// dynamic dispatch occurs here.
    pub fn render<R, E>(
        &mut self,
        output: &mut [f32],
        source: impl FnOnce(&mut [f32]) -> Result<R, E>,
    ) -> Result<R, E>
    where
        E: From<AudioError>,
    {
        let target_channels = usize::from(self.target.channels());
        let source_channels = usize::from(self.source.channels());
        if !output.len().is_multiple_of(target_channels) {
            return Err(AudioError::InvalidBuffer.into());
        }
        let frames = output.len() / target_channels;
        if frames > self.max_output_frames {
            return Err(AudioError::RenderCapacity.into());
        }
        if self.continuous.is_some() {
            return self.render_continuous(output, source);
        }
        if self.half_taps == 0 {
            self.validate_frames(frames, frames).map_err(E::from)?;
            if self.identity_mix {
                let report = source(output)?;
                self.count_sanitized(sanitize(output));
                self.commit_frames(frames, frames);
                return Ok(report);
            }
            let report = source(&mut self.window[..frames * source_channels])?;
            let sanitized = sanitize(&mut self.window[..frames * source_channels]);
            self.count_sanitized(sanitized);
            for (index, frame) in output.chunks_exact_mut(target_channels).enumerate() {
                let mut mixed = [0.0f64; MAX_CHANNELS];
                for (channel, value) in self.window
                    [index * source_channels..(index + 1) * source_channels]
                    .iter()
                    .enumerate()
                {
                    mixed[channel] = f64::from(*value);
                }
                self.mix_into(&mixed[..source_channels], frame);
            }
            self.commit_frames(frames, frames);
            return Ok(report);
        }

        let taps = 2 * self.half_taps;
        let (advance, needed) = if frames == 0 {
            (0, self.window_frames)
        } else {
            let frames = frames as u64;
            let last = (self.fraction + (frames - 1) * self.step) / self.denominator;
            let next = (self.fraction + frames * self.step) / self.denominator;
            (
                next as usize,
                (last as usize + taps)
                    .max(next as usize)
                    .max(self.window_frames),
            )
        };
        if needed > self.max_source_frames {
            return Err(AudioError::RenderCapacity.into());
        }
        let pulled = needed - self.window_frames;
        self.validate_frames(frames, pulled).map_err(E::from)?;
        let pulled_samples = self.window_frames * source_channels..needed * source_channels;
        let report = source(&mut self.window[pulled_samples.clone()])?;
        let sanitized = sanitize(&mut self.window[pulled_samples]);
        self.count_sanitized(sanitized);

        for (index, frame) in output.chunks_exact_mut(target_channels).enumerate() {
            let position = self.fraction + index as u64 * self.step;
            let whole = (position / self.denominator) as usize;
            let phase = (position % self.denominator) as f64 / self.denominator as f64;
            let mut weights = [0.0f64; 2 * ResampleQuality::MAX_HALF_TAPS as usize];
            let weights = &mut weights[..taps];
            if self.kernel.is_empty() {
                weights[0] = 1.0 - phase;
                weights[1] = phase;
            } else {
                let mut total = 0.0;
                for (tap, weight) in weights.iter_mut().enumerate() {
                    let distance = (tap as f64 - (self.half_taps - 1) as f64 - phase).abs();
                    *weight = kernel_at(&self.kernel, self.half_taps, distance);
                    total += *weight;
                }
                if total != 0.0 {
                    weights.iter_mut().for_each(|weight| *weight /= total);
                }
            }
            let mut mixed = [0.0f64; MAX_CHANNELS];
            for (tap, weight) in weights.iter().enumerate() {
                let start = (whole + tap) * source_channels;
                for (channel, value) in self.window[start..start + source_channels]
                    .iter()
                    .enumerate()
                {
                    mixed[channel] += f64::from(*value) * weight;
                }
            }
            self.mix_into(&mixed[..source_channels], frame);
        }

        self.window
            .copy_within(advance * source_channels..needed * source_channels, 0);
        self.window_frames = needed - advance;
        self.fraction = (self.fraction + frames as u64 * self.step) % self.denominator;
        self.commit_frames(frames, pulled);
        Ok(report)
    }

    fn render_continuous<R, E>(
        &mut self,
        output: &mut [f32],
        source: impl FnOnce(&mut [f32]) -> Result<R, E>,
    ) -> Result<R, E>
    where
        E: From<AudioError>,
    {
        let source_channels = usize::from(self.source.channels());
        let target_channels = usize::from(self.target.channels());
        let frames = output.len() / target_channels;
        if frames == 0 {
            return source(&mut []);
        }
        let position = self.continuous.as_ref().unwrap();
        let start = position.window_start;
        let first = position.frame;
        // u128 intermediates cover every u64 fraction, step and callback bound;
        // narrowing and all cursor checks precede the source call.
        let next_rational = u128::from(self.fraction) + frames as u128 * u128::from(self.step);
        let advance = u64::try_from(next_rational / u128::from(self.denominator))
            .map_err(|_| E::from(AudioError::Overflow))?;
        let next = first
            .checked_add(advance)
            .ok_or_else(|| E::from(AudioError::Overflow))?;
        let last_rational =
            u128::from(self.fraction) + (frames - 1) as u128 * u128::from(self.step);
        let last_advance = u64::try_from(last_rational / u128::from(self.denominator))
            .map_err(|_| E::from(AudioError::Overflow))?;
        let last = first
            .checked_add(last_advance)
            .ok_or_else(|| E::from(AudioError::Overflow))?;
        let needed_frontier = last
            .checked_add(self.half_taps as u64)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| E::from(AudioError::Overflow))?
            .max(next)
            .max(self.source_cursor);
        let pulled = usize::try_from(needed_frontier - self.source_cursor)
            .map_err(|_| E::from(AudioError::Overflow))?;
        let needed = self
            .window_frames
            .checked_add(pulled)
            .ok_or_else(|| E::from(AudioError::Overflow))?;
        if pulled > self.max_source_frames || needed > self.window.len() / source_channels {
            return Err(AudioError::RenderCapacity.into());
        }
        self.validate_frames(frames, pulled).map_err(E::from)?;
        let range = self.window_frames * source_channels..needed * source_channels;
        let report = source(&mut self.window[range.clone()])?;
        let sanitized = sanitize(&mut self.window[range]);
        self.count_sanitized(sanitized);
        let taps = 2 * self.half_taps;
        for (index, frame) in output.chunks_exact_mut(target_channels).enumerate() {
            let rational = u128::from(self.fraction) + index as u128 * u128::from(self.step);
            let whole = first + (rational / u128::from(self.denominator)) as u64;
            let remainder = (rational % u128::from(self.denominator)) as u64;
            let phase = remainder as f64 / self.denominator as f64;
            let mut mixed = [0.0f64; MAX_CHANNELS];
            if self.source.sample_rate() == self.target.sample_rate() && remainder == 0 {
                let offset = (whole - start) as usize * source_channels;
                for (channel, sample) in self.window[offset..offset + source_channels]
                    .iter()
                    .enumerate()
                {
                    mixed[channel] = f64::from(*sample);
                }
            } else {
                let mut weights = [0.0f64; 2 * ResampleQuality::MAX_HALF_TAPS as usize];
                let weights = &mut weights[..taps];
                if self.kernel.is_empty() {
                    weights[0] = 1.0 - phase;
                    weights[1] = phase;
                } else {
                    let mut total = 0.0;
                    for (tap, weight) in weights.iter_mut().enumerate() {
                        let distance = (tap as f64 - (self.half_taps - 1) as f64 - phase).abs();
                        *weight = kernel_at(&self.kernel, self.half_taps, distance);
                        total += *weight;
                    }
                    if total != 0.0 {
                        weights.iter_mut().for_each(|weight| *weight /= total);
                    }
                }
                for (tap, weight) in weights.iter().enumerate() {
                    let absolute = i128::from(whole) + tap as i128 - (self.half_taps - 1) as i128;
                    if absolute < 0 {
                        continue;
                    }
                    let offset = (absolute as u64 - start) as usize * source_channels;
                    for (channel, sample) in self.window[offset..offset + source_channels]
                        .iter()
                        .enumerate()
                    {
                        mixed[channel] += f64::from(*sample) * weight;
                    }
                }
            }
            if self.identity_mix
                && self.source.sample_rate() == self.target.sample_rate()
                && remainder == 0
            {
                for (destination, value) in frame.iter_mut().zip(&mixed) {
                    *destination = *value as f32;
                }
            } else {
                self.mix_into(&mixed[..source_channels], frame);
            }
        }
        let keep_start = next
            .saturating_sub((self.half_taps - 1) as u64)
            .min(needed_frontier);
        let discard = (keep_start - start) as usize;
        self.window
            .copy_within(discard * source_channels..needed * source_channels, 0);
        self.window_frames = needed - discard;
        self.fraction = (next_rational % u128::from(self.denominator)) as u64;
        let position = self.continuous.as_mut().unwrap();
        position.frame = next;
        position.window_start = keep_start;
        self.commit_frames(frames, pulled);
        Ok(report)
    }

    fn mix_into(&self, mixed: &[f64], frame: &mut [f32]) {
        let sources = mixed.len();
        if self.identity_mix {
            for (destination, value) in frame.iter_mut().zip(mixed) {
                *destination = value.clamp(-1.0, 1.0) as f32;
            }
            return;
        }
        for (target, destination) in frame.iter_mut().enumerate() {
            let gains = &self.matrix.coefficients[target * sources..(target + 1) * sources];
            let sum: f64 = gains
                .iter()
                .zip(mixed)
                .map(|(gain, value)| f64::from(*gain) * value)
                .sum();
            *destination = sum.clamp(-1.0, 1.0) as f32;
        }
    }

    fn count_sanitized(&mut self, count: u64) {
        self.sanitized_samples = self.sanitized_samples.saturating_add(count);
    }

    fn commit_frames(&mut self, output: usize, source: usize) {
        // validate_frames runs before invoking the source on every render path.
        self.output_cursor += output as u64;
        self.source_cursor += source as u64;
    }

    fn validate_frames(&self, output: usize, source: usize) -> Result<(), AudioError> {
        self.output_cursor
            .checked_add(output as u64)
            .ok_or(AudioError::Overflow)?;
        self.source_cursor
            .checked_add(source as u64)
            .ok_or(AudioError::Overflow)?;
        Ok(())
    }
}

#[cfg(test)]
mod cursor_fixtures {
    use super::*;

    #[test]
    fn cursor_overflow_refuses_source_before_mutating_output_or_conversion_state() {
        for (target_rate, channels) in [(48_000, 1), (48_000, 2), (44_100, 1)] {
            for source_overflow in [false, true] {
                let source = AudioFormat::new(48_000, 1).unwrap();
                let target = AudioFormat::new(target_rate, channels).unwrap();
                let mut converter = FormatConverter::new(
                    source,
                    target,
                    ChannelMatrix::default_mix(1, channels).unwrap(),
                    ResampleQuality::Linear,
                    4,
                )
                .unwrap();
                if source_overflow {
                    converter.source_cursor = u64::MAX;
                } else {
                    converter.output_cursor = u64::MAX;
                }
                let before = (
                    converter.output_cursor,
                    converter.source_cursor,
                    converter.fraction,
                    converter.window_frames,
                    converter.window.clone(),
                );
                let mut output = vec![0.75; usize::from(channels)];
                assert_eq!(
                    converter.render(&mut output, |_| -> Result<(), AudioError> {
                        panic!("source must not run on cursor overflow")
                    }),
                    Err(AudioError::Overflow)
                );
                assert_eq!(
                    (
                        converter.output_cursor,
                        converter.source_cursor,
                        converter.fraction,
                        converter.window_frames,
                        converter.window.clone()
                    ),
                    before
                );
                assert!(output.iter().all(|value| *value == 0.75));
            }
        }
    }
}

// Replaces non-finite samples with silence so they cannot reach the device.
fn sanitize(values: &mut [f32]) -> u64 {
    let mut count = 0;
    for value in values.iter_mut().filter(|value| !value.is_finite()) {
        *value = 0.0;
        count += 1;
    }
    count
}

fn sinc_table(half_taps: usize, cutoff: f64) -> Result<Vec<f32>, AudioError> {
    let len = half_taps * KERNEL_RESOLUTION + 2;
    let mut table = Vec::new();
    table
        .try_reserve_exact(len)
        .map_err(|_| AudioError::AllocationFailed)?;
    table.extend((0..len).map(|index| {
        let distance = index as f64 / KERNEL_RESOLUTION as f64;
        if distance >= half_taps as f64 {
            return 0.0;
        }
        let x = std::f64::consts::PI * cutoff * distance;
        let sinc = if x == 0.0 { 1.0 } else { x.sin() / x };
        let t = std::f64::consts::PI * distance / half_taps as f64;
        let window = 0.42 + 0.5 * t.cos() + 0.08 * (2.0 * t).cos();
        (cutoff * sinc * window) as f32
    }));
    Ok(table)
}

fn kernel_at(table: &[f32], half_taps: usize, distance: f64) -> f64 {
    if distance >= half_taps as f64 {
        return 0.0;
    }
    let position = distance * KERNEL_RESOLUTION as f64;
    let index = position as usize;
    let blend = position - index as f64;
    f64::from(table[index]) * (1.0 - blend) + f64::from(table[index + 1]) * blend
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn gcd64(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}
