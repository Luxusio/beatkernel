//! Deferred actual Desktop Results controls; no window, thread or socket opens.
use super::*;
use beatkernel_bms_runtime::{
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    room_opponent_hud::RoomOpponentHud,
    room_presentation::{RoomLobby, RoomPresentation, RoomResults, RoomStatus, RoomUiAction},
};

fn archive(cancelled: bool) -> Arc<RoomResults> {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 4, 8, 100).unwrap());
    let ids = (0..4)
        .map(|_| {
            registry
                .join(
                    "results",
                    b"actual identity",
                    &[PlayerId(7), PlayerId(9), PlayerId(u32::MAX)],
                    0,
                )
                .unwrap()
                .id
        })
        .collect::<Vec<_>>();
    registry.seal(ids[0], 1).unwrap();
    for id in &ids {
        registry.ready(*id, 2).unwrap();
    }
    let room = registry.room("results").unwrap();
    let lobby = Arc::new(
        RoomLobby::new(
            Some(ids[0]),
            3,
            Some(room.phase),
            room.deadline_ns,
            room.members.to_vec(),
        )
        .unwrap(),
    );
    let mut hud = RoomOpponentHud::new(ids[0], room.members).unwrap();
    hud.update(
        ids[1],
        &GroupPrefix {
            sequence: u64::MAX,
            final_prefix: false,
            members: room.members[1]
                .players
                .iter()
                .map(|&player| MemberProgress {
                    player,
                    progress: Progress {
                        song_ns: i64::MAX,
                        hits: u64::MAX,
                        misses: 0,
                        combo: u64::MAX,
                        max_combo: u64::MAX,
                    },
                })
                .collect(),
        },
    )
    .unwrap();
    hud.set_page(1).unwrap();
    Arc::new(
        RoomResults::new(
            lobby,
            &hud,
            cancelled,
            cancelled.then(|| "cancelled with accepted prefix".into()),
        )
        .unwrap(),
    )
}
fn game(
    viewer: player::PlayerViewer,
    archive: Arc<RoomResults>,
    status: player::PlayerStatus,
) -> Game {
    Game {
        viewer,
        worker: None,
        snapshot: Some(player::PlayerSnapshot {
            room: Some(Arc::new(archive.project(archive.initial_page()).unwrap())),
            cancelled: archive.cancelled(),
            room_results: Some(archive),
            status,
            ..Default::default()
        }),
        completed_results: None,
        completed_results_error: None,
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
fn selected(game: &Game) -> &Arc<RoomPresentation> {
    game.snapshot.as_ref().unwrap().room.as_ref().unwrap()
}
fn room_hits(app: &Desktop) -> Vec<u64> {
    app.hits
        .iter()
        .filter(|(id, _)| (90..=94).contains(&id.0))
        .map(|(id, _)| id.0)
        .collect()
}

#[test]
fn joined_finished_failed_and_cancelled_results_page_locally_without_reopening_commands() {
    for (status, cancelled) in [
        (player::PlayerStatus::Finished, false),
        (
            player::PlayerStatus::Failed("native cleanup failed".into()),
            false,
        ),
        (player::PlayerStatus::Finished, true),
    ] {
        let archive = archive(cancelled);
        let (_, viewer) = player::channel();
        let mut app = super::tests::lifecycle_fixture();
        let mut result = game(viewer, archive.clone(), status.clone());
        result.cancelling = cancelled;
        assert!(!result.room_action_allowed(RoomUiAction::Page(2)));
        assert!(result.select_room_result_page(2).is_err());
        result.owner_finished(!matches!(status, player::PlayerStatus::Failed(_)));
        assert_eq!(selected(&result).page, 1);
        assert!(result.room_action_allowed(RoomUiAction::Page(2)));
        for action in [RoomUiAction::Seal, RoomUiAction::Ready, RoomUiAction::Leave] {
            assert!(!result.room_action_allowed(action));
            assert!(result.request_room(action).is_err());
        }
        app.game = Some(result);
        app.navigate(ScreenRoute::Play { replay: false }).unwrap();
        app.navigate(ScreenRoute::Results { replay: false })
            .unwrap();
        app.draw().unwrap();
        assert_eq!(room_hits(&app), vec![93, 94]);
        app.activate(ControlId(94));
        let result = app.game.as_ref().unwrap();
        assert_eq!(selected(result).page, 2);
        assert_eq!(selected(result).rows.len(), 1);
        assert_eq!(result.local_page, 0);
        assert!(!result.viewer.room_pending());
        assert!(result.viewer.take_room_reply().unwrap().is_none());
        assert!(Arc::ptr_eq(
            result
                .snapshot
                .as_ref()
                .unwrap()
                .room_results
                .as_ref()
                .unwrap(),
            &archive
        ));
        app.draw().unwrap();
        assert_eq!(room_hits(&app), vec![93]);
        app.activate(ControlId(93));
        assert_eq!(selected(app.game.as_ref().unwrap()).page, 1);
        let result = app.game.as_mut().unwrap();
        let before = selected(result).clone();
        assert!(result.select_room_result_page(3).is_err());
        assert!(result.select_room_result_page(usize::MAX).is_err());
        assert!(Arc::ptr_eq(selected(result), &before));
        assert_eq!(result.snapshot.as_ref().unwrap().status, status);
        assert_eq!(result.snapshot.as_ref().unwrap().cancelled, cancelled);
        assert_eq!(archive.initial_page(), 1);
        app.navigate(ScreenRoute::Closing).unwrap();
        app.draw().unwrap();
        assert!(room_hits(&app).is_empty());
    }
}

#[test]
fn retained_selection_and_archive_are_isolated_from_other_views_retry_and_old_channel_completion() {
    let archive = archive(true);
    let (old_publisher, old_viewer) = player::channel();
    let (_, second_viewer) = player::channel();
    let mut first = game(old_viewer, archive.clone(), player::PlayerStatus::Finished);
    let mut second = game(
        second_viewer,
        archive.clone(),
        player::PlayerStatus::Finished,
    );
    first.owner_finished(true);
    second.owner_finished(true);
    first.select_room_result_page(2).unwrap();
    second.select_room_result_page(0).unwrap();
    assert_eq!((selected(&first).page, selected(&second).page), (2, 0));
    let repeated = player::PlayerSnapshot {
        room: Some(Arc::new(archive.project(1).unwrap())),
        room_results: Some(archive.clone()),
        status: player::PlayerStatus::Finished,
        cancelled: true,
        ..Default::default()
    };
    first.accept_snapshot(repeated);
    assert_eq!(selected(&first).page, 2);
    assert!(Arc::ptr_eq(
        first
            .snapshot
            .as_ref()
            .unwrap()
            .room_results
            .as_ref()
            .unwrap(),
        &archive
    ));
    first.accept_snapshot(player::PlayerSnapshot::default());
    assert_eq!(selected(&first).page, 2);
    assert!(Arc::ptr_eq(
        first
            .snapshot
            .as_ref()
            .unwrap()
            .room_results
            .as_ref()
            .unwrap(),
        &archive
    ));
    first.prepared_retry = Some(first.launch.retry().unwrap());
    assert!(!first.room_action_allowed(RoomUiAction::Page(0)));
    assert!(first.select_room_result_page(0).is_err());
    first.prepared_retry = None;
    first.replay = true;
    assert!(!first.room_action_allowed(RoomUiAction::Page(0)));
    assert!(first.select_room_result_page(0).is_err());
    first.replay = false;

    // A retired owner's terminal channel stays separate from a replacement.
    // Completing it must not replace the new Game's selected archive or controls.
    let (fresh_publisher, fresh_viewer) = player::channel();
    let mut app = super::tests::lifecycle_fixture();
    app.game = Some(game(
        fresh_viewer,
        archive.clone(),
        player::PlayerStatus::Loading,
    ));
    app.game
        .as_mut()
        .unwrap()
        .snapshot
        .as_mut()
        .unwrap()
        .room_results = None;
    let fresh_room = Arc::new(
        RoomPresentation::new(archive.lobby().clone(), RoomStatus::Waiting, None, None).unwrap(),
    );
    app.game.as_mut().unwrap().snapshot.as_mut().unwrap().room = Some(fresh_room.clone());
    player::with_publisher(old_publisher, || Ok(())).unwrap();
    assert!(first.viewer.take_latest().is_some());
    player::with_publisher(fresh_publisher, || {
        player::publish_room(fresh_room.clone()).unwrap();
        app.collect_game();
        let current = app.game.as_ref().unwrap();
        assert!(!current.joined);
        assert!(current.snapshot.as_ref().unwrap().room_results.is_none());
        assert!(Arc::ptr_eq(
            current.snapshot.as_ref().unwrap().room.as_ref().unwrap(),
            &fresh_room
        ));
        assert!(!current.viewer.room_pending());
        assert!(first.viewer.request_room(RoomUiAction::Page(0)).is_err());
        assert!(current.viewer.take_room_reply().unwrap().is_none());
        Ok(())
    })
    .unwrap();
    assert_eq!((selected(&first).page, selected(&second).page), (2, 0));
    assert!(Arc::ptr_eq(
        first
            .snapshot
            .as_ref()
            .unwrap()
            .room_results
            .as_ref()
            .unwrap(),
        &archive
    ));
}
