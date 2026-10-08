//! Mounted Players parity uses actual native drafts and borrowed browser metadata.
use super::*;
use crate::{
    local_players::PlayerId,
    local_setup::{BrowserInputSource, BrowserPlayerMember},
    settings::{NativeSettings, SettingsHost},
    ui::layout::{LayoutChange, LayoutUpdate, NodeId},
};

fn native(count: usize) -> LocalSetup {
    let settings = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
    let mut model = LocalSetup::from_settings(&settings, SettingsHost::Linux).unwrap();
    model.resize(count).unwrap();
    model
}
fn native_frame(model: &LocalSetup) -> PlayersFrame<'_> {
    PlayersFrame {
        model,
        selected: 0,
        first: 0,
        pending: false,
        error: None,
        message: None,
        hovered: None,
        armed: None,
    }
}
fn browser_frame<'a>(model: &'a BrowserLocalProjection<'a>) -> BrowserPlayersFrame<'a> {
    BrowserPlayersFrame {
        model,
        selected: 0,
        first: 0,
        pending: false,
        error: None,
        message: None,
        hovered: None,
        armed: None,
    }
}
fn sources() -> [BrowserInputSource<'static>; 5] {
    [
        ("kbd:41", BrowserInputKind::Keyboard, "KEYS"),
        ("touch:2", BrowserInputKind::Touch, "TOUCHSCREEN"),
        ("hid:9007199254740993", BrowserInputKind::Hid, "CONTROLLER"),
        ("pad:17", BrowserInputKind::Gamepad, "GAMEPAD"),
        ("pointer:8", BrowserInputKind::Pointer, "POINTER"),
    ]
    .map(|(id, kind, label)| BrowserInputSource {
        id,
        kind,
        label,
        detail: "original acquired input source",
        selectable: true,
    })
}
fn compose(view: &PlayersView, width: u32, height: u32) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(width, height);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    if width != 0 && height != 0 {
        scene.status().unwrap();
    }
    (scene, hits)
}
fn packet(scene: &Scene) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|r| (r.bounds, r.color, r.uv))
        .collect()
}
fn label(scene: &Scene, value: &str) {
    let expected: Vec<_> = value.chars().map(crate::font::glyph_uv).collect();
    let actual: Vec<_> = scene.rectangles().iter().map(|r| r.uv).collect();
    assert!(
        actual.windows(expected.len()).any(|row| row == expected),
        "missing Players label {value}"
    );
}
fn assignment_label(scene: &Scene, visible: &str) {
    // The original 150px button reserves 16px and uses a 12px glyph stride:
    // eleven visible characters. Inspect this action's actual paint region.
    assert!(scene
        .rectangles()
        .iter()
        .any(|rect| rect.bounds == [624.0, 620.0, 150.0, 34.0]));
    let actual = scene
        .rectangles()
        .iter()
        .filter(|rect| rect.bounds[0] >= 632.0 && rect.bounds[0] < 774.0 && rect.bounds[1] == 628.0)
        .map(|rect| rect.uv)
        .collect::<Vec<_>>();
    let expected = visible
        .chars()
        .map(crate::font::glyph_uv)
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}
fn assignment_control(hits: &[(ControlId, Bounds)], enabled: bool) {
    let bounds = hits
        .iter()
        .find(|(id, _)| *id == ControlId(34))
        .map(|(_, bounds)| [bounds.x, bounds.y, bounds.width, bounds.height]);
    assert_eq!(bounds, enabled.then_some([624, 620, 150, 34]));
}
fn row_node(view: &PlayersView, slot: usize) -> NodeId {
    view.layout
        .borrow()
        .leaves()
        .iter()
        .find(|leaf| matches!(leaf.component, Component::Row(index) if index == slot))
        .unwrap()
        .id
}

#[test]
fn native_allocator_survivors_and_restored_max_player_identity_are_data_not_control_indices() {
    let mut model = native(4);
    model.resize(2).unwrap();
    model.resize(4).unwrap();
    assert_eq!(
        model.players().iter().map(|p| p.id).collect::<Vec<_>>(),
        [PlayerId(1), PlayerId(2), PlayerId(5), PlayerId(6)]
    );
    let view = PlayersView::new(ScreenInstanceId(1401), 960, 720).unwrap();
    view.update(native_frame(&model)).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    label(&scene, "P5  NO KEYBOARD ASSIGNED");
    assert_eq!(
        hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        [20000, 20001, 20002, 20003, 30, 31, 32, 33, 34, 35]
    );
    assert_eq!(view.hit((30.0, 200.0)), Some(ControlId(20002)));
    let settings = NativeSettings::from_args(
        &[
            "--local-player".into(),
            "4294967295:/last".into(),
            "--local-player".into(),
            "7:/other".into(),
        ],
        SettingsHost::Linux,
    )
    .unwrap();
    let mut restored = LocalSetup::from_settings(&settings, SettingsHost::Linux).unwrap();
    let before = restored.players().to_vec();
    assert!(restored.resize(3).is_err());
    assert_eq!(restored.players(), before);
    restored.clear(PlayerId(u32::MAX)).unwrap();
    assert_eq!(restored.players()[0].id, PlayerId(u32::MAX));
    view.update(native_frame(&restored)).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    label(&scene, "P4294967295  NO KEYBOARD ASSIGNED");
    label(&scene, "P7  /other");
    assert_eq!(hits[0].0, ControlId(20000));
    assert_eq!(hits[1].0, ControlId(20001));
    assert_eq!(view.rows[0].get_untracked().as_ref().unwrap().id, u32::MAX);
}

