use beatkernel::{chart::*, time::*, visual::*};

fn source() -> SourceChart {
    let mut chart = SourceChart::new(1, Bpm::new(60, 1).unwrap()).unwrap();
    chart.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(0).unwrap(),
        end: Some(Beat::new(100).unwrap()),
        interaction: InteractionId(1),
        visual: VisualId(1),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    chart
}

#[test]
fn overlapping_long_path_survives_late_window_and_reuses_storage() {
    let projector = VisualProjector::new(
        source().compile().unwrap(),
        vec![VisualBinding {
            id: VisualId(1),
            projection: Projection::Path {
                points: vec![[0.0, 0.0], [1.0, 0.0]],
            },
        }],
    )
    .unwrap();
    let mut frame = RenderFrame {
        objects: Vec::with_capacity(4),
        ..RenderFrame::default()
    };
    let pointer = frame.objects.as_ptr();
    projector
        .project(
            Timestamp::from_nanos(50_000_000_000),
            Timestamp::from_nanos(49_000_000_000),
            Timestamp::from_nanos(51_000_000_000),
            &[],
            &mut frame,
        )
        .unwrap();
    assert_eq!(frame.objects.as_ptr(), pointer);
    assert_eq!(
        frame.objects,
        vec![RenderObjectState::Path {
            object: ObjectId(1),
            visual: VisualId(1),
            head: [0.5, 0.0],
            progress: 0.5,
            visible_start: 0.49,
            visible_end: 0.51
        }]
    );
}

#[test]
fn signed_and_zero_scroll_do_not_change_judge_targets() {
    let mut chart = source();
    chart.scroll_changes.push(ScrollChange {
        beat: Beat::new(1).unwrap(),
        velocity: ScrollVelocity::new(0, 1).unwrap(),
    });
    chart.scroll_changes.push(ScrollChange {
        beat: Beat::new(2).unwrap(),
        velocity: ScrollVelocity::new(-1, 1).unwrap(),
    });
    let compiled = chart.compile().unwrap();
    assert_eq!(
        compiled.objects()[0].time.end,
        Some(Timestamp::from_nanos(100_000_000_000))
    );
    let projector = VisualProjector::new(
        compiled,
        vec![VisualBinding {
            id: VisualId(1),
            projection: Projection::Lane {
                lane: 7,
                unit: Duration::from_nanos(1_000_000_000),
            },
        }],
    )
    .unwrap();
    let mut frame = RenderFrame::default();
    projector
        .project(
            Timestamp::from_nanos(3_000_000_000),
            Timestamp::ZERO,
            Timestamp::from_nanos(4_000_000_000),
            &[],
            &mut frame,
        )
        .unwrap();
    assert_eq!(
        frame.objects,
        vec![RenderObjectState::Lane {
            object: ObjectId(1),
            lane: 7,
            distance: 0.0,
            tail_distance: Some(-97.0)
        }]
    );
}

#[test]
fn invalid_geometry_and_reversed_windows_are_explicit() {
    assert!(matches!(
        VisualProjector::new(
            source().compile().unwrap(),
            vec![VisualBinding {
                id: VisualId(1),
                projection: Projection::Path {
                    points: vec![[f64::NAN, 0.0], [0.0, 0.0]]
                }
            }]
        ),
        Err(VisualError::InvalidGeometry)
    ));
    let projector = VisualProjector::new(
        source().compile().unwrap(),
        vec![VisualBinding {
            id: VisualId(1),
            projection: Projection::Lane {
                lane: 0,
                unit: Duration::from_nanos(1),
            },
        }],
    )
    .unwrap();
    let mut frame = RenderFrame::default();
    let before = frame.clone();
    assert_eq!(
        projector.project(
            Timestamp::ZERO,
            Timestamp::from_nanos(1),
            Timestamp::ZERO,
            &[],
            &mut frame
        ),
        Err(VisualError::ReversedWindow)
    );
    assert_eq!(frame, before);
}

