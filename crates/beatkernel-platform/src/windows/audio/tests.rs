use super::*;

fn decode(wave: &WAVEFORMATEXTENSIBLE) -> Result<DeviceFormat, AudioPlatformError> {
    // SAFETY: This owned full stack structure physically contains WAVEFORMATEX
    // plus its complete 22-byte extension; all fixtures declare cbSize<=22.
    unsafe { read_format(std::ptr::addr_of!(wave.Format)) }
}

#[test]
fn production_waveformat_packing_and_readback_preserve_pcm_widths_and_direct_output() {
    for (container_bits, valid_bits, channels, mask) in [
        (16, 16, 1, None),
        (24, 24, 2, None),
        (32, 32, 2, Some(3)),
        (24, 20, 2, Some(0)),
        (32, 24, 3, Some(7)),
        (16, 1, 1, Some(4)),
    ] {
        let format = DeviceFormat::new(
            12_345,
            channels,
            SampleEncoding::Pcm {
                container_bits,
                valid_bits,
            },
            mask,
        )
        .unwrap();
        let wave = wave_format(format);
        let base = wave.Format;
        assert_eq!(
            (
                base.nSamplesPerSec,
                base.nChannels,
                base.nBlockAlign,
                base.nAvgBytesPerSec,
                base.wBitsPerSample
            ),
            (
                12_345,
                channels,
                channels * (container_bits / 8),
                12_345 * u32::from(channels) * (u32::from(container_bits) / 8),
                container_bits
            )
        );
        let extensible = mask.is_some() || container_bits != valid_bits;
        assert_eq!(
            (base.wFormatTag, base.cbSize),
            (
                if extensible { 0xfffe } else { 1 },
                if extensible { 22 } else { 0 }
            )
        );
        let subformat = wave.SubFormat;
        assert_eq!(subformat, PCM_GUID);
        // SAFETY: PCM's documented active union member is wValidBitsPerSample.
        let actual_valid = unsafe { wave.Samples.wValidBitsPerSample };
        assert_eq!(actual_valid, valid_bits);
        let actual_mask = wave.dwChannelMask;
        assert_eq!(actual_mask, mask.unwrap_or(0));
        assert_eq!(decode(&wave), Ok(format));
    }
}

#[test]
fn production_waveformat_float_guid_and_unspecified_layout_roundtrip() {
    for mask in [None, Some(0), Some(3)] {
        let format = DeviceFormat::new(48_000, 2, SampleEncoding::Float32, mask).unwrap();
        let wave = wave_format(format);
        let base = wave.Format;
        let subformat = wave.SubFormat;
        assert_eq!(
            (base.wFormatTag, base.cbSize),
            (
                if mask.is_some() { 0xfffe } else { 3 },
                if mask.is_some() { 22 } else { 0 }
            )
        );
        assert_eq!(subformat, FLOAT_GUID);
        assert_eq!(decode(&wave), Ok(format));
    }
}

#[test]
fn native_owned_format_readback_rejects_reserved_speaker_assignments_with_matching_counts() {
    for (channels, valid_mask, reserved_mask) in [
        (1, 4, 0x0004_0000),
        (1, 4, 0x8000_0000),
        (2, 3, 0x8000_0001),
    ] {
        let format =
            DeviceFormat::new(48_000, channels, SampleEncoding::Float32, Some(valid_mask)).unwrap();
        let mut wave = wave_format(format);
        assert_eq!(decode(&wave), Ok(format));
        wave.dwChannelMask = reserved_mask;
        // decode uses the complete owned stack allocation and production FFI
        // parser. The mutation preserves channel popcount, isolating bit validity.
        assert_eq!(decode(&wave), Err(AudioPlatformError::InvalidFormat));
    }
}

#[test]
fn native_format_readback_rejects_invalid_full_owned_structures_before_use() {
    let valid =
        wave_format(DeviceFormat::new(48_000, 2, SampleEncoding::Float32, Some(3)).unwrap());
    for change in 0..10 {
        let mut wave = valid;
        match change {
            0 => wave.Format.nChannels = 0,
            1 => wave.Format.nSamplesPerSec = 0,
            2 => wave.Format.nBlockAlign = 1,
            3 => wave.Format.nAvgBytesPerSec = 1,
            4 => wave.Format.cbSize = 0,
            5 => wave.SubFormat = PCM_GUID,
            6 => {
                wave.Samples = WAVEFORMATEXTENSIBLE_0 {
                    wValidBitsPerSample: 31,
                }
            }
            7 => wave.dwChannelMask = 1,
            8 => wave.Format.wBitsPerSample = 16,
            _ => wave.SubFormat = GUID::from_u128(0),
        }
        // A PCM subformat is valid with 32 bits, unlike the other mutations.
        if change == 5 {
            assert_eq!(
                decode(&wave).unwrap().encoding(),
                SampleEncoding::Pcm {
                    container_bits: 32,
                    valid_bits: 32
                }
            );
        } else {
            assert_eq!(decode(&wave), Err(AudioPlatformError::InvalidFormat));
        }
    }
    // SAFETY: Null is explicitly rejected without dereferencing any pointer.
    assert_eq!(
        unsafe { read_format(std::ptr::null()) },
        Err(AudioPlatformError::InvalidFormat)
    );
}
