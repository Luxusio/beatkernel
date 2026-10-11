//! Cold live timing contracts, with literal original-song/output expectations.
//! No output observation, mixer completion or input authority is manufactured.
use beatkernel::{
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport, TransportError},
};
use beatkernel_bms_runtime::{live_speed::LiveSpeedSetup, replay_speed::ReplaySpeed};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn ns(value: i64) -> Duration {
    Duration::from_nanos(value)
}
fn speed(value: &str) -> ReplaySpeed {
    value.parse().unwrap()
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
    }
}
fn setup(start: i64, preroll: i64, fraction: &str, end: Option<i64>) -> LiveSpeedSetup {
    LiveSpeedSetup::new(ts(start), ns(preroll), speed(fraction), end.map(ts)).unwrap()
}

#[test]
fn setup_is_copy_and_preserves_the_complete_normalized_cold_contract() {
    let selected = setup(50_000_000, 3_000_000, "6/4", Some(54_000_000));
    let copied = selected;
    assert_eq!(copied.start(), ts(50_000_000));
    assert_eq!(copied.preroll(), ns(3_000_000));
    assert_eq!(copied.speed().rate(), Rate::new(3, 2).unwrap());
    assert_eq!(copied.end(), Some(ts(54_000_000)));
    assert_eq!(copied.song_origin(), ts(45_500_000));
    let transport = selected.transport(ts(-17)).unwrap();
    let snapshot = transport.preroll_setup().unwrap();
    assert_eq!(snapshot.logical_origin, ts(-17));
    assert_eq!(snapshot.original_start, ts(50_000_000));
    assert_eq!(snapshot.output_preroll, ns(3_000_000));
    assert_eq!(snapshot.positive_rate, Rate::new(3, 2).unwrap());
    assert_eq!(snapshot.original_end, Some(ts(54_000_000)));
}

#[test]
fn four_rates_use_output_preroll_and_keep_sample_hz_independent() {
    for (fraction, initial, after, output_nanos, frames) in [
        ("1/2", 48_500_000, 52_000_000, 11_000_000, 11),
        ("1/1", 47_000_000, 54_000_000, 7_000_000, 7),
        ("2/1", 44_000_000, 58_000_000, 5_000_000, 5),
        ("3/2", 45_500_000, 56_000_000, 5_666_666, 6),
    ] {
        let unlimited = setup(50_000_000, 3_000_000, fraction, None);
        assert_eq!(unlimited.song_origin(), ts(initial), "{fraction}");
        let transport = unlimited.transport(ts(100)).unwrap();
        assert_eq!(
            transport.position_at(ts(7_000_100)),
            Ok(ts(after)),
            "{fraction}"
        );
        assert_eq!(transport.position_at(ts(3_000_100)), Ok(ts(50_000_000)));
        assert_eq!(
            transport.position_at(ts(99)),
            Err(TransportError::BeforeOrigin)
        );
        assert_eq!(
            unlimited.output_at(point(7, 100), ts(54_000_000)).unwrap(),
            point(7, output_nanos + 100)
        );
        let finite = setup(50_000_000, 3_000_000, fraction, Some(54_000_000));
        assert_eq!(finite.playback_end_frame(1000).unwrap(), Some(frames));
    }
}

#[test]
fn transport_created_by_setup_preserves_single_signed_preroll_truncation() {
    for (preroll, fraction, expected) in [(1, "1/2", [0, 0, 0, 1]), (3, "3/2", [-4, -3, -1, 0])] {
        let selected = setup(0, preroll, fraction, None);
        let transport = selected.transport(ts(70)).unwrap();
        assert_eq!(selected.song_origin(), ts(expected[0]));
        for (elapsed, wanted) in expected.into_iter().enumerate() {
            assert_eq!(
                transport.position_at(ts(70 + elapsed as i64)),
                Ok(ts(wanted))
            );
        }
    }
}

#[test]
fn actual_transport_exact_pause_controls_keep_the_setup_fractional_phase() {
    let mut transport = setup(0, 1, "1/2", Some(9)).transport(ts(0)).unwrap();
    transport.pause_at_output_elapsed(ts(100), 2).unwrap();
    assert_eq!(transport.position_at(ts(109)), Ok(ts(0)));
    transport.resume_preroll(ts(110)).unwrap();
    assert_eq!(transport.position_at(ts(111)), Ok(ts(1)));
    assert_eq!(transport.position_at(ts(3)), Ok(ts(1)));
    let mut negative = setup(0, 3, "3/2", None).transport(ts(0)).unwrap();
    negative.pause_at_output_elapsed(ts(10), 1).unwrap();
    assert_eq!(negative.position_at(ts(19)), Ok(ts(-3)));
    negative.resume_preroll(ts(20)).unwrap();
    assert_eq!(negative.position_at(ts(21)), Ok(ts(-1)));
}

