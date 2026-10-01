//! Original minimal silent Vorbis packets and Ogg pages, never copied audio.
struct Bits {
    bytes: Vec<u8>,
    count: usize,
}
impl Bits {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            count: 0,
        }
    }
    fn push(&mut self, value: u32, count: usize) {
        for bit in 0..count {
            if self.count % 8 == 0 {
                self.bytes.push(0);
            }
            let last = self.bytes.len() - 1;
            self.bytes[last] |= (((value >> bit) & 1) as u8) << (self.count % 8);
            self.count += 1;
        }
    }
}
fn setup() -> Vec<u8> {
    let mut bits = Bits::new();
    for (value, count) in [
        (0, 8), // One codebook.
        (0x564342, 24),
        (1, 16),
        (1, 24), // Sync, dimension, entries.
        (0, 1),
        (0, 1),
        (0, 5),
        (0, 4), // Unordered nonsparse length1, no lookup.
        (0, 6),
        (0, 16), // One time transform, type0.
        (0, 6),
        (1, 16),
        (0, 5),
        (0, 2),
        (1, 4), // One floor1, no partitions.
        (0, 6),
        (0, 16),
        (0, 24),
        (0, 24),
        (0, 24),
        (0, 6),
        (0, 8),
        (0, 3),
        (0, 1), // Empty residue0.
        (0, 6),
        (0, 16),
        (0, 1),
        (0, 1),
        (0, 2),
        (0, 8),
        (0, 8),
        (0, 8), // One uncoupled mapping0.
        (0, 6),
        (0, 1),
        (0, 16),
        (0, 16),
        (0, 8),
        (1, 1), // One short mode + framing.
    ] {
        bits.push(value, count);
    }
    [b"\x05vorbis".as_slice(), &bits.bytes].concat()
}
fn page(packet: &[u8], flags: u8, granule: u64, sequence: u32) -> Vec<u8> {
    let segments = packet.len() / 255 + 1;
    assert!(segments <= 255);
    let mut bytes = b"OggS".to_vec();
    bytes.extend_from_slice(&[0, flags]);
    bytes.extend_from_slice(&granule.to_le_bytes());
    bytes.extend_from_slice(&0x424b_0001u32.to_le_bytes());
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.push(segments as u8);
    bytes.extend(std::iter::repeat_n(255, segments - 1));
    bytes.push((packet.len() % 255) as u8);
    bytes.extend_from_slice(packet);
    reseal_page(&mut bytes, 0);
    bytes
}
/// Tiny zero-origin stream with 64/64 blocks at24000Hz. Audio packets omit every
/// floor, yielding silence; the first primes overlap and final granule trims it.
pub fn silence(channels: u8, final_frames: u64) -> Vec<u8> {
    assert!((1..=32).contains(&channels));
    assert!(final_frames <= 4096);
    let mut id = b"\x01vorbis".to_vec();
    id.extend_from_slice(&0u32.to_le_bytes());
    id.push(channels);
    id.extend_from_slice(&24000u32.to_le_bytes());
    id.extend_from_slice(&[0; 12]);
    id.extend_from_slice(&[0x66, 1]);
    let vendor = b"BeatKernel original fixture";
    let mut comment = b"\x03vorbis".to_vec();
    comment.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    comment.extend_from_slice(vendor);
    comment.extend_from_slice(&0u32.to_le_bytes());
    comment.push(1);
    let mut bytes = page(&id, 2, 0, 0);
    bytes.extend_from_slice(&page(&comment, 0, 0, 1));
    bytes.extend_from_slice(&page(&setup(), 0, 0, 2));
    let packet = vec![0; (usize::from(channels) + 1).div_ceil(8)];
    let subsequent = final_frames.div_ceil(32);
    for index in 0..=subsequent {
        let final_packet = index == subsequent;
        bytes.extend_from_slice(&page(
            &packet,
            if final_packet { 4 } else { 0 },
            if final_packet {
                final_frames
            } else {
                index * 32
            },
            index as u32 + 3,
        ));
    }
    bytes
}
/// Short-file layout: prime and every audio packet share a single EOS page.
/// Its granule belongs only to the last packet, not to the intermediate ones.
pub fn packed_audio(channels: u8, final_frames: u64) -> Vec<u8> {
    let original = silence(channels, final_frames);
    let offsets = pages(&original);
    let mut eos = page(&[], 4, final_frames, 3);
    eos.truncate(27);
    eos[26] = (offsets.len() - 3) as u8;
    let packets: Vec<_> = offsets[3..]
        .iter()
        .map(|&(offset, size)| {
            let start = offset + 27 + usize::from(original[offset + 26]);
            &original[start..offset + size]
        })
        .collect();
    for packet in &packets {
        eos.push(packet.len() as u8);
    }
    for packet in packets {
        eos.extend_from_slice(packet);
    }
    reseal_page(&mut eos, 0);
    let mut bytes = original[..offsets[3].0].to_vec();
    bytes.extend_from_slice(&eos);
    bytes
}
/// The same original stream with its comment packet crossing a255-byte page
/// boundary. Only the unfinished page has an undefined granule.
pub fn continued_comment(channels: u8, final_frames: u64) -> Vec<u8> {
    let original = silence(channels, final_frames);
    let offsets = pages(&original);
    let mut comment = b"\x03vorbis".to_vec();
    comment.extend_from_slice(&300u32.to_le_bytes());
    comment.extend(std::iter::repeat_n(b'a', 300));
    comment.extend_from_slice(&0u32.to_le_bytes());
    comment.push(1);
    let mut unfinished = page(&comment[..255], 0, u64::MAX, 1);
    // Remove page()'s terminating zero lace for a255-byte complete packet.
    unfinished.remove(28);
    unfinished[26] = 1;
    reseal_page(&mut unfinished, 0);
    let mut bytes = original[..offsets[0].1].to_vec();
    bytes.extend_from_slice(&unfinished);
    bytes.extend_from_slice(&page(&comment[255..], 1, 0, 2));
    for &(offset, size) in &offsets[2..] {
        let mut next = original[offset..offset + size].to_vec();
        let sequence = u32::from_le_bytes(next[18..22].try_into().unwrap()) + 1;
        next[18..22].copy_from_slice(&sequence.to_le_bytes());
        reseal_page(&mut next, 0);
        bytes.extend_from_slice(&next);
    }
    bytes
}
/// Page offset/lengths of this original helper's valid fixture stream.
pub fn pages(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut offset = 0;
    let mut result = Vec::new();
    while offset < bytes.len() {
        let segments = usize::from(bytes[offset + 26]);
        let size = 27
            + segments
            + bytes[offset + 27..offset + 27 + segments]
                .iter()
                .map(|&size| usize::from(size))
                .sum::<usize>();
        result.push((offset, size));
        offset += size;
    }
    result
}
/// Recomputes Ogg's original non-reflected CRC32 after an authored page mutation.
pub fn reseal_page(bytes: &mut [u8], offset: usize) {
    let segments = usize::from(bytes[offset + 26]);
    let size = 27
        + segments
        + bytes[offset + 27..offset + 27 + segments]
            .iter()
            .map(|&size| usize::from(size))
            .sum::<usize>();
    bytes[offset + 22..offset + 26].fill(0);
    let mut crc = 0u32;
    for &byte in &bytes[offset..offset + size] {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x04c1_1db7
            };
        }
    }
    bytes[offset + 22..offset + 26].copy_from_slice(&crc.to_le_bytes());
}
