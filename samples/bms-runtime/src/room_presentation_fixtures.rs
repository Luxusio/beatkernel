//! Deferred actual player-channel and bounded room-page presentation fixtures.
use super::*;
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    room_opponent_hud::RoomOpponentHud,
    room_presentation::{RoomLobby, RoomPresentation, RoomStatus, RoomUiAction, RoomUiReply},
};

fn room_registry(hosts: usize, slots: usize) -> GroupRoomRegistry {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, hosts, 8, 100).unwrap());
    let players = (0..slots)
        .map(|slot| PlayerId(u32::MAX - slot as u32))
        .collect::<Vec<_>>();
    let ids = (0..hosts)
        .map(|_| {
            registry
                .join("room", b"actual identity", &players, 0)
                .unwrap()
                .id
        })
        .collect::<Vec<_>>();
    registry.seal(ids[0], 1).unwrap();
    for id in ids {
        registry.ready(id, 2).unwrap();
    }
    registry
}

fn presentation(
    registry: &GroupRoomRegistry,
    hud: Option<&RoomOpponentHud>,
) -> Arc<RoomPresentation> {
    let room = registry.room("room").unwrap();
    let lobby = RoomLobby::new(
        Some(room.members[0].id),
        1,
        Some(room.phase),
        room.deadline_ns,
        room.members.to_vec(),
    )
    .unwrap();
    Arc::new(RoomPresentation::new(Arc::new(lobby), RoomStatus::Waiting, hud, None).unwrap())
}

