//! Deferred real desktop control/routing fixtures; no window or renderer opens.
use super::*;
use beatkernel_bms_runtime::{
    multiplayer_group_rooms::{GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry},
    room_opponent_hud::RoomOpponentHud,
    room_presentation::{RoomLobby, RoomPresentation, RoomStatus, RoomUiAction, RoomUiReply},
};

fn page(phase: GroupRoomPhase, selected: usize) -> Arc<RoomPresentation> {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 3, 8, 100).unwrap());
    let ids = (0..3)
        .map(|_| {
            registry
                .join(
                    "room",
                    b"actual identity",
                    &[PlayerId(7), PlayerId(9), PlayerId(u32::MAX)],
                    0,
                )
                .unwrap()
                .id
        })
        .collect::<Vec<_>>();
    if phase != GroupRoomPhase::Collecting {
        registry.seal(ids[0], 1).unwrap();
    }
    if phase == GroupRoomPhase::Prepared {
        for id in ids.iter().copied() {
            registry.ready(id, 2).unwrap();
        }
    }
    let room = registry.room("room").unwrap();
    let lobby = Arc::new(
        RoomLobby::new(
            Some(ids[0]),
            1,
            Some(room.phase),
            room.deadline_ns,
            room.members.to_vec(),
        )
        .unwrap(),
    );
    let mut hud = if phase == GroupRoomPhase::Prepared {
        Some(RoomOpponentHud::new(ids[0], room.members).unwrap())
    } else {
        None
    };
    if let Some(hud) = &mut hud {
        hud.set_page(selected).unwrap();
    }
    Arc::new(RoomPresentation::new(lobby, RoomStatus::Waiting, hud.as_ref(), None).unwrap())
}
fn game(viewer: player::PlayerViewer, room: Arc<RoomPresentation>) -> Game {
    Game {
        viewer,
        worker: None,
        snapshot: Some(player::PlayerSnapshot {
            room: Some(room),
            ..Default::default()
        }),
        cancelling: false,
        joined: false,
        local_page: 0,
        local_comparisons: true,
        replay: false,
        launch: SessionLaunch::new(vec!["--chart".into(), "pinned.bms".into()]).unwrap(),
        prepared_retry: None,
        practice_bookmark: None,
        practice_loop: None,
        loop_enabled: false,
    }
}

#[test]
fn desktop_room_hit_bounds_and_capabilities_follow_real_lobby_and_game_lifecycle() {
    let bounds = (90..=94)
        .map(|id| (ControlId(id), room_control_bounds(ControlId(id)).unwrap()))
        .collect::<Vec<_>>();
    for (index, (_, bound)) in bounds.iter().enumerate() {
        assert!(bound.contains((bound.x as f64, bound.y as f64)));
        assert!(!bound.contains(((bound.x + bound.width) as f64, bound.y as f64)));
        assert!(!bound.contains((bound.x as f64, (bound.y + bound.height) as f64)));
        for (_, other) in &bounds[..index] {
            assert!(
                bound.x + bound.width <= other.x
                    || other.x + other.width <= bound.x
                    || bound.y + bound.height <= other.y
                    || other.y + other.height <= bound.y
            );
        }
    }
    assert!(room_control_bounds(ControlId(89)).is_none());
    assert!(room_control_bounds(ControlId(95)).is_none());
    let (_, viewer) = player::channel();
    let mut app = super::tests::lifecycle_fixture();
    app.game = Some(game(viewer, page(GroupRoomPhase::Collecting, 0)));
    app.navigate(ScreenRoute::Play { replay: false }).unwrap();
    app.draw().unwrap();
    let hits = |app: &Desktop| {
        app.hits
            .iter()
            .filter(|(id, _)| (90..=94).contains(&id.0))
            .map(|(id, _)| id.0)
            .collect::<Vec<_>>()
    };
    assert!(hits(&app).contains(&90));
    assert!(hits(&app).contains(&92));
    assert!(!hits(&app).contains(&91));
    app.game
        .as_mut()
        .unwrap()
        .accept_snapshot(player::PlayerSnapshot {
            room: Some(page(GroupRoomPhase::Frozen, 0)),
            status: player::PlayerStatus::Playing,
            ..Default::default()
        });
    app.draw().unwrap();
    assert!(hits(&app).contains(&91));
    assert!(!hits(&app).contains(&90));
    app.game
        .as_mut()
        .unwrap()
        .accept_snapshot(player::PlayerSnapshot {
            room: Some(page(GroupRoomPhase::Prepared, 0)),
            status: player::PlayerStatus::Playing,
            ..Default::default()
        });
    app.draw().unwrap();
    assert!(hits(&app).contains(&94));
    assert!(!hits(&app).contains(&93));
    let game = app.game.as_mut().unwrap();
    for status in [player::PlayerStatus::Loading, player::PlayerStatus::Playing] {
        game.snapshot.as_mut().unwrap().status = status;
        assert!(game.room_action_allowed(RoomUiAction::Page(1)));
    }
    for status in [
        player::PlayerStatus::Stopping,
        player::PlayerStatus::Finished,
        player::PlayerStatus::Failed("stopped".into()),
    ] {
        game.snapshot.as_mut().unwrap().status = status;
        assert!(!game.room_action_allowed(RoomUiAction::Page(1)));
    }
    game.snapshot.as_mut().unwrap().status = player::PlayerStatus::Playing;
    game.replay = true;
    assert!(!game.room_action_allowed(RoomUiAction::Leave));
    game.replay = false;
    game.prepared_retry = Some(game.launch.retry().unwrap());
    assert!(!game.room_action_allowed(RoomUiAction::Leave));
    game.prepared_retry = None;
    game.cancelling = true;
    assert!(!game.room_action_allowed(RoomUiAction::Page(1)));
    game.cancelling = false;
    game.joined = true;
    assert!(!game.room_action_allowed(RoomUiAction::Page(1)));
    app.navigate(ScreenRoute::Results { replay: false })
        .unwrap();
    app.draw().unwrap();
    assert!(hits(&app).is_empty());
    app.navigate(ScreenRoute::Closing).unwrap();
    app.draw().unwrap();
    assert!(hits(&app).is_empty());
}

