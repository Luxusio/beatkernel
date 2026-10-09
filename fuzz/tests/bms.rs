use beatkernel::chart::ChartError;
use beatkernel_bms::{parse_seeded, BgaChannel, BmsErrorKind, BmsRank, DuplicatePolicy, ImageId};
use beatkernel_layer_fuzz::bms::{check_bms, parser_options, BMS_MAX_BYTES};

const TIMED: &str = "#BPM 60\n#BPM01 120\n#STOP01 48\n#WAV01 head.wav\n#WAV02 other.wav\n#00008:00010000\n#00009:00010000\n#00011:01010101\n#00052:000100FF\n#00001:00010200\n#00004:00010200\n#00006:00010000\n#00007:00000200\n#0000A:00000003\n#0000B:00018000\n#0000C:00004000\n#0000D:000000FF\n#0000E:00010000\n#00031:00010200\n#000D2:0001ZZ00";

#[test]
fn campaign_limits_are_explicit_parser_limits() {
    let options = parser_options();
    assert_eq!(BMS_MAX_BYTES, 16_384);
    assert_eq!(options.max_bytes, BMS_MAX_BYTES);
    assert_eq!(options.max_lines, 256);
    assert_eq!(options.max_line_bytes, 1024);
    assert_eq!(options.max_objects, 256);
    assert_eq!(options.max_resolution, 65_536);
    assert_eq!(options.duplicates, DuplicatePolicy::Reject);
}

#[test]
fn piecewise_tempo_stop_and_long_note_have_literal_song_times() {
    // One beat at 60 BPM, then 120 BPM; STOP48 is one beat at the new BPM.
    // Objects at the STOP beat precede the pause; the unsounded FF tail is legal.
    for seed in [0, 73] {
        let parsed = parse_seeded(TIMED, parser_options(), seed).unwrap();
        assert_eq!(parsed.source.stops[0].duration.as_nanos(), 500_000_000);
        let compiled = parsed.compile().unwrap();
        let objects: Vec<_> = compiled
            .chart
            .objects()
            .iter()
            .map(|object| {
                (
                    object.time.start.as_nanos(),
                    object.time.end.map(|end| end.as_nanos()),
                )
            })
            .collect();
        assert_eq!(
            objects,
            [
                (0, None),
                (1_000_000_000, None),
                (1_000_000_000, Some(2_500_000_000)),
                (2_000_000_000, None),
                (2_500_000_000, None)
            ]
        );
        let long = parsed
            .notes
            .iter()
            .find(|note| note.tail_sample.is_some())
            .unwrap();
        assert_eq!(long.lane.channel(), 0x12);
        assert_eq!(long.sample.0, 1);
        assert_eq!(long.tail_sample.unwrap().0, 555);
        assert!(!parsed.samples.contains_key(&555));
        assert_eq!(
            compiled
                .bgm
                .iter()
                .map(|event| (event.at.as_nanos(), event.sample.0))
                .collect::<Vec<_>>(),
            [(1_000_000_000, 1), (2_000_000_000, 2)]
        );
        assert!(check_bms(TIMED.as_bytes()));
    }
}

#[test]
fn image_and_opacity_schedules_preserve_all_four_roles_and_raw_bytes() {
    let parsed = parse_seeded(TIMED, parser_options(), 73).unwrap();
    let compiled = parsed.compile().unwrap();
    assert_eq!(
        compiled
            .bga
            .iter()
            .map(|event| (event.at.as_nanos(), event.channel, event.image))
            .collect::<Vec<_>>(),
        [
            (1_000_000_000, BgaChannel::Base, ImageId(1)),
            (1_000_000_000, BgaChannel::Poor, ImageId(1)),
            (2_000_000_000, BgaChannel::Base, ImageId(2)),
            (2_000_000_000, BgaChannel::Layer, ImageId(2)),
            (2_500_000_000, BgaChannel::Layer2, ImageId(3)),
        ]
    );
    assert_eq!(
        compiled
            .bga_opacity
            .iter()
            .map(|event| (event.at.as_nanos(), event.channel, event.alpha))
            .collect::<Vec<_>>(),
        [
            (1_000_000_000, BgaChannel::Base, 1),
            (1_000_000_000, BgaChannel::Poor, 1),
            (2_000_000_000, BgaChannel::Base, 128),
            (2_000_000_000, BgaChannel::Layer, 64),
            (2_500_000_000, BgaChannel::Layer2, 255),
        ]
    );
    assert!(check_bms(TIMED.as_bytes()));
}

