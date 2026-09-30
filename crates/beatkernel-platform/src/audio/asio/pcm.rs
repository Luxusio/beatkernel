//! Allocation-free conversion of one interleaved mixer channel to ASIO PCM.

use crate::audio::convert::quantize_signed_pcm;

/// Native ASIO PCM sample layout and byte order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AsioPcmEncoding {
    /// Big-endian signed 16-bit integer.
    Int16Msb,
    /// Big-endian signed 24-bit integer.
    Int24Msb,
    /// Big-endian signed 32-bit integer.
    Int32Msb,
    /// Big-endian float32.
    Float32Msb,
    /// Big-endian float64.
    Float64Msb,
    /// Big-endian 32-bit container with 16 low valid bits.
    Int32Msb16,
    /// Big-endian 32-bit container with 18 low valid bits.
    Int32Msb18,
    /// Big-endian 32-bit container with 20 low valid bits.
    Int32Msb20,
    /// Big-endian 32-bit container with 24 low valid bits.
    Int32Msb24,
    /// Little-endian signed 16-bit integer.
    Int16Lsb,
    /// Little-endian signed 24-bit integer.
    Int24Lsb,
    /// Little-endian signed 32-bit integer.
    Int32Lsb,
    /// Little-endian float32.
    Float32Lsb,
    /// Little-endian float64.
    Float64Lsb,
    /// Little-endian 32-bit container with 16 low valid bits.
    Int32Lsb16,
    /// Little-endian 32-bit container with 18 low valid bits.
    Int32Lsb18,
    /// Little-endian 32-bit container with 20 low valid bits.
    Int32Lsb20,
    /// Little-endian 32-bit container with 24 low valid bits.
    Int32Lsb24,
}

impl AsioPcmEncoding {
    /// Resolves a native ASIO sample type, rejecting DSD and unknown identities.
    pub fn from_native(sample_type: i32) -> Result<Self, AsioPcmError> {
        Ok(match sample_type {
            0 => Self::Int16Msb,
            1 => Self::Int24Msb,
            2 => Self::Int32Msb,
            3 => Self::Float32Msb,
            4 => Self::Float64Msb,
            8 => Self::Int32Msb16,
            9 => Self::Int32Msb18,
            10 => Self::Int32Msb20,
            11 => Self::Int32Msb24,
            16 => Self::Int16Lsb,
            17 => Self::Int24Lsb,
            18 => Self::Int32Lsb,
            19 => Self::Float32Lsb,
            20 => Self::Float64Lsb,
            24 => Self::Int32Lsb16,
            25 => Self::Int32Lsb18,
            26 => Self::Int32Lsb20,
            27 => Self::Int32Lsb24,
            _ => return Err(AsioPcmError::UnsupportedSampleType { sample_type }),
        })
    }

    /// Returns the SDK's native sample-type identity.
    pub const fn native_type(self) -> i32 {
        match self {
            Self::Int16Msb => 0,
            Self::Int24Msb => 1,
            Self::Int32Msb => 2,
            Self::Float32Msb => 3,
            Self::Float64Msb => 4,
            Self::Int32Msb16 => 8,
            Self::Int32Msb18 => 9,
            Self::Int32Msb20 => 10,
            Self::Int32Msb24 => 11,
            Self::Int16Lsb => 16,
            Self::Int24Lsb => 17,
            Self::Int32Lsb => 18,
            Self::Float32Lsb => 19,
            Self::Float64Lsb => 20,
            Self::Int32Lsb16 => 24,
            Self::Int32Lsb18 => 25,
            Self::Int32Lsb20 => 26,
            Self::Int32Lsb24 => 27,
        }
    }