#[test]
fn actual_native_solo_and_capacity64_pages_keep_original_action_and_painter_gates() {
    let view = PlayersView::new(ScreenInstanceId(1402), 960, 720).unwrap();
    let solo = native(1);
    view.update(native_frame(&solo)).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    label(&scene, "SOLO - INPUT AUTOMATIC");
    assert_eq!(
        hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        [20000, 30, 31, 33]
    );
    assert_eq!(view.hit((330.0, 625.0)), None);
    let mut full = native(64);
    let before = full.players().to_vec();
    assert!(full.resize(65).is_err());
    assert_eq!(full.players(), before);
    for first in [0, 50, 60] {
        let mut update = native_frame(&full);
        update.first = first;
        update.selected = first;
        view.update(update).unwrap();
        let (_, hits) = compose(&view, 960, 720);
        assert_eq!(
            hits.iter()
                .filter(|(id, _)| id.0 >= 20000)
                .map(|(id, _)| id.0)
                .collect::<Vec<_>>(),
            (20000 + first as u64..20000 + (first + 10).min(64) as u64).collect::<Vec<_>>()
        );
        assert_eq!(hits.iter().any(|(id, _)| id.0 == 36), first > 0);
        assert_eq!(hits.iter().any(|(id, _)| id.0 == 37), first + 10 < 64);
        assert!(!hits.iter().any(|(id, _)| id.0 == 33));
        for id in [30, 31, 32, 34, 35] {
            assert!(hits.iter().any(|(control, _)| control.0 == id));
        }
    }
}

#[test]
fn borrowed_browser_sources_preserve_all_kinds_original_member_ids_and_assignment_capability() {
    let sources = sources();
    let ids = [u32::MAX, 7, 77, 91, 1];
    let members = std::array::from_fn::<_, 5, _>(|index| BrowserPlayerMember {
        id: PlayerId(ids[index]),
        source: Some(sources[index].id),
    });
    let projection = BrowserLocalProjection {
        players: &members,
        sources: &sources,
        can_assign: true,
    };
    projection.validate().unwrap();
    let view = PlayersView::new(ScreenInstanceId(1403), 960, 720).unwrap();
    view.update_browser(browser_frame(&projection)).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    assignment_label(&scene, "ASSIGN INPU");
    assignment_control(&hits, true);
    label(&scene, "ASSIGN A DISTINCT INPUT SOURCE TO EACH PLAYER");
    for (index, source) in sources.iter().enumerate() {
        label(
            &scene,
            &format!(
                "P{}  {} {} ({})",
                ids[index],
                source.kind.label(),
                source.label,
                source.id
            ),
        );
        assert_eq!(
            view.rows[index].get_untracked().as_ref().unwrap().id,
            ids[index]
        );
        assert_eq!(
            view.rows[index]
                .get_untracked()
                .as_ref()
                .unwrap()
                .input
                .as_deref(),
            Some(source.id)
        );
        assert_eq!(hits[index].0, ControlId(20000 + index as u64));
    }
    assert_eq!(view.hit((630.0, 625.0)), Some(ControlId(34)));
    let before = packet(&scene);
    let paints = view.nodes.paints();
    let identities = view.nodes.identities();
    view.update_browser(browser_frame(&projection)).unwrap();
    assert!(!view.dirty());
    assert_eq!(view.nodes.paints(), paints);
    assert_eq!(view.nodes.identities(), identities);
    assert_eq!(packet(&compose(&view, 960, 720).0), before);
    let read_only = BrowserLocalProjection {
        can_assign: false,
        ..projection
    };
    view.update_browser(browser_frame(&read_only)).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    assignment_label(&scene, "ASSIGN INPU");
    assignment_control(&hits, false);
    assert!(!hits.iter().any(|(id, _)| matches!(id.0, 34 | 35)));
    assert_eq!(view.hit((630.0, 625.0)), None);
    assert!(read_only
        .admit_assignment(PlayerId(7), "pointer:8")
        .is_err());
    view.update_browser(browser_frame(&projection)).unwrap();
    compose(&view, 960, 720);
    assert_eq!(view.hit((630.0, 625.0)), Some(ControlId(34)));
}

