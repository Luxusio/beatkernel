//! Deferred payload fixtures; no connection, start or write receipt is implied.
use crate::{
    local_players::PlayerId,
    multiplayer_group::{
        MemberProgress, decode_prefix, encode_prefix, encode_words, validate_members,
    },
    multiplayer_protocol::Progress,
};

fn member(
    player: u32,
    song_ns: i64,
    hits: u64,
    misses: u64,
    combo: u64,
    max_combo: u64,
) -> MemberProgress {
    MemberProgress {
        player: PlayerId(player),
        progress: Progress {
            song_ns,
            hits,
            misses,
            combo,
            max_combo,
        },
    }
}

#[test]
fn literal_group_payload_and_browser_words_preserve_every_signed_and_unsigned_bit() {
    let members = [
        member(0x01020304, i64::MIN, u64::MAX, 0, u64::MAX, u64::MAX),
        member(u32::MAX, i64::MAX, 0, u64::MAX, 0, 0),
    ];
    // Independently written schema-1 bytes: header, then player/song/four counters.
    let literal = [
        1, 1, 2, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 4, 3, 2, 1, 0, 0, 0, 0, 0, 0,
        0, 0x80, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f, 0, 0, 0, 0, 0, 0,
        0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0,
    ];
    let sequence = 0x8877665544332211;
    assert_eq!(literal.len(), 100);
    assert_eq!(encode_prefix(sequence, true, &members).unwrap(), literal);
    let decoded = decode_prefix(&literal, sequence, None).unwrap();
    assert_eq!(decoded.sequence, sequence);
    assert!(decoded.final_prefix);
    assert_eq!(decoded.members, members);
    assert_eq!(
        encode_words(&members).unwrap(),
        [
            0x01020304, 0, 0x80000000, 0xffffffff, 0xffffffff, 0, 0, 0xffffffff, 0xffffffff,
            0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0x7fffffff, 0, 0, 0xffffffff,
            0xffffffff, 0, 0, 0, 0,
        ]
    );
    for sequence in [0, u64::MAX] {
        let bytes = encode_prefix(sequence, false, &members).unwrap();
        let decoded = decode_prefix(&bytes, sequence, Some(&members)).unwrap();
        assert_eq!(decoded.sequence, sequence);
        assert!(!decoded.final_prefix);
        assert_eq!(decoded.members, members);
        assert!(decode_prefix(&bytes, sequence ^ 1, None).is_err());
    }
}

#[test]
fn exact_sixty_four_member_capacity_keeps_roster_order_and_rejects_invalid_whole_snapshots() {
    let mut members: Vec<_> = (0..64_u32)
        .map(|index| {
            member(
                (index + 1) * 17,
                604_800_000_000_000 + i64::from(index),
                u64::from(index),
                0,
                u64::from(index),
                u64::from(index),
            )
        })
        .collect();
    members[63] = member(u32::MAX, i64::MAX, u64::MAX, 0, u64::MAX, u64::MAX);
    let payload = encode_prefix(u64::MAX, true, &members).unwrap();
    assert_eq!(payload.len(), 2828);
    assert_eq!(&payload[..4], &[1, 1, 64, 0]);
    assert_eq!(
        decode_prefix(&payload, u64::MAX, None).unwrap().members,
        members
    );
    let words = encode_words(&members).unwrap();
    assert_eq!(words.len(), 704);
    assert_eq!(
        words.chunks_exact(11).map(|row| row[0]).collect::<Vec<_>>(),
        members.iter().map(|row| row.player.0).collect::<Vec<_>>()
    );
    assert_eq!(
        &words[693..],
        &[
            0xffffffff, 0xffffffff, 0x7fffffff, 0xffffffff, 0xffffffff, 0, 0, 0xffffffff,
            0xffffffff, 0xffffffff, 0xffffffff,
        ]
    );

    let mut over_capacity = members.clone();
    over_capacity.push(member(3, 0, 0, 0, 0, 0));
    let mut zero_id = members.clone();
    zero_id[63].player = PlayerId(0);
    let mut duplicate = members.clone();
    duplicate[63].player = duplicate[0].player;
    for invalid in [Vec::new(), over_capacity, zero_id, duplicate] {
        assert!(validate_members(None, &invalid).is_err());
        assert!(encode_prefix(0, false, &invalid).is_err());
        assert!(encode_words(&invalid).is_err());
    }
    for invalid_last in [
        member(u32::MAX, 0, 1, 0, 2, 1),
        member(u32::MAX, 0, 1, 0, 0, 2),
        member(u32::MAX, 0, u64::MAX, 1, 0, 0),
    ] {
        let mut invalid = members.clone();
        invalid[63] = invalid_last;
        assert!(validate_members(None, &invalid).is_err());
        assert!(encode_prefix(1, true, &invalid).is_err());
        assert!(encode_words(&invalid).is_err());
    }
}