#[test]
fn ui_queue_credit_is_not_network_admission_and_unread_replies_remain_bounded() {
    let registry = room_registry(2, 1);
    let dto = presentation(&registry, None);
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_room(dto.clone()).unwrap();
        let held = viewer.0.room.lock().unwrap();
        assert_eq!(
            viewer.request_room(RoomUiAction::Leave).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(held);
        let mut ids = Vec::new();
        for _ in 0..16 {
            ids.push(viewer.request_room(RoomUiAction::Leave).unwrap());
        }
        assert!(ids[0] > 0 && ids.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            viewer.request_room(RoomUiAction::Leave).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(
            viewer.take_room_reply().unwrap().is_none(),
            "UI enqueue is not a protocol result"
        );
        let first = take_room_request().unwrap().unwrap();
        assert_eq!(first.id, ids[0]);
        assert_eq!(first.action, RoomUiAction::Leave);
        reply_room(RoomUiReply {
            id: first.id,
            result: Err("room already closed remotely".into()),
        })
        .unwrap();
        assert_eq!(
            viewer.request_room(RoomUiAction::Leave).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        let reply = viewer.take_room_reply().unwrap().unwrap();
        assert_eq!(reply.id, first.id);
        assert_eq!(reply.result.unwrap_err(), "room already closed remotely");
        assert!(
            reply_room(RoomUiReply {
                id: first.id,
                result: Ok(())
            })
            .is_err()
        );
        assert!(
            reply_room(RoomUiReply {
                id: u64::MAX,
                result: Ok(())
            })
            .is_err()
        );
        let replacement = viewer.request_room(RoomUiAction::Leave).unwrap();
        assert!(replacement > *ids.last().unwrap());
        let mut delivered = Vec::new();
        while let Some(request) = take_room_request().unwrap() {
            delivered.push(request.id);
            reply_room(RoomUiReply {
                id: request.id,
                result: Ok(()),
            })
            .unwrap();
        }
        assert_eq!(
            delivered,
            ids[1..]
                .iter()
                .copied()
                .chain([replacement])
                .collect::<Vec<_>>()
        );
        assert_eq!(
            viewer.request_room(RoomUiAction::Leave).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        for id in delivered {
            assert_eq!(
                viewer.take_room_reply().unwrap().unwrap(),
                RoomUiReply { id, result: Ok(()) }
            );
        }
        assert!(viewer.take_room_reply().unwrap().is_none());
        assert_eq!(
            viewer.take_latest().unwrap().room.as_deref(),
            Some(dto.as_ref())
        );
        close_room_controls();
        Ok(())
    })
    .unwrap();
}

#[test]
fn latest_room_retry_and_cancelled_old_controls_cannot_mutate_a_fresh_session() {
    let registry = room_registry(3, 2);
    let room = registry.room("room").unwrap();
    let hud = RoomOpponentHud::new(room.members[0].id, room.members).unwrap();
    let dto = presentation(&registry, Some(&hud));
    let (publisher, viewer) = channel();
    let mut ids = Vec::new();
    with_publisher(publisher, || {
        let held = viewer.0.latest.lock().unwrap();
        publish_room(dto.clone()).unwrap();
        drop(held);
        retry_room_publication();
        let latest = viewer.take_latest().unwrap();
        assert!(Arc::ptr_eq(latest.room.as_ref().unwrap(), &dto));
        ids.push(viewer.request_room(RoomUiAction::Leave).unwrap());
        ids.push(viewer.request_room(RoomUiAction::Page(0)).unwrap());
        let acquired = take_room_request().unwrap().unwrap();
        assert_eq!(acquired.id, ids[0]);
        viewer.cancel();
        assert!(cancelled());
        assert!(take_room_request().unwrap().is_none());
        reply_room(RoomUiReply {
            id: acquired.id,
            result: Ok(()),
        })
        .unwrap();
        close_room_controls();
        assert!(viewer.request_room(RoomUiAction::Seal).is_err());
        Ok(())
    })
    .unwrap();
    let mut settled = Vec::new();
    for _ in &ids {
        let reply = viewer.take_room_reply().unwrap().unwrap();
        settled.push(reply.id);
        assert!(reply.result.is_err());
    }
    settled.sort_unstable();
    assert_eq!(settled, ids);
    assert!(viewer.take_room_reply().unwrap().is_none());
    let (fresh, new_viewer) = channel();
    with_publisher(fresh, || {
        publish_room(dto.clone()).unwrap();
        viewer.cancel();
        assert!(!cancelled());
        assert!(viewer.request_room(RoomUiAction::Leave).is_err());
        assert!(take_room_request().unwrap().is_none());
        let id = new_viewer.request_room(RoomUiAction::Page(0)).unwrap();
        let request = take_room_request().unwrap().unwrap();
        assert_eq!(request.id, id);
        reply_room(RoomUiReply { id, result: Ok(()) }).unwrap();
        assert!(
            new_viewer
                .take_room_reply()
                .unwrap()
                .unwrap()
                .result
                .is_ok()
        );
        close_room_controls();
        Ok(())
    })
    .unwrap();
}

#[test]
fn cached_presentation_contains_only_four_qualified_rows_and_retains_shared_lobby() {
    let registry = room_registry(64, 64);
    let room = registry.room("room").unwrap();
    let lobby = Arc::new(
        RoomLobby::new(
            Some(room.members[0].id),
            9,
            Some(room.phase),
            room.deadline_ns,
            room.members.to_vec(),
        )
        .unwrap(),
    );
    let mut hud = RoomOpponentHud::new(room.members[0].id, room.members).unwrap();
    let remote = room.members.last().unwrap();
    hud.update(
        remote.id,
        &GroupPrefix {
            sequence: u64::MAX,
            final_prefix: true,
            members: remote
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
    assert_eq!(hud.entry_count(), 4032);
    assert_eq!(hud.page_count(), 1008);
    let mut seen = Vec::new();
    let mut last = None;
    for page in 0..1008 {
        hud.set_page(page).unwrap();
        let dto =
            RoomPresentation::new(lobby.clone(), RoomStatus::Connected, Some(&hud), None).unwrap();
        assert!(Arc::ptr_eq(&dto.lobby, &lobby));
        assert_eq!((dto.page, dto.pages), (page, 1008));
        assert_eq!(dto.rows.len(), 4);
        seen.extend(dto.rows.iter().map(|row| (row.participant, row.player)));
        last = Some(dto);
    }
    let expected = room.members[1..]
        .iter()
        .flat_map(|member| {
            member
                .players
                .iter()
                .map(move |&player| (member.id, player))
        })
        .collect::<Vec<_>>();
    assert_eq!(seen, expected);
    let last = last.unwrap();
    assert!(last.rows.last().unwrap().counters[0].contains("18446744073709551615"));
    assert_eq!(
        last.rows.last().unwrap().progress.unwrap().song_ns,
        i64::MAX
    );
    assert!(last.rows.last().unwrap().final_prefix);
    assert!(last.allows(RoomUiAction::Page(0)));
    assert!(!last.allows(RoomUiAction::Page(1008)));
    assert!(!last.allows(RoomUiAction::Seal));
    assert!(!last.allows(RoomUiAction::Ready));
    #[cfg(feature = "graphics")]
    {
        use crate::{
            bga_render::BgaFrame,
            scene::Scene,
            ui::organisms::{self, LocalPlayerView},
            playfield_layout::local_touch_bounds,
        };
        let source = beatkernel_bms::parse(
            "#BPM 120\n#WAV01 key.wav\n#00011:0100\n#00012:0001\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let chart = PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap();
        let scores = [
            ScoreSummary::default(),
            ScoreSummary::default(),
            ScoreSummary::default(),
            ScoreSummary::default(),
        ];
        for count in [3, 4] {
            let views = (0..count)
                .map(|slot| LocalPlayerView {
                    player: PlayerId(u32::MAX - slot as u32),
                    chart: Some(&chart),
                    song_time: Some(Timestamp::ZERO),
                    score: &scores[slot],
                    gauge: None,
                    last_judge: None,
                    recent_results: &[],
                    pressed_lanes: 0,
                    note_progress: None,
                    competition: None,
                })
                .collect::<Vec<_>>();
            let mut scene = Scene::new(960, 720);
            organisms::local_player_views_with_background(
                &mut scene,
                &views,
                2_000_000_000,
                0,
                true,
                &[BgaFrame::default(); 4],
            )
            .unwrap();
            let rectangles = scene
                .rectangles()
                .iter()
                .map(|rect| {
                    (
                        rect.bounds.map(f32::to_bits),
                        rect.color.map(f32::to_bits),
                        rect.uv.map(f32::to_bits),
                    )
                })
                .collect::<Vec<_>>();
            let fields = scene
                .playfields()
                .iter()
                .map(|field| (field.top, field.bottom))
                .collect::<Vec<_>>();
            let selected = last.rows.as_ptr();
            organisms::room_presentation_footer(&mut scene, &last).unwrap();
            assert_eq!(last.rows.as_ptr(), selected);
            assert_eq!(
                scene
                    .playfields()
                    .iter()
                    .map(|field| (field.top, field.bottom))
                    .collect::<Vec<_>>(),
                fields
            );
            assert_eq!(
                scene.rectangles()[..rectangles.len()]
                    .iter()
                    .map(|rect| (
                        rect.bounds.map(f32::to_bits),
                        rect.color.map(f32::to_bits),
                        rect.uv.map(f32::to_bits)
                    ))
                    .collect::<Vec<_>>(),
                rectangles
            );
            for (slot, field) in scene.playfields().iter().enumerate() {
                let contact = local_touch_bounds(&chart.lanes, count, slot).unwrap();
                assert_eq!(field.top, contact[1]);
                assert!(field.bottom <= contact[3] && contact[3] < 646.0);
            }
            let footer = &scene.rectangles()[rectangles.len()..];
            assert!(!footer.is_empty());
            for rectangle in footer {
                let [x, y, width, height] = rectangle.bounds;
                assert!(x >= 0.0 && x + width <= 600.0 && y >= 646.0 && y + height <= 720.0);
            }
        }
    }
}
