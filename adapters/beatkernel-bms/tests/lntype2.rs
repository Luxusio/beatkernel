//! Deferred genuine parser/compiler fixtures for the documented cell-span dialect.
use beatkernel_bms::{BmsChart, BmsErrorKind, DuplicatePolicy, ParseOptions, parse, parse_seeded};

fn lane_times(chart: &BmsChart, lane: u8) -> Vec<(i64, i64)> {
    let compiled = chart.compile().unwrap();
    chart
        .notes
        .iter()
        .filter(|note| note.lane.channel() == lane)
        .map(|note| {
            let object = compiled
                .chart
                .objects()
                .iter()
                .find(|object| object.id == note.object)
                .unwrap();
            (
                object.time.start.as_nanos(),
                object.time.end.unwrap().as_nanos(),
            )
        })
        .collect()
}

#[test]
fn nonzero_cells_union_overlaps_and_measure_touches_but_zeros_and_omitted_measures_leave_gaps() {
    let text = "#BPM 60\n#lntype 02\n#WAV01 head.wav\n\
        #00051:01ZZ0001\n#00151:ZZ000000\n#00351:01\n\
        #00052:0100\n#00052:00ZZ00\n#00052:00000100\n#00066:000001\n";
    let chart = parse(text, ParseOptions::default()).unwrap();
    assert_eq!(
        lane_times(&chart, 0x11),
        [
            (0, 2_000_000_000),
            (3_000_000_000, 5_000_000_000),
            (12_000_000_000, 16_000_000_000)
        ]
    );
    assert_eq!(lane_times(&chart, 0x12), [(0, 3_000_000_000)]);
    assert_eq!(lane_times(&chart, 0x26), [(2_666_666_666, 4_000_000_000)]);
    assert!(
        chart
            .notes
            .iter()
            .all(|note| note.sample.0 == 1 && note.tail_sample.is_none())
    );
    assert!(
        !chart.samples.contains_key(&1295),
        "continuation values are not sounded WAV references"
    );
    assert_eq!(
        chart
            .notes
            .iter()
            .find(|note| note.lane.channel() == 0x12)
            .unwrap()
            .line,
        7
    );
    assert!(
        chart
            .notes
            .iter()
            .any(|note| note.lane.is_scratch() && note.lane.player() == 2)
    );
    assert!(chart.bgm.is_empty());
    let final_hold = chart
        .source
        .objects
        .iter()
        .max_by_key(|object| object.end)
        .unwrap();
    assert_eq!(final_hold.end.unwrap(), chart.measures.last().unwrap().end);
}

#[test]
fn same_head_policy_resolves_the_entire_cell_before_union_and_zero_rows_never_erase() {
    let text = "#BPM 60\n#LNTYPE 2\n#WAV01 first.wav\n#WAV02 replacement.wav\n\
        #00051:01\n#00051:02000000\n#00051:00\n";
    let error = parse(text, ParseOptions::default()).unwrap_err();
    assert_eq!(error.line, 6);
    assert!(matches!(error.kind, BmsErrorKind::Duplicate(_)));
    let last = ParseOptions {
        duplicates: DuplicatePolicy::LastWins,
        ..ParseOptions::default()
    };
    let chart = parse(text, last).unwrap();
    assert_eq!(lane_times(&chart, 0x11), [(0, 1_000_000_000)]);
    assert_eq!(chart.notes[0].sample.0, 2);
    assert_eq!(chart.notes[0].line, 6);
    assert_eq!(chart.notes[0].tail_sample, None);
    let extended = parse(
        &text.replace("#00051:01\n#00051:02000000", "#00051:02000000\n#00051:01"),
        last,
    )
    .unwrap();
    assert_eq!(lane_times(&extended, 0x11), [(0, 4_000_000_000)]);
    assert_eq!(extended.notes[0].sample.0, 1);
    let repeated_header = "#BPM 60\n#LNTYPE 1\n#LNTYPE 02\n#WAV01 a.wav\n#00051:01";
    assert!(matches!(
        parse(repeated_header, ParseOptions::default())
            .unwrap_err()
            .kind,
        BmsErrorKind::Duplicate("LNTYPE")
    ));
    assert_eq!(
        lane_times(&parse(repeated_header, last).unwrap(), 0x11),
        [(0, 4_000_000_000)]
    );
}

