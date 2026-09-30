use beatkernel::chart::*;
use beatkernel::time::{Duration, Timestamp};

fn beat(ticks: i64) -> Beat {
    Beat::new(ticks).unwrap()
}

fn bpm(value: u32) -> Bpm {
    Bpm::new(value, 1).unwrap()
}

fn object(id: u64, start: i64, end: Option<i64>) -> SourceObject {
    SourceObject {
        id: ObjectId(id),
        start: beat(start),
        end: end.map(beat),
        interaction: InteractionId(7),
        visual: VisualId(8),
        audio: Some(AudioBinding(9)),
        metadata: ObjectMetadata(vec![10, 11]),
    }
}

fn times(chart: &CompiledChart) -> Vec<(u64, i64, Option<i64>)> {
    chart
        .objects()
        .iter()
        .map(|object| {
            (
                object.id.0,
                object.time.start.as_nanos(),
                object.time.end.map(Timestamp::as_nanos),
            )
        })
        .collect()
}

#[test]
fn validated_rationals_include_zero_reverse_and_minimum_signed_velocity() {
    assert_eq!(Beat::new(-1), Err(ChartError::InvalidBeat));
    assert_eq!(Beat::new(i64::MAX).unwrap().ticks(), i64::MAX);
    assert_eq!(Bpm::new(0, 1), Err(ChartError::InvalidBpm));
    assert_eq!(Bpm::new(1, 0), Err(ChartError::InvalidBpm));
    assert_eq!(Bpm::new(240, 2).unwrap(), bpm(120));
    assert_eq!(
        ScrollVelocity::new(1, 0),
        Err(ChartError::InvalidScrollVelocity)
    );
    assert_eq!(
        ScrollVelocity::new(0, 100).unwrap(),
        ScrollVelocity::new(0, 1).unwrap()
    );
    let reverse = ScrollVelocity::new(-6, 4).unwrap();
    assert_eq!((reverse.numerator(), reverse.denominator()), (-3, 2));
    let minimum = ScrollVelocity::new(i64::MIN, 2).unwrap();
    assert_eq!(
        (minimum.numerator(), minimum.denominator()),
        (-4_611_686_018_427_387_904, 1)
    );
    assert_eq!(
        ScrollVelocity::new(i64::MIN, 1).unwrap().numerator(),
        i64::MIN
    );
    assert_eq!(
        SourceChart::new(0, bpm(120)),
        Err(ChartError::InvalidResolution)
    );
    let mut chart = SourceChart::new(480, bpm(120)).unwrap();
    chart.ticks_per_beat = 0;
    assert_eq!(chart.compile(), Err(ChartError::InvalidResolution));
}

#[test]
fn empty_and_basic_chart_preserve_bindings_and_own_metadata() {
    let mut source = SourceChart::new(480, bpm(120)).unwrap();
    let empty = source.compile().unwrap();
    assert!(empty.objects().is_empty());
    assert!(empty.bpm_changes().is_empty());
    assert!(empty.stops().is_empty());
    assert!(empty.scroll_changes().is_empty());
    assert_eq!(empty.ticks_per_beat(), 480);
    assert_eq!(empty.initial_bpm(), bpm(120));
    source.objects = vec![object(1, 0, None), object(2, 480, Some(960))];
    let compiled = source.compile().unwrap();
    assert_eq!(
        times(&compiled),
        vec![(1, 0, None), (2, 500_000_000, Some(1_000_000_000))]
    );
    source.objects[1].metadata.0[0] = 99;
    let second = &compiled.objects()[1];
    assert_eq!(second.interaction, InteractionId(7));
    assert_eq!(second.visual, VisualId(8));
    assert_eq!(second.audio, Some(AudioBinding(9)));
    assert_eq!(second.metadata, ObjectMetadata(vec![10, 11]));
}

#[test]
fn rational_bpm_and_distinct_ticks_collapsing_to_one_timestamp_are_exact() {
    let mut source = SourceChart::new(2, Bpm::new(3, 2).unwrap()).unwrap();
    source.objects = vec![object(1, 1, Some(2))];
    assert_eq!(
        times(&source.compile().unwrap()),
        vec![(1, 20_000_000_000, Some(40_000_000_000))]
    );
    source.ticks_per_beat = u32::MAX;
    source.initial_bpm = bpm(u32::MAX);
    source.objects = vec![object(9, 1, None), object(2, 2, None)];
    let compiled = source.compile().unwrap();
    assert_eq!(times(&compiled), vec![(2, 0, None), (9, 0, None)]);
    assert_eq!(
        compiled.objects_in_window(Timestamp::from_nanos(0), Timestamp::from_nanos(1)),
        compiled.objects()
    );
}

