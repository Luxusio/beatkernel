//! Deferred actual group-admission helpers over bounded channels and duplex I/O.
//! These fixtures do not open TLS endpoints or establish multi-host gameplay.
use super::*;
use crate::{
    local_players::PlayerId,
    multiplayer_group::{GroupPrefix, MemberProgress},
    multiplayer_group_rooms::{GroupRoomPhase, GroupRoomPolicy, GroupRoomRegistry},
    multiplayer_protocol::Progress,
    multiplayer_room_play::RoomPlayClient,
    multiplayer_room_wire::{RoomMessage, decode_message, encode_message},
    multiplayer_rooms::ParticipantId,
    multiplayer_start::{StartMessage, StartPolicy},
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

type Senders = BTreeMap<ParticipantId, mpsc::Sender<QueuedFrame>>;
type Receivers = BTreeMap<ParticipantId, mpsc::Receiver<QueuedFrame>>;

fn queued(bytes: Arc<Vec<u8>>, receipt: Option<u64>) -> QueuedFrame {
    QueuedFrame { bytes, receipt }
}

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

        let ping = RoomMessage::ClockPing {
            sequence: 1,
            sent_ns: 0,
        };
        let bytes = encode_message(&ping).unwrap();
        let (mut peer, mut server) = tokio::io::duplex(64);
        let (_stop, mut stopped) = watch::channel(false);
        let origin = Instant::now();
        peer.write_all(&bytes).await.unwrap();
        let before = now(origin).unwrap();
        let captured = read_control(&mut server, Duration::from_secs(1), &mut stopped, origin)
            .await
            .unwrap()
            .unwrap();
        let after = now(origin).unwrap();
        assert_eq!(captured.message, ping);
        assert!((before..=after).contains(&captured.captured_ns));
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
        let control = encode_message(&RoomMessage::ClockPing {
            sequence: 1,
            sent_ns: 0,
        })
        .unwrap();
        let expected = [admitted.as_slice(), snapshot.as_slice(), control.as_slice()].concat();
        let (send, receive) = mpsc::channel(OUTGOING_CAPACITY);
        assert!(send.try_send(queued(Arc::new(admitted), None)).is_ok());
        assert!(send.try_send(queued(Arc::new(snapshot), None)).is_ok());
        assert!(
            send.try_send(queued(Arc::new(control), Some(u64::MAX)))
                .is_ok()
        );
        drop(send);
        let (mut peer, server) = tokio::io::duplex(7);
        let (_stop, stopped) = watch::channel(false);
        let (commands, mut events) = mpsc::channel(COMMAND_CAPACITY);
        let origin = Instant::now();
        let before = now(origin).unwrap();
        let write = write_frames(
            server,
            receive,
            Duration::from_secs(1),
            stopped,
            ParticipantId(u64::MAX),
            origin,
            commands,
        );
        let read = async {
            let mut actual = Vec::new();
            peer.read_to_end(&mut actual).await.unwrap();
            actual
        };
        let (result, actual) = tokio::join!(write, read);
        result.unwrap();
        assert_eq!(actual, expected);
        let receipt = events.try_recv().unwrap();
        assert_eq!(receipt.id, ParticipantId(u64::MAX));
        let PeerEvent::Written {
            write_id,
            completed_ns,
        } = receipt.event
        else {
            panic!("actual complete-write receipt required")
        };
        assert_eq!(write_id, u64::MAX);
        assert!((before..=now(origin).unwrap()).contains(&completed_ns));
        assert!(events.try_recv().is_err());

        let frame = Arc::new(encode_message(&RoomMessage::Ready).unwrap());
        let (send, receive) = mpsc::channel(OUTGOING_CAPACITY);
        assert!(send.try_send(queued(frame.clone(), Some(1))).is_ok());
        let (mut peer, server) = tokio::io::duplex(1);
        let (_stop, stopped) = watch::channel(false);
        let (commands, mut events) = mpsc::channel(COMMAND_CAPACITY);
        assert_eq!(
            write_frames(
                server,
                receive,
                Duration::from_millis(20),
                stopped,
                ParticipantId(1),
                origin,
                commands
            )
            .await
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        let mut partial = Vec::new();
        peer.read_to_end(&mut partial).await.unwrap();
        assert!(partial.len() < frame.len());
        assert!(send.is_closed());
        assert!(events.try_recv().is_err());

        let (send, receive) = mpsc::channel(OUTGOING_CAPACITY);
        assert!(send.try_send(queued(frame, Some(2))).is_ok());
        let (mut peer, server) = tokio::io::duplex(32);
        let (stop, stopped) = watch::channel(false);
        stop.send(true).unwrap();
        let (commands, mut events) = mpsc::channel(COMMAND_CAPACITY);
        assert!(
            write_frames(
                server,
                receive,
                Duration::from_secs(1),
                stopped,
                ParticipantId(1),
                origin,
                commands
            )
            .await
            .is_err()
        );
        let mut actual = Vec::new();
        peer.read_to_end(&mut actual).await.unwrap();
        assert!(actual.is_empty());
        assert!(send.is_closed());
        assert!(events.try_recv().is_err());
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
                    decode_message(actual.bytes.as_slice()).unwrap(),
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
                assert_eq!(actual.receipt, None);
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
        RoomMessage::ClockPing {
            sequence: 1,
            sent_ns: 0,
        },
        RoomMessage::ClockPong {
            sequence: 1,
            sent_ns: 100,
            received_ns: 0,
            replied_ns: 0,
        },
        RoomMessage::Start(crate::multiplayer_start::StartMessage::ClockReady(0)),
        RoomMessage::Start(crate::multiplayer_start::StartMessage::Propose(100)),
        RoomMessage::Start(crate::multiplayer_start::StartMessage::Accept(100)),
        RoomMessage::Start(crate::multiplayer_start::StartMessage::Commit(100)),
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
                    assert!(
                        senders[&blocked]
                            .try_send(queued(old_frame.clone(), None))
                            .is_ok()
                    );
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
                    receiver.try_recv().unwrap().bytes.as_slice(),
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

struct ControlCohort {
    registry: GroupRoomRegistry,
    senders: Senders,
    receivers: Receivers,
    ids: Vec<ParticipantId>,
    clients: Vec<RoomPlayClient>,
}

fn client_receive(client: &mut RoomPlayClient, message: RoomMessage, now: i64) {
    let bytes = encode_message(&message).unwrap();
    client
        .receive(decode_message(&bytes).unwrap(), now)
        .unwrap();
}

impl ControlCohort {
    fn broadcast(&mut self, now: i64) {
        publish_room(&self.registry, &self.senders, "room").unwrap();
        for (index, id) in self.ids.iter().enumerate() {
            let frame = self.receivers.get_mut(id).unwrap().try_recv().unwrap();
            assert_eq!(frame.receipt, None);
            client_receive(
                &mut self.clients[index],
                decode_message(&frame.bytes).unwrap(),
                now,
            );
        }
    }

    fn prepared(count: usize) -> Self {
        let mut cohort = Self {
            registry: GroupRoomRegistry::new(
                GroupRoomPolicy::new(4, count, 32, 1_000_000).unwrap(),
            ),
            senders: Senders::new(),
            receivers: Receivers::new(),
            ids: Vec::new(),
            clients: Vec::new(),
        };
        for index in 0..count {
            let mut client = RoomPlayClient::new(
                b"canonical",
                &[PlayerId(index as u32 + 1)],
                StartPolicy::default(),
                (index as i64 + 1) * 1_000_000,
            )
            .unwrap();
            let join = client.poll_write(0).unwrap().unwrap();
            let RoomMessage::Join { identity, players } = decode_message(&join.bytes).unwrap()
            else {
                panic!("real client Join required")
            };
            assert_eq!(identity, b"canonical");
            let ticket = add_host(
                &mut cohort.registry,
                &mut cohort.senders,
                &mut cohort.receivers,
                "room",
                &players,
                0,
            );
            client.written(join.id, 0).unwrap();
            client_receive(
                &mut client,
                RoomMessage::Admitted {
                    participant: ticket.id,
                },
                0,
            );
            cohort.ids.push(ticket.id);
            cohort.clients.push(client);
            cohort.broadcast(0);
        }
        cohort.clients[0].request_seal().unwrap();
        let seal = cohort.clients[0].poll_write(1).unwrap().unwrap();
        cohort.clients[0].written(seal.id, 1).unwrap();
        assert!(
            apply_request(
                &mut cohort.registry,
                cohort.ids[0],
                &decode_message(&seal.bytes).unwrap(),
                1
            )
            .unwrap()
            .is_empty()
        );
        cohort.broadcast(1);
        for index in 0..count {
            cohort.clients[index].request_ready().unwrap();
            let ready = cohort.clients[index].poll_write(2).unwrap().unwrap();
            cohort.clients[index].written(ready.id, 2).unwrap();
            assert!(
                apply_request(
                    &mut cohort.registry,
                    cohort.ids[index],
                    &decode_message(&ready.bytes).unwrap(),
                    2
                )
                .unwrap()
                .is_empty()
            );
            cohort.broadcast(2);
        }
        cohort
    }
}

#[test]
fn actual_prepared_room_pump_composes_two_three_four_clients_and_waits_for_real_control_writes() {
    for count in [2usize, 3, 4] {
        let mut cohort = ControlCohort::prepared(count);
        let mut room =
            PreparedRoom::new(cohort.registry.room("room").unwrap(), 3, 10_000_000_000).unwrap();
        // The actual snapshot broadcast precedes the first control in every queue.
        publish_room(&cohort.registry, &cohort.senders, "room").unwrap();
        let mut last_writes = vec![0u64; count];
        let mut schedules = vec![None; count];
        let mut target = None;
        let mut early_accepts = 0usize;
        let mut commit_writes = 0usize;
        // Each bounded turn follows actual messages or write completions from
        // the prior turn; no idle timer supplies protocol progress.
        for turn in 0..64 {
            let now = 10_000 + turn * 1_000;
            room.pump(&cohort.senders, now).unwrap();
            let mut receipts = Vec::new();
            let mut proposals = vec![false; count];
            let mut progressed = false;
            for index in 0..count {
                let id = cohort.ids[index];
                let shift = if index % 2 == 0 { 1_000 } else { -1_000 };
                let receiver = cohort.receivers.get_mut(&id).unwrap();
                let mut seen = 0;
                while let Ok(frame) = receiver.try_recv() {
                    seen += 1;
                    assert!(seen <= OUTGOING_CAPACITY);
                    progressed = true;
                    let message = decode_message(&frame.bytes).unwrap();
                    if turn == 0 && seen == 1 {
                        assert!(matches!(
                            message,
                            RoomMessage::Snapshot {
                                phase: GroupRoomPhase::Prepared,
                                ..
                            }
                        ));
                        assert_eq!(frame.receipt, None);
                    } else {
                        let write_id = frame.receipt.expect("actual control receipt identity");
                        assert_eq!(write_id, last_writes[index] + 1);
                        last_writes[index] = write_id;
                        if let RoomMessage::Start(StartMessage::Propose(song_target)) = message {
                            proposals[index] = true;
                            let expected = now + 2_000_000_000 + count as i64 * 1_000_000 + 40;
                            if target.is_none() {
                                assert_eq!(song_target, expected);
                                target = Some(song_target);
                            }
                            assert_eq!(Some(song_target), target);
                        }
                        let is_commit =
                            matches!(message, RoomMessage::Start(StartMessage::Commit(_)));
                        receipts.push((id, write_id, is_commit));
                    }
                    cohort.clients[index]
                        .receive_at(message, now + 10 + shift, now + 100 + shift)
                        .unwrap();
                }
            }
            for index in 0..count {
                let shift = if index % 2 == 0 { 1_000 } else { -1_000 };
                if let Some(frame) = cohort.clients[index].poll_write(now + 200 + shift).unwrap() {
                    progressed = true;
                    let message = decode_message(&frame.bytes).unwrap();
                    cohort.clients[index]
                        .written_at(frame.id, now + 201 + shift, now + 202 + shift)
                        .unwrap();
                    if matches!(message, RoomMessage::Start(StartMessage::Accept(_))) {
                        assert!(proposals[index]);
                        early_accepts += 1;
                    }
                    // Responses precede the queued local completion notification
                    // for this turn's actual server frame, including Propose.
                    room.receive(cohort.ids[index], &message, now + 230, now + 300)
                        .unwrap();
                }
                if let Some(schedule) = cohort.clients[index].take_schedule() {
                    assert!(schedules[index].is_none());
                    schedules[index] = Some(schedule);
                }
            }
            if receipts.iter().any(|(_, _, commit)| *commit) {
                assert!(!room.committed());
            }
            for (id, write_id, is_commit) in receipts {
                room.written(id, write_id, now + 1, now + 400).unwrap();
                if is_commit {
                    commit_writes += 1;
                    assert_eq!(room.committed(), commit_writes == count);
                }
            }
            if room.committed() {
                break;
            }
            assert!(progressed, "an incomplete handshake must have actual work");
        }
        assert!(room.committed());
        assert!(!room.expired(i64::MAX));
        assert_eq!(early_accepts, count);
        assert_eq!(commit_writes, count);
        assert_eq!(last_writes, vec![19; count]);
        for (index, schedule) in schedules.into_iter().enumerate() {
            let shift = if index % 2 == 0 { 1_000 } else { -1_000 };
            let schedule = schedule.expect("real matching Commit required");
            assert_eq!(schedule.song_target_ns, target.unwrap() + shift - 10);
            assert_eq!(
                schedule.target_ns,
                schedule.song_target_ns - (index as i64 + 1) * 1_000_000
            );
            assert_eq!(schedule.uncertainty_ns, 40);
            assert_eq!(cohort.clients[index].take_schedule(), None);
        }
    }
}

#[test]
fn prepared_deadline_receipt_lease_and_output_failures_preserve_exact_room_release_ownership() {
    let mut collecting = registry(2);
    let first = collecting
        .join("room", b"canonical", &[PlayerId(1)], 0)
        .unwrap();
    collecting
        .join("room", b"canonical", &[PlayerId(2)], 0)
        .unwrap();
    assert!(PreparedRoom::new(collecting.room("room").unwrap(), 0, 100).is_err());
    collecting.seal(first.id, 1).unwrap();
    assert!(PreparedRoom::new(collecting.room("room").unwrap(), 1, 100).is_err());

    let mut cohort = ControlCohort::prepared(2);
    let other = add_host(
        &mut cohort.registry,
        &mut cohort.senders,
        &mut cohort.receivers,
        "other",
        &[PlayerId(u32::MAX)],
        2,
    );
    let snapshot = cohort.registry.room("room").unwrap();
    assert_eq!(snapshot.deadline_ns, None);
    assert!(PreparedRoom::new(snapshot, -1, 100).is_err());
    assert!(PreparedRoom::new(snapshot, 3, 0).is_err());
    assert!(PreparedRoom::new(snapshot, i64::MAX, 1).is_err());
    let mut room = PreparedRoom::new(snapshot, 3, 1_000).unwrap();
    room.pump(&cohort.senders, 10).unwrap();
    let frame = cohort
        .receivers
        .get_mut(&cohort.ids[0])
        .unwrap()
        .try_recv()
        .unwrap();
    let write_id = frame.receipt.unwrap();
    assert!(room.written(other.id, write_id, 11, 20).is_err());
    assert!(room.written(cohort.ids[0], write_id + 1, 11, 20).is_err());
    assert!(room.written(cohort.ids[0], write_id, 9, 20).is_err());
    room.written(cohort.ids[0], write_id, 11, 20).unwrap();
    assert!(room.written(cohort.ids[0], write_id, 11, 21).is_err());
    assert!(
        room.receive(
            cohort.ids[0],
            &RoomMessage::Admitted {
                participant: other.id
            },
            15,
            30
        )
        .is_err()
    );
    let ping = RoomMessage::ClockPing {
        sequence: 1,
        sent_ns: 0,
    };
    assert!(room.receive(cohort.ids[0], &ping, 2, 30).is_err());
    room.receive(cohort.ids[0], &ping, 15, 30).unwrap();
    assert!(!room.expired(1_002));
    assert!(room.expired(1_003));
    assert!(room.pump(&cohort.senders, 1_003).is_err());
    assert!(!room.committed());
    let released = cohort.registry.release(cohort.ids[0], 1_003).unwrap();
    assert_eq!(
        released.iter().map(|ticket| ticket.id).collect::<Vec<_>>(),
        cohort.ids
    );
    assert_eq!(
        cohort.registry.room("other").unwrap().members[0].id,
        other.id
    );
    for ticket in released {
        cohort.senders.remove(&ticket.id);
    }
    let a = cohort
        .registry
        .join("room", b"replacement", &[PlayerId(7)], 1_003)
        .unwrap();
    let b = cohort
        .registry
        .join("room", b"replacement", &[PlayerId(8)], 1_003)
        .unwrap();
    cohort.registry.seal(a.id, 1_004).unwrap();
    cohort.registry.ready(a.id, 1_005).unwrap();
    cohort.registry.ready(b.id, 1_005).unwrap();
    let mut replacement =
        PreparedRoom::new(cohort.registry.room("room").unwrap(), 1_006, 1_000).unwrap();
    assert!(
        replacement
            .written(cohort.ids[0], write_id, 1_010, 1_010)
            .is_err()
    );
    assert!(
        replacement
            .receive(cohort.ids[0], &ping, 1_010, 1_010)
            .is_err()
    );
    assert!(
        cohort
            .registry
            .release(cohort.ids[0], 1_010)
            .unwrap()
            .is_empty()
    );
    assert_eq!(cohort.registry.room("room").unwrap().members[0].id, a.id);

    for failure in 0..3 {
        let mut cohort = ControlCohort::prepared(2);
        let other = add_host(
            &mut cohort.registry,
            &mut cohort.senders,
            &mut cohort.receivers,
            "other",
            &[PlayerId(9)],
            2,
        );
        let mut room = PreparedRoom::new(cohort.registry.room("room").unwrap(), 3, 1_000).unwrap();
        let blocked = cohort.ids[1];
        match failure {
            0 => {
                for _ in 0..OUTGOING_CAPACITY {
                    let bytes = Arc::new(encode_message(&RoomMessage::Ready).unwrap());
                    assert!(
                        cohort.senders[&blocked]
                            .try_send(queued(bytes, None))
                            .is_ok()
                    );
                }
            }
            1 => {
                cohort.receivers.remove(&blocked);
            }
            _ => {
                cohort.senders.remove(&blocked);
            }
        }
        assert!(room.pump(&cohort.senders, 10).is_err());
        assert!(!room.committed());
        let released = cohort.registry.release(blocked, 10).unwrap();
        assert_eq!(
            released.iter().map(|ticket| ticket.id).collect::<Vec<_>>(),
            cohort.ids
        );
        assert_eq!(
            cohort.registry.room("other").unwrap().members[0].id,
            other.id
        );
        assert!(
            cohort
                .receivers
                .get_mut(&other.id)
                .unwrap()
                .try_recv()
                .is_err()
        );
    }
}

// Exchange actual client/actor messages through the same bounded channels used
// above, stopping only after each real client has received its matching Commit.
// The returned server write IDs remain uncredited.
fn pending_commit_room(
    cohort: &mut ControlCohort,
) -> (PreparedRoom, Vec<(ParticipantId, u64)>, i64) {
    let mut room =
        PreparedRoom::new(cohort.registry.room("room").unwrap(), 3, 10_000_000_000).unwrap();
    for turn in 0..64 {
        let at = 10_000 + turn * 1_000;
        room.pump(&cohort.senders, at).unwrap();
        let mut writes = Vec::new();
        let mut commits = Vec::new();
        for index in 0..cohort.ids.len() {
            let id = cohort.ids[index];
            let shift = if index % 2 == 0 { 1_000 } else { -1_000 };
            if let Ok(frame) = cohort.receivers.get_mut(&id).unwrap().try_recv() {
                let message = decode_message(&frame.bytes).unwrap();
                let write_id = frame.receipt.unwrap();
                if matches!(message, RoomMessage::Start(StartMessage::Commit(_))) {
                    commits.push((id, write_id));
                } else {
                    writes.push((id, write_id));
                }
                cohort.clients[index]
                    .receive_at(message, at + shift + 10, at + shift + 100)
                    .unwrap();
                assert!(cohort.receivers.get_mut(&id).unwrap().try_recv().is_err());
            }
        }
        for index in 0..cohort.ids.len() {
            let shift = if index % 2 == 0 { 1_000 } else { -1_000 };
            if let Some(frame) = cohort.clients[index].poll_write(at + shift + 200).unwrap() {
                cohort.clients[index]
                    .written_at(frame.id, at + shift + 201, at + shift + 202)
                    .unwrap();
                room.receive(
                    cohort.ids[index],
                    &decode_message(&frame.bytes).unwrap(),
                    at + 230,
                    at + 300,
                )
                .unwrap();
            }
        }
        for (id, write_id) in writes {
            room.written(id, write_id, at + 1, at + 400).unwrap();
        }
        if !commits.is_empty() {
            assert_eq!(commits.len(), cohort.ids.len());
            for client in &mut cohort.clients {
                assert!(
                    client.take_schedule().is_some(),
                    "actual matching Commit produced the schedule"
                );
                assert!(client.take_schedule().is_none());
            }
            assert!(!room.committed());
            return (room, commits, at);
        }
    }
    panic!("bounded real clock/start exchange did not reach Commit");
}

fn room_upload(player: PlayerId, final_prefix: bool) -> RoomMessage {
    RoomMessage::Progress(GroupPrefix {
        sequence: 1,
        final_prefix,
        members: vec![MemberProgress {
            player,
            progress: Progress {
                song_ns: 604_800_000_000_000,
                hits: 1,
                misses: 0,
                combo: 1,
                max_combo: 1,
            },
        }],
    })
}

// Retain actual aggregate frames after a genuine shared start and all peer
// final-prefix/ACK exchanges. Their outer write receipts are still outstanding.
fn pending_drain_room(
    cohort: &mut ControlCohort,
) -> (PreparedRoom, Vec<(ParticipantId, QueuedFrame)>, i64) {
    let (mut room, commits, at) = pending_commit_room(cohort);
    for (id, write_id) in commits {
        room.written(id, write_id, at + 1, at + 500).unwrap();
    }
    assert!(room.committed());
    for (index, &id) in cohort.ids.iter().enumerate() {
        room.receive(
            id,
            &room_upload(PlayerId(index as u32 + 1), true),
            at + 510,
            at + 520,
        )
        .unwrap();
    }
    let mut acks = Vec::new();
    for turn in 1..cohort.ids.len() {
        let time = at + 600 + turn as i64 * 100;
        room.pump(&cohort.senders, time).unwrap();
        for &id in &cohort.ids {
            let frame = cohort.receivers.get_mut(&id).unwrap().try_recv().unwrap();
            let RoomMessage::PeerProgress {
                participant,
                prefix,
            } = decode_message(&frame.bytes).unwrap()
            else {
                panic!("actual final peer frame required before aggregate admission")
            };
            assert_ne!(participant, id);
            assert!(prefix.final_prefix);
            acks.push((
                id,
                RoomMessage::FinalAck {
                    participant,
                    sequence: prefix.sequence,
                },
            ));
            room.written(id, frame.receipt.unwrap(), time + 1, time + 10)
                .unwrap();
        }
    }
    let admitted = at + 1000 + cohort.ids.len() as i64 * 100;
    for (id, ack) in acks {
        room.receive(id, &ack, admitted - 20, admitted - 10)
            .unwrap();
    }
    room.pump(&cohort.senders, admitted).unwrap();
    let frames = cohort
        .ids
        .iter()
        .map(|&id| {
            let frame = cohort.receivers.get_mut(&id).unwrap().try_recv().unwrap();
            assert_eq!(
                decode_message(&frame.bytes).unwrap(),
                RoomMessage::FinalAck {
                    participant: id,
                    sequence: 1
                }
            );
            (id, frame)
        })
        .collect();
    assert!(!room.complete());
    (room, frames, admitted)
}

#[test]
fn actual_server_stages_after_own_commit_and_relays_only_after_all_commit_and_ack_receipts() {
    for count in [2usize, 3, 4] {
        let mut cohort = ControlCohort::prepared(count);
        let mut before_start =
            PreparedRoom::new(cohort.registry.room("room").unwrap(), 3, 10_000_000_000).unwrap();
        assert!(
            before_start
                .receive(cohort.ids[0], &room_upload(PlayerId(1), false), 4, 4)
                .is_err()
        );
        let (mut room, commits, at) = pending_commit_room(&mut cohort);
        for index in 0..count {
            let upload = room_upload(PlayerId(index as u32 + 1), true);
            assert!(
                room.receive(cohort.ids[index], &upload, at - 1, at + 500)
                    .is_err(),
                "capture must follow this lease's actual Commit admission"
            );
            room.receive(cohort.ids[index], &upload, at + 20, at + 500)
                .unwrap();
        }
        room.pump(&cohort.senders, at + 600).unwrap();
        for id in &cohort.ids {
            assert!(cohort.receivers.get_mut(id).unwrap().try_recv().is_err());
        }
        for &(id, write_id) in &commits[..count - 1] {
            room.written(id, write_id, at + 1, at + 700).unwrap();
        }
        room.pump(&cohort.senders, at + 701).unwrap();
        assert!(!room.committed());
        for id in &cohort.ids {
            assert!(cohort.receivers.get_mut(id).unwrap().try_recv().is_err());
        }
        let (last, last_write) = commits[count - 1];
        room.written(last, last_write, at + 1, at + 800).unwrap();
        assert!(room.committed());
        assert!(!room.complete());

        let mut delivered = Vec::new();
        let mut aggregate_writes = Vec::new();
        let mut shared_prefixes: BTreeMap<ParticipantId, Arc<Vec<u8>>> = BTreeMap::new();
        let mut outer_ids = commits.iter().map(|(_, id)| *id).collect::<Vec<_>>();
        for turn in 0..count * 3 {
            let now = at + 900 + turn as i64 * 100;
            room.pump(&cohort.senders, now).unwrap();
            let mut writes = Vec::new();
            for index in 0..count {
                let recipient = cohort.ids[index];
                if let Ok(frame) = cohort.receivers.get_mut(&recipient).unwrap().try_recv() {
                    let write_id = frame.receipt.unwrap();
                    assert_eq!(
                        write_id,
                        outer_ids[index] + 1,
                        "control and relay share one opaque write sequence"
                    );
                    outer_ids[index] = write_id;
                    match decode_message(&frame.bytes).unwrap() {
                        RoomMessage::PeerProgress {
                            participant,
                            prefix,
                        } => {
                            assert_ne!(participant, recipient);
                            assert!(!delivered.contains(&(recipient, participant)));
                            delivered.push((recipient, participant));
                            let source_index =
                                cohort.ids.iter().position(|&id| id == participant).unwrap();
                            let RoomMessage::Progress(expected) =
                                room_upload(PlayerId(source_index as u32 + 1), true)
                            else {
                                unreachable!()
                            };
                            assert_eq!(prefix, expected);
                            if let Some(shared) = shared_prefixes.get(&participant) {
                                assert!(Arc::ptr_eq(shared, &frame.bytes));
                            } else {
                                shared_prefixes.insert(participant, frame.bytes.clone());
                            }
                            // Real application ACK may arrive before the actor receives
                            // its write-completion command, but cannot replace it.
                            let ack = RoomMessage::FinalAck {
                                participant,
                                sequence: prefix.sequence,
                            };
                            assert!(
                                room.receive(recipient, &ack, now - 1, now + 20).is_err(),
                                "an earlier captured ACK cannot acknowledge a later admitted final relay"
                            );
                            room.receive(recipient, &ack, now + 10, now + 20).unwrap();
                            writes.push((recipient, write_id));
                        }
                        RoomMessage::FinalAck {
                            participant,
                            sequence,
                        } => {
                            assert_eq!((participant, sequence), (recipient, 1));
                            assert!(!aggregate_writes.iter().any(|&(id, _)| id == recipient));
                            aggregate_writes.push((recipient, write_id));
                            // Release earlier aggregate slots so other sources can
                            // still reach this recipient; retain the last barrier.
                            if aggregate_writes.len() < count {
                                writes.push((recipient, write_id));
                            }
                        }
                        message => panic!("unexpected post-Commit frame: {message:?}"),
                    }
                    assert!(
                        cohort
                            .receivers
                            .get_mut(&recipient)
                            .unwrap()
                            .try_recv()
                            .is_err()
                    );
                }
            }
            assert!(!room.complete());
            for (id, write_id) in writes {
                room.written(id, write_id, now + 1, now + 30).unwrap();
            }
            if aggregate_writes.len() == count {
                break;
            }
        }
        assert_eq!(delivered.len(), count * (count - 1));
        assert_eq!(aggregate_writes.len(), count);
        assert!(!room.complete());
        let receipt_now = at + 10_000;
        let (id, write_id) = *aggregate_writes.last().unwrap();
        room.written(id, write_id, receipt_now, receipt_now)
            .unwrap();
        assert!(room.complete());
        assert_eq!(
            cohort.registry.participant_count(),
            count,
            "completion does not manufacture Leave or close streams"
        );
    }
}

#[test]
fn relay_queue_failure_and_stale_receipts_release_exact_room_after_real_committed_start() {
    for failure in 0..3 {
        let mut cohort = ControlCohort::prepared(2);
        let other = add_host(
            &mut cohort.registry,
            &mut cohort.senders,
            &mut cohort.receivers,
            "other",
            &[PlayerId(9)],
            2,
        );
        let (mut room, commits, at) = pending_commit_room(&mut cohort);
        for &(id, write_id) in &commits {
            room.written(id, write_id, at + 1, at + 500).unwrap();
        }
        assert!(room.committed());
        assert!(
            room.receive(
                other.id,
                &room_upload(PlayerId(9), false),
                at + 510,
                at + 520
            )
            .is_err()
        );
        let source = cohort.ids[0];
        let blocked = cohort.ids[1];
        room.receive(source, &room_upload(PlayerId(1), false), at + 510, at + 520)
            .unwrap();
        match failure {
            0 => {
                for _ in 0..OUTGOING_CAPACITY {
                    cohort.senders[&blocked]
                        .try_send(queued(
                            Arc::new(encode_message(&RoomMessage::Ready).unwrap()),
                            None,
                        ))
                        .unwrap();
                }
            }
            1 => {
                cohort.receivers.remove(&blocked);
            }
            _ => {
                cohort.senders.remove(&blocked);
            }
        }
        assert!(room.pump(&cohort.senders, at + 530).is_err());
        assert!(!room.complete());
        assert!(
            room.written(blocked, commits[1].1 + 1, at + 531, at + 540)
                .is_err(),
            "queue failure cannot create a completed relay write"
        );
        let released = cohort.registry.release(blocked, at + 550).unwrap();
        assert_eq!(
            released.iter().map(|ticket| ticket.id).collect::<Vec<_>>(),
            cohort.ids
        );
        for ticket in released {
            cohort.senders.remove(&ticket.id);
        }
        drop(room);
        assert_eq!(
            cohort.registry.room("other").unwrap().members[0].id,
            other.id
        );
        assert!(
            cohort
                .receivers
                .get_mut(&other.id)
                .unwrap()
                .try_recv()
                .is_err()
        );
        let a = cohort
            .registry
            .join("room", b"replacement", &[PlayerId(7)], at + 550)
            .unwrap();
        let b = cohort
            .registry
            .join("room", b"replacement", &[PlayerId(8)], at + 550)
            .unwrap();
        cohort.registry.seal(a.id, at + 551).unwrap();
        cohort.registry.ready(a.id, at + 552).unwrap();
        cohort.registry.ready(b.id, at + 552).unwrap();
        let mut replacement =
            PreparedRoom::new(cohort.registry.room("room").unwrap(), at + 553, 1_000).unwrap();
        assert!(
            replacement
                .written(blocked, commits[1].1 + 1, at + 554, at + 554)
                .is_err()
        );
        assert!(
            replacement
                .receive(source, &room_upload(PlayerId(1), true), at + 554, at + 554)
                .is_err()
        );
        assert!(
            cohort
                .registry
                .release(blocked, at + 555)
                .unwrap()
                .is_empty()
        );
        assert_eq!(cohort.registry.room("room").unwrap().members[0].id, a.id);
    }
}

#[test]
fn actual_actor_drain_uses_original_ready_capture_and_exact_outer_aggregate_and_complete_writes() {
    for count in [2usize, 3, 4, 64] {
        let mut cohort = ControlCohort::prepared(count);
        let (mut room, aggregates, admitted) = pending_drain_room(&mut cohort);
        let first = cohort.ids[0];
        let ready = RoomMessage::DrainReady {
            participant: first,
            sequence: 1,
        };
        assert!(
            room.receive(first, &ready, admitted - 1, admitted + 10)
                .is_err()
        );
        assert!(
            room.receive(cohort.ids[1], &ready, admitted, admitted + 10)
                .is_err()
        );
        assert!(
            room.receive(first, &ready, admitted, i64::MAX).is_err(),
            "overflow cannot install a drain deadline or processing frontier"
        );
        assert!(
            !room.expired(i64::MAX),
            "rejected readiness installs no committed drain deadline"
        );
        room.receive(first, &ready, admitted, admitted + 10)
            .unwrap();
        assert!(
            room.receive(first, &ready, admitted + 1, admitted + 11)
                .is_err()
        );
        assert!(
            room.written(first, count as u64, admitted + 1, admitted + 12)
                .is_err(),
            "a child relay ID cannot substitute for the actual outer write ID"
        );
        for &(id, ref aggregate) in &aggregates[1..] {
            room.written(id, aggregate.receipt.unwrap(), admitted + 1, admitted + 20)
                .unwrap();
        }
        for &id in &cohort.ids[1..count - 1] {
            room.receive(
                id,
                &RoomMessage::DrainReady {
                    participant: id,
                    sequence: 1,
                },
                admitted + 2,
                admitted + 21,
            )
            .unwrap();
        }
        room.pump(&cohort.senders, admitted + 22).unwrap();
        for id in &cohort.ids {
            assert!(cohort.receivers.get_mut(id).unwrap().try_recv().is_err());
        }
        assert!(!room.complete());
        room.written(
            first,
            aggregates[0].1.receipt.unwrap(),
            admitted + 1,
            admitted + 23,
        )
        .unwrap();
        assert!(room.complete());
        assert!(!room.drain_settled());
        room.pump(&cohort.senders, admitted + 24).unwrap();
        for id in &cohort.ids {
            assert!(
                cohort.receivers.get_mut(id).unwrap().try_recv().is_err(),
                "one host is still not Ready"
            );
        }
        let last = cohort.ids[count - 1];
        room.receive(
            last,
            &RoomMessage::DrainReady {
                participant: last,
                sequence: 1,
            },
            admitted + 3,
            admitted + 25,
        )
        .unwrap();
        room.pump(&cohort.senders, admitted + 30).unwrap();
        for (index, &(id, ref aggregate)) in aggregates.iter().enumerate() {
            let frame = cohort.receivers.get_mut(&id).unwrap().try_recv().unwrap();
            let write_id = frame.receipt.unwrap();
            assert_eq!(write_id, aggregate.receipt.unwrap() + 1);
            assert_eq!(
                decode_message(&frame.bytes).unwrap(),
                RoomMessage::DrainComplete {
                    participant: id,
                    sequence: 1
                }
            );
            assert!(room.relay.drain_complete_admitted(id).unwrap());
            assert!(!room.relay.drain_complete_written(id).unwrap());
            assert!(!room.drain_settled());
            assert!(
                room.written(id, aggregate.receipt.unwrap(), admitted + 31, admitted + 40)
                    .is_err()
            );
            room.written(id, write_id, admitted + 31, admitted + 40)
                .unwrap();
            assert_eq!(room.relay.drained(), index + 1 == count);
        }
        assert!(room.relay.drained());
        let mut rooms = BTreeMap::from([("room".to_owned(), room)]);
        assert!(
            release_settled(&mut cohort.registry, &rooms, "room", admitted + 50)
                .unwrap()
                .is_empty(),
            "local full writes cannot prove that remote clients received their Complete notices"
        );
        assert_eq!(cohort.registry.room("room").unwrap().members.len(), count);
        assert_eq!(cohort.senders.len(), count);
        let mut tickets = Vec::new();
        for (index, &id) in cohort.ids.iter().enumerate() {
            let retired = terminal_room(
                &mut cohort.registry,
                &mut rooms,
                &mut cohort.senders,
                "room",
                id,
                admitted + 60 + index as i64,
            )
            .unwrap();
            if index + 1 < count {
                assert!(retired.is_empty());
            } else {
                tickets = retired;
            }
        }
        assert_eq!(
            tickets.iter().map(|ticket| ticket.id).collect::<Vec<_>>(),
            cohort.ids
        );
        let mut resources = BTreeMap::<ParticipantId, PeerResource>::new();
        release(&mut resources, &mut cohort.senders, &mut rooms, tickets);
        assert!(cohort.registry.room("room").is_none());
        assert!(rooms.is_empty());
        assert!(cohort.senders.is_empty());
        assert!(
            release_settled(&mut cohort.registry, &rooms, "room", admitted + 200)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn expected_peer_retirement_keeps_other_complete_writes_and_never_releases_a_replacement_room() {
    for all_retired in [false, true] {
        let mut cohort = ControlCohort::prepared(4);
        let other = add_host(
            &mut cohort.registry,
            &mut cohort.senders,
            &mut cohort.receivers,
            "other",
            &[PlayerId(u32::MAX)],
            2,
        );
        let (mut room, aggregates, admitted) = pending_drain_room(&mut cohort);
        for &(id, ref frame) in &aggregates {
            room.written(id, frame.receipt.unwrap(), admitted + 1, admitted + 10)
                .unwrap();
            room.receive(
                id,
                &RoomMessage::DrainReady {
                    participant: id,
                    sequence: 1,
                },
                admitted + 2,
                admitted + 10,
            )
            .unwrap();
        }
        room.pump(&cohort.senders, admitted + 20).unwrap();
        let notices = cohort
            .ids
            .iter()
            .map(|&id| {
                let frame = cohort.receivers.get_mut(&id).unwrap().try_recv().unwrap();
                assert_eq!(
                    decode_message(&frame.bytes).unwrap(),
                    RoomMessage::DrainComplete {
                        participant: id,
                        sequence: 1
                    }
                );
                (id, frame.receipt.unwrap())
            })
            .collect::<Vec<_>>();
        let first = notices[0].0;
        let mut rooms = BTreeMap::from([("room".to_owned(), room)]);
        let mut resources = BTreeMap::<ParticipantId, PeerResource>::new();
        let early = terminal_room(
            &mut cohort.registry,
            &mut rooms,
            &mut cohort.senders,
            "room",
            first,
            admitted + 21,
        )
        .unwrap();
        assert!(early.is_empty());
        assert!(!cohort.senders.contains_key(&first));
        assert_eq!(cohort.registry.room("room").unwrap().members.len(), 4);
        assert!(
            cohort.ids[1..]
                .iter()
                .all(|id| cohort.senders.contains_key(id))
        );
        assert!(!rooms["room"].relay.drain_complete_written(first).unwrap());
        assert!(!rooms["room"].relay.drained());
        assert!(!rooms["room"].drain_settled());
        assert!(
            terminal_room(
                &mut cohort.registry,
                &mut rooms,
                &mut cohort.senders,
                "room",
                first,
                admitted + 22
            )
            .unwrap()
            .is_empty()
        );
        // The joined task and its delayed callback may both arrive. Neither is
        // evidence that a retired Complete was fully written.
        rooms
            .get_mut("room")
            .unwrap()
            .written(first, notices[0].1, admitted + 21, admitted + 23)
            .unwrap();
        rooms
            .get_mut("room")
            .unwrap()
            .receive(
                first,
                &RoomMessage::DrainReady {
                    participant: first,
                    sequence: 1,
                },
                admitted + 2,
                admitted + 23,
            )
            .unwrap();
        rooms
            .get_mut("room")
            .unwrap()
            .pump(&cohort.senders, admitted + 24)
            .unwrap();
        assert!(!rooms["room"].relay.drain_complete_written(first).unwrap());
        let mut released = Vec::new();
        for (index, &(id, write_id)) in notices.iter().enumerate().skip(1) {
            let now = admitted + 30 + index as i64;
            let tickets = if all_retired {
                terminal_room(
                    &mut cohort.registry,
                    &mut rooms,
                    &mut cohort.senders,
                    "room",
                    id,
                    now,
                )
                .unwrap()
            } else {
                rooms
                    .get_mut("room")
                    .unwrap()
                    .written(id, write_id, admitted + 21, now)
                    .unwrap();
                release_settled(&mut cohort.registry, &rooms, "room", now).unwrap()
            };
            if !all_retired || index < notices.len() - 1 {
                assert!(tickets.is_empty());
            } else {
                released = tickets;
            }
        }
        if !all_retired {
            assert!(released.is_empty());
            assert_eq!(cohort.registry.room("room").unwrap().members.len(), 4);
            assert!(
                cohort.ids[1..]
                    .iter()
                    .all(|id| cohort.senders.contains_key(id)),
                "remaining full-written peers stay connected until their actual terminal events"
            );
            for (index, &(id, _)) in notices.iter().enumerate().skip(1) {
                let tickets = terminal_room(
                    &mut cohort.registry,
                    &mut rooms,
                    &mut cohort.senders,
                    "room",
                    id,
                    admitted + 40 + index as i64,
                )
                .unwrap();
                if index + 1 < notices.len() {
                    assert!(tickets.is_empty());
                } else {
                    released = tickets;
                }
            }
        }
        assert_eq!(
            released.iter().map(|ticket| ticket.id).collect::<Vec<_>>(),
            cohort.ids
        );
        assert!(
            !rooms["room"].relay.drained(),
            "retirement settles resources without fabricating all Complete writes"
        );
        release(&mut resources, &mut cohort.senders, &mut rooms, released);
        assert!(rooms.is_empty());
        assert!(cohort.registry.room("room").is_none());
        assert_eq!(
            cohort.registry.room("other").unwrap().members[0].id,
            other.id
        );
        assert!(cohort.senders.contains_key(&other.id));
        let replacement = add_host(
            &mut cohort.registry,
            &mut cohort.senders,
            &mut cohort.receivers,
            "room",
            &[PlayerId(90)],
            admitted + 50,
        );
        add_host(
            &mut cohort.registry,
            &mut cohort.senders,
            &mut cohort.receivers,
            "room",
            &[PlayerId(91)],
            admitted + 50,
        );
        cohort.registry.seal(replacement.id, admitted + 51).unwrap();
        let replacement_ids = cohort
            .registry
            .room("room")
            .unwrap()
            .members
            .iter()
            .map(|member| member.id)
            .collect::<Vec<_>>();
        for &id in &replacement_ids {
            cohort.registry.ready(id, admitted + 52).unwrap();
        }
        rooms.insert(
            "room".to_owned(),
            PreparedRoom::new(cohort.registry.room("room").unwrap(), admitted + 53, 1000).unwrap(),
        );
        for &(old, write_id) in &notices {
            assert!(
                terminal_room(
                    &mut cohort.registry,
                    &mut rooms,
                    &mut cohort.senders,
                    "room",
                    old,
                    admitted + 54
                )
                .unwrap()
                .is_empty()
            );
            assert!(
                rooms
                    .get_mut("room")
                    .unwrap()
                    .written(old, write_id, admitted + 21, admitted + 54)
                    .is_err()
            );
        }
        assert_eq!(
            cohort
                .registry
                .room("room")
                .unwrap()
                .members
                .iter()
                .map(|member| member.id)
                .collect::<Vec<_>>(),
            replacement_ids
        );
        assert!(
            replacement_ids
                .iter()
                .all(|id| cohort.senders.contains_key(id))
        );
    }
}

#[test]
fn pre_drain_disconnect_explicit_leave_and_fixed_committed_deadline_keep_whole_room_cancellation() {
    for mode in [
        "before-ready",
        "before-complete",
        "leave",
        "full-complete",
        "closed-complete",
        "deadline",
    ] {
        let mut cohort = ControlCohort::prepared(3);
        let other = add_host(
            &mut cohort.registry,
            &mut cohort.senders,
            &mut cohort.receivers,
            "other",
            &[PlayerId(8)],
            2,
        );
        let (mut room, aggregates, admitted) = pending_drain_room(&mut cohort);
        let first = cohort.ids[0];
        let mut closing = first;
        if mode != "before-ready" {
            // First accepted readiness is held behind the aggregate callback.
            room.receive(
                first,
                &RoomMessage::DrainReady {
                    participant: first,
                    sequence: 1,
                },
                admitted,
                admitted + 10,
            )
            .unwrap();
            assert!(!room.expired(admitted + 10 + 10_000_000_000 - 1));
            assert!(room.expired(admitted + 10 + 10_000_000_000));
        }
        if matches!(mode, "leave" | "full-complete" | "closed-complete") {
            for &(id, ref aggregate) in &aggregates {
                room.written(id, aggregate.receipt.unwrap(), admitted + 1, admitted + 20)
                    .unwrap();
                if id != first {
                    room.receive(
                        id,
                        &RoomMessage::DrainReady {
                            participant: id,
                            sequence: 1,
                        },
                        admitted + 1,
                        admitted + 20,
                    )
                    .unwrap();
                }
            }
            if mode == "leave" {
                room.pump(&cohort.senders, admitted + 30).unwrap();
                assert!(room.relay.drain_complete_admitted(first).unwrap());
            } else {
                closing = *cohort.ids.last().unwrap();
                if mode == "full-complete" {
                    for _ in 0..OUTGOING_CAPACITY {
                        cohort.senders[&closing]
                            .try_send(queued(
                                Arc::new(encode_message(&RoomMessage::Ready).unwrap()),
                                None,
                            ))
                            .unwrap();
                    }
                } else {
                    cohort.receivers.remove(&closing);
                }
                assert!(room.pump(&cohort.senders, admitted + 30).is_err());
                assert!(
                    !room.retire(closing, admitted + 31).unwrap(),
                    "a failed Complete queue admission never permits expected retirement"
                );
            }
            assert!(!room.expired(admitted + 10 + 10_000_000_000 - 1));
            assert!(
                room.expired(admitted + 10 + 10_000_000_000),
                "later Ready messages do not renew the first drain deadline"
            );
        }
        let mut rooms = BTreeMap::from([("room".to_owned(), room)]);
        let time = if mode == "deadline" {
            admitted + 10 + 10_000_000_000
        } else {
            admitted + 40
        };
        let tickets = match mode {
            "leave" => {
                apply_request(&mut cohort.registry, first, &RoomMessage::Leave, time).unwrap()
            }
            "deadline" => {
                assert!(
                    rooms
                        .get_mut("room")
                        .unwrap()
                        .pump(&cohort.senders, time)
                        .is_err()
                );
                cohort.registry.release(first, time).unwrap()
            }
            _ => terminal_room(
                &mut cohort.registry,
                &mut rooms,
                &mut cohort.senders,
                "room",
                closing,
                time,
            )
            .unwrap(),
        };
        assert_eq!(
            tickets.iter().map(|ticket| ticket.id).collect::<Vec<_>>(),
            cohort.ids
        );
        let mut resources = BTreeMap::<ParticipantId, PeerResource>::new();
        release(&mut resources, &mut cohort.senders, &mut rooms, tickets);
        assert!(rooms.is_empty());
        assert!(cohort.registry.room("room").is_none());
        assert_eq!(
            cohort.registry.room("other").unwrap().members[0].id,
            other.id
        );
        assert!(cohort.senders.contains_key(&other.id));
    }
}
