//! Deferred source/timing fixtures. Mines remain separate from judged objects,
//! keysounds, hazard outcomes and gauge/runtime behavior.
use crate::{
    parse, parse_seeded, BmsChart, BmsErrorKind, BmsLane, DuplicatePolicy, MineDamage, ParseOptions,
};
use beatkernel::{
    chart::{Beat, Bpm, BpmChange, Stop, MAX_SOURCE_ITEMS},
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    time::Duration,
};
use std::fmt::Write;

fn last_wins() -> ParseOptions {
    ParseOptions {
        duplicates: DuplicatePolicy::LastWins,
        ..ParseOptions::default()
    }
}
fn fingerprint(chart: &BmsChart) -> u64 {
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(100),
            late: Duration::from_nanos(100),
        }],
        Duration::ZERO,
    )
    .unwrap();
    JudgeEngine::new(chart.compile().unwrap().chart, chart.rules(), profile)
        .unwrap()
        .stable_hash()
        .unwrap()
}

#[test]
fn fixed_base36_damage_maps_every_original_lane_and_preserves_selection_duplicate_and_physical_line_rules()
 {
    for raw in 1..=1294 {
        let damage = MineDamage::from_raw(raw).unwrap();
        assert_eq!(damage.raw(), raw);
        assert!(!damage.is_fatal());
        assert_eq!(damage.half_percent_units(), Some(raw));
    }
    let fatal = MineDamage::from_raw(1295).unwrap();
    assert!(fatal.is_fatal());
    assert_eq!(fatal.raw(), 1295);
    assert_eq!(fatal.half_percent_units(), None);
    for raw in [0, 1296, u16::MAX] {
        assert!(matches!(
            MineDamage::from_raw(raw),
            Err(BmsErrorKind::Syntax(_))
        ));
    }
    for base in ["", "#BASE 16", "#BASE 36", "#BASE 62"] {
        let mut text = "#BPM 60\n".to_owned();
        for channel in [
            0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5,
            0xe6, 0xe7, 0xe8, 0xe9,
        ] {
            writeln!(text, "#000{channel:02X}:001E00").unwrap();
        }
        text.push_str(base); // Late resource BASE does not reinterpret damage.
        let chart = parse(&text, ParseOptions::default()).unwrap();
        assert_eq!(
            (
                chart.source.ticks_per_beat,
                chart.bga_ticks_per_beat,
                chart.invisible_ticks_per_beat,
                chart.mine_ticks_per_beat
            ),
            (1, 1, 1, 3)
        );
        assert!(
            chart.samples.is_empty()
                && chart.notes.is_empty()
                && chart.bgm.is_empty()
                && chart.invisible.is_empty()
        );
        assert!(chart.rules().is_empty() && chart.compile().unwrap().chart.objects().is_empty());
        assert_eq!(
            chart
                .mines
                .iter()
                .map(|event| event.lane.channel())
                .collect::<Vec<_>>(),
            [
                0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x21, 0x22, 0x23, 0x24, 0x25,
                0x26, 0x27, 0x28, 0x29
            ]
        );
        for (index, (source, scheduled)) in chart
            .mines
            .iter()
            .zip(chart.compile_mines().unwrap())
            .enumerate()
        {
            assert_eq!(
                (
                    source.beat.ticks(),
                    source.damage.raw(),
                    source.ordinal,
                    source.line
                ),
                (4, 50, index as u64, index + 2)
            );
            assert_eq!(scheduled.at.as_nanos(), 1_333_333_333);
            assert_eq!(
                (
                    scheduled.lane,
                    scheduled.damage,
                    scheduled.ordinal,
                    scheduled.line
                ),
                (source.lane, source.damage, index as u64, index + 2)
            );
        }
    }
    let cases = parse(
        "#BASE 62\n#000d1:0A0a1ezzzy\n#WAV00 optional-explosion.wav",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        cases
            .mines
            .iter()
            .map(|mine| mine.damage.raw())
            .collect::<Vec<_>>(),
        [10, 10, 50, 1295, 1294]
    );
    assert_eq!(cases.samples[&0], "optional-explosion.wav");
    assert_eq!(cases.samples.len(), 1);
    assert_eq!(
        parse("#BASE 16\n#000D1:0G", ParseOptions::default())
            .unwrap()
            .mines[0]
            .damage
            .raw(),
        16
    );
    for visible in ["#00011:01", "#00051:0101"] {
        let chart = parse(
            &format!("#WAV01 head\n{visible}\n#00031:01\n#000D1:01"),
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(
            (chart.notes.len(), chart.invisible.len(), chart.mines.len()),
            (1, 1, 1)
        );
        assert_eq!(chart.mines[0].lane, chart.notes[0].lane);
        assert_eq!(chart.mines[0].lane, chart.invisible[0].lane);
    }
    let duplicates = "#000D1:01\n#000D1:ZZ\n#000D1:00";
    let error = parse(duplicates, ParseOptions::default()).unwrap_err();
    assert_eq!(error.line, 2);
    assert!(matches!(error.kind, BmsErrorKind::Duplicate(_)));
    let chart = parse(duplicates, last_wins()).unwrap();
    assert_eq!(
        (
            chart.mines.len(),
            chart.mines[0].damage.raw(),
            chart.mines[0].ordinal,
            chart.mines[0].line
        ),
        (1, 1295, 1, 2)
    );
    let branches = "#RANDOM 2\n#IF 1\n#000D1:1E\n#BASE 16\n#ELSE\n#000E9:ZZ\n#BASE 62\n#ENDIF";
    for (seed, lane, raw, line) in [(3, 0x11, 50, 3), (0, 0x29, 1295, 6)] {
        let chart = parse_seeded(branches, ParseOptions::default(), seed).unwrap();
        assert_eq!(
            chart,
            parse_seeded(branches, ParseOptions::default(), seed).unwrap()
        );
        assert_eq!(chart.mines.len(), 1);
        assert_eq!(
            (
                chart.mines[0].lane.channel(),
                chart.mines[0].damage.raw(),
                chart.mines[0].line
            ),
            (lane, raw, line)
        );
    }
    let discarded = "#SETRANDOM 1\n#IF 2\n#BASE invalid\n#000D1:INVALID_BUT_DISCARDED\n#ELSE\n#000E9:01\n#ENDIF";
    assert_eq!(
        parse(discarded, ParseOptions::default()).unwrap().mines[0].line,
        6
    );
    for options in [
        ParseOptions {
            max_lines: 3,
            ..ParseOptions::default()
        },
        ParseOptions {
            max_bytes: discarded.len() - 1,
            ..ParseOptions::default()
        },
        ParseOptions {
            max_line_bytes: 20,
            ..ParseOptions::default()
        },
    ] {
        assert!(matches!(
            parse(discarded, options).unwrap_err().kind,
            BmsErrorKind::Limit(_)
        ));
    }
    for (text, line) in [
        ("#000D1:Z", 1),
        ("; gap\n#000E9:$1", 2),
        ("#BASE 16\n; gap\n#000D3:0/", 3),
    ] {
        let error = parse(text, ParseOptions::default()).unwrap_err();
        assert_eq!(error.line, line);
        assert!(matches!(error.kind, BmsErrorKind::Syntax(_)));
    }
    let rest = parse("#000D1:00000000000000\n#000E9:00", ParseOptions::default()).unwrap();
    assert!(rest.mines.is_empty() && rest.samples.is_empty());
    assert_eq!(rest.mine_ticks_per_beat, rest.source.ticks_per_beat);
    assert!(rest.compile_mines().unwrap().is_empty());
}

#[test]
fn separate_mine_grid_uses_pre_stop_core_time_without_changing_judge_bgm_bga_or_invisible_identity()
{
    let make = |a: &str, b: &str, c: &str| {
        format!(
            "#BPM 120\n#BPM01 240\n#STOP01 48\n#WAV01 head\n#00002:0.75\n#00008:000100\n#00009:000100\n#00011:000001\n{a}\n#00001:000100\n#00004:0001000000\n#00031:00010000000000\n{b}\n{c}\n#00111:00"
        )
    };
    let plain = parse(
        &make("; reserved", "; reserved", "; reserved"),
        ParseOptions::default(),
    )
    .unwrap();
    let fractional = format!("#000D2:0001{}", "00".repeat(9)); // 11 cells: 3/11 quarter-beats.
    let chart = parse(
        &make("#000D1:011EZZ", "#001E1:01", &fractional),
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(chart.source, plain.source);
    assert_eq!(chart.notes, plain.notes);
    assert_eq!(chart.bgm, plain.bgm);
    assert_eq!(chart.bga, plain.bga);
    assert_eq!(chart.invisible, plain.invisible);
    assert_eq!(chart.measures, plain.measures);
    assert_eq!(chart.metadata, plain.metadata);
    assert_eq!(chart.samples, plain.samples);
    assert_eq!(
        (
            chart.source.ticks_per_beat,
            chart.bga_ticks_per_beat,
            chart.invisible_ticks_per_beat,
            chart.mine_ticks_per_beat
        ),
        (1, 5, 7, 11)
    );
    assert_eq!(chart.compile().unwrap(), plain.compile().unwrap());
    assert_eq!(
        chart.compile_invisible().unwrap(),
        plain.compile_invisible().unwrap()
    );
    assert_eq!(fingerprint(&chart), fingerprint(&plain));
    assert_eq!(chart.source.stops[0].duration.as_nanos(), 250_000_000);
    assert_eq!(chart.compile().unwrap().bgm[0].at.as_nanos(), 500_000_000);
    assert_eq!(chart.compile().unwrap().bga[0].at.as_nanos(), 300_000_000);
    assert_eq!(
        chart.compile_invisible().unwrap()[0].at.as_nanos(),
        214_285_714
    );
    assert_eq!(
        chart.compile().unwrap().chart.objects()[0]
            .time
            .start
            .as_nanos(),
        1_000_000_000
    );
    assert_eq!(
        chart
            .mines
            .iter()
            .map(|m| (m.beat.ticks(), m.lane.channel(), m.ordinal, m.line))
            .collect::<Vec<_>>(),
        [
            (0, 0x11, 0, 9),
            (3, 0x12, 4, 14),
            (11, 0x11, 1, 9),
            (22, 0x11, 2, 9),
            (33, 0x21, 3, 13)
        ]
    );
    assert_eq!(
        chart
            .compile_mines()
            .unwrap()
            .iter()
            .map(|m| (
                m.at.as_nanos(),
                m.lane.channel(),
                m.damage.raw(),
                m.ordinal,
                m.line
            ))
            .collect::<Vec<_>>(),
        [
            (0, 0x11, 1, 0, 9),
            (136_363_636, 0x12, 1, 4, 14),
            (500_000_000, 0x11, 50, 1, 9),
            (1_000_000_000, 0x11, 1295, 2, 9),
            (1_250_000_000, 0x21, 1, 3, 13)
        ]
    );
    let simultaneous = parse(
        "#BPM 60\n#STOP01 48\n#00009:01\n#000E9:ZZ\n#000D1:1E",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        simultaneous
            .compile_mines()
            .unwrap()
            .iter()
            .map(|m| (m.at.as_nanos(), m.lane.channel(), m.ordinal))
            .collect::<Vec<_>>(),
        [(0, 0x29, 0), (0, 0x11, 1)]
    );
}

#[test]
fn raw_and_combined_caps_and_fabricated_grid_lane_position_and_timing_fail_without_mutation() {
    let combined = "#WAV01 x\n#00011:01\n#00031:01\n#000D1:01";
    assert!(
        parse(
            combined,
            ParseOptions {
                max_objects: 2,
                ..ParseOptions::default()
            }
        )
        .is_err()
    );
    assert!(
        parse(
            combined,
            ParseOptions {
                max_objects: 3,
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    assert!(
        parse(
            "#000D1:01\n#000D1:ZZ",
            ParseOptions {
                max_objects: 1,
                ..last_wins()
            }
        )
        .is_err(),
        "raw tokens count before replacement"
    );
    let resolution = parse(
        "#000D1:0001000000000000000000",
        ParseOptions {
            max_resolution: 10,
            ..ParseOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(resolution.line, 1);
    assert_eq!(resolution.kind, BmsErrorKind::Resolution);
    let original = parse(combined, ParseOptions::default()).unwrap();
    let expected = original.compile_mines().unwrap();
    for (source_grid, mine_grid) in [(0, 1), (1, 0), (2, 3)] {
        let mut invalid = original.clone();
        invalid.source.ticks_per_beat = source_grid;
        invalid.mine_ticks_per_beat = mine_grid;
        assert_eq!(
            invalid.compile_mines().unwrap_err().kind,
            BmsErrorKind::Resolution
        );
    }
    for lane in [0, 0x10, 0x1a, 0x31, 0xd1] {
        let mut invalid = original.clone();
        invalid.mines[0].lane = BmsLane(lane);
        let error = invalid.compile_mines().unwrap_err();
        assert_eq!(error.line, 4);
        assert!(matches!(error.kind, BmsErrorKind::Syntax(_)));
    }
    for same_position in [false, true] {
        let mut invalid = original.clone();
        let mut second = invalid.mines[0];
        second.line = 42;
        if same_position {
            second.ordinal = 1;
        } else {
            second.beat = Beat::new(1).unwrap();
        }
        invalid.mines.push(second);
        let error = invalid.compile_mines().unwrap_err();
        assert_eq!(error.line, 42);
        assert!(matches!(error.kind, BmsErrorKind::Duplicate(_)));
    }
    let mut bpm = original.clone();
    bpm.mine_ticks_per_beat = 2;
    bpm.source.bpm_changes.push(BpmChange {
        beat: Beat::new(i64::MAX).unwrap(),
        bpm: Bpm::new(120, 1).unwrap(),
    });
    assert_eq!(
        bpm.compile_mines().unwrap_err().kind,
        BmsErrorKind::Overflow
    );
    let mut stop = original.clone();
    stop.mine_ticks_per_beat = 2;
    stop.source.stops.push(Stop {
        beat: Beat::new(i64::MAX).unwrap(),
        duration: Duration::from_nanos(1),
    });
    assert_eq!(
        stop.compile_mines().unwrap_err().kind,
        BmsErrorKind::Overflow
    );
    let mut oversized = original.clone();
    oversized.mines.resize(MAX_SOURCE_ITEMS, oversized.mines[0]);
    assert!(matches!(
        oversized.compile_mines().unwrap_err().kind,
        BmsErrorKind::Limit(_)
    ));
    assert!(matches!(
        oversized.compile_invisible().unwrap_err().kind,
        BmsErrorKind::Limit(_)
    ));
    assert_eq!(original.compile_mines().unwrap(), expected);
}

#[test]
fn bounded_long_duration_sources_preserve_integer_nanoseconds_and_refuse_unrepresentable_timing() {
    for (text, at, raw) in [
        ("#BPM 1\n#00002:301\n#001D1:1E", 72_240_000_000_000_i64, 50),
        ("#BPM 1\n#00002:2520\n#001D1:01", 604_800_000_000_000, 1),
        (
            "#BPM 0.5\n#00002:2520\n#001E9:ZZ",
            1_209_600_000_000_000,
            1295,
        ),
    ] {
        let chart = parse(text, ParseOptions::default()).unwrap();
        let scheduled = chart.compile_mines().unwrap();
        assert_eq!(scheduled.len(), 1);
        assert_eq!(
            (
                scheduled[0].at.as_nanos(),
                scheduled[0].damage.raw(),
                scheduled[0].line
            ),
            (at, raw, 3)
        );
        assert!(chart.compile().unwrap().chart.objects().is_empty());
    }
    let tiny_bpm = parse("#BPM 0.000001\n#999D1:01", ParseOptions::default()).unwrap();
    assert!(tiny_bpm.compile_mines().is_err());
    let mut boundary = parse("#BPM 60\n#000D1:01", ParseOptions::default()).unwrap();
    boundary.mines[0].beat = Beat::new(9_223_372_036).unwrap();
    assert_eq!(
        boundary.compile_mines().unwrap()[0].at.as_nanos(),
        9_223_372_036_000_000_000
    );
    boundary.mines[0].beat = Beat::new(9_223_372_037).unwrap();
    assert!(boundary.compile_mines().is_err());
    boundary.mines[0].beat = Beat::new(i64::MAX).unwrap();
    assert!(boundary.compile_mines().is_err());
}