#[test]
fn separate_invisible_and_mine_outputs_have_known_timing_and_provenance() {
    let parsed = parse_seeded(TIMED, parser_options(), 0).unwrap();
    let before = parsed.clone();
    assert_eq!(
        parsed
            .compile_invisible()
            .unwrap()
            .iter()
            .map(|event| (
                event.at.as_nanos(),
                event.lane.channel(),
                event.sample.0,
                event.line
            ))
            .collect::<Vec<_>>(),
        [(1_000_000_000, 0x11, 1, 19), (2_000_000_000, 0x11, 2, 19),]
    );
    let mines = parsed.compile_mines().unwrap();
    assert_eq!(
        mines
            .iter()
            .map(|event| (
                event.at.as_nanos(),
                event.lane.channel(),
                event.damage.raw(),
                event.line
            ))
            .collect::<Vec<_>>(),
        [
            (1_000_000_000, 0x12, 1, 20),
            (2_000_000_000, 0x12, 1295, 20),
        ]
    );
    assert_eq!(mines[0].damage.half_percent_units(), Some(1));
    assert!(mines[1].damage.is_fatal());
    assert_eq!(parsed, before);
    assert!(check_bms(TIMED.as_bytes()));
}

#[test]
fn frozen_seeds_select_different_physical_branches() {
    // SplitMix64 first draws are e220a8397b1dcdaf and d08f003850439a4b;
    // scaling by six selects literal branches six and five respectively.
    let text = "#RANDOM 6\n#IF 6\n#TITLE six\n#WAV01 six.wav\n#00011:01\n#ELSEIF 5\n#TITLE five\n#WAV02 five.wav\n#00012:02\n#ELSE\n#BPM malformed\n#ENDIF\n#ENDRANDOM";
    for (seed, title, sample, lane, line) in [(0, "six", 1, 0x11, 5), (73, "five", 2, 0x12, 9)] {
        let parsed = parse_seeded(text, parser_options(), seed).unwrap();
        assert_eq!(parsed.metadata["TITLE"], title);
        assert_eq!(
            (
                parsed.notes[0].sample.0,
                parsed.notes[0].lane.channel(),
                parsed.notes[0].line
            ),
            (sample, lane, line)
        );
        assert_eq!(
            parsed.compile().unwrap().chart.objects()[0]
                .time
                .start
                .as_nanos(),
            0
        );
    }
    assert!(check_bms(text.as_bytes()));
    // A primary error for seed0 must not hide seed73's accepted branch.
    let mixed = "#RANDOM 6\n#IF 6\n#BPM malformed\n#ELSE\n#BPM 120\n#ENDIF";
    assert!(parse_seeded(mixed, parser_options(), 0).is_err());
    assert!(parse_seeded(mixed, parser_options(), 73).is_ok());
    assert!(check_bms(mixed.as_bytes()));
}

#[test]
fn late_base62_and_rank_metadata_preserve_opaque_source_values() {
    let text = "#BPM 120\n#WAV0A Upper/音.wav\n#WAV0a lower.wav\n#00011:0A0a\n#RANK +0004\n#DEFEXRANK 87.5\n#BASE 62";
    let parsed = parse_seeded(text, parser_options(), 73).unwrap();
    assert_eq!(
        parsed
            .notes
            .iter()
            .map(|note| note.sample.0)
            .collect::<Vec<_>>(),
        [10, 36]
    );
    assert_eq!(parsed.samples[&10], "Upper/音.wav");
    let ranks = parsed.judge_rank_metadata().unwrap();
    assert_eq!(ranks.rank(), Some(BmsRank::VeryEasy));
    let percentage = ranks.defexrank().unwrap();
    assert_eq!((percentage.numerator(), percentage.denominator()), (175, 2));
    assert_eq!(parsed.metadata["RANK"], "+0004");
    assert_eq!(
        parsed.compile().unwrap().chart.objects()[1]
            .time
            .start
            .as_nanos(),
        1_000_000_000
    );
    assert!(check_bms(text.as_bytes()));
}

