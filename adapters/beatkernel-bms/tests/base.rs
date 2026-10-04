//! Deferred resource-radix fixtures with literal IDs, timings and physical lines.
use beatkernel_bms::{
    BgaChannel, BgaCrop, BmsErrorKind, DuplicatePolicy, ImageId, ParseOptions, parse, parse_seeded,
};

fn last_wins() -> ParseOptions {
    ParseOptions {
        duplicates: DuplicatePolicy::LastWins,
        ..ParseOptions::default()
    }
}

#[test]
fn absent_base_is_36_and_a_late_selected_header_reinterprets_all_resource_tokens() {
    for (header, id) in [("", 1295), ("#BASE 36\n", 1295), ("#bAsE 62\n", 3843)] {
        let text = format!("#BPM 60\n#WaVzz Case/音.wav\n#00011:zz\n#00001:zz\n{header}");
        let chart = parse(&text, ParseOptions::default()).unwrap();
        assert_eq!(chart.samples.len(), 1);
        assert_eq!(chart.samples[&id], "Case/音.wav");
        assert_eq!(chart.notes[0].sample.0, u64::from(id));
        assert_eq!(chart.notes[0].line, 3);
        assert_eq!(chart.bgm[0].sample.0, u64::from(id));
        assert_eq!(chart.source.objects[0].audio.unwrap().0, u32::from(id));
        assert_eq!(chart.source.objects[0].start.ticks(), 0);
        assert!(!chart.metadata.contains_key("BASE"));
        assert_eq!(
            chart.compile().unwrap().chart.objects()[0]
                .time
                .start
                .as_nanos(),
            0
        );
    }
    for (base, maximum, maximum_id) in [(16, "fF", 255), (36, "zZ", 1295), (62, "zz", 3843)] {
        let text = format!(
            "#WAV0A upper.wav\n#WAV{maximum} maximum.wav\n#00011:0A{maximum}\n#BASE {base}"
        );
        let chart = parse(&text, ParseOptions::default()).unwrap();
        assert_eq!(
            chart.notes.iter().map(|n| n.sample.0).collect::<Vec<_>>(),
            [10, maximum_id]
        );
        assert_eq!(
            chart
                .source
                .objects
                .iter()
                .map(|o| o.start.ticks())
                .collect::<Vec<_>>(),
            [0, 2]
        );
    }
    let distinct = parse(
        "#wav0A Upper.wav\n#WaV0a lower.wav\n#WAVzz last.wav\n#00011:0A0azz00\n#BASE 62",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        distinct.samples.keys().copied().collect::<Vec<_>>(),
        [10, 36, 3843]
    );
    assert_eq!(
        distinct
            .notes
            .iter()
            .map(|n| n.sample.0)
            .collect::<Vec<_>>(),
        [10, 36, 3843]
    );
    assert_eq!(distinct.samples[&10], "Upper.wav");
    assert_eq!(distinct.samples[&36], "lower.wav");
}

