//! Strict native FLAC preparation; no filesystem or native codec ownership.
use crate::AssetDecoder;
use beatkernel::audio::{AudioFormat, PcmLimits, PcmSample};
use claxon::{FlacReader, FlacReaderOptions, frame::FrameReader, metadata::StreamInfo};
use std::{
    error::Error,
    io::{self, Cursor},
    path::Path,
};

/// Decodes native FLAC to finite source-rate interleaved PCM outside callbacks.
/// Claxon validates compressed frames/CRCs; MD5 is not checked. Library frame
/// scratch is independent of the caller's decoded-output cap (up to 65535×8
/// i32 samples logically), so this is not a total-memory or CPU sandbox.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlacDecoder;
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn audio_offset(bytes: &[u8]) -> io::Result<usize> {
    let mut offset = 4usize;
    loop {
        let header = bytes
            .get(offset..offset + 4)
            .ok_or_else(|| invalid("truncated FLAC metadata"))?;
        let last = header[0] & 0x80 != 0;
        let size =
            (usize::from(header[1]) << 16) | (usize::from(header[2]) << 8) | usize::from(header[3]);
        offset = offset
            .checked_add(4)
            .and_then(|offset| offset.checked_add(size))
            .filter(|&offset| offset <= bytes.len())
            .ok_or_else(|| invalid("truncated FLAC metadata body"))?;
        if last {
            return Ok(offset);
        }
    }
}

// Claxon's public Block omits these header fields. Peek only the actual frame
// format at an exact cursor boundary; CRC, coding and subframe validation stay
// with claxon. No compressed data is rewritten to work around codec limits.
fn check_frame_format(
    bytes: &[u8],
    info: StreamInfo,
    frame_index: u64,
    sample_index: u64,
) -> io::Result<()> {
    let header = bytes
        .get(..4)
        .ok_or_else(|| invalid("truncated FLAC frame header"))?;
    let assignment = header[3] >> 4;
    let channels = match assignment {
        0..=7 => u32::from(assignment) + 1,
        8..=10 => 2,
        _ => return Err(invalid("invalid FLAC channel assignment")),
    };
    let bits = match (header[3] >> 1) & 7 {
        0 => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "FLAC frame bit depth inherited from STREAMINFO is unsupported by claxon",
            ));
        }
        1 => 8,
        2 => 12,
        4 => 16,
        5 => 20,
        6 => 24,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "FLAC frame bit depth is unsupported by claxon",
            ));
        }
    };
    if channels != info.channels || bits != info.bits_per_sample {
        return Err(invalid(
            "FLAC frame channels or bit depth differ from STREAMINFO",
        ));
    }
    let first = *bytes
        .get(4)
        .ok_or_else(|| invalid("truncated FLAC frame time"))?;
    let leading = first.leading_ones() as usize;
    let time_bytes = match leading {
        0 => 1,
        2..=7 => leading,
        _ => return Err(invalid("invalid FLAC frame time")),
    };
    let mut number = u64::from(first & (0x7f >> leading));
    for offset in 1..time_bytes {
        let byte = *bytes
            .get(4 + offset)
            .ok_or_else(|| invalid("truncated FLAC frame time"))?;
        if byte & 0xc0 != 0x80 {
            return Err(invalid("invalid FLAC frame time continuation"));
        }
        number = number
            .checked_mul(64)
            .and_then(|value| value.checked_add(u64::from(byte & 0x3f)))
            .filter(|&value| value < (1u64 << 36))
            .ok_or_else(|| invalid("FLAC frame time overflow"))?;
    }
    let expected = if header[1] & 1 == 0 {
        frame_index
    } else {
        sample_index
    };
    if number != expected {
        return Err(invalid(
            "FLAC frame number or sample position is not contiguous",
        ));
    }
    let rate_code = header[2] & 15;
    let rate = match rate_code {
        0 => info.sample_rate,
        1 => 88_200,
        2 => 176_400,
        3 => 192_000,
        4 => 8_000,
        5 => 16_000,
        6 => 22_050,
        7 => 24_000,
        8 => 32_000,
        9 => 44_100,
        10 => 48_000,
        11 => 96_000,
        12..=14 => {
            let size_bytes = match header[2] >> 4 {
                6 => 1,
                7 => 2,
                _ => 0,
            };
            let offset = 4 + time_bytes + size_bytes;
            if rate_code == 12 {
                u32::from(
                    *bytes
                        .get(offset)
                        .ok_or_else(|| invalid("truncated FLAC frame rate"))?,
                ) * 1000
            } else {
                let pair = bytes
                    .get(offset..offset + 2)
                    .ok_or_else(|| invalid("truncated FLAC frame rate"))?;
                u32::from(u16::from_be_bytes([pair[0], pair[1]]))
                    * if rate_code == 14 { 10 } else { 1 }
            }
        }
        _ => return Err(invalid("invalid FLAC frame sample rate")),
    };
    if rate != info.sample_rate {
        return Err(invalid("FLAC frame sample rate differs from STREAMINFO"));
    }
    Ok(())
}