#[test]
fn strict_utf8_and_malformed_primary_inputs_are_rejected() {
    for bytes in [&b"#TITLE \xff"[..], &b"\xc0\xaf"[..], &b"\xed\xa0\x80"[..]] {
        assert!(std::str::from_utf8(bytes).is_err());
        assert!(!check_bms(bytes));
    }
    for text in [
        "#BPM 0",
        "#WAV01 a\n#WAV01 b",
        "#00011:01",
        "#BASE 37",
        "#WAV01 a\n#00051:01",
        "#DEFEXRANK NaN",
        "#IF 1",
    ] {
        for seed in [0, 73] {
            assert!(
                parse_seeded(text, parser_options(), seed).is_err(),
                "{text}"
            );
        }
        assert!(!check_bms(text.as_bytes()), "{text}");
    }
    assert!(check_bms("\u{feff}#TITLE 音楽\n#BPM 120".as_bytes()));
}

#[test]
fn exact_byte_line_and_object_caps_admit_boundaries_and_refuse_excess() {
    let byte_boundary = (";".to_owned() + &"x".repeat(1022) + "\n").repeat(16);
    assert_eq!(byte_boundary.len(), BMS_MAX_BYTES);
    assert!(check_bms(byte_boundary.as_bytes()));
    assert!(!check_bms(&(byte_boundary + "x").into_bytes()));
    for (accepted, rejected) in [
        (";\n".repeat(256), ";\n".repeat(257)),
        (
            format!(";{}", "x".repeat(1023)),
            format!(";{}", "x".repeat(1024)),
        ),
        (
            format!(
                "#WAV01 a\n#00001:{}\n#00101:{}",
                "01".repeat(128),
                "01".repeat(128)
            ),
            format!(
                "#WAV01 a\n#00001:{}\n#00101:{}",
                "01".repeat(128),
                "01".repeat(129)
            ),
        ),
    ] {
        assert!(parse_seeded(&accepted, parser_options(), 0).is_ok());
        assert!(check_bms(accepted.as_bytes()));
        let error = parse_seeded(&rejected, parser_options(), 0).unwrap_err();
        assert!(matches!(error.kind, BmsErrorKind::Limit(_)), "{error:?}");
        assert!(!check_bms(rejected.as_bytes()));
    }
}

#[test]
fn exact_grid_resolution_refuses_rounding_even_below_other_caps() {
    let make = |cells: usize| {
        format!(
            "#WAV01 a\n#00011:0001{}\n#00012:0001{}",
            "00".repeat(249),
            "00".repeat(cells - 2)
        )
    };
    let accepted = make(257);
    assert_eq!(
        parse_seeded(&accepted, parser_options(), 0)
            .unwrap()
            .source
            .ticks_per_beat,
        64_507
    );
    assert!(check_bms(accepted.as_bytes()));
    let rejected = make(263);
    assert!(matches!(
        parse_seeded(&rejected, parser_options(), 73)
            .unwrap_err()
            .kind,
        BmsErrorKind::Resolution
    ));
    assert!(!check_bms(rejected.as_bytes()));
}

#[test]
fn accepted_text_with_timestamp_overflow_remains_a_normal_compiler_error() {
    // 3996 quarter beats at 1/1e9 BPM exceed signed nanosecond song time.
    let text = "#BPM .000000001\n#WAV01 distant.wav\n#99911:01";
    for seed in [0, 73] {
        let parsed = parse_seeded(text, parser_options(), seed).unwrap();
        assert_eq!(parsed.source.objects[0].start.ticks(), 3996);
        assert!(matches!(
            parsed.compile().unwrap_err().kind,
            BmsErrorKind::Compile(ChartError::Overflow)
        ));
    }
    assert!(check_bms(text.as_bytes()));
}
