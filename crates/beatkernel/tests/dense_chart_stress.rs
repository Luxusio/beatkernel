use beatkernel::{chart::*, input::*, interaction::*, judge::*, replay::*, time::*, visual::*};

#[path = "../examples/dense_chart_stress/workload.rs"]
mod workload;
use workload::{generate, make_engine, run, Generated, Options};

const MS: i64 = 1_000_000;
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn rows(options: Options) -> usize {
    options.notes.div_ceil(options.lanes)
}
fn head(options: Options, row: usize) -> i64 {
    options.origin_ns + row as i64 * 4 * MS
}
fn hold_count(options: Options) -> usize {
    (0..options.notes)
        .filter(|i| (i / options.lanes).is_multiple_of(4))
        .count()
}
fn button(record: &ReplayRecord) -> &ButtonEvent {
    let ReplayOperation::Input(input) = &record.operation else {
        panic!("fixture must contain inputs only")
    };
    let PhysicalInputEvent::Button(button) = &input.physical else {
        panic!("fixture must contain physical buttons")
    };
    button
}
fn input(record: &ReplayRecord) -> &GameInputEvent {
    let ReplayOperation::Input(input) = &record.operation else {
        panic!("fixture must contain inputs only")
    };
    input
}

fn expected_meta(at: i64, ordinal: usize, lane: usize) -> EventMeta {
    let native_point = ClockPoint {
        domain: ClockDomainId(8),
        timestamp: ts(at),
    };
    EventMeta {
        source: DeviceId(1),
        timestamp: ts(at),
        clock_domain: ClockDomainId(7),
        sequence: ordinal as u64 + 1,
        native: Some(NativeEventMeta {
            backend: BackendId(2),
            code: Some(4 + lane as u32),
            timestamp: Some(native_point),
        }),
        original_clock_point: Some(native_point),
    }
}

// Expected result order is row heads in lane order, then only hold tails.
// It is derived from chart intent, not from workload verification helpers.
fn golden(options: Options) -> Vec<JudgeEvent> {
    let mut expected = Vec::new();
    let mut cursor = 0;
    for row in 0..rows(options) {
        let width = options.lanes.min(options.notes - row * options.lanes);
        for lane in 0..width {
            expected.push(JudgeEvent {
                object: ObjectId((row * options.lanes + lane + 1) as u64),
                stage: if row.is_multiple_of(4) {
                    JudgeStage::HoldHead
                } else {
                    JudgeStage::Instant
                },
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(1),
                    delta: Duration::ZERO,
                },
                at: ts(head(options, row)),
                input: Some(expected_meta(head(options, row), cursor + lane, lane)),
            });
        }
        if row.is_multiple_of(4) {
            for lane in 0..width {
                expected.push(JudgeEvent {
                    object: ObjectId((row * options.lanes + lane + 1) as u64),
                    stage: JudgeStage::HoldTail,
                    outcome: JudgeOutcome::Hit {
                        grade: JudgeGrade(1),
                        delta: Duration::ZERO,
                    },
                    at: ts(head(options, row) + 2 * MS),
                    input: Some(expected_meta(
                        head(options, row) + 2 * MS,
                        cursor + width + lane,
                        lane,
                    )),
                });
            }
        }
        cursor += 2 * width;
    }
    expected
}