#[test]
fn base62_flows_through_every_resource_namespace_without_changing_hex_channels() {
    let text = "#bPm 60\n#wAv0A upper.wav\n#WaV0a lower.wav\n#WAVzz max.wav\n\
        #bMp00 poor.bmp\n#bMp0A upper.bmp\n#BmP0a lower.bmp\n#BMPzz max.bmp\n\
        #bPm0A 120\n#BpM0a 240\n#sToP0A 48\n#StOp0a 96\n#lNoBj zz\n\
        #bGa0A a -1 -2 3 4 5 6\n#@BgA0a z -1 -2 4 6 5 6\n#BGAzz zz 0 0 1 1 0 0\n\
        #00001:0A0azz00\n#00011:0A00zz00\n#00012:0a000000\n#00053:000azz00\n\
        #00004:0A0azz00\n#00006:0a000000\n#00007:zz000000\n#0000a:0A000000\n\
        #00008:0A000a00\n#00009:000A000a\n#00103:Af\n\
        #0010b:Af\n#0010C:80\n#0010D:01\n#0010E:ff\n#bAsE 62";
    let chart = parse(text, ParseOptions::default()).unwrap();
    assert_eq!(chart.metadata["LNOBJ"], "zz");
    assert!(!chart.metadata.contains_key("BASE"));
    assert!(chart.warnings.is_empty());
    assert_eq!(chart.source.ticks_per_beat, 1);
    assert_eq!(chart.source.initial_bpm.numerator(), 60);
    assert_eq!(
        chart
            .source
            .bpm_changes
            .iter()
            .map(|b| (b.beat.ticks(), b.bpm.numerator()))
            .collect::<Vec<_>>(),
        [(0, 120), (2, 240), (4, 175)]
    );
    assert_eq!(
        chart
            .source
            .stops
            .iter()
            .map(|s| (s.beat.ticks(), s.duration.as_nanos()))
            .collect::<Vec<_>>(),
        [(1, 500_000_000), (3, 500_000_000)]
    );
    assert_eq!(
        chart.images.keys().map(|id| id.0).collect::<Vec<_>>(),
        [0, 10, 36, 3843]
    );
    assert_eq!(chart.images[&ImageId(0)], "poor.bmp");
    assert_eq!(chart.images[&ImageId(36)], "lower.bmp");
    assert_eq!(
        chart.bga_crops[&ImageId(10)],
        BgaCrop {
            source: ImageId(36),
            source_rect: [-1, -2, 3, 4],
            destination: [5, 6],
        }
    );
    assert_eq!(
        chart.bga_crops[&ImageId(36)],
        BgaCrop {
            source: ImageId(61),
            source_rect: [-1, -2, 3, 4],
            destination: [5, 6],
        }
    );
    assert_eq!(chart.bga_crops[&ImageId(3843)].source, ImageId(3843));
    for (channel, expected) in [
        (BgaChannel::Base, vec![(0, 10), (1, 36), (2, 3843)]),
        (BgaChannel::Poor, vec![(0, 36)]),
        (BgaChannel::Layer, vec![(0, 3843)]),
        (BgaChannel::Layer2, vec![(0, 10)]),
    ] {
        assert_eq!(
            chart
                .bga
                .iter()
                .filter(|e| e.channel == channel)
                .map(|e| (e.beat.ticks(), e.image.0))
                .collect::<Vec<_>>(),
            expected
        );
    }
    assert_eq!(
        chart
            .bga_opacity
            .iter()
            .map(|e| (e.channel, e.alpha))
            .collect::<Vec<_>>(),
        [
            (BgaChannel::Base, 175),
            (BgaChannel::Layer, 128),
            (BgaChannel::Layer2, 1),
            (BgaChannel::Poor, 255)
        ]
    );
    assert!(chart.bga_opacity.iter().all(|e| e.beat.ticks() == 4));
    assert_eq!(chart.notes.len(), 3);
    let compiled = chart.compile().unwrap();
    for (lane, sample, tail, start, end, start_ns, end_ns) in [
        (0x11, 10, Some(3843), 0, Some(2), 0, Some(1_500_000_000)),
        (0x12, 36, None, 0, None, 0, None),
        (
            0x13,
            36,
            Some(3843),
            1,
            Some(2),
            500_000_000,
            Some(1_500_000_000),
        ),
    ] {
        let note = chart
            .notes
            .iter()
            .find(|n| n.lane.channel() == lane)
            .unwrap();
        let source = chart
            .source
            .objects
            .iter()
            .find(|o| o.id == note.object)
            .unwrap();
        let timed = compiled
            .chart
            .objects()
            .iter()
            .find(|o| o.id == note.object)
            .unwrap();
        assert_eq!(
            (note.sample.0, note.tail_sample.map(|s| s.0)),
            (sample, tail)
        );
        assert_eq!(
            (source.start.ticks(), source.end.map(|b| b.ticks())),
            (start, end)
        );
        assert_eq!(
            (
                timed.time.start.as_nanos(),
                timed.time.end.map(|t| t.as_nanos())
            ),
            (start_ns, end_ns)
        );
    }
    assert_eq!(
        compiled
            .bgm
            .iter()
            .map(|e| (e.sample.0, e.at.as_nanos()))
            .collect::<Vec<_>>(),
        [(10, 0), (36, 500_000_000), (3843, 1_500_000_000)]
    );
    assert!(
        compiled
            .bga_opacity
            .iter()
            .all(|e| e.at.as_nanos() == 2_500_000_000)
    );
}

