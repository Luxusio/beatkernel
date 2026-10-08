use super::*;

fn frame(start: &str, end: &str) -> PracticeFrame {
    PracticeFrame {
        editor: LineEditor::new(start, 64).unwrap(),
        end_editor: LineEditor::new(end, 64).unwrap(),
        end_focused: false,
        error: None,
        hovered: None,
        armed: None,
    }
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

fn compose(view: &PracticeView, width: u32, height: u32) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(width, height);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    (scene, hits)
}

// Pre-migration default rendering, independent of mounted declaration/reflow.
fn original_scene(
    state: &PracticeFrame,
    start_preview: &str,
    end_preview: &str,
) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    rect(&mut scene, 0, 0, 960, 720, 0x10151e);
    for (x, y, label, scale, color) in [
        (24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff),
        (24, 65, "PRACTICE SECTION", 2, 0xf0f4ff),
        (24, 134, "START", 1, 0x9bb1cf),
        (
            24,
            260,
            "END (OPTIONAL; EMPTY PLAYS THROUGH SONG END)",
            1,
            0x9bb1cf,
        ),
        (
            24,
            105,
            "SECONDS / M:SS / H:MM:SS  FRACTION UP TO 9 DIGITS",
            2,
            0x9bb1cf,
        ),
        (
            24,
            450,
            "DONE UPDATES SETTINGS DRAFT; APPLY CHANGES THE NEXT SESSION",
            1,
            0x9bb1cf,
        ),
        (
            24,
            472,
            "F5 RETRIES THE PINNED SESSION; BACK DISCARDS THESE EDITS",
            1,
            0x9bb1cf,
        ),
        (
            24,
            494,
            "FULL SONG RESETS START AND END; THROUGH END CLEARS ONLY END",
            1,
            0x9bb1cf,
        ),
        (
            24,
            516,
            "TAB SWITCHES START / END; END MUST BE AFTER START",
            1,
            0x9bb1cf,
        ),
    ] {
        text(&mut scene, x, y, label, scale, color);
    }
    for (id, bounds, editor, focused, y, preview) in [
        (
            70,
            Bounds {
                x: 24,
                y: 150,
                width: 906,
                height: 40,
            },
            &state.editor,
            !state.end_focused,
            230,
            start_preview,
        ),
        (
            75,
            Bounds {
                x: 24,
                y: 280,
                width: 906,
                height: 40,
            },
            &state.end_editor,
            state.end_focused,
            335,
            end_preview,
        ),
    ] {
        text_field_with_font(&mut scene, editor, bounds, focused, None);
        hits.push((ControlId(id), bounds));
        text(&mut scene, 24, y, preview, 2, 0xd8b36b);
    }
    for (id, x, label) in [
        (71, 24, "DONE"),
        (72, 220, "BACK"),
        (73, 416, "FULL SONG"),
        (76, 612, "THROUGH END"),
    ] {
        let bounds = Bounds {
            x,
            y: 380,
            width: 180,
            height: 34,
        };
        button(
            &mut scene,
            bounds,
            label,
            state.hovered == Some(ControlId(id)),
            state.armed == Some(ControlId(id)),
        );
        hits.push((ControlId(id), bounds));
    }
    if let Some(error) = &state.error {
        text(&mut scene, 24, 560, error, 1, 0xffaaaa);
    }
    (scene, hits)
}

#[test]
fn typed_practice_preserves_original_preview_editor_action_and_error_painter_order() {
    let view = PracticeView::new(ScreenInstanceId(91), 960, 720).unwrap();
    for (mut state, start_preview, end_preview) in [
        (frame("0:00", ""), "EXACT START: 0 NS", "THROUGH SONG END"),
        (
            frame("1.000000001", "2.000000002"),
            "EXACT START: 1000000001 NS",
            "EXACT END: 2000000002 NS",
        ),
        (frame("invalid", "1:60"), "INVALID START", "INVALID END"),
    ] {
        if state.editor.value() == "invalid" {
            state.end_focused = true;
            state.error = Some("REJECTED".into());
            state.hovered = Some(ControlId(72));
            state.armed = Some(ControlId(76));
        }
        let expected = original_scene(&state, start_preview, end_preview);
        view.update(state);
        let actual = compose(&view, 960, 720);
        assert_eq!(geometry(&actual.0), geometry(&expected.0));
        assert_eq!(regions(&actual.1), regions(&expected.1));
    }
}

