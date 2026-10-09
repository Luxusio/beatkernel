use beatkernel::{
    audio::{command_queue, CommandConsumer},
    chart::*,
    input::*,
    interaction::InstantEvaluator,
    judge::*,
    replay::*,
    runtime::{Runtime, RuntimeError},
    time::*,
    transport::{Rate, Transport},
    visual::*,
};

const HOUR: i64 = 3_600_000_000_000;
const WEEK: i64 = 168 * HOUR;
const MS: i64 = 1_000_000;

fn ts(nanos: i64) -> Timestamp {
    Timestamp::from_nanos(nanos)
}

fn object(id: u64, start: i64, end: Option<i64>, visual: u32) -> SourceObject {
    SourceObject {
        id: ObjectId(id),
        start: Beat::new(start).unwrap(),
        end: end.map(|n| Beat::new(n).unwrap()),
        interaction: InteractionId(1),
        visual: VisualId(visual),
        audio: None,
        metadata: ObjectMetadata::default(),
    }
}

fn source() -> SourceChart {
    SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-12,
        "actual {actual}, expected {expected}"
    );
}

#[test]
fn lane_local_and_cross_segment_displacement_survive_huge_prior_scroll() {
    for horizon in [20 * HOUR, WEEK] {
        for crossed in [false, true] {
            let mut chart = source();
            chart.objects = vec![object(1, 0, None, 1), object(2, horizon + 2 * MS, None, 1)];
            chart.scroll_changes = vec![
                ScrollChange {
                    beat: Beat::new(0).unwrap(),
                    velocity: ScrollVelocity::new(1_000_000_000, 1).unwrap(),
                },
                ScrollChange {
                    beat: Beat::new(horizon).unwrap(),
                    velocity: ScrollVelocity::new(1, 3).unwrap(),
                },
            ];
            if crossed {
                chart.scroll_changes.push(ScrollChange {
                    beat: Beat::new(horizon + MS).unwrap(),
                    velocity: ScrollVelocity::new(-2, 3).unwrap(),
                });
            } else {
                chart.objects[1].start = Beat::new(horizon + MS).unwrap();
            }
            let projector = VisualProjector::new(
                chart.compile().unwrap(),
                vec![VisualBinding {
                    id: VisualId(1),
                    projection: Projection::Lane {
                        lane: 0,
                        unit: Duration::from_nanos(MS),
                    },
                }],
            )
            .unwrap();
            let mut frame = RenderFrame::default();
            projector
                .project(
                    ts(horizon),
                    ts(horizon),
                    ts(horizon + 3 * MS),
                    &[],
                    &mut frame,
                )
                .unwrap();
            let RenderObjectState::Lane { distance, .. } = frame.objects[0] else {
                panic!("expected lane")
            };
            near(distance, if crossed { -1.0 / 3.0 } else { 1.0 / 3.0 });
        }
    }
}

#[test]
fn lane_non_power_of_two_marker_ranges_exclude_unrelated_history() {
    for marker_count in [5usize, 6, 9, 17] {
        let mut chart = source();
        chart.scroll_changes.push(ScrollChange {
            beat: Beat::new(0).unwrap(),
            velocity: ScrollVelocity::new(1_000_000_000, 1).unwrap(),
        });
        for index in 0..marker_count {
            chart.scroll_changes.push(ScrollChange {
                beat: Beat::new(WEEK + index as i64 * MS).unwrap(),
                velocity: ScrollVelocity::new(if index % 2 == 0 { 1 } else { -2 }, 3).unwrap(),
            });
            chart
                .objects
                .push(object(index as u64 + 2, WEEK + index as i64 * MS, None, 1));
        }
        chart.objects.push(object(1, 0, None, 1));
        let projector = VisualProjector::new(
            chart.compile().unwrap(),
            vec![VisualBinding {
                id: VisualId(1),
                projection: Projection::Lane {
                    lane: 0,
                    unit: Duration::from_nanos(MS),
                },
            }],
        )
        .unwrap();
        let mut frame = RenderFrame::default();
        // Every range, including interior-only and reversed queries, is compared
        // to the integer sum of local thirds. No cumulative history is an oracle.
        for start in 0..marker_count {
            projector
                .project(
                    ts(WEEK + start as i64 * MS),
                    ts(WEEK),
                    ts(WEEK + marker_count as i64 * MS),
                    &[],
                    &mut frame,
                )
                .unwrap();
            for state in &frame.objects {
                let RenderObjectState::Lane {
                    object, distance, ..
                } = *state
                else {
                    panic!("lane")
                };
                let end = object.0 as usize - 2;
                let (first, last, sign) = if end >= start {
                    (start, end, 1)
                } else {
                    (end, start, -1)
                };
                let thirds: i64 = (first..last).map(|i| if i % 2 == 0 { 1 } else { -2 }).sum();
                near(distance, (thirds * sign) as f64 / 3.0);
            }
        }
    }
}

