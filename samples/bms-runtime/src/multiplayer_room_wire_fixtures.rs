//! Deferred BKMR v2 codec fixtures; actual start agreements remain the timing authority.
use super::*;
use crate::multiplayer_clock::{ClockFilter, ClockSample};
use crate::multiplayer_group::{GroupPrefix, MemberProgress};
use crate::multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry};
use crate::multiplayer_protocol::Progress;
use crate::multiplayer_room_client::RoomClientSession;
use crate::multiplayer_room_play::RoomPlayClient;
use crate::multiplayer_start::{StartAgreement, StartMessage, StartPolicy, StartRole};

const JOIN: &[u8] = &[
    b'B', b'K', b'M', b'R', 2, 0, 1, 16, 0, 0, 0, 3, 0, 0, 0, 0, 255, 17, 2, 255, 255, 255, 255, 4,
    3, 2, 1,
];
const ADMITTED: &[u8] = &[
    b'B', b'K', b'M', b'R', 2, 0, 2, 8, 0, 0, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
];
const SNAPSHOT: &[u8] = &[
    b'B', b'K', b'M', b'R', 2, 0, 3, 42, 0, 0, 0, 1, 8, 7, 6, 5, 4, 3, 2, 1, 2, 0x11, 0x22, 0x33,
    0x44, 0x55, 0x66, 0x77, 0x88, 1, 1, 255, 255, 255, 255, 1, 0, 0, 0, 0, 0, 0, 0, 0, 2, 255, 255,
    255, 255, 7, 0, 0, 0,
];
const SEAL: &[u8] = &[b'B', b'K', b'M', b'R', 2, 0, 4, 0, 0, 0, 0];
const READY: &[u8] = &[b'B', b'K', b'M', b'R', 2, 0, 5, 0, 0, 0, 0];
const LEAVE: &[u8] = &[b'B', b'K', b'M', b'R', 2, 0, 6, 0, 0, 0, 0];
const PING: &[u8] = &[
    b'B', b'K', b'M', b'R', 2, 0, 7, 16, 0, 0, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    8, 7, 6, 5, 4, 3, 2, 1,
];
const PONG: &[u8] = &[
    b'B', b'K', b'M', b'R', 2, 0, 8, 32, 0, 0, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    8, 7, 6, 5, 4, 3, 2, 1, 3, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0,
];

// Independent schema-1 golden: one full-width player, sequence and counters.
const PROGRESS: &[u8] = &[
    b'B', b'K', b'M', b'R', 2, 0, 13, 56, 0, 0, 0, 1, 0, 1, 0, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 127, 255, 255, 255, 255, 255, 255,
    255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1, 255, 255, 255, 255, 255, 255, 255,
    255,
];

