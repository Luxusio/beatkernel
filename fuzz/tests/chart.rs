use beatkernel::chart::{
    AudioBinding, Beat, Bpm, BpmChange, ChartError, InteractionId, ObjectId, ObjectMetadata,
    ScrollChange, ScrollVelocity, SourceChart, SourceObject, Stop, VisualId,
};
use beatkernel::time::Duration;
use beatkernel_layer_fuzz::chart::{chart_from_bytes, check_chart, CHART_MAX_BYTES};

fn beat(ticks: i64) -> Beat {
    Beat::new(ticks).unwrap()
}

// The frozen arbitrary schema uses little-endian integers, without a separate codec.
fn header(mode: u8, domain: u8, resolution: u32, numerator: u32, denominator: u32) -> Vec<u8> {
    let mut bytes = vec![mode, domain];
    bytes.extend(resolution.to_le_bytes());
    bytes.extend(numerator.to_le_bytes());
    bytes.extend(denominator.to_le_bytes());
    bytes.extend([0; 4]);
    bytes
}

fn raw_object(bytes: &mut Vec<u8>, id: u64, start: i64, end: Option<i64>, metadata: &[u8]) {
    bytes.extend(id.to_le_bytes());
    bytes.extend(start.to_le_bytes());
    bytes.push(u8::from(end.is_some()));
    if let Some(end) = end {
        bytes.extend(end.to_le_bytes());
    }
    bytes.extend(17u32.to_le_bytes());
    bytes.extend(29u32.to_le_bytes());
    bytes.push(1);
    bytes.extend(43u32.to_le_bytes());
    bytes.push(u8::try_from(metadata.len()).unwrap());
    bytes.extend(metadata);
}

fn object(id: u64, start: i64, end: Option<i64>) -> SourceObject {
    SourceObject {
        id: ObjectId(id),
        start: beat(start),
        end: end.map(beat),
        interaction: InteractionId(17),
        visual: VisualId(29),
        audio: Some(AudioBinding(43)),
        metadata: ObjectMetadata(vec![0, 255, id as u8]),
    }
}

#[test]
fn campaign_limit_and_invalid_constructors_are_rejected() {
    assert_eq!(CHART_MAX_BYTES, 4096);
    assert!(chart_from_bytes(&[]).is_none());
    assert!(!check_chart(&[]));
    assert!(chart_from_bytes(&vec![0; CHART_MAX_BYTES + 1]).is_none());
    assert!(!check_chart(&vec![0; CHART_MAX_BYTES + 1]));
    let mut negative = header(0, 1, 1, 120, 1);
    negative[14] = 1;
    raw_object(&mut negative, 1, -1, None, &[]);
    assert!(chart_from_bytes(&negative).is_none());
    assert!(!check_chart(&negative));
    assert!(chart_from_bytes(&header(0, 0, 0, 120, 1)).is_none());
    assert!(chart_from_bytes(&header(0, 0, 480, 0, 1)).is_none());
    let valid = header(0, 0, 480, 120, 1);
    for length in 0..valid.len() {
        assert!(
            chart_from_bytes(&valid[..length]).is_none(),
            "length {length}"
        );
    }
}

#[test]
fn every_mode_reaches_the_real_compiler_with_a_golden_result() {
    let expected = [
        None,
        Some(ChartError::Overflow),
        Some(ChartError::NegativeStop { beat: beat(0) }),
        Some(ChartError::DuplicateObjectId {
            id: ObjectId(u64::MAX),
        }),
        Some(ChartError::ReversedRange {
            id: ObjectId(u64::MAX),
        }),
        Some(ChartError::DuplicateBpm { beat: beat(0) }),
        Some(ChartError::DuplicateStop { beat: beat(0) }),
        Some(ChartError::InvalidResolution),
    ];
    for (mode, error) in expected.into_iter().enumerate() {
        for raw_mode in [mode as u8, mode as u8 + 8] {
            let bytes = header(raw_mode, 0, 480, 120, 1);
            let source = chart_from_bytes(&bytes).expect("mode must not stop at construction");
            assert_eq!(chart_from_bytes(&bytes), Some(source.clone()));
            let actual = source.compile();
            match error {
                Some(error) => assert_eq!(actual, Err(error), "mode {raw_mode}"),
                None => {
                    let compiled = actual.unwrap();
                    assert!(compiled.objects().is_empty());
                    assert!(compiled.bpm_changes().is_empty());
                    assert!(compiled.stops().is_empty());
                    assert!(compiled.scroll_changes().is_empty());
                }
            }
            assert_eq!(check_chart(&bytes), error.is_none(), "mode {raw_mode}");
            assert_eq!(
                check_chart(&bytes),
                error.is_none(),
                "mode {raw_mode} repeated"
            );
        }
    }
}