#[test]
fn signed_zero_scroll_preroll_and_hold_endpoints_use_local_integrals() {
    let mut chart = source();
    chart.objects = vec![
        object(1, 0, None, 1),
        object(2, WEEK + 3 * MS, Some(WEEK + 4 * MS), 1),
    ];
    chart.scroll_changes = [(WEEK, 0, 1), (WEEK + MS, -1, 2), (WEEK + 2 * MS, 2, 3)]
        .into_iter()
        .map(|(at, n, d)| ScrollChange {
            beat: Beat::new(at).unwrap(),
            velocity: ScrollVelocity::new(n, d).unwrap(),
        })
        .collect();
    let compiled = chart.compile().unwrap();
    assert_eq!(
        compiled.objects()[1].time,
        TimeRange {
            start: ts(WEEK + 3 * MS),
            end: Some(ts(WEEK + 4 * MS))
        }
    );
    let projector = VisualProjector::new(
        compiled,
        vec![VisualBinding {
            id: VisualId(1),
            projection: Projection::Lane {
                lane: 4,
                unit: Duration::from_nanos(MS),
            },
        }],
    )
    .unwrap();
    let mut frame = RenderFrame::default();
    projector
        .project(ts(-MS), ts(-MS), ts(0), &[], &mut frame)
        .unwrap();
    assert_eq!(
        frame.objects,
        vec![RenderObjectState::Lane {
            object: ObjectId(1),
            lane: 4,
            distance: 1.0,
            tail_distance: None
        }]
    );
    for (song, head, tail) in [
        (WEEK, 1.0 / 6.0, 5.0 / 6.0),
        (WEEK + MS, 1.0 / 6.0, 5.0 / 6.0),
        (WEEK + 4 * MS, -2.0 / 3.0, 0.0),
    ] {
        projector
            .project(ts(song), ts(WEEK), ts(WEEK + 5 * MS), &[], &mut frame)
            .unwrap();
        let RenderObjectState::Lane {
            distance,
            tail_distance,
            ..
        } = frame.objects[0]
        else {
            panic!("lane")
        };
        near(distance, head);
        near(tail_distance.unwrap(), tail);
    }
}

#[test]
fn point_polar_and_true_long_path_keep_local_precision_and_tail_overlap() {
    for horizon in [20 * HOUR, WEEK] {
        let mut chart = source();
        chart.objects = vec![
            object(1, 0, Some(horizon + 2 * MS), 3),
            object(2, horizon + MS, None, 1),
            object(3, horizon + MS, None, 2),
        ];
        let projector = VisualProjector::new(
            chart.compile().unwrap(),
            vec![
                VisualBinding {
                    id: VisualId(1),
                    projection: Projection::Point {
                        position: [2.0, 3.0],
                        approach: Duration::from_nanos(2 * MS),
                    },
                },
                VisualBinding {
                    id: VisualId(2),
                    projection: Projection::Polar {
                        center: [4.0, 5.0],
                        angle: 0.0,
                        radius: 2.0,
                        approach_radius: 10.0,
                        approach: Duration::from_nanos(2 * MS),
                    },
                },
                VisualBinding {
                    id: VisualId(3),
                    projection: Projection::Path {
                        points: vec![[0.0, 0.0], [1.0, 2.0]],
                    },
                },
            ],
        )
        .unwrap();
        let mut frame = RenderFrame::default();
        for (song, progress, radius) in [
            (horizon - MS, 0.0, 10.0),
            (horizon, 0.5, 6.0),
            (horizon + MS, 1.0, 2.0),
            (horizon + 2 * MS, 1.0, 2.0),
        ] {
            projector
                .project(ts(song), ts(horizon), ts(horizon + 2 * MS), &[], &mut frame)
                .unwrap();
            assert_eq!(frame.objects.len(), 3);
            assert_eq!(
                frame.objects[1],
                RenderObjectState::Point {
                    object: ObjectId(2),
                    position: [2.0, 3.0],
                    approach_progress: progress
                }
            );
            assert_eq!(
                frame.objects[2],
                RenderObjectState::Polar {
                    object: ObjectId(3),
                    center: [4.0, 5.0],
                    angle: 0.0,
                    radius,
                    approach_progress: progress
                }
            );
            let RenderObjectState::Path {
                head,
                progress,
                visible_start,
                visible_end,
                ..
            } = frame.objects[0]
            else {
                panic!("path")
            };
            let expected = song as f64 / (horizon + 2 * MS) as f64;
            near(progress, expected);
            near(head[0], expected);
            near(head[1], 2.0 * expected);
            near(visible_start, horizon as f64 / (horizon + 2 * MS) as f64);
            near(visible_end, 1.0);
        }
    }
}