#[test]
fn actual_section_model_retains_preroll_and_pinned_retry_while_rejecting_invalid_end_atomically() {
    use crate::{
        practice_loop::PracticeLoop,
        session_launch::SessionLaunch,
        settings::{NativeSettings, SettingsHost},
    };
    use beatkernel::time::{Duration, Timestamp};
    let mut settings = NativeSettings::from_args(
        &[
            "--preroll-ns".into(),
            "250000000".into(),
            "--bind".into(),
            "11:04".into(),
        ],
        SettingsHost::Linux,
    )
    .unwrap();
    let unrelated = |settings: &NativeSettings| {
        settings
            .native_args()
            .chunks_exact(2)
            .filter(|pair| !matches!(pair[0].as_str(), "--start-ns" | "--end-ns"))
            .flat_map(|pair| pair.iter().cloned())
            .collect::<Vec<_>>()
    };
    let original = unrelated(&settings);
    let view = PracticeView::new(ScreenInstanceId(92), 960, 720).unwrap();
    let state = frame("1.000000001", "2.000000002");
    view.update(state.clone());
    let start = PracticeStart::parse(state.editor.value()).unwrap();
    let end = PracticeStart::parse(state.end_editor.value()).unwrap();
    assert_eq!(start.nanoseconds(), 1_000_000_001);
    assert_eq!(end.nanoseconds(), 2_000_000_002);
    start.apply_section(Some(end), &mut settings).unwrap();
    assert_eq!(unrelated(&settings), original);
    let region = PracticeLoop::new(start, end).unwrap();
    assert_eq!(
        region
            .playback_end_frame(start, Duration::from_nanos(250_000_000), 48_000)
            .unwrap(),
        60_001
    );
    assert!(!region.reached(Timestamp::from_nanos(2_000_000_001)));
    assert!(region.reached(Timestamp::from_nanos(2_000_000_002)));
    let launch = SessionLaunch::new(vec![
        "--chart".into(),
        "songs/pinned.bms".into(),
        "--start-ns".into(),
        "0".into(),
        "--preroll-ns".into(),
        "250000000".into(),
        "--record-replay".into(),
        "take.bkr".into(),
    ])
    .unwrap();
    let finite = launch.retry_loop(region).unwrap();
    let value = |launch: &SessionLaunch, flag: &str| {
        launch
            .args()
            .chunks_exact(2)
            .find(|pair| pair[0] == flag)
            .unwrap()[1]
            .clone()
    };
    assert_eq!(value(&finite, "--start-ns"), "1000000001");
    assert_eq!(value(&finite, "--end-ns"), "2000000002");
    assert_eq!(value(&finite, "--preroll-ns"), "250000000");
    let retry = finite.retry().unwrap();
    assert_eq!(value(&retry, "--start-ns"), "0");
    assert!(!retry.args().iter().any(|arg| arg == "--end-ns"));
    assert_eq!(value(&retry, "--chart"), "songs/pinned.bms");
    assert_eq!(value(&retry, "--record-replay"), "take.retry2.bkr");
    let before = settings.native_args();
    for text in ["0", "1.000000001"] {
        assert!(start
            .apply_section(Some(PracticeStart::parse(text).unwrap()), &mut settings)
            .is_err());
        assert_eq!(settings.native_args(), before);
    }
    start.apply_section(None, &mut settings).unwrap();
    assert_eq!(PracticeStart::from_settings(&settings).unwrap(), start);
    assert_eq!(PracticeStart::section_end(&settings).unwrap(), None);
    assert_eq!(unrelated(&settings), original);
}