#[test]
fn actual_desktop_page_request_waits_for_correlated_result_and_confirmed_page_snapshot() {
    let (publisher, viewer) = player::channel();
    let initial = page(GroupRoomPhase::Prepared, 0);
    let mut game = game(viewer, initial.clone());
    player::with_publisher(publisher, || {
        player::publish_room(initial.clone()).unwrap();
        game.accept_snapshot(game.viewer.take_latest().unwrap());
        let first = game.request_room(RoomUiAction::Page(1)).unwrap();
        assert!(game.viewer.room_pending());
        assert!(!game.room_action_allowed(RoomUiAction::Leave));
        assert_eq!(
            game.snapshot.as_ref().unwrap().room.as_ref().unwrap().page,
            0
        );
        assert!(game.request_room(RoomUiAction::Page(1)).is_err());
        let request = player::take_room_request().unwrap().unwrap();
        assert_eq!(request.id, first);
        assert_eq!(request.action, RoomUiAction::Page(1));
        player::reply_room(RoomUiReply {
            id: first,
            result: Err("owner refused page".into()),
        })
        .unwrap();
        assert!(game.viewer.room_pending());
        assert!(!game.room_action_allowed(RoomUiAction::Page(1)));
        let reply = game.viewer.take_room_reply().unwrap().unwrap();
        assert_eq!(reply.id, first);
        assert!(reply.result.is_err());
        assert!(!game.viewer.room_pending());
        assert!(
            game.viewer
                .room_notice()
                .unwrap()
                .contains("owner refused page")
        );
        assert_eq!(
            game.snapshot.as_ref().unwrap().room.as_ref().unwrap().page,
            0
        );
        let second = game.request_room(RoomUiAction::Page(1)).unwrap();
        assert!(second > first);
        assert_eq!(player::take_room_request().unwrap().unwrap().id, second);
        player::reply_room(RoomUiReply {
            id: second,
            result: Ok(()),
        })
        .unwrap();
        assert_eq!(game.viewer.take_room_reply().unwrap().unwrap().id, second);
        assert_eq!(
            game.snapshot.as_ref().unwrap().room.as_ref().unwrap().page,
            0,
            "a reply alone cannot invent confirmed view data"
        );
        player::publish_room(page(GroupRoomPhase::Prepared, 1)).unwrap();
        game.accept_snapshot(game.viewer.take_latest().unwrap());
        assert_eq!(
            game.snapshot.as_ref().unwrap().room.as_ref().unwrap().page,
            1
        );
        let pending = game.request_room(RoomUiAction::Page(0)).unwrap();
        game.viewer.cancel();
        assert!(game.request_room(RoomUiAction::Leave).is_err());
        let reply = game.viewer.take_room_reply().unwrap().unwrap();
        assert_eq!(reply.id, pending);
        assert!(reply.result.is_err());
        assert!(player::take_room_request().unwrap().is_none());
        assert!(game.viewer.take_room_reply().unwrap().is_none());
        player::close_room_controls();
        Ok(())
    })
    .unwrap();
}