#[test]
fn normal_setup_preserves_legacy_integer_mapping_and_independent_replay_selection() {
    let selected = setup(50_000_000, 3_000_000, "8/8", None);
    let transport = selected.transport(ts(-10)).unwrap();
    let legacy = Transport::new(ts(-10), ts(47_000_000), Rate::NORMAL);
    for value in [-10, 0, 2_999_990, 6_999_990, 604_800_000_000_000] {
        assert_eq!(
            transport.position_at(ts(value)),
            legacy.position_at(ts(value))
        );
    }
    let independent_replay_output = speed("2/1");
    assert_eq!(independent_replay_output.rate(), Rate::new(2, 1).unwrap());
    assert_eq!(selected.speed().rate(), Rate::NORMAL);
    assert_eq!(
        selected.output_at(point(3, 0), ts(54_000_000)).unwrap(),
        point(3, 7_000_000)
    );
}

#[test]
fn inverse_scheduler_retains_domain_and_leaves_finite_command_filtering_to_caller() {
    let selected = setup(-50, 3, "3/2", Some(-46));
    assert_eq!(
        selected.output_at(point(19, -100), ts(-50)).unwrap(),
        point(19, -97)
    );
    assert_eq!(
        selected.output_at(point(19, -100), ts(-46)).unwrap(),
        point(19, -95)
    );
    assert_eq!(
        selected.output_at(point(19, -100), ts(-44)).unwrap(),
        point(19, -93)
    );
    assert!(selected.output_at(point(19, -100), ts(-51)).is_err());
    assert_eq!(selected.end(), Some(ts(-46)));
}

#[test]
fn zero_span_and_subnanosecond_inverse_have_real_zero_output_endpoints() {
    for fraction in ["1/2", "1/1", "2/1", "3/2"] {
        let zero = setup(4, 0, fraction, Some(4));
        assert_eq!(zero.playback_end_frame(48_000).unwrap(), Some(0));
        assert_eq!(
            zero.transport(ts(10)).unwrap().position_at(ts(i64::MAX)),
            Ok(ts(4))
        );
        let preroll = setup(4, 3_000_000, fraction, Some(4));
        assert_eq!(preroll.playback_end_frame(1000).unwrap(), Some(3));
    }
    let quantized = setup(0, 0, "2/1", Some(1));
    assert_eq!(
        quantized.playback_end_frame(1_000_000_000).unwrap(),
        Some(0)
    );
    assert_eq!(
        quantized.output_at(point(7, 9), ts(1)).unwrap(),
        point(7, 9)
    );
    // These numeric endpoints do not grant completion or presentation authority.
    assert_eq!(
        quantized.transport(ts(0)).unwrap().position_at(ts(0)),
        Ok(ts(0))
    );
}

#[test]
fn endpoint_grid_ceils_after_inverse_quantization_at_actual_output_hz() {
    let selected = setup(50_000_000, 3_000_000, "3/2", Some(54_000_000));
    assert_eq!(selected.playback_end_frame(44_100).unwrap(), Some(250));
    assert_eq!(selected.playback_end_frame(48_000).unwrap(), Some(272));
    assert_eq!(
        selected.playback_end_frame(1_000_000_000).unwrap(),
        Some(5_666_666)
    );
    let one = setup(0, 0, "1/1", Some(1));
    assert_eq!(one.playback_end_frame(1).unwrap(), Some(1));
    assert_eq!(one.playback_end_frame(48_000).unwrap(), Some(1));
}

#[test]
fn invalid_cold_preroll_end_and_initial_negative_overflow_refuse() {
    assert!(LiveSpeedSetup::new(ts(0), ns(-1), speed("1/1"), None).is_err());
    assert!(LiveSpeedSetup::new(ts(2), ns(0), speed("1/1"), Some(ts(1))).is_err());
    for end in [None, Some(ts(i64::MAX))] {
        assert!(LiveSpeedSetup::new(ts(i64::MIN), ns(1), speed("1/1"), end).is_err());
        assert!(LiveSpeedSetup::new(ts(i64::MIN), ns(1), speed("2/1"), end).is_err());
    }
    assert_eq!(setup(-100, 7, "3/2", None).song_origin(), ts(-110));
    assert_eq!(
        setup(i64::MAX, i64::MAX, "2/1", None).song_origin(),
        ts(-i64::MAX)
    );
}