#[test]
fn resized_practice_uses_shared_cropped_control_geometry_and_original_action_ids() {
    let mut view = PracticeView::new(ScreenInstanceId(93), 960, 720).unwrap();
    let identities = view.nodes.identities();
    assert!(view.resize(1200, 800).unwrap());
    let (wide, hits) = compose(&view, 1200, 800);
    assert_eq!(
        regions(&hits),
        vec![
            (70, [30, 166, 906, 40]),
            (75, [30, 311, 906, 40]),
            (71, [30, 422, 180, 34]),
            (72, [226, 422, 180, 34]),
            (73, [422, 422, 180, 34]),
            (76, [618, 422, 180, 34]),
        ]
    );
    assert_eq!(wide.rectangles()[0].bounds, [0.0, 0.0, 1200.0, 800.0]);
    assert!(view.resize(480, 360).unwrap());
    let (narrow, hits) = compose(&view, 480, 360);
    assert_eq!(
        regions(&hits),
        vec![
            (70, [12, 75, 468, 40]),
            (75, [12, 140, 468, 40]),
            (71, [12, 190, 180, 34]),
            (72, [208, 190, 180, 34]),
            (73, [404, 190, 76, 34]),
        ]
    );
    assert!(narrow.rectangles().iter().all(|r| r.bounds[0] >= 0.0
        && r.bounds[1] >= 0.0
        && r.bounds[0] + r.bounds[2] <= 480.0
        && r.bounds[1] + r.bounds[3] <= 360.0));
    for point in [
        (12.0, 75.0),
        (479.999, 114.999),
        (480.0, 75.0),
        (12.0, 115.0),
        (404.0, 190.0),
        (479.999, 223.999),
        (480.0, 190.0),
        (f64::NAN, 190.0),
    ] {
        assert_eq!(
            view.hit(point),
            hits.iter()
                .rev()
                .find(|(_, b)| b.contains(point))
                .map(|(id, _)| *id)
        );
    }
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn editor_height_reflows_preview_siblings_only_and_refused_height_preserves_drafts_and_packets() {
    let mut view = PracticeView::new(ScreenInstanceId(94), 960, 720).unwrap();
    let state = frame("1.000000001", "2.000000002");
    view.update(state.clone());
    compose(&view, 960, 720);
    let paints = view.nodes.paints();
    let identities = view.nodes.identities();
    assert!(view.set_editor_height(60).unwrap());
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(
        regions(&hits)[..2],
        [(70, [24, 150, 906, 60]), (75, [24, 280, 906, 60])]
    );
    for y in [250.0, 355.0] {
        assert!(scene
            .rectangles()
            .iter()
            .any(|r| r.bounds == [24.0, y, 10.0, 14.0] && r.uv == crate::font::glyph_uv('E')));
    }
    let changed = view
        .nodes
        .paints()
        .iter()
        .zip(&paints)
        .enumerate()
        .filter_map(|(index, (after, before))| (after != before).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(changed, vec![1, 2]);
    let paints = view.nodes.paints();
    let revision = view.layout.revision();
    assert!(view.set_editor_height(0).is_err());
    assert_eq!(view.editor_height, 60);
    assert_eq!(view.editor.get_untracked(), state.editor);
    assert_eq!(view.end_editor.get_untracked(), state.end_editor);
    assert_eq!(view.layout.revision(), revision);
    assert_eq!(view.nodes.paints(), paints);
    assert_eq!(view.nodes.identities(), identities);
    assert_eq!(geometry(&compose(&view, 960, 720).0), geometry(&scene));
    assert!(!view.set_editor_height(60).unwrap());
    assert_eq!(view.nodes.paints(), paints);
}

#[test]
fn focused_cursor_error_and_unchanged_updates_preserve_unrelated_packet_identity() {
    let view = PracticeView::new(ScreenInstanceId(95), 960, 720).unwrap();
    let mut state = frame("1", "2");
    view.update(state.clone());
    compose(&view, 960, 720);
    let identities = view.nodes.identities();
    let before = view.nodes.paints();
    view.update(state.clone());
    assert!(!view.dirty());
    assert_eq!(view.nodes.paints(), before);
    state.end_editor.left();
    view.update(state.clone());
    let cursor = view.nodes.paints();
    assert_eq!(cursor[2], before[2] + 1);
    assert_eq!(&cursor[..2], &before[..2]);
    assert_eq!(&cursor[3..], &before[3..]);
    state.end_focused = true;
    view.update(state.clone());
    let focus = view.nodes.paints();
    assert_eq!(focus[1], cursor[1] + 1);
    assert_eq!(focus[2], cursor[2] + 1);
    assert_eq!(focus[0], cursor[0]);
    assert_eq!(&focus[3..], &cursor[3..]);
    state.error = Some("END MUST FOLLOW START".into());
    view.update(state.clone());
    let error = view.nodes.paints();
    assert_eq!(&error[..7], &focus[..7]);
    assert_eq!(error[7], focus[7] + 1);
    compose(&view, 960, 720);
    view.update(state);
    assert!(!view.dirty());
    assert_eq!(view.nodes.paints(), error);
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn zero_extent_and_pending_back_preserve_parent_settings_until_an_explicit_section_commit() {
    use crate::{
        screen_lifecycle::{ScreenNavigator, ScreenRoute},
        settings::{NativeSettings, SettingsHost},
    };
    let mut navigator = ScreenNavigator::default();
    navigator
        .navigate(ScreenRoute::Settings, false, true)
        .unwrap();
    navigator
        .navigate(ScreenRoute::Practice, false, true)
        .unwrap();
    let mut view = PracticeView::new(navigator.active_id().unwrap(), 960, 720).unwrap();
    let settings = NativeSettings::from_args(
        &["--preroll-ns".into(), "500000000".into()],
        SettingsHost::Linux,
    )
    .unwrap();
    let original_args = settings.native_args();
    let state = frame("20:00:00.000000001", "20:00:01.000000001");
    view.update(state.clone());
    let original = compose(&view, 960, 720);
    let identities = view.nodes.identities();
    assert!(view.resize(0, 720).unwrap());
    let (hidden, hits) = compose(&view, 960, 720);
    assert!(hidden.rectangles().is_empty());
    assert!(hits.is_empty());
    assert_eq!(view.hit((30.0, 160.0)), None);
    assert_eq!(view.editor.get_untracked(), state.editor);
    assert_eq!(view.end_editor.get_untracked(), state.end_editor);
    assert_eq!(settings.native_args(), original_args);
    let before_back = navigator.clone();
    assert!(navigator.back(true, true).is_err());
    assert_eq!(navigator, before_back);
    assert!(view.resize(960, 720).unwrap());
    let restored = compose(&view, 960, 720);
    assert_eq!(geometry(&restored.0), geometry(&original.0));
    assert_eq!(regions(&restored.1), regions(&original.1));
    assert_eq!(view.nodes.identities(), identities);
    navigator.back(false, true).unwrap();
    assert_eq!(navigator.route(), ScreenRoute::Settings);
    assert!(!navigator.retains(view.id()));
    assert_eq!(settings.native_args(), original_args);
    let weak = view.nodes.weak_dirty();
    drop(view);
    assert!(weak.upgrade().is_none());
}
