use beatkernel::audio::{AudioError, AudioFormat, PcmLimits, PcmSample};

#[test]
fn fallible_pcm_copy_preserves_exact_finite_bits_and_independent_interleaved_storage() {
    let bits = [
        0x0000_0000,
        0x8000_0000,
        0x0000_0001,
        0x8000_0001,
        0x7f7f_ffff,
        0xff7f_ffff,
        0x3f80_0001,
        0xbf00_0001,
    ];
    let limits = PcmLimits::new(32, 32, 1).unwrap();
    for (rate, channels) in [(44_100, 1), (96_000, 2), (u32::MAX, 4)] {
        let format = AudioFormat::new(rate, channels).unwrap();
        let original = PcmSample::new(format, bits.map(f32::from_bits).to_vec(), limits).unwrap();
        let copied = original.try_clone(limits).unwrap();
        assert_eq!(copied.format(), format);
        assert_eq!(copied.frames(), 8 / usize::from(channels));
        assert_eq!(
            copied
                .samples()
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            bits
        );
        assert_ne!(copied.samples().as_ptr(), original.samples().as_ptr());
        let mut owned = copied.into_samples();
        owned[0] = 7.0;
        owned[1] = 0.0;
        assert_eq!(original.samples()[0].to_bits(), 0);
        assert_eq!(original.samples()[1].to_bits(), 0x8000_0000);
        drop(original);
        assert_eq!(owned[0], 7.0);
        assert_eq!(owned[4].to_bits(), 0x7f7f_ffff);
    }
}

#[test]
fn tighter_asset_limits_refuse_without_consuming_original_and_empty_pcm_stays_valid() {
    let format = AudioFormat::new(22_050, 2).unwrap();
    let original = PcmSample::new(
        format,
        vec![-0.0, 0.25, -2.0, 1.5],
        PcmLimits::new(64, 64, 1).unwrap(),
    )
    .unwrap();
    for capacity in [1, 4, 15] {
        assert!(matches!(
            original.try_clone(PcmLimits::new(capacity, 64, 1).unwrap()),
            Err(AudioError::PcmCapacity)
        ));
        assert_eq!(original.frames(), 2);
        assert_eq!(original.samples()[0].to_bits(), 0x8000_0000);
    }
    let exact = PcmLimits::new(16, 16, 1).unwrap();
    let first = original.try_clone(exact).unwrap();
    let second = original.try_clone(exact).unwrap();
    assert_ne!(first.samples().as_ptr(), second.samples().as_ptr());
    assert_eq!(first.samples(), original.samples());
    assert_eq!(second.samples(), original.samples());
    // A copy has no bank membership: count and aggregate admission remain the bank's job.
    let empty_format = AudioFormat::new(u32::MAX, 32).unwrap();
    let smallest = PcmLimits::new(1, 1, 1).unwrap();
    let empty = PcmSample::new(empty_format, Vec::new(), smallest).unwrap();
    let copied = empty.try_clone(smallest).unwrap();
    assert_eq!(copied.format(), empty_format);
    assert_eq!(copied.frames(), 0);
    assert!(copied.into_samples().is_empty());
    assert!(empty.samples().is_empty());
}