fn check_fixture(options: Options, fixture: &Generated) -> Vec<JudgeEvent> {
    assert_eq!(fixture.chart.objects().len(), options.notes);
    assert_eq!(fixture.records.len(), 2 * options.notes);
    assert_eq!(fixture.hold_count, hold_count(options));
    let objects = fixture.chart.objects();
    for (i, object) in objects.iter().enumerate() {
        let row = i / options.lanes;
        assert_eq!(object.id, ObjectId((i + 1) as u64));
        assert_eq!(object.time.start, ts(head(options, row)));
        assert_eq!(
            object.time.end,
            row.is_multiple_of(4)
                .then(|| ts(head(options, row) + 2 * MS))
        );
        assert_eq!(object.audio, None);
        assert_eq!(object.metadata, ObjectMetadata::default());
        assert_eq!(object.visual, VisualId((i % options.lanes + 1) as u32));
        assert_eq!(
            object.interaction,
            InteractionId(
                (2 * (i % options.lanes) + if row.is_multiple_of(4) { 2 } else { 1 }) as u32
            )
        );
        // A lane/family pair has a single rule; distinct pairs cannot alias.
        for earlier in &objects[..i] {
            let j = earlier.id.0 as usize - 1;
            let same_lane = j % options.lanes == i % options.lanes;
            let same_family = (j / options.lanes).is_multiple_of(4) == row.is_multiple_of(4);
            assert_eq!(
                object.interaction == earlier.interaction,
                same_lane && same_family
            );
            assert_eq!(object.visual == earlier.visual, same_lane);
        }
    }
    let mut cursor = 0;
    let mut owners: Vec<Option<InputOwner>> = vec![None; options.lanes];
    for row in 0..rows(options) {
        let width = options.lanes.min(options.notes - row * options.lanes);
        for half in 0..2 {
            for (lane, owner_slot) in owners.iter_mut().enumerate().take(width) {
                let ordinal = cursor + half * width + lane;
                let record = &fixture.records[ordinal];
                let event = input(record);
                let physical = button(record);
                assert_eq!(record.ordinal, ordinal as u64);
                assert_eq!(
                    record.song_time,
                    ts(head(options, row) + half as i64 * 2 * MS)
                );
                assert_eq!(
                    physical.state,
                    if half == 0 {
                        ButtonState::Down
                    } else {
                        ButtonState::Up
                    }
                );
                assert_eq!(physical.meta.timestamp, record.song_time);
                assert_eq!(
                    physical.meta,
                    expected_meta(record.song_time.as_nanos(), ordinal, lane)
                );
                assert_eq!(
                    physical.control,
                    PhysicalControlId::keyboard(4 + lane as u16)
                );
                assert_eq!(event.game_control, GameControlId((lane + 1) as u32));
                let owner = InputOwner {
                    source: physical.meta.source,
                    physical: physical.control,
                    game_control: event.game_control,
                };
                if let Some(previous) = owner_slot {
                    assert_eq!(
                        owner, *previous,
                        "Down and Up preserve ownership across rows"
                    );
                } else {
                    *owner_slot = Some(owner);
                }
            }
        }
        cursor += 2 * width;
    }
    for a in 0..owners.len() {
        for b in a + 1..owners.len() {
            if let (Some(one), Some(two)) = (owners[a], owners[b]) {
                assert_ne!(one.game_control, two.game_control);
                assert_ne!(one, two);
            }
        }
    }
    for pair in fixture.records.windows(2) {
        assert!(pair[0].song_time <= pair[1].song_time);
        assert!(button(&pair[0]).meta.sequence < button(&pair[1]).meta.sequence);
    }
    let expected = golden(options);
    let mut engine = make_engine(&fixture.chart, options.lanes).unwrap();
    assert_eq!(engine.profile().input_offset(), Duration::ZERO);
    assert_eq!(
        engine.profile().windows(),
        &[JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO
        }]
    );
    let mut actual = Vec::new();
    for record in &fixture.records {
        if button(record).state == ButtonState::Down {
            assert!(engine.is_fresh_press(input(record)));
        }
        actual.extend(engine.push_input(input(record), record.song_time).unwrap());
    }
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), options.notes + hold_count(options));
    for object in objects {
        assert_eq!(engine.state(object.id), Some(InteractionState::Completed));
    }
    for owner in owners.into_iter().flatten() {
        assert!(!engine.is_held(owner));
    }
    expected
}

fn target(options: Options, cycle: usize) -> i64 {
    if cycle % 5 == 4 {
        head(options, rows(options) - 1) + 3 * MS
    } else {
        let hold_row = ((cycle / 5) % rows(options).div_ceil(4)) * 4;
        head(options, hold_row) + (cycle % 5) as i64 * MS
    }
}

fn expected_frame(options: Options, at: i64, cycle: usize) -> Vec<RenderObjectState> {
    let radius = if cycle % 5 == 2 { MS / 2 } else { 4 * MS };
    (0..options.notes)
        .filter_map(|i| {
            let row = i / options.lanes;
            let start = head(options, row);
            let end = if row.is_multiple_of(4) {
                start + 2 * MS
            } else {
                start
            };
            (start <= at + radius && end >= at - radius).then(|| RenderObjectState::Lane {
                object: ObjectId((i + 1) as u64),
                lane: (i % options.lanes) as u32,
                distance: (start - at) as f64 / MS as f64,
                tail_distance: row.is_multiple_of(4).then(|| (end - at) as f64 / MS as f64),
            })
        })
        .collect()
}

