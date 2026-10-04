//! Deferred clock-exchange fixtures. Software frames and captured timestamps
//! do not constitute transport, output-start, or hardware-clock acceptance.
use crate::{
    local_players::PlayerId,
    multiplayer_clock::{ClockFilter, ClockSample},
    multiplayer_group_rooms::{
        GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry, GroupRoomSnapshot,
    },
    multiplayer_protocol::{ClockProbes, FrameDecoder, MultiplayerEvent, OutboundFrame},
    multiplayer_room_clock::RoomClockExchange,
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::StartMessage,
};

fn prepared_registry(count: usize) -> GroupRoomRegistry {
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, count, 8, 100).unwrap());
    let players = (0..64)
        .map(|index| PlayerId(u32::MAX - index))
        .collect::<Vec<_>>();
    let ids = (0..count)
        .map(|_| {
            registry
                .join("room", b"clock\0identity", &players, 0)
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

fn pair() -> (RoomClockExchange, RoomClockExchange) {
    let registry = prepared_registry(2);
    let snapshot = registry.room("room").unwrap();
    // Both ends describe the same actual stream lease, not two chosen wire IDs.
    let participant = snapshot.members[1].id;
    (
        RoomClockExchange::new(snapshot, participant).unwrap(),
        RoomClockExchange::new(snapshot, participant).unwrap(),
    )
}

fn message(frame: &OutboundFrame) -> RoomMessage {
    let decoded = decode_message(&frame.bytes).unwrap();
    assert_eq!(encode_message(&decoded).unwrap(), frame.bytes);
    decoded
}

fn round(
    a: &mut RoomClockExchange,
    b: &mut RoomClockExchange,
    sequence: u64,
    at: i64,
    offset: i64,
) -> [ClockSample; 2] {
    let a_ping = a.next(at).unwrap().unwrap();
    let b_ping = b.next(at + offset).unwrap().unwrap();
    assert_eq!(a_ping.id, sequence * 2 - 1);
    assert_eq!(b_ping.id, sequence * 2 - 1);
    assert_eq!(
        message(&a_ping),
        RoomMessage::ClockPing {
            sequence,
            sent_ns: at
        }
    );
    assert_eq!(
        message(&b_ping),
        RoomMessage::ClockPing {
            sequence,
            sent_ns: at + offset
        }
    );
    a.written(a_ping.id, at + 2).unwrap();
    b.written(b_ping.id, at + offset + 3).unwrap();
    a.receive(&message(&b_ping), at + 7).unwrap();
    b.receive(&message(&a_ping), at + offset + 11).unwrap();
    let a_pong = a.next(at + 13).unwrap().unwrap();
    let b_pong = b.next(at + offset + 17).unwrap().unwrap();
    assert_eq!(a_pong.id, sequence * 2);
    assert_eq!(b_pong.id, sequence * 2);
    assert_eq!(
        message(&a_pong),
        RoomMessage::ClockPong {
            sequence,
            sent_ns: at + offset,
            received_ns: at + 7,
            replied_ns: at + 13,
        }
    );
    assert_eq!(
        message(&b_pong),
        RoomMessage::ClockPong {
            sequence,
            sent_ns: at,
            received_ns: at + offset + 11,
            replied_ns: at + offset + 17,
        }
    );
    a.written(a_pong.id, at + 14).unwrap();
    b.written(b_pong.id, at + offset + 18).unwrap();
    a.receive(&message(&b_pong), at + 23).unwrap();
    b.receive(&message(&a_pong), at + offset + 29).unwrap();
    [
        ClockSample::new(at, at + offset + 11, at + offset + 17, at + 23).unwrap(),
        ClockSample::new(at + offset, at + 7, at + 13, at + offset + 29).unwrap(),
    ]
}

#[test]
fn only_valid_prepared_membership_is_owned_with_full_width_scoped_participants() {
    for count in [2usize, 3, 4, 64] {
        let registry = prepared_registry(count);
        let snapshot = registry.room("room").unwrap();
        for member in snapshot.members {
            let exchange = RoomClockExchange::new(snapshot, member.id).unwrap();
            assert_eq!(exchange.participant(), member.id);
            assert_eq!(exchange.estimate(), None);
        }
        assert!(RoomClockExchange::new(snapshot, ParticipantId(0)).is_err());
        assert!(RoomClockExchange::new(snapshot, ParticipantId(u64::MAX)).is_err());

        let mut members = snapshot.members.to_vec();
        for (index, member) in members.iter_mut().enumerate() {
            member.id = ParticipantId(u64::MAX - index as u64);
        }
        let bound = members[0].id;
        let mut exchange = RoomClockExchange::new(
            GroupRoomSnapshot {
                members: &members,
                ..snapshot
            },
            bound,
        )
        .unwrap();
        members[0].id = ParticipantId(0);
        members[0].players.clear();
        assert_eq!(exchange.participant(), bound);
        assert_eq!(
            message(&exchange.next(0).unwrap().unwrap()),
            RoomMessage::ClockPing {
                sequence: 1,
                sent_ns: 0
            }
        );
    }

    let registry = prepared_registry(2);
    let snapshot = registry.room("room").unwrap();
    let participant = snapshot.members[0].id;
    for phase in [GroupRoomPhase::Collecting, GroupRoomPhase::Frozen] {
        assert!(
            RoomClockExchange::new(GroupRoomSnapshot { phase, ..snapshot }, participant).is_err()
        );
    }
    assert!(
        RoomClockExchange::new(
            GroupRoomSnapshot {
                deadline_ns: Some(100),
                ..snapshot
            },
            participant,
        )
        .is_err()
    );
    for identity in [Vec::new(), vec![1; 65_537]] {
        assert!(
            RoomClockExchange::new(
                GroupRoomSnapshot {
                    identity: &identity,
                    ..snapshot
                },
                participant,
            )
            .is_err()
        );
    }
    let mut invalid_members = vec![Vec::new(), snapshot.members[..1].to_vec()];
    for mutation in 0..7 {
        let mut members = snapshot.members.to_vec();
        match mutation {
            0 => members[1].prepared = false,
            1 => members[1].id = ParticipantId(0),
            2 => members[1].id = members[0].id,
            3 => members[1].players.clear(),
            4 => members[1].players[63] = PlayerId(0),
            5 => members[1].players[63] = members[1].players[0],
            _ => members[1].players.push(PlayerId(1)),
        }
        invalid_members.push(members);
    }
    let mut too_many = snapshot.members.to_vec();
    while too_many.len() < 65 {
        let mut member = snapshot.members[0].clone();
        member.id = ParticipantId(too_many.len() as u64 + 1);
        too_many.push(member);
    }
    invalid_members.push(too_many);
    for members in invalid_members {
        assert!(
            RoomClockExchange::new(
                GroupRoomSnapshot {
                    members: &members,
                    ..snapshot
                },
                participant,
            )
            .is_err()
        );
    }
}

#[test]
fn symmetric_eight_probe_frames_preserve_signed_offsets_queue_delay_and_original_samples() {
    for offset in [-400i64, 400] {
        let (mut a, mut b) = pair();
        let (mut a_filter, mut b_filter) = (ClockFilter::new(), ClockFilter::new());
        for index in 0..8 {
            let at = 1_000 + index * 100;
            let samples = round(&mut a, &mut b, index as u64 + 1, at, offset);
            a_filter.observe(samples[0]).unwrap();
            b_filter.observe(samples[1]).unwrap();
            if index < 7 {
                assert_eq!(a.estimate(), None);
                assert_eq!(b.estimate(), None);
            }
        }
        let a_estimate = a.estimate().unwrap();
        let b_estimate = b.estimate().unwrap();
        assert_eq!(Some(a_estimate), a_filter.estimate());
        assert_eq!(Some(b_estimate), b_filter.estimate());
        assert_eq!(
            (a_estimate.lower_ns(), a_estimate.upper_ns()),
            (i128::from(offset - 6), i128::from(offset + 11))
        );
        assert_eq!(
            (b_estimate.lower_ns(), b_estimate.upper_ns()),
            (i128::from(-offset - 16), i128::from(-offset + 7))
        );
        assert_eq!(a_estimate.round_trip_ns(), 17);
        assert_eq!(b_estimate.round_trip_ns(), 23);
        assert_eq!(a_estimate.observed_local_ns(), 1_723);
        assert_eq!(b_estimate.observed_local_ns(), 1_729 + offset);
        assert!(a.next(1_800).unwrap().is_none());
        assert!(b.next(1_800 + offset).unwrap().is_none());
        for (exchange, now) in [(&mut a, 1_800), (&mut b, 1_800 + offset)] {
            let before = exchange.clone();
            assert!(
                exchange
                    .receive(
                        &RoomMessage::ClockPing {
                            sequence: 9,
                            sent_ns: 1_800
                        },
                        now
                    )
                    .is_err()
            );
            assert_eq!(*exchange, before);
        }
    }
}

#[test]
fn early_reply_and_last_pending_pong_do_not_replace_complete_write_barriers() {
    let (mut a, mut b) = pair();
    for index in 0..7 {
        round(&mut a, &mut b, index + 1, 1_000 + index as i64 * 100, 400);
    }
    let a_ping = a.next(1_700).unwrap().unwrap();
    let b_ping = b.next(2_100).unwrap().unwrap();
    assert_eq!((a_ping.id, b_ping.id), (15, 15));
    b.written(b_ping.id, 2_101).unwrap();
    a.receive(&message(&b_ping), 1_705).unwrap();
    assert!(a.next(1_706).unwrap().is_none());
    b.receive(&message(&a_ping), 2_110).unwrap();
    let b_pong = b.next(2_115).unwrap().unwrap();
    b.written(b_pong.id, 2_116).unwrap();
    a.receive(&message(&b_pong), 1_720).unwrap();
    assert_eq!(a.estimate(), None);
    assert!(a.next(1_721).unwrap().is_none());
    let before = a.clone();
    assert!(a.written(b_pong.id, 1_724).is_err());
    assert_eq!(a, before);
    a.written(a_ping.id, 1_725).unwrap();
    assert_eq!(a.estimate(), None);
    let a_pong = a.next(1_726).unwrap().unwrap();
    assert_eq!(a_pong.id, 16);
    assert_eq!(
        message(&a_pong),
        RoomMessage::ClockPong {
            sequence: 8,
            sent_ns: 2_100,
            received_ns: 1_705,
            replied_ns: 1_726,
        }
    );
    b.receive(&message(&a_pong), 2_140).unwrap();
    assert!(b.estimate().is_some());
    assert_eq!(a.estimate(), None);
    assert!(a.next(1_749).unwrap().is_none());
    a.written(a_pong.id, 1_750).unwrap();
    let estimate = a.estimate().unwrap();
    assert_eq!((estimate.lower_ns(), estimate.upper_ns()), (395, 410));
    assert_eq!(estimate.round_trip_ns(), 15);
    assert_eq!(estimate.observed_local_ns(), 1_720);
    let before = a.clone();
    assert!(a.written(a_pong.id, 1_751).is_err());
    assert_eq!(a, before);

    // A pending reply wins over a fresh local probe, and occupies the same slot.
    let (mut receiver, mut sender) = pair();
    let ping = sender.next(100).unwrap().unwrap();
    sender.written(ping.id, 101).unwrap();
    receiver.receive(&message(&ping), 110).unwrap();
    let reply = receiver.next(111).unwrap().unwrap();
    assert!(matches!(
        message(&reply),
        RoomMessage::ClockPong { sequence: 1, .. }
    ));
    assert!(receiver.next(112).unwrap().is_none());
    receiver.written(reply.id, 113).unwrap();
    let own_ping = receiver.next(114).unwrap().unwrap();
    assert_eq!(own_ping.id, reply.id + 1);
    assert_eq!(
        message(&own_ping),
        RoomMessage::ClockPing {
            sequence: 1,
            sent_ns: 114
        }
    );
}

#[test]
fn rejected_controls_echoes_samples_and_local_times_preserve_the_entire_accepted_state() {
    let (mut a, _) = pair();
    let ping = a.next(100).unwrap().unwrap();
    let bad = [
        RoomMessage::Ready,
        RoomMessage::Leave,
        RoomMessage::Start(StartMessage::ClockReady(0)),
        RoomMessage::ClockPing {
            sequence: 0,
            sent_ns: 100,
        },
        RoomMessage::ClockPing {
            sequence: 2,
            sent_ns: 100,
        },
        RoomMessage::ClockPing {
            sequence: u64::MAX,
            sent_ns: 100,
        },
        RoomMessage::ClockPing {
            sequence: 1,
            sent_ns: -1,
        },
        RoomMessage::ClockPong {
            sequence: 0,
            sent_ns: 100,
            received_ns: 105,
            replied_ns: 110,
        },
        RoomMessage::ClockPong {
            sequence: 2,
            sent_ns: 100,
            received_ns: 105,
            replied_ns: 110,
        },
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 99,
            received_ns: 105,
            replied_ns: 110,
        },
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 100,
            received_ns: -1,
            replied_ns: 110,
        },
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 100,
            received_ns: 111,
            replied_ns: 110,
        },
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 100,
            received_ns: 105,
            replied_ns: 250,
        },
    ];
    for message in bad {
        let before = a.clone();
        assert!(a.receive(&message, 200).is_err());
        assert_eq!(a, before);
    }
    for now in [-1, 99] {
        let before = a.clone();
        assert!(a.next(now).is_err());
        assert_eq!(a, before);
        assert!(a.written(ping.id, now).is_err());
        assert_eq!(a, before);
        assert!(
            a.receive(
                &RoomMessage::ClockPing {
                    sequence: 1,
                    sent_ns: 0
                },
                now
            )
            .is_err()
        );
        assert_eq!(a, before);
    }
    for id in [0, ping.id + 1, u64::MAX] {
        let before = a.clone();
        assert!(a.written(id, 200).is_err());
        assert_eq!(a, before);
    }
    // Failed future observations did not advance the accepted local baseline.
    a.written(ping.id, 100).unwrap();
    let pong = RoomMessage::ClockPong {
        sequence: 1,
        sent_ns: 100,
        received_ns: 105,
        replied_ns: 110,
    };
    a.receive(&pong, 120).unwrap();
    let before = a.clone();
    assert!(a.receive(&pong, 121).is_err());
    assert_eq!(a, before);
    let peer_ping = RoomMessage::ClockPing {
        sequence: 1,
        sent_ns: 5,
    };
    a.receive(&peer_ping, 120).unwrap();
    for message in [
        peer_ping,
        RoomMessage::ClockPing {
            sequence: 2,
            sent_ns: 6,
        },
    ] {
        let before = a.clone();
        assert!(a.receive(&message, 122).is_err());
        assert_eq!(a, before);
    }
    let reply = a.next(120).unwrap().unwrap();
    assert_eq!(
        message(&reply),
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 5,
            received_ns: 120,
            replied_ns: 120,
        }
    );
    a.written(reply.id, 123).unwrap();
    let before = a.clone();
    assert!(
        a.receive(
            &RoomMessage::ClockPing {
                sequence: 2,
                sent_ns: 4
            },
            200
        )
        .is_err()
    );
    assert_eq!(a, before);
    a.receive(
        &RoomMessage::ClockPing {
            sequence: 2,
            sent_ns: 5,
        },
        123,
    )
    .unwrap();
    assert!(matches!(
        message(&a.next(123).unwrap().unwrap()),
        RoomMessage::ClockPong {
            sequence: 2,
            received_ns: 123,
            ..
        }
    ));
}

