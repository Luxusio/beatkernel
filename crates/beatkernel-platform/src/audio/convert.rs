use super::{AudioPlatformError, DeviceFormat, SampleEncoding};

/// Converts complete finite interleaved PCM into an exactly sized native buffer.
///
/// Every input and extent is checked before any output write. Integer encoding
/// clamps, rounds ties away from zero, saturates and left-aligns valid bits.
/// Float32 preserves finite values. No allocation, dither or remapping occurs.
pub fn encode_pcm(
    format: DeviceFormat,
    input: &[f32],
    output: &mut [u8],
) -> Result<(), AudioPlatformError> {
    let width = usize::from(format.encoding().bytes_per_sample());
    if !input.len().is_multiple_of(usize::from(format.channels()))
        || input.len().checked_mul(width) != Some(output.len())
        || input.iter().any(|value| !value.is_finite())
    {
        return Err(AudioPlatformError::InvalidFormat);
    }
    for (value, bytes) in input.iter().zip(output.chunks_exact_mut(width)) {
        match format.encoding() {
            SampleEncoding::Float32 => bytes.copy_from_slice(&value.to_le_bytes()),
            SampleEncoding::Pcm {
                container_bits,
                valid_bits,
            } => {
                let signed = quantize_signed_pcm(*value, valid_bits);
                let encoded = (signed << (container_bits - valid_bits)).to_le_bytes();
                bytes.copy_from_slice(&encoded[..width]);
            }
        }
    }
    Ok(())
}

// Callers validate finite samples and derive widths from validated formats.
// The result is signed valid-bit PCM before choosing native container alignment.
pub(super) fn quantize_signed_pcm(value: f32, valid_bits: u16) -> i64 {
    let scale = 1i64 << (valid_bits - 1);
    (f64::from(value).clamp(-1.0, 1.0) * scale as f64)
        .round()
        .clamp(-scale as f64, (scale - 1) as f64) as i64
}
