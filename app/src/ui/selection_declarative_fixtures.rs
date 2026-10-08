use super::*;

fn frame(selected: usize) -> SelectionFrame {
    SelectionFrame {
        selected,
        hovered: None,
        armed: None,
        error: None,
        backend_pending: false,
    }
}

fn view(count: usize) -> SelectionView {
    SelectionView::new(
        ScreenInstanceId(81),
        (0..count)
            .map(|index| SelectionItem {
                title: format!("CHART {index}"),
                artist: "ARTIST".into(),
            })
            .collect::<Vec<_>>()
            .into(),
        Arc::from([]),
        960,
        720,
    )
    .unwrap()
}

fn geometry(scene: &Scene) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|r| (r.bounds, r.color, r.uv))
        .collect()
}

fn regions(hits: &[(ControlId, Bounds)]) -> Vec<(u64, [i64; 4])> {
    hits.iter()
        .map(|(id, b)| (id.0, [b.x, b.y, b.width, b.height]))
        .collect()
}

fn compose(view: &SelectionView, width: u32, height: u32) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(width, height);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    (scene, hits)
}

// Independent pre-migration 960x720 rendering oracle with two short rows.
// It contains no mounted tree or responsive-layout implementation.
fn original_scene(state: &SelectionFrame) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    rect(&mut scene, 0, 0, 960, 720, 0x10151e);
    text(&mut scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
    text(
        &mut scene,
        24,
        65,
        "ARROWS/PAGE SELECT  ENTER PLAY  F2 SETTINGS  F3 SEARCH",
        1,
        0x9bb1cf,
    );
    text(
        &mut scene,
        24,
        86,
        "HOME/END FIRST/LAST  WHEEL OVER CHARTS",
        1,
        0x9bb1cf,
    );
    text(
        &mut scene,
        24,
        106,
        "2/2 CHARTS  1 SCAN DIAGNOSTICS",
        1,
        0xd8b36b,
    );
    for (index, title, artist) in [(0, "A", "B"), (1, "C", "")] {
        let y = 140 + index * 34;
        let bounds = Bounds {
            x: 18,
            y: y as i64 - 6,
            width: 924,
            height: 30,
        };
        if state.selected == index {
            rect(&mut scene, bounds.x, bounds.y, 924, 30, 0x263d59);
        }
        text_clipped(
            &mut scene,
            28,
            if artist.is_empty() { y } else { y - 5 },
            title,
            2,
            0xf0f4ff,
            ClipRect::new([28, bounds.y, 904, if artist.is_empty() { 30 } else { 15 }]).unwrap(),
        )
        .unwrap();
        text_clipped(
            &mut scene,
            28,
            y + 14,
            artist,
            1,
            0x9bb1cf,
            ClipRect::new([28, bounds.y + 15, 904, 15]).unwrap(),
        )
        .unwrap();
        hits.push((ControlId(100 + index as u64), bounds));
    }
    text(&mut scene, 24, 654, "BAD", 1, 0xd8b36b);
    for (id, bounds, label) in [
        (
            ControlId(1),
            Bounds {
                x: 550,
                y: 65,
                width: 180,
                height: 34,
            },
            "START",
        ),
        (
            ControlId(5),
            Bounds {
                x: 750,
                y: 20,
                width: 180,
                height: 30,
            },
            "SETTINGS",
        ),
        (
            ControlId(4),
            Bounds {
                x: 750,
                y: 65,
                width: 180,
                height: 34,
            },
            "EXIT",
        ),
    ] {
        button(
            &mut scene,
            bounds,
            label,
            state.hovered == Some(id),
            state.armed == Some(id),
        );
        hits.push((id, bounds));
    }
    let search = Bounds {
        x: 440,
        y: 102,
        width: 490,
        height: 34,
    };
    text_field_with_font(
        &mut scene,
        &LineEditor::new("", 256).unwrap(),
        search,
        false,
        None,
    );
    hits.push((ControlId(80), search));
    if state.backend_pending {
        text(
            &mut scene,
            24,
            700,
            "GPU BACKEND PENDING - SAVE PROFILE AND RESTART",
            1,
            0xd8b36b,
        );
    }
    if let Some(error) = &state.error {
        text(
            &mut scene,
            24,
            650,
            "ERROR - ENTER RETURNS TO SELECTION",
            2,
            0xff8e8e,
        );
        text(&mut scene, 24, 682, error, 1, 0xffaaaa);
    }
    (scene, hits)
}

