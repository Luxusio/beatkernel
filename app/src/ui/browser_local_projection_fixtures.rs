//! Real shared Players/Devices views with borrowed browser metadata, no native host.
use crate::{
    local_players::PlayerId,
    local_setup::{
        BrowserInputKind, BrowserInputSource, BrowserLocalProjection, BrowserPlayerMember,
    },
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
    ui::{
        devices::{self, BrowserDevicesFrame, DevicesView},
        interaction::{Bounds, ControlId},
        players::{self, BrowserPlayersFrame, PlayersView},
    },
};

fn sources() -> Vec<BrowserInputSource<'static>> {
    [
        BrowserInputKind::Keyboard,
        BrowserInputKind::Touch,
        BrowserInputKind::Hid,
        BrowserInputKind::Gamepad,
        BrowserInputKind::Pointer,
    ]
    .into_iter()
    .zip([
        "kbd:41",
        "touch:2",
        "hid:9007199254740993",
        "pad:17",
        "pointer:8",
    ])
    .map(|(kind, id)| BrowserInputSource {
        id,
        kind,
        label: id,
        detail: "original acquired source",
        selectable: true,
    })
    .collect()
}
fn players_frame<'a>(model: &'a BrowserLocalProjection<'a>) -> BrowserPlayersFrame<'a> {
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
fn devices_frame<'a>(sources: &'a [BrowserInputSource<'a>]) -> BrowserDevicesFrame<'a> {
    BrowserDevicesFrame {
        sources,
        player: Some(PlayerId(77)),
        selected: Some(0),
        first: 0,
        can_assign: true,
        can_refresh: true,
        pending: false,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn compose_players(view: &PlayersView) -> Vec<(ControlId, Bounds)> {
    let mut scene = Scene::with_capacity(960, 720, 64);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    hits
}
fn compose_devices(view: &DevicesView) -> Vec<(ControlId, Bounds)> {
    let mut scene = Scene::with_capacity(960, 720, 64);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    hits
}
fn player_paint(view: &PlayersView) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    let mut scene = Scene::with_capacity(960, 720, 64);
    view.compose(&mut scene, &mut Vec::new()).unwrap();
    scene
        .rectangles()
        .iter()
        .map(|rect| (rect.bounds, rect.color, rect.uv))
        .collect()
}

#[test]
fn browser_metadata_preserves_all_kinds_and_original_stable_identities() {
    let sources = sources();
    let members = [
        BrowserPlayerMember {
            id: PlayerId(77),
            source: Some(sources[2].id),
        },
        BrowserPlayerMember {
            id: PlayerId(9),
            source: Some(sources[0].id),
        },
    ];
    let model = BrowserLocalProjection {
        players: &members,
        sources: &sources,
        can_assign: true,
    };
    model.validate().unwrap();
    let view = PlayersView::new(ScreenInstanceId(700), 960, 720).unwrap();
    let frame = players_frame(&model);
    view.update_browser(frame).unwrap();
    let original_paint = player_paint(&view);
    let hits = compose_players(&view);
    assert!(hits.iter().any(|(id, _)| *id == ControlId(20000)));
    assert_eq!(
        players::hit_browser(&players_frame(&model), Some((30.0, 125.0))),
        Some(ControlId(20000))
    );
    assert_eq!(
        players::hit_browser(&players_frame(&model), Some((630.0, 625.0))),
        Some(ControlId(34))
    );
    assert_eq!(model.players[0].id, PlayerId(77));
    assert_eq!(model.players[0].source, Some("hid:9007199254740993"));
    assert_eq!(model.sources[4].kind, BrowserInputKind::Pointer);
    view.update_browser(players_frame(&model)).unwrap();
    assert_eq!(player_paint(&view), original_paint);
    assert_eq!(
        compose_players(&view)
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        hits.iter().map(|(id, _)| *id).collect::<Vec<_>>()
    );
}

#[test]
fn browser_solo_and_unavailable_assignment_have_matching_paint_and_hit_gates() {
    let sources = sources();
    for (count, can_assign) in [(1, true), (2, false)] {
        let members = [
            BrowserPlayerMember {
                id: PlayerId(77),
                source: None,
            },
            BrowserPlayerMember {
                id: PlayerId(9),
                source: None,
            },
        ];
        let model = BrowserLocalProjection {
            players: &members[..count],
            sources: &sources,
            can_assign,
        };
        let view = PlayersView::new(ScreenInstanceId(701), 960, 720).unwrap();
        view.update_browser(players_frame(&model)).unwrap();
        let hits = compose_players(&view);
        for control in [34, 35] {
            assert!(!hits.iter().any(|(id, _)| id.0 == control));
        }
        assert_eq!(
            players::hit_browser(&players_frame(&model), Some((630.0, 625.0))),
            None
        );
        let mut pending = players_frame(&model);
        pending.pending = true;
        view.update_browser(pending).unwrap();
        assert!(compose_players(&view).is_empty());
    }
}

#[test]
fn browser_devices_gate_disabled_sources_and_unavailable_native_controls() {
    let mut sources = sources();
    sources[1].selectable = false;
    let view = DevicesView::new(ScreenInstanceId(702), 960, 720).unwrap();
    view.update_browser(devices_frame(&sources)).unwrap();
    assert_eq!(
        devices::hit_browser(&devices_frame(&sources), Some((30.0, 125.0))),
        Some(ControlId(10000))
    );
    assert_eq!(
        devices::hit_browser(&devices_frame(&sources), Some((30.0, 164.0))),
        None
    );
    assert!(!compose_devices(&view)
        .iter()
        .any(|(id, _)| *id == ControlId(10001)));
    let mut unavailable = devices_frame(&sources);
    unavailable.can_assign = false;
    unavailable.can_refresh = false;
    assert_eq!(
        devices::hit_browser(&unavailable, Some((30.0, 625.0))),
        None
    );
    assert_eq!(
        devices::hit_browser(&unavailable, Some((410.0, 625.0))),
        None
    );
    view.update_browser(unavailable).unwrap();
    let hits = compose_devices(&view);
    assert!(!hits.iter().any(|(id, _)| [20, 22].contains(&id.0)));
    assert!(
        hits.iter().any(|(id, _)| *id == ControlId(21)),
        "Back remains available without native capabilities"
    );
    let mut disabled = devices_frame(&sources);
    disabled.selected = Some(1);
    assert_eq!(devices::hit_browser(&disabled, Some((30.0, 625.0))), None);
}

#[test]
fn malformed_browser_roster_and_metadata_refuse_atomically_before_shared_view_update() {
    let sources = sources();
    let members = [BrowserPlayerMember {
        id: PlayerId(77),
        source: Some("kbd:41"),
    }];
    let good = BrowserLocalProjection {
        players: &members,
        sources: &sources,
        can_assign: true,
    };
    let view = PlayersView::new(ScreenInstanceId(703), 960, 720).unwrap();
    view.update_browser(players_frame(&good)).unwrap();
    let before = compose_players(&view);
    let before_paint = player_paint(&view);
    for members in [
        vec![],
        vec![BrowserPlayerMember {
            id: PlayerId(0),
            source: None,
        }],
        vec![BrowserPlayerMember {
            id: PlayerId(77),
            source: Some("foreign source"),
        }],
        vec![
            BrowserPlayerMember {
                id: PlayerId(77),
                source: None
            };
            2
        ],
        (1..=65)
            .map(|id| BrowserPlayerMember {
                id: PlayerId(id),
                source: None,
            })
            .collect(),
    ] {
        let bad = BrowserLocalProjection {
            players: &members,
            sources: &sources,
            can_assign: true,
        };
        assert!(bad.validate().is_err());
        assert!(view.update_browser(players_frame(&bad)).is_err());
        assert_eq!(
            compose_players(&view)
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            before.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );
        assert_eq!(
            player_paint(&view),
            before_paint,
            "rejected metadata preserves actual shared painting"
        );
    }
    let huge = "x".repeat(4097);
    for sources in [
        vec![BrowserInputSource {
            id: "",
            ..sources[0]
        }],
        vec![sources[0], sources[0]],
        vec![BrowserInputSource {
            label: &huge,
            ..sources[0]
        }],
    ] {
        let bad = BrowserLocalProjection {
            players: &members,
            sources: &sources,
            can_assign: true,
        };
        assert!(bad.validate().is_err());
    }
}

#[test]
fn browser_projection_extension_keeps_existing_native_constructor_and_hit_behavior() {
    use crate::{
        device_catalog::{DeviceCatalog, DeviceChoice, DeviceRequest},
        local_setup::LocalSetup,
        settings::{NativeSettings, SettingsHost},
        ui::{devices::DevicesFrame, players::PlayersFrame},
    };
    let settings = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
    let mut local = LocalSetup::from_settings(&settings, SettingsHost::Linux).unwrap();
    local.resize(2).unwrap();
    let frame = PlayersFrame {
        model: &local,
        selected: 0,
        first: 0,
        pending: false,
        error: None,
        message: None,
        hovered: None,
        armed: None,
    };
    let players_view = PlayersView::new(ScreenInstanceId(704), 960, 720).unwrap();
    players_view.update(frame).unwrap();
    assert!(compose_players(&players_view)
        .iter()
        .any(|(id, _)| *id == ControlId(34)));
    let catalog = DeviceCatalog::new(
        DeviceRequest::LinuxKeyboard,
        vec![DeviceChoice {
            id: "/dev/input/event9".into(),
            label: "original native keyboard".into(),
            detail: "metadata only".into(),
            selectable: true,
        }],
    )
    .unwrap();
    let frame = DevicesFrame {
        catalog: &catalog,
        player: Some(PlayerId(77)),
        selected: Some(0),
        first: 0,
        pending: false,
        error: None,
        hovered: None,
        armed: None,
    };
    assert_eq!(
        devices::hit(&frame, Some((30.0, 625.0))),
        Some(ControlId(20))
    );
    let devices_view = DevicesView::new(ScreenInstanceId(705), 960, 720).unwrap();
    devices_view.update(frame).unwrap();
    assert!(compose_devices(&devices_view)
        .iter()
        .any(|(id, _)| *id == ControlId(20)));
}

#[test]
fn browser_source_count_aggregate_budget_and_invalid_indices_are_explicit_refusals() {
    let ids = (0..1025)
        .map(|id| format!("source:{id}"))
        .collect::<Vec<_>>();
    let text = "x".repeat(4096);
    let metadata = ids
        .iter()
        .map(|id| BrowserInputSource {
            id,
            kind: BrowserInputKind::Hid,
            label: &text,
            detail: &text,
            selectable: true,
        })
        .collect::<Vec<_>>();
    let players = [BrowserPlayerMember {
        id: PlayerId(77),
        source: None,
    }];
    for extent in [1024, 1025] {
        let model = BrowserLocalProjection {
            players: &players,
            sources: &metadata[..extent],
            can_assign: true,
        };
        assert!(
            model.validate().is_err(),
            "count and aggregate metadata limits are independent"
        );
    }
    let sources = sources();
    let view = DevicesView::new(ScreenInstanceId(706), 960, 720).unwrap();
    view.update_browser(devices_frame(&sources)).unwrap();
    let original = compose_devices(&view)
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    for (selected, first) in [
        (Some(sources.len()), 0),
        (Some(0), sources.len()),
        (Some(0), usize::MAX),
    ] {
        let mut frame = devices_frame(&sources);
        frame.selected = selected;
        frame.first = first;
        assert_eq!(devices::hit_browser(&frame, Some((30.0, 125.0))), None);
        assert!(view.update_browser(frame).is_err());
        assert_eq!(
            compose_devices(&view)
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            original
        );
    }
}

#[test]
fn actual_browser_assignment_guard_preserves_original_source_and_member_ownership() {
    let mut sources = sources();
    sources[4].selectable = false;
    let players = [
        BrowserPlayerMember {
            id: PlayerId(77),
            source: Some("kbd:41"),
        },
        BrowserPlayerMember {
            id: PlayerId(9),
            source: None,
        },
    ];
    let model = BrowserLocalProjection {
        players: &players,
        sources: &sources,
        can_assign: true,
    };
    assert!(model.admit_assignment(PlayerId(77), "kbd:41").is_ok());
    assert!(model
        .admit_assignment(PlayerId(9), "hid:9007199254740993")
        .is_ok());
    for (player, source) in [
        (PlayerId(9), "kbd:41"),
        (PlayerId(999), "touch:2"),
        (PlayerId(9), "foreign source"),
        (PlayerId(9), "pointer:8"),
    ] {
        assert!(model.admit_assignment(player, source).is_err());
    }
    let unavailable = BrowserLocalProjection {
        can_assign: false,
        ..model
    };
    assert!(unavailable
        .admit_assignment(PlayerId(9), "touch:2")
        .is_err());
    let solo = BrowserLocalProjection {
        players: &players[..1],
        ..model
    };
    assert!(solo.admit_assignment(PlayerId(77), "touch:2").is_err());
    assert_eq!(model.players[0].source, Some("kbd:41"));
    assert_eq!(
        model.players[1].source, None,
        "admission checks do not execute or substitute an assignment"
    );
}
