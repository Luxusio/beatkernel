use beatkernel_bms::{BmsErrorKind, DuplicatePolicy, ParseOptions, parse, parse_seeded};
fn title(text: &str, seed: u64) -> String {
    parse_seeded(text, ParseOptions::default(), seed)
        .unwrap()
        .metadata["TITLE"]
        .clone()
}
#[test]
fn literal_splitmix_seed_and_default_sequence() {
    // seed0 draws e220a8397b1dcdaf, 6e789e6aa1b965f4, 06c45d188009454f.
    let first = "#RANDOM 2\n#IF 1\n#TITLE one\n#ELSE\n#TITLE two\n#ENDIF";
    assert_eq!(title(first, 0), "two");
    assert_eq!(title(first, 3), "one"); // 1d0b14e4db018fed
    assert_eq!(
        parse(first, ParseOptions::default()).unwrap().metadata["TITLE"],
        "two"
    );
    let sequential = "#RANDOM 2\n#IF 2\n#ARTIST first\n#ENDIF\n#ENDRANDOM\n#RANDOM 2\n#IF 1\n#GENRE second\n#ENDIF\n#ENDRANDOM\n#RANDOM 2\n#IF 1\n#TITLE third\n#ENDIF";
    let chart = parse_seeded(sequential, ParseOptions::default(), 0).unwrap();
    assert_eq!(chart.metadata["ARTIST"], "first");
    assert_eq!(chart.metadata["GENRE"], "second");
    assert_eq!(chart.metadata["TITLE"], "third");
}
#[test]
fn first_match_else_case_and_nested_restore() {
    assert_eq!(
        title(
            "#setrandom 2\n#if 1\n#TITLE bad\n#elseif 2\n#TITLE first\n#elseif 2\n#TITLE duplicate\n#else\n#TITLE bad\n#endif",
            42
        ),
        "first"
    );
    assert_eq!(
        title(
            "#SETRANDOM 2\n#IF 2\n#SETRANDOM 1\n#IF 1\n#ARTIST inner\n#ENDIF\n#ENDRANDOM\n#ENDIF\n#IF 2\n#TITLE restored\n#ENDIF\n#ENDRANDOM",
            0
        ),
        "restored"
    );
    assert_eq!(
        title(
            "#SETRANDOM 4294967295\n#IF 4294967295\n#TITLE max\n#ENDIF",
            0
        ),
        "max"
    );
}
#[test]
fn inactive_scopes_do_not_consume_draws_or_parse_payload() {
    let text = "#SETRANDOM 1\n#IF 2\n#RANDOM 2\n#IF 1\n#RANDOM 100\n#IF 99\n#SCROLL unsupported\n#00011:not-pairs\n#ENDIF\n#ENDRANDOM\n#ENDIF\n#ENDRANDOM\n#WAV01 unused.wav\n#BPM invalid\n#ENDIF\n#ENDRANDOM\n#RANDOM 2\n#IF 2\n#TITLE first-draw\n#ELSE\n#TITLE wrong\n#ENDIF";
    let parsed = parse_seeded(text, ParseOptions::default(), 0).unwrap();
    assert_eq!(parsed.metadata["TITLE"], "first-draw");
    assert!(parsed.samples.is_empty());
    let malformed = "#SETRANDOM 1\n#IF 2\n#RANDOM 0\n#ENDIF";
    assert_eq!(
        parse(malformed, ParseOptions::default()).unwrap_err().line,
        3
    );
}
#[test]
fn strict_control_scopes_and_eof_original_lines() {
    for (text, line) in [
        ("#IF 1", 1),
        ("#ELSE", 1),
        ("#ELSEIF 1", 1),
        ("#ENDIF", 1),
        ("#ENDRANDOM", 1),
        ("#RANDOM 2\n#IF 1\n#ENDRANDOM", 3),
        ("#RANDOM 2\n#IF 1\n#RANDOM 2\n#ENDIF", 4),
        ("#RANDOM 2\n#IF 1\n#ELSE\n#ELSE", 4),
        ("#RANDOM 2\n#IF 1\n#ELSE\n#ELSEIF 2", 4),
        ("#RANDOM 2\n#IF 1\n#ELSE extra", 3),
        ("#RANDOM 2\n#IF 1\n#ENDIF extra", 3),
        ("#RANDOM 2\n#ENDRANDOM extra", 2),
        ("#RANDOM -1", 1),
        ("#RANDOM +1", 1),
        ("#SETRANDOM 4294967296", 1),
        ("#RANDOM 2\n#IF 1", 2),
    ] {
        let error = parse(text, ParseOptions::default()).unwrap_err();
        assert_eq!(error.line, line, "{text}");
        assert!(matches!(error.kind, BmsErrorKind::Syntax(_)), "{error:?}");
    }
    assert!(parse("#RANDOM 2\n#RANDOM 3", ParseOptions::default()).is_ok());
}
#[test]
fn combined_depth_and_physical_caps_include_discarded_lines() {
    let exact = "#SETRANDOM 1\n".repeat(128);
    assert!(parse(&exact, ParseOptions::default()).is_ok());
    let error = parse(&(exact + "#IF 1"), ParseOptions::default()).unwrap_err();
    assert_eq!(error.line, 129);
    assert!(matches!(
        error.kind,
        BmsErrorKind::Limit("conditional depth")
    ));
    let text = "#SETRANDOM 1\n#IF 2\n#THIS_IS_DISCARDED\n#ENDIF";
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
            max_line_bytes: 15,
            ..ParseOptions::default()
        },
    ] {
        assert!(matches!(
            parse(text, options).unwrap_err().kind,
            BmsErrorKind::Limit(_)
        ));
    }
    let error = parse(
        "#SETRANDOM 1\n#IF 2\n#ENDIF\n#00011:ZZ",
        ParseOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.line, 4);
    assert!(matches!(error.kind, BmsErrorKind::MissingDefinition { .. }));
}
#[test]
fn selected_definitions_only_preserve_timing_binding_and_duplicate_policy() {
    let text = "#SETRANDOM 2\n#IF 1\n#BPM bad\n#WAV01 unused\n#00011:ZZ\n#ELSE\n#BPM 120\n#BPM01 240\n#STOP01 48\n#WAV01 selected.wav\n#00008:01\n#00009:01\n#00011:01\n#ENDIF";
    let parsed = parse(text, ParseOptions::default()).unwrap();
    assert_eq!(parsed.samples[&1], "selected.wav");
    assert_eq!(parsed.notes.len(), 1);
    assert_eq!(parsed.notes[0].line, 13);
    assert_eq!(parsed.source.bpm_changes[0].bpm.numerator(), 240);
    assert_eq!(parsed.source.stops[0].duration.as_nanos(), 250_000_000);
    assert_eq!(parsed.compile().unwrap().chart.objects().len(), 1);
    let repeated = "#SETRANDOM 1\n#IF 1\n#WAV01 a\n#WAV01 b\n#ELSE\n#WAV01 inactive\n#ENDIF";
    assert_eq!(
        parse(repeated, ParseOptions::default()).unwrap_err().line,
        4
    );
    assert_eq!(
        parse(
            repeated,
            ParseOptions {
                duplicates: DuplicatePolicy::LastWins,
                ..ParseOptions::default()
            }
        )
        .unwrap()
        .samples[&1],
        "b"
    );
    let capped = "#SETRANDOM 1\n#IF 2\n#00011:010101\n#ELSE\n#WAV01 a\n#00011:01\n#ENDIF";
    assert!(
        parse(
            capped,
            ParseOptions {
                max_objects: 1,
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
}
