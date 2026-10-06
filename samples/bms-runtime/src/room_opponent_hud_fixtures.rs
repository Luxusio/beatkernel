//! Deferred source fixtures: actual room admission and retained HUD model.
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    multiplayer_rooms::ParticipantId,
    room_opponent_hud::{RoomHudStatus, RoomOpponentHud},
};

fn prepared(hosts: usize, players: usize) -> GroupRoomRegistry {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, hosts, 8, 100).unwrap());
    let roster = (0..players)
        .map(|index| PlayerId(u32::MAX - index as u32))
        .collect::<Vec<_>>();
    let ids = (0..hosts)
        .map(|_| {
            registry
                .join("hud", b"actual identity", &roster, 0)
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

fn prefix(players: &[PlayerId], sequence: u64, progress: Progress) -> GroupPrefix {
    GroupPrefix {
        sequence,
        final_prefix: false,
        members: players
            .iter()
            .map(|&player| MemberProgress { player, progress })
            .collect(),
    }
}

fn score(song_ns: i64, hits: u64) -> Progress {
    Progress {
        song_ns,
        hits,
        misses: 0,
        combo: hits,
        max_combo: hits,
    }
}

#[test]
fn prepared_host_order_and_scoped_player_ids_cover_every_page_without_truncation() {
    for hosts in [2, 3, 4, 64] {
        for players in [1, 64] {
            let registry = prepared(hosts, players);
            let room = registry.room("hud").unwrap();
            // Exclude an interior host too, rather than assuming the creator is local.
            let own = room.members[hosts / 2].id;
            let mut hud = RoomOpponentHud::new(own, room.members).unwrap();
            assert_eq!(hud.entry_count(), (hosts - 1) * players);
            assert_eq!(hud.page_count(), ((hosts - 1) * players).div_ceil(4));
            if hosts == 64 && players == 64 {
                assert_eq!(hud.entry_count(), 4032);
                assert_eq!(hud.page_count(), 1008);
            }
            let expected = room
                .members
                .iter()
                .filter(|member| member.id != own)
                .flat_map(|member| {
                    member
                        .players
                        .iter()
                        .map(move |&player| (member.id, player))
                })
                .collect::<Vec<_>>();
            let mut observed = Vec::new();
            for page in 0..hud.page_count() {
                hud.set_page(page).unwrap();
                assert_eq!(hud.page_index(), page);
                assert!(!hud.page().is_empty());
                assert!(hud.page().len() <= 4);
                for row in hud.page() {
                    assert_ne!(row.participant, own);
                    assert_eq!(row.progress, None);
                    assert!(!row.final_prefix);
                    observed.push((row.participant, row.player));
                }
            }
            assert_eq!(observed, expected);
            let before = hud.clone();
            assert!(hud.set_page(hud.page_count()).is_err());
            assert_eq!(hud, before);
            assert!(hud.set_page(usize::MAX).is_err());
            assert_eq!(hud, before);
            assert!(RoomOpponentHud::new(ParticipantId(0), room.members).is_err());
            let mut bad = room.members.to_vec();
            bad.last_mut().unwrap().prepared = false;
            assert!(RoomOpponentHud::new(own, &bad).is_err());
            bad = room.members.to_vec();
            bad.last_mut().unwrap().players.push(PlayerId(0));
            assert!(RoomOpponentHud::new(own, &bad).is_err());
            bad = room.members.to_vec();
            bad[1].id = bad[0].id;
            assert!(RoomOpponentHud::new(own, &bad).is_err());
        }
    }
}

#[test]
fn exact_integer_prefixes_and_cached_text_preserve_original_member_and_host_identity() {
    let registry = prepared(3, 2);
    let mut members = registry.room("hud").unwrap().members.to_vec();
    members[1].id = ParticipantId(9_007_199_254_740_993);
    members[2].id = ParticipantId(u64::MAX);
    let mut hud = RoomOpponentHud::new(members[0].id, &members).unwrap();
    let initial = prefix(&members[1].players, 1, score(i64::MIN, 0));
    hud.update(members[1].id, &initial).unwrap();
    let mut last = prefix(&members[2].players, u64::MAX, score(i64::MAX, u64::MAX));
    last.final_prefix = true;
    hud.update(members[2].id, &last).unwrap();
    assert_eq!(hud.page()[0].progress, Some(score(i64::MIN, 0)));
    assert_eq!(hud.page()[2].progress, Some(score(i64::MAX, u64::MAX)));
    assert_eq!(hud.page()[0].player, hud.page()[2].player);
    assert_ne!(hud.page()[0].participant, hud.page()[2].participant);
    assert!(hud.page()[2].label().contains("18446744073709551615"));
    assert!(hud.page()[2].label().contains("4294967295"));
    assert!(hud.page()[2].final_prefix);
    assert!(hud.page()[2].counters()[0].contains("18446744073709551615"));
    assert!(hud.page()[2].counters()[1].contains("18446744073709551615"));
    let remote_label = hud.page()[2].label().as_ptr();
    let remote_counters = hud.page()[2].counters()[0].as_ptr();
    for (sequence, song) in [(2, 72_000_000_000_000), (3, 604_800_000_000_000)] {
        hud.update(
            members[1].id,
            &prefix(&members[1].players, sequence, score(song, 9)),
        )
        .unwrap();
        assert_eq!(hud.page()[0].progress.unwrap().song_ns, song);
        assert_eq!(hud.page()[2].label().as_ptr(), remote_label);
        assert_eq!(hud.page()[2].counters()[0].as_ptr(), remote_counters);
    }
    let retained = hud.clone();
    hud.set_page(0).unwrap();
    assert_eq!(hud, retained);
    assert_eq!(hud.page()[2].label().as_ptr(), remote_label);
    assert_eq!(initial.members[0].progress.song_ns, i64::MIN);
    assert_eq!(last.members[0].progress.hits, u64::MAX);
}

#[test]
fn malformed_later_rows_and_prefix_regressions_refuse_atomically_and_final_is_immutable() {
    let registry = prepared(3, 3);
    let room = registry.room("hud").unwrap();
    let own = room.members[0].id;
    let remote = &room.members[1];
    let mut hud = RoomOpponentHud::new(own, room.members).unwrap();
    hud.update(remote.id, &prefix(&remote.players, 7, score(100, 4)))
        .unwrap();
    let valid = prefix(&remote.players, 9, score(200, 5));
    let mut invalid = Vec::new();
    for sequence in [0, 6, 7] {
        let mut candidate = valid.clone();
        candidate.sequence = sequence;
        invalid.push(candidate);
    }
    let mut candidate = valid.clone();
    candidate.members.swap(0, 2);
    invalid.push(candidate);
    let mut candidate = valid.clone();
    candidate.members.pop();
    invalid.push(candidate);
    let mut candidate = valid.clone();
    candidate.members[2].player = PlayerId(0);
    invalid.push(candidate);
    let mut candidate = valid.clone();
    candidate.members[2].player = candidate.members[0].player;
    invalid.push(candidate);
    let mut candidate = valid.clone();
    candidate.members[2].progress.song_ns = 99;
    invalid.push(candidate);
    let mut candidate = valid.clone();
    candidate.members[2].progress = score(200, 3);
    invalid.push(candidate);
    let mut candidate = valid.clone();
    candidate.members[2].progress.combo = 6;
    invalid.push(candidate);
    let mut candidate = valid.clone();
    candidate.members[2].progress = score(200, u64::MAX);
    candidate.members[2].progress.misses = 1;
    invalid.push(candidate);
    for candidate in invalid {
        let before = hud.clone();
        assert!(hud.update(remote.id, &candidate).is_err());
        assert_eq!(
            hud, before,
            "no accepted early row or sequence may survive a rejected tail"
        );
    }
    for id in [own, ParticipantId(u64::MAX)] {
        let before = hud.clone();
        assert!(hud.update(id, &valid).is_err());
        assert_eq!(hud, before);
    }
    hud.update(remote.id, &valid).unwrap(); // A skipped positive sequence is a retained latest prefix.
    let mut final_prefix = valid.clone();
    final_prefix.sequence = 10;
    final_prefix.final_prefix = true;
    hud.update(remote.id, &final_prefix).unwrap();
    let before = hud.clone();
    final_prefix.sequence = 11;
    assert!(hud.update(remote.id, &final_prefix).is_err());
    assert_eq!(hud, before);
    let other = &room.members[2];
    hud.update(other.id, &prefix(&other.players, 1, score(-1, 1)))
        .unwrap();
    assert_eq!(hud.page()[0].progress, Some(score(200, 5)));
}

#[test]
fn status_failure_and_paging_preserve_the_fixed_local_field_and_touch_geometry() {
    use crate::playfield_layout::{local_field_bounds, local_touch_bounds};
    let registry = prepared(4, 3);
    let room = registry.room("hud").unwrap();
    let mut hud = RoomOpponentHud::new(room.members[0].id, room.members).unwrap();
    let fields = (0..4)
        .map(|slot| local_field_bounds(4, slot).unwrap())
        .collect::<Vec<_>>();
    let touches = (0..4)
        .map(|slot| local_touch_bounds(&[0x11, 0x12], 4, slot).unwrap())
        .collect::<Vec<_>>();
    assert!(hud.heading().contains("WAITING"));
    hud.set_status(RoomHudStatus::Connected).unwrap();
    let remote = &room.members[1];
    hud.update(
        remote.id,
        &prefix(&remote.players, 1, score(72_000_000_000_000, 23)),
    )
    .unwrap();
    let distant = &room.members[2];
    hud.update(
        distant.id,
        &prefix(&distant.players, u64::MAX, score(i64::MAX, u64::MAX)),
    )
    .unwrap();
    let retained = hud.page()[0].clone();
    hud.set_status(RoomHudStatus::Disconnected).unwrap();
    assert!(hud.heading().contains("DISCONNECTED"));
    assert_eq!(hud.page()[0], retained);
    let before = hud.clone();
    for status in [RoomHudStatus::Waiting, RoomHudStatus::Connected] {
        assert!(hud.set_status(status).is_err());
        assert_eq!(hud, before);
    }
    assert!(
        hud.update(
            remote.id,
            &prefix(&remote.players, 2, score(72_000_000_000_001, 24))
        )
        .is_err()
    );
    assert_eq!(
        hud, before,
        "disconnection retains the last original prefix without accepting later scores"
    );
    for page in 0..hud.page_count() {
        hud.set_page(page).unwrap();
        assert_eq!(
            (0..4)
                .map(|slot| local_field_bounds(4, slot).unwrap())
                .collect::<Vec<_>>(),
            fields
        );
        assert_eq!(
            (0..4)
                .map(|slot| local_touch_bounds(&[0x11, 0x12], 4, slot).unwrap())
                .collect::<Vec<_>>(),
            touches
        );
        #[cfg(feature = "graphics")]
        {
            use crate::{
                bga_render::BgaFrame,
                competition::ScoreSummary,
                player_chart::PlayerChart,
                scene::Scene,
                ui::organisms::{
                    LocalPlayerView, local_player_views_with_background, room_opponent_footer,
                },
            };
            use beatkernel::time::Timestamp;
            let source = beatkernel_bms::parse(
                "#BPM 120\n#WAV01 key.wav\n#00011:0100\n#00012:0001\n",
                beatkernel_bms::ParseOptions::default(),
            )
            .unwrap();
            let chart =
                PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap();
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
                local_player_views_with_background(
                    &mut scene,
                    &views,
                    2_000_000_000,
                    0,
                    true,
                    &[BgaFrame::default(); 4],
                )
                .unwrap();
                let original_rectangles = scene
                    .rectangles()
                    .iter()
                    .map(|rectangle| {
                        (
                            rectangle.bounds.map(f32::to_bits),
                            rectangle.color.map(f32::to_bits),
                            rectangle.uv.map(f32::to_bits),
                        )
                    })
                    .collect::<Vec<_>>();
                let original_fields = scene
                    .playfields()
                    .iter()
                    .map(|field| (field.top, field.bottom))
                    .collect::<Vec<_>>();
                let first_row = hud.page().as_ptr();
                room_opponent_footer(&mut scene, &hud).unwrap();
                assert_eq!(
                    hud.page().as_ptr(),
                    first_row,
                    "drawing borrows the retained selected page"
                );
                assert_eq!(
                    scene
                        .playfields()
                        .iter()
                        .map(|field| (field.top, field.bottom))
                        .collect::<Vec<_>>(),
                    original_fields
                );
                assert_eq!(scene.playfields().len(), count);
                for (slot, field) in scene.playfields().iter().enumerate() {
                    let contact = local_touch_bounds(&chart.lanes, count, slot).unwrap();
                    assert_eq!(field.top, contact[1]);
                    assert!(field.bottom <= contact[3]);
                    assert!(
                        contact[3] < 646.0,
                        "the fixed footer is outside every local contact field"
                    );
                    assert!(std::ptr::eq(views[slot].score, &scores[slot]));
                }
                assert_eq!(
                    scene.rectangles()[..original_rectangles.len()]
                        .iter()
                        .map(|rectangle| (
                            rectangle.bounds.map(f32::to_bits),
                            rectangle.color.map(f32::to_bits),
                            rectangle.uv.map(f32::to_bits)
                        ))
                        .collect::<Vec<_>>(),
                    original_rectangles
                );
                let footer = &scene.rectangles()[original_rectangles.len()..];
                assert!(!footer.is_empty());
                for rectangle in footer {
                    let [x, y, width, height] = rectangle.bounds;
                    assert!(x >= 0.0 && x + width <= 960.0 && y >= 646.0 && y + height <= 720.0);
                    if y >= 663.0 {
                        assert!(
                            [(12.0, 663.0), (492.0, 663.0), (12.0, 690.0), (492.0, 690.0)]
                                .into_iter()
                                .any(|(left, top)| x >= left
                                    && x + width <= left + 456.0
                                    && y >= top
                                    && y + height <= top + 27.0)
                        );
                    }
                }
            }
        }
    }
    hud.mark_failed();
    assert!(hud.failed());
    assert!(hud.page().is_empty());
    assert!(hud.heading().contains("UNAVAILABLE"));
    let failed = hud.clone();
    assert!(hud.set_page(0).is_err());
    assert!(hud.set_status(RoomHudStatus::Disconnected).is_err());
    assert!(
        hud.update(
            remote.id,
            &prefix(&remote.players, 2, score(72_000_000_000_001, 24))
        )
        .is_err()
    );
    hud.mark_failed();
    assert_eq!(hud, failed);
    #[cfg(feature = "graphics")]
    {
        let mut scene = crate::scene::Scene::new(960, 720);
        crate::ui::organisms::room_opponent_footer(&mut scene, &hud).unwrap();
        assert!(!scene.rectangles().is_empty());
        assert!(
            scene
                .rectangles()
                .iter()
                .all(|rectangle| rectangle.bounds[1] >= 646.0
                    && rectangle.bounds[1] + rectangle.bounds[3] <= 663.0)
        );
        assert!(scene.playfields().is_empty());
    }
}
