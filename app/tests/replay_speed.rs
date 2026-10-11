use beatkernel::{
    time::{Duration, Timestamp},
    transport::Rate,
};
use beatkernel_bms_runtime::replay_speed::ReplaySpeed;

fn speed(fraction: &str) -> ReplaySpeed {
    fraction.parse().unwrap()
}

fn song(value: ReplaySpeed, start: i64, preroll: i64, elapsed: i128) -> i64 {
    value
        .song_at_elapsed(
            Timestamp::from_nanos(start),
            Duration::from_nanos(preroll),
            elapsed,
            None,
        )
        .unwrap()
        .as_nanos()
}

#[test]
fn positive_domain_normalizes_parser_constructor_and_display() {
    for (text, numerator, denominator, display) in [
        ("1/2", 1, 2, "1/2"),
        ("1/1", 1, 1, "1/1"),
        ("2/1", 2, 1, "2/1"),
        ("3/2", 3, 2, "3/2"),
        ("6/4", 3, 2, "3/2"),
        ("8/8", 1, 1, "1/1"),
        ("000006/000004", 3, 2, "3/2"),
        (
            "9223372036854775807/18446744073709551615",
            i64::MAX,
            u64::MAX,
            "9223372036854775807/18446744073709551615",
        ),
    ] {
        let parsed = speed(text);
        assert_eq!(parsed.rate(), Rate::new(numerator, denominator).unwrap());
        assert_eq!(parsed.to_string(), display);
        assert_eq!(
            ReplaySpeed::new(Rate::new(numerator, denominator).unwrap())
                .unwrap()
                .to_string(),
            display
        );
    }
    // The Rust/offline grammar has no browser-specific 40-character limit.
    let zeros = "0".repeat(128);
    assert_eq!(speed(&format!("{zeros}6/{zeros}4")).to_string(), "3/2");
    assert_eq!(ReplaySpeed::new(Rate::NORMAL).unwrap().rate(), Rate::NORMAL);
}

#[test]
fn invalid_ascii_fractions_and_nonpositive_core_rates_are_rejected() {
    for text in [
        "",
        "1",
        "/1",
        "1/",
        "1/1/1",
        "0/1",
        "1/0",
        "000/001",
        "001/000",
        "-1/2",
        "1/-2",
        "+1/2",
        "1/+2",
        " 1/2",
        "1/2 ",
        "1 /2",
        "1/ 2",
        "1\n/2",
        "1/2\n",
        "1\t/2",
        "１/2",
        "1/２",
        "١/2",
        "1.0/2",
        "1e1/2",
        "0x1/2",
        "1_000/2",
        "1/2\0",
        "9223372036854775808/1",
        "1/18446744073709551616",
        "18446744073709551614/2",
    ] {
        assert!(text.parse::<ReplaySpeed>().is_err(), "accepted {text:?}");
    }
    for rate in [Rate::ZERO, Rate::REVERSE, Rate::new(i64::MIN, 1).unwrap()] {
        assert!(ReplaySpeed::new(rate).is_err());
    }
}

#[test]
fn four_rates_map_output_preroll_once_and_truncate_toward_zero() {
    // Literal original song positions at output E=0,3,7,8,10,11,12,17.
    // Start=100 and output preroll=7, including odd negative deltas.
    for (fraction, expected) in [
        ("1/2", [97, 98, 100, 100, 101, 102, 102, 105]),
        ("1/1", [93, 96, 100, 101, 103, 104, 105, 110]),
        ("2/1", [86, 92, 100, 102, 106, 108, 110, 120]),
        ("3/2", [90, 94, 100, 101, 104, 106, 107, 115]),
    ] {
        let selected = speed(fraction);
        for (elapsed, wanted) in [0, 3, 7, 8, 10, 11, 12, 17].into_iter().zip(expected) {
            assert_eq!(
                song(selected, 100, 7, elapsed),
                wanted,
                "{fraction} E={elapsed}"
            );
        }
    }
    assert_eq!(song(speed("3/2"), -100, 7, 0), -110);
    assert_eq!(song(speed("3/2"), -100, 7, 17), -85);
    assert_eq!(song(speed("6/4"), 100, 7, 17), 115);
}

#[test]
fn normal_speed_preserves_literal_week_and_twenty_hour_positions() {
    assert_eq!(
        song(speed("1/1"), 0, 100_000_000, 72_000_100_000_000),
        72_000_000_000_000
    );
    assert_eq!(
        song(speed("1/1"), -9, 100_000_000, 604_800_100_000_000),
        604_799_999_999_991
    );
    assert_eq!(
        song(speed("2/1"), 0, 100_000_000, 302_400_100_000_000),
        604_800_000_000_000
    );
    assert_eq!(
        song(speed("1/2"), 0, 100_000_000, 1_209_600_100_000_000),
        604_800_000_000_000
    );
}