fn progress_prefix() -> GroupPrefix {
    GroupPrefix {
        sequence: u64::MAX,
        final_prefix: false,
        members: vec![MemberProgress {
            player: PlayerId(u32::MAX),
            progress: Progress {
                song_ns: i64::MAX,
                hits: u64::MAX,
                misses: 0,
                combo: 0x0102_0304_0506_0708,
                max_combo: u64::MAX,
            },
        }],
    }
}

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
        &[b'B', b'K', b'M', b'R', 2, 0, 1, 5, 1, 1, 0]
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
        (13, 0),
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
    for (offset, byte) in [(0, b'X'), (3, b'P'), (4, 0), (4, 1), (4, 3), (5, 1)] {
        let mut bad = ADMITTED.to_vec();
        bad[offset] = byte;
        assert!(decode_message(&bad).is_err());
    }
    for literal in [JOIN, ADMITTED, SNAPSHOT, SEAL, READY, LEAVE] {
        let mut old_version = literal.to_vec();
        old_version[4] = 1;
        assert!(decode_message(&old_version).is_err());
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

#[test]
fn literal_v2_controls_preserve_full_width_sequences_and_independent_clock_domains() {
    let ping = RoomMessage::ClockPing {
        sequence: 0x8877_6655_4433_2211,
        sent_ns: 0x0102_0304_0506_0708,
    };
    let pong = RoomMessage::ClockPong {
        sequence: 0x8877_6655_4433_2211,
        sent_ns: 0x0102_0304_0506_0708,
        received_ns: 3,
        replied_ns: 9,
    };
    for (message, literal) in [(ping, PING), (pong, PONG)] {
        assert_eq!(encode_message(&message).unwrap(), literal);
        assert_eq!(decode_message(literal).unwrap(), message);
    }
    let value = 0x0102_0304_0506_0708;
    for (message, tag) in [
        (StartMessage::ClockReady(value), 9),
        (StartMessage::Propose(value), 10),
        (StartMessage::Accept(value), 11),
        (StartMessage::Commit(value), 12),
    ] {
        let literal = [
            b'B', b'K', b'M', b'R', 2, 0, tag, 8, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1,
        ];
        assert_eq!(
            encode_message(&RoomMessage::Start(message)).unwrap(),
            literal
        );
        assert_eq!(
            decode_message(&literal).unwrap(),
            RoomMessage::Start(message)
        );
    }
    for sequence in [1u64, 8, 9, u64::MAX] {
        for sent_ns in [0, 72_000_000_000_000, 604_800_000_000_000, i64::MAX] {
            for message in [
                RoomMessage::ClockPing { sequence, sent_ns },
                RoomMessage::ClockPong {
                    sequence,
                    sent_ns,
                    received_ns: 0,
                    replied_ns: 0,
                },
                RoomMessage::ClockPong {
                    sequence,
                    sent_ns,
                    received_ns: i64::MAX,
                    replied_ns: i64::MAX,
                },
            ] {
                assert_eq!(
                    decode_message(&encode_message(&message).unwrap()).unwrap(),
                    message
                );
            }
        }
    }
    for (sent_ns, received_ns, replied_ns) in [(1000, 3, 9), (3, 1000, 1006)] {
        let message = RoomMessage::ClockPong {
            sequence: 1,
            sent_ns,
            received_ns,
            replied_ns,
        };
        assert_eq!(
            decode_message(&encode_message(&message).unwrap()).unwrap(),
            message,
            "local send and remote receive have independent origins"
        );
    }
    for value in [0, 72_000_000_000_000, 604_800_000_000_000, i64::MAX] {
        for start in [
            StartMessage::ClockReady(value),
            StartMessage::Propose(value),
            StartMessage::Accept(value),
            StartMessage::Commit(value),
        ] {
            let message = RoomMessage::Start(start);
            assert_eq!(
                decode_message(&encode_message(&message).unwrap()).unwrap(),
                message
            );
        }
    }
}

#[test]
fn invalid_control_extents_and_values_refuse_before_allocation_or_retain_the_owned_malformed_frame()
{
    let mut invalid = vec![
        RoomMessage::ClockPing {
            sequence: 0,
            sent_ns: 0,
        },
        RoomMessage::ClockPing {
            sequence: 1,
            sent_ns: -1,
        },
        RoomMessage::ClockPong {
            sequence: 0,
            sent_ns: 0,
            received_ns: 0,
            replied_ns: 0,
        },
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: -1,
            received_ns: 0,
            replied_ns: 0,
        },
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 0,
            received_ns: -1,
            replied_ns: 0,
        },
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 0,
            received_ns: 0,
            replied_ns: -1,
        },
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 100,
            received_ns: 2,
            replied_ns: 1,
        },
    ];
    for value in [-1, i64::MIN] {
        invalid.extend(
            [
                StartMessage::ClockReady(value),
                StartMessage::Propose(value),
                StartMessage::Accept(value),
                StartMessage::Commit(value),
            ]
            .map(RoomMessage::Start),
        );
    }
    for message in invalid {
        assert!(encode_message(&message).is_err());
    }

    for (tag, size) in [(7u8, 16u32), (8, 32), (9, 8), (10, 8), (11, 8), (12, 8)] {
        for wrong in [0, size - 1, size + 1, 65_798, u32::MAX] {
            let mut header = SEAL.to_vec();
            header[6] = tag;
            header[7..11].copy_from_slice(&wrong.to_le_bytes());
            let mut chunk = header.clone();
            chunk.extend_from_slice(&[0xAA; 64]);
            let mut decoder = RoomFrameDecoder::new();
            assert!(decoder.push(&chunk).is_err());
            assert_eq!(decoder.bytes, header);
            assert!(decoder.needed().is_err());
            assert!(decoder.take().is_err());
            assert!(decoder.push(PING).is_err());
            assert_eq!(decoder.bytes, header);
        }
    }
    let mut malformed = Vec::new();
    for literal in [PING, PONG] {
        let mut zero_sequence = literal.to_vec();
        zero_sequence[11..19].fill(0);
        malformed.push(zero_sequence);
        let mut negative_sent = literal.to_vec();
        negative_sent[19..27].copy_from_slice(&(-1i64).to_le_bytes());
        malformed.push(negative_sent);
    }
    for range in [27..35, 35..43] {
        let mut negative = PONG.to_vec();
        negative[range].copy_from_slice(&(-1i64).to_le_bytes());
        malformed.push(negative);
    }
    let mut backwards = PONG.to_vec();
    backwards[27..35].copy_from_slice(&10i64.to_le_bytes());
    malformed.push(backwards);
    for tag in 9..=12 {
        let mut frame = vec![b'B', b'K', b'M', b'R', 2, 0, tag, 8, 0, 0, 0];
        frame.extend_from_slice(&(-1i64).to_le_bytes());
        malformed.push(frame);
    }
    for frame in malformed {
        assert!(decode_message(&frame).is_err());
        let mut decoder = RoomFrameDecoder::new();
        feed_fragment(&mut decoder, &frame);
        assert_eq!(decoder.needed().unwrap(), 0);
        assert_eq!(decoder.take(), Err(RoomWireError::InvalidMessage));
        assert_eq!(decoder.bytes, frame);
        assert_eq!(decoder.push(READY).unwrap(), 0);
        assert_eq!(decoder.take(), Err(RoomWireError::InvalidMessage));
        assert_eq!(decoder.bytes, frame);
    }
    let controls = [
        PING.to_vec(),
        PONG.to_vec(),
        encode_message(&RoomMessage::Start(StartMessage::Commit(0))).unwrap(),
    ];
    for frame in controls {
        let mut old = frame.clone();
        old[4] = 1;
        assert!(decode_message(&old).is_err());
        let mut bilateral = frame.clone();
        bilateral[3] = b'P';
        assert!(decode_message(&bilateral).is_err());
        let mut trailing = frame.clone();
        trailing.push(0);
        assert!(decode_message(&trailing).is_err());
        for end in [0, 10, frame.len() - 1] {
            assert!(decode_message(&frame[..end]).is_err());
        }
    }
}

