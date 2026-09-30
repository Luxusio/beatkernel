//! Finite seeded authoring corpus; no fuzz dependency, panic suppression or IO.
use beatkernel::{chart::*, time::Duration};
use beatkernel_bms::{parse, BmsErrorKind, ParseOptions};

const OPTIONS: ParseOptions = ParseOptions {
    max_bytes: 4096,
    max_lines: 128,
    max_line_bytes: 256,
    max_objects: 64,
    max_resolution: 4096,
    duplicates: beatkernel_bms::DuplicatePolicy::Reject,
};

struct Seed(u64);
impl Seed {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn valid_document(length: &str, padding: usize) -> String {
    // Subdivision expansion inserts only zeros, preserving every token position.
    let expand = |tokens: &[&str]| {
        tokens
            .iter()
            .map(|token| format!("{token}{}", "00".repeat(padding)))
            .collect::<String>()
    };
    format!(
        "#BPM 60\n#BPM01 120\n#STOP01 48\n#LNTYPE 1\n#WAV01 a.wav\n\
         #00002:{length}\n#00008:{}\n#00009:{}\n#00011:{}\n#00052:{}\n\
         #00001:{}\n#00001:{}\n#00111:01\n",
        expand(&["00", "01"]),
        expand(&["00", "01"]),
        expand(&["01", "01"]),
        expand(&["01", "01"]),
        expand(&["00", "01"]),
        expand(&["00", "01"]),
    )
}

#[test]
fn valid_timing_hold_and_bgm_variants_have_independent_piecewise_expectations() {
    let mut seed = Seed(0x719d_4a3b_1256_0fe1);
    for _ in 0..96 {
        let (length, midpoint_beats) = [("0.5", 1i64), ("1", 2), ("1.5", 3)][seed.below(3)];
        let document = valid_document(length, seed.below(8));
        let parsed = parse(&document, OPTIONS).unwrap();
        let compiled = parsed.compile().unwrap();
        let middle = midpoint_beats * 1_000_000_000;
        let next_measure = middle + 500_000_000 + midpoint_beats * 500_000_000;
        assert_eq!(compiled.chart.objects().len(), 4);
        let mut instant_times = Vec::new();
        let mut holds = 0;
        for object in compiled.chart.objects() {
            if let Some(end) = object.time.end {
                holds += 1;
                assert_eq!(object.time.start.as_nanos(), 0);
                assert_eq!(end.as_nanos(), middle); // Endpoints use pre-STOP time.
                assert_eq!(object.interaction, InteractionId(0x52));
            } else {
                instant_times.push(object.time.start.as_nanos());
            }
        }
        assert_eq!(holds, 1);
        assert_eq!(instant_times, vec![0, middle, next_measure]);
        assert_eq!(compiled.bgm.len(), 2);
        assert!(compiled
            .bgm
            .iter()
            .all(|event| event.at.as_nanos() == middle));
        assert!(compiled.bgm[0].ordinal < compiled.bgm[1].ordinal);
        assert_eq!(parsed.source.stops[0].duration.as_nanos(), 500_000_000);

        // A distinct syntactic/grid representation must preserve semantics.
        let alternate = format!(
            "#TITLE corpus 日本語\r\n{}",
            valid_document(length, 0).replace('\n', "\r\n")
        );
        let alternative = parse(&alternate, OPTIONS).unwrap().compile().unwrap();
        assert_eq!(alternative.chart.objects(), compiled.chart.objects());
        assert_eq!(alternative.bgm, compiled.bgm);
    }
}

fn bounded_parse_and_compile(text: &str) {
    assert!(text.len() <= OPTIONS.max_bytes);
    match parse(text, OPTIONS) {
        Ok(parsed) => {
            let source = &parsed.source;
            let total = source.objects.len()
                + source.bpm_changes.len()
                + source.stops.len()
                + source.scroll_changes.len()
                + parsed.bgm.len();
            assert!(total <= OPTIONS.max_objects);
            assert!(source.ticks_per_beat <= OPTIONS.max_resolution);
            assert!(parsed.measures.len() <= 1000); // BMS has exactly three measure digits.
            assert_eq!(parsed.notes.len(), source.objects.len());
            assert!(parsed.samples.len() <= OPTIONS.max_lines);
            assert!(parsed.metadata.len() <= OPTIONS.max_lines);
            assert!(parsed.warnings.len() <= OPTIONS.max_lines);
            match parsed.compile() {
                Ok(compiled) => {
                    assert_eq!(compiled.chart.objects().len(), source.objects.len());
                    assert_eq!(compiled.bgm.len(), parsed.bgm.len());
                    assert!(compiled.chart.objects().windows(2).all(|pair| {
                        (pair[0].time.start, pair[0].id) <= (pair[1].time.start, pair[1].id)
                    }));
                    for object in compiled.chart.objects() {
                        assert!(object.time.start.as_nanos() >= 0);
                        assert!(object.time.end.is_none_or(|end| end >= object.time.start));
                    }
                }
                Err(error) => {
                    assert_eq!(error.line, 0);
                    assert!(matches!(error.kind, BmsErrorKind::Compile(_)));
                }
            }
        }
        Err(error) => {
            assert!(error.line <= text.lines().count());
            assert!(!matches!(error.kind, BmsErrorKind::Compile(_)));
            assert!(error.to_string().len() <= text.len() + 256);
        }
    }
}

#[test]
fn seeded_arbitrary_bytes_and_valid_document_mutations_stay_bounded() {
    let mut seed = Seed(0x8e51_a02c_739b_6fd4);
    for _ in 0..256 {
        let length = seed.below(513);
        let bytes: Vec<_> = (0..length).map(|_| seed.next() as u8).collect();
        match std::str::from_utf8(&bytes) {
            Ok(text) => bounded_parse_and_compile(text),
            Err(error) => {
                assert!(error.valid_up_to() < bytes.len());
                let text = String::from_utf8_lossy(&bytes);
                assert!(text.len() <= bytes.len() * 3);
                bounded_parse_and_compile(&text);
            }
        }
    }
    for index in 0..256 {
        let mut bytes = valid_document(["0.5", "1", "1.5"][index % 3], index % 8).into_bytes();
        for _ in 0..=seed.below(8) {
            match seed.below(3) {
                0 if !bytes.is_empty() => {
                    let position = seed.below(bytes.len());
                    bytes[position] = seed.next() as u8;
                }
                1 if !bytes.is_empty() => {
                    let position = seed.below(bytes.len());
                    bytes.remove(position);
                }
                _ => {
                    let position = seed.below(bytes.len() + 1);
                    bytes.insert(position, seed.next() as u8);
                }
            }
        }
        assert!(bytes.len() < 1024);
        bounded_parse_and_compile(&String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn structured_parser_failures_have_selected_categories_and_lines() {
    let cases = [
        ("#BPM 60\n#00011:ZZ", 2, "missing"),
        ("#BPM 60\n#RANDOM 2", 2, "unsupported"),
        ("#WAV01 a.wav\n#00051:01", 2, "hold"),
        ("#WAV01 a.wav\n#00011:01\n#00011:01", 3, "duplicate"),
        ("#BPM 0", 1, "syntax"),
        ("#BPM 4294967296", 1, "overflow"),
        ("#00002:184467440737095516160000", 1, "precision"),
    ];
    for (document, line, kind) in cases {
        let error = parse(document, OPTIONS).unwrap_err();
        assert_eq!(error.line, line, "{document}");
        assert!(
            match kind {
                "missing" => matches!(
                    error.kind,
                    BmsErrorKind::MissingDefinition {
                        kind: "WAV",
                        index: 1295
                    }
                ),
                "unsupported" => matches!(error.kind, BmsErrorKind::Unsupported(_)),
                "hold" => matches!(error.kind, BmsErrorKind::LongNote(_)),
                "duplicate" => matches!(error.kind, BmsErrorKind::Duplicate(_)),
                "syntax" => matches!(error.kind, BmsErrorKind::Syntax(_)),
                "overflow" => matches!(error.kind, BmsErrorKind::Overflow),
                "precision" => matches!(error.kind, BmsErrorKind::Limit("decimal precision")),
                _ => unreachable!(),
            },
            "{error:?}"
        );
    }
    for (document, field) in [
        ("x".repeat(OPTIONS.max_bytes + 1), "input bytes"),
        ("\n".repeat(OPTIONS.max_lines + 1), "line count"),
        ("x".repeat(OPTIONS.max_line_bytes + 1), "line bytes"),
        (
            format!("#WAV01 a.wav\n#00011:{}", "01".repeat(65)),
            "objects",
        ),
    ] {
        let error = parse(&document, OPTIONS).unwrap_err();
        if field == "objects" {
            assert!(matches!(error.kind, BmsErrorKind::Limit(_)));
            assert_eq!(error.line, 2);
        } else {
            assert_eq!(error.kind, BmsErrorKind::Limit(field));
        }
    }
    let options = ParseOptions {
        max_resolution: 2,
        ..OPTIONS
    };
    assert_eq!(
        parse("#WAV01 a.wav\n#00011:000001", options)
            .unwrap_err()
            .kind,
        BmsErrorKind::Resolution
    );
}

fn object(id: u64, start: i64, end: Option<i64>) -> SourceObject {
    SourceObject {
        id: ObjectId(id),
        start: Beat::new(start).unwrap(),
        end: end.map(|tick| Beat::new(tick).unwrap()),
        interaction: InteractionId(1),
        visual: VisualId(2),
        audio: None,
        metadata: ObjectMetadata(vec![id as u8]),
    }
}

#[test]
fn seeded_core_subdivisions_bpm_stop_and_holds_obey_piecewise_time_and_grid_scaling() {
    let mut seed = Seed(0x69c2_0d4a_875e_13bf);
    for _ in 0..128 {
        let resolution = (seed.below(31) + 1) as u32;
        let bpm1 = [60u32, 120, 240][seed.below(3)];
        let bpm2 = [60u32, 120, 240][seed.below(3)];
        let stop = seed.below(1_000_001) as i64;
        let boundary = i64::from(resolution) * 2;
        let mut source = SourceChart::new(resolution, Bpm::new(bpm1, 1).unwrap()).unwrap();
        source.bpm_changes.push(BpmChange {
            beat: Beat::new(boundary).unwrap(),
            bpm: Bpm::new(bpm2, 1).unwrap(),
        });
        source.stops.push(Stop {
            beat: Beat::new(boundary).unwrap(),
            duration: Duration::from_nanos(stop),
        });
        for id in 0..16 {
            let tick = seed.below(resolution as usize * 4 + 1) as i64;
            let end = (id % 3 == 0).then_some(tick + i64::from(resolution));
            source.objects.push(object(id, tick, end));
        }
        let compiled = source.compile().unwrap();
        let expected = |tick: i64| {
            // BPM choices make the two-beat boundary exact; subdivision division
            // independently truncates within each constant-tempo segment.
            if tick <= boundary {
                i128::from(tick) * 60_000_000_000 / i128::from(resolution) / i128::from(bpm1)
            } else {
                120_000_000_000i128 / i128::from(bpm1)
                    + i128::from(stop)
                    + i128::from(tick - boundary) * 60_000_000_000
                        / i128::from(resolution)
                        / i128::from(bpm2)
            }
        };
        for timed in compiled.objects() {
            let original = &source.objects[timed.id.0 as usize];
            assert_eq!(
                i128::from(timed.time.start.as_nanos()),
                expected(original.start.ticks())
            );
            assert_eq!(
                timed.time.end.map(|time| i128::from(time.as_nanos())),
                original.end.map(|beat| expected(beat.ticks()))
            );
            assert_eq!(timed.metadata, original.metadata);
        }
        source.objects.reverse();
        assert_eq!(source.compile().unwrap(), compiled);
        source.ticks_per_beat *= 2;
        for object in &mut source.objects {
            object.start = Beat::new(object.start.ticks() * 2).unwrap();
            object.end = object.end.map(|beat| Beat::new(beat.ticks() * 2).unwrap());
        }
        source.bpm_changes[0].beat = Beat::new(boundary * 2).unwrap();
        source.stops[0].beat = Beat::new(boundary * 2).unwrap();
        assert_eq!(source.compile().unwrap().objects(), compiled.objects());
    }
}

#[test]
fn extreme_core_inputs_distinguish_representable_time_from_overflow_and_validation() {
    let mut source = SourceChart::new(u32::MAX, Bpm::new(u32::MAX, 1).unwrap()).unwrap();
    source.objects.push(object(1, i64::MAX, None));
    let expected =
        i128::from(i64::MAX) * 60_000_000_000 / i128::from(u32::MAX) / i128::from(u32::MAX);
    assert_eq!(
        i128::from(source.compile().unwrap().objects()[0].time.start.as_nanos()),
        expected
    );
    source.ticks_per_beat = 1;
    source.initial_bpm = Bpm::new(1, u32::MAX).unwrap();
    assert_eq!(source.compile(), Err(ChartError::Overflow));
    source.initial_bpm = Bpm::new(60, 1).unwrap();
    source.objects = vec![object(1, 1, None)];
    source.stops = vec![Stop {
        beat: Beat::new(0).unwrap(),
        duration: Duration::from_nanos(i64::MAX),
    }];
    assert_eq!(source.compile(), Err(ChartError::Overflow));
    source.stops.clear();
    source.objects = vec![object(1, 2, Some(1))];
    assert_eq!(
        source.compile(),
        Err(ChartError::ReversedRange { id: ObjectId(1) })
    );
    source.objects.clear();
    let marker = BpmChange {
        beat: Beat::new(1).unwrap(),
        bpm: Bpm::new(120, 1).unwrap(),
    };
    source.bpm_changes = vec![marker; 2];
    assert_eq!(
        source.compile(),
        Err(ChartError::DuplicateBpm {
            beat: Beat::new(1).unwrap()
        })
    );
    source.bpm_changes.clear();
    source.stops = vec![
        Stop {
            beat: Beat::new(1).unwrap(),
            duration: Duration::from_nanos(0)
        };
        2
    ];
    assert_eq!(
        source.compile(),
        Err(ChartError::DuplicateStop {
            beat: Beat::new(1).unwrap()
        })
    );
}