impl AssetDecoder for FlacDecoder {
    fn decode(
        &self,
        _path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        if encoded.len() > 64 * 1024 * 1024 {
            return Err(invalid("encoded FLAC exceeds 64 MiB").into());
        }
        if !encoded.starts_with(b"fLaC") {
            return Err(invalid("native FLAC signature required").into());
        }
        let reader = FlacReader::new_ext(
            Cursor::new(encoded),
            FlacReaderOptions {
                metadata_only: false,
                read_vorbis_comment: false,
            },
        )?;
        let info = reader.streaminfo();
        drop(reader);
        if !(1..=32).contains(&info.bits_per_sample) || !(1..=8).contains(&info.channels) {
            return Err(invalid("invalid FLAC source bit depth or channels").into());
        }
        let format = AudioFormat::new(info.sample_rate, u16::try_from(info.channels)?)?;
        let max_samples = limits.max_asset_bytes() / std::mem::size_of::<f32>();
        let declared = info
            .samples
            .map(|frames| {
                frames
                    .checked_mul(u64::from(info.channels))
                    .and_then(|samples| usize::try_from(samples).ok())
                    .filter(|&samples| samples <= max_samples)
                    .ok_or_else(|| invalid("declared FLAC PCM exceeds asset limit"))
            })
            .transpose()?;
        let mut samples = Vec::new();
        if let Some(count) = declared {
            samples
                .try_reserve_exact(count)
                .map_err(|_| io::Error::other("FLAC PCM allocation failed"))?;
        }
        let offset = audio_offset(encoded)?;
        let mut cursor = Cursor::new(&encoded[offset..]);
        let mut scratch = Vec::new();
        let mut frame_index = 0u64;
        let magnitude = 1i64 << (info.bits_per_sample - 1);
        while (cursor.position() as usize) < encoded.len() - offset {
            check_frame_format(
                &encoded[offset + cursor.position() as usize..],
                info,
                frame_index,
                u64::try_from(samples.len() / usize::try_from(info.channels)?)?,
            )?;
            let block = FrameReader::new(&mut cursor)
                .read_next_or_eof(scratch)?
                .ok_or_else(|| invalid("truncated FLAC audio frame"))?;
            if block.channels() != info.channels {
                return Err(invalid("FLAC decoded channel count changed").into());
            }
            let added = usize::try_from(block.duration())?
                .checked_mul(usize::try_from(info.channels)?)
                .ok_or_else(|| invalid("FLAC decoded extent overflow"))?;
            let extent = samples
                .len()
                .checked_add(added)
                .filter(|&extent| {
                    extent <= max_samples && declared.is_none_or(|count| extent <= count)
                })
                .ok_or_else(|| invalid("FLAC decoded PCM exceeds declared or asset limit"))?;
            if extent > samples.capacity() {
                let capacity = samples
                    .capacity()
                    .saturating_mul(2)
                    .max(extent)
                    .min(max_samples);
                samples
                    .try_reserve_exact(capacity - samples.len())
                    .map_err(|_| io::Error::other("FLAC PCM allocation failed"))?;
            }
            for frame in 0..block.duration() {
                for channel in 0..info.channels {
                    let signed = i64::from(block.sample(channel, frame));
                    if signed < -magnitude || signed >= magnitude {
                        return Err(invalid("FLAC sample exceeds source bit depth").into());
                    }
                    let value = (signed as f64 / magnitude as f64) as f32;
                    if !value.is_finite() {
                        return Err(invalid("FLAC sample is not finite").into());
                    }
                    samples.push(value);
                }
            }
            scratch = block.into_buffer();
            frame_index = frame_index
                .checked_add(1)
                .ok_or_else(|| invalid("FLAC frame index overflow"))?;
        }
        if declared.is_some_and(|count| samples.len() != count) {
            return Err(invalid("FLAC decoded frame count differs from STREAMINFO").into());
        }
        Ok(PcmSample::new(format, samples, limits)?)
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::flac_fixture::{flac16, refresh_flac16_checksums};
    fn limits(bytes: usize) -> PcmLimits {
        PcmLimits::new(bytes, bytes, 8).unwrap()
    }
    fn decode(bytes: &[u8], cap: usize) -> Result<PcmSample, Box<dyn Error>> {
        FlacDecoder.decode(Path::new("unused.no-extension"), bytes, limits(cap))
    }
    #[test]
    fn original_mono_and_stereo_frames_preserve_source_format_and_amplitude() {
        let mono = decode(&flac16(22_050, 1, &[i16::MIN, 0, i16::MAX], Some(3)), 12).unwrap();
        assert_eq!(mono.format(), AudioFormat::new(22_050, 1).unwrap());
        assert_eq!(mono.frames(), 3);
        assert_eq!(mono.samples(), [-1.0, 0.0, 32767.0 / 32768.0]);
        let stereo = decode(
            &flac16(
                48_000,
                2,
                &[i16::MIN, i16::MAX, 0, 16384, -16384, 0],
                Some(3),
            ),
            24,
        )
        .unwrap();
        assert_eq!(stereo.format(), AudioFormat::new(48_000, 2).unwrap());
        assert_eq!(stereo.frames(), 3);
        assert_eq!(
            stereo.samples(),
            [-1.0, 32767.0 / 32768.0, 0.0, 0.5, -0.5, 0.0]
        );
        for channels in [1, 3, 8] {
            let source: Vec<_> = (0..usize::from(channels) * 256)
                .map(|i| i as i16 - 1024)
                .collect();
            let sample = decode(
                &flac16(44_100, channels, &source, Some(256)),
                source.len() * 4,
            )
            .unwrap();
            assert_eq!(sample.frames(), 256);
            assert_eq!(
                sample.samples(),
                source
                    .iter()
                    .map(|&value| f32::from(value) / 32768.0)
                    .collect::<Vec<_>>()
            );
        }
    }
    #[test]
    fn unknown_counts_are_bounded_known_counts_are_preflighted_and_exact() {
        let unknown = flac16(8000, 1, &[0, 16384], None);
        assert_eq!(decode(&unknown, 8).unwrap().samples(), [0.0, 0.5]);
        assert!(decode(&unknown, 7).is_err());
        for declared in [1, 3, 1u64 << 35] {
            assert!(decode(&flac16(8000, 1, &[0, 16384], Some(declared)), 8).is_err());
        }
        assert!(decode(&flac16(8000, 2, &[1, 2, 3, 4], Some(2)), 15).is_err());
    }
    #[test]
    fn malformed_truncated_and_checksum_errors_never_return_partial_pcm() {
        let original = flac16(44_100, 1, &[1, 2, 3], Some(3));
        for end in [0, 4, 7, 20, 41, 42, 43, original.len() - 1] {
            assert!(decode(&original[..end], 12).is_err(), "cutoff {end}");
        }
        let mut header_crc = original.clone();
        header_crc[48] ^= 1;
        assert!(decode(&header_crc, 12).is_err());
        let mut frame_crc = original.clone();
        let last = frame_crc.len() - 1;
        frame_crc[last] ^= 1;
        assert!(decode(&frame_crc, 12).is_err());
        let mut trailing = original.clone();
        trailing.push(0xff);
        assert!(decode(&trailing, 12).is_err());
        assert!(decode(b"OggSnot-native-FLAC", 12).is_err());
        assert!(decode(&vec![0; 64 * 1024 * 1024 + 1], 12).is_err());
    }
    #[test]
    fn actual_frame_format_conflicts_reject_even_with_valid_fixture_checksums() {
        let original = flac16(48_000, 1, &[0, 16384], Some(2));
        for (index, value, message) in [
            (44, 0x69, "sample rate"),           // Explicit44100 conflictswith48000.
            (45, 0x18, "channels or bit depth"), // Stereo conflictswithmono.
            (45, 0x0c, "channels or bit depth"), // 24bits conflictswith16.
            (45, 0x00, "inherited"),             // Upstream cannot inferbitdepthcode0.
            (45, 0x0e, "unsupported"),           // Upstream doesn'timplement32bitscode7.
        ] {
            let mut encoded = original.clone();
            encoded[index] = value;
            refresh_flac16_checksums(&mut encoded);
            let error = decode(&encoded, 8).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
        }
    }
    #[test]
    fn explicit_frame_rate_extensions_preserve_hertz_with_valid_checksums() {
        for (code, extension) in [
            (12, vec![8]),
            (13, vec![0x1f, 0x40]),
            (14, vec![0x03, 0x20]),
        ] {
            let mut encoded = flac16(8000, 1, &[16384], Some(1));
            encoded[44] = 0x60 | code;
            encoded.splice(48..48, extension);
            refresh_flac16_checksums(&mut encoded);
            let sample = decode(&encoded, 4).unwrap();
            assert_eq!(sample.format().sample_rate(), 8000);
            assert_eq!(sample.samples(), [0.5]);
        }
    }
    #[test]
    fn ignored_comment_payload_is_not_loaded_or_validated_as_tags() {
        let mut encoded = flac16(8000, 1, &[123], Some(1));
        encoded[4] = 0; // STREAMINFO is followed by a final malformed comment.
        encoded.splice(42..42, [0x84, 0, 0, 1, 0xff]);
        assert_eq!(decode(&encoded, 4).unwrap().samples(), [123.0 / 32768.0]);
    }
    #[test]
    fn real_multiframe_chronology_rejects_gaps_duplicates_and_accepts_final_short_block() {
        for variable in [false, true] {
            let mut first = flac16(8000, 1, &[1; 16], Some(17));
            let mut second = flac16(8000, 1, &[2], None);
            if variable {
                first[43] |= 1;
                second[43] |= 1;
            }
            second[46] = if variable { 16 } else { 1 };
            refresh_flac16_checksums(&mut first);
            refresh_flac16_checksums(&mut second);
            let mut encoded = first.clone();
            encoded.extend_from_slice(&second[42..]);
            let decoded = decode(&encoded, 68).unwrap();
            assert_eq!(decoded.frames(), 17);
            assert_eq!(decoded.samples()[16], 2.0 / 32768.0);
            // Fixed frame1 has only one sample: Block.time's currentblocksize
            // arithmetic is deliberately not used to reject this valid tail.
            for number in [0, if variable { 17 } else { 2 }] {
                second[46] = number;
                refresh_flac16_checksums(&mut second);
                let mut invalid = first.clone();
                invalid.extend_from_slice(&second[42..]);
                assert!(
                    decode(&invalid, 68)
                        .unwrap_err()
                        .to_string()
                        .contains("not contiguous")
                );
            }
        }
    }
}
