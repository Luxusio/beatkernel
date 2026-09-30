//! SDK-independent source metadata fixtures, with no driver enumeration.
use beatkernel_platform::audio::asio::{
    validate_clock_sources, AsioClockSource, AsioClockSourceError,
};
use std::error::Error;

fn name(bytes: &[u8]) -> [u8; 32] {
    assert!(bytes.len() < 32);
    let mut storage = [0; 32];
    storage[..bytes.len()].copy_from_slice(bytes);
    storage
}
fn source(index: i32, current: i32) -> AsioClockSource {
    AsioClockSource::from_raw(index, -1, -1, current, name(b"Internal")).unwrap()
}

#[test]
fn internal_and_word_clock_keep_unassociated_identity_and_current_flag() {
    let internal = source(0, 1);
    assert_eq!(internal.index(), 0);
    assert_eq!(internal.associated_channel(), None);
    assert_eq!(internal.associated_group(), None);
    assert!(internal.is_current());
    assert_eq!(internal.name_bytes(), b"Internal");
    let word = AsioClockSource::from_raw(7, -1, -1, 0, name(b"WordClock")).unwrap();
    assert_eq!(word.index(), 7);
    assert_eq!(word.associated_channel(), None);
    assert_eq!(word.associated_group(), None);
    assert!(!word.is_current());
    assert_eq!(word.name_bytes(), b"WordClock");
    assert_eq!(validate_clock_sources(&[internal, word], 2), Ok(()));
}

#[test]
fn external_channel_group_and_signed_native_maximum_are_retained_without_channel_count_guessing() {
    let external = AsioClockSource::from_raw(3, 0, 2, 1, name(b"Digital input")).unwrap();
    assert_eq!(external.associated_channel(), Some(0));
    assert_eq!(external.associated_group(), Some(2));
    let extreme =
        AsioClockSource::from_raw(i32::MAX, i32::MAX, i32::MAX, 0, name(b"External")).unwrap();
    assert_eq!(extreme.index(), 2_147_483_647);
    assert_eq!(extreme.associated_channel(), Some(2_147_483_647));
    assert_eq!(extreme.associated_group(), Some(2_147_483_647));
    assert_eq!(validate_clock_sources(&[external, extreme], 2), Ok(()));
}

#[test]
fn fixed_native_name_extent_preserves_non_utf8_and_requires_a_terminator() {
    let raw = [0xff, 0xfe, b'A', 0x80];
    let non_utf8 = AsioClockSource::from_raw(1, -1, -1, 0, name(&raw)).unwrap();
    assert_eq!(non_utf8.name_bytes(), raw);
    assert!(std::str::from_utf8(non_utf8.name_bytes()).is_err());
    let exact = [b'x'; 31];
    let boundary = AsioClockSource::from_raw(2, -1, -1, 0, name(&exact)).unwrap();
    assert_eq!(boundary.name_bytes(), exact);
    let empty = AsioClockSource::from_raw(3, -1, -1, 0, [0; 32]).unwrap();
    assert!(empty.name_bytes().is_empty());
    let mut tail = [0xab; 32];
    tail[0] = b'A';
    tail[1] = 0;
    assert_eq!(
        AsioClockSource::from_raw(4, -1, -1, 0, tail)
            .unwrap()
            .name_bytes(),
        b"A"
    );
    assert_eq!(
        AsioClockSource::from_raw(5, -1, -1, 0, [b'x'; 32]),
        Err(AsioClockSourceError::MalformedReport)
    );
}

