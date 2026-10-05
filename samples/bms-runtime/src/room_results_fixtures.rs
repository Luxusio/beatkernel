//! Deferred immutable archive fixtures; all score rows use the actual HUD.
use super::*;
use crate::{
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    room_opponent_hud::RoomHudStatus,
};

fn prepared_members(hosts: usize, slots: usize) -> Vec<GroupRoomMember> {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, hosts, 8, 100).unwrap());
    let players = (0..slots)
        .map(|index| PlayerId(u32::MAX - index as u32))
        .collect::<Vec<_>>();
    let ids = (0..hosts)
        .map(|_| {
            registry
                .join("results", b"actual identity", &players, 0)
                .unwrap()
                .id
        })
        .collect::<Vec<_>>();
    registry.seal(ids[0], 1).unwrap();
    for id in ids {
        registry.ready(id, 2).unwrap();
    }
    registry.room("results").unwrap().members.to_vec()
}
fn lobby(members: &[GroupRoomMember], own: ParticipantId) -> Arc<RoomLobby> {
    Arc::new(
        RoomLobby::new(
            Some(own),
            7,
            Some(GroupRoomPhase::Prepared),
            None,
            members.to_vec(),
        )
        .unwrap(),
    )
}
fn prefix(member: &GroupRoomMember, sequence: u64, final_prefix: bool, hits: u64) -> GroupPrefix {
    GroupPrefix {
        sequence,
        final_prefix,
        members: member
            .players
            .iter()
            .map(|&player| MemberProgress {
                player,
                progress: Progress {
                    song_ns: i64::MAX,
                    hits,
                    misses: 0,
                    combo: hits,
                    max_combo: hits,
                },
            })
            .collect(),
    }
}

#[test]
fn archive_keeps_all_qualified_players_and_pages_without_cloning_the_live_hud() {
    for (hosts, slots) in [(2, 3), (3, 3), (4, 3), (64, 64)] {
        let mut members = prepared_members(hosts, slots);
        // The real archive/HUD validators must preserve valid full-width host
        // identity; these explicit IDs do not claim a transport authenticated them.
        for (index, member) in members.iter_mut().enumerate() {
            member.id = ParticipantId(u64::MAX - index as u64);
        }
        let own = members[hosts / 2].id;
        let shared = lobby(&members, own);
        let mut hud = RoomOpponentHud::new(own, &members).unwrap();
        let first_remote = members.iter().find(|member| member.id != own).unwrap();
        let accepted = prefix(first_remote, u64::MAX, true, u64::MAX);
        hud.update(first_remote.id, &accepted).unwrap();
        hud.set_status(RoomHudStatus::Disconnected).unwrap();
        hud.set_page(hud.page_count() - 1).unwrap();
        let archive = Arc::new(
            RoomResults::new(
                shared.clone(),
                &hud,
                true,
                Some("cancelled after retained prefix".into()),
            )
            .unwrap(),
        );
        assert!(Arc::ptr_eq(archive.lobby(), &shared));
        assert!(archive.cancelled());
        assert!(!archive.failed());
        assert_eq!(archive.error(), Some("cancelled after retained prefix"));
        assert_eq!(archive.rows().len(), (hosts - 1) * slots);
        assert_eq!(archive.initial_page(), hud.page_count() - 1);
        assert_eq!(archive.page_count(), ((hosts - 1) * slots).div_ceil(4));
        if hosts == 64 {
            assert_eq!((archive.rows().len(), archive.page_count()), (4032, 1008));
        }
        let expected = members
            .iter()
            .filter(|member| member.id != own)
            .flat_map(|member| {
                member
                    .players
                    .iter()
                    .map(move |&player| (member.id, player))
            })
            .collect::<Vec<_>>();
        let stored = archive.rows().as_ptr();
        let mut actual = Vec::new();
        for page in 0..archive.page_count() {
            let view = archive.project(page).unwrap();
            assert_eq!(view.status, RoomStatus::Closed);
            assert!(Arc::ptr_eq(&view.lobby, archive.lobby()));
            assert_eq!((view.page, view.pages), (page, archive.page_count()));
            assert_eq!(view.rows.len(), (expected.len() - page * 4).min(4));
            assert_eq!(
                view.rows.as_slice(),
                &archive.rows()[page * 4..(page * 4 + 4).min(expected.len())]
            );
            for action in [RoomUiAction::Seal, RoomUiAction::Ready, RoomUiAction::Leave] {
                assert!(!view.allows(action));
            }
            actual.extend(view.rows.iter().map(|row| (row.participant, row.player)));
        }
        assert_eq!(actual, expected);
        assert_eq!(archive.rows().as_ptr(), stored);
        assert_eq!(
            archive.rows()[0].progress,
            Some(accepted.members[0].progress)
        );
        assert!(archive.rows()[0].final_prefix);
        assert!(archive.rows()[0].counters[0].contains("18446744073709551615"));
        assert!(
            archive
                .rows()
                .iter()
                .skip(slots)
                .all(|row| row.progress.is_none() && !row.final_prefix)
        );
        let held = archive.project(archive.initial_page()).unwrap();
        assert!(archive.project(archive.page_count()).is_err());
        assert!(archive.project(usize::MAX).is_err());
        assert_eq!(archive.project(archive.initial_page()).unwrap(), held);
        drop(hud);
        drop(members);
        assert_eq!(archive.rows().as_ptr(), stored);
        assert_eq!(
            archive.project(0).unwrap().rows[0].progress,
            Some(accepted.members[0].progress)
        );
    }
}