#[test]
fn wide_intermediate_preserves_full_signed_span_and_cancellation() {
    assert_eq!(
        song(speed("1/1"), i64::MIN, 0, 18_446_744_073_709_551_615),
        i64::MAX
    );
    assert_eq!(
        song(speed("2/1"), i64::MIN, 0, 9_223_372_036_854_775_807),
        i64::MAX - 1
    );
    assert_eq!(
        song(speed("3/2"), i64::MIN, 0, 12_297_829_382_473_034_410),
        i64::MAX
    );
    assert_eq!(song(speed("2/1"), i64::MAX, i64::MAX, 0), -i64::MAX);
    assert_eq!(
        song(speed("1/2"), i64::MIN, 0, 36_893_488_147_419_103_230),
        i64::MAX
    );
}

#[test]
fn maximal_components_and_large_products_do_not_require_float_or_i64_delta() {
    let almost_half = speed("9223372036854775807/18446744073709551615");
    assert_eq!(
        song(almost_half, 0, 0, 18_446_744_073_709_551_615),
        i64::MAX
    );
    assert_eq!(song(almost_half, 0, 0, 2), 0);
    assert_eq!(song(almost_half, 0, 0, 3), 1);
    assert_eq!(song(almost_half, 0, 3, 0), -1);
    // Multiplying this E by the numerator exceeds i128, but division first
    // yields a representable song delta and finite cap. No float oracle.
    assert_eq!(
        almost_half
            .song_at_elapsed(
                Timestamp::ZERO,
                Duration::from_nanos(0),
                340_282_366_920_938_463_426,
                Some(Timestamp::from_nanos(17)),
            )
            .unwrap(),
        Timestamp::from_nanos(17)
    );
}

#[test]
fn finite_upper_cap_is_applied_before_final_narrowing_without_clamping_prestart() {
    let selected = speed("2/1");
    assert_eq!(
        speed("9223372036854775807/1")
            .song_at_elapsed(
                Timestamp::ZERO,
                Duration::from_nanos(0),
                i128::MAX,
                Some(Timestamp::from_nanos(123)),
            )
            .unwrap(),
        Timestamp::from_nanos(123)
    );
    for (start, elapsed, end, wanted) in [
        (0, 9_223_372_036_854_775_807, 7, 7),
        (i64::MAX - 5, 10, i64::MAX, i64::MAX),
        (-100, 3, -90, -94),
        (-100, 100, -90, -90),
    ] {
        assert_eq!(
            selected
                .song_at_elapsed(
                    Timestamp::from_nanos(start),
                    Duration::from_nanos(0),
                    elapsed,
                    Some(Timestamp::from_nanos(end)),
                )
                .unwrap()
                .as_nanos(),
            wanted
        );
    }
    assert_eq!(
        selected
            .song_at_elapsed(
                Timestamp::from_nanos(100),
                Duration::from_nanos(7),
                0,
                Some(Timestamp::from_nanos(110))
            )
            .unwrap()
            .as_nanos(),
        86
    );
}

#[test]
fn invalid_elapsed_preroll_and_uncapped_or_lower_overflow_remain_errors() {
    let normal = speed("1/1");
    assert!(normal
        .song_at_elapsed(Timestamp::ZERO, Duration::from_nanos(0), -1, None)
        .is_err());
    assert!(normal
        .song_at_elapsed(Timestamp::ZERO, Duration::from_nanos(-1), 0, None)
        .is_err());
    assert!(normal
        .song_at_elapsed(
            Timestamp::from_nanos(i64::MAX),
            Duration::from_nanos(0),
            1,
            None
        )
        .is_err());
    assert!(normal
        .song_at_elapsed(
            Timestamp::from_nanos(i64::MIN),
            Duration::from_nanos(1),
            0,
            Some(Timestamp::from_nanos(i64::MAX))
        )
        .is_err());
    assert!(speed("2/1")
        .song_at_elapsed(
            Timestamp::ZERO,
            Duration::from_nanos(0),
            9_223_372_036_854_775_807,
            None
        )
        .is_err());
    assert!(speed("9223372036854775807/1")
        .song_at_elapsed(Timestamp::ZERO, Duration::from_nanos(0), i128::MAX, None)
        .is_err());
    // Failure does not consume or mutate this Copy value.
    assert_eq!(song(normal, 9, 0, 1), 10);
}
