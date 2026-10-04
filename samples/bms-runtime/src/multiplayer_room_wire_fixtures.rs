//! Deferred BKMR codec fixtures; no session, stream, or start agreement is simulated.
use super::*;

const JOIN: &[u8] = &[
    b'B', b'K', b'M', b'R', 1, 0, 1, 16, 0, 0, 0, 3, 0, 0, 0, 0, 255, 17, 2, 255, 255, 255, 255, 4,
    3, 2, 1,
];
const ADMITTED: &[u8] = &[
    b'B', b'K', b'M', b'R', 1, 0, 2, 8, 0, 0, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
];
const SNAPSHOT: &[u8] = &[
    b'B', b'K', b'M', b'R', 1, 0, 3, 42, 0, 0, 0, 1, 8, 7, 6, 5, 4, 3, 2, 1, 2, 0x11, 0x22, 0x33,
    0x44, 0x55, 0x66, 0x77, 0x88, 1, 1, 255, 255, 255, 255, 1, 0, 0, 0, 0, 0, 0, 0, 0, 2, 255, 255,
    255, 255, 7, 0, 0, 0,
];
const SEAL: &[u8] = &[b'B', b'K', b'M', b'R', 1, 0, 4, 0, 0, 0, 0];
const READY: &[u8] = &[b'B', b'K', b'M', b'R', 1, 0, 5, 0, 0, 0, 0];
const LEAVE: &[u8] = &[b'B', b'K', b'M', b'R', 1, 0, 6, 0, 0, 0, 0];

fn join() -> RoomMessage {
    RoomMessage::Join {
        identity: vec![0, 255, 17],
        players: vec![PlayerId(u32::MAX), PlayerId(0x0102_0304)],
    }
}

fn members() -> Vec<GroupRoomMember> {
    vec![
        GroupRoomMember {
            id: ParticipantId(0x8877_6655_4433_2211),
            players: vec![PlayerId(u32::MAX)],
            prepared: true,
        },
        GroupRoomMember {
            id: ParticipantId(1),
            players: vec![PlayerId(u32::MAX), PlayerId(7)],
            prepared: false,
        },
    ]
}

fn snapshot() -> RoomMessage {
    RoomMessage::Snapshot {
        members: members(),
        phase: GroupRoomPhase::Frozen,
        deadline_ns: Some(0x0102_0304_0506_0708),
    }
}

fn feed_fragment(decoder: &mut RoomFrameDecoder, fragment: &[u8]) -> usize {
    let mut consumed = 0;
    while consumed < fragment.len() {
        let copied = decoder.push(&fragment[consumed..]).unwrap();
        assert!(copied > 0, "fragment contains bytes beyond the held frame");
        consumed += copied;
    }
    consumed
}

#[test]
fn literal_frames_pin_room_magic_tags_endianness_and_identity_free_requests() {
    for (message, literal) in [
        (join(), JOIN),
        (
            RoomMessage::Admitted {
                participant: ParticipantId(0x8877_6655_4433_2211),
            },
            ADMITTED,
        ),
        (snapshot(), SNAPSHOT),
        (RoomMessage::Seal, SEAL),
        (RoomMessage::Ready, READY),
        (RoomMessage::Leave, LEAVE),
    ] {
        assert_eq!(encode_message(&message).unwrap(), literal);
        assert_eq!(decode_message(literal).unwrap(), message);
    }
    for request in [RoomMessage::Seal, RoomMessage::Ready, RoomMessage::Leave] {
        assert_eq!(encode_message(&request).unwrap().len(), 11);
    }
}