#[test]
fn radial_approach_has_clamped_boundaries_and_finite_logical_radius() {
    let mut chart = source();
    chart.objects[0].start = Beat::new(100).unwrap();
    chart.objects[0].end = None;
    chart.scroll_changes.push(ScrollChange {
        beat: Beat::new(1).unwrap(),
        velocity: ScrollVelocity::new(0, 1).unwrap(),
    });
    chart.scroll_changes.push(ScrollChange {
        beat: Beat::new(2).unwrap(),
        velocity: ScrollVelocity::new(-1, 1).unwrap(),
    });
    let projector = VisualProjector::new(
        chart.compile().unwrap(),
        vec![VisualBinding {
            id: VisualId(1),
            projection: Projection::Polar {
                center: [4.0, 5.0],
                angle: std::f64::consts::FRAC_PI_2,
                radius: 2.0,
                approach_radius: 10.0,
                approach: Duration::from_nanos(20_000_000_000),
            },
        }],
    )
    .unwrap();
    let mut frame = RenderFrame::default();
    for (at, progress, radius) in [
        (70, 0.0, 10.0),
        (80, 0.0, 10.0),
        (90, 0.5, 6.0),
        (100, 1.0, 2.0),
        (110, 1.0, 2.0),
    ] {
        projector
            .project(
                Timestamp::from_nanos(at * 1_000_000_000),
                Timestamp::ZERO,
                Timestamp::from_nanos(110_000_000_000),
                &[],
                &mut frame,
            )
            .unwrap();
        assert_eq!(
            frame.objects,
            vec![RenderObjectState::Polar {
                object: ObjectId(1),
                center: [4.0, 5.0],
                angle: std::f64::consts::FRAC_PI_2,
                radius,
                approach_progress: progress
            }]
        );
    }
    for (angle, radius, approach_radius, approach) in [
        (f64::NAN, 1.0, 2.0, 1),
        (0.0, -1.0, 2.0, 1),
        (0.0, 1.0, f64::INFINITY, 1),
        (0.0, 1.0, 2.0, 0),
    ] {
        assert!(matches!(
            VisualProjector::new(
                source().compile().unwrap(),
                vec![VisualBinding {
                    id: VisualId(1),
                    projection: Projection::Polar {
                        center: [0.0, 0.0],
                        angle,
                        radius,
                        approach_radius,
                        approach: Duration::from_nanos(approach)
                    }
                }]
            ),
            Err(VisualError::InvalidGeometry)
        ));
    }
}

#[derive(Debug)]
struct Target3D {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    reject_setup: bool,
    failure: u8,
}
impl CustomProjection for Target3D {
    fn validate(&self, object: &TimedObject) -> Result<(), VisualError> {
        if self.reject_setup || object.time.end.is_none_or(|end| end <= object.time.start) {
            Err(VisualError::InvalidGeometry)
        } else {
            Ok(())
        }
    }
    fn project(
        &self,
        context: CustomProjectionContext<'_>,
    ) -> Result<CustomRenderState, VisualError> {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let elapsed = (i128::from(context.song_time.as_nanos())
            - i128::from(context.object.time.start.as_nanos())) as f64;
        let duration = (i128::from(context.object.time.end.unwrap().as_nanos())
            - i128::from(context.object.time.start.as_nanos())) as f64;
        let progress = (elapsed / duration).clamp(0.0, 1.0);
        let mut state = CustomRenderState {
            object: context.object.id,
            visual: context.object.visual,
            type_tag: 7,
            geometry_ref: 44,
            values: [0.0; 16],
            value_count: 7,
            progress,
        };
        state.values[..3].copy_from_slice(&[progress, progress * 2.0, progress * 3.0]);
        state.values[6] = 1.0;
        match self.failure {
            1 => state.object = ObjectId(u64::MAX),
            2 => state.visual = VisualId(u32::MAX),
            3 => state.values[15] = f64::NAN,
            4 => state.value_count = 17,
            5 => state.progress = 1.1,
            6 => return Err(VisualError::InvalidGeometry),
            7 => state.progress = f64::NAN,
            _ => {}
        }
        Ok(state)
    }
}
fn custom(
    failure: u8,
) -> (
    CustomProjectionHandle,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    (
        CustomProjectionHandle::new(Target3D {
            calls: std::sync::Arc::clone(&calls),
            reject_setup: false,
            failure,
        }),
        calls,
    )
}

#[test]
fn custom_3d_projection_preserves_geometry_reference_and_reuses_frame_storage() {
    let (callback, calls) = custom(0);
    let projector = VisualProjector::new(
        source().compile().unwrap(),
        vec![VisualBinding {
            id: VisualId(1),
            projection: Projection::Custom {
                projection: callback.clone(),
            },
        }],
    )
    .unwrap();
    let cloned = projector.clone();
    assert_eq!(
        projector.projection(VisualId(1)),
        cloned.projection(VisualId(1))
    );
    let (different, _) = custom(0);
    assert_ne!(callback, different); // Equality is genuine shared allocation identity.
    let mut frame = RenderFrame {
        objects: Vec::with_capacity(8),
        transient_events: Vec::with_capacity(8),
        ..RenderFrame::default()
    };
    let storage = frame.objects.as_ptr();
    for projector in [&projector, &cloned] {
        projector
            .project(
                Timestamp::from_nanos(50_000_000_000),
                Timestamp::from_nanos(49_000_000_000),
                Timestamp::from_nanos(51_000_000_000),
                &[],
                &mut frame,
            )
            .unwrap();
        assert_eq!(frame.objects.as_ptr(), storage);
        let RenderObjectState::Custom(state) = frame.objects[0] else {
            panic!("custom projection lost");
        };
        assert_eq!(
            (
                state.object,
                state.visual,
                state.type_tag,
                state.geometry_ref,
                state.value_count
            ),
            (ObjectId(1), VisualId(1), 7, 44, 7)
        );
        assert_eq!(&state.values[..7], &[0.5, 1.0, 1.5, 0.0, 0.0, 0.0, 1.0]);
        assert_eq!(state.progress, 0.5);
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 2);
}

