//! Off-thread music section preparation on a fresh output timeline.
//!
//! The host still owns native stop/reset/start and judge reconstruction. A
//! prepared mixer cannot flush a different stream's already-buffered samples.

use crate::{
    audio::{
        AudioCommand, AudioError, CommandProducer, CommandPushError, Mixer, MixerConfig, PcmLimits,
        PcmSample, SampleBank, SampleId, VoiceId, command_queue,
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
    transport::{Rate, Transport},
};

const NANOS: i128 = 1_000_000_000;

/// Explicit policy for choosing a source frame at a requested song position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRounding {
    /// Reject positions that do not lie exactly on a rational source frame.
    Exact,
    /// Select the frame at or immediately before the requested position.
    Floor,
    /// Select the frame at or immediately after the requested position.
    Ceil,
    /// Select the closest frame; exact half-frame ties choose the later frame.
    Nearest,
}

/// Explicit preparation failure; existing output owners are never touched.
#[derive(Debug)]
pub enum RestartError {
    /// Requested position precedes the source origin or selected frame exceeds EOF.
    OutsideSource,
    /// Exact selection was requested for a position between source frames.
    BetweenFrames,
    /// Applied song time cannot be represented as signed integer nanoseconds.
    Overflow,
    /// No explicit relation maps the output origin to the host domain.
    UnmappedClock,
    /// PCM capacity, allocation, channel or mixer setup failure.
    Audio(AudioError),
    /// Fresh initial command could not be admitted.
    Command(CommandPushError),
}
impl From<AudioError> for RestartError {
    fn from(value: AudioError) -> Self {
        Self::Audio(value)
    }
}
impl std::fmt::Display for RestartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "section restart: {self:?}")
    }
}
impl std::error::Error for RestartError {}

/// Selection borrowed from the original immutable PCM; no iterative rounding.
pub struct RestartPlan<'a> {
    source: &'a PcmSample,
    requested: Timestamp,
    applied: Timestamp,
    frame: usize,
}
impl<'a> RestartPlan<'a> {
    /// Selects a source frame with wide integer arithmetic and explicit rounding.
    ///
    /// The selected frame's rational time is rounded upward to nanoseconds once.
    /// EOF is permitted and produces an empty suffix. Before-origin preroll must
    /// be represented by the host separately rather than silently clamped.
    pub fn select(
        source: &'a PcmSample,
        source_origin: Timestamp,
        requested: Timestamp,
        rounding: FrameRounding,
    ) -> Result<Self, RestartError> {
        let delta = i128::from(requested.as_nanos()) - i128::from(source_origin.as_nanos());
        if delta < 0 {
            return Err(RestartError::OutsideSource);
        }
        let numerator = delta * i128::from(source.format().sample_rate());
        let whole = numerator / NANOS;
        let remainder = numerator % NANOS;
        let frame = match rounding {
            FrameRounding::Exact if remainder != 0 => return Err(RestartError::BetweenFrames),
            FrameRounding::Ceil => whole + i128::from(remainder != 0),
            FrameRounding::Nearest => whole + i128::from(remainder * 2 >= NANOS),
            _ => whole,
        };
        let frame = usize::try_from(frame).map_err(|_| RestartError::OutsideSource)?;
        if frame > source.frames() {
            return Err(RestartError::OutsideSource);
        }
        let rate = i128::from(source.format().sample_rate());
        let frame_nanos = ((frame as i128) * NANOS + rate - 1) / rate;
        let applied = i128::from(source_origin.as_nanos()) + frame_nanos;
        let applied = i64::try_from(applied)
            .map(Timestamp::from_nanos)
            .map_err(|_| RestartError::Overflow)?;
        Ok(Self {
            source,
            requested,
            applied,
            frame,
        })
    }
    /// Requested unoffset song time.
    pub const fn requested_song_time(&self) -> Timestamp {
        self.requested
    }
    /// Selected frame represented on the integer song timeline.
    pub const fn applied_song_time(&self) -> Timestamp {
        self.applied
    }
    /// Absolute frame index in the original source, before any resampling.
    pub const fn source_frame(&self) -> usize {
        self.frame
    }
    /// Signed applied-minus-requested correction, with a wide representation.
    pub fn correction_nanos(&self) -> i128 {
        i128::from(self.applied.as_nanos()) - i128::from(self.requested.as_nanos())
    }
    /// Copies only the selected suffix off-thread with explicit allocation limits.
    pub fn copy_pcm(&self, limits: PcmLimits) -> Result<PcmSample, RestartError> {
        let start = self
            .frame
            .checked_mul(usize::from(self.source.format().channels()))
            .ok_or(RestartError::Overflow)?;
        let samples = &self.source.samples()[start..];
        let bytes = samples
            .len()
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or(RestartError::Overflow)?;
        if bytes > limits.max_asset_bytes() {
            return Err(AudioError::PcmCapacity.into());
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(samples.len())
            .map_err(|_| AudioError::AllocationFailed)?;
        owned.extend_from_slice(samples);
        Ok(PcmSample::new(self.source.format(), owned, limits)?)
    }
    /// Creates a new music-only bank, queue and mixer with Play at output frame zero.
    ///
    /// No old queue, voices or native stream are accepted. Channels must match;
    /// existing Mixer resampling handles source/output rate differences. The host
    /// may use `copy_pcm` with a larger custom bank to include keysound assets.
    pub fn prepare_audio(
        &self,
        config: MixerConfig,
        pcm_limits: PcmLimits,
        sample: SampleId,
        voice: VoiceId,
    ) -> Result<PreparedAudioRestart, RestartError> {
        let mut bank = SampleBank::new(config.format(), pcm_limits)?;
        bank.insert(sample, self.copy_pcm(pcm_limits)?)?;
        let (mut producer, consumer) = command_queue(config.limits().queue_capacity())?;
        let mixer = Mixer::new(config, bank, consumer)?;
        producer
            .try_push(AudioCommand::Play {
                voice,
                sample,
                at: config.origin(),
                gain: 1.0,
            })
            .map_err(RestartError::Command)?;
        Ok(PreparedAudioRestart {
            producer,
            mixer,
            applied_song_time: self.applied,
            output_origin: ClockPoint {
                domain: config.domain(),
                timestamp: config.origin(),
            },
        })
    }
}

/// New output owners prepared before the host changes the running stream.
pub struct PreparedAudioRestart {
    /// Fresh producer for subsequent runtime keysounds or playback control.
    pub producer: CommandProducer,
    /// Fresh music mixer; install only after old native output is stopped/reset.
    pub mixer: Mixer,
    /// Reconstruct the logical judge at this unoffset song position.
    pub applied_song_time: Timestamp,
    /// New output frame zero containing the first selected music sample.
    pub output_origin: ClockPoint,
}
impl PreparedAudioRestart {
    /// Builds normal-speed Transport from an explicitly supplied output/host mapping.
    ///
    /// The host must calibrate this mapping against the first sample's observed
    /// presentation. Unknown quality is preserved, never promoted to exact. A
    /// submitted-frame cursor alone cannot establish physical presentation time.
    pub fn mapped_transport(
        &self,
        host_domain: ClockDomainId,
        mapper: &dyn ClockMapper,
    ) -> Result<(Transport, ClockMappingQuality), RestartError> {
        let host_time = mapper
            .map(self.output_origin, host_domain)
            .ok_or(RestartError::UnmappedClock)?;
        Ok((
            Transport::new(host_time, self.applied_song_time, Rate::NORMAL),
            mapper.quality(),
        ))
    }
}