#[test]
fn dense_offwindow_objects_and_scroll_markers_reuse_prepared_frame_storage() {
    let mut chart = source();
    for id in 0..4096u64 {
        let at = id as i64 * (WEEK / 8192);
        chart.objects.push(object(id + 1, at, None, 1));
        chart.scroll_changes.push(ScrollChange {
            beat: Beat::new(at).unwrap(),
            velocity: ScrollVelocity::new(if id % 2 == 0 { 1 } else { -1 }, 3).unwrap(),
        });
    }
    chart.scroll_changes.push(ScrollChange {
        beat: Beat::new(WEEK).unwrap(),
        velocity: ScrollVelocity::new(1, 3).unwrap(),
    });
    chart
        .objects
        .push(object(5000, WEEK + MS, Some(WEEK + 2 * MS), 1));
    let projector = VisualProjector::new(
        chart.compile().unwrap(),
        vec![VisualBinding {
            id: VisualId(1),
            projection: Projection::Lane {
                lane: 1,
                unit: Duration::from_nanos(MS),
            },
        }],
    )
    .unwrap();
    let mut frame = RenderFrame {
        objects: Vec::with_capacity(4),
        transient_events: Vec::with_capacity(4),
        ..RenderFrame::default()
    };
    let pointers = (frame.objects.as_ptr(), frame.transient_events.as_ptr());
    let capacities = (frame.objects.capacity(), frame.transient_events.capacity());
    for song in [WEEK, WEEK + MS, WEEK + 2 * MS, WEEK + MS, WEEK] {
        projector
            .project(ts(song), ts(WEEK), ts(WEEK + 3 * MS), &[], &mut frame)
            .unwrap();
        assert_eq!(frame.objects.len(), 1);
        let RenderObjectState::Lane {
            object,
            distance,
            tail_distance,
            ..
        } = frame.objects[0]
        else {
            panic!("lane")
        };
        assert_eq!(object, ObjectId(5000));
        near(distance, (WEEK + MS - song) as f64 / MS as f64 / 3.0);
        near(
            tail_distance.unwrap(),
            (WEEK + 2 * MS - song) as f64 / MS as f64 / 3.0,
        );
        assert_eq!(
            (frame.objects.as_ptr(), frame.transient_events.as_ptr()),
            pointers
        );
        assert_eq!(
            (frame.objects.capacity(), frame.transient_events.capacity()),
            capacities
        );
    }
}

fn point(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(1),
        timestamp: ts(nanos),
    }
}

struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

fn engine(chart: CompiledChart) -> JudgeEngine {
    JudgeEngine::new(
        chart,
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(InstantEvaluator),
        }],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap()
}

fn runtime(chart: CompiledChart) -> (Runtime, CommandConsumer) {
    let bindings = BindingMap::from_bindings((1..=3).map(|key| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(key),
        game_control: GameControlId(1),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(4).unwrap();
    (
        Runtime::new(
            ClockDomainId(1),
            ClockDomainId(1),
            Transport::new(ts(0), ts(0), Rate::NORMAL),
            bindings,
            engine(chart),
            producer,
            vec![],
            4,
        )
        .unwrap(),
        consumer,
    )
}

fn input(at: i64, key: u16) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(1), point(at), u64::from(key)),
        control: PhysicalControlId::keyboard(key),
        state: ButtonState::Down,
    })
}

fn header() -> ReplayHeader {
    ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"longextent".to_vec(),
        rules_identity: b"instant/zero".to_vec(),
        options: vec![],
        seed: 0,
        normalized_clock: ClockDomainId(1),
    }
}

