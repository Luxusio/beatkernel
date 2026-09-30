use beatkernel::audio::{
    AudioError, AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, WavError,
};

fn limits() -> PcmLimits {
    PcmLimits::new(1024, 4096, 8).unwrap()
}
fn format(rate: u32, channels: u16) -> AudioFormat {
    AudioFormat::new(rate, channels).unwrap()
}
fn fmt(tag: u16, bits: u16, channels: u16, rate: u32) -> Vec<u8> {
    let align = channels * (bits / 8);
    let mut bytes = Vec::new();
    bytes.extend(tag.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * u32::from(align)).to_le_bytes());
    bytes.extend(align.to_le_bytes());
    bytes.extend(bits.to_le_bytes());
    bytes
}
fn extensible(subformat: u32, bits: u16, channels: u16, mask: u32) -> Vec<u8> {
    let mut bytes = fmt(0xfffe, bits, channels, 44_100);
    bytes.extend(22_u16.to_le_bytes());
    bytes.extend(bits.to_le_bytes());
    bytes.extend(mask.to_le_bytes());
    bytes.extend(subformat.to_le_bytes());
    // KSDATAFORMAT_SUBTYPE_PCM/FLOAT: xxxxxxxx-0000-0010-8000-00aa00389b71.
    bytes.extend([
        0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
    ]);
    bytes
}
fn riff(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
    for (id, data) in chunks {
        bytes.extend(*id);
        bytes.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
        bytes.extend(*data);
        if data.len() % 2 != 0 {
            bytes.push(0);
        }
    }
    let size = u32::try_from(bytes.len() - 8).unwrap();
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    bytes
}
fn wav(header: &[u8], data: &[u8]) -> Vec<u8> {
    riff(&[(b"fmt ", header), (b"data", data)])
}
fn assert_pcm(bytes: &[u8], expected_format: AudioFormat, expected: &[f32]) {
    let sample = PcmSample::from_wav(bytes, limits()).unwrap();
    assert_eq!(sample.format(), expected_format);
    assert_eq!(
        sample.frames(),
        expected.len() / usize::from(expected_format.channels())
    );
    assert_eq!(sample.samples(), expected);
}
fn error(bytes: &[u8]) -> WavError {
    match PcmSample::from_wav(bytes, limits()) {
        Err(error) => error,
        Ok(_) => panic!("invalid WAV accepted"),
    }
}

#[test]
fn integer_wav_widths_decode_literal_extrema_and_interleaved_channels() {
    assert_pcm(
        &wav(&fmt(1, 16, 2, 48_000), &[0, 128, 255, 127, 0, 192, 0, 64]),
        format(48_000, 2),
        &[-1.0, 32767.0 / 32768.0, -0.5, 0.5],
    );
    assert_pcm(
        &wav(
            &fmt(1, 24, 1, 44_100),
            &[
                0, 0, 128, 255, 255, 127, 255, 255, 255, 1, 0, 0, 0, 0, 192, 0, 0, 64,
            ],
        ),
        format(44_100, 1),
        &[
            -1.0,
            8388607.0 / 8388608.0,
            -1.0 / 8388608.0,
            1.0 / 8388608.0,
            -0.5,
            0.5,
        ],
    );
    assert_pcm(
        &wav(
            &fmt(1, 32, 1, 96_000),
            &[0, 0, 0, 128, 255, 255, 255, 127, 0, 0, 0, 192, 0, 0, 0, 64],
        ),
        format(96_000, 1),
        &[-1.0, 1.0, -0.5, 0.5],
    );
}

#[test]
fn float_wav_preserves_finite_values_and_signed_zero_without_clipping() {
    let values = [-0.0_f32, 0.25, -2.0, 1.5];
    let data: Vec<_> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let bytes = wav(&fmt(3, 32, 2, 48_000), &data);
    assert_pcm(&bytes, format(48_000, 2), &values);
    let sample = PcmSample::from_wav(&bytes, limits()).unwrap();
    assert_eq!(sample.samples()[0].to_bits(), 0x8000_0000);
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            error(&wav(&fmt(3, 32, 1, 48_000), &value.to_le_bytes())),
            WavError::Audio(AudioError::NonFiniteSample),
        );
    }
}

