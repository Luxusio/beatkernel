//! Original tiny native FLAC fixture writer; no external audio assets or tools.
pub(crate) fn flac16(
    rate: u32,
    channels: u8,
    samples: &[i16],
    declared_frames: Option<u64>,
) -> Vec<u8> {
    assert!((1..=8).contains(&channels));
    assert!((1..(1 << 20)).contains(&rate));
    assert_eq!(samples.len() % usize::from(channels), 0);
    let frames = samples.len() / usize::from(channels);
    assert!((1..=256).contains(&frames));
    let declared = declared_frames.unwrap_or(0);
    assert!(declared < (1u64 << 36));
    let mut bytes = b"fLaC\x80\0\0\x22".to_vec(); // Last metadata block, STREAMINFO length34.
    let mut info = [0; 34];
    let block = (frames as u16).max(16);
    info[..2].copy_from_slice(&block.to_be_bytes());
    info[2..4].copy_from_slice(&block.to_be_bytes());
    let packed =
        (u64::from(rate) << 44) | (u64::from(channels - 1) << 41) | (15u64 << 36) | declared;
    info[10..18].copy_from_slice(&packed.to_be_bytes());
    // Zero frame-byte hints and MD5 mean unspecified, not a fake digest.
    bytes.extend_from_slice(&info);
    let mut frame = vec![
        0xff,
        0xf8,
        0x60,
        ((channels - 1) << 4) | 0x08,
        0,
        (frames - 1) as u8,
    ];
    frame.push(crc8(&frame));
    for channel in 0..usize::from(channels) {
        frame.push(0x02); // Verbatim subframe, no wasted bits.
        for sample in samples.iter().skip(channel).step_by(usize::from(channels)) {
            frame.extend_from_slice(&sample.to_be_bytes());
        }
    }
    frame.extend_from_slice(&crc16(&frame).to_be_bytes());
    bytes.extend_from_slice(&frame);
    bytes
}

fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |mut crc, &byte| {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
        crc
    })
}
fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0, |mut crc, &byte| {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
        crc
    })
}

/// Refreshes only this original fixture's single-byte-time header/frame checksums
/// after a format-field edit, so format rejection is independent of CRC failure.
pub(crate) fn refresh_flac16_checksums(bytes: &mut [u8]) {
    let frame = &mut bytes[42..];
    let rate_bytes = match frame[2] & 15 {
        12 => 1,
        13 | 14 => 2,
        _ => 0,
    };
    let header_end = 6 + rate_bytes;
    frame[header_end] = crc8(&frame[..header_end]);
    let end = frame.len() - 2;
    let crc = crc16(&frame[..end]);
    frame[end..].copy_from_slice(&crc.to_be_bytes());
}
