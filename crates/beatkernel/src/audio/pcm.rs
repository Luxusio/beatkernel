use super::{AudioError, AudioFormat, PcmLimits, SampleId};
use std::fmt;

/// Owned, finite interleaved PCM decoded before output starts.
///
/// Assets are not cloned or freed by rendering; the sample bank owns them for
/// the mixer's entire lifetime. Sample amplitudes need not be normalized.
#[derive(Debug)]
pub struct PcmSample {
    format: AudioFormat,
    samples: Vec<f32>,
}

impl PcmSample {
    /// Validates caller-owned PCM without allocating a second sample buffer.
    pub fn new(
        format: AudioFormat,
        samples: Vec<f32>,
        limits: PcmLimits,
    ) -> Result<Self, AudioError> {
        validate_storage(format, samples.len(), limits)?;
        if samples.iter().any(|value| !value.is_finite()) {
            return Err(AudioError::NonFiniteSample);
        }
        Ok(Self { format, samples })
    }

    /// Decodes strict RIFF WAVE PCM16/24/32 or IEEE float32 off the audio thread.
    ///
    /// All chunks, format fields, limits and floating samples are validated
    /// before decoded storage is allocated. Unknown chunks are skipped; data
    /// may precede fmt. An empty data chunk is a valid empty asset.
    pub fn from_wav(bytes: &[u8], limits: PcmLimits) -> Result<Self, WavError> {
        let (format_bytes, data) = wav_chunks(bytes)?;
        let (format, encoding, width, valid_bits) = wav_format(format_bytes)?;
        let frame_bytes = usize::from(format.channels()) * width;
        if data.len() % frame_bytes != 0 {
            return Err(WavError::Malformed);
        }
        let sample_count = data.len() / width;
        validate_storage(format, sample_count, limits)?;
        if encoding == Encoding::Float
            && data
                .as_chunks::<4>()
                .0
                .iter()
                .any(|sample| !float_sample(sample).is_finite())
        {
            return Err(AudioError::NonFiniteSample.into());
        }
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(sample_count)
            .map_err(|_| AudioError::AllocationFailed)?;
        for sample in data.chunks_exact(width) {
            let value = if encoding == Encoding::Float {
                float_sample(sample)
            } else {
                let signed = match width {
                    2 => i32::from(i16::from_le_bytes([sample[0], sample[1]])),
                    3 => i32::from_le_bytes([
                        sample[0],
                        sample[1],
                        sample[2],
                        if sample[2] & 0x80 == 0 { 0 } else { 0xff },
                    ]),
                    4 => i32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]),
                    _ => unreachable!("validated WAV sample width"),
                };
                // Extensible PCM valid bits are left-aligned. Discard unused
                // low bits before normalization, including any nonzero padding.
                let shift = width * 8 - usize::from(valid_bits);
                (signed >> shift) as f32 / (1u64 << (valid_bits - 1)) as f32
            };
            samples.push(value);
        }
        Ok(Self { format, samples })
    }

    /// Asset sample rate and channel count; no implicit conversion is applied.
    pub const fn format(&self) -> AudioFormat {
        self.format
    }

    /// Number of complete interleaved frames.
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.format.channels())
    }

    /// Immutable decoded interleaved storage.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Transfers decoded storage during setup without copying the PCM buffer.
    pub fn into_samples(self) -> Vec<f32> {
        self.samples
    }
}

/// Setup-time immutable asset ownership with deterministic sorted lookup.
#[derive(Debug)]
pub struct SampleBank {
    format: AudioFormat,
    limits: PcmLimits,
    samples: Vec<(SampleId, PcmSample)>,
    total_bytes: usize,
}

impl SampleBank {
    /// Creates an empty bank; insertion and allocation happen before rendering.
    pub fn new(format: AudioFormat, limits: PcmLimits) -> Result<Self, AudioError> {
        Ok(Self {
            format,
            limits,
            samples: Vec::new(),
            total_bytes: 0,
        })
    }

    /// Inserts an asset without replacing duplicate identities.
    ///
    /// Source sample rates may differ; channels must match the mix format.
    /// Validation/allocation failure leaves existing bank state unchanged.
    pub fn insert(&mut self, id: SampleId, sample: PcmSample) -> Result<(), AudioError> {
        let index = match self.samples.binary_search_by_key(&id, |entry| entry.0) {
            Ok(_) => return Err(AudioError::DuplicateSample),
            Err(index) => index,
        };
        if sample.format.channels() != self.format.channels() {
            return Err(AudioError::ChannelMismatch);
        }
        validate_storage(sample.format, sample.samples.len(), self.limits)?;
        let bytes = pcm_bytes(sample.samples.len())?;
        let total_bytes = self
            .total_bytes
            .checked_add(bytes)
            .ok_or(AudioError::Overflow)?;
        if self.samples.len() >= self.limits.max_samples()
            || total_bytes > self.limits.max_total_bytes()
        {
            return Err(AudioError::PcmCapacity);
        }
        self.samples
            .try_reserve(1)
            .map_err(|_| AudioError::AllocationFailed)?;
        self.samples.insert(index, (id, sample));
        self.total_bytes = total_bytes;
        Ok(())
    }

    /// Borrows an asset without allocation or ownership changes.
    pub fn get(&self, id: SampleId) -> Option<&PcmSample> {
        self.samples
            .binary_search_by_key(&id, |entry| entry.0)
            .ok()
            .map(|index| &self.samples[index].1)
    }

    /// Transfers setup-time assets without copying decoded PCM buffers.
    /// The bank is consumed; rendering owners must already be stopped.
    pub fn into_samples(self) -> impl Iterator<Item = (SampleId, PcmSample)> {
        self.samples.into_iter()
    }