#[test]
fn output_hz_is_validated_even_for_unlimited_setup_without_mutating_it() {
    let unlimited = setup(0, 0, "3/2", None);
    let finite = setup(0, 0, "3/2", Some(2));
    for selected in [unlimited, finite] {
        for hz in [0, 1_000_000_001, u32::MAX] {
            assert!(selected.playback_end_frame(hz).is_err(), "Hz {hz}");
        }
        assert_eq!(selected.start(), ts(0));
        assert_eq!(selected.speed().rate(), Rate::new(3, 2).unwrap());
    }
    assert_eq!(unlimited.playback_end_frame(48_000).unwrap(), None);
    assert_eq!(finite.playback_end_frame(1_000_000_000).unwrap(), Some(1));
}

#[test]
fn inverse_wide_delta_and_origin_cancel_before_final_timestamp_narrowing() {
    let full_span = setup(i64::MIN, 0, "1/1", Some(i64::MAX));
    assert_eq!(
        full_span
            .output_at(point(23, i64::MIN), ts(i64::MAX))
            .unwrap(),
        point(23, i64::MAX)
    );
    assert!(full_span.output_at(point(23, 0), ts(i64::MAX)).is_err());
    assert!(full_span.playback_end_frame(1_000_000_000).is_err());
    let max_parts = setup(0, 0, "9223372036854775807/18446744073709551615", None);
    assert_eq!(
        max_parts
            .output_at(point(23, i64::MIN), ts(i64::MAX))
            .unwrap(),
        point(23, i64::MAX)
    );
    assert_eq!(
        max_parts.output_at(point(23, 100), ts(1)).unwrap(),
        point(23, 102)
    );
    assert_eq!(
        max_parts.transport(ts(0)).unwrap().position_at(ts(3)),
        Ok(ts(1))
    );
}

#[test]
fn endpoint_refuses_unrepresentable_elapsed_and_rounded_physical_grid() {
    let exact_max = setup(0, 0, "1/1", Some(i64::MAX));
    assert_eq!(
        exact_max.playback_end_frame(1_000_000_000).unwrap(),
        Some(i64::MAX as u64)
    );
    // A one-Hz ceiling pushes the physical frame timestamp past MAX.
    assert!(exact_max.playback_end_frame(1).is_err());
    let too_long = setup(0, 1, "1/1", Some(i64::MAX));
    assert!(too_long.playback_end_frame(1_000_000_000).is_err());
    let denominator = setup(0, 0, "1/18446744073709551615", Some(2));
    assert!(denominator.playback_end_frame(1_000_000_000).is_err());
    assert!(denominator.output_at(point(9, i64::MIN), ts(2)).is_err());
}

#[test]
fn finite_positive_song_cap_precedes_narrowing_but_uncapped_mapping_refuses() {
    let finite = setup(50, 0, "9223372036854775807/1", Some(70));
    let owner = finite.transport(ts(i64::MIN)).unwrap();
    assert_eq!(owner.position_at(ts(i64::MAX)), Ok(ts(70)));
    let unlimited = setup(50, 0, "2/1", None);
    assert_eq!(
        unlimited
            .transport(ts(0))
            .unwrap()
            .position_at(ts(i64::MAX)),
        Err(TransportError::Overflow)
    );
    assert_eq!(
        unlimited.output_at(point(9, 0), ts(52)).unwrap(),
        point(9, 1)
    );
}

#[test]
fn final_output_offset_overflow_refuses_without_consuming_cold_setup() {
    let selected = setup(9, 3, "1/1", Some(13));
    assert!(selected.output_at(point(42, i64::MAX), ts(9)).is_err());
    assert!(selected.output_at(point(42, 0), ts(8)).is_err());
    assert_eq!(selected.start(), ts(9));
    assert_eq!(selected.preroll(), ns(3));
    assert_eq!(selected.end(), Some(ts(13)));
    assert_eq!(selected.song_origin(), ts(6));
    assert_eq!(
        selected.output_at(point(42, i64::MAX - 7), ts(13)).unwrap(),
        point(42, i64::MAX)
    );
    assert_eq!(selected.playback_end_frame(1_000_000_000).unwrap(), Some(7));
}