fn start_through_wire(message: StartMessage) -> StartMessage {
    let frame = encode_message(&RoomMessage::Start(message)).unwrap();
    let mut decoder = RoomFrameDecoder::new();
    for byte in frame.chunks(1) {
        assert_eq!(decoder.push(byte).unwrap(), 1);
    }
    let Some(RoomMessage::Start(decoded)) = decoder.take().unwrap() else {
        panic!("actual StartMessage lost its variant");
    };
    assert_eq!(decoded, message);
    decoded
}

#[test]
fn fragmented_and_coalesced_controls_preserve_actual_start_agreement_receipts_without_granting_timing_authority()
 {
    let controls = [
        RoomMessage::ClockPing {
            sequence: u64::MAX,
            sent_ns: i64::MAX,
        },
        RoomMessage::ClockPong {
            sequence: u64::MAX,
            sent_ns: i64::MAX,
            received_ns: 0,
            replied_ns: 1,
        },
        RoomMessage::Start(StartMessage::ClockReady(0)),
        RoomMessage::Start(StartMessage::Propose(i64::MAX)),
        RoomMessage::Start(StartMessage::Accept(i64::MAX)),
        RoomMessage::Start(StartMessage::Commit(i64::MAX)),
    ];
    let mut stream = Vec::new();
    for message in &controls {
        let frame = encode_message(message).unwrap();
        stream.extend_from_slice(&frame);
        for split in 0..=frame.len() {
            let mut decoder = RoomFrameDecoder::new();
            feed_fragment(&mut decoder, &frame[..split]);
            assert_eq!(
                decoder.needed().unwrap(),
                if split < 11 {
                    11 - split
                } else {
                    frame.len() - split
                }
            );
            if split != frame.len() {
                assert_eq!(decoder.take().unwrap(), None);
            }
            feed_fragment(&mut decoder, &frame[split..]);
            assert_eq!(decoder.push(PING).unwrap(), 0);
            assert_eq!(decoder.take().unwrap(), Some(message.clone()));
            assert_eq!(decoder.needed().unwrap(), 11);
        }
    }
    stream.extend_from_slice(LEAVE);
    let mut decoder = RoomFrameDecoder::new();
    let mut offset = 0;
    for expected in controls.into_iter().chain([RoomMessage::Leave]) {
        while decoder.needed().unwrap() > 0 {
            let count = decoder.push(&stream[offset..]).unwrap();
            assert!(count > 0);
            offset += count;
        }
        assert_eq!(decoder.push(&stream[offset..]).unwrap(), 0);
        assert_eq!(decoder.take().unwrap(), Some(expected));
    }
    assert_eq!(offset, stream.len());

    let policy = StartPolicy {
        lead_ns: 1000,
        min_remaining_ns: 100,
        max_age_ns: 10_000,
        max_uncertainty_ns: 100,
        max_release_lateness_ns: 25,
    };
    let mut host_clock = ClockFilter::new();
    host_clock
        .observe(ClockSample::new(100, 160, 180, 140).unwrap())
        .unwrap();
    let mut join_clock = ClockFilter::new();
    join_clock
        .observe(ClockSample::new(100, 60, 80, 140).unwrap())
        .unwrap();
    let mut host = StartAgreement::new_at(StartRole::Host, policy, 100).unwrap();
    let mut join = StartAgreement::new_at(StartRole::Join, policy, 300).unwrap();
    host.prepare(host_clock.estimate().unwrap()).unwrap();
    join.prepare(join_clock.estimate().unwrap()).unwrap();
    let host_ready = host.next(1000).unwrap().unwrap();
    let join_ready = join.next(1050).unwrap().unwrap();
    host.written(host_ready, 1000).unwrap();
    join.receive(start_through_wire(host_ready), 1050).unwrap();
    join.written(join_ready, 1050).unwrap();
    host.receive(start_through_wire(join_ready), 1000).unwrap();
    let before = join;
    assert!(
        join.receive(start_through_wire(StartMessage::Propose(0)), 1051)
            .is_err()
    );
    assert_eq!(
        join, before,
        "syntactically valid zero does not authorize an elapsed start target"
    );
    let proposal = host.next(1001).unwrap().unwrap();
    assert_eq!(proposal, StartMessage::Propose(2301));
    host.written(proposal, 1002).unwrap();
    join.receive(start_through_wire(proposal), 1052).unwrap();
    let accept = join.next(1053).unwrap().unwrap();
    join.written(accept, 1053).unwrap();
    let before = host;
    assert!(
        host.receive(start_through_wire(StartMessage::Accept(2302)), 1003)
            .is_err()
    );
    assert_eq!(
        host, before,
        "codec acceptance cannot grant a mismatched proposal echo"
    );
    host.receive(start_through_wire(accept), 1003).unwrap();
    let commit = host.next(1004).unwrap().unwrap();
    host.written(commit, 1005).unwrap();
    assert!(host.committed());
    assert!(!join.committed());
    join.receive(start_through_wire(commit), 1055).unwrap();
    assert_eq!(host.take_schedule().unwrap().song_target_ns, 2301);
    let schedule = join.take_schedule().unwrap();
    assert_eq!(
        (
            schedule.song_target_ns,
            schedule.target_ns,
            schedule.uncertainty_ns
        ),
        (2351, 2051, 20)
    );
}

