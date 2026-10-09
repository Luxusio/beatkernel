use crate::{
    play_policy::{ResolvedTimingPolicy, TimingPresetSelection},
    replay_timing_policy::{split_options, wrap_header, PolicyError, PREFIX},
};
use beatkernel::{
    input::codec::CodecLimits,
    replay::{
        codec::{decode_replay, encode_replay, ReplayCodecLimits, ReplayFile},
        ReplayHeader, REPLAY_VERSION,
    },
    time::ClockDomainId,
};
use beatkernel_bms::{
    BmsDefExRank, BmsJudgeDifficulty, BmsRank, BmsRankPrecedence, BmsTimingPreset,
};

fn limits(header_bytes: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        65536,
        128,
        header_bytes,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}
fn header() -> ReplayHeader {
    ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: vec![1, 2],
        rules_identity: vec![3],
        options: b"bms-judge-profile/v1:example".to_vec(),
        seed: 37,
        normalized_clock: ClockDomainId(17),
    }
}
fn policy(difficulty: BmsJudgeDifficulty, precedence: BmsRankPrecedence) -> ResolvedTimingPolicy {
    ResolvedTimingPolicy::from_recorded(
        TimingPresetSelection {
            preset: BmsTimingPreset::BeatorajaSevenKeys8320241dV1,
            precedence,
        },
        difficulty,
    )
    .unwrap()
}
fn rank_policy() -> ResolvedTimingPolicy {
    policy(
        BmsJudgeDifficulty::Rank(BmsRank::Easy),
        BmsRankPrecedence::RankFirst,
    )
}
fn fixed_start(original: &ReplayHeader) -> usize {
    PREFIX.len() + 4 + original.options.len()
}
fn difficulty_tag(original: &ReplayHeader) -> usize {
    fixed_start(original)
        + 1
        + rank_policy().selection().preset.id().len()
        + 1
        + rank_policy().interaction_semantics().len()
        + 1
}
fn windows_start(original: &ReplayHeader, defex: bool) -> usize {
    difficulty_tag(original) + 1 + if defex { 16 } else { 1 } + 16
}

#[test]
fn canonical_all_ranks_and_integer_defex_round_trip_and_keep_source_precedence() {
    for difficulty in [
        BmsJudgeDifficulty::Rank(BmsRank::VeryHard),
        BmsJudgeDifficulty::Rank(BmsRank::Hard),
        BmsJudgeDifficulty::Rank(BmsRank::Normal),
        BmsJudgeDifficulty::Rank(BmsRank::Easy),
        BmsJudgeDifficulty::Rank(BmsRank::VeryEasy),
        BmsJudgeDifficulty::DefExRank(BmsDefExRank::parse("1").unwrap()),
        BmsJudgeDifficulty::DefExRank(BmsDefExRank::parse("100").unwrap()),
        BmsJudgeDifficulty::DefExRank(BmsDefExRank::parse("133").unwrap()),
    ] {
        for precedence in [
            BmsRankPrecedence::RankFirst,
            BmsRankPrecedence::DefExRankFirst,
        ] {
            let selected = policy(difficulty, precedence);
            let original = header();
            let wrapped = wrap_header(original.clone(), Some(&selected), limits(8192)).unwrap();
            let (inner, decoded) = split_options(&wrapped.options).unwrap();
            assert_eq!(inner, original.options);
            assert_eq!(decoded.as_ref(), Some(&selected));
            assert_eq!(decoded.unwrap().profiles().difficulty(), difficulty);
            assert_eq!(
                wrap_header(original, Some(&selected), limits(8192)).unwrap(),
                wrapped
            );
            let file = ReplayFile::new(wrapped, vec![]);
            assert_eq!(
                decode_replay(&encode_replay(&file, limits(8192)).unwrap(), limits(8192)).unwrap(),
                file
            );
        }
    }
    // Same effective table does not erase the selected declaration identity.
    let rank = policy(
        BmsJudgeDifficulty::Rank(BmsRank::Normal),
        BmsRankPrecedence::RankFirst,
    );
    let defex = policy(
        BmsJudgeDifficulty::DefExRank(BmsDefExRank::parse("100").unwrap()),
        BmsRankPrecedence::RankFirst,
    );
    assert_eq!(
        rank.profiles().effective_percentage(),
        defex.profiles().effective_percentage()
    );
    assert_ne!(
        wrap_header(header(), Some(&rank), limits(8192))
            .unwrap()
            .options,
        wrap_header(header(), Some(&defex), limits(8192))
            .unwrap()
            .options
    );
}