#[test]
fn maximum_identity_and_two_three_four_sixty_four_host_rosters_keep_full_width_scoped_ids() {
    let players = (0..64)
        .map(|index| PlayerId(u32::MAX - index))
        .collect::<Vec<_>>();
    let maximum = RoomMessage::Join {
        identity: vec![0xA5; 65_536],
        players: players.clone(),
    };
    let encoded = encode_message(&maximum).unwrap();
    assert_eq!(encoded.len(), 65_808);
    assert_eq!(
        &encoded[..11],
        &[b'B', b'K', b'M', b'R', 1, 0, 1, 5, 1, 1, 0]
    );
    assert_eq!(&encoded[11..15], &[0, 0, 1, 0]);
    assert_eq!(decode_message(&encoded).unwrap(), maximum);
    for count in [2usize, 3, 4, 64] {
        let hosts = (0..count)
            .map(|index| GroupRoomMember {
                id: ParticipantId(u64::MAX - index as u64),
                players: players.clone(),
                prepared: true,
            })
            .collect::<Vec<_>>();
        let message = RoomMessage::Snapshot {
            members: hosts,
            phase: GroupRoomPhase::Prepared,
            deadline_ns: None,
        };
        let mut encoded = encode_message(&message).unwrap();
        assert_eq!(encoded.len(), 11 + 10 + count * 266);
        assert_eq!(encoded[11], 2);
        assert_eq!(&encoded[12..20], &[255; 8]);
        assert_eq!(encoded[20], count as u8);
        assert_eq!(&encoded[21..29], &[255; 8]);
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(decoded, message);
        encoded.fill(0);
        assert_eq!(
            decoded, message,
            "decoded rows own their bytes independently"
        );
    }
    let collecting = RoomMessage::Snapshot {
        members: vec![GroupRoomMember {
            id: ParticipantId(1),
            players: vec![PlayerId(1)],
            prepared: false,
        }],
        phase: GroupRoomPhase::Collecting,
        deadline_ns: Some(0),
    };
    assert_eq!(encode_message(&collecting).unwrap().len(), 35);
    assert_eq!(
        decode_message(&encode_message(&collecting).unwrap()).unwrap(),
        collecting
    );
    let max_deadline = RoomMessage::Snapshot {
        members: members(),
        phase: GroupRoomPhase::Frozen,
        deadline_ns: Some(i64::MAX),
    };
    assert_eq!(
        decode_message(&encode_message(&max_deadline).unwrap()).unwrap(),
        max_deadline
    );
}

#[test]
fn wrong_protocol_version_tag_size_and_whole_frame_extent_are_rejected() {
    let bilateral = crate::multiplayer_protocol::encode_frame(1, b"setup").unwrap();
    assert!(decode_message(&bilateral).is_err());
    for (tag, size) in [
        (1, 9u32),
        (1, 65_798),
        (2, 7),
        (2, 9),
        (3, 23),
        (3, 17_035),
        (4, 1),
        (5, 1),
        (6, 1),
        (0, 0),
        (7, 0),
        (255, 0),
    ] {
        let mut header = SEAL.to_vec();
        header[6] = tag;
        header[7..11].copy_from_slice(&size.to_le_bytes());
        assert!(decode_message(&header).is_err());
        let mut decoder = RoomFrameDecoder::new();
        assert!(decoder.push(&header).is_err());
        assert!(decoder.needed().is_err());
    }
    for (offset, byte) in [(0, b'X'), (3, b'P'), (4, 0), (4, 2), (5, 1)] {
        let mut bad = ADMITTED.to_vec();
        bad[offset] = byte;
        assert!(decode_message(&bad).is_err());
    }
    for literal in [JOIN, ADMITTED, SNAPSHOT, SEAL, READY, LEAVE] {
        for end in [0, 1, 10, literal.len() - 1] {
            assert!(decode_message(&literal[..end]).is_err());
        }
        let mut trailing = literal.to_vec();
        trailing.push(0);
        assert!(decode_message(&trailing).is_err());
    }
    let mut coalesced = JOIN.to_vec();
    coalesced.extend_from_slice(SEAL);
    assert!(decode_message(&coalesced).is_err());
}