#[test]
fn long_running_and_extreme_separate_clocks_remain_exact_and_stop_withholds_estimates() {
    for base in [
        20 * 60 * 60 * 1_000_000_000i64,
        7 * 24 * 60 * 60 * 1_000_000_000i64,
    ] {
        let (mut a, mut b) = pair();
        for index in 0..8 {
            round(&mut a, &mut b, index + 1, base + index as i64 * 100, -400);
        }
        let estimate = a.estimate().unwrap();
        assert_eq!((estimate.lower_ns(), estimate.upper_ns()), (-406, -389));
        assert_eq!(estimate.observed_local_ns(), base + 723);
    }

    // Nonnegative clock domains may be separated by the entire signed range.
    let (mut zero, mut far) = pair();
    for sequence in 1..=8 {
        let zero_ping = zero.next(0).unwrap().unwrap();
        let far_ping = far.next(i64::MAX).unwrap().unwrap();
        zero.written(zero_ping.id, 0).unwrap();
        far.written(far_ping.id, i64::MAX).unwrap();
        zero.receive(&message(&far_ping), 0).unwrap();
        far.receive(&message(&zero_ping), i64::MAX).unwrap();
        let zero_pong = zero.next(0).unwrap().unwrap();
        let far_pong = far.next(i64::MAX).unwrap().unwrap();
        assert_eq!(
            message(&far_pong),
            RoomMessage::ClockPong {
                sequence,
                sent_ns: 0,
                received_ns: i64::MAX,
                replied_ns: i64::MAX,
            }
        );
        zero.written(zero_pong.id, 0).unwrap();
        far.written(far_pong.id, i64::MAX).unwrap();
        zero.receive(&message(&far_pong), 0).unwrap();
        far.receive(&message(&zero_pong), i64::MAX).unwrap();
    }
    let zero_estimate = zero.estimate().unwrap();
    let far_estimate = far.estimate().unwrap();
    assert_eq!(
        (zero_estimate.lower_ns(), zero_estimate.upper_ns()),
        (i128::from(i64::MAX), i128::from(i64::MAX))
    );
    assert_eq!(
        (far_estimate.lower_ns(), far_estimate.upper_ns()),
        (-i128::from(i64::MAX), -i128::from(i64::MAX))
    );
    assert_eq!(zero_estimate.round_trip_ns(), 0);
    assert_eq!(far_estimate.observed_local_ns(), i64::MAX);
    let before = far.clone();
    assert!(far.next(i64::MAX - 1).is_err());
    assert_eq!(far, before);

    let (mut partial, _) = pair();
    let partial_frame = partial.next(0).unwrap().unwrap();
    for (exchange, now, id) in [
        (&mut far, i64::MAX, 16),
        (&mut partial, 0, partial_frame.id),
    ] {
        let participant = exchange.participant();
        exchange.stop();
        assert_eq!(exchange.estimate(), None);
        let stopped = exchange.clone();
        exchange.stop();
        assert_eq!(*exchange, stopped);
        assert!(exchange.next(now).is_err());
        assert!(exchange.written(id, now).is_err());
        assert!(
            exchange
                .receive(
                    &RoomMessage::ClockPing {
                        sequence: 1,
                        sent_ns: 0
                    },
                    now
                )
                .is_err()
        );
        assert_eq!(*exchange, stopped);
        assert_eq!(exchange.participant(), participant);
    }
}

