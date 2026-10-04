//! Deferred invisible timeline fixtures; literal resource, lane and song-time expectations.
use beatkernel::{
    audio::SampleId,
    chart::{Beat, Bpm, BpmChange, MAX_SOURCE_ITEMS, Stop},
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    time::Duration,
};
use beatkernel_bms::{
    parse, parse_seeded, BgmEvent, BmsChart, BmsErrorKind, DuplicatePolicy, ParseOptions,
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
fn all_eighteen_invisible_lanes_keep_resource_radix_and_rest_semantics_without_gameplay_objects() {
    for (base, token, sample) in [
        ("", "zZ", 1295),
        ("#BASE 16\n", "fF", 255),
        ("#BASE 36\n", "zz", 1295),
        ("#BASE 62\n", "zz", 3843),
    ] {
        let mut text = format!("#BPM 60\n#WaV{token} selected.wav\n");
        for channel in [
            0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x41, 0x42, 0x43, 0x44, 0x45,
            0x46, 0x47, 0x48, 0x49,
        ] {
            writeln!(text, "#000{channel:02X}:00{token}00").unwrap();
        }
        text.push_str(base); // A selected late BASE also governs invisible tokens.
        let parsed = parse(&text, ParseOptions::default()).unwrap();
        assert_eq!(parsed.source.ticks_per_beat, 1);
        assert_eq!(parsed.invisible_ticks_per_beat, 3);
        assert!(
            parsed.source.objects.is_empty() && parsed.notes.is_empty() && parsed.bgm.is_empty()
        );
        assert!(parsed.rules().is_empty() && parsed.compile().unwrap().chart.objects().is_empty());
        assert_eq!(
            parsed
                .invisible
                .iter()
                .map(|event| event.lane.channel())
                .collect::<Vec<_>>(),
            [
                0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x21, 0x22, 0x23, 0x24, 0x25,
                0x26, 0x27, 0x28, 0x29
            ]
        );
        for (index, event) in parsed.invisible.iter().enumerate() {
            assert_eq!(
                (
                    event.beat.ticks(),
                    event.sample.0,
                    event.ordinal,
                    event.line
                ),
                (4, sample, index as u64, index + 3)
            );
        }
        let scheduled = parsed.compile_invisible().unwrap();
        assert_eq!(scheduled.len(), 18);
        for (index, event) in scheduled.iter().enumerate() {
            assert_eq!(event.at.as_nanos(), 1_333_333_333);
            assert_eq!(
                (event.lane, event.sample, event.ordinal, event.line),
                (
                    parsed.invisible[index].lane,
                    SampleId(sample),
                    index as u64,
                    index + 3
                )
            );
        }
    }
    let cases = parse(
        "#wav0A upper\n#WaV0a lower\n#00031:0A0a\n#base 62",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        cases
            .invisible
            .iter()
            .map(|event| (event.sample.0, event.beat.ticks()))
            .collect::<Vec<_>>(),
        [(10, 0), (36, 2)]
    );
    for visible in ["#00011:01", "#00051:0101"] {
        let parsed = parse(
            &format!("#WAV01 head\n{visible}\n#00031:01\n#00031:00"),
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!((parsed.notes.len(), parsed.invisible.len()), (1, 1));
        assert_eq!(parsed.notes[0].lane, parsed.invisible[0].lane);
    }
    let rest = parse("#00031:000000\n#00049:00", ParseOptions::default()).unwrap();
    assert!(rest.samples.is_empty() && rest.invisible.is_empty());
    assert_eq!(rest.invisible_ticks_per_beat, rest.source.ticks_per_beat);
    assert!(rest.compile_invisible().unwrap().is_empty());
}

#[test]
fn independent_grid_preserves_real_judge_identity_and_uses_variable_measure_bpm_and_pre_stop_time()
{
    let make = |a: &str, b: &str, c: &str| {
        format!(
            "#BPM 120\n#BPM01 240\n#STOP01 48\n#WAV01 head\n\
        #00002:0.75\n#00008:000100\n#00009:000100\n#00011:000001\n{a}\n\
        #00001:000100\n#00004:0001000000\n{b}\n{c}\n#00111:00"
        )
    };
    let plain = parse(&make("; slot", "; slot", "; slot"), ParseOptions::default()).unwrap();
    let parsed = parse(
        &make("#00031:010101", "#00141:01", "#00032:00010000000000"),
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(parsed.source, plain.source);
    assert_eq!(parsed.notes, plain.notes);
    assert_eq!(parsed.bgm, plain.bgm);
    assert_eq!(parsed.bga, plain.bga);
    assert_eq!(parsed.measures, plain.measures);
    assert_eq!(parsed.metadata, plain.metadata);
    assert_eq!(
        (
            parsed.source.ticks_per_beat,
            parsed.bga_ticks_per_beat,
            parsed.invisible_ticks_per_beat
        ),
        (1, 5, 7)
    );
    assert_eq!(parsed.compile().unwrap(), plain.compile().unwrap());
    assert_eq!(fingerprint(&parsed), fingerprint(&plain));
    assert_eq!(parsed.source.stops[0].duration.as_nanos(), 250_000_000);
    assert_eq!(parsed.compile().unwrap().bgm[0].at.as_nanos(), 500_000_000);
    assert_eq!(parsed.compile().unwrap().bga[0].at.as_nanos(), 300_000_000);
    assert_eq!(
        parsed.compile().unwrap().chart.objects()[0]
            .time
            .start
            .as_nanos(),
        1_000_000_000
    );
    assert_eq!(
        parsed
            .invisible
            .iter()
            .map(|event| (
                event.beat.ticks(),
                event.lane.channel(),
                event.ordinal,
                event.line
            ))
            .collect::<Vec<_>>(),
        [
            (0, 0x11, 0, 9),
            (3, 0x12, 4, 13),
            (7, 0x11, 1, 9),
            (14, 0x11, 2, 9),
            (21, 0x21, 3, 12)
        ]
    );
    assert_eq!(
        parsed
            .compile_invisible()
            .unwrap()
            .iter()
            .map(|event| (
                event.at.as_nanos(),
                event.lane.channel(),
                event.sample.0,
                event.ordinal,
                event.line
            ))
            .collect::<Vec<_>>(),
        [
            (0, 0x11, 1, 0, 9),
            (214_285_714, 0x12, 1, 4, 13),
            (500_000_000, 0x11, 1, 1, 9),
            (1_000_000_000, 0x11, 1, 2, 9),
            (1_250_000_000, 0x21, 1, 3, 12)
        ]
    );
    let simultaneous = parse(
        "#BPM 60\n#STOP01 48\n#WAV01 x\n#00009:01\n#00049:01\n#00031:01",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        simultaneous
            .compile_invisible()
            .unwrap()
            .iter()
            .map(|event| (event.at.as_nanos(), event.lane.channel(), event.ordinal))
            .collect::<Vec<_>>(),
        [(0, 0x29, 0), (0, 0x11, 1)]
    );
}

#[test]
fn selection_duplicate_policy_and_physical_limits_apply_before_invisible_timeline_publication() {
    let branches = "#RANDOM 2\n#IF 1\n#WAV0a first\n#00031:0a\n#BASE 62\n#ELSE\n#WAVZZ second\n#00049:ZZ\n#BASE 36\n#ENDIF";
    for (seed, sample, lane, line) in [(3, 36, 0x11, 4), (0, 1295, 0x29, 8)] {
        let chart = parse_seeded(branches, ParseOptions::default(), seed).unwrap();
        assert_eq!(
            chart,
            parse_seeded(branches, ParseOptions::default(), seed).unwrap()
        );
        assert_eq!(chart.invisible.len(), 1);
        assert_eq!(
            (
                chart.invisible[0].sample.0,
                chart.invisible[0].lane.channel(),
                chart.invisible[0].line
            ),
            (sample, lane, line)
        );
    }
    let discarded = "#SETRANDOM 1\n#IF 2\n#BASE invalid\n#00031:NOT_PAIRS\n#ELSE\n#WAV01 selected\n#00031:01\n#ENDIF";
    assert_eq!(
        parse(discarded, ParseOptions::default()).unwrap().invisible[0].line,
        7
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
            max_line_bytes: 15,
            ..ParseOptions::default()
        },
    ] {
        assert!(matches!(
            parse(discarded, options).unwrap_err().kind,
            BmsErrorKind::Limit(_)
        ));
    }
    let duplicates = "#WAV01 first\n#WAV02 second\n#00031:01\n#00031:02\n#00031:00";
    let error = parse(duplicates, ParseOptions::default()).unwrap_err();
    assert_eq!(error.line, 4);
    assert!(matches!(error.kind, BmsErrorKind::Duplicate(_)));
    let chart = parse(duplicates, last_wins()).unwrap();
    assert_eq!(
        (
            chart.invisible.len(),
            chart.invisible[0].sample.0,
            chart.invisible[0].ordinal,
            chart.invisible[0].line
        ),
        (1, 2, 1, 4)
    );
    let missing = parse("#00031:01", ParseOptions::default()).unwrap_err();
    assert_eq!(
        (missing.line, missing.kind),
        (
            1,
            BmsErrorKind::MissingDefinition {
                kind: "WAV",
                index: 1
            }
        )
    );
    for (text, line) in [
        ("#BASE 16\n#00031:0G", 2),
        ("#WAV01 x\n#00041:0", 2),
        ("#WAV01 x\n; physical gap\n#00039:$1", 3),
    ] {
        let error = parse(text, ParseOptions::default()).unwrap_err();
        assert_eq!(error.line, line);
        assert!(matches!(error.kind, BmsErrorKind::Syntax(_)));
    }
    let combined = "#WAV01 x\n#00011:01\n#00031:01";
    assert!(
        parse(
            combined,
            ParseOptions {
                max_objects: 1,
                ..ParseOptions::default()
            }
        )
        .is_err()
    );
    assert!(
        parse(
            combined,
            ParseOptions {
                max_objects: 2,
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    let resolution = parse(
        "#WAV01 x\n#00031:00010000000000",
        ParseOptions {
            max_resolution: 6,
            ..ParseOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(resolution.line, 2);
    assert_eq!(resolution.kind, BmsErrorKind::Resolution);
}

#[test]
fn fabricated_invisible_grids_samples_positions_and_joint_source_budgets_refuse_without_partial_compilation()
 {
    let original = parse(
        "#BPM 60\n#WAV01 x\n#00031:01\n#00011:0001",
        ParseOptions::default(),
    )
    .unwrap();
    let expected = original.compile_invisible().unwrap();
    let mut zero = original.clone();
    zero.invisible_ticks_per_beat = 0;
    assert!(matches!(
        zero.compile_invisible().unwrap_err().kind,
        BmsErrorKind::Resolution
    ));
    let mut fractional = original.clone();
    fractional.source.ticks_per_beat = 2;
    fractional.invisible_ticks_per_beat = 3;
    assert!(matches!(
        fractional.compile_invisible().unwrap_err().kind,
        BmsErrorKind::Resolution
    ));
    for sample in [0, 2, 3844, u64::MAX] {
        let mut chart = original.clone();
        chart.invisible[0].sample = SampleId(sample);
        assert!(chart.compile_invisible().is_err());
    }
    let mut duplicate = original.clone();
    duplicate.invisible.push(duplicate.invisible[0]);
    assert!(duplicate.compile_invisible().is_err());
    let mut duplicate_ordinal = original.clone();
    let mut later = duplicate_ordinal.invisible[0];
    later.beat = Beat::new(1).unwrap();
    duplicate_ordinal.invisible.push(later);
    assert!(duplicate_ordinal.compile_invisible().is_err());
    let mut bpm_overflow = original.clone();
    bpm_overflow.invisible_ticks_per_beat = 2;
    bpm_overflow.source.bpm_changes.push(BpmChange {
        beat: Beat::new(i64::MAX).unwrap(),
        bpm: Bpm::new(120, 1).unwrap(),
    });
    assert!(bpm_overflow.compile_invisible().is_err());
    let mut stop_overflow = original.clone();
    stop_overflow.invisible_ticks_per_beat = 2;
    stop_overflow.source.stops.push(Stop {
        beat: Beat::new(i64::MAX).unwrap(),
        duration: Duration::from_nanos(1),
    });
    assert!(stop_overflow.compile_invisible().is_err());
    let mut song_overflow = original.clone();
    song_overflow.invisible[0].beat = Beat::new(i64::MAX).unwrap();
    assert!(song_overflow.compile_invisible().is_err());
    let mut oversized = original.clone();
    oversized.bgm.resize(
        MAX_SOURCE_ITEMS,
        BgmEvent {
            beat: Beat::new(0).unwrap(),
            sample: SampleId(1),
            ordinal: 0,
        },
    );
    assert!(matches!(
        oversized.compile_invisible().unwrap_err().kind,
        BmsErrorKind::Limit(_)
    ));
    assert_eq!(original.compile_invisible().unwrap(), expected);
}
