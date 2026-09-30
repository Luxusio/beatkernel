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
