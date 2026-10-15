//! Variable source conditions have an independent canonical wire oracle.
//! These metadata tests do not claim an audio or presentation acknowledgement.
use beatkernel::{
    input::CodecLimits,
    judge::JudgeEngine,
    replay::{
        codec::{encode_replay, ReplayCodecLimits, ReplayFile},
        ReplayHeader,
    },
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms::{BmsChart, BmsInputMode};
use beatkernel_bms_runtime::{
    competition::{compatible_headers, Competition, OpponentKind},
    multiplayer::{competition_identity, competition_identity_for_section},
    play_policy::ResolvedPlayPolicy,
    record_catalog::RecordPreview,
    replay_capture::LiveReplayCapture,
    replay_playback::{
        decode_chart_setup, decode_profile, decode_section_setup, decode_setup,
        reconstruct_section, validate_section_setup,
    },
    replay_speed::ReplaySpeed,
    replay_speed_policy::{
        split_options, split_schedule, wrap_header, wrap_header_with_schedule, SourceRateCut,
        SourceRateSchedule,
    },
};
use std::path::Path;

const V1: &[u8] = b"bms-speed-setup/v1:";
const V2: &[u8] = b"bms-speed-setup/v2:";
type WireCut = (i128, u64, u64, i64, u64);
fn limits() -> ReplayCodecLimits {
    bounded(1 << 20, 4096)
}
fn bounded(file: usize, header: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(file, 100, header, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn speed(text: &str) -> ReplaySpeed {
    text.parse().unwrap()
}
fn source() -> BmsChart {
    beatkernel_bms::parse(
        "#BPM 80\n#WAV01 note.wav\n#00011:01\n#00111:01\n",
        Default::default(),
    )
    .unwrap()
}
fn header(initial: &str) -> ReplayHeader {
    let source = source();
    let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        policy.judge().clone(),
    )
    .unwrap();
    LiveReplayCapture::new_with_policy_and_speed(
        &judge,
        ClockDomainId(1),
        limits(),
        Timestamp::ZERO,
        0,
        None,
        BmsInputMode::ButtonOnly,
        None,
        &policy,
        speed(initial),
    )
    .unwrap()
    .into_file()
    .header
}
fn cut(whole: i128, n: u64, d: u64, rate: &str) -> SourceRateCut {
    SourceRateCut::new(whole, n, d, speed(rate)).unwrap()
}
fn schedule(initial: &str, cuts: &[SourceRateCut]) -> SourceRateSchedule {
    let mut result = SourceRateSchedule::fixed(speed(initial));
    for &cut in cuts {
        result.try_reserve_change().unwrap();
        result.try_push(cut).unwrap();
    }
    result
}
// Deliberately independent from the production envelope encoder.
fn wire(inner: &[u8], initial: (i64, u64), cuts: &[WireCut]) -> Vec<u8> {
    let mut bytes = V2.to_vec();
    bytes.extend_from_slice(&(inner.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&initial.0.to_le_bytes());
    bytes.extend_from_slice(&initial.1.to_le_bytes());
    bytes.extend_from_slice(&(cuts.len() as u32).to_le_bytes());
    for &(whole, n, d, rn, rd) in cuts {
        bytes.extend_from_slice(&whole.to_le_bytes());
        bytes.extend_from_slice(&n.to_le_bytes());
        bytes.extend_from_slice(&d.to_le_bytes());
        bytes.extend_from_slice(&rn.to_le_bytes());
        bytes.extend_from_slice(&rd.to_le_bytes());
    }
    bytes.extend_from_slice(inner);
    bytes
}

#[test]
fn fixed_normal_and_v1_preserve_exact_legacy_options_and_file_bytes() {
    let normal = header("1/1");
    for (text, n, d) in [("1/1", 1i64, 1u64), ("3/2", 3, 2), ("1/2", 1, 2)] {
        let selected = speed(text);
        let fixed = SourceRateSchedule::fixed(selected);
        let old = wrap_header(normal.clone(), selected, limits()).unwrap();
        let new = wrap_header_with_schedule(normal.clone(), &fixed, limits()).unwrap();
        let mut expected = normal.options.clone();
        if text != "1/1" {
            expected = V1.to_vec();
            expected.extend_from_slice(&(normal.options.len() as u32).to_le_bytes());
            expected.extend_from_slice(&n.to_le_bytes());
            expected.extend_from_slice(&d.to_le_bytes());
            expected.extend_from_slice(&normal.options);
        }
        assert_eq!(new.options, expected);
        assert_eq!(new, old);
        assert_eq!(
            encode_replay(&ReplayFile::new(new.clone(), vec![]), limits()).unwrap(),
            encode_replay(&ReplayFile::new(old, vec![]), limits()).unwrap()
        );
        let (inner, decoded) = split_schedule(&new.options).unwrap();
        assert_eq!(inner, normal.options);
        assert_eq!(decoded.initial_speed(), selected);
        assert_eq!(decoded.current_speed(), selected);
        assert!(!decoded.is_variable());
        assert!(decoded.cuts().is_empty());
        assert_eq!(split_options(&new.options).unwrap().1, selected);
    }
    let (inner, _) = split_schedule(&normal.options).unwrap();
    assert_eq!(inner.as_ptr(), normal.options.as_ptr());
}

#[test]
fn actual_changes_returning_to_original_and_equal_phase_cuts_stay_variable() {
    let old = header("1/1");
    let cuts = [
        cut(-1, 1, 2, "3/2"),
        cut(15_114, 2, 3, "1/2"),
        cut(15_114, 2, 3, "1/1"),
    ];
    let s = schedule("1/1", &cuts);
    assert!(s.is_variable());
    assert_eq!(s.current_speed(), speed("1/1"));
    let wrapped = wrap_header_with_schedule(old.clone(), &s, limits()).unwrap();
    let expected = wire(
        &old.options,
        (1, 1),
        &[(-1, 1, 2, 3, 2), (15_114, 2, 3, 1, 2), (15_114, 2, 3, 1, 1)],
    );
    assert_eq!(wrapped.options, expected);
    let (inner, decoded) = split_schedule(&expected).unwrap();
    assert_eq!(inner, old.options);
    assert_eq!(decoded.cuts(), cuts);
    assert_eq!(decoded.initial_speed(), speed("1/1"));
    assert_eq!(
        decode_section_setup(&expected).unwrap().source_speed,
        speed("1/1")
    );
    assert!(split_options(&expected).is_err());
    assert!(decode_profile(&expected).is_err());
    assert!(decode_setup(&expected).is_err());
    assert!(decode_chart_setup(&expected).is_err());
    assert_eq!(cuts[0].whole(), -1);
    assert_eq!(cuts[0].numerator(), 1);
    assert_eq!(cuts[0].denominator(), 2);
    assert_eq!(cuts[0].speed(), speed("3/2"));
}

#[test]
fn schedule_refuses_adjacent_noop_and_backwards_exact_phase_atomically() {
    let mut s = schedule("1/1", &[cut(7, 1, 2, "3/2")]);
    let before = s.cuts().to_vec();
    for invalid in [
        cut(8, 0, 1, "6/4"),
        cut(7, 1, 3, "2/1"),
        cut(6, 9, 10, "2/1"),
    ] {
        s.try_reserve_change().unwrap();
        assert!(s.try_push(invalid).is_err());
        assert_eq!(s.cuts(), before);
        assert_eq!(s.current_speed(), speed("3/2"));
    }
    s.try_reserve_change().unwrap();
    s.try_push(cut(7, 1, 2, "1/1")).unwrap();
    assert_eq!(s.cuts().len(), 2);
}

#[test]
fn cut_fraction_is_proper_canonical_and_handles_extreme_whole_without_float() {
    for (n, d) in [(0, 0), (1, 0), (0, 2), (2, 4), (1, 1), (3, 2)] {
        assert!(
            SourceRateCut::new(0, n, d, speed("3/2")).is_err(),
            "accepted {n}/{d}"
        );
    }
    for whole in [i128::MIN, -1, 0, i128::MAX] {
        let s = schedule("1/1", &[cut(whole, u64::MAX - 1, u64::MAX, "3/2")]);
        let old = header("1/1");
        let wrapped = wrap_header_with_schedule(old, &s, limits()).unwrap();
        assert_eq!(split_schedule(&wrapped.options).unwrap().1.cuts(), s.cuts());
    }
}

#[test]
fn independent_wire_rejects_noncanonical_rates_fractions_order_and_zero_cuts() {
    let inner = header("1/1").options;
    for initial in [(0, 1), (-1, 2), (1, 0), (2, 2), (6, 4)] {
        assert!(split_schedule(&wire(&inner, initial, &[(0, 0, 1, 2, 1)])).is_err());
    }
    for (n, d) in [(0, 1), (-1, 2), (1, 0), (6, 4)] {
        assert!(split_schedule(&wire(&inner, (1, 1), &[(0, 0, 1, n, d)])).is_err());
    }
    for (n, d) in [(0, 0), (1, 0), (0, 2), (2, 4), (1, 1), (9, 8)] {
        assert!(split_schedule(&wire(&inner, (1, 1), &[(0, n, d, 3, 2)])).is_err());
    }
    for cuts in [
        vec![],
        vec![(0, 0, 1, 1, 1)],
        vec![(5, 1, 2, 3, 2), (5, 1, 3, 2, 1)],
        vec![(0, 0, 1, 3, 2), (1, 0, 1, 3, 2)],
    ] {
        let bytes = wire(&inner, (1, 1), &cuts);
        assert!(split_schedule(&bytes).is_err());
        assert!(decode_section_setup(&bytes).is_err());
    }
}

#[test]
fn every_truncation_count_extent_and_trailing_byte_fails_closed() {
    let inner = header("1/1").options;
    let valid = wire(&inner, (1, 1), &[(0, 1, 2, 3, 2), (1, 0, 1, 2, 1)]);
    for len in V2.len()..valid.len() {
        assert!(
            split_schedule(&valid[..len]).is_err(),
            "accepted truncation {len}"
        );
    }
    for (offset, value) in [
        (V2.len(), 0),
        (V2.len(), u32::MAX),
        (V2.len() + 20, 0),
        (V2.len() + 20, 1),
        (V2.len() + 20, u32::MAX),
    ] {
        let mut bad = valid.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            split_schedule(&bad).is_err(),
            "accepted offset {offset} value {value}"
        );
    }
    let mut trailing = valid;
    trailing.push(0);
    assert!(split_schedule(&trailing).is_err());
}

#[test]
fn unknown_versions_nested_envelopes_and_rewrapping_refuse_even_normal_rate() {
    let old = header("1/1");
    let valid = wire(&old.options, (1, 1), &[(0, 0, 1, 3, 2)]);
    let mut unknown = valid.clone();
    unknown[b"bms-speed-setup/v".len()] = b'9';
    let fixed = wrap_header(old.clone(), speed("3/2"), limits())
        .unwrap()
        .options;
    for bad in [
        unknown,
        wire(&valid, (1, 1), &[(1, 0, 1, 2, 1)]),
        wire(&fixed, (1, 1), &[(1, 0, 1, 2, 1)]),
    ] {
        assert!(split_schedule(&bad).is_err());
        assert!(decode_section_setup(&bad).is_err());
    }
    let mut nested = old;
    nested.options = valid;
    assert!(wrap_header(nested.clone(), speed("1/1"), limits()).is_err());
    assert!(
        wrap_header_with_schedule(nested, &SourceRateSchedule::fixed(speed("1/1")), limits())
            .is_err()
    );
}

#[test]
fn exact_header_and_file_budgets_include_entire_variable_envelope() {
    let old = header("1/1");
    let s = schedule("1/1", &[cut(0, 0, 1, "3/2")]);
    let wrapped = wrap_header_with_schedule(old.clone(), &s, limits()).unwrap();
    let h = wrapped.chart_identity.len()
        + wrapped.rules_identity.len()
        + wrapped.options.len()
        + env!("CARGO_PKG_VERSION").len();
    let file = encode_replay(&ReplayFile::new(wrapped.clone(), vec![]), limits())
        .unwrap()
        .len();
    assert!(wrap_header_with_schedule(old.clone(), &s, bounded(file, h)).is_ok());
    assert!(wrap_header_with_schedule(old.clone(), &s, bounded(file - 1, h)).is_err());
    assert!(wrap_header_with_schedule(old, &s, bounded(file, h - 1)).is_err());
    let mut too_large = wrapped;
    too_large.options = vec![0; 4097];
    assert!(
        validate_section_setup(&source(), &ReplayFile::new(too_large, vec![]), limits()).is_err()
    );
}

#[test]
fn variable_is_readable_for_replay_and_history_but_not_saved_or_network_competition() {
    for initial in ["1/1", "3/2"] {
        let raw = header("1/1");
        let changed = if initial == "1/1" { "3/2" } else { "1/1" };
        let s = schedule(initial, &[cut(0, 1, 2, changed)]);
        let effective = wrap_header_with_schedule(raw, &s, limits()).unwrap();
        let file = ReplayFile::new(effective.clone(), vec![]);
        assert_eq!(
            decode_section_setup(&effective.options)
                .unwrap()
                .source_speed,
            speed(initial)
        );
        assert!(validate_section_setup(&source(), &file, limits()).is_ok());
        assert!(reconstruct_section(&source(), file.clone(), limits()).is_ok());
        let preview = RecordPreview::from_file_for_history(
            Path::new("records/variable.bkr"),
            &source(),
            file.clone(),
        )
        .unwrap();
        assert_eq!(preview.records, 0);
        assert!(!compatible_headers(&effective, &effective));
        let mut competition = Competition::new(effective.clone(), 2).unwrap();
        assert!(competition
            .add_replay(
                &source(),
                file.clone(),
                limits(),
                OpponentKind::Own,
                "variable"
            )
            .is_err());
        assert!(competition.opponents().is_empty());
        assert!(competition_identity(&effective, env!("CARGO_PKG_VERSION"), limits()).is_err());
        assert!(competition_identity_for_section(
            &effective,
            env!("CARGO_PKG_VERSION"),
            limits(),
            Some(Timestamp::from_nanos(10))
        )
        .is_err());
    }
    for initial in ["1/1", "3/2"] {
        let fixed = header(initial);
        assert!(compatible_headers(&fixed, &fixed));
        assert!(competition_identity(&fixed, env!("CARGO_PKG_VERSION"), limits()).is_ok());
        assert!(competition_identity_for_section(
            &fixed,
            env!("CARGO_PKG_VERSION"),
            limits(),
            Some(Timestamp::from_nanos(10))
        )
        .is_ok());
    }
}