#[test]
fn browser_solo_capacity_pages_and_switch_to_native_preserve_distinct_capability_models() {
    let view = PlayersView::new(ScreenInstanceId(1407), 960, 720).unwrap();
    let solo = [BrowserPlayerMember {
        id: PlayerId(u32::MAX),
        source: None,
    }];
    let automatic = BrowserLocalProjection {
        players: &solo,
        sources: &[],
        can_assign: true,
    };
    view.update_browser(browser_frame(&automatic)).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    label(&scene, "SOLO - INPUT AUTOMATIC");
    assert_eq!(
        hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        [20000, 30, 31, 33]
    );
    assert_eq!(view.rows[0].get_untracked().as_ref().unwrap().id, u32::MAX);
    let full = (0..64)
        .map(|index| BrowserPlayerMember {
            id: PlayerId(u32::MAX - index),
            source: None,
        })
        .collect::<Vec<_>>();
    let projected = BrowserLocalProjection {
        players: &full,
        sources: &[],
        can_assign: false,
    };
    let mut last_page = browser_frame(&projected);
    last_page.first = 60;
    last_page.selected = 60;
    view.update_browser(last_page).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    assignment_label(&scene, "ASSIGN INPU");
    assignment_control(&hits, false);
    assert_eq!(
        hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
        [20060, 20061, 20062, 20063, 36, 30, 31, 32]
    );
    assert_eq!(
        view.rows[0].get_untracked().as_ref().unwrap().id,
        u32::MAX - 60
    );
    assert_eq!(view.hit((630.0, 625.0)), None);
    let actual_native = native(2);
    view.update(native_frame(&actual_native)).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    assignment_label(&scene, "ASSIGN KEYB");
    assignment_control(&hits, true);
    label(&scene, "P1  NO KEYBOARD ASSIGNED");
    assert!(hits.iter().any(|(id, _)| id.0 == 34));
    assert_eq!(view.hit((630.0, 625.0)), Some(ControlId(34)));
    assert_eq!(
        view.rows[0].get_untracked().as_ref().unwrap().id,
        actual_native.players()[0].id.0
    );
}

#[test]
fn mounted_browser_rows_reflow_shared_paint_hit_clips_without_remount() {
    let members = [
        BrowserPlayerMember {
            id: PlayerId(77),
            source: None,
        },
        BrowserPlayerMember {
            id: PlayerId(7),
            source: None,
        },
    ];
    let projection = BrowserLocalProjection {
        players: &members,
        sources: &[],
        can_assign: false,
    };
    let view = PlayersView::new(ScreenInstanceId(1404), 960, 720).unwrap();
    view.update_browser(browser_frame(&projection)).unwrap();
    compose(&view, 960, 720);
    let identities = view.nodes.identities();
    let row = row_node(&view, 0);
    assert!(view
        .update_layout(&[LayoutUpdate {
            id: row,
            change: LayoutChange::Size([906, 26])
        }])
        .unwrap());
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(hits[0].1.height, 26);
    assert_eq!(hits[1].1.y, 151);
    assert_eq!(view.hit((30.0, 150.0)), None);
    assert_eq!(view.hit((30.0, 152.0)), Some(ControlId(20001)));
    assert!(scene
        .rectangles()
        .iter()
        .any(|r| r.bounds == [24.0, 151.0, 906.0, 34.0]));
    assert!(view
        .update_layout(&[LayoutUpdate {
            id: row,
            change: LayoutChange::Clip(Some(Bounds {
                x: 0,
                y: 0,
                width: 200,
                height: 10
            }))
        }])
        .unwrap());
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(
        (hits[0].1.x, hits[0].1.y, hits[0].1.width, hits[0].1.height),
        (24, 120, 200, 10)
    );
    assert!(scene
        .rectangles()
        .iter()
        .any(|r| r.bounds == [24.0, 120.0, 200.0, 10.0]));
    assert_eq!(view.hit((30.0, 125.0)), Some(ControlId(20000)));
    assert_eq!(view.hit((30.0, 135.0)), None);
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn true_extent_and_zero_suspend_keep_native_model_and_atomic_packets() {
    let model = native(10);
    let view = PlayersView::new(ScreenInstanceId(1405), 960, 720).unwrap();
    view.update(native_frame(&model)).unwrap();
    let ids = model.players().iter().map(|p| p.id).collect::<Vec<_>>();
    let identities = view.nodes.identities();
    assert!(view.resize(480, 300).unwrap());
    let (scene, hits) = compose(&view, 480, 300);
    assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 480.0, 300.0]);
    assert_eq!(hits[0].1.width, 456);
    assert_eq!(view.hit((479.0, 125.0)), Some(ControlId(20000)));
    assert_eq!(view.hit((480.0, 125.0)), None);
    assert_eq!(view.hit((175.0, 625.0)), None);
    assert!(scene.rectangles().iter().all(|r| r.bounds[0] >= 0.0
        && r.bounds[1] >= 0.0
        && r.bounds[0] + r.bounds[2] <= 480.0
        && r.bounds[1] + r.bounds[3] <= 300.0));
    let before = packet(&scene);
    let paints = view.nodes.paints();
    assert!(!view.resize(480, 300).unwrap());
    let mut invalid_frame = native_frame(&model);
    invalid_frame.selected = model.players().len();
    invalid_frame.error = Some("MUST NOT CHANGE");
    assert!(view.update(invalid_frame).is_err());
    assert_eq!(packet(&compose(&view, 480, 300).0), before);
    assert!(view
        .update_layout(&[
            LayoutUpdate {
                id: row_node(&view, 0),
                change: LayoutChange::Size([906, 26])
            },
            LayoutUpdate {
                id: NodeId(usize::MAX),
                change: LayoutChange::Size([1, 1])
            },
        ])
        .is_err());
    assert_eq!(packet(&compose(&view, 480, 300).0), before);
    assert_eq!(view.nodes.paints(), paints);
    for (width, height) in [(0, 300), (480, 0)] {
        assert!(view.resize(width, height).unwrap());
        view.update(native_frame(&model)).unwrap();
        let (scene, hits) = compose(&view, width, height);
        assert!(scene.rectangles().is_empty());
        assert!(hits.is_empty());
        assert_eq!(view.hit((30.0, 125.0)), None);
    }
    assert!(view.resize(960, 720).unwrap());
    compose(&view, 960, 720);
    assert_eq!(view.hit((175.0, 625.0)), Some(ControlId(31)));
    assert_eq!(view.nodes.identities(), identities);
    assert_eq!(
        model.players().iter().map(|p| p.id).collect::<Vec<_>>(),
        ids
    );
}