#[test]
fn extensible_pcm_and_float_accept_known_guids_and_consistent_layouts() {
    assert_pcm(
        &wav(&extensible(1, 24, 2, 3), &[0, 0, 128, 0, 0, 64]),
        format(44_100, 2),
        &[-1.0, 0.5],
    );
    assert_pcm(
        &wav(&extensible(3, 32, 1, 4), &[0, 0, 128, 62]),
        format(44_100, 1),
        &[0.25],
    );
    assert_pcm(
        &wav(&extensible(1, 16, 2, 0), &[0, 128, 0, 64]),
        format(44_100, 2),
        &[-1.0, 0.5],
    );
    let mut twenty_in_twenty_four = extensible(1, 24, 1, 4);
    twenty_in_twenty_four[18..20].copy_from_slice(&20_u16.to_le_bytes());
    assert_pcm(
        &wav(
            &twenty_in_twenty_four,
            &[0, 0, 128, 240, 255, 127, 240, 255, 255, 16, 0, 0],
        ),
        format(44_100, 1),
        &[-1.0, 524287.0 / 524288.0, -1.0 / 524288.0, 1.0 / 524288.0],
    );
    let data = [0, 0, 0, 0];
    let mut unknown_guid = extensible(3, 32, 1, 4);
    unknown_guid[39] ^= 1;
    assert_eq!(
        error(&wav(&unknown_guid, &data)),
        WavError::UnsupportedFormat
    );
    assert_eq!(
        error(&wav(&extensible(99, 32, 1, 4), &data)),
        WavError::UnsupportedFormat
    );
    for offset in [16, 18] {
        let mut invalid = extensible(3, 32, 1, 4);
        invalid[offset..offset + 2].copy_from_slice(&16_u16.to_le_bytes());
        assert_eq!(error(&wav(&invalid, &data)), WavError::Malformed);
    }
    for valid_bits in [0_u16, 25] {
        let mut invalid = extensible(1, 24, 1, 4);
        invalid[18..20].copy_from_slice(&valid_bits.to_le_bytes());
        assert_eq!(error(&wav(&invalid, &[0; 3])), WavError::Malformed);
    }
    assert_eq!(
        error(&wav(&extensible(3, 32, 1, 3), &data)),
        WavError::Malformed
    );
}

#[test]
fn data_before_fmt_unknown_odd_chunk_and_empty_asset_are_valid() {
    let header = fmt(1, 16, 1, 22_050);
    assert_pcm(
        &riff(&[(b"data", &[0, 64]), (b"JUNK", &[7]), (b"fmt ", &header)]),
        format(22_050, 1),
        &[0.5],
    );
    assert_pcm(&wav(&header, &[]), format(22_050, 1), &[]);
    let mut nonzero_pad = riff(&[(b"JUNK", &[7]), (b"data", &[0, 64]), (b"fmt ", &header)]);
    nonzero_pad[21] = 0xab;
    assert_pcm(&nonzero_pad, format(22_050, 1), &[0.5]);
    let mut plain_pcm_with_ignored_cbsize = header;
    plain_pcm_with_ignored_cbsize.extend(123_u16.to_le_bytes());
    assert_pcm(
        &wav(&plain_pcm_with_ignored_cbsize, &[0, 64]),
        format(22_050, 1),
        &[0.5],
    );
}

