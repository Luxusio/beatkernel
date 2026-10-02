//! Pure report ABI fixtures: no JS values, generated bindings or browser calls.
use super::decode_output;

fn put(words: &mut [u32; 56], field: usize, value: u64) {
    words[field * 2] = value as u32;
    words[field * 2 + 1] = (value >> 32) as u32;
}

fn rendered(start: u64, cursor: u64, frames: u64) -> [u32; 56] {
    let mut words = [0; 56];
    for (field, value) in [
        (0, 1),
        (1, cursor),
        (2, frames),
        (3, cursor),
        (4, frames),
        (8, 1),
        (9, 2),
        (12, cursor + frames),
        (23, 1),
        (24, start + cursor + frames),
        (25, 1),
        (26, start),
    ] {
        put(&mut words, field, value);
    }
    words
}

#[test]
fn exact_wide_fields_partial_blocks_and_unavailable_evidence_are_preserved() {
    let start = 9_007_199_254_740_993;
    let cursor = 0x1_ffff_fff0;
    for (frames, song) in [(1, i64::MIN), (3, -1), (257, i64::MAX), (4096, 0)] {
        let mut words = rendered(start, cursor, frames);
        put(&mut words, 10, song as u64);
        put(&mut words, 13, u64::MAX);
        put(&mut words, 14, u64::MAX - 1);
        put(&mut words, 15, 1 << 63);
        let evidence = decode_output(&words).unwrap();
        assert_eq!(evidence.start, start);
        assert_eq!(evidence.context, Some(start + cursor + frames));
        let report = evidence.report.unwrap();
        assert_eq!(report.start_frame, cursor);
        assert_eq!(report.playback_start_frame, cursor);
        assert_eq!(
            (report.frames, report.playback_frames),
            (frames as usize, frames as usize)
        );
        assert_eq!((report.active_voices, report.pending_commands), (1, 2));
        assert_eq!(report.song_position.as_nanos(), song);
        assert_eq!(report.counters.rendered_frames, cursor + frames);
        assert_eq!(report.counters.commands_consumed, u64::MAX);
        assert_eq!(report.counters.commands_applied, u64::MAX - 1);
        assert_eq!(report.counters.late_commands, 1 << 63);
        assert!(!report.paused && !report.producer_disconnected);
        assert_eq!(report.playback_end_physical_frame, None);
    }
    let mut absent = [0; 56];
    put(&mut absent, 25, 1);
    put(&mut absent, 26, u64::MAX);
    assert_eq!(decode_output(&absent).unwrap().context, None);
    put(&mut absent, 23, 1);
    put(&mut absent, 24, u64::MAX);
    let evidence = decode_output(&absent).unwrap();
    assert_eq!(evidence.start, u64::MAX);
    assert_eq!(evidence.context, Some(u64::MAX));
    assert!(evidence.report.is_none());
}

#[test]
fn flags_modes_capacities_and_context_must_describe_one_normal_output_grid() {
    let valid = rendered(1000, 12, 3);
    assert!(decode_output(&valid[..55]).is_err());
    assert!(decode_output(&[0; 57]).is_err());
    for field in [0, 5, 6, 11, 23, 25, 27] {
        for value in [2, 1 << 32] {
            let mut invalid = valid;
            put(&mut invalid, field, value);
            assert!(decode_output(&invalid).is_err(), "flag {field}={value}");
        }
    }
    for (field, value) in [
        (2, 0),
        (2, 4097),
        (8, 4097),
        (9, 4097),
        (3, 13),
        (4, 2),
        (5, 1),
        (6, 1),
        (7, 15),
        (11, 1),
        (23, 0),
        (24, 1014),
        (24, 1016),
        (25, 0),
        (27, 1),
    ] {
        let mut invalid = valid;
        put(&mut invalid, field, value);
        assert!(decode_output(&invalid).is_err(), "field {field}={value}");
    }
    // Error counters are decoded as actual evidence; shared completion rejects
    // them later. The ABI boundary must not silently clear or narrow them.
    let mut failed = valid;
    for field in 16..=22 {
        put(&mut failed, field, (1 << 63) + field as u64);
    }
    let counters = decode_output(&failed).unwrap().report.unwrap().counters;
    assert_eq!(
        [
            counters.pending_full,
            counters.voice_full,
            counters.unknown_samples,
            counters.unknown_stops,
            counters.invalid_gains,
            counters.invalid_rates,
            counters.invalid_times
        ],
        [16, 17, 18, 19, 20, 21, 22].map(|n| (1 << 63) + n)
    );
}

#[test]
fn unavailable_reports_cannot_smuggle_render_data_and_extent_additions_are_checked() {
    let mut absent = [0; 56];
    put(&mut absent, 25, 1);
    put(&mut absent, 26, 100);
    for field in 1..=22 {
        let mut stray = absent;
        put(&mut stray, field, 1);
        assert!(decode_output(&stray).is_err(), "unavailable field {field}");
    }
    put(&mut absent, 24, 1);
    assert!(
        decode_output(&absent).is_err(),
        "absent context has a nonzero cursor"
    );
    put(&mut absent, 23, 1);
    put(&mut absent, 24, 101);
    assert!(
        decode_output(&absent).is_err(),
        "unavailable output passed its armed start"
    );
    let mut relative_overflow = rendered(0, 0, 1);
    put(&mut relative_overflow, 1, u64::MAX);
    put(&mut relative_overflow, 3, u64::MAX);
    assert!(decode_output(&relative_overflow).is_err());
    let mut absolute_overflow = rendered(0, 0, 1);
    put(&mut absolute_overflow, 26, u64::MAX);
    assert!(decode_output(&absolute_overflow).is_err());
}
