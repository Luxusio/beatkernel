//! Complete single-stream native Ogg/Vorbis preparation outside callbacks.
use crate::AssetDecoder;
use beatkernel::audio::{AudioFormat, PcmLimits, PcmSample};
use lewton::{
    audio::{PreviousWindowRight, read_audio_packet_generic},
    inside_ogg::read_headers,
    samples::InterleavedSamples,
};
use ogg::PacketReader;
use std::{
    error::Error,
    io::{self, Cursor},
    path::Path,
};

/// Preserves source rate/channels and native floating samples. Encoded input is
/// bounded to64MiB; owned PCM is capped independently. Library packet, comment,
/// setup/codebook and transform allocations are separate, not a RAM/CPU sandbox.
/// Codec unwinds become errors without changing panic hooks; abort/OOM cannot
/// be caught and exhaustive damaged-stream conformance is not established.
#[derive(Clone, Copy, Debug, Default)]
pub struct VorbisDecoder;
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = (index as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000_0000 == 0 {
                value << 1
            } else {
                (value << 1) ^ 0x04c1_1db7
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}
const CRC: [u32; 256] = crc_table();

fn validate_pages(bytes: &[u8]) -> io::Result<u64> {
    if bytes.is_empty() || bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid("Ogg input is empty or exceeds64MiB"));
    }
    let mut offset = 0usize;
    let mut sequence = 0u32;
    let mut serial = None;
    let mut unfinished = false;
    let mut granule = None;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 27)
            .ok_or_else(|| invalid("truncated Ogg page header"))?;
        let flags = header[5];
        if &header[..4] != b"OggS"
            || header[4] != 0
            || flags & !7 != 0
            || (flags & 2 != 0) != (offset == 0)
            || (flags & 1 != 0) != unfinished
        {
            return Err(invalid("invalid Ogg framing, flags or packet continuation"));
        }
        let page_serial = u32::from_le_bytes(header[14..18].try_into().unwrap());
        let page_sequence = u32::from_le_bytes(header[18..22].try_into().unwrap());
        if serial.is_some_and(|old| old != page_serial) || page_sequence != sequence {
            return Err(invalid("multiplexed, chained or nonconsecutive Ogg pages"));
        }
        serial = Some(page_serial);
        sequence = sequence
            .checked_add(1)
            .ok_or_else(|| invalid("Ogg page sequence overflow"))?;
        let count = usize::from(header[26]);
        let lacing = bytes
            .get(offset + 27..offset + 27 + count)
            .ok_or_else(|| invalid("truncated Ogg segment table"))?;
        let payload: usize = lacing.iter().map(|&size| usize::from(size)).sum();
        let end = offset
            .checked_add(27 + count)
            .and_then(|start| start.checked_add(payload))
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| invalid("truncated Ogg page body"))?;
        let mut crc = 0u32;
        for (index, &byte) in bytes[offset..end].iter().enumerate() {
            let value = if (22..26).contains(&index) { 0 } else { byte };
            crc = (crc << 8) ^ CRC[((crc >> 24) as u8 ^ value) as usize];
        }
        if crc != u32::from_le_bytes(header[22..26].try_into().unwrap()) {
            return Err(invalid("Ogg page CRC mismatch"));
        }
        let completed = lacing.iter().any(|&size| size < 255);
        if let Some(&last) = lacing.last() {
            unfinished = last == 255;
        }
        let current = u64::from_le_bytes(header[6..14].try_into().unwrap());
        if completed {
            if current == u64::MAX
                || granule.is_some_and(|old| current < old)
                || (granule.is_none() && current != 0)
            {
                return Err(invalid(
                    "Ogg completed-page granule is undefined, nonzero-origin or regressed",
                ));
            }
            granule = Some(current);
        } else if current != u64::MAX {
            return Err(invalid("Ogg unfinished page must have undefined granule"));
        }
        if flags & 4 != 0 {
            if unfinished || !completed || current == u64::MAX || end != bytes.len() {
                return Err(invalid(
                    "Ogg EOS is incomplete or has trailing/chained data",
                ));
            }
            return Ok(current);
        }
        offset = end;
    }
    Err(invalid("Ogg stream has no final EOS page"))
}