#[test]
fn narrow_window_invokes_custom_projection_only_for_indexed_overlap() {
    let mut chart = SourceChart::new(1, Bpm::new(60, 1).unwrap()).unwrap();
    for id in 1..=1000 {
        let start = (id as i64 - 1) * 100;
        chart.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(start).unwrap(),
            end: Some(Beat::new(start + 10).unwrap()),
            interaction: InteractionId(1),
            visual: VisualId(1),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let (callback, calls) = custom(0);
    let projector = VisualProjector::new(
        chart.compile().unwrap(),
        vec![VisualBinding {
            id: VisualId(1),
            projection: Projection::Custom {
                projection: callback,
            },
        }],
    )
    .unwrap();
    let mut frame = RenderFrame::default();
    projector
        .project(
            Timestamp::from_nanos(505_000_000_000),
            Timestamp::from_nanos(504_000_000_000),
            Timestamp::from_nanos(506_000_000_000),
            &[],
            &mut frame,
        )
        .unwrap();
    assert_eq!(frame.objects.len(), 1);
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    let RenderObjectState::Custom(state) = frame.objects[0] else {
        panic!("custom projection lost");
    };
    assert_eq!(state.object, ObjectId(6));
    assert_eq!(state.progress, 0.5);
}

#[test]
fn invalid_custom_output_or_callback_error_clears_partial_frame_and_transients() {
    let mut chart = source();
    chart.objects.push(SourceObject {
        id: ObjectId(2),
        start: Beat::new(1).unwrap(),
        end: Some(Beat::new(2).unwrap()),
        interaction: InteractionId(1),
        visual: VisualId(2),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    for failure in 1..=7 {
        let (callback, _) = custom(failure);
        let projector = VisualProjector::new(
            chart.compile().unwrap(),
            vec![
                VisualBinding {
                    id: VisualId(1),
                    projection: Projection::Lane {
                        lane: 1,
                        unit: Duration::from_nanos(1_000_000_000),
                    },
                },
                VisualBinding {
                    id: VisualId(2),
                    projection: Projection::Custom {
                        projection: callback,
                    },
                },
            ],
        )
        .unwrap();
        let mut frame = RenderFrame {
            objects: vec![RenderObjectState::Point {
                object: ObjectId(99),
                position: [0.0, 0.0],
                approach_progress: 0.0,
            }],
            transient_events: vec![beatkernel::judge::JudgeEvent {
                object: ObjectId(99),
                stage: beatkernel::judge::JudgeStage::Instant,
                outcome: beatkernel::judge::JudgeOutcome::Miss {
                    reason: beatkernel::judge::MissReason::HeadTimeout,
                },
                at: Timestamp::ZERO,
                input: None,
            }],
            ..RenderFrame::default()
        };
        let error = projector
            .project(
                Timestamp::ZERO,
                Timestamp::ZERO,
                Timestamp::from_nanos(2_000_000_000),
                &[],
                &mut frame,
            )
            .unwrap_err();
        assert_eq!(
            error,
            match failure {
                1 | 2 => VisualError::CustomIdentityMismatch(ObjectId(2)),
                3 | 7 => VisualError::Overflow,
                4 | 5 => VisualError::InvalidCustomState(ObjectId(2)),
                _ => VisualError::InvalidGeometry,
            }
        );
        assert!(frame.objects.is_empty());
        assert!(frame.transient_events.is_empty());
    }
}

#[test]
fn custom_setup_validation_happens_before_projector_construction() {
    let callback = CustomProjectionHandle::new(Target3D {
        calls: Default::default(),
        reject_setup: true,
        failure: 0,
    });
    assert!(matches!(
        VisualProjector::new(
            source().compile().unwrap(),
            vec![VisualBinding {
                id: VisualId(1),
                projection: Projection::Custom {
                    projection: callback
                }
            }]
        ),
        Err(VisualError::InvalidGeometry)
    ));
}