#[test]
fn malformed_payload_or_later_member_regression_never_admits_a_sibling_prefix() {
    let previous = [member(7, 10, 2, 0, 2, 2), member(u32::MAX, 20, 3, 1, 1, 3)];
    let next = [member(7, 11, 3, 0, 3, 3), member(u32::MAX, 21, 4, 1, 2, 3)];
    let valid = encode_prefix(9, true, &next).unwrap();
    for (offset, value) in [(0, 0), (0, 2), (1, 2), (2, 0), (2, 3), (2, 65), (3, 1)] {
        let mut malformed = valid.clone();
        malformed[offset] = value;
        assert!(decode_prefix(&malformed, 9, Some(&previous)).is_err());
    }
    for length in [0, 11, 12, 55, 99] {
        assert!(decode_prefix(&valid[..length], 9, Some(&previous)).is_err());
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(decode_prefix(&trailing, 9, Some(&previous)).is_err());
    for player in [0_u32, 7] {
        let mut malformed = valid.clone();
        malformed[56..60].copy_from_slice(&player.to_le_bytes());
        assert!(decode_prefix(&malformed, 9, None).is_err());
    }
    let mut invalid_counts = valid.clone();
    invalid_counts[76..84].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(decode_prefix(&invalid_counts, 9, None).is_err());
    assert!(decode_prefix(&valid, 8, Some(&previous)).is_err());

    for invalid_last in [
        member(u32::MAX, 19, 4, 1, 2, 3),
        member(u32::MAX, 21, 2, 1, 1, 2),
        member(u32::MAX, 21, 4, 0, 2, 3),
        member(u32::MAX, 21, 4, 1, 2, 2),
        member(u32::MAX, 21, 4, 1, 3, 3),
        member(u32::MAX, 21, 4, 1, 0, 3),
    ] {
        let invalid = [next[0], invalid_last];
        // Each row is independently valid. Only its transition is forbidden.
        let payload = encode_prefix(9, true, &invalid).unwrap();
        assert!(validate_members(Some(&previous), &invalid).is_err());
        assert!(decode_prefix(&payload, 9, Some(&previous)).is_err());
        assert_eq!(
            decode_prefix(&valid, 9, Some(&previous)).unwrap().members,
            next
        );
    }
    for changed_roster in [
        vec![next[1], next[0]],
        vec![next[0]],
        vec![next[0], member(91, 21, 4, 1, 2, 3)],
    ] {
        let bytes = encode_prefix(9, false, &changed_roster).unwrap();
        assert!(decode_prefix(&bytes, 9, Some(&previous)).is_err());
    }
    for invalid_previous in [
        vec![],
        vec![member(0, 10, 2, 0, 2, 2), previous[1]],
        vec![previous[0], previous[0]],
        vec![member(7, 10, 1, 0, 2, 2), previous[1]],
    ] {
        assert!(validate_members(Some(&invalid_previous), &next).is_err());
        assert!(decode_prefix(&valid, 9, Some(&invalid_previous)).is_err());
    }
    assert_eq!(
        previous,
        [member(7, 10, 2, 0, 2, 2), member(u32::MAX, 20, 3, 1, 1, 3)]
    );
}