#[test]
fn invalid_join_and_snapshot_semantics_never_admit_a_valid_prefix_of_members() {
    for players in [
        vec![],
        vec![PlayerId(0)],
        vec![PlayerId(7), PlayerId(7)],
        (1..=65).map(PlayerId).collect(),
    ] {
        assert!(
            encode_message(&RoomMessage::Join {
                identity: vec![1],
                players
            })
            .is_err()
        );
    }
    for identity in [vec![], vec![1; 65_537]] {
        assert!(
            encode_message(&RoomMessage::Join {
                identity,
                players: vec![PlayerId(1)]
            })
            .is_err()
        );
    }
    assert!(
        encode_message(&RoomMessage::Admitted {
            participant: ParticipantId(0)
        })
        .is_err()
    );
    for case in 0..12 {
        let mut hosts = members();
        let mut phase = GroupRoomPhase::Frozen;
        let mut deadline_ns = Some(0);
        match case {
            0 => hosts.clear(),
            1 => {
                hosts = (1..=65)
                    .map(|id| GroupRoomMember {
                        id: ParticipantId(id),
                        players: vec![PlayerId(1)],
                        prepared: false,
                    })
                    .collect()
            }
            2 => hosts[1].id = ParticipantId(0),
            3 => hosts[1].id = hosts[0].id,
            4 => hosts[1].players.clear(),
            5 => hosts[1].players = vec![PlayerId(0)],
            6 => hosts[1].players = vec![PlayerId(7), PlayerId(7)],
            7 => hosts[1].players = (1..=65).map(PlayerId).collect(),
            8 => deadline_ns = Some(-1),
            9 => deadline_ns = None,
            10 => hosts.pop().map(|_| ()).unwrap(),
            _ => phase = GroupRoomPhase::Collecting,
        }
        assert!(
            encode_message(&RoomMessage::Snapshot {
                members: hosts,
                phase,
                deadline_ns
            })
            .is_err()
        );
    }
    let mut all_prepared = members();
    all_prepared[1].prepared = true;
    assert!(
        encode_message(&RoomMessage::Snapshot {
            members: all_prepared.clone(),
            phase: GroupRoomPhase::Frozen,
            deadline_ns: Some(0)
        })
        .is_err()
    );
    assert!(
        encode_message(&RoomMessage::Snapshot {
            members: all_prepared,
            phase: GroupRoomPhase::Prepared,
            deadline_ns: Some(0)
        })
        .is_err()
    );
    assert!(
        encode_message(&RoomMessage::Snapshot {
            members: members(),
            phase: GroupRoomPhase::Prepared,
            deadline_ns: None
        })
        .is_err()
    );
    for (offset, replacement) in [(18, 0), (18, 65)] {
        let mut bad = JOIN.to_vec();
        bad[offset] = replacement;
        assert!(decode_message(&bad).is_err());
    }
    for identity_length in [0u32, 65_537, u32::MAX] {
        let mut bad = JOIN.to_vec();
        bad[11..15].copy_from_slice(&identity_length.to_le_bytes());
        assert!(decode_message(&bad).is_err());
    }
    let mut trailing_body = JOIN.to_vec();
    trailing_body.push(0);
    trailing_body[7..11].copy_from_slice(&17u32.to_le_bytes());
    assert!(decode_message(&trailing_body).is_err());
    for replacement in [0u32, u32::MAX] {
        let mut bad = JOIN.to_vec();
        bad[23..27].copy_from_slice(&replacement.to_le_bytes());
        assert!(decode_message(&bad).is_err());
    }
    for (offset, replacement) in [
        (11, 3),
        (11, 0),
        (11, 2),
        (20, 0),
        (20, 65),
        (29, 2),
        (30, 0),
        (30, 65),
        (43, 1),
    ] {
        let mut bad = SNAPSHOT.to_vec();
        bad[offset] = replacement;
        assert!(decode_message(&bad).is_err());
    }
    let mut negative_deadline = SNAPSHOT.to_vec();
    negative_deadline[12..20].copy_from_slice(&(-2i64).to_le_bytes());
    assert!(decode_message(&negative_deadline).is_err());
    let mut duplicate_host = SNAPSHOT.to_vec();
    duplicate_host[35..43].copy_from_slice(&SNAPSHOT[21..29]);
    assert!(decode_message(&duplicate_host).is_err());
    for player in [0u32, u32::MAX] {
        let mut invalid_later_player = SNAPSHOT.to_vec();
        invalid_later_player[49..53].copy_from_slice(&player.to_le_bytes());
        assert!(decode_message(&invalid_later_player).is_err());
    }
    let mut zero_admission = ADMITTED.to_vec();
    zero_admission[11..19].fill(0);
    assert!(decode_message(&zero_admission).is_err());
}