#[test]
fn archive_is_a_snapshot_and_failed_presentation_stays_unavailable_with_original_evidence() {
    let members = prepared_members(3, 3);
    let shared = lobby(&members, members[0].id);
    let mut hud = RoomOpponentHud::new(members[0].id, &members).unwrap();
    let first = prefix(&members[1], 1, false, 7);
    hud.update(members[1].id, &first).unwrap();
    hud.set_page(1).unwrap();
    let before = RoomResults::new(shared.clone(), &hud, false, None).unwrap();
    let final_prefix = prefix(&members[1], 2, true, 8);
    hud.update(members[1].id, &final_prefix).unwrap();
    let mut other_members = members.clone();
    other_members[2].players.reverse();
    assert!(RoomResults::new(lobby(&other_members, members[0].id), &hud, false, None).is_err());
    assert!(
        RoomResults::new(shared.clone(), &hud, false, Some("invalid\ncontrol".into())).is_err()
    );
    let after = RoomResults::new(
        shared.clone(),
        &hud,
        true,
        Some("connection reset after accepted prefix".into()),
    )
    .unwrap();
    assert_eq!(before.rows()[0].progress, Some(first.members[0].progress));
    assert!(!before.rows()[0].final_prefix);
    assert_eq!(
        after.rows()[0].progress,
        Some(final_prefix.members[0].progress)
    );
    assert!(after.rows()[0].final_prefix);
    assert_eq!(after.project(1).unwrap().rows.len(), 2);
    assert!(
        after
            .project(1)
            .unwrap()
            .rows
            .iter()
            .all(|row| row.progress.is_none())
    );
    hud.mark_failed();
    let unavailable = RoomResults::new(
        shared,
        &hud,
        true,
        Some("room score presentation unavailable".into()),
    )
    .unwrap();
    assert!(unavailable.failed());
    assert_eq!(
        unavailable.rows(),
        after.rows(),
        "failure may retain evidence but cannot display invented values"
    );
    let view = unavailable.project(unavailable.initial_page()).unwrap();
    assert!(view.failed);
    assert!(view.rows.is_empty());
    assert!(view.heading.contains("UNAVAILABLE"));
    assert_eq!(before.rows()[0].progress, Some(first.members[0].progress));
    #[cfg(feature = "graphics")]
    {
        use crate::{
            bga_render::BgaFrame,
            competition::ScoreSummary,
            player_chart::PlayerChart,
            playfield_layout::local_touch_bounds,
            scene::Scene,
            ui::organisms::{self, LocalPlayerView},
        };
        let source =
            beatkernel_bms::parse("#BPM 120\n#00011:0100\n#00012:0001\n", Default::default())
                .unwrap();
        let chart = PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap();
        let scores: [ScoreSummary; 4] = std::array::from_fn(|_| ScoreSummary::default());
        for view in [before.project(0).unwrap(), after.project(1).unwrap(), view] {
            let mut scene = Scene::new(960, 720);
            let views = (0..4)
                .map(|slot| LocalPlayerView {
                    player: PlayerId(u32::MAX - slot as u32),
                    chart: Some(&chart),
                    song_time: Some(beatkernel::time::Timestamp::ZERO),
                    score: &scores[slot],
                    gauge: None,
                    last_judge: None,
                    recent_results: &[],
                    pressed_lanes: 0,
                    note_progress: None,
                    competition: None,
                })
                .collect::<Vec<_>>();
            organisms::local_player_views_with_background(
                &mut scene,
                &views,
                2_000_000_000,
                0,
                true,
                &[BgaFrame::default(); 4],
            )
            .unwrap();
            let fields = scene
                .playfields()
                .iter()
                .map(|field| (field.top, field.bottom))
                .collect::<Vec<_>>();
            let previous = scene.rectangles().len();
            let rows = view.rows.as_ptr();
            organisms::room_presentation_footer(&mut scene, &view).unwrap();
            assert_eq!(view.rows.as_ptr(), rows);
            assert_eq!(
                scene
                    .playfields()
                    .iter()
                    .map(|field| (field.top, field.bottom))
                    .collect::<Vec<_>>(),
                fields
            );
            for (slot, field) in scene.playfields().iter().enumerate() {
                let bounds = local_touch_bounds(&chart.lanes, 4, slot).unwrap();
                assert_eq!(field.top, bounds[1]);
                assert!(field.bottom <= bounds[3] && bounds[3] < 646.0);
            }
            assert!(scene.rectangles().len() > previous);
            for rectangle in &scene.rectangles()[previous..] {
                assert!(rectangle.bounds[0] >= 0.0 && rectangle.bounds[1] >= 646.0);
                assert!(rectangle.bounds[0] + rectangle.bounds[2] <= 600.0);
                assert!(rectangle.bounds[1] + rectangle.bounds[3] <= 720.0);
            }
        }
    }
}
