//! Deferred actual group-admission helpers over bounded channels and duplex I/O.
//! These fixtures do not open TLS endpoints or establish multi-host gameplay.
use super::*;
use crate::{
    local_players::PlayerId,
    multiplayer_group_rooms::{GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_rooms::ParticipantId,
};
use std::{collections::BTreeMap, future::Future, io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, watch},
};

fn deferred_io(future: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(5), future)
                .await
                .expect("bounded in-memory group fixture exceeded cleanup deadline");
        });
}

type Senders = BTreeMap<ParticipantId, mpsc::Sender<Arc<Vec<u8>>>>;
type Receivers = BTreeMap<ParticipantId, mpsc::Receiver<Arc<Vec<u8>>>>;

fn registry(hosts: usize) -> GroupRoomRegistry {
    GroupRoomRegistry::new(GroupRoomPolicy::new(4, hosts, 32, 100).unwrap())
}

fn add_host(
    registry: &mut GroupRoomRegistry,
    senders: &mut Senders,
    receivers: &mut Receivers,
    key: &str,
    players: &[PlayerId],
    now: i64,
) -> crate::multiplayer_group_rooms::GroupParticipantTicket {
    let ticket = registry.join(key, b"canonical", players, now).unwrap();
    let (send, receive) = mpsc::channel(OUTGOING_CAPACITY);
    senders.insert(ticket.id, send);
    receivers.insert(ticket.id, receive);
    ticket
}

#[test]
fn actual_room_reader_preserves_fragmented_coalesced_messages_and_refuses_malformed_tails() {
    deferred_io(async {
        let join = RoomMessage::Join {
            identity: vec![0xA5; 65_536],
            players: (0..64).map(|index| PlayerId(u32::MAX - index)).collect(),
        };
        let mut stream = encode_message(&join).unwrap();
        stream.extend_from_slice(&encode_message(&RoomMessage::Seal).unwrap());
        let (mut peer, mut server) = tokio::io::duplex(17);
        let (_stop, mut stopped) = watch::channel(false);
        let send = async {
            for fragment in stream.chunks(37) {
                peer.write_all(fragment).await.unwrap();
            }
            peer.shutdown().await.unwrap();
        };
        let receive = async {
            assert_eq!(
                read_message(&mut server, Duration::from_secs(1), &mut stopped)
                    .await
                    .unwrap(),
                Some(join)
            );
            assert_eq!(
                read_message(&mut server, Duration::from_secs(1), &mut stopped)
                    .await
                    .unwrap(),
                Some(RoomMessage::Seal)
            );
            assert_eq!(
                read_message(&mut server, Duration::from_secs(1), &mut stopped)
                    .await
                    .unwrap(),
                None
            );
        };
        tokio::join!(send, receive);

        let mut invalid_header = encode_message(&RoomMessage::Ready).unwrap();
        invalid_header[3] = b'P';
        let mut invalid_body = encode_message(&RoomMessage::Admitted {
            participant: ParticipantId(1),
        })
        .unwrap();
        invalid_body[11..].fill(0);
        let mut oversized_header = encode_message(&RoomMessage::Ready).unwrap();
        oversized_header[6] = 1;
        oversized_header[7..11].copy_from_slice(&65_798u32.to_le_bytes());
        let truncated = encode_message(&RoomMessage::Admitted {
            participant: ParticipantId(1),
        })
        .unwrap()[..14]
            .to_vec();
        for tail in [invalid_header, invalid_body, oversized_header, truncated] {
            let valid = encode_message(&RoomMessage::Ready).unwrap();
            let (mut peer, mut server) = tokio::io::duplex(128);
            let (_stop, mut stopped) = watch::channel(false);
            peer.write_all(&valid).await.unwrap();
            peer.write_all(&tail).await.unwrap();
            peer.shutdown().await.unwrap();
            assert_eq!(
                read_message(&mut server, Duration::from_secs(1), &mut stopped)
                    .await
                    .unwrap(),
                Some(RoomMessage::Ready)
            );
            assert!(
                read_message(&mut server, Duration::from_secs(1), &mut stopped)
                    .await
                    .is_err()
            );
        }
    });
}

#[test]
fn idle_first_byte_waits_but_started_frame_and_explicit_stop_have_bounded_read_ownership() {
    deferred_io(async {
        let (mut peer, mut server) = tokio::io::duplex(16);
        let (_stop, mut stopped) = watch::channel(false);
        let read = read_message(&mut server, Duration::from_millis(20), &mut stopped);
        tokio::pin!(read);
        assert!(
            tokio::time::timeout(Duration::from_millis(40), &mut read)
                .await
                .is_err()
        );
        peer.write_all(b"B").await.unwrap();
        assert_eq!(read.await.unwrap_err().kind(), io::ErrorKind::TimedOut);

        for partial in [&b""[..], &b"BK"[..]] {
            let (mut peer, mut server) = tokio::io::duplex(16);
            peer.write_all(partial).await.unwrap();
            let (stop, mut stopped) = watch::channel(false);
            let read = read_message(&mut server, Duration::from_secs(2), &mut stopped);
            let cancel = async {
                tokio::task::yield_now().await;
                stop.send(true).unwrap();
            };
            let (result, ()) =
                tokio::time::timeout(Duration::from_secs(1), async { tokio::join!(read, cancel) })
                    .await
                    .unwrap();
            assert!(result.is_err());
        }
    });
}