fn legacy_message(bytes: &[u8]) -> (u8, Vec<u8>) {
    let mut decoder = FrameDecoder::new();
    for byte in bytes {
        assert_eq!(decoder.push(std::slice::from_ref(byte)).unwrap(), 1);
    }
    decoder.take().unwrap().unwrap()
}

#[test]
fn shared_typed_probes_preserve_legacy_bkmp_bytes_zero_based_sequence_and_selected_estimate() {
    let (mut legacy_a, mut legacy_b) = (ClockProbes::default(), ClockProbes::default());
    let (mut typed_a, mut typed_b) = (ClockProbes::default(), ClockProbes::default());
    for sequence in 0..8u64 {
        let at = 1_000 + sequence as i64 * 100;
        let a_ping = legacy_a.next_ping(at).unwrap().unwrap();
        let b_ping = legacy_b.next_ping(at + 400).unwrap().unwrap();
        let a_fields = typed_a.next_ping_fields(at).unwrap().unwrap();
        let b_fields = typed_b.next_ping_fields(at + 400).unwrap().unwrap();
        assert_eq!(a_fields, (sequence, at));
        assert_eq!(b_fields, (sequence, at + 400));
        let (tag, a_payload) = legacy_message(&a_ping);
        assert_eq!(tag, 6);
        let (tag, b_payload) = legacy_message(&b_ping);
        assert_eq!(tag, 6);
        if sequence == 0 {
            let mut golden = vec![23, 0, 0, 0, b'B', b'K', b'M', b'P', 6, 0, 6];
            golden.extend_from_slice(&0u64.to_le_bytes());
            golden.extend_from_slice(&1_000i64.to_le_bytes());
            assert_eq!(a_ping, golden);
            assert!(decode_message(&a_ping).is_err());
        }
        let room_ping = RoomMessage::ClockPing {
            sequence: a_fields.0 + 1,
            sent_ns: a_fields.1,
        };
        assert_eq!(
            decode_message(&encode_message(&room_ping).unwrap()).unwrap(),
            room_ping
        );
        legacy_a.receive_ping(&b_payload, at + 7).unwrap();
        legacy_b.receive_ping(&a_payload, at + 411).unwrap();
        typed_a
            .receive_ping_fields(b_fields.0, b_fields.1, at + 7)
            .unwrap();
        typed_b
            .receive_ping_fields(a_fields.0, a_fields.1, at + 411)
            .unwrap();
        let a_pong = legacy_a.next_pong(at + 13).unwrap().unwrap();
        let b_pong = legacy_b.next_pong(at + 417).unwrap().unwrap();
        let a_fields = typed_a.next_pong_fields(at + 13).unwrap().unwrap();
        let b_fields = typed_b.next_pong_fields(at + 417).unwrap().unwrap();
        assert_eq!(a_fields, (sequence, at + 400, at + 7, at + 13));
        assert_eq!(b_fields, (sequence, at, at + 411, at + 417));
        let (a_tag, a_payload) = legacy_message(&a_pong);
        let (b_tag, b_payload) = legacy_message(&b_pong);
        assert_eq!((a_tag, b_tag), (7, 7));
        if sequence == 0 {
            let mut golden = vec![39, 0, 0, 0, b'B', b'K', b'M', b'P', 6, 0, 7];
            golden.extend_from_slice(&0u64.to_le_bytes());
            for time in [1_000i64, 1_411, 1_417] {
                golden.extend_from_slice(&time.to_le_bytes());
            }
            assert_eq!(b_pong, golden);
        }
        legacy_a.receive_pong(&b_payload, at + 23).unwrap();
        legacy_b.receive_pong(&a_payload, at + 429).unwrap();
        typed_a
            .receive_pong_fields(b_fields.0, b_fields.1, b_fields.2, b_fields.3, at + 23)
            .unwrap();
        typed_b
            .receive_pong_fields(a_fields.0, a_fields.1, a_fields.2, a_fields.3, at + 429)
            .unwrap();
        if sequence < 7 {
            assert_eq!(legacy_a.estimate_event(), None);
            assert_eq!(legacy_b.estimate_event(), None);
            assert_eq!(typed_a.estimate_event(), None);
            assert_eq!(typed_b.estimate_event(), None);
        }
    }
    let a_event = legacy_a.estimate_event().unwrap();
    let b_event = legacy_b.estimate_event().unwrap();
    assert_eq!(Some(a_event.clone()), typed_a.estimate_event());
    assert_eq!(Some(b_event.clone()), typed_b.estimate_event());
    let MultiplayerEvent::ClockEstimated(a_estimate) = a_event else {
        panic!("actual clock estimate required")
    };
    let MultiplayerEvent::ClockEstimated(b_estimate) = b_event else {
        panic!("actual clock estimate required")
    };
    assert_eq!(
        (
            a_estimate.lower_ns(),
            a_estimate.upper_ns(),
            a_estimate.observed_local_ns()
        ),
        (394, 411, 1_723)
    );
    assert_eq!(
        (
            b_estimate.lower_ns(),
            b_estimate.upper_ns(),
            b_estimate.observed_local_ns()
        ),
        (-416, -393, 2_129)
    );
    assert_eq!(legacy_a.estimate_event(), None);
    assert_eq!(legacy_b.estimate_event(), None);
    assert!(legacy_a.next_ping(1_800).unwrap().is_none());
    assert!(legacy_b.next_ping(2_200).unwrap().is_none());
    assert!(legacy_a.next_pong(1_800).unwrap().is_none());
    assert!(legacy_b.next_pong(2_200).unwrap().is_none());
}

