//! Raw UTF-8 BMS checks against the production parser and scheduling APIs.

use beatkernel::chart::{AudioBinding, InteractionId, VisualId};
use beatkernel_bms::{parse_seeded, BmsChart, DuplicatePolicy, ParseOptions};

/// Maximum raw text size admitted by this campaign.
pub const BMS_MAX_BYTES: usize = 16_384;

/// Explicit campaign limits, including the combined source-item budget.
pub fn parser_options() -> ParseOptions {
    ParseOptions {
        max_bytes: BMS_MAX_BYTES,
        max_lines: 256,
        max_line_bytes: 1024,
        max_objects: 256,
        max_resolution: 65_536,
        duplicates: DuplicatePolicy::Reject,
    }
}

/// Checks deterministic parsing and all independently scheduled BMS families.
/// Returns true when either seed parses successfully, including compile errors.
pub fn check_bms(data: &[u8]) -> bool {
    if data.len() > BMS_MAX_BYTES {
        return false;
    }
    let Ok(text) = std::str::from_utf8(data) else {
        return false;
    };
    let options = parser_options();
    let lines = text.lines().count();
    let mut accepted = false;
    for seed in [0, 73] {
        let parsed = parse_seeded(text, options, seed);
        assert_eq!(parsed, parse_seeded(text, options, seed));
        match parsed {
            Ok(parsed) => {
                accepted = true;
                check_source(&parsed, options, lines);
                check_schedules(&parsed);
            }
            Err(error) => assert!(error.line <= lines),
        }
    }
    accepted
}

fn check_source(parsed: &BmsChart, options: ParseOptions, lines: usize) {
    let source = &parsed.source;
    let items = source.objects.len()
        + source.bpm_changes.len()
        + source.stops.len()
        + source.scroll_changes.len()
        + parsed.bgm.len()
        + parsed.bga.len()
        + parsed.bga_opacity.len()
        + parsed.invisible.len()
        + parsed.mines.len();
    assert!(items <= options.max_objects);
    for resolution in [
        source.ticks_per_beat,
        parsed.bga_ticks_per_beat,
        parsed.invisible_ticks_per_beat,
        parsed.mine_ticks_per_beat,
    ] {
        assert!((1..=options.max_resolution).contains(&resolution));
        assert_eq!(resolution % source.ticks_per_beat, 0);
    }
    // Measure channels have exactly three decimal digits.
    assert!(parsed.measures.len() <= 1000);
    for count in [
        parsed.samples.len(),
        parsed.images.len(),
        parsed.image_argb.len(),
        parsed.bga_crops.len(),
        parsed.metadata.len(),
        parsed.warnings.len(),
    ] {
        assert!(count <= options.max_lines);
    }
    assert!(parsed
        .warnings
        .iter()
        .all(|warning| (1..=lines).contains(&warning.line)));
    assert_eq!(parsed.notes.len(), source.objects.len());
    for (note, object) in parsed.notes.iter().zip(&source.objects) {
        assert_eq!(note.object, object.id);
        assert!((1..=lines).contains(&note.line));
        assert_eq!(object.visual, VisualId(u32::from(note.lane.channel())));
        assert_eq!(
            object.interaction,
            InteractionId(
                u32::from(note.lane.channel()) + if object.end.is_some() { 0x40 } else { 0 }
            )
        );
        assert_eq!(object.audio, Some(AudioBinding(note.sample.0 as u32)));
        assert!(parsed.samples.contains_key(&(note.sample.0 as u16)));
    }
    for event in &parsed.invisible {
        assert!((1..=lines).contains(&event.line));
        assert!(parsed.samples.contains_key(&(event.sample.0 as u16)));
    }
    assert!(parsed
        .mines
        .iter()
        .all(|event| (1..=lines).contains(&event.line)));
    assert!(parsed
        .bgm
        .iter()
        .all(|event| parsed.samples.contains_key(&(event.sample.0 as u16))));
}

// Scheduling sorts by timestamp/ordinal and preserves each source selection.
// Comparing sorted provenance tuples also detects omitted or duplicated events.
macro_rules! check_schedule {
    ($source:expr, $scheduled:expr, [$($field:ident),+]) => {{
        let source = $source;
        let scheduled = $scheduled;
        assert_eq!(scheduled.len(), source.len());
        assert!(scheduled.windows(2).all(|pair| {
            (pair[0].at, pair[0].ordinal) <= (pair[1].at, pair[1].ordinal)
        }));
        let mut original: Vec<_> = source.iter()
            .map(|event| (event.ordinal, $(event.$field),+)).collect();
        let mut projected: Vec<_> = scheduled.iter()
            .map(|event| (event.ordinal, $(event.$field),+)).collect();
        original.sort_by_key(|event| event.0);
        projected.sort_by_key(|event| event.0);
        assert_eq!(original, projected);
    }};
}

fn check_schedules(parsed: &BmsChart) {
    let compiled = parsed.compile();
    assert_eq!(compiled, parsed.compile());
    if let Ok(compiled) = compiled {
        let source = &parsed.source;
        let chart = &compiled.chart;
        assert_eq!(chart.ticks_per_beat(), source.ticks_per_beat);
        assert_eq!(chart.initial_bpm(), source.initial_bpm);
        assert_eq!(chart.objects().len(), source.objects.len());
        assert_eq!(chart.bpm_changes().len(), source.bpm_changes.len());
        assert_eq!(chart.stops().len(), source.stops.len());
        assert_eq!(chart.scroll_changes().len(), source.scroll_changes.len());
        assert!(chart
            .objects()
            .windows(2)
            .all(|pair| { (pair[0].time.start, pair[0].id) <= (pair[1].time.start, pair[1].id) }));
        let mut original_ids: Vec<_> = source.objects.iter().map(|object| object.id).collect();
        let mut compiled_ids: Vec<_> = chart.objects().iter().map(|object| object.id).collect();
        original_ids.sort_unstable();
        compiled_ids.sort_unstable();
        assert_eq!(original_ids, compiled_ids);
        for object in chart.objects() {
            let original = source
                .objects
                .iter()
                .find(|item| item.id == object.id)
                .unwrap();
            assert_eq!(object.interaction, original.interaction);
            assert_eq!(object.visual, original.visual);
            assert_eq!(object.audio, original.audio);
            assert_eq!(object.metadata, original.metadata);
            assert_eq!(object.time.end.is_some(), original.end.is_some());
            assert!(object.time.end.is_none_or(|end| end >= object.time.start));
        }
        check_schedule!(&parsed.bgm, &compiled.bgm, [sample]);
        check_schedule!(&parsed.bga, &compiled.bga, [channel, image]);
        check_schedule!(&parsed.bga_opacity, &compiled.bga_opacity, [channel, alpha]);
    }

    // These APIs remain reachable even when gameplay or visual timing overflows.
    let invisible = parsed.compile_invisible();
    assert_eq!(invisible, parsed.compile_invisible());
    if let Ok(scheduled) = invisible {
        check_schedule!(&parsed.invisible, &scheduled, [lane, sample, line]);
    }
    let mines = parsed.compile_mines();
    assert_eq!(mines, parsed.compile_mines());
    if let Ok(scheduled) = mines {
        check_schedule!(&parsed.mines, &scheduled, [lane, damage, line]);
    }
}