fn check_run(options: Options) -> workload::Report {
    let fixture = generate(options).unwrap();
    let expected_results = check_fixture(options, &fixture);
    let report = run(options).unwrap();
    // Wall-clock durations are informational and deliberately excluded from
    // equality/threshold assertions; still inspect every reported phase field.
    let _phase_times = (
        report.setup_ns,
        report.record_ns,
        report.seek_ns,
        report.projection_ns,
        report.verification_ns,
    );
    assert_eq!(report.options, options);
    assert_eq!(report.hold_count, hold_count(options));
    assert_eq!(report.record_count, options.notes * 2);
    assert_eq!(report.result_count, expected_results.len());
    assert!((1..=3).contains(&report.checkpoint_count));
    assert_eq!(report.seek_checks, options.seek_cycles);
    assert_eq!(report.projection_checks, options.seek_cycles);
    let mut completed = make_engine(&fixture.chart, options.lanes).unwrap();
    for record in &fixture.records {
        completed
            .push_input(input(record), record.song_time)
            .unwrap();
    }
    assert_eq!(report.final_engine_hash, completed.stable_hash().unwrap());
    // These fixtures fit in the bounded evidence vector; every requested target
    // must expose the measured state, including actual backwards reconstruction.
    assert_eq!(report.probes.len(), options.seek_cycles);
    let mut max_visible = 0;
    for (cycle, probe) in report.probes.iter().enumerate() {
        let at = target(options, cycle);
        assert_eq!(probe.target_ns, at);
        // Independently replay the original prefix directly into a fresh judge.
        // No ReplaySession, seek, or source oracle is used here.
        let mut engine = make_engine(&fixture.chart, options.lanes).unwrap();
        let mut results = Vec::new();
        let mut cursor = 0;
        for record in &fixture.records {
            if record.song_time > ts(at) {
                break;
            }
            results.extend(engine.push_input(input(record), record.song_time).unwrap());
            cursor += 1;
        }
        if cursor == 0 || fixture.records[cursor - 1].song_time < ts(at) {
            results.extend(engine.advance_to(ts(at)).unwrap());
        }
        let golden_prefix: Vec<_> = expected_results
            .iter()
            .copied()
            .filter(|e| e.at <= ts(at))
            .collect();
        assert_eq!(results, golden_prefix);
        assert_eq!(probe.cursor, cursor);
        assert_eq!(probe.result_count, results.len());
        assert_eq!(probe.engine_hash, engine.stable_hash().unwrap());
        assert_eq!(probe.objects, expected_frame(options, at, cycle));
        for object in &probe.objects {
            let RenderObjectState::Lane {
                distance,
                tail_distance,
                ..
            } = object
            else {
                panic!("expected lane geometry")
            };
            assert!(distance.is_finite());
            assert!(tail_distance.is_none_or(f64::is_finite));
        }
        max_visible = max_visible.max(probe.objects.len());
    }
    assert_eq!(report.max_visible, max_visible);
    report
}

#[test]
fn single_hold_has_exact_head_tail_and_empty_final_ownership() {
    let options = Options {
        notes: 1,
        lanes: 1,
        seek_cycles: 10,
        origin_ns: 0,
    };
    let report = check_run(options);
    assert_eq!(report.hold_count, 1);
    assert_eq!(report.result_count, 2);
    assert_eq!(report.probes[0].result_count, 1);
    assert_eq!(report.probes[1].result_count, 1);
    assert_eq!(report.probes[2].result_count, 2);
    assert_eq!(report.probes[5].result_count, 1);
}

#[test]
fn partial_last_hold_row_and_mixed_chords_match_full_golden() {
    check_run(Options {
        notes: 14,
        lanes: 3,
        seek_cycles: 15,
        origin_ns: 0,
    });
}

#[test]
fn maximum_lane_count_keeps_chord_ownership_and_partial_instant_row() {
    check_run(Options {
        notes: 65,
        lanes: 64,
        seek_cycles: 10,
        origin_ns: 0,
    });
}

#[test]
fn backwards_seeks_rotate_holds_and_tail_window_excludes_head() {
    let options = Options {
        notes: 39,
        lanes: 4,
        seek_cycles: 20,
        origin_ns: 0,
    };
    let report = check_run(options);
    assert!(report
        .probes
        .windows(2)
        .any(|p| p[1].target_ns < p[0].target_ns));
    for cycle in [2, 7, 12, 17] {
        let probe = &report.probes[cycle];
        assert!(!probe.objects.is_empty());
        for object in &probe.objects {
            let RenderObjectState::Lane {
                distance,
                tail_distance,
                ..
            } = object
            else {
                unreachable!()
            };
            assert_eq!(*distance, -2.0);
            assert_eq!(*tail_distance, Some(0.0));
        }
    }
}

#[test]
fn twenty_hour_and_one_week_origins_keep_exact_absolute_time() {
    for origin_ns in [72_000_000_000_000, 604_800_000_000_000] {
        check_run(Options {
            notes: 11,
            lanes: 2,
            seek_cycles: 15,
            origin_ns,
        });
    }
}

