//! Deferred SDK-gated channel preflight; no control/registry/clock construction.
use super::*;
use beatkernel::audio::AudioFormat;
#[test]
fn actual_channel_preflight_accepts_original_order_and_signed_index_boundary_for_every_supported_count()
 {
    for count in 1..=32u16 {
        let format = AudioFormat::new(48_000, count).unwrap();
        let channels = (0..u32::from(count))
            .map(|i| i32::MAX as u32 - i * 7)
            .collect::<Vec<_>>();
        let original = channels.clone();
        let pointer = channels.as_ptr();
        assert!(validate_channels(&channels, format).is_ok());
        assert_eq!(channels, original);
        assert_eq!(channels.as_ptr(), pointer);
    }
    assert!(validate_channels(&[0], AudioFormat::new(48_000, 1).unwrap()).is_ok());
}
#[test]
fn actual_channel_preflight_refuses_empty_count_mismatch_duplicate_later_row_and_nonrepresentable_index_atomically()
 {
    let format = AudioFormat::new(48_000, 2).unwrap();
    for channels in [
        vec![],
        vec![0],
        vec![0, 1, 2],
        vec![0, 0],
        vec![0, i32::MAX as u32 + 1],
        vec![u32::MAX, 1],
    ] {
        let original = channels.clone();
        let pointer = channels.as_ptr();
        assert!(matches!(
            validate_channels(&channels, format),
            Err(AsioStreamError::InvalidChannels)
        ));
        assert_eq!(channels, original);
        assert_eq!(channels.as_ptr(), pointer);
    }
    let too_many = (0..33u32).collect::<Vec<_>>();
    assert!(matches!(
        validate_channels(&too_many, AudioFormat::new(48_000, 32).unwrap()),
        Err(AsioStreamError::InvalidChannels)
    ));
    let mut late_duplicate = (0..32u32).collect::<Vec<_>>();
    late_duplicate[31] = late_duplicate[0];
    assert!(matches!(
        validate_channels(&late_duplicate, AudioFormat::new(48_000, 32).unwrap()),
        Err(AsioStreamError::InvalidChannels)
    ));
}
