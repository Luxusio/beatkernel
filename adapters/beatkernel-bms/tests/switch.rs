use beatkernel_bms::{BmsErrorKind, DuplicatePolicy, ParseOptions, parse, parse_seeded};
fn title(text: &str, seed: u64) -> String {
    parse_seeded(text, ParseOptions::default(), seed)
        .unwrap()
        .metadata["TITLE"]
        .clone()
}
#[test]
fn literal_seed_selection_and_shared_random_sequence() {
    let text = "#switch 2\n#case 1\n#TITLE one\n#skip\n#case 2\n#TITLE two\n#skip\n#def\n#TITLE default\n#endsw";
    assert_eq!(title(text, 0), "two");
    assert_eq!(title(text, 3), "one");
    let text = "#RANDOM 2\n#IF 2\n#ARTIST first\n#ENDIF\n#ENDRANDOM\n#SWITCH 2\n#CASE 1\n#GENRE second\n#SKIP\n#DEF\n#GENRE wrong\n#ENDSW\n#RANDOM 2\n#IF 1\n#TITLE third\n#ENDIF";
    let chart = parse_seeded(text, ParseOptions::default(), 0).unwrap();
    assert_eq!(chart.metadata["ARTIST"], "first");
    assert_eq!(chart.metadata["GENRE"], "second");
    assert_eq!(chart.metadata["TITLE"], "third");
    assert_eq!(
        title(
            "#SETSWITCH 4294967295\n#CASE 4294967295\n#TITLE max\n#ENDSW",
            u64::MAX
        ),
        "max"
    );
}
#[test]
fn selected_case_falls_through_repeated_unordered_labels_and_default() {
    let text = "#SETSWITCH 2\n#WAV01 a\n#CASE 1\n#00001:01\n#CASE 2\n#WAV01 a\n#00001:01\n#CASE 1\n#00001:01\n#CASE 2\n#00001:01\n#DEF\n#00001:01\n#ENDSW";
    let chart = parse(text, ParseOptions::default()).unwrap();
    // Pre-label payload and CASE1 before selection are ignored; later labels fall through.
    assert_eq!(chart.bgm.len(), 4);
    assert_eq!(chart.samples.len(), 1);
    assert_eq!(
        chart
            .bgm
            .iter()
            .map(|event| event.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        title(
            "#SETSWITCH 7\n#CASE 1\n#TITLE bad\n#DEF\n#TITLE fallback\n#ENDSW",
            0
        ),
        "fallback"
    );
    assert!(
        parse(
            "#SETSWITCH 7\n#CASE 1\n#TITLE bad\n#ENDSW",
            ParseOptions::default()
        )
        .unwrap()
        .metadata
        .is_empty()
    );
}
#[test]
fn inactive_skip_and_skipped_parent_cannot_be_resurrected() {
    assert_eq!(
        title(
            "#SETSWITCH 2\n#CASE 1\n#SKIP\n#CASE 2\n#TITLE selected\n#SKIP\n#DEF\n#TITLE bad\n#ENDSW",
            0
        ),
        "selected"
    );
    for nested in [
        "#SETRANDOM 1\n#IF 1\n#SKIP\n#ELSE\n#WAV01 bad\n#ENDIF\n#WAV02 bad\n#ENDRANDOM",
        "#SETRANDOM 1\n#IF 2\n#WAV01 bad\n#ELSE\n#SKIP\n#WAV02 bad\n#ENDIF\n#WAV03 bad\n#ENDRANDOM",
        "#SETRANDOM 1\n#SKIP\n#IF 2\n#WAV01 bad\n#ELSE\n#WAV02 bad\n#ENDIF\n#ENDRANDOM",
    ] {
        let text = format!(
            "#SETSWITCH 1\n#CASE 1\n{nested}\n#CASE 2\n#WAV04 bad\n#DEF\n#WAV05 bad\n#ENDSW\n#TITLE outside"
        );
        let chart = parse(&text, ParseOptions::default()).unwrap();
        assert!(chart.samples.is_empty());
        assert_eq!(chart.metadata["TITLE"], "outside");
    }
    // SKIP terminates the nearest switch, leaving the outer one active.
    assert_eq!(
        title(
            "#SETSWITCH 1\n#CASE 1\n#SETSWITCH 1\n#CASE 1\n#SKIP\n#DEF\n#TITLE bad\n#ENDSW\n#TITLE outer\n#ENDSW",
            0
        ),
        "outer"
    );
}
#[test]
fn inactive_nested_controls_do_not_consume_and_switch_does_not_replace_if_choice() {
    let text = "#SETSWITCH 1\n#CASE 2\n#SWITCH 2\n#CASE 2\n#SKIP\n#ENDSW\n#RANDOM 2\n#IF 1\n#SCROLL unsupported\n#ENDIF\n#ENDRANDOM\n#CASE 1\n#SWITCH 2\n#CASE 2\n#TITLE first-draw\n#SKIP\n#ENDSW\n#ENDSW";
    assert_eq!(title(text, 0), "first-draw");
    assert_eq!(
        title(
            "#SETRANDOM 7\n#SETSWITCH 2\n#CASE 2\n#IF 7\n#TITLE random-choice\n#ENDIF\n#ENDSW\n#ENDRANDOM",
            0
        ),
        "random-choice"
    );
    assert!(
        parse(
            "#SETSWITCH 1\n#CASE 1\n#IF 1\n#ENDIF\n#ENDSW",
            ParseOptions::default()
        )
        .is_err()
    );
    let malformed = "#SETSWITCH 1\n#CASE 2\n#SWITCH 0\n#ENDSW";
    assert_eq!(
        parse(malformed, ParseOptions::default()).unwrap_err().line,
        3
    );
}
#[test]
fn control_validation_strict_scopes_and_eof_preserve_original_lines() {
    for (text, line) in [
        ("#CASE 1", 1),
        ("#DEF", 1),
        ("#SKIP", 1),
        ("#ENDSW", 1),
        ("#SWITCH 0", 1),
        ("#SETSWITCH +1", 1),
        ("#SWITCH 4294967296", 1),
        ("#SWITCH 2\n#CASE 0", 2),
        ("#SWITCH 2\n#SKIP", 2),
        ("#SWITCH 2\n#DEF\n#DEF", 3),
        ("#SWITCH 2\n#DEF\n#CASE 1", 3),
        ("#SWITCH 2\n#DEF extra", 2),
        ("#SWITCH 2\n#CASE 1\n#SKIP extra", 3),
        ("#SWITCH 2\n#ENDSW extra", 2),
        ("#SWITCH 2\n#RANDOM 1\n#ENDSW", 3),
        ("#RANDOM 1\n#IF 1\n#SWITCH 2\n#ENDIF", 4),
        ("#SWITCH 2\n#RANDOM 1\n#CASE 1", 3),
        ("#SWITCH 2\n#RANDOM 1\n#DEF", 3),
        ("#RANDOM 1\n#SWITCH 2\n#ENDRANDOM", 3),
        ("#SWITCH 2", 1),
        ("#SETSWITCH 1\n#CASE 2\n#SWITCH 2\n#ENDSW", 1),
    ] {
        let error = parse(text, ParseOptions::default()).unwrap_err();
        assert_eq!(error.line, line, "{text}");
        assert!(matches!(error.kind, BmsErrorKind::Syntax(_)), "{error:?}");
    }
    assert!(parse("#SWITCH 2\n#ENDSW", ParseOptions::default()).is_ok());
    assert!(parse("#SWITCH 2\n#DEF\n#SKIP\n#ENDSW", ParseOptions::default()).is_ok());
}
#[test]
fn shared_depth_physical_bounds_and_selected_diagnostics() {
    let mut text = "#SWITCH 2\n".repeat(128);
    text.push_str(&"#ENDSW\n".repeat(128));
    assert!(parse(&text, ParseOptions::default()).is_ok());
    let text = format!("{}#SWITCH 2", "#RANDOM 1\n".repeat(128));
    let error = parse(&text, ParseOptions::default()).unwrap_err();
    assert_eq!(error.line, 129);
    assert!(matches!(
        error.kind,
        BmsErrorKind::Limit("conditional depth")
    ));
    let text = "#SETSWITCH 1\n#CASE 2\n#SCROLL discarded_payload\n#ENDSW";
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
            max_line_bytes: 19,
            ..ParseOptions::default()
        },
    ] {
        assert!(matches!(
            parse(text, options).unwrap_err().kind,
            BmsErrorKind::Limit(_)
        ));
    }
    let error = parse(
        "#SETSWITCH 1\n#CASE 2\n#00011:ZZ\n#CASE 1\n#00011:ZZ\n#ENDSW",
        ParseOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.line, 5);
    assert!(matches!(error.kind, BmsErrorKind::MissingDefinition { .. }));
}
#[test]
fn selected_payload_reuses_timing_and_duplicate_policy() {
    let text = "#SETSWITCH 2\n#CASE 1\n#BPM bad\n#00011:ZZ\n#CASE 2\n#BPM 120\n#WAV01 head.wav\n#BPM01 240\n#STOP01 48\n#00008:01\n#00009:01\n#00011:01\n#SKIP\n#DEF\n#WAV01 ignored\n#ENDSW";
    let chart = parse(text, ParseOptions::default()).unwrap();
    assert_eq!(chart.notes[0].line, 12);
    assert_eq!(chart.source.stops[0].duration.as_nanos(), 250_000_000);
    assert_eq!(chart.source.bpm_changes[0].bpm.numerator(), 240);
    assert_eq!(chart.compile().unwrap().chart.objects().len(), 1);
    let repeated = "#SETSWITCH 1\n#CASE 1\n#WAV01 first\n#CASE 9\n#WAV01 later\n#ENDSW";
    assert_eq!(
        parse(repeated, ParseOptions::default()).unwrap_err().line,
        5
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
        "later"
    );
}
