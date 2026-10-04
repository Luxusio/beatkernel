//! Deferred portable coverage for the browser's one-time retained archive.
use crate::{
    local_players::PlayerId,
    multiplayer_group_rooms::{GroupRoomMember, GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_rooms::ParticipantId,
    room_presentation::RoomStatus,
    room_results_builder::{decode_room_roster, RoomResultsBuilder},
};
use std::sync::Arc;

fn prepared(hosts: usize, players: usize) -> Vec<GroupRoomMember> {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, hosts, 8, 100).unwrap());
    let roster = (0..players)
        .map(|n| PlayerId(u32::MAX - n as u32))
        .collect::<Vec<_>>();
    let ids = (0..hosts)
        .map(|_| {
            registry
                .join("results", b"shared identity", &roster, 0)
                .unwrap()
                .id
        })
        .collect::<Vec<_>>();
    registry.seal(ids[0], 1).unwrap();
    for id in ids {
        registry.ready(id, 2).unwrap();
    }
    let mut members = registry.room("results").unwrap().members.to_vec();
    // These codec identities exercise the full domain, not transport authentication.
    for (index, member) in members.iter_mut().enumerate() {
        member.id = ParticipantId(u64::MAX - index as u64);
    }
    members
}

fn roster_words(members: &[GroupRoomMember]) -> Vec<u32> {
    members
        .iter()
        .flat_map(|member| {
            let mut words = vec![
                member.id.0 as u32,
                (member.id.0 >> 32) as u32,
                member.players.len() as u32,
            ];
            words.extend(member.players.iter().map(|player| player.0));
            words
        })
        .collect()
}

fn maximum_prefix(member: &GroupRoomMember) -> Vec<u32> {
    member
        .players
        .iter()
        .flat_map(|player| {
            [
                player.0,
                u32::MAX,
                0x7fff_ffff,
                u32::MAX,
                u32::MAX,
                0,
                0,
                u32::MAX,
                u32::MAX,
                u32::MAX,
                u32::MAX,
            ]
        })
        .collect()
}

#[test]
fn builder_owns_full_qualified_rosters_and_freezes_one_archive_for_every_selected_page() {
    for (hosts, players) in [(2, 3), (3, 3), (4, 3), (64, 64)] {
        let members = prepared(hosts, players);
        let own = members[hosts / 2].id;
        let mut words = roster_words(&members);
        assert_eq!(decode_room_roster(&words).unwrap(), members);
        let mut builder = RoomResultsBuilder::new(own, &words).unwrap();
        assert_eq!(builder.pages(), 0);
        assert!(builder.archive().is_none());
        assert!(builder.presentation().is_none());
        assert!(builder.set_page(0).is_err());
        words.fill(0);
        let peer = members.iter().find(|member| member.id != own).unwrap();
        let mut prefix = maximum_prefix(peer);
        builder.update(peer.id, u64::MAX, true, &prefix).unwrap();
        prefix.fill(0);
        let pages = ((hosts - 1) * players).div_ceil(4);
        builder
            .freeze(
                pages - 1,
                true,
                Some("connection ended after accepted prefix".into()),
                false,
            )
            .unwrap();
        let archive = builder.archive().unwrap().clone();
        assert_eq!(archive.initial_page(), pages - 1);
        assert_eq!(archive.rows().len(), (hosts - 1) * players);
        assert_eq!(builder.pages(), pages);
        assert!(archive.cancelled());
        assert!(!builder.failed());
        assert_eq!(
            archive.error(),
            Some("connection ended after accepted prefix")
        );
        assert_eq!(archive.rows()[0].participant, peer.id);
        let progress = archive.rows()[0].progress.unwrap();
        assert_eq!(
            (
                progress.song_ns,
                progress.hits,
                progress.misses,
                progress.combo,
                progress.max_combo
            ),
            (i64::MAX, u64::MAX, 0, u64::MAX, u64::MAX)
        );
        assert!(archive.rows()[0].final_prefix);
        assert!(
            archive
                .rows()
                .iter()
                .skip(players)
                .all(|row| row.progress.is_none() && !row.final_prefix)
        );
        let expected = members
            .iter()
            .filter(|member| member.id != own)
            .flat_map(|member| {
                member
                    .players
                    .iter()
                    .map(move |player| (member.id, *player))
            })
            .collect::<Vec<_>>();
        let mut actual = Vec::new();
        for page in 0..pages {
            builder.set_page(page).unwrap();
            let view = builder.presentation().unwrap();
            assert_eq!(
                (view.status, view.page, view.pages),
                (RoomStatus::Closed, page, pages)
            );
            assert!(view.rows.len() <= 4);
            actual.extend(view.rows.iter().map(|row| (row.participant, row.player)));
            assert!(Arc::ptr_eq(builder.archive().unwrap(), &archive));
        }
        assert_eq!(actual, expected);
        if hosts == 64 {
            assert_eq!(
                (words.len(), archive.rows().len(), pages),
                (4288, 4032, 1008)
            );
        }
        let held = builder.presentation().unwrap().clone();
        assert!(builder.set_page(pages).is_err());
        assert!(builder.set_page(usize::MAX).is_err());
        assert!(
            builder
                .update(peer.id, 1, false, &maximum_prefix(peer))
                .is_err()
        );
        assert!(builder.freeze(0, false, None, false).is_err());
        assert_eq!(builder.presentation(), Some(&held));
        assert!(Arc::ptr_eq(builder.archive().unwrap(), &archive));
    }
}