#[test]
fn literal_progress_upload_peer_and_final_ack_preserve_full_width_original_fields() {
    let ordinary = RoomMessage::Progress(progress_prefix());
    assert_eq!(encode_message(&ordinary).unwrap(), PROGRESS);
    assert_eq!(decode_message(PROGRESS).unwrap(), ordinary);
    let mut final_prefix = progress_prefix();
    final_prefix.final_prefix = true;
    let mut final_literal = PROGRESS.to_vec();
    final_literal[12] = 1;
    let final_upload = RoomMessage::Progress(final_prefix.clone());
    assert_eq!(encode_message(&final_upload).unwrap(), final_literal);
    assert_eq!(decode_message(&final_literal).unwrap(), final_upload);

    let peer = RoomMessage::PeerProgress {
        participant: ParticipantId(0x8877_6655_4433_2211),
        prefix: final_prefix,
    };
    let mut peer_literal = vec![
        b'B', b'K', b'M', b'R', 2, 0, 14, 64, 0, 0, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
        0x88,
    ];
    peer_literal.extend_from_slice(&final_literal[11..]);
    assert_eq!(encode_message(&peer).unwrap(), peer_literal);
    let decoded = decode_message(&peer_literal).unwrap();
    peer_literal.fill(0);
    assert_eq!(decoded, peer, "the decoded prefix owns its member storage");
    let ack = RoomMessage::FinalAck {
        participant: ParticipantId(u64::MAX),
        sequence: u64::MAX,
    };
    let ack_literal = [
        b'B', b'K', b'M', b'R', 2, 0, 15, 16, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255,
    ];
    assert_eq!(encode_message(&ack).unwrap(), ack_literal);
    assert_eq!(decode_message(&ack_literal).unwrap(), ack);

    for song_ns in [i64::MIN, 72_000_000_000_000, 604_800_000_000_000, i64::MAX] {
        let mut prefix = progress_prefix();
        prefix.members[0].progress.song_ns = song_ns;
        let mut expected = PROGRESS.to_vec();
        expected[27..35].copy_from_slice(&song_ns.to_le_bytes());
        let message = RoomMessage::Progress(prefix);
        assert_eq!(encode_message(&message).unwrap(), expected);
        assert_eq!(decode_message(&expected).unwrap(), message);
    }
}