#[test]
fn invalid_browser_members_and_pending_frames_leave_current_identity_and_lifetime_honest() {
    let sources = sources();
    let members = [
        BrowserPlayerMember {
            id: PlayerId(77),
            source: Some(sources[0].id),
        },
        BrowserPlayerMember {
            id: PlayerId(7),
            source: Some(sources[1].id),
        },
    ];
    let projection = BrowserLocalProjection {
        players: &members,
        sources: &sources,
        can_assign: true,
    };
    let view = PlayersView::new(ScreenInstanceId(1406), 960, 720).unwrap();
    view.update_browser(browser_frame(&projection)).unwrap();
    let (scene, _) = compose(&view, 960, 720);
    let before = packet(&scene);
    let identities = view.nodes.identities();
    let invalid_members = [
        [
            BrowserPlayerMember {
                id: PlayerId(0),
                source: None,
            },
            members[1],
        ],
        [
            members[0],
            BrowserPlayerMember {
                id: PlayerId(77),
                source: None,
            },
        ],
        [
            members[0],
            BrowserPlayerMember {
                id: PlayerId(7),
                source: Some("unacquired"),
            },
        ],
        [
            members[0],
            BrowserPlayerMember {
                id: PlayerId(7),
                source: Some(sources[0].id),
            },
        ],
    ];
    for invalid in &invalid_members {
        let model = BrowserLocalProjection {
            players: invalid,
            ..projection
        };
        let mut update = browser_frame(&model);
        update.error = Some("MUST NOT APPLY");
        assert!(view.update_browser(update).is_err());
        assert_eq!(packet(&compose(&view, 960, 720).0), before);
        assert_eq!(view.rows[0].get_untracked().as_ref().unwrap().id, 77);
    }
    let mut bad_index = browser_frame(&projection);
    bad_index.first = usize::MAX;
    assert!(view.update_browser(bad_index).is_err());
    let mut pending = browser_frame(&projection);
    pending.pending = true;
    pending.hovered = Some(ControlId(31));
    pending.armed = Some(ControlId(31));
    view.update_browser(pending).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    label(&scene, "LOADING KEYBOARDS");
    assert!(hits.is_empty());
    assert_eq!(view.hit((175.0, 625.0)), None);
    view.update_browser(browser_frame(&projection)).unwrap();
    compose(&view, 960, 720);
    assert_eq!(view.hit((175.0, 625.0)), Some(ControlId(31)));
    assert_eq!(view.nodes.identities(), identities);
    let weak = view.nodes.weak_dirty();
    drop(view);
    assert!(weak.upgrade().is_none());
}