fn identification(bytes: &[u8]) -> Result<AudioFormat, Box<dyn Error>> {
    // The Vorbis mapping puts its fixed identification packet alone on BOS.
    // Reject unsupported source format before setup/codebook allocations.
    if bytes[26] != 1 || bytes[27] != 30 {
        return Err(
            invalid("Vorbis identification must be the sole fixed30-byte BOS packet").into(),
        );
    }
    let id = bytes
        .get(28..58)
        .ok_or_else(|| invalid("truncated Vorbis identification"))?;
    if &id[..7] != b"\x01vorbis"
        || id[7..11] != [0; 4]
        || id[29] != 1
        || !(6..=13).contains(&(id[28] & 15))
        || !(6..=13).contains(&(id[28] >> 4))
        || id[28] & 15 > id[28] >> 4
    {
        return Err(invalid("unsupported codec or invalid Vorbis identification").into());
    }
    Ok(AudioFormat::new(
        u32::from_le_bytes(id[12..16].try_into().unwrap()),
        u16::from(id[11]),
    )?)
}

impl AssetDecoder for VorbisDecoder {
    fn decode(
        &self,
        _path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        let final_frames = validate_pages(encoded)?;
        let format = identification(encoded)?;
        let channels = usize::from(format.channels());
        let max_samples = limits.max_asset_bytes() / std::mem::size_of::<f32>();
        let expected = final_frames
            .checked_mul(u64::try_from(channels)?)
            .and_then(|count| usize::try_from(count).ok())
            .filter(|&count| count <= max_samples)
            .ok_or_else(|| invalid("Ogg final granule exceeds PCM asset limit"))?;
        std::panic::catch_unwind(|| decode_packets(encoded, format, expected, limits))
            .map_err(|_| invalid("Vorbis codec panicked on malformed stream"))?
    }
}