#[test]
fn progress_bounds_and_whole_member_validation_reject_malformed_headers_and_payloads() {
    for (tag, length) in [
        (13, 55u32),
        (13, 57),
        (13, 2829),
        (14, 63),
        (14, 65),
        (14, 2837),
        (15, 15),
        (15, 17),
        (16, 0),
        (13, u32::MAX),
    ] {
        let mut header = vec![b'B', b'K', b'M', b'R', 2, 0, tag];
        header.extend_from_slice(&length.to_le_bytes());
        let mut arriving = header.clone();
        arriving.extend_from_slice(PROGRESS);
        let mut decoder = RoomFrameDecoder::new();
        assert!(decoder.push(&arriving).is_err());
        assert_eq!(
            decoder.bytes, header,
            "no body admitted for an invalid extent"
        );
        assert!(decoder.take().is_err());
    }

    let mut malformed = Vec::new();
    for (offset, value) in [(11, 0), (11, 2), (12, 2), (13, 0), (13, 2), (13, 65)] {
        let mut frame = PROGRESS.to_vec();
        frame[offset] = value;
        malformed.push(frame);
    }
    for (start, end) in [(15, 23), (23, 27), (59, 67)] {
        let mut frame = PROGRESS.to_vec();
        frame[start..end].fill(0);
        malformed.push(frame);
    }
    let mut overflow = PROGRESS.to_vec();
    overflow[43] = 1; // hits MAX plus one miss cannot be a cumulative total.
    malformed.push(overflow);
    let mut pair = progress_prefix();
    pair.members.push(MemberProgress {
        player: PlayerId(7),
        ..pair.members[0]
    });
    let valid_pair = encode_message(&RoomMessage::Progress(pair)).unwrap();
    for id in [0u32, u32::MAX] {
        let mut frame = valid_pair.clone();
        frame[67..71].copy_from_slice(&id.to_le_bytes());
        malformed.push(frame);
    }
    let valid_peer = encode_message(&RoomMessage::PeerProgress {
        participant: ParticipantId(u64::MAX),
        prefix: progress_prefix(),
    })
    .unwrap();
    for range in [11..19, 23..31] {
        let mut frame = valid_peer.clone();
        frame[range].fill(0);
        malformed.push(frame);
    }
    let valid_ack = encode_message(&RoomMessage::FinalAck {
        participant: ParticipantId(1),
        sequence: 1,
    })
    .unwrap();
    for range in [11..19, 19..27] {
        let mut frame = valid_ack.clone();
        frame[range].fill(0);
        malformed.push(frame);
    }
    for frame in malformed {
        assert!(decode_message(&frame).is_err());
    }
    for frame in [PROGRESS.to_vec(), valid_peer, valid_ack] {
        let mut trailing = frame.clone();
        trailing.push(0);
        assert!(decode_message(&trailing).is_err());
        assert!(decode_message(&frame[..frame.len() - 1]).is_err());
        let mut old = frame.clone();
        old[4] = 1;
        assert!(decode_message(&old).is_err());
        let mut bilateral = frame;
        bilateral[3] = b'P';
        assert!(decode_message(&bilateral).is_err());
    }

    let mut maximum = progress_prefix();
    maximum.members = (0..64)
        .map(|index| MemberProgress {
            player: PlayerId(u32::MAX - index),
            ..maximum.members[0]
        })
        .collect();
    for (message, expected_length) in [
        (RoomMessage::Progress(maximum.clone()), 2839),
        (
            RoomMessage::PeerProgress {
                participant: ParticipantId(u64::MAX),
                prefix: maximum.clone(),
            },
            2847,
        ),
    ] {
        let bytes = encode_message(&message).unwrap();
        assert_eq!(bytes.len(), expected_length);
        assert_eq!(decode_message(&bytes).unwrap(), message);
    }
    let mut invalid = vec![maximum.clone()];
    invalid[0].members.push(MemberProgress {
        player: PlayerId(1),
        ..maximum.members[0]
    });
    let mut empty = maximum.clone();
    empty.members.clear();
    invalid.push(empty);
    let mut zero_sequence = maximum.clone();
    zero_sequence.sequence = 0;
    let legacy = crate::multiplayer_group::encode_prefix(0, false, &zero_sequence.members).unwrap();
    assert_eq!(
        crate::multiplayer_group::decode_prefix(&legacy, 0, None).unwrap(),
        zero_sequence,
        "the existing group codec keeps its zero-based sequence contract"
    );
    invalid.push(zero_sequence);
    let mut duplicate = maximum.clone();
    duplicate.members[63].player = duplicate.members[0].player;
    invalid.push(duplicate);
    let mut bad_later = maximum;
    bad_later.members[63].progress.misses = 1;
    invalid.push(bad_later);
    for prefix in invalid {
        assert!(encode_message(&RoomMessage::Progress(prefix.clone())).is_err());
        assert!(
            encode_message(&RoomMessage::PeerProgress {
                participant: ParticipantId(1),
                prefix,
            })
            .is_err()
        );
    }
    for message in [
        RoomMessage::PeerProgress {
            participant: ParticipantId(0),
            prefix: progress_prefix(),
        },
        RoomMessage::FinalAck {
            participant: ParticipantId(0),
            sequence: 1,
        },
        RoomMessage::FinalAck {
            participant: ParticipantId(1),
            sequence: 0,
        },
    ] {
        assert!(encode_message(&message).is_err());
    }
}