#[test]
fn duplicate_policy_selects_filewide_base_and_case_folded_definition_values_explicitly() {
    let bases = "#WAV0a note.wav\n#00011:0a\n#BASE 16\n#base 62";
    let error = parse(bases, ParseOptions::default()).unwrap_err();
    assert_eq!(
        (error.line, error.kind),
        (4, BmsErrorKind::Duplicate("BASE"))
    );
    assert_eq!(parse(bases, last_wins()).unwrap().notes[0].sample.0, 36);
    assert_eq!(
        parse(
            &bases.replace("#BASE 16\n#base 62", "#BASE 62\n#base 16"),
            last_wins()
        )
        .unwrap()
        .notes[0]
            .sample
            .0,
        10
    );
    assert!(matches!(
        parse("#BASE 62\n#BASE 62", ParseOptions::default())
            .unwrap_err()
            .kind,
        BmsErrorKind::Duplicate("BASE")
    ));
    for (text, line) in [
        ("#BASE 16\n#BASE invalid", 2),
        ("#BASE invalid\n#BASE 62", 1),
    ] {
        let error = parse(text, last_wins()).unwrap_err();
        assert_eq!(error.line, line);
        assert!(matches!(error.kind, BmsErrorKind::Syntax(_)));
    }
    for base in [16, 36] {
        for (definitions, reference) in [
            ("#WAV0A first.wav\n#wav0a second.wav", "#00011:0A"),
            ("#BMP0A first.bmp\n#bmp0a second.bmp", "#00004:0A"),
            ("#BPM0A 120\n#bpm0a 240", "#00008:0A"),
            ("#STOP0A 48\n#stop0a 96", "#00009:0A"),
            ("#BGA0A A 0 0 1 1 0 0\n#@bga0a B 0 0 2 3 4 5", "#00004:0A"),
            ("#LNOBJ 0B\n#lnobj 0C", "#WAV01 head.wav\n#00011:010C"),
        ] {
            let text = format!("#BPM 60\n{definitions}\n{reference}\n#BASE {base}");
            let error = parse(&text, ParseOptions::default()).unwrap_err();
            assert_eq!(error.line, 3);
            assert!(matches!(error.kind, BmsErrorKind::Duplicate(_)));
            let chart = parse(&text, last_wins()).unwrap();
            if definitions.starts_with("#WAV") {
                assert_eq!(chart.samples[&10], "second.wav");
            }
            if definitions.starts_with("#BMP") {
                assert_eq!(chart.images[&ImageId(10)], "second.bmp");
            }
            if definitions.starts_with("#BPM") {
                assert_eq!(chart.source.bpm_changes[0].bpm.numerator(), 240);
            }
            if definitions.starts_with("#STOP") {
                assert_eq!(chart.source.stops[0].duration.as_nanos(), 2_000_000_000);
            }
            if definitions.starts_with("#BGA") {
                assert_eq!(
                    chart.bga_crops[&ImageId(10)],
                    BgaCrop {
                        source: ImageId(11),
                        source_rect: [0, 0, 2, 3],
                        destination: [4, 5],
                    }
                );
            }
            if definitions.starts_with("#LNOBJ") {
                assert_eq!(chart.notes[0].tail_sample.unwrap().0, 12);
            }
        }
    }
    // Resource-position duplicates use the selected radix too; zeros do not erase.
    let rows = "#BASE 62\n#WAV0A a\n#WAV0a b\n#00011:0A\n#00011:0a\n#00011:00";
    assert_eq!(parse(rows, ParseOptions::default()).unwrap_err().line, 5);
    let chart = parse(rows, last_wins()).unwrap();
    assert_eq!((chart.notes[0].sample.0, chart.notes[0].line), (36, 5));
}