#[test]
fn malformed_rosters_and_prefixes_refuse_atomically_while_failed_archive_preserves_missing_and_original_rows()
 {
    let members = prepared(3, 3);
    let valid = roster_words(&members);
    let mut malformed = vec![
        Vec::new(),
        valid[..valid.len() - 1].to_vec(),
        vec![1, 0, 0],
        vec![1, 0, 65],
    ];
    let mut zero_host = valid.clone();
    zero_host[0] = 0;
    zero_host[1] = 0;
    malformed.push(zero_host);
    let mut duplicate_host = valid.clone();
    duplicate_host[6] = valid[0];
    duplicate_host[7] = valid[1];
    malformed.push(duplicate_host);
    let mut zero_player = valid.clone();
    zero_player[3] = 0;
    malformed.push(zero_player);
    let mut duplicate_player = valid.clone();
    duplicate_player[4] = duplicate_player[3];
    malformed.push(duplicate_player);
    let mut trailing = valid.clone();
    trailing.push(9);
    malformed.push(trailing);
    let mut too_many = roster_words(&prepared(64, 1));
    too_many.extend_from_slice(&[1, 0, 1, 1]);
    malformed.push(too_many);
    for words in malformed {
        assert!(decode_room_roster(&words).is_err());
        assert!(RoomResultsBuilder::new(members[0].id, &words).is_err());
    }
    assert!(RoomResultsBuilder::new(ParticipantId(7), &valid).is_err());
    let mut builder = RoomResultsBuilder::new(members[0].id, &valid).unwrap();
    let peer = &members[1];
    let words = maximum_prefix(peer);
    builder.update(peer.id, 1, false, &words).unwrap();
    let mut bad_counter = words.clone();
    bad_counter[5] = 1; // hits + misses overflows u64.
    let mut bad_tail = words.clone();
    bad_tail[22] = bad_tail[0];
    let mut wrong_order = words.clone();
    wrong_order.swap(0, 11);
    for rejected in [
        Vec::new(),
        words[..32].to_vec(),
        bad_counter,
        bad_tail,
        wrong_order,
    ] {
        assert!(builder.update(peer.id, 2, true, &rejected).is_err());
    }
    assert!(builder.update(members[0].id, 2, false, &words).is_err());
    assert!(builder.update(ParticipantId(17), 2, false, &words).is_err());
    assert!(builder.update(peer.id, 0, false, &words).is_err());
    assert!(builder.update(peer.id, 1, false, &words).is_err());
    builder.update(peer.id, 2, true, &words).unwrap();
    assert!(builder.update(peer.id, 3, false, &words).is_err());
    assert!(builder.freeze(2, false, None, false).is_err());
    assert!(builder.archive().is_none());
    builder
        .freeze(1, true, Some("renderer unavailable".into()), true)
        .unwrap();
    let archive = builder.archive().unwrap();
    assert!(builder.failed());
    assert_eq!(archive.rows().len(), 6);
    assert_eq!(archive.rows()[0].progress.unwrap().hits, u64::MAX);
    assert!(archive.rows()[0].final_prefix);
    assert!(
        archive.rows()[3..]
            .iter()
            .all(|row| row.progress.is_none() && !row.final_prefix)
    );
    assert!(
        builder.presentation().unwrap().rows.is_empty(),
        "unavailable presentation must not invent zero scores"
    );
    assert_eq!(archive.error(), Some("renderer unavailable"));
    let independent = RoomResultsBuilder::new(members[0].id, &valid).unwrap();
    assert!(independent.archive().is_none());
}