#[test]
fn invalid_signed_indices_current_flags_and_association_pairs_are_malformed() {
    for (index, channel, group, current) in [
        (-1, -1, -1, 0),
        (i32::MIN, -1, -1, 0),
        (0, -1, -1, -1),
        (0, -1, -1, 2),
        (0, -1, -1, i32::MAX),
        (0, -1, 0, 0),
        (0, 0, -1, 0),
        (0, -2, -1, 0),
        (0, -1, -2, 0),
        (0, -2, -2, 0),
        (0, i32::MIN, i32::MIN, 0),
        (0, 0, -2, 1),
        (0, -2, 0, 1),
    ] {
        assert_eq!(
            AsioClockSource::from_raw(index, channel, group, current, name(b"clock")),
            Err(AsioClockSourceError::MalformedReport),
            "({index},{channel},{group},{current})"
        );
    }
}

#[test]
fn collection_limits_are_inclusive_and_checked_before_report_shape() {
    let one = [source(0, 0)];
    assert_eq!(validate_clock_sources(&one, 1), Ok(()));
    assert_eq!(validate_clock_sources(&one, 4096), Ok(()));
    for max_sources in [0, 4097, usize::MAX] {
        assert_eq!(
            validate_clock_sources(&one, max_sources),
            Err(AsioClockSourceError::InvalidLimits)
        );
        assert_eq!(
            validate_clock_sources(&[], max_sources),
            Err(AsioClockSourceError::InvalidLimits)
        );
    }
    assert_eq!(
        validate_clock_sources(&[], 1),
        Err(AsioClockSourceError::MalformedReport)
    );
    assert_eq!(
        validate_clock_sources(&[source(0, 0), source(1, 0)], 1),
        Err(AsioClockSourceError::Capacity)
    );
    let maximum: Vec<_> = (0..4096).map(|index| source(index, 0)).collect();
    assert_eq!(validate_clock_sources(&maximum, 4096), Ok(()));
    let mut overflow = maximum;
    overflow.push(source(4096, 0));
    assert_eq!(
        validate_clock_sources(&overflow, 4096),
        Err(AsioClockSourceError::Capacity)
    );
}

#[test]
fn index_gaps_unsorted_sources_and_no_current_clock_are_valid() {
    let sources = [source(400, 0), source(0, 0), source(37, 0)];
    assert_eq!(validate_clock_sources(&sources, 3), Ok(()));
    assert_eq!(
        sources
            .iter()
            .map(AsioClockSource::index)
            .collect::<Vec<_>>(),
        vec![400, 0, 37]
    );
    assert!(sources.iter().all(|clock| !clock.is_current()));
}

#[test]
fn duplicate_identity_or_multiple_current_sources_are_malformed_even_with_different_metadata() {
    let differently_named = AsioClockSource::from_raw(0, 9, 2, 0, name(b"Different port")).unwrap();
    assert_eq!(
        validate_clock_sources(&[source(0, 0), differently_named], 2),
        Err(AsioClockSourceError::MalformedReport)
    );
    assert_eq!(
        validate_clock_sources(&[source(0, 1), source(7, 1)], 2),
        Err(AsioClockSourceError::MalformedReport)
    );
    assert_eq!(
        validate_clock_sources(&[source(0, 0), source(7, 1)], 2),
        Ok(())
    );
}

#[test]
fn copied_metadata_and_public_errors_retain_identity_and_diagnostic_values() {
    let original = source(19, 1);
    let copied = original;
    assert_eq!(copied, original);
    assert_ne!(copied, source(20, 1));
    fn standard_error(error: &dyn Error) {
        assert!(!error.to_string().is_empty());
    }
    for error in [
        AsioClockSourceError::InvalidLimits,
        AsioClockSourceError::Capacity,
        AsioClockSourceError::MalformedReport,
        AsioClockSourceError::InvalidSelection { index: 19 },
    ] {
        standard_error(&error);
        assert!(!format!("{error:?}").is_empty());
    }
    assert_ne!(
        AsioClockSourceError::InvalidSelection { index: 19 },
        AsioClockSourceError::InvalidSelection { index: 20 }
    );
    assert!(AsioClockSourceError::InvalidSelection { index: 19 }
        .to_string()
        .contains("19"));
}