#[test]
fn cell_endpoints_keep_exact_resolution_tempo_stop_measure_ratios_and_selected_branch_policy() {
    let fractional = "#BPM 60\n#LNTYPE 2\n#WAV01 a.wav\n#00002:0.125\n#00051:010000";
    let exact = parse(fractional, ParseOptions::default()).unwrap();
    assert_eq!(exact.source.ticks_per_beat, 6);
    assert_eq!(exact.source.objects[0].start.ticks(), 0);
    assert_eq!(exact.source.objects[0].end.unwrap().ticks(), 1);
    assert_eq!(lane_times(&exact, 0x11), [(0, 166_666_666)]);
    assert_eq!(
        parse(
            fractional,
            ParseOptions {
                max_resolution: 2,
                ..ParseOptions::default()
            }
        )
        .unwrap_err()
        .kind,
        BmsErrorKind::Resolution
    );
    let timed = parse(
        "#BPM 60\n#BPM0A 120\n#STOP01 48\n#LNTYPE 2\n#WAV01 a.wav\n\
        #00002:0.75\n#00102:0.5\n#00051:0101\n#00151:ZZ00\n#00008:000A\n#00009:0001",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(timed.source.ticks_per_beat, 2);
    assert_eq!(timed.source.objects[0].end.unwrap().ticks(), 8);
    assert_eq!(timed.source.stops[0].duration.as_nanos(), 500_000_000);
    assert_eq!(lane_times(&timed, 0x11), [(0, 3_250_000_000)]);
    let branches = "#BPM 60\n#WAV01 a.wav\n#RANDOM 2\n#IF 1\n#LNTYPE 02\n#00051:01ZZ0000\n\
        #ELSE\n#LNTYPE 01\n#00051:010000ZZ\n#ENDIF\n#ENDRANDOM";
    let cells = parse_seeded(branches, ParseOptions::default(), 3).unwrap();
    let pairs = parse_seeded(branches, ParseOptions::default(), 0).unwrap();
    assert_eq!(lane_times(&cells, 0x11), [(0, 2_000_000_000)]);
    assert_eq!(cells.notes[0].tail_sample, None);
    assert_eq!(lane_times(&pairs, 0x11), [(0, 3_000_000_000)]);
    assert_eq!(pairs.notes[0].tail_sample.unwrap().0, 1295);
}

#[test]
fn continuations_do_not_bypass_head_definitions_collision_checks_or_nonzero_and_physical_limits() {
    let text = "#LNTYPE 2\n#WAV01 a.wav\n#00051:01ZZ";
    let error = parse(
        text,
        ParseOptions {
            max_objects: 1,
            ..ParseOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.line, 3);
    assert_eq!(error.kind, BmsErrorKind::Limit("nonzero tokens"));
    assert_eq!(
        parse(
            text,
            ParseOptions {
                max_objects: 2,
                ..ParseOptions::default()
            }
        )
        .unwrap()
        .notes
        .len(),
        1
    );
    let zeros = format!("#LNTYPE 2\n#00051:{}", "00".repeat(4096));
    assert!(
        parse(
            &zeros,
            ParseOptions {
                max_objects: 1,
                ..ParseOptions::default()
            }
        )
        .unwrap()
        .notes
        .is_empty()
    );
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
            max_line_bytes: 10,
            ..ParseOptions::default()
        },
    ] {
        assert!(matches!(
            parse(text, options).unwrap_err().kind,
            BmsErrorKind::Limit(_)
        ));
    }
    for row in ["#00051:ZZ00", "#00051:0100ZZ00", "#00051:01ZZ\n#00001:ZZ"] {
        assert!(matches!(
            parse(
                &format!("#LNTYPE 2\n#WAV01 a.wav\n{row}"),
                ParseOptions::default()
            )
            .unwrap_err()
            .kind,
            BmsErrorKind::MissingDefinition {
                kind: "WAV",
                index: 1295
            }
        ));
    }
    for rows in [
        "#00051:01\n#00011:0001",
        "#00051:0100\n#LNOBJ ZZ\n#00011:000100ZZ",
    ] {
        for duplicates in [DuplicatePolicy::Reject, DuplicatePolicy::LastWins] {
            assert!(matches!(
                parse(
                    &format!("#LNTYPE 2\n#WAV01 a.wav\n{rows}"),
                    ParseOptions {
                        duplicates,
                        ..ParseOptions::default()
                    }
                )
                .unwrap_err()
                .kind,
                BmsErrorKind::LongNote(_)
            ));
        }
    }
    let disjoint = parse(
        "#BPM 60\n#LNTYPE 2\n#LNOBJ ZZ\n#WAV01 a.wav\n#00051:01000000\n#00011:000001ZZ",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        lane_times(&disjoint, 0x11),
        [(0, 1_000_000_000), (2_000_000_000, 3_000_000_000)]
    );
    assert_eq!(
        disjoint
            .notes
            .iter()
            .filter(|note| note.tail_sample.is_none())
            .count(),
        1
    );
}