#[test]
fn every_fragment_boundary_and_coalesced_suffix_preserve_one_owned_frame_and_allow_reuse() {
    for (literal, message) in [
        (JOIN, join()),
        (
            ADMITTED,
            RoomMessage::Admitted {
                participant: ParticipantId(0x8877_6655_4433_2211),
            },
        ),
        (SNAPSHOT, snapshot()),
        (READY, RoomMessage::Ready),
    ] {
        for split in 0..=literal.len() {
            let mut decoder = RoomFrameDecoder::new();
            assert_eq!(decoder.needed().unwrap(), 11);
            assert_eq!(decoder.push(&[]).unwrap(), 0);
            assert_eq!(feed_fragment(&mut decoder, &literal[..split]), split);
            if split != literal.len() {
                assert!(decoder.take().unwrap().is_none());
            }
            assert_eq!(
                feed_fragment(&mut decoder, &literal[split..]),
                literal.len() - split
            );
            assert_eq!(decoder.needed().unwrap(), 0);
            assert_eq!(decoder.push(SEAL).unwrap(), 0);
            assert_eq!(decoder.take().unwrap(), Some(message.clone()));
            assert_eq!(decoder.needed().unwrap(), 11);
            assert!(decoder.take().unwrap().is_none());
            assert_eq!(feed_fragment(&mut decoder, LEAVE), LEAVE.len());
            assert_eq!(decoder.take().unwrap(), Some(RoomMessage::Leave));
        }
    }
    let mut stream = JOIN.to_vec();
    stream.extend_from_slice(SNAPSHOT);
    stream.extend_from_slice(SEAL);
    let mut decoder = RoomFrameDecoder::new();
    assert_eq!(decoder.push(&stream).unwrap(), 11);
    assert_eq!(decoder.push(&stream[11..]).unwrap(), JOIN.len() - 11);
    assert_eq!(decoder.push(&stream[JOIN.len()..]).unwrap(), 0);
    stream[..JOIN.len()].fill(0);
    assert_eq!(decoder.take().unwrap(), Some(join()));
    let remaining = &stream[JOIN.len()..];
    assert_eq!(decoder.push(remaining).unwrap(), 11);
    assert_eq!(decoder.push(&remaining[11..]).unwrap(), SNAPSHOT.len() - 11);
    assert_eq!(decoder.take().unwrap(), Some(snapshot()));
    assert_eq!(
        feed_fragment(&mut decoder, &remaining[SNAPSHOT.len()..]),
        SEAL.len()
    );
    assert_eq!(decoder.take().unwrap(), Some(RoomMessage::Seal));

    let maximum = RoomMessage::Join {
        identity: vec![0xA5; 65_536],
        players: (1..=64).map(PlayerId).collect(),
    };
    let frame = encode_message(&maximum).unwrap();
    let mut decoder = RoomFrameDecoder::new();
    assert_eq!(decoder.push(&frame[..11]).unwrap(), 11);
    assert_eq!(decoder.push(&frame[11..12]).unwrap(), 1);
    let capacity = decoder.bytes.capacity();
    assert!(capacity >= frame.len());
    for byte in frame[12..].chunks(1) {
        assert_eq!(decoder.push(byte).unwrap(), 1);
        assert_eq!(decoder.bytes.capacity(), capacity);
    }
    assert_eq!(decoder.take().unwrap(), Some(maximum));
}

#[test]
fn malformed_headers_stop_before_body_admission_and_semantic_failures_retain_the_complete_frame() {
    for case in 0..4 {
        let mut header = ADMITTED[..11].to_vec();
        match case {
            0 => header[3] = b'P',
            1 => header[7..11].copy_from_slice(&u32::MAX.to_le_bytes()),
            2 => header[6] = 255,
            _ => header[6] = 4,
        }
        let mut chunk = header.clone();
        chunk.extend_from_slice(&[0xA5; 128]);
        chunk.extend_from_slice(READY);
        let mut decoder = RoomFrameDecoder::new();
        assert!(decoder.push(&chunk).is_err());
        assert_eq!(decoder.bytes, header);
        assert!(decoder.needed().is_err());
        assert!(decoder.take().is_err());
        assert!(decoder.push(READY).is_err());
        assert!(decoder.push(&[]).is_err());
        assert_eq!(
            decoder.bytes, header,
            "bad header must not admit or replace body bytes"
        );
    }

    let mut zero_admission = ADMITTED.to_vec();
    zero_admission[11..19].fill(0);
    let mut bad_boolean = SNAPSHOT.to_vec();
    bad_boolean[43] = 2;
    let mut empty_identity = JOIN.to_vec();
    empty_identity[11..15].fill(0);
    for malformed in [zero_admission, bad_boolean, empty_identity] {
        let mut decoder = RoomFrameDecoder::new();
        assert_eq!(feed_fragment(&mut decoder, &malformed), malformed.len());
        assert_eq!(decoder.needed().unwrap(), 0);
        assert_eq!(decoder.take(), Err(RoomWireError::InvalidMessage));
        assert_eq!(decoder.bytes, malformed);
        assert_eq!(decoder.push(READY).unwrap(), 0);
        assert_eq!(decoder.take(), Err(RoomWireError::InvalidMessage));
        assert_eq!(
            decoder.bytes, malformed,
            "semantic refusal must not silently reset the stream"
        );
    }
}