    /// Returns the planar buffer extent required for each sample.
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::Int16Msb | Self::Int16Lsb => 2,
            Self::Int24Msb | Self::Int24Lsb => 3,
            Self::Float64Msb | Self::Float64Lsb => 8,
            _ => 4,
        }
    }

    fn write_sample(self, value: f32, output: &mut [u8]) {
        match self {
            Self::Float32Msb => output.copy_from_slice(&value.to_be_bytes()),
            Self::Float32Lsb => output.copy_from_slice(&value.to_le_bytes()),
            Self::Float64Msb => output.copy_from_slice(&f64::from(value).to_be_bytes()),
            Self::Float64Lsb => output.copy_from_slice(&f64::from(value).to_le_bytes()),
            _ => {
                let bits = match self {
                    Self::Int16Msb | Self::Int16Lsb | Self::Int32Msb16 | Self::Int32Lsb16 => 16,
                    Self::Int32Msb18 | Self::Int32Lsb18 => 18,
                    Self::Int32Msb20 | Self::Int32Lsb20 => 20,
                    Self::Int24Msb | Self::Int24Lsb | Self::Int32Msb24 | Self::Int32Lsb24 => 24,
                    _ => 32,
                };
                // Reduced-width containers retain only the low valid bits;
                // signed full-width samples also use their two's-complement bits.
                let encoded =
                    (quantize_signed_pcm(value, bits) as u32) & ((1u64 << bits) - 1) as u32;
                let little_endian = matches!(
                    self,
                    Self::Int16Lsb
                        | Self::Int24Lsb
                        | Self::Int32Lsb
                        | Self::Int32Lsb16
                        | Self::Int32Lsb18
                        | Self::Int32Lsb20
                        | Self::Int32Lsb24
                );
                let width = output.len();
                if little_endian {
                    output.copy_from_slice(&encoded.to_le_bytes()[..width]);
                } else {
                    output.copy_from_slice(&encoded.to_be_bytes()[4 - width..]);
                }
            }
        }
    }
}

/// ASIO PCM conversion failure; every failure leaves the destination untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioPcmError {
    /// Native sample type is unsupported, including DSD.
    UnsupportedSampleType {
        /// Original native identity.
        sample_type: i32,
    },
    /// Channel selection or interleaved frame layout is invalid.
    InvalidLayout,
    /// Destination length does not exactly match the selected planar channel.
    OutputSize,
    /// Required destination extent cannot be represented by usize.
    ExtentOverflow,
    /// A selected-channel sample is NaN or infinite.
    NonFiniteSample,
}

impl std::fmt::Display for AsioPcmError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSampleType { sample_type } => {
                write!(formatter, "unsupported ASIO sample type {sample_type}")
            }
            Self::InvalidLayout => formatter.write_str("invalid ASIO PCM channel layout"),
            Self::OutputSize => formatter.write_str("ASIO PCM output size mismatch"),
            Self::ExtentOverflow => formatter.write_str("ASIO PCM extent overflow"),
            Self::NonFiniteSample => {
                formatter.write_str("nonfinite ASIO PCM selected-channel sample")
            }
        }
    }
}

impl std::error::Error for AsioPcmError {}

/// Encodes one explicit interleaved channel into an exactly sized planar buffer.
///
/// All layout, extent and selected-channel finiteness checks precede writes.
/// Unselected channels are ignored. Integer samples clamp and quantize with
/// ties away from zero; reduced-valid-bit containers zero unused high bits.
/// Floating samples preserve finite values without clipping. This performs no
/// allocation, locking, I/O, channel mixing or device access.
pub fn encode_asio_channel(
    encoding: AsioPcmEncoding,
    input: &[f32],
    channels: usize,
    channel: usize,
    output: &mut [u8],
) -> Result<(), AsioPcmError> {
    if channels == 0 || channel >= channels || !input.len().is_multiple_of(channels) {
        return Err(AsioPcmError::InvalidLayout);
    }
    let width = encoding.bytes_per_sample();
    let expected = (input.len() / channels)
        .checked_mul(width)
        .ok_or(AsioPcmError::ExtentOverflow)?;
    if output.len() != expected {
        return Err(AsioPcmError::OutputSize);
    }
    if input
        .chunks_exact(channels)
        .any(|frame| !frame[channel].is_finite())
    {
        return Err(AsioPcmError::NonFiniteSample);
    }
    for (frame, bytes) in input
        .chunks_exact(channels)
        .zip(output.chunks_exact_mut(width))
    {
        encoding.write_sample(frame[channel], bytes);
    }
    Ok(())
}