fn decode_packets(
    encoded: &[u8],
    format: AudioFormat,
    expected: usize,
    limits: PcmLimits,
) -> Result<PcmSample, Box<dyn Error>> {
    let channels = usize::from(format.channels());
    let max_samples = limits.max_asset_bytes() / std::mem::size_of::<f32>();
    let mut reader = PacketReader::new(Cursor::new(encoded));
    let ((ident, comments, setup), serial) = read_headers(&mut reader)?;
    if ident.audio_sample_rate != format.sample_rate()
        || usize::from(ident.audio_channels) != channels
    {
        return Err(invalid("Vorbis source format changed after identification").into());
    }
    drop(comments);
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(expected)
        .map_err(|_| io::Error::other("Vorbis PCM allocation failed"))?;
    let mut previous = PreviousWindowRight::new();
    let mut ended = false;
    while let Some(actual) = reader.read_packet()? {
        if ended || actual.stream_serial() != serial {
            return Err(invalid("Vorbis packet follows EOS or changes stream").into());
        }
        let mut packet = read_audio_packet_generic::<InterleavedSamples<f32>>(
            &ident,
            &setup,
            &actual.data,
            &mut previous,
        )?;
        if packet.channel_count != channels
            || packet.samples.len() % channels != 0
            || packet.samples.iter().any(|sample| !sample.is_finite())
        {
            return Err(invalid(
                "Vorbis decoded packet has invalid channel extent or nonfinite PCM",
            )
            .into());
        }
        if actual.last_in_stream() {
            let remaining = expected
                .checked_sub(samples.len())
                .filter(|&count| count <= packet.samples.len())
                .ok_or_else(|| invalid("Vorbis final packet cannot reach EOS granule"))?;
            packet.samples.truncate(remaining);
            ended = true;
        }
        let extent = samples
            .len()
            .checked_add(packet.samples.len())
            .filter(|&count| count <= expected && count <= max_samples)
            .ok_or_else(|| invalid("Vorbis decoded PCM exceeds granule or asset limit"))?;
        if actual.last_in_page() && actual.absgp_page() != (extent / channels) as u64 {
            return Err(
                invalid("Vorbis granule differs from zero-origin decoded frame position").into(),
            );
        }
        samples.extend_from_slice(&packet.samples);
    }
    if !ended || samples.len() != expected {
        return Err(invalid("Vorbis decoded frames differ from final EOS granule").into());
    }
    Ok(PcmSample::new(format, samples, limits)?)
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::vorbis_fixture::{continued_comment, packed_audio, pages, reseal_page, silence};
    fn decode(bytes: &[u8], cap: usize) -> Result<PcmSample, Box<dyn Error>> {
        VorbisDecoder.decode(
            Path::new("not-opened.ogg"),
            bytes,
            PcmLimits::new(cap, cap, 4).unwrap(),
        )
    }
    #[test]
    fn original_silent_float_stream_preserves_channels_rate_and_final_trim() {
        for channels in [1u8, 2, 3, 8, 32] {
            for frames in [0u64, 1, 32, 48, 64, 97] {
                let encoded = silence(channels, frames);
                assert_eq!(validate_pages(&encoded).unwrap(), frames);
                let pcm = decode(
                    &encoded,
                    (frames as usize * usize::from(channels) * 4).max(1),
                )
                .unwrap();
                assert_eq!(
                    pcm.format(),
                    AudioFormat::new(24000, u16::from(channels)).unwrap()
                );
                assert_eq!(pcm.frames(), frames as usize);
                assert_eq!(pcm.samples().len(), frames as usize * usize::from(channels));
                assert!(
                    pcm.samples()
                        .iter()
                        .all(|&value| value == 0.0 && value.is_finite())
                );
            }
        }
    }
    #[test]
    fn prime_and_all_audio_on_same_eos_page_trim_actual_final_packet() {
        for channels in [1u8, 2, 8, 32] {
            for frames in [0u64, 1, 32, 48, 64, 97] {
                let encoded = packed_audio(channels, frames);
                assert_eq!(pages(&encoded).len(), 4);
                assert_eq!(validate_pages(&encoded).unwrap(), frames);
                let cap = (frames as usize * usize::from(channels) * 4).max(1);
                let pcm = decode(&encoded, cap).unwrap();
                assert_eq!(pcm.frames(), frames as usize);
                assert!(pcm.samples().iter().all(|&value| value == 0.0));
                if cap > 1 {
                    assert!(decode(&encoded, cap - 1).is_err());
                }
            }
        }
    }
    #[test]
    fn eos_on_setup_without_actual_audio_packet_rejects_but_empty_prime_audio_is_valid() {
        let original = silence(1, 0);
        let offsets = pages(&original);
        let (setup, size) = offsets[2];
        let mut header_only = original[..setup + size].to_vec();
        header_only[setup + 5] = 4;
        reseal_page(&mut header_only, setup);
        assert_eq!(validate_pages(&header_only).unwrap(), 0);
        assert!(decode(&header_only, 1).is_err());
        assert_eq!(decode(&original, 1).unwrap().frames(), 0);
    }
    #[test]
    fn continued_comment_packet_accepts_only_consistent_flags_and_granules() {
        let original = continued_comment(2, 48);
        assert_eq!(validate_pages(&original).unwrap(), 48);
        assert_eq!(decode(&original, 384).unwrap().frames(), 48);
        let offsets = pages(&original);
        let unfinished = offsets[1].0;
        let continued = offsets[2].0;
        assert_eq!(original[unfinished + 26], 1);
        assert_eq!(original[unfinished + 27], 255);
        assert_eq!(original[continued + 5], 1);
        let mut defined_unfinished = original.clone();
        defined_unfinished[unfinished + 6..unfinished + 14].copy_from_slice(&0u64.to_le_bytes());
        reseal_page(&mut defined_unfinished, unfinished);
        assert!(decode(&defined_unfinished, 384).is_err());
        let mut missing_continuation = original.clone();
        missing_continuation[continued + 5] = 0;
        reseal_page(&mut missing_continuation, continued);
        assert!(decode(&missing_continuation, 384).is_err());
        let mut undefined_completed = original.clone();
        undefined_completed[continued + 6..continued + 14].copy_from_slice(&u64::MAX.to_le_bytes());
        reseal_page(&mut undefined_completed, continued);
        assert!(decode(&undefined_completed, 384).is_err());
    }
    #[test]
    fn final_granule_preflight_and_decoded_extent_are_independently_exact() {
        let original = silence(2, 48);
        assert!(decode(&original, 383).is_err());
        assert_eq!(decode(&original, 384).unwrap().frames(), 48);
        let offsets = pages(&original);
        let final_offset = offsets.last().unwrap().0;
        for granule in [100, u64::MAX - 1] {
            let mut changed = original.clone();
            changed[final_offset + 6..final_offset + 14].copy_from_slice(&granule.to_le_bytes());
            reseal_page(&mut changed, final_offset);
            assert!(decode(&changed, 1024).is_err());
        }
        let mut prime_nonzero = original.clone();
        let prime = offsets[3].0;
        prime_nonzero[prime + 6..prime + 14].copy_from_slice(&7u64.to_le_bytes());
        reseal_page(&mut prime_nonzero, prime);
        assert!(
            decode(&prime_nonzero, 384)
                .unwrap_err()
                .to_string()
                .contains("zero-origin")
        );
    }
    #[test]
    fn crc_truncation_missing_eos_chains_multiplex_sequence_flags_and_granules_reject() {
        let original = silence(1, 48);
        let offsets = pages(&original);
        let final_offset = offsets.last().unwrap().0;
        for end in [0, 4, 26, 27, final_offset, original.len() - 1] {
            assert!(decode(&original[..end], 192).is_err());
        }
        let mut crc = original.clone();
        let last = crc.len() - 1;
        crc[last] ^= 1;
        assert!(decode(&crc, 192).is_err());
        let mut chained = original.clone();
        chained.extend_from_slice(&original);
        assert!(decode(&chained, 192).is_err());
        let mut trailing = original.clone();
        trailing.push(0);
        assert!(decode(&trailing, 192).is_err());
        for (offset, byte_offset, value) in [
            (0, 4, 1),             // Version.
            (0, 5, 0),             // Missing BOS.
            (0, 5, 3),             // Impossible initial continuation.
            (offsets[1].0, 5, 2),  // Repeated BOS.
            (offsets[1].0, 5, 1),  // Unexpected continuation.
            (offsets[1].0, 5, 8),  // Reserved flag.
            (offsets[1].0, 14, 2), // Multiplexed serial.
            (offsets[1].0, 18, 0), // Duplicate sequence.
            (offsets[1].0, 18, 2), // Sequence gap.
            (final_offset, 5, 0),  // Missing EOS.
        ] {
            let mut changed = original.clone();
            changed[offset + byte_offset] = value;
            reseal_page(&mut changed, offset);
            assert!(decode(&changed, 192).is_err());
        }
        let mut undefined = original.clone();
        undefined[final_offset + 6..final_offset + 14].copy_from_slice(&u64::MAX.to_le_bytes());
        reseal_page(&mut undefined, final_offset);
        assert!(decode(&undefined, 192).is_err());
        let mut regressed = original.clone();
        regressed[final_offset + 6..final_offset + 14].copy_from_slice(&31u64.to_le_bytes());
        reseal_page(&mut regressed, final_offset);
        assert!(decode(&regressed, 192).is_err());
        assert!(decode(&vec![0; 64 * 1024 * 1024 + 1], 192).is_err());
    }
    #[test]
    fn crc_valid_codec_and_identification_failures_reject_before_setup_output() {
        let original = silence(1, 48);
        let id = 28;
        for (index, value) in [
            (id, 0x7f),
            (id + 7, 1),
            (id + 11, 0),
            (id + 11, 33),
            (id + 28, 0x55),
            (id + 29, 0),
        ] {
            let mut changed = original.clone();
            changed[index] = value;
            reseal_page(&mut changed, 0);
            assert!(decode(&changed, 192).is_err());
        }
        let mut zero_rate = original.clone();
        zero_rate[id + 12..id + 16].fill(0);
        reseal_page(&mut zero_rate, 0);
        assert!(decode(&zero_rate, 192).is_err());
        let setup = pages(&original)[2].0;
        let mut broken_setup = original.clone();
        broken_setup[setup + 28] = 0;
        reseal_page(&mut broken_setup, setup);
        assert!(decode(&broken_setup, 192).is_err());
    }
}