fn project_runtime_time(
    projector: &VisualProjector,
    song: Timestamp,
    events: &[JudgeEvent],
    targets: &[i64; 3],
    frame: &mut RenderFrame,
) {
    let pointer = frame.objects.as_ptr();
    let capacity = frame.objects.capacity();
    let start = song.as_nanos() - MS;
    let end = song.as_nanos() + MS;
    projector
        .project(song, ts(start), ts(end), events, frame)
        .unwrap();
    assert_eq!(frame.song_time, song);
    assert_eq!(frame.transient_events, events);
    let expected_ids: Vec<_> = targets
        .iter()
        .enumerate()
        .filter(|(_, at)| **at >= start && **at <= end)
        .map(|(i, _)| ObjectId(i as u64 + 1))
        .collect();
    assert_eq!(frame.objects.len(), expected_ids.len());
    for (state, expected_id) in frame.objects.iter().zip(expected_ids) {
        let RenderObjectState::Lane {
            object,
            distance,
            tail_distance,
            ..
        } = *state
        else {
            panic!("lane")
        };
        assert_eq!(object, expected_id);
        assert_eq!(tail_distance, None);
        // Independent local integer displacement, not scroll prefix arithmetic.
        near(
            distance,
            (targets[object.0 as usize - 1] - song.as_nanos()) as f64 / MS as f64,
        );
    }
    assert_eq!(frame.objects.as_ptr(), pointer);
    assert_eq!(frame.objects.capacity(), capacity);
}

#[test]
fn true_longextent_runtime_judge_and_replay_are_independent_of_frame_cadence() {
    for horizon in [20 * HOUR, WEEK] {
        let targets = [0, horizon, horizon + MS];
        let mut chart = source();
        chart.objects = targets
            .into_iter()
            .enumerate()
            .map(|(i, at)| object(i as u64 + 1, at, None, 1))
            .collect();
        let compiled = chart.compile().unwrap();
        assert_eq!(
            compiled
                .objects()
                .iter()
                .map(|o| o.time.start.as_nanos())
                .collect::<Vec<_>>(),
            targets
        );
        let mut final_hash = None;
        for fine in [false, true] {
            let projector = VisualProjector::new(
                compiled.clone(),
                vec![VisualBinding {
                    id: VisualId(1),
                    projection: Projection::Lane {
                        lane: 0,
                        unit: Duration::from_nanos(MS),
                    },
                }],
            )
            .unwrap();
            let mut frame = RenderFrame {
                objects: Vec::with_capacity(3),
                transient_events: Vec::with_capacity(3),
                ..RenderFrame::default()
            };
            let (mut live, _consumer) = runtime(compiled.clone());
            let mut replay = ReplaySession::new(header(), engine(compiled.clone())).unwrap();
            let mut events = vec![];
            for (i, at) in targets.into_iter().enumerate() {
                if fine && i > 0 {
                    let previous = i128::from(targets[i - 1]);
                    let interval = i128::from(at) - previous;
                    let probes = (1..=128)
                        .map(|step| i64::try_from(previous + interval * step / 129).unwrap())
                        .chain(std::iter::once(at - 1));
                    for probe in probes {
                        let report = live
                            .advance_to(point(probe), &SameDomain, point(probe))
                            .unwrap();
                        assert_eq!(report.song_time, ts(probe));
                        assert!(report.judge_events.is_empty());
                        project_runtime_time(
                            &projector,
                            report.song_time,
                            &report.judge_events,
                            &targets,
                            &mut frame,
                        );
                        replay.advance_to(ts(probe)).unwrap();
                    }
                }
                let report = live
                    .process_input(input(at, i as u16 + 1), &SameDomain, point(at))
                    .unwrap();
                assert_eq!(report.song_time, ts(at));
                assert_eq!(report.judge_error, None);
                assert_eq!(report.judge_events.len(), 1);
                assert_eq!(report.judge_events[0].object, ObjectId(i as u64 + 1));
                assert_eq!(report.judge_events[0].at, ts(at));
                assert_eq!(
                    report.judge_events[0].outcome,
                    JudgeOutcome::Hit {
                        grade: JudgeGrade(7),
                        delta: Duration::ZERO
                    }
                );
                project_runtime_time(
                    &projector,
                    report.song_time,
                    &report.judge_events,
                    &targets,
                    &mut frame,
                );
                assert_eq!(
                    replay
                        .push_input(report.bound_inputs[0].clone(), ts(at))
                        .unwrap(),
                    report.judge_events
                );
                events.extend(report.judge_events);
            }
            live.advance_to(
                point(horizon + MS + 1),
                &SameDomain,
                point(horizon + MS + 1),
            )
            .unwrap();
            replay.advance_to(ts(horizon + MS + 1)).unwrap();
            assert_eq!(replay.results(), events);
            let hash = live.judge().stable_hash().unwrap();
            assert_eq!(replay.engine().stable_hash().unwrap(), hash);
            if let Some(previous) = final_hash {
                assert_eq!(hash, previous);
            }
            final_hash = Some(hash);
            for target in [horizon, 0, horizon, horizon + MS + 1, horizon, 0] {
                replay.seek(ts(target)).unwrap();
                let prefix: Vec<_> = events
                    .iter()
                    .copied()
                    .filter(|e| e.at <= ts(target))
                    .collect();
                assert_eq!(replay.results(), prefix);
                assert_eq!(replay.engine().effective_song_time(), Some(ts(target)));
                project_runtime_time(
                    &projector,
                    replay.engine().effective_song_time().unwrap(),
                    replay.results(),
                    &targets,
                    &mut frame,
                );
                let mut direct = RenderFrame::default();
                projector
                    .project(
                        ts(target),
                        ts(target - MS),
                        ts(target + MS),
                        &prefix,
                        &mut direct,
                    )
                    .unwrap();
                assert_eq!(frame, direct);
            }
            // Direct reverse/decreasing continuation cannot mutate the judge.
            let host = horizon + MS + 1;
            live.transport_mut()
                .set_rate(ts(host), Rate::REVERSE)
                .unwrap();
            assert_eq!(
                live.advance_to(point(host + 1), &SameDomain, point(host + 1))
                    .unwrap_err(),
                RuntimeError::RequiresReplayRestore
            );
            assert_eq!(live.judge().stable_hash().unwrap(), hash);
            live.transport_mut()
                .seek(ts(host + 2), ts(horizon))
                .unwrap();
            live.transport_mut()
                .set_rate(ts(host + 2), Rate::NORMAL)
                .unwrap();
            assert_eq!(
                live.advance_to(point(host + 2), &SameDomain, point(host + 2))
                    .unwrap_err(),
                RuntimeError::RequiresReplayRestore
            );
            assert_eq!(live.judge().stable_hash().unwrap(), hash);
            replay.seek(ts(horizon)).unwrap();
            let restored =
                JudgeEngine::from_snapshot(&replay.engine().snapshot().unwrap()).unwrap();
            live.replace_state(
                restored,
                Transport::new(ts(host + 2), ts(horizon), Rate::NORMAL),
            );
            let resumed = live
                .advance_to(point(host + 2), &SameDomain, point(host + 2))
                .unwrap();
            assert_eq!(resumed.song_time, ts(horizon));
            project_runtime_time(
                &projector,
                resumed.song_time,
                &resumed.judge_events,
                &targets,
                &mut frame,
            );
            assert_eq!(
                live.judge().stable_hash().unwrap(),
                replay.engine().stable_hash().unwrap()
            );
        }
    }
}