#[test]
fn strict_base_and_digit_errors_keep_original_physical_lines_and_hex_exceptions() {
    for value in [
        "", "0", "2", "17", "35", "63", "016", "+16", "-16", "16.0", "62 extra", "６２",
    ] {
        let text = format!("; physical comment\n\n#BPM 60\n#BASE {value}");
        let error = parse(&text, ParseOptions::default()).unwrap_err();
        assert_eq!(error.line, 4, "{value:?}");
        assert!(matches!(error.kind, BmsErrorKind::Syntax(_)));
    }
    for row in [
        "#WAV0G bad.wav",
        "#BMP0g bad.bmp",
        "#BPM0G 120",
        "#STOP0g 48",
        "#LNOBJ 0g",
        "#BGA0G 1 0 0 1 1 0 0",
        "#@BGA01 g 0 0 1 1 0 0",
        "#00011:0G",
        "#00051:0g",
        "#00001:0G",
        "#00004:0g",
        "#00008:0G",
        "#00009:0g",
    ] {
        let error = parse(
            &format!("; one\n\n{row}\n#BASE 16"),
            ParseOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.line, 3, "{row}");
        assert!(
            matches!(error.kind, BmsErrorKind::Syntax(_)),
            "{row}: {error:?}"
        );
    }
    for base in [36, 62] {
        for row in [
            "#WAV0_ bad",
            "#BMPé bad",
            "#00011:0_",
            "#BGA01 _ 0 0 1 1 0 0",
        ] {
            assert!(parse(&format!("#BASE {base}\n{row}"), ParseOptions::default()).is_err());
        }
    }
    for row in [
        "#00003:zz",
        "#0000B:0g",
        "#0000C:zz",
        "#0000D:0G",
        "#0000E:zz",
    ] {
        let error = parse(&format!("#BASE 62\n{row}"), ParseOptions::default()).unwrap_err();
        assert_eq!(error.line, 2);
        assert!(matches!(error.kind, BmsErrorKind::Syntax(_)));
    }
    for (definitions, row, kind) in [
        ("#WAV0A upper", "#00011:0a", "WAV"),
        ("#BPM0A 120", "#00008:0a", "BPM"),
        ("#STOP0A 48", "#00009:0a", "STOP"),
    ] {
        let error = parse(
            &format!("#BASE 62\n{definitions}\n{row}"),
            ParseOptions::default(),
        )
        .unwrap_err();
        assert_eq!(
            (error.line, error.kind),
            (3, BmsErrorKind::MissingDefinition { kind, index: 36 })
        );
    }
}