#[test]
fn progress_fragments_coalescing_and_semantic_failure_retain_exact_owned_frames() {
    let mut final_prefix = progress_prefix();
    final_prefix.final_prefix = true;
    let messages = [
        RoomMessage::Progress(progress_prefix()),
        RoomMessage::Progress(final_prefix.clone()),
        RoomMessage::PeerProgress {
            participant: ParticipantId(u64::MAX),
            prefix: final_prefix,
        },
        RoomMessage::FinalAck {
            participant: ParticipantId(u64::MAX),
            sequence: u64::MAX,
        },
    ];
    let mut stream = Vec::new();
    for message in &messages {
        let frame = encode_message(message).unwrap();
        stream.extend_from_slice(&frame);
        for split in 0..=frame.len() {
            let mut decoder = RoomFrameDecoder::new();
            feed_fragment(&mut decoder, &frame[..split]);
            assert_eq!(
                decoder.needed().unwrap(),
                if split < 11 {
                    11 - split
                } else {
                    frame.len() - split
                }
            );
            if split < frame.len() {
                assert_eq!(decoder.take().unwrap(), None);
            }
            feed_fragment(&mut decoder, &frame[split..]);
            assert_eq!(decoder.push(LEAVE).unwrap(), 0);
            assert_eq!(decoder.take().unwrap(), Some(message.clone()));
            assert_eq!(decoder.needed().unwrap(), 11);
        }
    }
    let mut decoder = RoomFrameDecoder::new();
    let mut offset = 0;
    for message in messages {
        while decoder.needed().unwrap() > 0 {
            offset += decoder.push(&stream[offset..]).unwrap();
        }
        assert_eq!(decoder.take().unwrap(), Some(message));
    }
    assert_eq!(offset, stream.len());
    let mut malformed = PROGRESS.to_vec();
    malformed[12] = 2;
    for byte in malformed.chunks(1) {
        assert_eq!(decoder.push(byte).unwrap(), 1);
    }
    assert_eq!(decoder.take(), Err(RoomWireError::InvalidMessage));
    assert_eq!(decoder.bytes, malformed);
    assert_eq!(decoder.needed().unwrap(), 0);
    assert_eq!(decoder.push(LEAVE).unwrap(), 0);
    assert_eq!(decoder.take(), Err(RoomWireError::InvalidMessage));
    assert_eq!(decoder.bytes, malformed);
}