#[test]
fn riff_headers_declared_sizes_chunk_bounds_and_padding_are_checked() {
    let valid = wav(&fmt(1, 16, 1, 48_000), &[0, 64]);
    for length in [0, 3, 8, 11, 12, 15, 19, valid.len() - 1] {
        assert_eq!(error(&valid[..length]), WavError::Malformed);
    }
    for offset in [0, 8] {
        let mut invalid = valid.clone();
        invalid[offset] ^= 1;
        assert_eq!(error(&invalid), WavError::Malformed);
    }
    for size in [0_u32, 4, u32::MAX] {
        let mut invalid = valid.clone();
        invalid[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(error(&invalid), WavError::Malformed);
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert_eq!(error(&trailing), WavError::Malformed);
    let mut huge_chunk = valid;
    huge_chunk[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(error(&huge_chunk), WavError::Malformed);
    let header = fmt(1, 16, 1, 48_000);
    let mut missing_pad = riff(&[(b"fmt ", &header), (b"data", &[0, 64]), (b"JUNK", &[7])]);
    missing_pad.pop();
    let size = u32::try_from(missing_pad.len() - 8).unwrap();
    missing_pad[4..8].copy_from_slice(&size.to_le_bytes());
    assert_eq!(error(&missing_pad), WavError::Malformed);
}

#[test]
fn required_chunks_are_unique_and_compressed_or_unknown_widths_are_rejected() {
    let header = fmt(1, 16, 1, 48_000);
    assert_eq!(error(&riff(&[(b"fmt ", &header)])), WavError::MissingChunk);
    assert_eq!(error(&riff(&[(b"data", &[0, 0])])), WavError::MissingChunk);
    for chunks in [
        vec![
            (b"fmt ", header.as_slice()),
            (b"fmt ", header.as_slice()),
            (b"data", &[0, 0]),
        ],
        vec![
            (b"fmt ", header.as_slice()),
            (b"data", &[0, 0]),
            (b"data", &[0, 0]),
        ],
    ] {
        assert_eq!(error(&riff(&chunks)), WavError::DuplicateChunk);
    }
    for (tag, bits) in [(2, 16), (6, 16), (1, 8), (3, 16), (3, 64)] {
        assert_eq!(
            error(&wav(&fmt(tag, bits, 1, 48_000), &[0; 8])),
            WavError::UnsupportedFormat
        );
    }
}

#[test]
fn fmt_consistency_frame_alignment_and_decoded_storage_limits_are_checked() {
    let header = fmt(1, 16, 2, 48_000);
    for offset in [2, 4, 8, 12] {
        let mut invalid = header.clone();
        invalid[offset] = 0;
        invalid[offset + 1] = 0;
        assert_eq!(
            error(&wav(&invalid, &[0; 4])),
            if offset == 2 || offset == 4 {
                WavError::Audio(AudioError::InvalidFormat)
            } else {
                WavError::Malformed
            },
        );
    }
    assert_eq!(error(&wav(&header[..15], &[0; 4])), WavError::Malformed);
    assert_eq!(error(&wav(&header, &[0; 2])), WavError::Malformed);
    let tiny = PcmLimits::new(4, 4, 1).unwrap();
    assert!(matches!(
        PcmSample::from_wav(&wav(&fmt(1, 16, 1, 48_000), &[0; 4]), tiny),
        Err(WavError::Audio(AudioError::PcmCapacity))
    ));
    let exact = PcmSample::from_wav(&wav(&fmt(1, 16, 1, 48_000), &[0; 2]), tiny).unwrap();
    assert_eq!(exact.samples(), &[0.0]);
}

#[test]
fn direct_pcm_rejects_partial_frames_nonfinite_samples_and_asset_overflow() {
    assert!(matches!(
        PcmSample::new(format(48_000, 2), vec![0.0], limits()),
        Err(AudioError::InvalidBuffer)
    ));
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(matches!(
            PcmSample::new(format(48_000, 1), vec![value], limits()),
            Err(AudioError::NonFiniteSample)
        ));
    }
    assert!(matches!(
        PcmSample::new(
            format(48_000, 1),
            vec![0.0; 2],
            PcmLimits::new(4, 8, 1).unwrap()
        ),
        Err(AudioError::PcmCapacity)
    ));
    let sample = PcmSample::new(format(12_345, 2), vec![-0.0, 2.0, -1.0, 0.5], limits()).unwrap();
    assert_eq!(sample.format(), format(12_345, 2));
    assert_eq!(sample.frames(), 2);
    assert_eq!(sample.samples(), &[-0.0, 2.0, -1.0, 0.5]);
    assert_eq!(sample.samples()[0].to_bits(), 0x8000_0000);
}

fn mono(samples: Vec<f32>) -> PcmSample {
    PcmSample::new(format(44_100, 1), samples, limits()).unwrap()
}
fn assert_bank_kept(bank: &SampleBank, expected: &[f32]) {
    assert_eq!(bank.len(), 1);
    assert!(!bank.is_empty());
    assert_eq!(bank.total_bytes(), expected.len() * 4);
    assert_eq!(bank.get(SampleId(7)).unwrap().samples(), expected);
    assert!(bank.get(SampleId(8)).is_none());
}

#[test]
fn bank_keeps_source_rate_and_rejects_duplicate_or_channels_without_changes() {
    let mut bank = SampleBank::new(format(48_000, 1), limits()).unwrap();
    assert!(bank.is_empty());
    assert_eq!(bank.len(), 0);
    assert_eq!(bank.total_bytes(), 0);
    assert_eq!(bank.format(), format(48_000, 1));
    bank.insert(SampleId(7), mono(vec![0.25, -0.5])).unwrap();
    assert_eq!(bank.get(SampleId(7)).unwrap().format(), format(44_100, 1));
    assert_eq!(
        bank.insert(SampleId(7), mono(vec![1.0])),
        Err(AudioError::DuplicateSample)
    );
    assert_bank_kept(&bank, &[0.25, -0.5]);
    let stereo = PcmSample::new(format(48_000, 2), vec![0.0, 0.0], limits()).unwrap();
    assert_eq!(
        bank.insert(SampleId(8), stereo),
        Err(AudioError::ChannelMismatch)
    );
    assert_bank_kept(&bank, &[0.25, -0.5]);
}

#[test]
fn bank_count_asset_and_aggregate_limits_reject_atomically_at_exact_boundaries() {
    // Assets constructed with generous limits must still obey bank limits.
    for (bank_limits, existing, rejected) in [
        (PcmLimits::new(8, 16, 1).unwrap(), vec![0.25], vec![]),
        (PcmLimits::new(4, 16, 4).unwrap(), vec![0.25], vec![0.0; 2]),
        (
            PcmLimits::new(8, 8, 4).unwrap(),
            vec![0.25, -0.5],
            vec![0.0],
        ),
    ] {
        let mut bank = SampleBank::new(format(48_000, 1), bank_limits).unwrap();
        bank.insert(SampleId(7), mono(existing.clone())).unwrap();
        assert_eq!(
            bank.insert(SampleId(8), mono(rejected)),
            Err(AudioError::PcmCapacity)
        );
        assert_bank_kept(&bank, &existing);
    }
    let mut bank = SampleBank::new(format(48_000, 1), PcmLimits::new(4, 8, 2).unwrap()).unwrap();
    bank.insert(SampleId(7), mono(vec![0.25])).unwrap();
    bank.insert(SampleId(8), mono(vec![-0.5])).unwrap();
    assert_eq!(bank.len(), 2);
    assert_eq!(bank.total_bytes(), 8);
    assert_eq!(bank.get(SampleId(8)).unwrap().samples(), &[-0.5]);
    let mut empty = SampleBank::new(format(48_000, 1), PcmLimits::new(4, 4, 1).unwrap()).unwrap();
    empty.insert(SampleId(7), mono(vec![])).unwrap();
    assert_eq!(empty.len(), 1);
    assert_eq!(empty.total_bytes(), 0);
    assert_eq!(empty.get(SampleId(7)).unwrap().frames(), 0);
}