#[test]
fn repeat_runs_preserve_every_probe_and_final_hash() {
    let options = Options {
        notes: 23,
        lanes: 3,
        seek_cycles: 15,
        origin_ns: 72_000_000_000_000,
    };
    let one = check_run(options);
    let two = check_run(options);
    assert_eq!(one.final_engine_hash, two.final_engine_hash);
    assert_eq!(one.max_visible, two.max_visible);
    assert_eq!(one.checkpoint_count, two.checkpoint_count);
    for (a, b) in one.probes.iter().zip(&two.probes) {
        assert_eq!(a.target_ns, b.target_ns);
        assert_eq!(a.cursor, b.cursor);
        assert_eq!(a.result_count, b.result_count);
        assert_eq!(a.engine_hash, b.engine_hash);
        assert_eq!(a.objects, b.objects);
    }
    assert_eq!(
        generate(options).unwrap().records,
        generate(options).unwrap().records
    );
}

#[test]
fn admission_rejects_each_range_budget_and_integer_overflow() {
    let base = Options {
        notes: 1,
        lanes: 1,
        seek_cycles: 1,
        origin_ns: 0,
    };
    let invalid = [
        Options { notes: 0, ..base },
        Options {
            notes: 100_001,
            ..base
        },
        Options {
            notes: usize::MAX,
            ..base
        },
        Options { lanes: 0, ..base },
        Options { lanes: 65, ..base },
        Options {
            lanes: usize::MAX,
            ..base
        },
        Options {
            seek_cycles: 0,
            ..base
        },
        Options {
            seek_cycles: 1001,
            ..base
        },
        Options {
            seek_cycles: usize::MAX,
            ..base
        },
        Options {
            origin_ns: -1,
            ..base
        },
        Options {
            origin_ns: 604_800_000_000_001,
            ..base
        },
        Options {
            origin_ns: i64::MAX,
            ..base
        },
        Options {
            notes: 100_000,
            seek_cycles: 50,
            ..base
        },
        Options {
            notes: usize::MAX,
            seek_cycles: usize::MAX,
            ..base
        },
    ];
    for options in invalid {
        assert!(options.validate().is_err(), "admitted {options:?}");
        assert!(generate(options).is_err(), "fixture accepted {options:?}");
        assert!(run(options).is_err(), "run accepted {options:?}");
    }
    assert!(Options {
        notes: 100_000,
        lanes: 64,
        seek_cycles: 49,
        origin_ns: 604_800_000_000_000
    }
    .validate()
    .is_ok());
    assert!(Options {
        notes: 4995,
        lanes: 1,
        seek_cycles: 1000,
        origin_ns: 0
    }
    .validate()
    .is_ok());
    assert!(Options {
        notes: 4996,
        lanes: 1,
        seek_cycles: 1000,
        origin_ns: 0
    }
    .validate()
    .is_err());
}

#[test]
fn defaults_are_the_documented_bounded_fixture() {
    assert_eq!(
        Options::default(),
        Options {
            notes: 20_000,
            lanes: 8,
            seek_cycles: 16,
            origin_ns: 0
        }
    );
    Options::default().validate().unwrap();
}

#[test]
fn indexed_overlap_keeps_full_geometry_and_preallocated_storage() {
    let options = Options {
        notes: 39,
        lanes: 4,
        seek_cycles: 20,
        origin_ns: 604_800_000_000_000,
    };
    let fixture = generate(options).unwrap();
    let projector = VisualProjector::new(
        fixture.chart,
        (0..options.lanes)
            .map(|lane| VisualBinding {
                id: VisualId((lane + 1) as u32),
                projection: Projection::Lane {
                    lane: lane as u32,
                    unit: Duration::from_nanos(MS),
                },
            })
            .collect(),
    )
    .unwrap();
    let mut frame = RenderFrame {
        objects: Vec::with_capacity(4 * options.lanes),
        ..RenderFrame::default()
    };
    let pointer = frame.objects.as_ptr();
    let capacity = frame.objects.capacity();
    for cycle in 0..options.seek_cycles {
        let at = target(options, cycle);
        let radius = if cycle % 5 == 2 { MS / 2 } else { 4 * MS };
        projector
            .project(ts(at), ts(at - radius), ts(at + radius), &[], &mut frame)
            .unwrap();
        assert_eq!(frame.objects, expected_frame(options, at, cycle));
        assert_eq!(frame.objects.as_ptr(), pointer);
        assert_eq!(frame.objects.capacity(), capacity);
        assert!(frame.transient_events.is_empty());
    }
}