#[test]
fn syntactic_progress_and_ack_do_not_authorize_existing_prepared_client_owners() {
    let identity = b"progress-wire-scope";
    let players = [PlayerId(u32::MAX)];
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 2, 8, 1000).unwrap());
    let creator = registry.join("room", identity, &[PlayerId(7)], 0).unwrap();
    let own = registry.join("room", identity, &players, 0).unwrap();
    let snapshot = |registry: &GroupRoomRegistry| {
        let room = registry.room("room").unwrap();
        RoomMessage::Snapshot {
            members: room.members.to_vec(),
            phase: room.phase,
            deadline_ns: room.deadline_ns,
        }
    };
    let policy = StartPolicy {
        lead_ns: 1000,
        min_remaining_ns: 100,
        max_age_ns: 10_000,
        max_uncertainty_ns: 100,
        max_release_lateness_ns: 25,
    };
    let mut admission = RoomClientSession::new(identity, &players).unwrap();
    let mut play = RoomPlayClient::new(identity, &players, policy, 0).unwrap();
    let admission_join = admission.poll_write().unwrap().unwrap();
    let play_join = play.poll_write(0).unwrap().unwrap();
    let expected_join = RoomMessage::Join {
        identity: identity.to_vec(),
        players: players.to_vec(),
    };
    assert_eq!(
        decode_message(&admission_join.bytes).unwrap(),
        expected_join
    );
    assert_eq!(decode_message(&play_join.bytes).unwrap(), expected_join);
    admission.written(admission_join.id).unwrap();
    play.written(play_join.id, 0).unwrap();
    for message in [
        RoomMessage::Admitted {
            participant: own.id,
        },
        snapshot(&registry),
    ] {
        let frame = encode_message(&message).unwrap();
        admission.receive(decode_message(&frame).unwrap()).unwrap();
        play.receive(decode_message(&frame).unwrap(), 0).unwrap();
    }
    registry.seal(creator.id, 1).unwrap();
    admission.receive(snapshot(&registry)).unwrap();
    play.receive(snapshot(&registry), 1).unwrap();
    admission.request_ready().unwrap();
    play.request_ready().unwrap();
    let admission_ready = admission.poll_write().unwrap().unwrap();
    let play_ready = play.poll_write(2).unwrap().unwrap();
    assert_eq!(admission_ready.bytes, READY);
    assert_eq!(play_ready.bytes, READY);
    admission.written(admission_ready.id).unwrap();
    play.written(play_ready.id, 2).unwrap();
    registry.ready(creator.id, 2).unwrap();
    registry.ready(own.id, 2).unwrap();
    admission.receive(snapshot(&registry)).unwrap();
    play.receive(snapshot(&registry), 2).unwrap();
    assert_eq!(admission.room().unwrap().phase, GroupRoomPhase::Prepared);
    assert_eq!(play.room().unwrap().phase, GroupRoomPhase::Prepared);

    let mut final_prefix = progress_prefix();
    final_prefix.final_prefix = true;
    for message in [
        RoomMessage::Progress(progress_prefix()),
        RoomMessage::Progress(final_prefix.clone()),
        RoomMessage::PeerProgress {
            participant: own.id,
            prefix: final_prefix,
        },
        RoomMessage::FinalAck {
            participant: own.id,
            sequence: u64::MAX,
        },
    ] {
        let bytes = encode_message(&message).unwrap();
        assert_eq!(decode_message(&bytes).unwrap(), message);
        let prior_admission = admission.clone();
        let prior_play = play.clone();
        assert!(admission.receive(decode_message(&bytes).unwrap()).is_err());
        assert!(play.receive(decode_message(&bytes).unwrap(), 3).is_err());
        assert_eq!(admission, prior_admission);
        assert_eq!(play, prior_play);
        assert_eq!(play.take_schedule(), None);
        assert!(!admission.leave_written());
        assert!(!play.leave_written());
    }
}