#[test]
fn bpm_stop_and_hold_endpoints_share_pre_stop_time() {
    let mut source = SourceChart::new(480, bpm(120)).unwrap();
    source.bpm_changes.push(BpmChange {
        beat: beat(480),
        bpm: bpm(60),
    });
    source.stops.push(Stop {
        beat: beat(480),
        duration: Duration::from_nanos(250_000_000),
    });
    source.objects = vec![
        object(1, 0, Some(960)),
        object(2, 480, Some(480)),
        object(3, 960, None),
    ];
    let compiled = source.compile().unwrap();
    assert_eq!(
        times(&compiled),
        vec![
            (1, 0, Some(1_750_000_000)),
            (2, 500_000_000, Some(500_000_000)),
            (3, 1_750_000_000, None)
        ]
    );
    assert_eq!(compiled.bpm_changes()[0].time.as_nanos(), 500_000_000);
    assert_eq!(compiled.stops()[0].time.as_nanos(), 500_000_000);
    source.bpm_changes.clear();
    assert_eq!(
        times(&source.compile().unwrap())[2],
        (3, 1_250_000_000, None)
    );
    source.stops.clear();
    source.bpm_changes.push(BpmChange {
        beat: beat(480),
        bpm: bpm(60),
    });
    assert_eq!(
        times(&source.compile().unwrap())[2],
        (3, 1_500_000_000, None)
    );
}

#[test]
fn beat_zero_bpm_stop_and_scroll_apply_after_origin() {
    let mut source = SourceChart::new(1, bpm(120)).unwrap();
    source.bpm_changes.push(BpmChange {
        beat: beat(0),
        bpm: bpm(60),
    });
    source.stops.push(Stop {
        beat: beat(0),
        duration: Duration::from_nanos(7),
    });
    source.scroll_changes.push(ScrollChange {
        beat: beat(0),
        velocity: ScrollVelocity::new(-1, 1).unwrap(),
    });
    source.objects = vec![object(1, 0, Some(1))];
    let compiled = source.compile().unwrap();
    assert_eq!(times(&compiled), vec![(1, 0, Some(1_000_000_007))]);
    assert_eq!(compiled.bpm_changes()[0].time.as_nanos(), 0);
    assert_eq!(compiled.stops()[0].time.as_nanos(), 0);
    assert_eq!(compiled.scroll_changes()[0].time.as_nanos(), 0);
}

#[test]
fn genuine_boundaries_truncate_but_noop_markers_and_objects_never_add_drift() {
    let mut source = SourceChart::new(7, bpm(120)).unwrap();
    source.objects = vec![object(1, 3, None)];
    assert_eq!(
        times(&source.compile().unwrap()),
        vec![(1, 214_285_714, None)]
    );
    let original = source.compile().unwrap();
    source.bpm_changes.push(BpmChange {
        beat: beat(1),
        bpm: Bpm::new(240, 2).unwrap(),
    });
    source.stops.push(Stop {
        beat: beat(2),
        duration: Duration::from_nanos(0),
    });
    assert_eq!(source.compile().unwrap().objects(), original.objects());
    source
        .objects
        .extend([object(2, 1, None), object(3, 2, None)]);
    assert_eq!(
        times(&source.compile().unwrap()),
        vec![
            (2, 71_428_571, None),
            (3, 142_857_142, None),
            (1, 214_285_714, None)
        ]
    );
    source.bpm_changes[0].bpm = bpm(60);
    source.stops[0].duration = Duration::from_nanos(1);
    assert_eq!(
        times(&source.compile().unwrap()),
        vec![
            (2, 71_428_571, None),
            (3, 214_285_713, None),
            (1, 357_142_856, None)
        ]
    );
}

#[test]
fn scroll_changes_never_change_judge_targets_even_at_timing_boundaries() {
    let mut source = SourceChart::new(480, bpm(120)).unwrap();
    source.objects = vec![object(1, 0, Some(960)), object(2, 480, None)];
    source.stops.push(Stop {
        beat: beat(480),
        duration: Duration::from_nanos(250_000_000),
    });
    let original = source.compile().unwrap();
    source.scroll_changes = vec![
        ScrollChange {
            beat: beat(960),
            velocity: ScrollVelocity::new(0, 1).unwrap(),
        },
        ScrollChange {
            beat: beat(480),
            velocity: ScrollVelocity::new(-3, 2).unwrap(),
        },
        ScrollChange {
            beat: beat(240),
            velocity: ScrollVelocity::new(2, 1).unwrap(),
        },
    ];
    let compiled = source.compile().unwrap();
    assert_eq!(compiled.objects(), original.objects());
    assert_eq!(
        compiled
            .scroll_changes()
            .iter()
            .map(|event| event.time.as_nanos())
            .collect::<Vec<_>>(),
        vec![250_000_000, 500_000_000, 1_250_000_000]
    );
    assert_eq!(
        compiled.scroll_changes()[1].velocity,
        ScrollVelocity::new(-3, 2).unwrap()
    );
}