#[test]
fn delayed_actor_processing_preserves_captured_ping_receipts_and_pong_sample_times() {
    let (mut a, mut b) = pair();
    for index in 0..8 {
        let at = 1_000 + index * 1_000;
        let a_ping = a.next(at).unwrap().unwrap();
        let b_ping = b.next(at + 400).unwrap().unwrap();
        assert!(a.next(at + 200).unwrap().is_none());
        assert!(b.next(at + 600).unwrap().is_none());
        a.written_at(a_ping.id, at + 2, at + 201).unwrap();
        b.written_at(b_ping.id, at + 403, at + 601).unwrap();
        a.receive_at(&message(&b_ping), at + 7, at + 202).unwrap();
        b.receive_at(&message(&a_ping), at + 411, at + 602).unwrap();
        let a_pong = a.next(at + 203).unwrap().unwrap();
        let b_pong = b.next(at + 603).unwrap().unwrap();
        assert_eq!(
            message(&a_pong),
            RoomMessage::ClockPong {
                sequence: index as u64 + 1,
                sent_ns: at + 400,
                received_ns: at + 7,
                replied_ns: at + 203,
            }
        );
        assert_eq!(
            message(&b_pong),
            RoomMessage::ClockPong {
                sequence: index as u64 + 1,
                sent_ns: at,
                received_ns: at + 411,
                replied_ns: at + 603,
            }
        );
        a.written_at(a_pong.id, at + 204, at + 300).unwrap();
        b.written_at(b_pong.id, at + 604, at + 700).unwrap();
        a.receive_at(&message(&b_pong), at + 220, at + 301).unwrap();
        b.receive_at(&message(&a_pong), at + 630, at + 701).unwrap();
        if index < 7 {
            assert_eq!(a.estimate(), None);
            assert_eq!(b.estimate(), None);
        }
    }
    let a_estimate = a.estimate().unwrap();
    let b_estimate = b.estimate().unwrap();
    assert_eq!(
        (
            a_estimate.lower_ns(),
            a_estimate.upper_ns(),
            a_estimate.round_trip_ns()
        ),
        (383, 411, 28)
    );
    assert_eq!(
        (
            b_estimate.lower_ns(),
            b_estimate.upper_ns(),
            b_estimate.round_trip_ns()
        ),
        (-427, -393, 34)
    );
    assert_eq!(a_estimate.observed_local_ns(), 8_220);
    assert_eq!(b_estimate.observed_local_ns(), 8_630);
}

