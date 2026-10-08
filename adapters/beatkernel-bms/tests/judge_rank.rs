use beatkernel::{
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    time::Duration,
};
use beatkernel_bms::{
    parse, parse_seeded, BmsDefExRank, BmsErrorKind, BmsInputMode, BmsJudgeDifficulty, BmsRank,
    BmsRankPrecedence, DuplicatePolicy, ParseOptions,
};

#[test]
fn rank_codes_are_exact_and_accept_plus_and_leading_zeros() {
    for (code, expected) in [
        (0, BmsRank::VeryHard),
        (1, BmsRank::Hard),
        (2, BmsRank::Normal),
        (3, BmsRank::Easy),
        (4, BmsRank::VeryEasy),
    ] {
        for raw in [code.to_string(), format!("+000{code}")] {
            let actual = BmsRank::parse(&raw).unwrap();
            assert_eq!(actual, expected, "{raw}");
            assert_eq!(actual.code(), code);
        }
    }
    assert_eq!(
        BmsRank::parse("000000000000000004").unwrap(),
        BmsRank::VeryEasy
    );
}

#[test]
fn invalid_rank_syntax_range_and_precision_refuse_at_line_zero() {
    for raw in [
        "", "+", "5", "99", "-0", "-1", "2.0", "2e0", "NaN", "inf", "++2", "2+", "1 2", "２",
    ] {
        assert_eq!(BmsRank::parse(raw).unwrap_err().line, 0, "{raw}");
    }
    let error = BmsRank::parse("0000000000000000004").unwrap_err();
    assert_eq!(error.line, 0);
    assert!(matches!(error.kind, BmsErrorKind::Limit(_)));
}

#[test]
fn percentages_retain_independent_exact_reduced_fractions() {
    for (raw, numerator, denominator) in [
        ("0", 0, 1),
        ("100", 100, 1),
        ("120.125", 961, 8),
        (".5", 1, 2),
        ("+000120.1250", 961, 8),
        ("100.", 100, 1),
        ("999999999999999999", 999_999_999_999_999_999, 1),
        ("0.00000000000000001", 1, 100_000_000_000_000_000),
    ] {
        let percentage = BmsDefExRank::parse(raw).unwrap();
        assert_eq!(percentage.numerator(), numerator, "{raw}");
        assert_eq!(percentage.denominator(), denominator, "{raw}");
    }
}

#[test]
fn invalid_percentages_and_total_digit_limit_refuse() {
    for raw in [
        "", "+", ".", "-0", "-1", "NaN", "inf", "Infinity", "1e2", "++1", "1.2.3", "1 2", "１２",
    ] {
        assert_eq!(BmsDefExRank::parse(raw).unwrap_err().line, 0, "{raw}");
    }
    for raw in ["1000000000000000000", "0.000000000000000001"] {
        let error = BmsDefExRank::parse(raw).unwrap_err();
        assert_eq!(error.line, 0, "{raw}");
        assert!(matches!(error.kind, BmsErrorKind::Limit(_)), "{error:?}");
    }
}

