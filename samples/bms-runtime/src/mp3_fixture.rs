//! Original zero-side-information Layer III fixtures, no recorded audio.
/// MPEG versions used by the original fixture generator.
#[derive(Clone, Copy, Debug)]
pub enum Mp3Version {
    Mpeg1,
    Mpeg2,
    Mpeg25,
}
/// Silent MPEG2/24000Hz frames.
pub fn silence(channels: u8, frames: usize) -> Vec<u8> {
    silence_version(Mp3Version::Mpeg2, channels, frames)
}
/// Silent complete frames at 44100, 24000, or 11025Hz respectively.
pub fn silence_version(version: Mp3Version, channels: u8, frames: usize) -> Vec<u8> {
    assert!((1..=2).contains(&channels));
    assert!(frames <= 4096);
    let (b1, b2, length) = match version {
        Mp3Version::Mpeg1 => (0xfb, 0x90, 417),
        Mp3Version::Mpeg2 => (0xf3, 0x84, 192), // 64kbps, rate index 1: 24000Hz.
        Mp3Version::Mpeg25 => (0xe3, 0x80, 417),
    };
    let mut bytes = vec![0; length * frames];
    for frame in bytes.chunks_exact_mut(length) {
        frame[..4].copy_from_slice(&[255, b1, b2, if channels == 1 { 0xc0 } else { 0 }]);
    }
    bytes
}
/// Info/LAME metadata frame followed by original silent MPEG2 audio frames.
/// Frame count excludes the metadata frame, matching nanomp3's public contract.
pub fn tagged_silence(channels: u8, audio_frames: usize, delay: u16, padding: u16) -> Vec<u8> {
    assert!(delay < 4096 && padding < 4096);
    let mut bytes = silence(channels, audio_frames + 1);
    let offset = 4 + if channels == 1 { 9 } else { 17 };
    bytes[offset..offset + 4].copy_from_slice(b"Info");
    bytes[offset + 4..offset + 8].copy_from_slice(&1u32.to_be_bytes());
    bytes[offset + 8..offset + 12].copy_from_slice(&(audio_frames as u32).to_be_bytes());
    let extension = offset + 12;
    bytes[extension..extension + 9].copy_from_slice(b"LAME3.100");
    bytes[extension + 21] = (delay >> 4) as u8;
    bytes[extension + 22] = ((delay & 15) << 4) as u8 | (padding >> 8) as u8;
    bytes[extension + 23] = padding as u8;
    bytes
}