    /// Chosen output format; individual assets retain their own sample rates.
    pub const fn format(&self) -> AudioFormat {
        self.format
    }

    /// Number of owned assets, including empty assets.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no asset identities are stored.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Logical decoded f32 storage bytes, excluding container bookkeeping.
    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

/// Typed offline decoding failure; diagnostics can be formatted off RT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavError {
    /// Invalid RIFF/chunk bounds, padding, format fields or frame alignment.
    Malformed,
    /// Compressed/unknown encoding, width or extensible subformat.
    UnsupportedFormat,
    /// A required fmt or data chunk is absent.
    MissingChunk,
    /// More than one fmt or data chunk was supplied.
    DuplicateChunk,
    /// PCM format, sample, allocation or configured capacity error.
    Audio(AudioError),
}

impl From<AudioError> for WavError {
    fn from(error: AudioError) -> Self {
        Self::Audio(error)
    }
}

impl fmt::Display for WavError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => formatter.write_str("malformed RIFF WAVE structure or format"),
            Self::UnsupportedFormat => formatter.write_str("unsupported WAVE encoding or width"),
            Self::MissingChunk => formatter.write_str("WAVE requires one fmt and one data chunk"),
            Self::DuplicateChunk => formatter.write_str("duplicate WAVE fmt or data chunk"),
            Self::Audio(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for WavError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Audio(error) => Some(error),
            _ => None,
        }
    }
}

fn pcm_bytes(samples: usize) -> Result<usize, AudioError> {
    samples
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or(AudioError::Overflow)
}

fn validate_storage(
    format: AudioFormat,
    samples: usize,
    limits: PcmLimits,
) -> Result<(), AudioError> {
    if !samples.is_multiple_of(usize::from(format.channels())) {
        return Err(AudioError::InvalidBuffer);
    }
    if pcm_bytes(samples)? > limits.max_asset_bytes() {
        return Err(AudioError::PcmCapacity);
    }
    Ok(())
}

fn wav_chunks(bytes: &[u8]) -> Result<(&[u8], &[u8]), WavError> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(WavError::Malformed);
    }
    let declared_end = usize::try_from(read_u32(bytes, 4))
        .ok()
        .and_then(|size| size.checked_add(8))
        .ok_or(WavError::Malformed)?;
    if declared_end != bytes.len() {
        return Err(WavError::Malformed);
    }
    let mut format = None;
    let mut data = None;
    let mut cursor = 12usize;
    while cursor < bytes.len() {
        let start = cursor.checked_add(8).ok_or(WavError::Malformed)?;
        if start > bytes.len() {
            return Err(WavError::Malformed);
        }
        let size = usize::try_from(read_u32(bytes, cursor + 4)).map_err(|_| WavError::Malformed)?;
        let end = start.checked_add(size).ok_or(WavError::Malformed)?;
        let padded_end = end.checked_add(size % 2).ok_or(WavError::Malformed)?;
        if padded_end > bytes.len() {
            return Err(WavError::Malformed);
        }
        let destination = match &bytes[cursor..cursor + 4] {
            b"fmt " => Some(&mut format),
            b"data" => Some(&mut data),
            _ => None,
        };
        if let Some(destination) = destination {
            if destination.replace(&bytes[start..end]).is_some() {
                return Err(WavError::DuplicateChunk);
            }
        }
        cursor = padded_end;
    }
    Ok((
        format.ok_or(WavError::MissingChunk)?,
        data.ok_or(WavError::MissingChunk)?,
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Integer,
    Float,
}

fn wav_format(bytes: &[u8]) -> Result<(AudioFormat, Encoding, usize, u16), WavError> {
    if bytes.len() < 16 {
        return Err(WavError::Malformed);
    }
    let channels = read_u16(bytes, 2);
    let rate = read_u32(bytes, 4);
    let format = AudioFormat::new(rate, channels)?;
    let bits = read_u16(bytes, 14);
    let tag = read_u16(bytes, 0);
    let mut valid_bits = bits;
    let encoding_tag = if tag == 0xfffe {
        if bytes.len() < 40
            || read_u16(bytes, 16) < 22
            || usize::from(read_u16(bytes, 16)) + 18 > bytes.len()
        {
            return Err(WavError::Malformed);
        }
        valid_bits = read_u16(bytes, 18);
        let mask = read_u32(bytes, 20);
        if valid_bits == 0
            || valid_bits > bits
            || (mask != 0 && mask.count_ones() != u32::from(channels))
            || mask & !0x0003_ffff != 0
        {
            return Err(WavError::Malformed);
        }
        let guid = &bytes[24..40];
        const SUFFIX: [u8; 12] = [0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113];
        if guid[4..] != SUFFIX || (read_u32(guid, 0) != 1 && read_u32(guid, 0) != 3) {
            return Err(WavError::UnsupportedFormat);
        }
        read_u32(guid, 0) as u16
    } else {
        tag
    };
    let encoding = match encoding_tag {
        1 if matches!(bits, 16 | 24 | 32) => Encoding::Integer,
        3 if bits == 32 && valid_bits == 32 => Encoding::Float,
        3 if bits == 32 => return Err(WavError::Malformed),
        _ => return Err(WavError::UnsupportedFormat),
    };
    let width = usize::from(bits / 8);
    let block_alignment = u32::from(channels) * u32::from(bits / 8);
    if u32::from(read_u16(bytes, 12)) != block_alignment
        || rate.checked_mul(block_alignment) != Some(read_u32(bytes, 8))
    {
        return Err(WavError::Malformed);
    }
    Ok((format, encoding, width, valid_bits))
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn float_sample(bytes: &[u8]) -> f32 {
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
