//! Preallocated Mixer delivery into a complete set of ASIO planar buffers.

use super::{encode_asio_channel, AsioPcmEncoding, AsioPcmError};
use beatkernel::audio::{AudioError, AudioFormat, Mixer, RenderReport};
use std::{error::Error, fmt};

/// Setup, Mixer or planar output failure without implicit resizing or remapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioRenderError {
    /// Nonpositive frame count or encoding count mismatch.
    InvalidConfiguration,
    /// Output count or an exact channel extent does not match setup.
    InvalidBuffers,
    /// Mixer render limit, setup extent or allocation failed.
    Capacity,
    /// The actual core Mixer rejected rendering.
    Core(AudioError),
    /// Mixed samples or native PCM conversion failed.
    Pcm(AsioPcmError),
}
impl fmt::Display for AsioRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => f.write_str("invalid ASIO Mixer block configuration"),
            Self::InvalidBuffers => f.write_str("ASIO output planes do not match exact setup"),
            Self::Capacity => f.write_str("ASIO Mixer setup capacity/allocation failure"),
            Self::Core(error) => write!(f, "ASIO core Mixer: {error}"),
            Self::Pcm(error) => write!(f, "ASIO output PCM: {error}"),
        }
    }
}
impl Error for AsioRenderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            Self::Pcm(error) => Some(error),
            _ => None,
        }
    }
}

/// Actual Mixer plus off-thread allocated scratch for fixed native buffer frames.
///
/// Rendering validates every destination before advancing the Mixer, then checks
/// the entire mixed block before writing any plane. A successful Mixer report is
/// retained even if a later finite-sample check rejects native delivery.
pub struct AsioBlockRenderer {
    mixer: Mixer,
    format: AudioFormat,
    frames: u32,
    encodings: Vec<AsioPcmEncoding>,
    scratch: Vec<f32>,
    last_report: Option<RenderReport>,
}
impl AsioBlockRenderer {
    /// Allocates scratch off-thread, using the Mixer's actual format and limits.
    /// Encodings map in mixer-channel order; no hidden extra channels are created.
    pub fn new(
        mixer: Mixer,
        frames: u32,
        encodings: Vec<AsioPcmEncoding>,
    ) -> Result<Self, AsioRenderError> {
        let config = mixer.configuration();
        let format = config.format();
        if frames == 0 || encodings.len() != usize::from(format.channels()) {
            return Err(AsioRenderError::InvalidConfiguration);
        }
        if frames as usize > config.limits().max_render_frames() {
            return Err(AsioRenderError::Capacity);
        }
        let samples = (frames as usize)
            .checked_mul(encodings.len())
            .ok_or(AsioRenderError::Capacity)?;
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(samples)
            .map_err(|_| AsioRenderError::Capacity)?;
        scratch.resize(samples, 0.0);
        Ok(Self {
            mixer,
            format,
            frames,
            encodings,
            scratch,
            last_report: None,
        })
    }
    /// Configured native frame count for every render.
    pub const fn frames(&self) -> u32 {
        self.frames
    }
    /// Actual immutable Mixer format.
    pub const fn format(&self) -> AudioFormat {
        self.format
    }
    /// Native encoding per mixer channel, retained in explicit setup order.
    pub fn encodings(&self) -> &[AsioPcmEncoding] {
        &self.encodings
    }
    /// Last successful Mixer report, independently of native delivery success.
    pub const fn last_render_report(&self) -> Option<RenderReport> {
        self.last_report
    }
    /// Fills all exact planar buffers without allocation, locks, I/O or decoding.
    /// Bad destination layouts do not consume commands or advance the Mixer.
    /// Nonfinite mixed output leaves every native destination untouched.
    pub fn render(&mut self, outputs: &mut [&mut [u8]]) -> Result<RenderReport, AsioRenderError> {
        if outputs.len() != self.encodings.len()
            || outputs
                .iter()
                .zip(&self.encodings)
                .any(|(output, encoding)| {
                    (self.frames as usize).checked_mul(encoding.bytes_per_sample())
                        != Some(output.len())
                })
        {
            return Err(AsioRenderError::InvalidBuffers);
        }
        let report = self
            .mixer
            .render(&mut self.scratch)
            .map_err(AsioRenderError::Core)?;
        self.last_report = Some(report);
        if self.scratch.iter().any(|sample| !sample.is_finite()) {
            return Err(AsioRenderError::Pcm(AsioPcmError::NonFiniteSample));
        }
        for (channel, (output, encoding)) in outputs.iter_mut().zip(&self.encodings).enumerate() {
            encode_asio_channel(
                *encoding,
                &self.scratch,
                usize::from(self.format.channels()),
                channel,
                output,
            )
            .map_err(AsioRenderError::Pcm)?;
        }
        Ok(report)
    }
}
