//! Complete Layer III preparation with explicit declared gapless timing.
use crate::AssetDecoder;
use beatkernel::audio::{AudioFormat, PcmLimits, PcmSample};
use nanomp3::{Decoder, MAX_SAMPLES_PER_FRAME, VbrTag};
use std::{error::Error, io, path::Path};

/// Timing policy for an optional Xing/Info encoder tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mp3TimingPolicy {
    /// Apply declared encoder delay/padding, including decoder delay.
    #[default]
    TaggedGapless,
    /// Keep all following audio frames, skipping only the metadata frame.
    RawFrames,
}
/// Source-rate float decoder. Codec scratch is fixed; output has caller limits.
/// Unwinds become errors in unwind builds; aborts/OOM cannot be caught. CRC
/// protects header/side information only, not all compressed main data.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mp3Decoder;
fn bad(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn unsupported(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Header {
    version: u8,
    rate: u32,
    channels: u16,
    length: usize,
    frames: usize,
    side: usize,
    protected: bool,
}
fn header(bytes: &[u8]) -> io::Result<Header> {
    let h = bytes.get(..4).ok_or_else(|| bad("truncated MPEG header"))?;
    if h[0] != 255 || h[1] & 0xe0 != 0xe0 {
        return Err(bad("MPEG frame synchronization lost"));
    }
    let version = (h[1] >> 3) & 3;
    if version == 1 {
        return Err(bad("reserved MPEG version"));
    }
    if (h[1] >> 1) & 3 != 1 {
        return Err(unsupported("only MPEG Layer III is supported"));
    }
    let bitrate = (h[2] >> 4) as usize;
    if bitrate == 0 {
        return Err(unsupported("free-format MPEG is unsupported"));
    }
    if bitrate == 15 || (h[2] >> 2) & 3 == 3 || h[3] & 3 == 2 {
        return Err(bad("reserved MPEG header field"));
    }
    let rates = [44100, 48000, 32000];
    let rate = rates[((h[2] >> 2) & 3) as usize]
        / match version {
            3 => 1,
            2 => 2,
            _ => 4,
        };
    let kbps = if version == 3 {
        [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ][bitrate]
    } else {
        [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160][bitrate]
    };
    let channels = if h[3] >> 6 == 3 { 1 } else { 2 };
    Ok(Header {
        version,
        rate,
        channels,
        length: ((if version == 3 { 144000 } else { 72000 }) * kbps / rate) as usize
            + usize::from((h[2] >> 1) & 1),
        frames: if version == 3 { 1152 } else { 576 },
        side: if version == 3 {
            if channels == 1 { 17 } else { 32 }
        } else if channels == 1 {
            9
        } else {
            17
        },
        protected: h[1] & 1 == 0,
    })
}
fn check_crc(frame: &[u8], h: Header) -> io::Result<()> {
    if !h.protected {
        return Ok(());
    }
    let end = 6 + h.side;
    let side = frame
        .get(6..end)
        .ok_or_else(|| bad("truncated MPEG side information"))?;
    let mut crc = 0xffffu16;
    for byte in frame[2..4].iter().chain(side) {
        for bit in (0..8).rev() {
            let feedback = ((crc >> 15) as u8) ^ ((byte >> bit) & 1);
            crc <<= 1;
            if feedback != 0 {
                crc ^= 0x8005;
            }
        }
    }
    if crc != u16::from_be_bytes([frame[4], frame[5]]) {
        return Err(bad("MPEG side-information CRC mismatch"));
    }
    Ok(())
}
fn audio_bytes(mut bytes: &[u8]) -> io::Result<&[u8]> {
    if bytes.starts_with(b"ID3") {
        let tag = bytes.get(..10).ok_or_else(|| bad("truncated ID3 header"))?;
        let allowed = match tag[3] {
            3 => 0xe0,
            4 => 0xf0,
            _ => return Err(unsupported("ID3 version unsupported")),
        };
        if tag[4] == 255 || tag[5] & !allowed != 0 || tag[6..10].iter().any(|x| x & 128 != 0) {
            return Err(bad("invalid ID3 header"));
        }
        let size = tag[6..10]
            .iter()
            .fold(0usize, |n, b| (n << 7) | usize::from(*b));
        let footer = tag[3] == 4 && tag[5] & 16 != 0;
        let total = 10 + size + if footer { 10 } else { 0 };
        if total > bytes.len() {
            return Err(bad("truncated ID3 body"));
        }
        if footer {
            let f = &bytes[10 + size..total];
            if &f[..3] != b"3DI" || f[3..] != tag[3..] {
                return Err(bad("invalid ID3 footer"));
            }
        }
        bytes = &bytes[total..];
    }
    if bytes.len() >= 128 && &bytes[bytes.len() - 128..bytes.len() - 125] == b"TAG" {
        bytes = &bytes[..bytes.len() - 128];
    }
    if bytes.starts_with(b"APETAGEX")
        || bytes.len() >= 32 && &bytes[bytes.len() - 32..bytes.len() - 24] == b"APETAGEX"
    {
        return Err(unsupported("APEv2 tags unsupported"));
    }
    Ok(bytes)
}
fn tag(frame: &[u8], h: Header) -> io::Result<Option<VbrTag>> {
    let off = 4 + if h.protected { 2 } else { 0 } + h.side;
    let Some(marker) = frame.get(off..off + 4) else {
        return Ok(None);
    };
    if marker != b"Xing" && marker != b"Info" {
        return Ok(None);
    }
    let flags = frame
        .get(off + 4..off + 8)
        .ok_or_else(|| bad("truncated Xing flags"))?;
    let flags = u32::from_be_bytes(flags.try_into().unwrap());
    if flags & !15 != 0 {
        return Err(bad("unknown Xing flags"));
    }
    let mut cursor = off + 8;
    for (bit, size) in [(1, 4), (2, 4), (4, 100), (8, 4)] {
        if flags & bit != 0 {
            cursor += size;
            if cursor > frame.len() {
                return Err(bad("truncated Xing field"));
            }
        }
    }
    let extension = frame
        .get(cursor..)
        .ok_or_else(|| bad("truncated Xing extension"))?;
    if extension.first().is_some_and(|b| *b != 0) {
        if !extension.starts_with(b"LAME") && !extension.starts_with(b"Lavc") {
            return Err(unsupported("unknown Xing encoder extension"));
        }
        if flags & 1 == 0 || extension.len() < 36 {
            return Err(bad("truncated encoder timing extension"));
        }
    }
    // Public parser supplies the encoder layout; own checks prevent malformed
    // declarations from falling back to an ordinary audio frame.
    std::panic::catch_unwind(|| VbrTag::parse(frame))
        .map_err(|_| bad("MP3 metadata parser unwound"))?
        .map(Some)
        .ok_or_else(|| bad("invalid Xing/Info tag"))
}
impl Mp3Decoder {
    /// Decode complete bytes using an explicit trim policy; never opens `path`.
    pub fn decode_with_timing(
        &self,
        _path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
        policy: Mp3TimingPolicy,
    ) -> Result<PcmSample, Box<dyn Error>> {
        if encoded.len() > 64 * 1024 * 1024 {
            return Err(bad("MP3 encoded input exceeds 64 MiB").into());
        }
        let bytes = audio_bytes(encoded)?;
        let first = header(bytes)?;
        let format = AudioFormat::new(first.rate, first.channels)?;
        let mut offset = 0;
        let mut count = 0usize;
        let mut metadata = None;
        while offset < bytes.len() {
            let h = header(&bytes[offset..])?;
            if h.version != first.version || h.rate != first.rate || h.channels != first.channels {
                return Err(bad("MPEG source format changed").into());
            }
            let frame = bytes
                .get(offset..offset + h.length)
                .ok_or_else(|| bad("truncated MPEG frame"))?;
            check_crc(frame, h)?;
            if offset == 0 {
                metadata = tag(frame, h)?;
            }
            count = count
                .checked_add(1)
                .ok_or_else(|| bad("MP3 frame count overflow"))?;
            offset += h.length;
        }
        let skip = usize::from(metadata.is_some());
        let audio_count = count - skip;
        if let Some(declared) = metadata.and_then(|t| t.frames) {
            if declared as usize != audio_count {
                return Err(bad("Xing audio-frame count mismatch").into());
            }
        }
        let raw = audio_count
            .checked_mul(first.frames)
            .ok_or_else(|| bad("MP3 sample count overflow"))?;
        let (lead, tail) = match (policy, metadata.and_then(|t| t.encoder_delay_padding)) {
            (Mp3TimingPolicy::TaggedGapless, Some((delay, padding))) => {
                if padding < 529 {
                    return Err(unsupported("encoder padding is smaller than decoder delay").into());
                }
                (usize::from(delay) + 529, usize::from(padding) - 529)
            }
            _ => (0, 0),
        };
        let retained = raw
            .checked_sub(lead.checked_add(tail).ok_or_else(|| bad("trim overflow"))?)
            .ok_or_else(|| bad("encoder trim exceeds decoded frames"))?;
        let samples = retained
            .checked_mul(usize::from(first.channels))
            .filter(|n| *n <= limits.max_asset_bytes() / 4)
            .ok_or_else(|| bad("MP3 PCM exceeds asset limit"))?;
        let output =
            std::panic::catch_unwind(|| decode(bytes, first, skip, lead, raw - tail, samples))
                .map_err(|_| bad("MP3 codec unwound"))??;
        Ok(PcmSample::new(format, output, limits)?)
    }
}
impl AssetDecoder for Mp3Decoder {
    fn decode(
        &self,
        path: &Path,
        encoded: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        self.decode_with_timing(path, encoded, limits, Mp3TimingPolicy::TaggedGapless)
    }
}
fn decode(
    bytes: &[u8],
    first: Header,
    skip: usize,
    lead: usize,
    end: usize,
    samples: usize,
) -> io::Result<Vec<f32>> {
    let mut decoder = Decoder::new();
    let mut scratch = [0f32; MAX_SAMPLES_PER_FRAME];
    let mut output = Vec::new();
    output
        .try_reserve_exact(samples)
        .map_err(|_| bad("MP3 PCM allocation failed"))?;
    let mut offset = if skip == 1 { first.length } else { 0 };
    let mut position = 0usize;
    while offset < bytes.len() {
        let h = header(&bytes[offset..])?;
        let (consumed, result) = decoder.decode(&bytes[offset..], &mut scratch);
        let info = result.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if consumed != h.length
            || info.samples_produced != h.frames
            || info.sample_rate != h.rate
            || u16::from(info.channels.num()) != h.channels
            || info.layer != 3
        {
            return Err(bad("MP3 codec frame shape mismatch"));
        }
        let channels = usize::from(h.channels);
        if scratch[..h.frames * channels]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(bad("nonfinite MP3 PCM"));
        }
        let begin = lead.saturating_sub(position).min(h.frames);
        let finish = end.saturating_sub(position).min(h.frames);
        if finish > begin {
            let added = (finish - begin)
                .checked_mul(channels)
                .ok_or_else(|| bad("MP3 retained extent overflow"))?;
            output
                .len()
                .checked_add(added)
                .filter(|&extent| extent <= samples)
                .ok_or_else(|| bad("MP3 retained output exceeds declared extent"))?;
            output.extend_from_slice(&scratch[begin * channels..finish * channels]);
        }
        position = position
            .checked_add(h.frames)
            .ok_or_else(|| bad("MP3 position overflow"))?;
        offset = offset
            .checked_add(consumed)
            .ok_or_else(|| bad("MP3 byte position overflow"))?;
    }
    if output.len() != samples {
        return Err(bad("MP3 retained sample count mismatch"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp3_fixture::{Mp3Version, silence, silence_version, tagged_silence};
    fn pcm(bytes: &[u8], cap: usize, policy: Mp3TimingPolicy) -> Result<PcmSample, Box<dyn Error>> {
        Mp3Decoder.decode_with_timing(
            Path::new("unused.mp3"),
            bytes,
            PcmLimits::new(cap, cap, 4).unwrap(),
            policy,
        )
    }
    #[test]
    fn actual_versions_channels_and_source_pcm() {
        for (version, rate, frames) in [
            (Mp3Version::Mpeg1, 44100, 1152),
            (Mp3Version::Mpeg2, 24000, 576),
            (Mp3Version::Mpeg25, 11025, 576),
        ] {
            for channels in [1, 2] {
                let value = pcm(
                    &silence_version(version, channels, 3),
                    32768,
                    Mp3TimingPolicy::TaggedGapless,
                )
                .unwrap();
                assert_eq!(
                    value.format(),
                    AudioFormat::new(rate, channels as u16).unwrap()
                );
                assert_eq!(value.frames(), frames * 3);
                assert!(value.samples().iter().all(|v| *v == 0.0));
            }
        }
    }
    #[test]
    fn declared_trim_exact_cap_and_raw_policy() {
        let bytes = tagged_silence(1, 4, 576, 1000);
        let value = pcm(&bytes, 2912, Mp3TimingPolicy::TaggedGapless).unwrap();
        assert_eq!(value.frames(), 728);
        assert!(pcm(&bytes, 2908, Mp3TimingPolicy::TaggedGapless).is_err());
        assert_eq!(
            pcm(&bytes, 9216, Mp3TimingPolicy::RawFrames)
                .unwrap()
                .frames(),
            2304
        );
        assert!(
            pcm(
                &tagged_silence(1, 4, 0, 528),
                10000,
                Mp3TimingPolicy::TaggedGapless
            )
            .is_err()
        );
        assert!(
            pcm(
                &tagged_silence(1, 1, 4095, 529),
                10000,
                Mp3TimingPolicy::TaggedGapless
            )
            .is_err()
        );
    }
    #[test]
    fn damaged_tags_and_complete_frame_integrity() {
        let mut bytes = tagged_silence(1, 4, 576, 1000);
        bytes[24] = 5;
        assert!(pcm(&bytes, 16384, Mp3TimingPolicy::RawFrames).is_err());
        let mut bytes = tagged_silence(1, 4, 576, 1000);
        bytes[17] = 128;
        assert!(pcm(&bytes, 16384, Mp3TimingPolicy::RawFrames).is_err());
        let mut bytes = silence(1, 3);
        bytes.pop();
        assert!(pcm(&bytes, 16384, Mp3TimingPolicy::RawFrames).is_err());
        let mut bytes = silence(1, 3);
        bytes.push(0);
        assert!(pcm(&bytes, 16384, Mp3TimingPolicy::RawFrames).is_err());
        let mut bytes = silence(1, 3);
        bytes[192 + 3] = 0;
        assert!(pcm(&bytes, 16384, Mp3TimingPolicy::RawFrames).is_err());
        let mut bytes = silence(1, 3);
        bytes[4] = 128; // nonzero main_data_begin, no reservoir prefix
        assert!(pcm(&bytes, 16384, Mp3TimingPolicy::RawFrames).is_err());
    }
    #[test]
    fn bounded_id3_and_unsupported_headers() {
        let audio = silence(1, 2);
        let mut tagged = b"ID3\x04\x00\x00\x00\x00\x00\x00".to_vec();
        tagged.extend_from_slice(&audio);
        tagged.extend_from_slice(b"TAG");
        tagged.resize(tagged.len() + 125, 0);
        assert_eq!(
            pcm(&tagged, 8192, Mp3TimingPolicy::RawFrames)
                .unwrap()
                .frames(),
            1152
        );
        let mut footer = b"ID3\x04\x00\x10\x00\x00\x00\x00".to_vec();
        footer.extend_from_slice(b"3DI\x04\x00\x10\x00\x00\x00\x00");
        footer.extend_from_slice(&audio);
        assert!(pcm(&footer, 8192, Mp3TimingPolicy::RawFrames).is_ok());
        footer[13] = 3;
        assert!(pcm(&footer, 8192, Mp3TimingPolicy::RawFrames).is_err());
        for index in [1, 2] {
            let mut bytes = audio.clone();
            bytes[index] = if index == 1 { 0xf5 } else { 0 };
            assert!(pcm(&bytes, 8192, Mp3TimingPolicy::RawFrames).is_err());
        }
        assert!(
            pcm(
                b"ID3\x04\x00\x00\x80\x00\x00\x00",
                8192,
                Mp3TimingPolicy::RawFrames
            )
            .is_err()
        );
    }
    #[test]
    fn bitrate_and_padding_can_change_without_changing_format() {
        let mut bytes = silence(1, 1);
        let mut second = vec![0; 241];
        second[..4].copy_from_slice(&[255, 0xf3, 0x92, 0xc0]); // 80kbps + padding
        bytes.extend_from_slice(&second);
        bytes.extend_from_slice(&silence(1, 1));
        assert_eq!(
            pcm(&bytes, 16384, Mp3TimingPolicy::RawFrames)
                .unwrap()
                .frames(),
            1728
        );
    }

    #[test]
    fn protected_side_information_crc_is_checked() {
        let mut bytes = silence(1, 2);
        for frame in bytes.chunks_exact_mut(192) {
            frame[1] &= !1;
            // Zero side information stays zero; bytes 4..6 now carry CRC.
            let mut crc = 0xffffu16;
            for byte in frame[2..4].iter().chain(&frame[6..15]) {
                for bit in (0..8).rev() {
                    let feedback = ((crc >> 15) as u8) ^ ((byte >> bit) & 1);
                    crc <<= 1;
                    if feedback != 0 {
                        crc ^= 0x8005;
                    }
                }
            }
            frame[4..6].copy_from_slice(&crc.to_be_bytes());
        }
        assert_eq!(
            pcm(&bytes, 8192, Mp3TimingPolicy::RawFrames)
                .unwrap()
                .frames(),
            1152
        );
        bytes[6] ^= 1;
        assert!(pcm(&bytes, 8192, Mp3TimingPolicy::RawFrames).is_err());
    }
}