#[test]
fn captured_read_and_write_lanes_validate_independently_without_retiming_or_partial_adoption() {
    let (mut exchange, _) = pair();
    let ping = exchange.next(100).unwrap().unwrap();
    assert!(exchange.next(200).unwrap().is_none());
    for (id, completed, processing) in [
        (ping.id, -1, 201),
        (ping.id, 99, 201),
        (ping.id, 202, 201),
        (ping.id + 1, 150, 201),
        (ping.id, 150, 199),
    ] {
        let before = exchange.clone();
        assert!(exchange.written_at(id, completed, processing).is_err());
        assert_eq!(exchange, before);
    }
    exchange.written_at(ping.id, 150, 201).unwrap();
    let pong = RoomMessage::ClockPong {
        sequence: 1,
        sent_ns: 100,
        received_ns: 105,
        replied_ns: 110,
    };
    // The read was captured before the delayed write notification. Neither
    // lane is retimestamped to the other's capture or the previous actor poll.
    exchange.receive_at(&pong, 120, 202).unwrap();
    let peer_ping = RoomMessage::ClockPing {
        sequence: 1,
        sent_ns: 5,
    };
    for (captured, processing) in [(-1, 203), (119, 203), (204, 203), (120, 201)] {
        let before = exchange.clone();
        assert!(
            exchange
                .receive_at(&peer_ping, captured, processing)
                .is_err()
        );
        assert_eq!(exchange, before);
    }
    let before = exchange.clone();
    assert!(exchange.written_at(ping.id, 150, 203).is_err());
    assert_eq!(exchange, before);
    exchange.receive_at(&peer_ping, 120, 203).unwrap();
    let reply = exchange.next(204).unwrap().unwrap();
    assert_eq!(
        message(&reply),
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 5,
            received_ns: 120,
            replied_ns: 204,
        }
    );
    let before = exchange.clone();
    assert!(exchange.written_at(reply.id, 203, 205).is_err());
    assert_eq!(exchange, before);
    exchange.written_at(reply.id, 204, 205).unwrap();
    assert_eq!(exchange.estimate(), None);

    let (mut old, mut explicit) = pair();
    let old_ping = old.next(100).unwrap().unwrap();
    let explicit_ping = explicit.next(100).unwrap().unwrap();
    old.written(old_ping.id, 101).unwrap();
    explicit.written_at(explicit_ping.id, 101, 101).unwrap();
    old.receive(&pong, 120).unwrap();
    explicit.receive_at(&pong, 120, 120).unwrap();
    assert_eq!(old, explicit);
    exchange.stop();
    let stopped = exchange.clone();
    assert!(exchange.receive_at(&peer_ping, 120, 206).is_err());
    assert!(exchange.written_at(reply.id, 204, 206).is_err());
    assert_eq!(exchange, stopped);
}