#[test]
fn parser_preserves_case_insensitive_raw_declaration_and_original_error_line() {
    let chart = parse(
        "#TITLE rank\n#dEfExRaNk +00120.1250",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(chart.metadata["DEFEXRANK"], "+00120.1250");
    let value = chart.judge_rank_metadata().unwrap().defexrank().unwrap();
    assert_eq!((value.numerator(), value.denominator()), (961, 8));
    for raw in ["", "NaN", "-1", "1e2", "0.000000000000000001"] {
        let error = parse(
            &format!("; physical comment\n\n#TITLE rank\n#defexrank {raw}"),
            ParseOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.line, 4, "{raw}");
    }
    // Legacy RANK admission stays descriptive; interpretation is explicitly opt-in.
    let descriptive = parse("#RANK engine-specific", ParseOptions::default()).unwrap();
    assert_eq!(descriptive.metadata["RANK"], "engine-specific");
    assert_eq!(descriptive.judge_rank_metadata().unwrap_err().line, 0);
}

#[test]
fn each_header_obeys_duplicate_policy_and_invalid_later_percent_still_refuses() {
    for text in [
        "#DEFEXRANK 50\n#defexrank +120.1250",
        "#RANK 1\n#rank +0004",
    ] {
        let error = parse(text, ParseOptions::default()).unwrap_err();
        assert_eq!(error.line, 2);
        assert!(matches!(error.kind, BmsErrorKind::Duplicate(_)));
    }
    let options = ParseOptions {
        duplicates: DuplicatePolicy::LastWins,
        ..ParseOptions::default()
    };
    let chart = parse(
        "#RANK 1\n#DEFEXRANK 50\n#rank +0004\n#defexrank +120.1250",
        options,
    )
    .unwrap();
    assert_eq!(chart.metadata["RANK"], "+0004");
    assert_eq!(chart.metadata["DEFEXRANK"], "+120.1250");
    assert_eq!(
        chart.judge_rank_metadata().unwrap().rank(),
        Some(BmsRank::VeryEasy)
    );
    for duplicates in [DuplicatePolicy::Reject, DuplicatePolicy::LastWins] {
        assert!(parse(
            "#DEFEXRANK 100\n#DEFEXRANK NaN",
            ParseOptions {
                duplicates,
                ..ParseOptions::default()
            },
        )
        .is_err());
    }
}

#[test]
fn random_and_switch_interpret_only_selected_payload_with_physical_diagnostics() {
    for text in [
        "#RANDOM 2\n#IF 1\n#DEFEXRANK NaN\n#ELSE\n#DEFEXRANK .5\n#ENDIF\n#ENDRANDOM",
        "#SWITCH 2\n#CASE 1\n#DEFEXRANK NaN\n#SKIP\n#CASE 2\n#DEFEXRANK .5\n#SKIP\n#ENDSW",
    ] {
        let selected = parse_seeded(text, ParseOptions::default(), 0).unwrap();
        assert_eq!(selected.metadata["DEFEXRANK"], ".5");
        assert_eq!(
            parse_seeded(text, ParseOptions::default(), 3)
                .unwrap_err()
                .line,
            3
        );
    }
    for text in [
        "#SETRANDOM 1\n#IF 2\n#DEFEXRANK malformed_payload\n#ENDIF",
        "#SETSWITCH 1\n#CASE 2\n#DEFEXRANK malformed_payload\n#ENDSW",
    ] {
        assert!(parse(text, ParseOptions::default())
            .unwrap()
            .metadata
            .is_empty());
        for options in [
            ParseOptions {
                max_bytes: text.len() - 1,
                ..ParseOptions::default()
            },
            ParseOptions {
                max_lines: 2,
                ..ParseOptions::default()
            },
            ParseOptions {
                max_line_bytes: 20,
                ..ParseOptions::default()
            },
        ] {
            assert!(matches!(
                parse(text, options).unwrap_err().kind,
                BmsErrorKind::Limit(_)
            ));
        }
    }
}

#[test]
fn snapshot_requires_explicit_precedence_falls_back_and_preserves_source_tag() {
    let percentage = BmsDefExRank::parse("120.125").unwrap();
    for (text, rank, defexrank) in [
        ("", None, None),
        ("#RANK 3", Some(BmsRank::Easy), None),
        ("#DEFEXRANK 120.125", None, Some(percentage)),
        (
            "#RANK 3\n#DEFEXRANK 120.125",
            Some(BmsRank::Easy),
            Some(percentage),
        ),
        (
            "#DEFEXRANK 120.125\n#RANK 3",
            Some(BmsRank::Easy),
            Some(percentage),
        ),
    ] {
        let chart = parse(text, ParseOptions::default()).unwrap();
        let before = chart.clone();
        let snapshot = chart.judge_rank_metadata().unwrap();
        assert_eq!(snapshot.rank(), rank);
        assert_eq!(snapshot.defexrank(), defexrank);
        assert_eq!(
            snapshot.resolve(BmsRankPrecedence::RankFirst),
            rank.map(BmsJudgeDifficulty::Rank)
                .or(defexrank.map(BmsJudgeDifficulty::DefExRank))
        );
        assert_eq!(
            snapshot.resolve(BmsRankPrecedence::DefExRankFirst),
            defexrank
                .map(BmsJudgeDifficulty::DefExRank)
                .or(rank.map(BmsJudgeDifficulty::Rank))
        );
        assert_eq!(chart, before);
    }
    let mut chart = parse("#RANK 3\n#DEFEXRANK 120.125", ParseOptions::default()).unwrap();
    let snapshot = chart.judge_rank_metadata().unwrap();
    chart.metadata.clear();
    assert_eq!(snapshot.rank(), Some(BmsRank::Easy));
    assert_eq!(snapshot.defexrank(), Some(percentage));
}

#[test]
fn fabricated_invalid_preferred_or_nonpreferred_metadata_refuses_without_mutation() {
    for (rank, percentage) in [("bad", "100"), ("2", "NaN"), ("bad", "NaN")] {
        let mut chart = parse("#WAV01 note.wav\n#00011:01", ParseOptions::default()).unwrap();
        chart.metadata.insert("RANK".into(), rank.into());
        chart.metadata.insert("DEFEXRANK".into(), percentage.into());
        let before = chart.clone();
        assert_eq!(chart.judge_rank_metadata().unwrap_err().line, 0);
        assert_eq!(chart, before);
    }
}

#[test]
fn metadata_changes_at_fixed_gameplay_lines_preserve_source_compilation_and_rules() {
    let gameplay = "#BPM 120\n#BPM01 240\n#STOP01 48\n#WAV01 head.wav\n#00011:0100\n#00051:0001\n#00151:02\n#00008:0001\n#00009:0001\n#00001:0101\n#00004:0101";
    let baseline = parse(
        &format!("; absent rank\n; absent percent\n{gameplay}"),
        ParseOptions::default(),
    )
    .unwrap();
    let compiled = baseline.compile().unwrap();
    for (rank, percentage) in [("0", "0"), ("4", "120.125"), ("+0002", ".5")] {
        let changed = parse(
            &format!("#RANK {rank}\n#DEFEXRANK {percentage}\n{gameplay}"),
            ParseOptions::default(),
        )
        .unwrap();
        changed.judge_rank_metadata().unwrap();
        assert_eq!(baseline.source, changed.source);
        assert_eq!(baseline.notes, changed.notes);
        assert_eq!(baseline.measures, changed.measures);
        assert_eq!(compiled, changed.compile().unwrap());
        for mode in [BmsInputMode::ButtonOnly, BmsInputMode::ButtonOrContact] {
            let original_rules = baseline.rules_with_input_mode(mode);
            let changed_rules = changed.rules_with_input_mode(mode);
            assert_eq!(original_rules.len(), changed_rules.len());
            for (original, changed) in original_rules.iter().zip(&changed_rules) {
                assert_eq!(original.interaction, changed.interaction);
                assert_eq!(original.control, changed.control);
                assert_eq!(
                    original.evaluator.start_eligibility(),
                    changed.evaluator.start_eligibility()
                );
            }
            let profile = JudgeProfile::new(
                vec![JudgeWindow {
                    grade: JudgeGrade(7),
                    early: Duration::from_nanos(123),
                    late: Duration::from_nanos(456),
                }],
                Duration::from_nanos(17),
            )
            .unwrap();
            let original_engine =
                JudgeEngine::new(compiled.chart.clone(), original_rules, profile.clone()).unwrap();
            let changed_engine =
                JudgeEngine::new(changed.compile().unwrap().chart, changed_rules, profile).unwrap();
            assert_eq!(
                original_engine.stable_hash().unwrap(),
                changed_engine.stable_hash().unwrap()
            );
        }
    }
}