#[test]
fn optional_absence_preserves_complete_legacy_file_bytes() {
    let file = ReplayFile::new(header(), vec![]);
    let before = encode_replay(&file, limits(8192)).unwrap();
    let unwrapped = wrap_header(file.header.clone(), None, limits(1)).unwrap();
    assert_eq!(unwrapped, file.header);
    assert_eq!(
        split_options(&unwrapped.options).unwrap(),
        (file.header.options.as_slice(), None)
    );
    assert_eq!(
        encode_replay(&ReplayFile::new(unwrapped, vec![]), limits(8192)).unwrap(),
        before
    );
}

#[test]
fn truncation_trailing_nested_unknown_versions_and_oversized_lengths_are_refused() {
    let original = header();
    let wrapped = wrap_header(original.clone(), Some(&rank_policy()), limits(8192)).unwrap();
    for end in b"bms-timing-setup/".len()..wrapped.options.len() {
        assert!(
            split_options(&wrapped.options[..end]).is_err(),
            "truncation {end}"
        );
    }
    let mut changed = wrapped.options.clone();
    changed.push(0);
    assert!(matches!(
        split_options(&changed),
        Err(PolicyError::Invalid("trailing timing setup bytes"))
    ));
    assert!(matches!(
        wrap_header(wrapped, Some(&rank_policy()), limits(8192)),
        Err(PolicyError::Invalid("nested timing setup"))
    ));
    assert!(matches!(
        split_options(b"bms-timing-setup/v2:any"),
        Err(PolicyError::Invalid("unknown timing setup version"))
    ));
    let wrapped = wrap_header(original.clone(), Some(&rank_policy()), limits(8192)).unwrap();
    let mut changed = wrapped.options.clone();
    changed[PREFIX.len()..PREFIX.len() + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(split_options(&changed).is_err());
    // Replace the inner legacy options with a reserved timing wrapper of equal size.
    let mut changed = wrapped.options.clone();
    changed[PREFIX.len() + 4..PREFIX.len() + 4 + b"bms-timing-setup/".len()]
        .copy_from_slice(b"bms-timing-setup/");
    assert!(matches!(
        split_options(&changed),
        Err(PolicyError::Invalid("nested timing setup"))
    ));
    for offset in [
        fixed_start(&original),
        fixed_start(&original) + 1 + rank_policy().selection().preset.id().len(),
    ] {
        let mut changed = wrapped.options.clone();
        changed[offset] = u8::MAX;
        assert!(split_options(&changed).is_err());
    }
}

#[test]
fn every_effective_window_is_verified_even_when_identity_and_envelope_match() {
    let original = header();
    let wrapped = wrap_header(original.clone(), Some(&rank_policy()), limits(8192)).unwrap();
    // All sixteen entries include grade, hit class, early, late; narrow-window
    // edits keep widest head envelopes unchanged but must still be refused.
    for index in 0..16 {
        for field in [0, 4, 5, 13] {
            let mut changed = wrapped.options.clone();
            changed[windows_start(&original, false) + index * 21 + field] ^= 1;
            assert!(
                matches!(
                    split_options(&changed),
                    Err(PolicyError::Invalid("effective stage window mismatch"))
                ),
                "window {index} field {field}"
            );
        }
    }
    let mut changed = wrapped.options.clone();
    changed[windows_start(&original, false) - 16] ^= 1;
    assert!(matches!(
        split_options(&changed),
        Err(PolicyError::Invalid("effective percentage mismatch"))
    ));
}

#[test]
fn pinned_numerical_and_hold_semantic_versions_and_source_tags_are_required() {
    let original = header();
    let wrapped = wrap_header(original.clone(), Some(&rank_policy()), limits(8192)).unwrap();
    let preset_start = fixed_start(&original) + 1;
    let semantics_start = preset_start + rank_policy().selection().preset.id().len() + 1;
    for (offset, message) in [
        (preset_start, "unknown timing preset version"),
        (semantics_start, "unknown interaction semantics"),
        (difficulty_tag(&original) - 1, "rank precedence tag"),
        (difficulty_tag(&original), "difficulty source tag"),
        (difficulty_tag(&original) + 1, "rank code"),
    ] {
        let mut changed = wrapped.options.clone();
        changed[offset] = u8::MAX;
        assert!(
            matches!(split_options(&changed), Err(PolicyError::Invalid(actual)) if actual == message)
        );
    }
    let selected = policy(
        BmsJudgeDifficulty::DefExRank(BmsDefExRank::parse("100").unwrap()),
        BmsRankPrecedence::DefExRankFirst,
    );
    let wrapped = wrap_header(original.clone(), Some(&selected), limits(8192)).unwrap();
    for numerator in [0, -1, i128::MAX] {
        let mut changed = wrapped.options.clone();
        let start = difficulty_tag(&original) + 1;
        changed[start..start + 16].copy_from_slice(&numerator.to_le_bytes());
        assert!(split_options(&changed).is_err());
    }
}

#[test]
fn exact_header_limit_counts_outer_wrapper_and_all_identity_bytes() {
    let original = header();
    let wrapped = wrap_header(original.clone(), Some(&rank_policy()), limits(8192)).unwrap();
    let total = wrapped.options.len()
        + original.chart_identity.len()
        + original.rules_identity.len()
        + env!("CARGO_PKG_VERSION").len();
    assert!(wrap_header(original.clone(), Some(&rank_policy()), limits(total)).is_ok());
    assert!(matches!(
        wrap_header(original, Some(&rank_policy()), limits(total - 1)),
        Err(PolicyError::HeaderTooLarge)
    ));
}

#[test]
fn timing_is_outermost_and_preserves_actual_judgment_and_gauge_payloads() {
    use crate::{
        gauge::GaugeProfile,
        judgment_policy::{BmsJudgmentPolicy, GradeClass},
    };
    use beatkernel::judge::JudgeGrade;
    use beatkernel_bms::BmsJudgment;
    let gauge = GaugeProfile::new(20_000_000, 0, 0, -30_000_000, true, vec![]).unwrap();
    let judgments = BmsJudgmentPolicy::new(&[GradeClass {
        grade: JudgeGrade(1),
        class: BmsJudgment::PGreat,
    }])
    .unwrap();
    let original = header();
    let gauged =
        crate::replay_gauge_policy::wrap_header(original.clone(), &gauge, limits(8192)).unwrap();
    let classified =
        crate::replay_judgment_policy::wrap_header(gauged, Some(&judgments), limits(8192)).unwrap();
    let wrapped = wrap_header(classified.clone(), Some(&rank_policy()), limits(8192)).unwrap();
    let (inner, timing) = split_options(&wrapped.options).unwrap();
    assert_eq!(inner, classified.options);
    assert_eq!(timing, Some(rank_policy()));
    let (inner, decoded_judgments) = crate::replay_judgment_policy::split_options(inner).unwrap();
    assert_eq!(decoded_judgments, Some(judgments));
    let (inner, decoded_gauge) = crate::replay_gauge_policy::split_options(inner).unwrap();
    assert_eq!(decoded_gauge, gauge);
    assert_eq!(inner, original.options);
}