#[test]
fn rational_transport_pause_and_equal_time_commands_preserve_long_positions() {
    for horizon in [20 * HOUR, WEEK] {
        let mut transport = Transport::new(ts(0), ts(0), Rate::new(2, 3).unwrap());
        let host = horizon * 3 / 2;
        for probe in [0, 1, 2, host / 2, host] {
            assert_eq!(transport.position_at(ts(probe)).unwrap(), ts(probe * 2 / 3));
        }
        transport.pause(ts(host)).unwrap();
        transport.pause(ts(host)).unwrap();
        assert_eq!(transport.position_at(ts(host + HOUR)).unwrap(), ts(horizon));
        transport.resume(ts(host + HOUR)).unwrap();
        transport.resume(ts(host + HOUR)).unwrap();
        assert_eq!(
            transport.position_at(ts(host + HOUR + 3)).unwrap(),
            ts(horizon + 2)
        );
        transport
            .set_rate(ts(host + HOUR + 3), Rate::REVERSE)
            .unwrap();
        assert_eq!(
            transport.position_at(ts(host + HOUR + 5)).unwrap(),
            ts(horizon)
        );
        assert_eq!(
            transport.position_at(ts(host - 1)).unwrap(),
            ts(horizon - 1)
        );
    }
}

#[test]
fn longextent_bpm_and_stop_targets_are_exact_pre_stop_integer_times() {
    for horizon in [20 * HOUR, WEEK] {
        let mut chart = source();
        chart.bpm_changes.push(BpmChange {
            beat: Beat::new(horizon).unwrap(),
            bpm: Bpm::new(120, 1).unwrap(),
        });
        chart.stops.push(Stop {
            beat: Beat::new(horizon).unwrap(),
            duration: Duration::from_nanos(7),
        });
        chart.objects = vec![
            object(1, 0, Some(horizon + 2 * MS), 1),
            object(2, horizon, None, 1),
            object(3, horizon + 2 * MS, None, 1),
        ];
        let compiled = chart.compile().unwrap();
        assert_eq!(compiled.bpm_changes()[0].time, ts(horizon));
        assert_eq!(compiled.objects()[0].time.end, Some(ts(horizon + MS + 7)));
        assert_eq!(compiled.objects()[1].time.start, ts(horizon));
        assert_eq!(compiled.objects()[2].time.start, ts(horizon + MS + 7));
    }
}