#[test]
fn mounted_selection_matches_original_order_geometry_actions_and_pending_error_states() {
    let view = SelectionView::new(
        ScreenInstanceId(82),
        vec![
            SelectionItem {
                title: "A".into(),
                artist: "B".into(),
            },
            SelectionItem {
                title: "C".into(),
                artist: String::new(),
            },
        ]
        .into(),
        vec!["BAD".into()].into(),
        960,
        720,
    )
    .unwrap();
    for state in [
        frame(0),
        SelectionFrame {
            selected: 1,
            hovered: Some(ControlId(1)),
            armed: Some(ControlId(4)),
            error: Some("FAILED".into()),
            backend_pending: true,
        },
        frame(0),
    ] {
        let (expected, expected_hits) = original_scene(&state);
        view.update(state);
        let (actual, actual_hits) = compose(&view, 960, 720);
        assert_eq!(geometry(&actual), geometry(&expected));
        assert_eq!(regions(&actual_hits), regions(&expected_hits));
    }
}

#[test]
fn projected_catalog_keeps_original_action_identity_and_rejects_invalid_tail_atomically() {
    let view = view(40);
    let indices: Arc<[usize]> = vec![2, 8, 20, 35].into();
    view.set_projection(Arc::clone(&indices), Some(2)).unwrap();
    let (before, hits) = compose(&view, 960, 720);
    assert_eq!(
        hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        vec![102, 108, 120, 135, 1, 5, 4, 80]
    );
    let identities = view.nodes.identities();
    let paints = view.nodes.paints();
    for (bad, cursor) in [
        (vec![2, 8, 40], Some(0)),
        (vec![2, 8, 8], Some(0)),
        (vec![2, 8, 1], Some(0)),
        (vec![2, 8], Some(2)),
        (vec![2], None),
    ] {
        assert!(view.set_projection(bad.into(), cursor).is_err());
        assert_eq!(view.nodes.identities(), identities);
        assert_eq!(view.nodes.paints(), paints);
        assert_eq!(view.cursor.get_untracked(), Some(2));
        assert!(view
            .projection
            .with_untracked(|old| Arc::ptr_eq(old, &indices)));
        assert_eq!(geometry(&compose(&view, 960, 720).0), geometry(&before));
    }
    view.set_projection(Arc::from([]), None).unwrap();
    assert_eq!(
        compose(&view, 960, 720)
            .1
            .iter()
            .map(|(id, _)| id.0)
            .collect::<Vec<_>>(),
        vec![5, 4, 80]
    );
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn unchanged_frame_and_independent_search_changes_reuse_rows_and_mounted_packets() {
    let view = view(40);
    let initial = compose(&view, 960, 720);
    let identities = view.nodes.identities();
    let paints = view.nodes.paints();
    view.update(frame(0));
    assert!(!view.dirty());
    assert_eq!(view.nodes.paints(), paints);
    assert_eq!(geometry(&compose(&view, 960, 720).0), geometry(&initial.0));
    let mut editor = LineEditor::new("音é", 256).unwrap();
    editor.select_all();
    view.set_search(&editor, true).unwrap();
    assert_eq!(&view.nodes.paints()[4..19], &paints[4..19]);
    assert_eq!(view.nodes.identities(), identities);
    compose(&view, 960, 720);
    let search_paints = view.nodes.paints();
    view.set_search(&editor, true).unwrap();
    assert!(!view.dirty());
    assert_eq!(view.nodes.paints(), search_paints);
    assert!(view
        .set_search(&LineEditor::new(&"x".repeat(257), 4096).unwrap(), false)
        .is_err());
    assert_eq!(view.nodes.paints(), search_paints);
    assert_eq!(view.search.get_untracked().selection(), Some((0, 5)));
    view.update(frame(1));
    let changed = view
        .nodes
        .paints()
        .iter()
        .zip(&search_paints)
        .enumerate()
        .filter_map(|(index, (after, before))| (after != before).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(changed, vec![4, 5]);
}

#[test]
fn responsive_selection_shares_actual_paint_and_hit_clips_without_replacing_identity() {
    let mut view = view(40);
    let identities = view.nodes.identities();
    assert!(view.resize(1200, 800).unwrap());
    let (wide, hits) = compose(&view, 1200, 800);
    assert_eq!(regions(&hits)[0], (100, [22, 148, 924, 30]));
    assert_eq!(regions(&hits)[1], (101, [22, 182, 924, 30]));
    assert_eq!(
        regions(&hits)[15..],
        [
            (1, [687, 72, 180, 34]),
            (5, [937, 22, 180, 30]),
            (4, [937, 72, 180, 34]),
            (80, [550, 113, 490, 34]),
        ]
    );
    assert_eq!(wide.rectangles()[0].bounds, [0.0, 0.0, 1200.0, 800.0]);
    assert!(view.resize(480, 360).unwrap());
    let (narrow, hits) = compose(&view, 480, 360);
    assert_eq!(regions(&hits)[0], (100, [9, 67, 471, 30]));
    assert_eq!(
        regions(&hits).into_iter().find(|(id, _)| *id == 80),
        Some((80, [220, 51, 260, 34]))
    );
    assert!(narrow.rectangles().iter().all(|r| r.bounds[0] >= 0.0
        && r.bounds[1] >= 0.0
        && r.bounds[0] + r.bounds[2] <= 480.0
        && r.bounds[1] + r.bounds[3] <= 360.0));
    for point in [
        (10.0, 70.0),
        (300.0, 70.0),
        (479.999, 96.999),
        (480.0, 70.0),
        (9.0, 67.0),
        (9.0, 97.0),
        (f64::NAN, 70.0),
    ] {
        let painted_hit = hits
            .iter()
            .rev()
            .find(|(_, b)| b.contains(point))
            .map(|(id, _)| *id);
        assert_eq!(view.hit(point), painted_hit);
        assert_eq!(
            view.contains_chart(point),
            painted_hit.is_some_and(|id| id.0 >= 100)
        );
    }
    assert_eq!(
        view.hit((300.0, 70.0)),
        Some(ControlId(80)),
        "search paints above the first catalog row"
    );
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn row_size_changes_reflow_siblings_and_crop_bottom_hits_then_invalid_size_preserves_everything() {
    let mut view = view(40);
    let identities = view.nodes.identities();
    compose(&view, 960, 720);
    let before = view.nodes.paints();
    assert!(view.set_row_height(44).unwrap());
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(
        regions(&hits)[..3],
        [
            (100, [18, 134, 924, 40]),
            (101, [18, 178, 924, 40]),
            (102, [18, 222, 924, 40])
        ]
    );
    let rows = regions(&hits)
        .into_iter()
        .filter(|(id, _)| *id >= 100)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 14);
    assert_eq!(rows.last(), Some(&(113, [18, 706, 924, 14])));
    assert!(!view.contains_chart((30.0, 175.0)));
    assert!(view.contains_chart((30.0, 178.0)));
    assert_eq!(view.nodes.identities(), identities);
    let changed = view
        .nodes
        .paints()
        .iter()
        .zip(&before)
        .enumerate()
        .filter_map(|(index, (after, before))| (after != before).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(changed, (4..19).collect::<Vec<_>>());
    let paints = view.nodes.paints();
    let revision = view.layout.revision();
    for invalid in [0, 1, 4] {
        assert!(view.set_row_height(invalid).is_err());
        assert_eq!(view.row_height, 44);
        assert_eq!(view.layout.revision(), revision);
        assert_eq!(view.nodes.paints(), paints);
        assert_eq!(view.nodes.identities(), identities);
        assert_eq!(geometry(&compose(&view, 960, 720).0), geometry(&scene));
    }
    assert!(!view.set_row_height(44).unwrap());
    assert_eq!(view.nodes.paints(), paints);
    assert!(view.set_row_height(34).unwrap());
    assert_eq!(
        regions(&compose(&view, 960, 720).1)[1],
        (101, [18, 168, 924, 30])
    );
}

#[test]
fn suspension_retains_draft_catalog_and_packets_and_back_resumes_same_selection_instance() {
    use crate::screen_lifecycle::{ScreenNavigator, ScreenRoute};
    let mut navigator = ScreenNavigator::default();
    let mut view = SelectionView::new(
        navigator.active_id().unwrap(),
        vec![SelectionItem {
            title: "A".into(),
            artist: "B".into(),
        }]
        .into(),
        Arc::from([]),
        960,
        720,
    )
    .unwrap();
    let mut search = LineEditor::new("音é", 256).unwrap();
    search.select_all();
    view.set_search(&search, true).unwrap();
    let original = compose(&view, 960, 720);
    let identities = view.nodes.identities();
    navigator
        .navigate(ScreenRoute::Settings, false, true)
        .unwrap();
    assert!(navigator.retains(view.id()));
    assert!(!navigator.accepts(view.id()));
    assert!(view.resize(0, 720).unwrap());
    let (hidden, hits) = compose(&view, 960, 720);
    assert!(hidden.rectangles().is_empty());
    assert!(hits.is_empty());
    assert_eq!(view.hit((28.0, 140.0)), None);
    assert!(!view.contains_chart((28.0, 140.0)));
    assert_eq!(view.search.get_untracked(), search);
    let before_back = navigator.clone();
    assert!(navigator.back(true, true).is_err());
    assert_eq!(navigator, before_back);
    navigator.back(false, true).unwrap();
    assert!(navigator.accepts(view.id()));
    assert!(view.resize(960, 720).unwrap());
    let restored = compose(&view, 960, 720);
    assert_eq!(geometry(&restored.0), geometry(&original.0));
    assert_eq!(regions(&restored.1), regions(&original.1));
    assert_eq!(view.nodes.identities(), identities);
    let paints = view.nodes.paints();
    assert!(!view.resize(960, 720).unwrap());
    assert_eq!(view.nodes.paints(), paints);
    let weak = view.nodes.weak_dirty();
    drop(view);
    assert!(
        weak.upgrade().is_none(),
        "dropping the screen releases every reactive packet subscription"
    );
}