#[test]
fn maximum_structured_collections_and_metadata_reach_compile() {
    let mut bytes = header(0, 1, 480, 120, 1);
    bytes[14..18].copy_from_slice(&[64, 16, 16, 16]);
    for id in 0..64 {
        let metadata = if id == 0 { vec![255; 64] } else { vec![] };
        raw_object(&mut bytes, id, id as i64 * 480, None, &metadata);
    }
    for tick in 0..16i64 {
        bytes.extend((tick * 480).to_le_bytes());
        bytes.extend(120u32.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
    }
    for tick in 0..16i64 {
        bytes.extend((tick * 480).to_le_bytes());
        bytes.extend(0i64.to_le_bytes());
    }
    for tick in 0..16i64 {
        bytes.extend((tick * 480).to_le_bytes());
        bytes.extend((-1i64).to_le_bytes());
        bytes.extend(2u32.to_le_bytes());
    }
    assert!(bytes.len() <= CHART_MAX_BYTES);
    let source = chart_from_bytes(&bytes).unwrap();
    assert_eq!(source.objects.len(), 64);
    assert_eq!(source.bpm_changes.len(), 16);
    assert_eq!(source.stops.len(), 16);
    assert_eq!(source.scroll_changes.len(), 16);
    assert_eq!(source.objects[0].metadata.0, vec![255; 64]);
    let compiled = source.compile().unwrap();
    assert_eq!(compiled.objects().len(), 64);
    assert_eq!(compiled.bpm_changes().len(), 16);
    assert_eq!(compiled.stops().len(), 16);
    assert_eq!(compiled.scroll_changes().len(), 16);
    assert_eq!(compiled.objects()[63].time.start.as_nanos(), 31_500_000_000);
    assert!(check_chart(&bytes));
}

#[test]
fn raw_source_preserves_bindings_metadata_and_exact_timing() {
    let mut bytes = header(0, 1, 3, 90, 1);
    bytes[14] = 3;
    raw_object(&mut bytes, 9, 6, Some(9), &[0, 255, 9]);
    raw_object(&mut bytes, 4, 1, None, &[0, 255, 4]);
    raw_object(&mut bytes, 2, 1, Some(3), &[0, 255, 2]);
    let source = chart_from_bytes(&bytes).expect("valid raw chart must reach compile");
    assert_eq!(source.ticks_per_beat, 3);
    assert_eq!(source.initial_bpm, Bpm::new(90, 1).unwrap());
    assert_eq!(
        source.objects,
        vec![
            object(9, 6, Some(9)),
            object(4, 1, None),
            object(2, 1, Some(3))
        ]
    );
    let compiled = source.compile().unwrap();
    assert_eq!(
        compiled
            .objects()
            .iter()
            .map(|o| o.id.0)
            .collect::<Vec<_>>(),
        vec![2, 4, 9]
    );
    assert_eq!(
        compiled
            .objects()
            .iter()
            .map(|o| o.time.start.as_nanos())
            .collect::<Vec<_>>(),
        vec![222_222_222, 222_222_222, 1_333_333_333]
    );
    assert_eq!(
        compiled.objects()[0].time.end.unwrap().as_nanos(),
        666_666_666
    );
    assert_eq!(
        compiled.objects()[2].time.end.unwrap().as_nanos(),
        2_000_000_000
    );
    for timed in compiled.objects() {
        let original = source.objects.iter().find(|o| o.id == timed.id).unwrap();
        assert_eq!(timed.interaction, original.interaction);
        assert_eq!(timed.visual, original.visual);
        assert_eq!(timed.audio, original.audio);
        assert_eq!(timed.metadata, original.metadata);
    }
    assert!(check_chart(&bytes));
}

#[test]
fn fresh_raw_constant_tempo_cases_match_i128_oracle() {
    for seed in 0..32u32 {
        let resolution = 1 + seed * 17;
        let numerator = 1 + seed * 31;
        let denominator = 1 + seed * 7;
        let mut bytes = header(0, 1, resolution, numerator, denominator);
        bytes[14] = 4;
        for (id, tick) in [(4, 0), (3, 1), (2, 129), (1, 9001)] {
            raw_object(&mut bytes, id, tick, Some(tick + 11), &[seed as u8]);
        }
        let source = chart_from_bytes(&bytes).unwrap();
        let compiled = source.compile().unwrap();
        for timed in compiled.objects() {
            let original = source.objects.iter().find(|o| o.id == timed.id).unwrap();
            let expected = |ticks: i64| {
                let nanos = i128::from(ticks) * 60_000_000_000 * i128::from(denominator)
                    / (i128::from(resolution) * i128::from(numerator));
                i64::try_from(nanos).unwrap()
            };
            assert_eq!(
                timed.time.start.as_nanos(),
                expected(original.start.ticks())
            );
            assert_eq!(
                timed.time.end.unwrap().as_nanos(),
                expected(original.end.unwrap().ticks())
            );
        }
        assert_eq!(chart_from_bytes(&bytes), Some(source.clone()));
        assert_eq!(source.compile(), Ok(compiled));
        assert!(check_chart(&bytes));
        assert!(check_chart(&bytes));
    }
}

#[test]
fn raw_overflow_is_a_compiler_error_after_valid_construction() {
    let mut bytes = header(0, 1, 1, 1, 1);
    bytes[14] = 1;
    raw_object(&mut bytes, 81, i64::MAX, None, &[255]);
    let source = chart_from_bytes(&bytes).expect("nonnegative beat has a valid constructor");
    assert_eq!(source.objects[0].start.ticks(), i64::MAX);
    let independent_nanos = i128::from(i64::MAX) * 60_000_000_000;
    assert!(i64::try_from(independent_nanos).is_err());
    assert_eq!(source.compile(), Err(ChartError::Overflow));
    assert!(!check_chart(&bytes));
    assert!(!check_chart(&bytes));
}

#[test]
fn stop_and_tempo_markers_use_pre_stop_time_and_survive_permutation() {
    let mut source = SourceChart::new(4, Bpm::new(120, 1).unwrap()).unwrap();
    source.objects = vec![
        object(7, 8, Some(12)),
        object(3, 4, None),
        object(2, 4, None),
    ];
    source.bpm_changes.push(BpmChange {
        beat: beat(4),
        bpm: Bpm::new(60, 1).unwrap(),
    });
    source.stops.push(Stop {
        beat: beat(4),
        duration: Duration::from_nanos(250_000_000),
    });
    source.scroll_changes = vec![
        ScrollChange {
            beat: beat(8),
            velocity: ScrollVelocity::new(-3, 2).unwrap(),
        },
        ScrollChange {
            beat: beat(4),
            velocity: ScrollVelocity::new(0, 1).unwrap(),
        },
    ];
    let mut bytes = header(0, 1, 4, 120, 1);
    bytes[14..18].copy_from_slice(&[3, 1, 1, 2]);
    for o in &source.objects {
        raw_object(
            &mut bytes,
            o.id.0,
            o.start.ticks(),
            o.end.map(Beat::ticks),
            &o.metadata.0,
        );
    }
    bytes.extend(4i64.to_le_bytes());
    bytes.extend(60u32.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(4i64.to_le_bytes());
    bytes.extend(250_000_000i64.to_le_bytes());
    for (tick, numerator, denominator) in [(8i64, -3i64, 2u32), (4, 0, 1)] {
        bytes.extend(tick.to_le_bytes());
        bytes.extend(numerator.to_le_bytes());
        bytes.extend(denominator.to_le_bytes());
    }
    assert_eq!(chart_from_bytes(&bytes), Some(source.clone()));
    assert!(check_chart(&bytes));
    let compiled = source.compile().unwrap();
    assert_eq!(
        compiled
            .objects()
            .iter()
            .map(|o| (o.id.0, o.time.start.as_nanos()))
            .collect::<Vec<_>>(),
        vec![(2, 500_000_000), (3, 500_000_000), (7, 1_750_000_000)]
    );
    assert_eq!(
        compiled.objects()[2].time.end.unwrap().as_nanos(),
        2_750_000_000
    );
    assert_eq!(compiled.bpm_changes()[0].time.as_nanos(), 500_000_000);
    assert_eq!(compiled.stops()[0].time.as_nanos(), 500_000_000);
    assert_eq!(compiled.scroll_changes()[0].time.as_nanos(), 500_000_000);
    source.objects.reverse();
    source.bpm_changes.reverse();
    source.stops.reverse();
    source.scroll_changes.reverse();
    assert_eq!(source.compile(), Ok(compiled.clone()));
    source.ticks_per_beat *= 3;
    for o in &mut source.objects {
        o.start = beat(o.start.ticks() * 3);
        o.end = o.end.map(|end| beat(end.ticks() * 3));
    }
    for m in &mut source.bpm_changes {
        m.beat = beat(m.beat.ticks() * 3);
    }
    for m in &mut source.stops {
        m.beat = beat(m.beat.ticks() * 3);
    }
    for m in &mut source.scroll_changes {
        m.beat = beat(m.beat.ticks() * 3);
    }
    let scaled = source.compile().unwrap();
    assert_eq!(scaled.objects(), compiled.objects());
    assert_eq!(scaled.bpm_changes(), compiled.bpm_changes());
    assert_eq!(scaled.stops(), compiled.stops());
    assert_eq!(scaled.scroll_changes(), compiled.scroll_changes());
}