#[test]
fn shuffled_sources_sort_by_timestamp_then_id_and_lookup_is_half_open() {
    let mut source = SourceChart::new(1, bpm(120)).unwrap();
    source.objects = vec![
        object(9, 2, None),
        object(8, 1, None),
        object(2, 1, None),
        object(1, 0, Some(4)),
    ];
    source.bpm_changes = vec![
        BpmChange {
            beat: beat(3),
            bpm: bpm(60),
        },
        BpmChange {
            beat: beat(1),
            bpm: bpm(240),
        },
    ];
    source.stops = vec![
        Stop {
            beat: beat(3),
            duration: Duration::from_nanos(10),
        },
        Stop {
            beat: beat(1),
            duration: Duration::from_nanos(20),
        },
    ];
    let compiled = source.compile().unwrap();
    assert_eq!(
        times(&compiled),
        vec![
            (1, 0, Some(2_000_000_030)),
            (2, 500_000_000, None),
            (8, 500_000_000, None),
            (9, 750_000_020, None)
        ]
    );
    source.objects.reverse();
    source.bpm_changes.reverse();
    source.stops.reverse();
    assert_eq!(source.compile().unwrap(), compiled);
    let window = compiled.objects_in_window(
        Timestamp::from_nanos(500_000_000),
        Timestamp::from_nanos(750_000_020),
    );
    assert_eq!(
        window.iter().map(|object| object.id.0).collect::<Vec<_>>(),
        vec![2, 8]
    );
    assert_eq!(window.as_ptr(), compiled.objects()[1..].as_ptr());
    for (start, end) in [(0, 0), (9, 8), (1, 499_999_999), (800_000_000, i64::MAX)] {
        assert!(compiled
            .objects_in_window(Timestamp::from_nanos(start), Timestamp::from_nanos(end))
            .is_empty());
    }
    assert_eq!(
        compiled.objects_in_window(
            Timestamp::from_nanos(i64::MIN),
            Timestamp::from_nanos(i64::MAX)
        ),
        compiled.objects()
    );
}

#[test]
fn duplicate_markers_ids_negative_stop_and_reversed_range_are_typed_errors() {
    let mut source = SourceChart::new(1, bpm(120)).unwrap();
    source.bpm_changes = vec![
        BpmChange {
            beat: beat(1),
            bpm: bpm(60)
        };
        2
    ];
    assert_eq!(
        source.compile(),
        Err(ChartError::DuplicateBpm { beat: beat(1) })
    );
    source.bpm_changes.clear();
    source.stops = vec![
        Stop {
            beat: beat(1),
            duration: Duration::from_nanos(0)
        };
        2
    ];
    assert_eq!(
        source.compile(),
        Err(ChartError::DuplicateStop { beat: beat(1) })
    );
    source.stops = vec![Stop {
        beat: beat(1),
        duration: Duration::from_nanos(-1),
    }];
    assert_eq!(
        source.compile(),
        Err(ChartError::NegativeStop { beat: beat(1) })
    );
    source.stops.clear();
    source.scroll_changes = vec![
        ScrollChange {
            beat: beat(1),
            velocity: ScrollVelocity::new(1, 1).unwrap()
        };
        2
    ];
    assert_eq!(
        source.compile(),
        Err(ChartError::DuplicateScroll { beat: beat(1) })
    );
    source.scroll_changes.clear();
    source.objects = vec![object(7, 0, None), object(7, 1, None)];
    assert_eq!(
        source.compile(),
        Err(ChartError::DuplicateObjectId { id: ObjectId(7) })
    );
    source.objects = vec![object(7, 1, Some(0))];
    assert_eq!(
        source.compile(),
        Err(ChartError::ReversedRange { id: ObjectId(7) })
    );
}

#[test]
fn timestamp_stop_sum_and_wide_intermediate_overflows_are_errors() {
    let mut source = SourceChart::new(1, bpm(60)).unwrap();
    source.objects = vec![object(1, i64::MAX, None)];
    assert_eq!(source.compile(), Err(ChartError::Overflow));
    source.objects = vec![object(1, 1, None)];
    source.stops = vec![Stop {
        beat: beat(0),
        duration: Duration::from_nanos(i64::MAX),
    }];
    assert_eq!(source.compile(), Err(ChartError::Overflow));
    source.objects.clear();
    // A valid maximum STOP is accepted; adding another nanosecond at a
    // distinct tick overflows even when that tick adds zero rounded time.
    source.ticks_per_beat = u32::MAX;
    source.initial_bpm = bpm(u32::MAX);
    assert_eq!(
        source.compile().unwrap().stops()[0].duration.as_nanos(),
        i64::MAX
    );
    source.stops.push(Stop {
        beat: beat(1),
        duration: Duration::from_nanos(1),
    });
    assert_eq!(source.compile(), Err(ChartError::Overflow));
    source.stops.clear();
    source.initial_bpm = Bpm::new(1, u32::MAX).unwrap();
    source.objects = vec![object(1, i64::MAX, None)];
    assert_eq!(source.compile(), Err(ChartError::Overflow));
}

#[test]
fn source_item_cap_is_checked_before_duplicate_processing() {
    let mut source = SourceChart::new(1, bpm(120)).unwrap();
    source.stops = vec![
        Stop {
            beat: beat(0),
            duration: Duration::from_nanos(0)
        };
        MAX_SOURCE_ITEMS + 1
    ];
    assert_eq!(source.compile(), Err(ChartError::TooManyItems));
}
