use beatkernel::{
    chart::ObjectId,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    time::Duration,
};
use beatkernel_bms::*;
#[test]
fn tempo_stop_measure_length_and_layered_bgm_use_true_song_time() {
    let text = "#BPM 120\n#BPM0A 240\n#STOP01 48\n#WAV01 kick.wav\n#WAV02 pad.ogg\n#00002:0.75\n#00011:010001\n#00012:000100\n#00008:000A00\n#00009:000100\n#00001:000100\n#00001:000200\n#00103:00780000\n#00111:01000001\n#00116:01";
    let parsed = parse(text, ParseOptions::default()).unwrap();
    assert_eq!(parsed.source.ticks_per_beat, 1);
    assert_eq!(parsed.measures[0].end.ticks(), 3);
    assert_eq!(parsed.measures[1].end.ticks(), 7);
    assert_eq!(parsed.source.stops[0].duration.as_nanos(), 250_000_000);
    let compiled = parsed.compile().unwrap();
    let lane_times: Vec<_> = compiled
        .chart
        .objects()
        .iter()
        .map(|object| {
            let note = parsed
                .notes
                .iter()
                .find(|note| note.object == object.id)
                .unwrap();
            (note.lane.channel(), object.time.start.as_nanos())
        })
        .collect();
    assert_eq!(
        lane_times,
        vec![
            (0x11, 0),
            (0x12, 500_000_000),
            (0x11, 1_000_000_000),
            (0x11, 1_250_000_000),
            (0x16, 1_250_000_000),
            (0x11, 2_500_000_000)
        ]
    );
    assert_eq!(compiled.bgm.len(), 2);
    assert_eq!(compiled.bgm[0].at.as_nanos(), 500_000_000);
    assert_eq!(compiled.bgm[1].at, compiled.bgm[0].at);
    assert!(compiled.bgm[0].ordinal < compiled.bgm[1].ordinal);
    assert_eq!(parsed.samples[&2], "pad.ogg");
    assert!(parsed.notes.iter().any(|note| note.lane.is_scratch()));
}
#[test]
fn exact_fractional_measure_and_subdivision_grid_never_truncates_beats() {
    let parsed = parse(
        "#BPM 60\n#WAV01 a.wav\n#00002:0.125\n#00011:000001",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(parsed.source.ticks_per_beat, 6);
    assert_eq!(parsed.source.objects[0].start.ticks(), 2);
    assert_eq!(parsed.measures[0].end.ticks(), 3);
    assert_eq!(
        parsed.compile().unwrap().chart.objects()[0]
            .time
            .start
            .as_nanos(),
        333_333_333
    );
    let options = ParseOptions {
        max_resolution: 2,
        ..ParseOptions::default()
    };
    let error = parse("#WAV01 a.wav\n#00011:000001", options).unwrap_err();
    assert_eq!(error.line, 2);
    assert_eq!(error.kind, BmsErrorKind::Resolution);
}
#[test]
fn long_notes_pair_across_measures_preserving_unsounded_tail_and_lane_rules() {
    let parsed = parse(
        "#BPM 60\n#LNTYPE 1\n#WAV01 head.wav\n#00051:0001\n#00151:0002\n#00016:01\n#00021:01",
        ParseOptions::default(),
    )
    .unwrap();
    let hold = parsed
        .source
        .objects
        .iter()
        .find(|object| object.end.is_some())
        .unwrap();
    assert_eq!(hold.start.ticks(), 2);
    assert_eq!(hold.end.unwrap().ticks(), 6);
    assert_eq!(hold.interaction.0, 0x51);
    let note = parsed
        .notes
        .iter()
        .find(|note| note.object == hold.id)
        .unwrap();
    assert_eq!(note.lane.control().0, 0x11);
    assert_eq!(note.tail_sample.unwrap().0, 2);
    assert!(!parsed.samples.contains_key(&2));
    let compiled = parsed.compile().unwrap();
    let compiled_hold = compiled
        .chart
        .objects()
        .iter()
        .find(|object| object.id == hold.id)
        .unwrap();
    assert_eq!(compiled_hold.time.start.as_nanos(), 2_000_000_000);
    assert_eq!(compiled_hold.time.end.unwrap().as_nanos(), 6_000_000_000);
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(1),
            late: Duration::from_nanos(1),
        }],
        Duration::ZERO,
    )
    .unwrap();
    assert!(JudgeEngine::new(compiled.chart, parsed.rules(), profile).is_ok());
}
#[test]
fn lnobj_pairs_nearest_head_across_measures_after_exact_sort_and_keeps_other_instants() {
    let parsed = parse(
        "#BPM 60\n#lnobj zz\n#WAV01 a.wav\n#WAV02 b.wav\n#00111:00ZZ0002\n#00011:0102",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(parsed.notes.len(), 3);
    let positions: Vec<_> = parsed
        .source
        .objects
        .iter()
        .map(|object| (object.start.ticks(), object.end.map(|end| end.ticks())))
        .collect();
    assert_eq!(positions, [(0, None), (2, Some(5)), (7, None)]);
    assert_eq!(
        parsed
            .notes
            .iter()
            .map(|note| note.sample.0)
            .collect::<Vec<_>>(),
        [1, 2, 2]
    );
    assert_eq!(parsed.notes[1].tail_sample.map(|id| id.0), Some(1295));
    assert_eq!(parsed.notes[1].line, 6);
    assert_eq!(parsed.source.objects[1].interaction.0, 0x51);
    assert!(!parsed.samples.contains_key(&1295));
    assert!(parsed.bgm.is_empty());
    let compiled = parsed.compile().unwrap();
    assert_eq!(
        compiled.chart.objects()[1].time.end.unwrap().as_nanos(),
        5_000_000_000
    );
    let forward = parse(
        "#BPM 60\n#LNOBJ ZZ\n#WAV01 a.wav\n#WAV02 b.wav\n#00011:0102\n#00111:00ZZ0002",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        forward
            .source
            .objects
            .iter()
            .map(|object| (object.start.ticks(), object.end.map(|end| end.ticks())))
            .collect::<Vec<_>>(),
        positions
    );
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(1),
            late: Duration::from_nanos(1),
        }],
        Duration::ZERO,
    )
    .unwrap();
    assert!(JudgeEngine::new(compiled.chart, parsed.rules(), profile).is_ok());
}