#[test]
fn actual_writer_keeps_admission_before_snapshot_and_bounds_blocked_or_cancelled_output() {
    deferred_io(async {
        let admitted = encode_message(&RoomMessage::Admitted {
            participant: ParticipantId(u64::MAX),
        })
        .unwrap();
        let snapshot = encode_message(&RoomMessage::Snapshot {
            members: vec![crate::multiplayer_group_rooms::GroupRoomMember {
                id: ParticipantId(u64::MAX),
                players: vec![PlayerId(u32::MAX)],
                prepared: false,
            }],
            phase: GroupRoomPhase::Collecting,
            deadline_ns: Some(i64::MAX),
        })
        .unwrap();
        let expected = [admitted.as_slice(), snapshot.as_slice()].concat();
        let (send, receive) = mpsc::channel(OUTGOING_CAPACITY);
        send.try_send(Arc::new(admitted)).unwrap();
        send.try_send(Arc::new(snapshot)).unwrap();
        drop(send);
        let (mut peer, server) = tokio::io::duplex(7);
        let (_stop, stopped) = watch::channel(false);
        let write = write_frames(server, receive, Duration::from_secs(1), stopped);
        let read = async {
            let mut actual = Vec::new();
            peer.read_to_end(&mut actual).await.unwrap();
            actual
        };
        let (result, actual) = tokio::join!(write, read);
        result.unwrap();
        assert_eq!(actual, expected);

        let frame = Arc::new(encode_message(&RoomMessage::Ready).unwrap());
        let (send, receive) = mpsc::channel(OUTGOING_CAPACITY);
        send.try_send(frame.clone()).unwrap();
        let (mut peer, server) = tokio::io::duplex(1);
        let (_stop, stopped) = watch::channel(false);
        assert_eq!(
            write_frames(server, receive, Duration::from_millis(20), stopped)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        let mut partial = Vec::new();
        peer.read_to_end(&mut partial).await.unwrap();
        assert!(partial.len() < frame.len());
        assert!(send.is_closed());

        let (send, receive) = mpsc::channel(OUTGOING_CAPACITY);
        send.try_send(frame).unwrap();
        let (mut peer, server) = tokio::io::duplex(32);
        let (stop, stopped) = watch::channel(false);
        stop.send(true).unwrap();
        assert!(
            write_frames(server, receive, Duration::from_secs(1), stopped)
                .await
                .is_err()
        );
        let mut actual = Vec::new();
        peer.read_to_end(&mut actual).await.unwrap();
        assert!(actual.is_empty());
        assert!(send.is_closed());
    });
}

#[test]
fn actual_request_and_broadcast_helpers_publish_whole_two_three_four_sixty_four_host_rosters() {
    let players = (0..64)
        .map(|index| PlayerId(u32::MAX - index))
        .collect::<Vec<_>>();
    for count in [2, 3, 4, 64] {
        let mut registry = registry(count);
        let mut senders = Senders::new();
        let mut receivers = Receivers::new();
        let tickets = (0..count)
            .map(|_| {
                add_host(
                    &mut registry,
                    &mut senders,
                    &mut receivers,
                    "room",
                    &players,
                    0,
                )
            })
            .collect::<Vec<_>>();
        for phase in [
            GroupRoomPhase::Collecting,
            GroupRoomPhase::Frozen,
            GroupRoomPhase::Prepared,
        ] {
            match phase {
                GroupRoomPhase::Collecting => {}
                GroupRoomPhase::Frozen => assert!(
                    apply_request(&mut registry, tickets[0].id, &RoomMessage::Seal, 1)
                        .unwrap()
                        .is_empty()
                ),
                GroupRoomPhase::Prepared => {
                    for ticket in &tickets {
                        assert!(
                            apply_request(&mut registry, ticket.id, &RoomMessage::Ready, 2)
                                .unwrap()
                                .is_empty()
                        );
                    }
                }
            }
            publish_room(&registry, &senders, "room").unwrap();
            for ticket in &tickets {
                let actual = receivers.get_mut(&ticket.id).unwrap().try_recv().unwrap();
                assert_eq!(
                    decode_message(actual.as_slice()).unwrap(),
                    RoomMessage::Snapshot {
                        members: tickets
                            .iter()
                            .map(|ticket| crate::multiplayer_group_rooms::GroupRoomMember {
                                id: ticket.id,
                                players: players.clone(),
                                prepared: phase == GroupRoomPhase::Prepared,
                            })
                            .collect(),
                        phase,
                        deadline_ns: if phase == GroupRoomPhase::Prepared {
                            None
                        } else {
                            Some(100)
                        },
                    }
                );
                assert!(receivers.get_mut(&ticket.id).unwrap().try_recv().is_err());
            }
        }
        assert_eq!(
            apply_request(&mut registry, tickets[count - 1].id, &RoomMessage::Leave, 3).unwrap(),
            tickets
        );
        assert_eq!(
            (registry.room_count(), registry.participant_count()),
            (0, 0)
        );
    }
}

#[test]
fn stream_bound_requests_reject_forged_messages_and_release_only_the_exact_current_room() {
    let forbidden = [
        RoomMessage::Join {
            identity: b"canonical".to_vec(),
            players: vec![PlayerId(1)],
        },
        RoomMessage::Admitted {
            participant: ParticipantId(999),
        },
        RoomMessage::Snapshot {
            members: vec![crate::multiplayer_group_rooms::GroupRoomMember {
                id: ParticipantId(999),
                players: vec![PlayerId(1)],
                prepared: false,
            }],
            phase: GroupRoomPhase::Collecting,
            deadline_ns: Some(100),
        },
        RoomMessage::Seal,
        RoomMessage::Ready,
    ];
    for message in forbidden {
        let mut registry = registry(3);
        let a = registry
            .join("room", b"canonical", &[PlayerId(1)], 0)
            .unwrap();
        let b = registry
            .join("room", b"canonical", &[PlayerId(1)], 0)
            .unwrap();
        let other = registry
            .join("other", b"canonical", &[PlayerId(1)], 0)
            .unwrap();
        // b cannot claim the first stream's sealing authority. Ready is also
        // invalid before sealing; requests contain no alternate participant ID.
        assert!(apply_request(&mut registry, b.id, &message, 1).is_err());
        assert_eq!(registry.room("room").unwrap().members.len(), 2);
        assert_eq!(
            registry.release(b.id, 1).unwrap(),
            vec![a.clone(), b.clone()]
        );
        assert_eq!(registry.room("other").unwrap().members[0].id, other.id);
        let replacement = registry
            .join("room", b"replacement", &[PlayerId(7)], 1)
            .unwrap();
        assert!(apply_request(&mut registry, a.id, &RoomMessage::Seal, 1).is_err());
        assert!(registry.release(b.id, 1).unwrap().is_empty());
        assert_eq!(registry.room("room").unwrap().members[0].id, replacement.id);
    }
}

#[test]
fn full_closed_or_missing_recipient_refuses_broadcast_atomically_and_leaves_exact_release_tickets()
{
    assert_eq!(OUTGOING_CAPACITY, 4);
    for failure in 0..3 {
        let mut registry = registry(4);
        let mut senders = Senders::new();
        let mut receivers = Receivers::new();
        let tickets = (0..3)
            .map(|_| {
                add_host(
                    &mut registry,
                    &mut senders,
                    &mut receivers,
                    "room",
                    &[PlayerId(1)],
                    0,
                )
            })
            .collect::<Vec<_>>();
        let other = add_host(
            &mut registry,
            &mut senders,
            &mut receivers,
            "other",
            &[PlayerId(1)],
            0,
        );
        let blocked = tickets[2].id;
        let old_frame = Arc::new(
            encode_message(&RoomMessage::Admitted {
                participant: blocked,
            })
            .unwrap(),
        );
        match failure {
            0 => {
                for _ in 0..OUTGOING_CAPACITY {
                    senders[&blocked].try_send(old_frame.clone()).unwrap();
                }
            }
            1 => {
                receivers.remove(&blocked);
            }
            _ => {
                senders.remove(&blocked);
            }
        }
        assert!(publish_room(&registry, &senders, "room").is_err());
        for ticket in &tickets[..2] {
            assert!(receivers.get_mut(&ticket.id).unwrap().try_recv().is_err());
        }
        assert!(receivers.get_mut(&other.id).unwrap().try_recv().is_err());
        if failure == 0 {
            let receiver = receivers.get_mut(&blocked).unwrap();
            for _ in 0..OUTGOING_CAPACITY {
                assert_eq!(
                    receiver.try_recv().unwrap().as_slice(),
                    old_frame.as_slice()
                );
            }
            assert!(receiver.try_recv().is_err());
        }
        let returned = registry.release(blocked, 1).unwrap();
        assert_eq!(returned, tickets);
        assert_eq!(registry.room("other").unwrap().members[0].id, other.id);
        let replacement = registry
            .join("room", b"new", &[PlayerId(u32::MAX)], 1)
            .unwrap();
        assert!(registry.release(blocked, 1).unwrap().is_empty());
        assert_eq!(registry.room("room").unwrap().members[0].id, replacement.id);
    }
}