#[test]
fn selected_seeded_base_is_retroactive_discarded_headers_do_not_count_but_physical_limits_do() {
    let text = "#WAV0a shared.wav\n#00011:0a\n#RANDOM 2\n#IF 1\n#BASE 16\n#TITLE sixteen\n\
        #ELSE\n#BASE 62\n#TITLE sixty-two\n#ENDIF\n#ENDRANDOM\n\
        #SETRANDOM 1\n#IF 2\n#BASE invalid\n#BASE 36\n#WAV0_ invalid\n#00011:broken\n#ENDIF\n#ENDRANDOM";
    for (seed, sample, title) in [(3, 10, "sixteen"), (0, 36, "sixty-two")] {
        let chart = parse_seeded(text, ParseOptions::default(), seed).unwrap();
        assert_eq!(chart.metadata["TITLE"], title);
        assert_eq!((chart.notes[0].sample.0, chart.notes[0].line), (sample, 2));
        assert_eq!(chart.samples.len(), 1);
        assert_eq!(
            chart,
            parse_seeded(text, ParseOptions::default(), seed).unwrap()
        );
        assert_eq!(
            parse_seeded(
                text,
                ParseOptions {
                    max_objects: 1,
                    ..ParseOptions::default()
                },
                seed
            )
            .unwrap()
            .notes
            .len(),
            1
        );
    }
    let discarded = "#SETRANDOM 1\n#IF 2\n#BASE invalid-with-a-long-physical-value\n#ENDIF\n#ENDRANDOM\n#BASE 62";
    assert!(parse(discarded, ParseOptions::default()).is_ok());
    for options in [
        ParseOptions {
            max_bytes: discarded.len() - 1,
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
        let error = parse(discarded, options).unwrap_err();
        assert!(matches!(error.kind, BmsErrorKind::Limit(_)));
        if options.max_bytes == ParseOptions::default().max_bytes {
            assert_eq!(error.line, 3);
        }
    }
    let selected_error = "#SETRANDOM 1\n#IF 2\n#BASE nope\n#ELSE\n#BASE 16\n#00011:0g\n#ENDIF";
    assert_eq!(
        parse(selected_error, ParseOptions::default())
            .unwrap_err()
            .line,
        6
    );
}

#[test]
fn crop_sources_keep_one_or_two_digits_and_generic_images_admit_exactly_the_base62_ceiling() {
    for (base, token, destination, one_digit) in [
        (16, "fF", 255, 15),
        (36, "zZ", 1295, 35),
        (62, "zz", 3843, 61),
    ] {
        let digit = if base == 16 { "f" } else { "z" };
        let text =
            format!("#BGA{token} {digit} 0 0 2 3 -4 5\n#@BGA00 {token} 1 2 3 4 5 6\n#BASE {base}");
        let chart = parse(&text, ParseOptions::default()).unwrap();
        assert_eq!(
            chart.bga_crops[&ImageId(destination)].source,
            ImageId(one_digit)
        );
        assert_eq!(
            chart.bga_crops[&ImageId(0)],
            BgaCrop {
                source: ImageId(destination),
                source_rect: [1, 2, 4, 6],
                destination: [5, 6],
            }
        );
    }
    for source in ["", "000", "+a", "あ"] {
        assert!(
            parse(
                &format!("#BASE 62\n#BGA01 {source} 0 0 1 1 0 0"),
                ParseOptions::default()
            )
            .is_err()
        );
    }
    let maximum = BgaCrop {
        source: ImageId(3843),
        source_rect: [0, 0, 1, 1],
        destination: [0, 0],
    };
    assert!(maximum.validate().is_ok());
    assert!(
        BgaCrop {
            source: ImageId(3844),
            ..maximum
        }
        .validate()
        .is_err()
    );
    let cells = parse(
        "#LNTYPE 2\n#WAV0a head.wav\n#00051:0azz0000\n#BASE 62",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(cells.notes.len(), 1);
    assert_eq!(
        (cells.notes[0].sample.0, cells.notes[0].tail_sample),
        (36, None)
    );
    assert_eq!(cells.source.objects[0].end.unwrap().ticks(), 2);
    assert!(
        !cells.samples.contains_key(&3843),
        "cell continuations do not become sounded resource heads"
    );
    let decimal_measure = parse(
        "#BASE 62\n#BPM 60\n#WAV0a head.wav\n#00002:0.5\n#00111:0a",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        (
            decimal_measure.measures[0].number,
            decimal_measure.measures[0].end.ticks()
        ),
        (0, 2)
    );
    assert_eq!(
        (
            decimal_measure.notes[0].lane.channel(),
            decimal_measure.source.objects[0].start.ticks()
        ),
        (0x11, 2)
    );
    assert_eq!(
        decimal_measure.compile().unwrap().chart.objects()[0]
            .time
            .start
            .as_nanos(),
        2_000_000_000
    );
}