#[test]
fn lnobj_lanes_and_scratch_are_independent_and_zero_tokens_do_not_close_notes() {
    let parsed = parse(
        "#LNOBJ ZZ\n#WAV01 a.wav\n#00011:01ZZ0001\n#00021:0001ZZ00\n#00016:01ZZ",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(parsed.notes.len(), 4);
    let holds: Vec<_> = parsed
        .source
        .objects
        .iter()
        .zip(&parsed.notes)
        .filter_map(|(object, note)| {
            object
                .end
                .map(|end| (note.lane.channel(), object.start.ticks(), end.ticks()))
        })
        .collect();
    assert_eq!(holds, [(0x11, 0, 1), (0x16, 0, 2), (0x21, 1, 2)]);
    assert!(
        parsed
            .notes
            .iter()
            .any(|note| note.lane.is_scratch() && note.tail_sample.is_some())
    );
    let zeros = parse(
        "#LNOBJ ZZ\n#WAV01 a.wav\n#00011:000001ZZ",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(zeros.source.objects[0].start.ticks(), 2);
    assert_eq!(zeros.source.objects[0].end.unwrap().ticks(), 3);
}

#[test]
fn lnobj_headers_and_endpoints_have_explicit_validation_and_duplicate_policy() {
    for marker in ["", "0", "000", "00", "Z!", "字", "ZZ extra"] {
        let error = parse(&format!("#LNOBJ {marker}"), ParseOptions::default()).unwrap_err();
        assert_eq!(error.line, 1);
        assert!(matches!(error.kind, BmsErrorKind::Syntax(_)));
    }
    for second in ["zz", "02"] {
        let error = parse(
            &format!("#LNOBJ ZZ\n#lnobj {second}"),
            ParseOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.line, 2);
        assert_eq!(error.kind, BmsErrorKind::Duplicate("LNOBJ"));
    }
    let options = ParseOptions {
        duplicates: DuplicatePolicy::LastWins,
        ..ParseOptions::default()
    };
    let parsed = parse(
        "#LNOBJ ZZ\n#LNOBJ 02\n#WAV01 a.wav\n#WAVZZ z.wav\n#00011:01ZZ02",
        options,
    )
    .unwrap();
    assert_eq!(parsed.metadata["LNOBJ"], "02");
    assert_eq!(parsed.notes.len(), 2);
    assert_eq!(parsed.notes[0].tail_sample, None);
    assert_eq!(parsed.notes[1].sample.0, 1295);
    assert_eq!(parsed.notes[1].tail_sample.unwrap().0, 2);
    for (rows, line) in [
        ("#00011:ZZ", 3),
        ("#00011:01ZZZZ", 3),
        ("#00011:01ZZ\n#00111:ZZ", 4),
        ("#00011:01\n#00012:ZZ", 4),
    ] {
        let error = parse(
            &format!("#LNOBJ ZZ\n#WAV01 a.wav\n{rows}"),
            ParseOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.line, line);
        assert!(matches!(error.kind, BmsErrorKind::LongNote(_)));
    }
    // Same-tick conflicts retain ordinary merge policy before LNOBJ pairing.
    let text = "#LNOBJ ZZ\n#WAV01 a.wav\n#00011:01\n#00011:ZZ";
    assert!(matches!(
        parse(text, ParseOptions::default()).unwrap_err().kind,
        BmsErrorKind::Duplicate(_)
    ));
    let error = parse(text, options).unwrap_err();
    assert_eq!(error.line, 4);
    assert!(matches!(error.kind, BmsErrorKind::LongNote(_)));
}

#[test]
fn mixed_lnobj_lntype_ranges_sort_together_and_reject_inclusive_lane_conflicts() {
    let prefix = "#LNOBJ ZZ\n#LNTYPE 1\n#WAV01 a.wav\n";
    for rows in [
        "#00011:01ZZ0000\n#00051:00000102",
        "#00051:00000102\n#00011:01ZZ0000",
    ] {
        let parsed = parse(&format!("{prefix}{rows}"), ParseOptions::default()).unwrap();
        assert_eq!(
            parsed
                .source
                .objects
                .iter()
                .map(|object| (object.start.ticks(), object.end.unwrap().ticks()))
                .collect::<Vec<_>>(),
            [(0, 1), (2, 3)]
        );
        assert_eq!(parsed.notes[0].tail_sample.unwrap().0, 1295);
        assert_eq!(parsed.notes[1].tail_sample.unwrap().0, 2);
    }
    for rows in [
        "#00011:01ZZ0000\n#00051:00010200", // Touch at LNOBJ tail.
        "#00011:0001ZZ00\n#00051:01000002", // LNTYPE encloses LNOBJ.
        "#00011:010000ZZ\n#00051:00010200", // LNOBJ encloses LNTYPE.
        "#00011:00010000\n#00051:01000002", // Instant inside LNTYPE.
        "#00011:01\n#00051:0102",           // Instant at head.
        "#00011:0001\n#00051:0102",         // Instant at tail.
    ] {
        assert!(
            matches!(
                parse(&format!("{prefix}{rows}"), ParseOptions::default())
                    .unwrap_err()
                    .kind,
                BmsErrorKind::LongNote(_)
            ),
            "{rows}"
        );
    }
    let different = parse(
        &format!("{prefix}#00011:01ZZ\n#00052:0102"),
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(different.notes.len(), 2);
}

#[test]
fn lnobj_uses_exact_tempo_stop_grid_and_counts_raw_endpoints_before_pairing() {
    let text = "#BPM 60\n#BPM01 120\n#STOP01 48\n#LNOBJ ZZ\n#WAV01 a.wav\n#00011:010000ZZ\n#00008:00010000\n#00009:00000100";
    let parsed = parse(text, ParseOptions::default()).unwrap();
    assert_eq!(parsed.source.objects[0].end.unwrap().ticks(), 3);
    assert_eq!(parsed.source.stops[0].duration.as_nanos(), 500_000_000);
    assert_eq!(
        parsed.compile().unwrap().chart.objects()[0]
            .time
            .end
            .unwrap()
            .as_nanos(),
        2_500_000_000
    );
    let fractional = parse(
        "#BPM 60\n#LNOBJ ZZ\n#WAV01 a.wav\n#00002:0.125\n#00011:0100ZZ",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(fractional.source.ticks_per_beat, 6);
    assert_eq!(fractional.source.objects[0].end.unwrap().ticks(), 2);
    assert_eq!(
        fractional.compile().unwrap().chart.objects()[0]
            .time
            .end
            .unwrap()
            .as_nanos(),
        333_333_333
    );
    let options = ParseOptions {
        max_objects: 1,
        ..ParseOptions::default()
    };
    let error = parse("#LNOBJ ZZ\n#WAV01 a.wav\n#00011:01ZZ", options).unwrap_err();
    assert_eq!(error.line, 3);
    assert_eq!(error.kind, BmsErrorKind::Limit("nonzero tokens"));
    let options = ParseOptions {
        max_objects: 2,
        ..options
    };
    assert_eq!(
        parse("#LNOBJ ZZ\n#WAV01 a.wav\n#00011:01ZZ", options)
            .unwrap()
            .notes
            .len(),
        1
    );
    let options = ParseOptions {
        max_resolution: 2,
        ..ParseOptions::default()
    };
    assert_eq!(
        parse("#LNOBJ ZZ\n#WAV01 a.wav\n#00011:0100ZZ", options)
            .unwrap_err()
            .kind,
        BmsErrorKind::Resolution
    );
}

#[test]
fn lnobj_marker_is_unsounded_optional_wav_only_for_gameplay_endpoint() {
    let text = "#LNOBJ ZZ\n#WAV01 a.wav\n#00011:01ZZ\n#00001:ZZ";
    let error = parse(text, ParseOptions::default()).unwrap_err();
    assert_eq!(error.line, 4);
    assert_eq!(
        error.kind,
        BmsErrorKind::MissingDefinition {
            kind: "WAV",
            index: 1295
        }
    );
    let defined = parse(&format!("#WAVZZ tail.wav\n{text}"), ParseOptions::default()).unwrap();
    assert_eq!(defined.source.objects.len(), 1);
    assert_eq!(defined.notes[0].sample.0, 1);
    assert_eq!(defined.notes[0].tail_sample.unwrap().0, 1295);
    assert_eq!(defined.bgm.len(), 1); // Only explicit channel01 schedules a sound.
    assert_eq!(defined.bgm[0].sample.0, 1295);
}
#[test]
fn duplicate_nonzero_positions_have_explicit_policy_and_zeros_never_delete() {
    let merged = parse(
        "#WAV01 a.wav\n#WAV02 b.wav\n#00011:0100\n#00011:00000200",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(merged.notes.len(), 2);
    let text = "#WAV01 a.wav\n#WAV02 b.wav\n#00011:01\n#00011:02\n#00011:00";
    let error = parse(text, ParseOptions::default()).unwrap_err();
    assert_eq!(error.line, 4);
    assert!(matches!(error.kind, BmsErrorKind::Duplicate(_)));
    let options = ParseOptions {
        duplicates: DuplicatePolicy::LastWins,
        ..ParseOptions::default()
    };
    let parsed = parse(text, options).unwrap();
    assert_eq!(parsed.notes.len(), 1);
    assert_eq!(parsed.notes[0].sample.0, 2);
    let layered = parse(
        "#WAV01 a.wav\n#00001:01\n#00001:01",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(layered.bgm.len(), 2);
    assert!(layered.source.objects.is_empty());
}
#[test]
fn stops_ignore_measure_length_and_use_same_beat_new_tempo() {
    let parsed = parse(
        "#BPM 60\n#BPMZZ 120\n#STOPZZ 48\n#00002:0.5\n#00008:ZZ\n#00009:ZZ",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(parsed.source.stops[0].duration.as_nanos(), 500_000_000);
    assert_eq!(parsed.source.bpm_changes[0].bpm.numerator(), 120);
}
#[test]
fn unsupported_and_missing_definitions_are_line_specific() {
    for command in [
        "#SCROLL 2",
        "#LNTYPE 2",
        "#STP 001.0 100",
        "#00031:01",
        "#000SC:01",
    ] {
        let error = parse(command, ParseOptions::default()).unwrap_err();
        assert_eq!(error.line, 1, "{command}");
        assert!(
            matches!(error.kind, BmsErrorKind::Unsupported(_)),
            "{command}: {error:?}"
        );
    }
    let error = parse("#BPM 120\n#00011:ZZ", ParseOptions::default()).unwrap_err();
    assert_eq!(error.line, 2);
    assert_eq!(
        error.kind,
        BmsErrorKind::MissingDefinition {
            kind: "WAV",
            index: 1295
        }
    );
    assert!(matches!(
        parse("#WAV01 a.wav\n#00051:01", ParseOptions::default())
            .unwrap_err()
            .kind,
        BmsErrorKind::LongNote(_)
    ));
    assert!(matches!(
        parse(
            "#WAV01 a.wav\n#00051:0101\n#00011:01",
            ParseOptions::default()
        )
        .unwrap_err()
        .kind,
        BmsErrorKind::LongNote(_)
    ));
}
#[test]
fn independent_timing_channels_conflict_even_with_last_wins() {
    let options = ParseOptions {
        duplicates: DuplicatePolicy::LastWins,
        ..ParseOptions::default()
    };
    assert!(matches!(
        parse("#BPM01 240\n#00003:78\n#00008:01", options)
            .unwrap_err()
            .kind,
        BmsErrorKind::Duplicate(_)
    ));
    assert!(matches!(
        parse("#STOP01 48\n#00009:01\n#00009:01", options)
            .unwrap_err()
            .kind,
        BmsErrorKind::Duplicate(_)
    ));
}
#[test]
fn parser_caps_bytes_lines_tokens_and_retains_utf8_metadata() {
    let options = ParseOptions {
        max_bytes: 2,
        ..ParseOptions::default()
    };
    assert!(matches!(
        parse("#BPM 120", options).unwrap_err().kind,
        BmsErrorKind::Limit("input bytes")
    ));
    let options = ParseOptions {
        max_objects: 1,
        ..ParseOptions::default()
    };
    assert_eq!(
        parse("#WAV01 a.wav\n#00011:0101", options)
            .unwrap_err()
            .line,
        2
    );
    let parsed = parse(
        "\u{feff}#TITLE 日本語\n#WAVzz exact path.ogg\n#00011:zz\n#BMP01 scene.png",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(parsed.metadata["TITLE"], "日本語");
    assert_eq!(parsed.samples[&1295], "exact path.ogg");
    assert_eq!(parsed.notes[0].object, ObjectId(1));
    assert_eq!(parsed.images[&ImageId(1)], "scene.png");
    assert!(parsed.warnings.is_empty());
}

#[test]
fn image_definitions_and_independent_channel_selections_preserve_opaque_resources() {
    let chart=parse("#BMP00 初期\\poor image.png\n#bmpAz 背景\\stage.PNG\n#00004:00AZ\n#00006:0001\n#00007:00AZ",ParseOptions::default()).unwrap();
    assert_eq!(chart.images[&ImageId(0)], "初期\\poor image.png");
    assert_eq!(chart.images[&ImageId(395)], "背景\\stage.PNG");
    assert_eq!(chart.bga.len(), 3);
    assert!(chart.source.objects.is_empty());
    assert!(chart.bgm.is_empty());
    assert!(chart.warnings.is_empty());
    let scheduled = chart.compile_bga().unwrap();
    assert_eq!(
        scheduled
            .iter()
            .map(|e| (e.at.as_nanos(), e.channel, e.image, e.ordinal))
            .collect::<Vec<_>>(),
        vec![
            (923_076_923, BgaChannel::Base, ImageId(395), 0),
            (923_076_923, BgaChannel::Poor, ImageId(1), 1),
            (923_076_923, BgaChannel::Layer, ImageId(395), 2)
        ]
    );
    assert_eq!(chart.compile().unwrap().bga, scheduled);
    assert!(!chart.images.contains_key(&ImageId(1))); // Undefined selections are admitted.
}

#[test]
fn image_definition_and_same_channel_duplicates_obey_explicit_policy() {
    let text = "#BMP01 first.png\n#bmp01 second.png\n#00004:01\n#00004:02";
    assert_eq!(parse(text, ParseOptions::default()).unwrap_err().line, 2);
    let last = ParseOptions {
        duplicates: DuplicatePolicy::LastWins,
        ..ParseOptions::default()
    };
    let chart = parse(text, last).unwrap();
    assert_eq!(chart.images[&ImageId(1)], "second.png");
    assert_eq!(chart.bga.len(), 1);
    assert_eq!(chart.bga[0].image, ImageId(2));
    assert_eq!(chart.bga[0].ordinal, 1);
    let error = parse("#00004:01\n#00004:02", ParseOptions::default()).unwrap_err();
    assert_eq!(error.line, 2);
    assert!(matches!(
        error.kind,
        BmsErrorKind::Duplicate("BGA channel position")
    ));
    for malformed in [
        "#BMP01",
        "#BMP01 \0bad",
        "#BMP$1 bad",
        "#BMP0 image.png",
        "#BMP001 image.png",
    ] {
        assert!(parse(malformed, ParseOptions::default()).is_err());
    }
    let zeros = parse(
        "#BMP00 poor.png\n#00004:0000\n#00006:00\n#00007:00",
        ParseOptions::default(),
    )
    .unwrap();
    assert!(zeros.bga.is_empty());
    assert_eq!(zeros.images.len(), 1);
}

#[test]
fn visual_only_subdivisions_do_not_change_gameplay_bgm_grid_ordinals_or_rounding() {
    let prefix = "#BPM 137\n#WAV01 head.wav\n#00011:0101\n";
    let plain = parse(
        &format!("{prefix}; ignored visual slot\n#00001:0101"),
        ParseOptions::default(),
    )
    .unwrap();
    let visual = parse(
        &format!("{prefix}#00004:00010000000000\n#00001:0101"),
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(plain.source, visual.source);
    assert_eq!(plain.notes, visual.notes);
    assert_eq!(plain.bgm, visual.bgm);
    assert_eq!(plain.measures, visual.measures);
    assert_eq!(visual.source.ticks_per_beat, 1);
    assert_eq!(visual.bga_ticks_per_beat, 7);
    assert_eq!(
        plain.compile().unwrap().chart,
        visual.compile().unwrap().chart
    );
    assert_eq!(plain.compile().unwrap().bgm, visual.compile().unwrap().bgm);
    assert_eq!(visual.compile_bga().unwrap()[0].at.as_nanos(), 250_260_688);
    let limited = ParseOptions {
        max_resolution: 6,
        ..ParseOptions::default()
    };
    assert!(matches!(
        parse(&format!("{prefix}#00004:00010000000000"), limited)
            .unwrap_err()
            .kind,
        BmsErrorKind::Resolution
    ));
}

#[test]
fn bga_uses_irregular_measure_bpm_and_pre_stop_core_timing() {
    let text = "#BPM 120\n#BPM01 240\n#STOP01 48\n#WAV01 head.wav\n#00002:0.75\n#00008:000100\n#00009:000100\n#00011:000001\n#00004:010203\n#00107:01";
    let parsed = parse(text, ParseOptions::default()).unwrap();
    assert_eq!(parsed.source.ticks_per_beat, 1);
    let compiled = parsed.compile().unwrap();
    assert_eq!(
        compiled
            .bga
            .iter()
            .map(|e| e.at.as_nanos())
            .collect::<Vec<_>>(),
        vec![0, 500_000_000, 1_000_000_000, 1_250_000_000]
    );
    assert_eq!(
        compiled.chart.objects()[0].time.start.as_nanos(),
        1_000_000_000
    );
    assert_eq!(compiled.bga[1].image, ImageId(2));
    let simultaneous = parse(
        "#BPM 60\n#STOP01 48\n#00009:01\n#00007:03\n#00004:01\n#00006:02",
        ParseOptions::default(),
    )
    .unwrap()
    .compile_bga()
    .unwrap();
    assert_eq!(
        simultaneous
            .iter()
            .map(|e| (e.at.as_nanos(), e.channel, e.ordinal))
            .collect::<Vec<_>>(),
        vec![
            (0, BgaChannel::Layer, 0),
            (0, BgaChannel::Base, 1),
            (0, BgaChannel::Poor, 2)
        ]
    );
}

#[test]
fn seeded_visual_payloads_and_visual_counts_use_the_same_bounded_parser() {
    let text = "#RANDOM 2\n#IF 1\n#BMP01 first.png\n#00004:01\n#ELSE\n#BMP02 second.png\n#00007:02\n#ENDIF";
    let first = parse_seeded(text, ParseOptions::default(), 3).unwrap();
    let second = parse_seeded(text, ParseOptions::default(), 0).unwrap();
    assert_eq!(first.bga[0].image, ImageId(1));
    assert_eq!(second.bga[0].image, ImageId(2));
    assert_eq!(first.images.len(), 1);
    assert_eq!(second.images.len(), 1);
    let limited = ParseOptions {
        max_objects: 1,
        ..ParseOptions::default()
    };
    assert_eq!(parse("#00004:0101", limited).unwrap_err().line, 1);
    assert!(parse("#WAV01 head.wav\n#00011:01\n#00007:01", limited).is_err());
    let chart = parse("#00004:01", limited).unwrap();
    assert_eq!(chart.bga.len(), 1);
    let mut bad_grid = chart.clone();
    bad_grid.bga_ticks_per_beat = 0;
    assert!(matches!(
        bad_grid.compile_bga().unwrap_err().kind,
        BmsErrorKind::Resolution
    ));
    let mut overflow = chart;
    overflow.bga_ticks_per_beat = 2;
    overflow
        .source
        .bpm_changes
        .push(beatkernel::chart::BpmChange {
            beat: beatkernel::chart::Beat::new(i64::MAX).unwrap(),
            bpm: beatkernel::chart::Bpm::new(120, 1).unwrap(),
        });
    assert!(matches!(
        overflow.compile_bga().unwrap_err().kind,
        BmsErrorKind::Overflow
    ));
}
